use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, bail};
use russh::{
    ChannelMsg, Disconnect, client,
    keys::{HashAlg, PrivateKeyWithHashAlg, load_secret_key, ssh_key},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

use crate::{
    config::{AppConfig, AppPaths, save_config},
    stats::ProxyStats,
};

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
    stats.clear_error();
    stats.set_status(format!("Listening on {local_addr}"));

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                stats.set_status("Stopped");
                return Ok(());
            }
            accepted = listener.accept() => {
                let (stream, peer_addr) = accepted.context("failed to accept local connection")?;
                let config = Arc::clone(&config);
                let paths = paths.clone();
                let stats = Arc::clone(&stats);
                tokio::spawn(async move {
                    stats.connection_opened();
                    if let Err(error) = handle_client(stream, peer_addr, config, paths, Arc::clone(&stats)).await {
                        stats.set_error(error.to_string());
                    }
                    stats.connection_closed();
                });
            }
        }
    }
}

async fn handle_client(
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
    stats: Arc<ProxyStats>,
) -> Result<()> {
    negotiate_no_auth(&mut stream).await?;
    let request = read_socks_request(&mut stream).await?;

    let session = match connect_ssh(Arc::clone(&config), paths).await {
        Ok(session) => session,
        Err(error) => {
            let _ = write_socks_reply(&mut stream, 0x05).await;
            return Err(error);
        }
    };

    let mut channel = match session
        .channel_open_direct_tcpip(
            request.host.clone(),
            request.port.into(),
            peer_addr.ip().to_string(),
            peer_addr.port().into(),
        )
        .await
    {
        Ok(channel) => channel,
        Err(error) => {
            let _ = write_socks_reply(&mut stream, 0x05).await;
            return Err(error).context("failed to open SSH direct-tcpip channel");
        }
    };

    write_socks_reply(&mut stream, 0x00).await?;
    pump(stream, &mut channel, stats).await?;
    session
        .disconnect(Disconnect::ByApplication, "", "English")
        .await
        .ok();
    Ok(())
}

async fn connect_ssh(
    config: Arc<Mutex<AppConfig>>,
    paths: AppPaths,
) -> Result<client::Handle<Client>> {
    let snapshot = config.lock().unwrap().clone();
    let key_pair = load_secret_key(&snapshot.key_path, None).context("failed to load SSH key")?;
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
