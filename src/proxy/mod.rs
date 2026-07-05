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
        .with_context(|| format!("failed to bind {local_addr}"))?;

    stats.set_status("Connecting to SSH server...");
    let handle = ssh::connect(Arc::clone(&config), paths.clone()).await?;
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

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                let state = session.read().await;
                if !state.is_dead() {
                    stats.ssh_disconnected();
                }
                state.disconnect().await;
                stats.set_status("Stopped");
                return Ok(());
            }
            accepted = listener.accept() => {
                let (stream, peer_addr) = accepted.context("failed to accept local connection")?;
                let session = Arc::clone(&session);
                let stats = Arc::clone(&stats);
                tokio::spawn(async move {
                    if let Err(error) = handle_client(stream, peer_addr, session, Arc::clone(&stats)).await {
                        stats.set_error(error.to_string());
                    } else {
                        stats.clear_error();
                    }
                });
            }
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
        .context("failed to set TCP_NODELAY")?;
    socks::negotiate_no_auth(&mut stream).await?;
    let request = socks::read_request(&mut stream).await?;

    let mut channel =
        match session::open_channel_with_retry(&session, &request, &peer_addr, &stats).await {
            Ok(channel) => channel,
            Err(error) => {
                let _ = socks::write_reply(&mut stream, socks::REPLY_GENERAL_FAILURE).await;
                return Err(error);
            }
        };

    socks::write_reply(&mut stream, socks::REPLY_SUCCEEDED).await?;
    tunnel::pump(stream, &mut channel, stats).await
}
