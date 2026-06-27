use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use russh::{
    ChannelMsg, Disconnect, client,
    keys::{HashAlg, PrivateKeyWithHashAlg, load_secret_key, ssh_key},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex as TokioMutex, oneshot},
    time::{sleep, timeout},
};

use crate::{
    config::{AppConfig, AppPaths, save_config},
    stats::ProxyStats,
};

const CHANNEL_OPEN_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
const SSH_PING_TIMEOUT: Duration = Duration::from_secs(3);

/// Shared SSH session reused across all SOCKS connections.
struct SessionState {
    handle: client::Handle<Client>,
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
    /// Set to true when `channel_open_direct_tcpip` fails, cleared on successful reconnect.
    /// Lets subsequent callers skip the dead handle and go straight to reconnection.
    dead: bool,
}

pub async fn run_proxy(
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
    stats: Arc<ProxyStats>,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<()> {
    let local_addr = {
        let config = config.lock().unwrap().clone();
        config.validate_ready()?;
        config.local_addr
    };

    let listener = TcpListener::bind(&local_addr)
        .await
        .with_context(|| format!("failed to bind {local_addr}"))?;

    // Establish one SSH session to reuse across all SOCKS connections.
    stats.set_status("Connecting to SSH server\u{2026}");
    let handle = connect_ssh(Arc::clone(&config), paths.clone()).await?;
    let session = Arc::new(TokioMutex::new(SessionState {
        handle,
        config: Arc::clone(&config),
        paths,
        dead: false,
    }));
    stats.clear_error();
    stats.set_status(format!("Listening on {local_addr}"));
    stats.ssh_connected();

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                let state = session.lock().await;
                if !state.dead {
                    stats.ssh_disconnected();
                }
                state.handle.disconnect(Disconnect::ByApplication, "", "English").await.ok();
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
    session: Arc<TokioMutex<SessionState>>,
    stats: Arc<ProxyStats>,
) -> Result<()> {
    negotiate_no_auth(&mut stream).await?;
    let request = read_socks_request(&mut stream).await?;

    let mut channel = match open_channel_with_retry(&session, &request, &peer_addr, &stats).await {
        Ok(channel) => channel,
        Err(error) => {
            let _ = write_socks_reply(&mut stream, 0x05).await;
            return Err(error);
        }
    };

    write_socks_reply(&mut stream, 0x00).await?;
    pump(stream, &mut channel, stats).await?;
    // Session is shared across all connections — do not disconnect.
    Ok(())
}

/// Open a direct-tcpip channel, reconnecting the SSH session if it has died.
///
/// Holds the session lock across the first attempt and any reconnection so that
/// concurrent SOCKS connections serialize on the mutex. Only one task
/// reconnects; the others wake up to a fresh handle. A `dead` flag on the
/// session state avoids retrying or recounting a handle that is already known
/// to be dead.
async fn open_channel_with_retry(
    session: &Arc<TokioMutex<SessionState>>,
    request: &SocksRequest,
    peer_addr: &SocketAddr,
    stats: &ProxyStats,
) -> Result<russh::Channel<client::Msg>> {
    let mut state = session.lock().await;

    if !state.dead && state.handle.is_closed() {
        log::warn!("SSH session handle is closed");
        mark_session_dead(&mut state, stats);
    }

    if !state.dead {
        match open_channel_with_stall_check(&state.handle, request, peer_addr).await {
            Ok(channel) => return Ok(channel),
            Err(ChannelAttemptError::Target(error)) => {
                return Err(error).context("SSH server failed to open target channel");
            }
            Err(ChannelAttemptError::Session(error)) => {
                log::warn!("SSH session failed: {error}");
                mark_session_dead(&mut state, stats);
            }
        }
    }

    reconnect_session(&mut state, stats).await?;
    open_direct_tcpip(&state.handle, request, peer_addr)
        .await
        .context("failed to open SSH channel after reconnect")
}

enum ChannelAttemptError {
    Target(russh::Error),
    Session(anyhow::Error),
}

async fn open_channel_with_stall_check(
    handle: &client::Handle<Client>,
    request: &SocksRequest,
    peer_addr: &SocketAddr,
) -> std::result::Result<russh::Channel<client::Msg>, ChannelAttemptError> {
    let open = open_direct_tcpip(handle, request, peer_addr);
    tokio::pin!(open);

    tokio::select! {
        result = &mut open => classify_channel_open(result),
        () = sleep(CHANNEL_OPEN_RESPONSE_TIMEOUT) => {
            log::warn!(
                "SSH channel open has not responded after {CHANNEL_OPEN_RESPONSE_TIMEOUT:?}; checking session liveness"
            );
            let ping = timeout(SSH_PING_TIMEOUT, handle.send_ping());
            tokio::pin!(ping);
            tokio::select! {
                result = &mut open => classify_channel_open(result),
                result = &mut ping => {
                    match result {
                        Ok(Ok(())) => {
                            log::info!("SSH session answered ping; continuing to wait for channel open");
                            classify_channel_open(open.as_mut().await)
                        }
                        Ok(Err(error)) => Err(ChannelAttemptError::Session(
                            anyhow::Error::new(error).context("SSH ping failed after channel-open stall"),
                        )),
                        Err(_) => Err(ChannelAttemptError::Session(anyhow!(
                            "SSH ping timed out after {SSH_PING_TIMEOUT:?}"
                        ))),
                    }
                }
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
    }
}

async fn reconnect_session(state: &mut SessionState, stats: &ProxyStats) -> Result<()> {
    log::info!("Reconnecting SSH session...");
    let new_handle = connect_ssh(Arc::clone(&state.config), state.paths.clone())
        .await
        .context("failed to reconnect SSH session")?;
    state.handle = new_handle;
    state.dead = false;
    stats.ssh_connected();
    log::info!("SSH session reconnected");
    Ok(())
}

async fn open_direct_tcpip(
    handle: &client::Handle<Client>,
    request: &SocksRequest,
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

async fn connect_ssh(
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
) -> Result<client::Handle<Client>> {
    let snapshot = config.lock().unwrap().clone();
    log::info!(
        "Connecting to {}@{}:{} (auth={})",
        snapshot.username,
        snapshot.server,
        snapshot.port,
        snapshot.auth_method,
    );
    let ssh_config = Arc::new(client::Config {
        nodelay: true,
        ..Default::default()
    });
    let handler = Client { config, paths };
    let mut session = client::connect(
        ssh_config,
        (snapshot.server.as_str(), snapshot.port),
        handler,
    )
    .await
    .with_context(|| format!("failed to connect SSH server {}", snapshot.server))?;

    if snapshot.auth_method == "password" {
        log::info!("Authenticating with password");
        let auth_result = session
            .authenticate_password(snapshot.username, snapshot.ssh_password)
            .await
            .context("SSH password authentication failed")?;
        if !auth_result.success() {
            bail!("SSH password authentication was rejected");
        }
    } else {
        log::info!("Authenticating with public key: {}", snapshot.key_path);
        let passphrase = if snapshot.key_password.is_empty() {
            None
        } else {
            Some(snapshot.key_password.as_str())
        };
        let key_pair =
            load_secret_key(&snapshot.key_path, passphrase).context("failed to load SSH key")?;
        let auth_result = session
            .authenticate_publickey(
                snapshot.username,
                PrivateKeyWithHashAlg::new(
                    Arc::new(key_pair),
                    session.best_supported_rsa_hash().await?.flatten(),
                ),
            )
            .await
            .context("SSH public key authentication failed")?;
        if !auth_result.success() {
            bail!("SSH public key authentication was rejected");
        }
    }

    log::info!("SSH authenticated successfully");
    Ok(session)
}

pub struct Client {
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
}

impl client::Handler for Client {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key.fingerprint(HashAlg::Sha256).to_string();
        let mut config = self.config.lock().unwrap();
        match &config.host_fingerprint {
            Some(expected) => Ok(expected == &fingerprint),
            None => {
                config.host_fingerprint = Some(fingerprint);
                save_config(&self.paths, &config)?;
                Ok(true)
            }
        }
    }
}

struct SocksRequest {
    host: String,
    port: u16,
}

async fn negotiate_no_auth(stream: &mut TcpStream) -> Result<()> {
    let mut header = [0; 2];
    stream.read_exact(&mut header).await?;
    if header[0] != 5 {
        bail!("unsupported SOCKS version {}", header[0]);
    }

    let mut methods = vec![0; header[1] as usize];
    stream.read_exact(&mut methods).await?;
    if methods.contains(&0) {
        stream.write_all(&[5, 0]).await?;
        Ok(())
    } else {
        stream.write_all(&[5, 0xff]).await?;
        bail!("SOCKS client did not offer no-auth method");
    }
}

async fn read_socks_request(stream: &mut TcpStream) -> Result<SocksRequest> {
    let mut header = [0; 4];
    stream.read_exact(&mut header).await?;
    if header[0] != 5 {
        bail!("unsupported SOCKS request version {}", header[0]);
    }
    if header[1] != 1 {
        write_socks_reply(stream, 0x07).await?;
        bail!("unsupported SOCKS command {}", header[1]);
    }

    let host = match header[3] {
        1 => {
            let mut bytes = [0; 4];
            stream.read_exact(&mut bytes).await?;
            IpAddr::V4(Ipv4Addr::from(bytes)).to_string()
        }
        3 => {
            let len = stream.read_u8().await? as usize;
            let mut bytes = vec![0; len];
            stream.read_exact(&mut bytes).await?;
            String::from_utf8(bytes).context("SOCKS domain is not UTF-8")?
        }
        4 => {
            let mut bytes = [0; 16];
            stream.read_exact(&mut bytes).await?;
            IpAddr::V6(Ipv6Addr::from(bytes)).to_string()
        }
        atyp => {
            write_socks_reply(stream, 0x08).await?;
            bail!("unsupported SOCKS address type {atyp}");
        }
    };

    let port = stream.read_u16().await?;
    Ok(SocksRequest { host, port })
}

async fn write_socks_reply(stream: &mut TcpStream, code: u8) -> Result<()> {
    stream
        .write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0])
        .await
        .context("failed to write SOCKS reply")
}

async fn pump(
    mut stream: TcpStream,
    channel: &mut russh::Channel<client::Msg>,
    stats: Arc<ProxyStats>,
) -> Result<()> {
    let mut stream_closed = false;
    let mut buf = [0; 16 * 1024];

    loop {
        tokio::select! {
            read = stream.read(&mut buf), if !stream_closed => {
                match read? {
                    0 => {
                        stream_closed = true;
                        channel.eof().await?;
                    }
                    n => {
                        stats.add_up(n);
                        channel.data(&buf[..n]).await?;
                    }
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        stats.add_down(data.len());
                        stream.write_all(&data).await?;
                    }
                    Some(ChannelMsg::Eof) | None => {
                        if !stream_closed {
                            channel.eof().await.ok();
                        }
                        break;
                    }
                    Some(ChannelMsg::WindowAdjusted { .. }) => {}
                    Some(_) => {}
                }
            }
        }
    }

    Ok(())
}
