use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use russh::{Disconnect, client};
use tokio::{
    sync::RwLock as TokioRwLock,
    time::{sleep, timeout},
};

use crate::{
    app::stats::ProxyStats,
    config::{AppConfig, AppPaths},
};

use super::{socks::Request, ssh};

const CHANNEL_OPEN_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
const SSH_PING_TIMEOUT: Duration = Duration::from_secs(3);

pub type SharedSession = Arc<TokioRwLock<SessionState>>;

pub struct OpenedChannel {
    pub channel: russh::Channel<client::Msg>,
    pub generation: u64,
}

/// Shared SSH session reused across all SOCKS connections.
pub struct SessionState {
    handle: client::Handle<ssh::Client>,
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
    ready_status: String,
    generation: u64,
    /// Set when direct-tcpip fails so later requests reconnect before retrying.
    dead: bool,
}

impl SessionState {
    pub fn new(
        handle: client::Handle<ssh::Client>,
        config: Arc<Mutex<AppConfig>>,
        paths: AppPaths,
        ready_status: String,
    ) -> Self {
        Self {
            handle,
            config,
            paths,
            ready_status,
            generation: 0,
            dead: false,
        }
    }

    pub fn is_dead(&self) -> bool {
        self.dead
    }

    pub async fn disconnect(&self) {
        self.handle
            .disconnect(Disconnect::ByApplication, "", "English")
            .await
            .ok();
    }
}

/// Open a direct-tcpip channel, reconnecting the SSH session if it has died.
///
/// Channel opens share read access to the current SSH handle. Reconnects take
/// write access so only one request replaces a dead handle.
pub async fn open_channel_with_retry(
    session: &SharedSession,
    request: &Request,
    peer_addr: &SocketAddr,
    stats: &ProxyStats,
) -> Result<OpenedChannel> {
    let failed_generation = {
        let state = session.read().await;

        if !state.dead && state.handle.is_closed() {
            state.generation
        } else if !state.dead {
            let generation = state.generation;
            match open_channel_with_stall_check(&state.handle, request, peer_addr).await {
                Ok(channel) => {
                    return Ok(OpenedChannel {
                        channel,
                        generation,
                    });
                }
                Err(ChannelAttemptError::Target(error)) => {
                    tracing::warn!(
                        event = "target_channel_failed",
                        peer = %peer_addr,
                        target_host = %request.host,
                        target_port = request.port,
                        error = %error,
                        "SSH server failed to open target channel"
                    );
                    return Err(error).context("SSH server failed to open target channel");
                }
                Err(ChannelAttemptError::Session(error)) => {
                    tracing::warn!(
                        event = "ssh_session_failed",
                        peer = %peer_addr,
                        target_host = %request.host,
                        target_port = request.port,
                        error = %error,
                        "SSH session failed"
                    );
                    generation
                }
            }
        } else {
            u64::MAX
        }
    };

    open_after_reconnect(session, request, peer_addr, stats, failed_generation).await
}

async fn open_after_reconnect(
    session: &SharedSession,
    request: &Request,
    peer_addr: &SocketAddr,
    stats: &ProxyStats,
    failed_generation: u64,
) -> Result<OpenedChannel> {
    let mut state = session.write().await;
    if state.generation == failed_generation && !state.dead {
        mark_session_dead(&mut state, stats);
    }
    if state.dead {
        reconnect_session(&mut state, stats).await?;
    }
    let channel = open_direct_tcpip(&state.handle, request, peer_addr)
        .await
        .map_err(|error| {
            tracing::warn!(
                event = "target_channel_failed_after_reconnect",
                peer = %peer_addr,
                target_host = %request.host,
                target_port = request.port,
                error = %error,
                "Failed to open SSH channel after reconnect"
            );
            error
        })
        .context("failed to open SSH channel after reconnect")?;
    Ok(OpenedChannel {
        channel,
        generation: state.generation,
    })
}

pub async fn mark_dead_if_generation(session: &SharedSession, stats: &ProxyStats, generation: u64) {
    let mut state = session.write().await;
    if state.generation == generation {
        mark_session_dead(&mut state, stats);
    }
}

enum ChannelAttemptError {
    Target(russh::Error),
    Session(anyhow::Error),
}

async fn open_channel_with_stall_check(
    handle: &client::Handle<ssh::Client>,
    request: &Request,
    peer_addr: &SocketAddr,
) -> std::result::Result<russh::Channel<client::Msg>, ChannelAttemptError> {
    let open = open_direct_tcpip(handle, request, peer_addr);
    tokio::pin!(open);

    tokio::select! {
        result = &mut open => classify_channel_open(result),
        () = sleep(CHANNEL_OPEN_RESPONSE_TIMEOUT) => {
            tracing::warn!(
                event = "ssh_channel_stalled",
                "SSH channel open has not responded after {CHANNEL_OPEN_RESPONSE_TIMEOUT:?}; checking session liveness"
            );
            let ping = timeout(SSH_PING_TIMEOUT, handle.send_ping());
            tokio::pin!(ping);
            tokio::select! {
                result = &mut open => classify_channel_open(result),
                result = &mut ping => match result {
                    Ok(Ok(())) => {
                        tracing::info!(
                            event = "ssh_ping_answered",
                            "SSH session answered ping; continuing to wait for channel open"
                        );
                        classify_channel_open(open.as_mut().await)
                    }
                    Ok(Err(error)) => Err(ChannelAttemptError::Session(
                        anyhow::Error::new(error).context("SSH ping failed after channel-open stall"),
                    )),
                    Err(_) => Err(ChannelAttemptError::Session(anyhow!(
                        "SSH ping timed out after {SSH_PING_TIMEOUT:?}"
                    ))),
                },
            }
        }
    }
}

fn classify_channel_open(
    result: std::result::Result<russh::Channel<client::Msg>, russh::Error>,
) -> std::result::Result<russh::Channel<client::Msg>, ChannelAttemptError> {
    match result {
        Ok(channel) => Ok(channel),
        Err(error @ russh::Error::ChannelOpenFailure(_)) => Err(ChannelAttemptError::Target(error)),
        Err(error) => Err(ChannelAttemptError::Session(anyhow::Error::new(error))),
    }
}

fn mark_session_dead(state: &mut SessionState, stats: &ProxyStats) {
    if !state.dead {
        state.dead = true;
        stats.ssh_disconnected();
        stats.set_status("SSH disconnected; reconnecting...");
    }
}

async fn reconnect_session(state: &mut SessionState, stats: &ProxyStats) -> Result<()> {
    tracing::info!(event = "ssh_reconnecting", "Reconnecting SSH session");
    stats.set_status("Reconnecting SSH session...");
    let new_handle = ssh::connect(Arc::clone(&state.config), state.paths.clone())
        .await
        .map_err(|error| {
            tracing::error!(
                event = "ssh_reconnect_failed",
                error = %error,
                "Failed to reconnect SSH session"
            );
            error
        })
        .context("failed to reconnect SSH session")?;
    state.handle = new_handle;
    state.dead = false;
    state.generation = state.generation.wrapping_add(1);
    stats.ssh_connected();
    stats.clear_error();
    stats.set_status(state.ready_status.clone());
    tracing::info!(event = "ssh_reconnected", "SSH session reconnected");
    Ok(())
}

async fn open_direct_tcpip(
    handle: &client::Handle<ssh::Client>,
    request: &Request,
    peer_addr: &SocketAddr,
) -> std::result::Result<russh::Channel<client::Msg>, russh::Error> {
    handle
        .channel_open_direct_tcpip(
            request.host.clone(),
            request.port.into(),
            peer_addr.ip().to_string(),
            peer_addr.port().into(),
        )
        .await
}
