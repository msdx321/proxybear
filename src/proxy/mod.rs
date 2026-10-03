mod pool;
mod socks;
mod ssh;
mod tunnel;

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinSet,
    time::sleep,
};

use crate::{
    app::stats::ProxyStats,
    config::{AppConfig, AppPaths},
};

use pool::{POOL_SIZE, Pool};

const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

pub async fn run_proxy(
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
    stats: Arc<ProxyStats>,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<()> {
    // Snapshot settings once so every session in the pool uses the same ones.
    let runtime = config
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .runtime_config()?;
    let local_addr = runtime.listen.local_addr;

    let listener = TcpListener::bind(local_addr)
        .await
        .map_err(|error| {
            tracing::error!(
                event = "proxy_bind_failed",
                local_addr = %local_addr,
                error = %error,
                "Failed to bind local proxy listener"
            );
            error
        })
        .with_context(|| format!("failed to bind {local_addr}"))?;

    tracing::info!(event = "proxy_starting", local_addr = %local_addr, "Proxy starting");
    stats.set_status("Connecting to SSH server...");
    let pool = Arc::new(Pool::new(
        Arc::clone(&stats),
        format!("Listening on {local_addr}"),
    ));
    let connector = ssh::Connector::new(runtime.ssh, Arc::clone(&config), paths);
    let mut supervisors = JoinSet::new();
    for index in 0..POOL_SIZE {
        supervisors.spawn(Arc::clone(&pool).supervise(index, connector.clone()));
    }

    let mut clients = JoinSet::new();
    let result = loop {
        tokio::select! {
            _ = &mut shutdown => break Ok(()),
            Some(result) = supervisors.join_next() => {
                // Supervisors only return on errors that retrying cannot fix.
                break result.context("SSH session supervisor failed").and_then(|result| result);
            }
            accepted = listener.accept() => match accepted {
                // Per-connection failures are logged where they happen; they
                // must not mark the whole proxy unhealthy.
                Ok((stream, peer_addr)) => {
                    clients.spawn(handle_client(stream, peer_addr, Arc::clone(&pool), Arc::clone(&stats)));
                }
                Err(error) => {
                    tracing::warn!(
                        event = "proxy_accept_failed",
                        error = %error,
                        "Failed to accept local connection"
                    );
                    sleep(ACCEPT_RETRY_DELAY).await;
                }
            },
            _ = clients.join_next(), if !clients.is_empty() => {}
        }
    };

    clients.abort_all();
    supervisors.abort_all();
    while clients.join_next().await.is_some() {}
    while supervisors.join_next().await.is_some() {}
    pool.close().await;
    stats.set_status("Stopped");
    match &result {
        Ok(()) => tracing::info!(
            event = "proxy_stopped",
            reason = "shutdown",
            "Proxy stopped"
        ),
        Err(error) => tracing::error!(
            event = "proxy_stopped",
            reason = "error",
            error = %error,
            "Proxy stopped"
        ),
    }
    result
}

async fn handle_client(
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    pool: Arc<Pool>,
    stats: Arc<ProxyStats>,
) -> Result<()> {
    stream
        .set_nodelay(true)
        .map_err(|error| {
            tracing::debug!(
                event = "socks_request_failed",
                peer = %peer_addr,
                phase = "tcp_setup",
                error = %error,
                "Failed to configure local SOCKS connection"
            );
            error
        })
        .context("failed to set TCP_NODELAY")?;
    socks::negotiate_no_auth(&mut stream)
        .await
        .map_err(|error| {
            tracing::debug!(
                event = "socks_request_failed",
                peer = %peer_addr,
                phase = "negotiation",
                error = %error,
                "SOCKS negotiation failed"
            );
            error
        })?;
    let request = socks::read_request(&mut stream).await.map_err(|error| {
        tracing::debug!(
            event = "socks_request_failed",
            peer = %peer_addr,
            phase = "request",
            error = %error,
            "SOCKS request failed"
        );
        error
    })?;

    let opened = match pool.open_channel(&request, &peer_addr).await {
        Ok(opened) => opened,
        Err(error) => {
            let _ = socks::write_reply(&mut stream, socks::REPLY_GENERAL_FAILURE).await;
            return Err(error);
        }
    };

    socks::write_reply(&mut stream, socks::REPLY_SUCCEEDED).await?;
    if let Err(error) = tunnel::pump(stream, opened.channel, &stats).await {
        if error.ssh_session_failed() {
            tracing::warn!(
                event = "ssh_tunnel_failed",
                peer = %peer_addr,
                target_host = %request.host,
                target_port = request.port,
                error = %error,
                "SSH tunnel failed"
            );
            opened.lease.kill_session();
            return Ok(());
        } else {
            tracing::debug!(
                event = "tunnel_failed",
                peer = %peer_addr,
                target_host = %request.host,
                target_port = request.port,
                error = %error,
                "Local tunnel failed"
            );
        }
        return Err(error.into());
    }
    Ok(())
}
