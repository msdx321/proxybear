use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use futures::future::join_all;
use russh::{Disconnect, client};
use tokio::{
    sync::{Notify, watch},
    time::{sleep, timeout},
};

use crate::app::stats::ProxyStats;

use super::{socks::Request, ssh};

/// Number of SSH sessions kept open. New channels go to the least-loaded one.
pub const POOL_SIZE: usize = 3;

const SESSION_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const CHANNEL_OPEN_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
const CHANNEL_OPEN_ATTEMPTS: usize = 2;
const SSH_DISCONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const SSH_PING_TIMEOUT: Duration = Duration::from_secs(3);
const RECONNECT_BACKOFF_MIN: Duration = Duration::from_secs(1);
const RECONNECT_BACKOFF_MAX: Duration = Duration::from_secs(30);
/// Sessions that lived at least this long reconnect immediately when closed.
const STABLE_SESSION_UPTIME: Duration = Duration::from_secs(30);

/// One authenticated SSH connection in the pool.
pub struct Session {
    handle: client::Handle<ssh::Client>,
    active_channels: AtomicUsize,
    dead: AtomicBool,
    kill: Notify,
}

impl Session {
    fn is_live(&self) -> bool {
        !self.dead.load(Ordering::Relaxed) && !self.handle.is_closed()
    }

    /// Stop handing out this session and ask its supervisor to replace it.
    fn kill(&self) {
        if !self.dead.swap(true, Ordering::Relaxed) {
            self.kill.notify_one();
        }
    }

    async fn disconnect(&self) {
        let result = timeout(
            SSH_DISCONNECT_TIMEOUT,
            self.handle
                .disconnect(Disconnect::ByApplication, "", "English"),
        )
        .await;
        if let Ok(Err(error)) = result {
            tracing::debug!(
                event = "ssh_disconnect_failed",
                error = %error,
                "Failed to send SSH disconnect"
            );
        }
    }
}

/// Counts an open channel against its session's load until dropped.
pub struct Lease(Arc<Session>);

impl Lease {
    fn new(session: Arc<Session>) -> Self {
        session.active_channels.fetch_add(1, Ordering::Relaxed);
        Self(session)
    }

    /// Mark the leased session as failed so new channels avoid it.
    pub fn kill_session(&self) {
        self.0.kill();
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.active_channels.fetch_sub(1, Ordering::Relaxed);
    }
}

pub struct OpenedChannel {
    pub channel: russh::Channel<client::Msg>,
    pub lease: Lease,
}

/// SSH sessions shared by all SOCKS connections.
pub struct Pool {
    slots: watch::Sender<Vec<Option<Arc<Session>>>>,
    stats: Arc<ProxyStats>,
    listening_status: String,
    /// Cuts reconnect backoff short when a client is waiting for a session.
    wake: Notify,
}

impl Pool {
    pub fn new(stats: Arc<ProxyStats>, listening_status: String) -> Self {
        Self {
            slots: watch::Sender::new(vec![None; POOL_SIZE]),
            stats,
            listening_status,
            wake: Notify::new(),
        }
    }

    /// Keep slot `index` connected until the proxy stops.
    pub async fn supervise(self: Arc<Self>, index: usize, connector: ssh::Connector) {
        let mut backoff = RECONNECT_BACKOFF_MIN;
        loop {
            let retry_in = match connector.connect().await {
                Ok((handle, closed)) => {
                    backoff = RECONNECT_BACKOFF_MIN;
                    let started = Instant::now();
                    let session = Arc::new(Session {
                        handle,
                        active_channels: AtomicUsize::new(0),
                        dead: AtomicBool::new(false),
                        kill: Notify::new(),
                    });
                    self.set_slot(index, Some(Arc::clone(&session)));
                    tracing::info!(
                        event = "ssh_session_ready",
                        slot = index,
                        "SSH session ready"
                    );

                    let reason = tokio::select! {
                        _ = closed => "closed",
                        () = session.kill.notified() => "failed",
                    };
                    session.dead.store(true, Ordering::Relaxed);
                    self.set_slot(index, None);
                    tracing::warn!(
                        event = "ssh_session_lost",
                        slot = index,
                        reason,
                        uptime = ?started.elapsed(),
                        "SSH session lost; reconnecting"
                    );
                    session.disconnect().await;

                    if started.elapsed() >= STABLE_SESSION_UPTIME {
                        continue;
                    }
                    Some(backoff)
                }
                // Rejected credentials or a changed host key are not retried
                // on a timer, which could hammer the server, but they can be
                // transient on a flaky network. Retry when a client needs a
                // session instead of stopping the proxy.
                Err(error) if ssh::is_fatal(&error) => {
                    tracing::warn!(
                        event = "ssh_connect_failed",
                        slot = index,
                        error = %error,
                        "SSH connect failed; retrying on the next connection request"
                    );
                    self.report_connect_error(&error);
                    None
                }
                Err(error) => {
                    tracing::warn!(
                        event = "ssh_connect_failed",
                        slot = index,
                        retry_in = ?backoff,
                        error = format!("{error:#}"),
                        "SSH connect failed"
                    );
                    self.report_connect_error(&error);
                    Some(backoff)
                }
            };
            self.wait_backoff(retry_in).await;
            backoff = (backoff * 2).min(RECONNECT_BACKOFF_MAX);
        }
    }

    /// Open a direct-tcpip channel on the least-loaded live session.
    ///
    /// Waits briefly for a session when none is live, and retries on another
    /// session if the chosen one turns out to be dead.
    pub async fn open_channel(
        &self,
        request: &Request,
        peer_addr: &SocketAddr,
    ) -> Result<OpenedChannel> {
        let mut last_error = None;
        for _ in 0..CHANNEL_OPEN_ATTEMPTS {
            let lease = self.acquire().await?;
            match self.open_on(&lease.0, request, peer_addr).await {
                Ok(channel) => return Ok(OpenedChannel { channel, lease }),
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
                    lease.kill_session();
                    last_error = Some(error);
                }
            }
        }
        Err(last_error
            .unwrap_or_else(|| anyhow!("no SSH session attempts were made"))
            .context("failed to open SSH channel"))
    }

    /// Disconnect every live session.
    pub async fn close(&self) {
        let sessions: Vec<_> = self.slots.send_replace(vec![None; POOL_SIZE]);
        join_all(
            sessions
                .iter()
                .flatten()
                .map(|session| session.disconnect()),
        )
        .await;
        self.stats.ssh_disconnected();
    }

    async fn acquire(&self) -> Result<Lease> {
        let mut slots = self.slots.subscribe();
        if !slots
            .borrow()
            .iter()
            .flatten()
            .any(|session| session.is_live())
        {
            self.wake.notify_waiters();
        }
        let slots = timeout(
            SESSION_WAIT_TIMEOUT,
            slots.wait_for(|slots| slots.iter().flatten().any(|session| session.is_live())),
        )
        .await
        .map_err(|_| anyhow!("no SSH session became available within {SESSION_WAIT_TIMEOUT:?}"))?
        .context("SSH session pool closed")?;
        let session = slots
            .iter()
            .flatten()
            .filter(|session| session.is_live())
            .min_by_key(|session| session.active_channels.load(Ordering::Relaxed))
            .cloned()
            .context("SSH session closed while being selected")?;
        Ok(Lease::new(session))
    }

    async fn open_on(
        &self,
        session: &Session,
        request: &Request,
        peer_addr: &SocketAddr,
    ) -> std::result::Result<russh::Channel<client::Msg>, ChannelAttemptError> {
        let open = session.handle.channel_open_direct_tcpip(
            request.host.clone(),
            request.port.into(),
            peer_addr.ip().to_string(),
            peer_addr.port().into(),
        );
        tokio::pin!(open);

        tokio::select! {
            result = &mut open => return classify_channel_open(result),
            () = sleep(CHANNEL_OPEN_RESPONSE_TIMEOUT) => {}
        }
        tracing::warn!(
            event = "ssh_channel_stalled",
            "SSH channel open has not responded after {CHANNEL_OPEN_RESPONSE_TIMEOUT:?}; checking session liveness"
        );
        tokio::select! {
            result = &mut open => classify_channel_open(result),
            () = self.check_sessions() => {
                if session.is_live() {
                    tracing::info!(
                        event = "ssh_ping_answered",
                        "SSH session answered ping; continuing to wait for channel open"
                    );
                    classify_channel_open(open.await)
                } else {
                    Err(ChannelAttemptError::Session(anyhow!(
                        "SSH session did not answer ping within {SSH_PING_TIMEOUT:?}"
                    )))
                }
            }
        }
    }

    /// Ping every live session at once and replace those that do not answer.
    ///
    /// Sessions usually die together (sleep, network change), so checking
    /// them all avoids paying the stall timeout once per session.
    async fn check_sessions(&self) {
        let sessions: Vec<_> = self
            .slots
            .borrow()
            .iter()
            .flatten()
            .filter(|session| session.is_live())
            .cloned()
            .collect();
        join_all(sessions.iter().map(|session| async move {
            if !matches!(
                timeout(SSH_PING_TIMEOUT, session.handle.send_ping()).await,
                Ok(Ok(()))
            ) {
                session.kill();
            }
        }))
        .await;
    }

    /// Show a connect error unless another session still serves clients.
    fn report_connect_error(&self, error: &anyhow::Error) {
        let slots = self.slots.borrow();
        if !slots.iter().flatten().any(|session| session.is_live()) {
            self.stats.set_error(format!("{error:#}"));
        }
    }

    /// Sleep for `backoff`, or until a client asks for a session, keeping
    /// attempts at least `RECONNECT_BACKOFF_MIN` apart. Without a `backoff`,
    /// wait for a client only.
    async fn wait_backoff(&self, backoff: Option<Duration>) {
        let started = Instant::now();
        let timer = async {
            match backoff {
                Some(backoff) => sleep(backoff).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = timer => {}
            () = self.wake.notified() => {
                sleep(RECONNECT_BACKOFF_MIN.saturating_sub(started.elapsed())).await;
            }
        }
    }

    fn set_slot(&self, index: usize, session: Option<Arc<Session>>) {
        // Report while holding the slots lock so concurrent updates cannot
        // publish stale counts out of order.
        self.slots.send_modify(|slots| {
            slots[index] = session;
            let live = slots.iter().flatten().filter(|s| s.is_live()).count();
            if live > 0 {
                self.stats.ssh_connected();
                self.stats.clear_error();
                self.stats.set_status(format!(
                    "{} · {live}/{POOL_SIZE} SSH sessions",
                    self.listening_status
                ));
            } else {
                self.stats.ssh_disconnected();
                self.stats.set_status("Connecting to SSH server...");
            }
        });
    }
}

enum ChannelAttemptError {
    Target(russh::Error),
    Session(anyhow::Error),
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
