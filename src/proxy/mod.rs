mod session;
mod socks;
mod ssh;
mod tunnel;

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{RwLock as TokioRwLock, oneshot},
    task::JoinSet,
};

use crate::{
    app::stats::ProxyStats,
    config::{AppConfig, AppPaths},
};

use session::{SessionState, SharedSession};

pub async fn run_proxy(
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
    stats: Arc<ProxyStats>,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<()> {
    let local_addr = {
        let config = config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        config.runtime_config()?.listen.local_addr
    };

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
    let handle = tokio::select! {
        result = ssh::connect(Arc::clone(&config), paths.clone()) => result.map_err(|error| {
            tracing::error!(
                event = "ssh_connect_failed",
                error = %error,
                "Failed to connect SSH session"
            );
            error
        })?,
        _ = &mut shutdown => {
            stats.set_status("Stopped");
            tracing::info!(event = "proxy_stopped", reason = "shutdown", "Proxy stopped");
            return Ok(());
        }
    };
    let listening_status = format!("Listening on {local_addr}");
    let session = Arc::new(TokioRwLock::new(SessionState::new(
        handle,
        Arc::clone(&config),
        paths,
        listening_status.clone(),
    )));
    stats.clear_error();
    stats.set_status(listening_status);
    stats.ssh_connected();
    tracing::info!(event = "proxy_started", local_addr = %local_addr, "Proxy started");

    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                clients.abort_all();
                while clients.join_next().await.is_some() {}
                let state = session.read().await;
                if !state.is_dead() {
                    stats.ssh_disconnected();
                }
                if let Err(error) = state.disconnect().await {
                    tracing::warn!(
                        event = "ssh_disconnect_failed",
                        error = %error,
                        "Failed to close SSH session during shutdown"
                    );
                }
                stats.set_status("Stopped");
                tracing::info!(event = "proxy_stopped", reason = "shutdown", "Proxy stopped");
                return Ok(());
            }
            accepted = listener.accept() => {
                let (stream, peer_addr) = accepted.context("failed to accept local connection")?;
                let session = Arc::clone(&session);
                let stats = Arc::clone(&stats);
                clients.spawn(async move {
                    if let Err(error) = handle_client(stream, peer_addr, session, Arc::clone(&stats)).await {
                        stats.set_error(error.to_string());
                    }
                });
            }
            _ = clients.join_next(), if !clients.is_empty() => {}
        }
    }
}

async fn handle_client(
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    session: SharedSession,
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

    let opened =
        match session::open_channel_with_retry(&session, &request, &peer_addr, &stats).await {
            Ok(opened) => opened,
            Err(error) => {
                let _ = socks::write_reply(&mut stream, socks::REPLY_GENERAL_FAILURE).await;
                return Err(error);
            }
        };

    socks::write_reply(&mut stream, socks::REPLY_SUCCEEDED).await?;
    let mut channel = opened.channel;
    if let Err(error) = tunnel::pump(stream, &mut channel, Arc::clone(&stats)).await {
        if error.ssh_session_failed() {
            tracing::warn!(
                event = "ssh_tunnel_failed",
                peer = %peer_addr,
                target_host = %request.host,
                target_port = request.port,
                error = %error,
                "SSH tunnel failed"
            );
            session::mark_dead_if_generation(&session, &stats, opened.generation).await;
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
