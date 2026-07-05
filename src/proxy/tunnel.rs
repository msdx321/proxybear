use std::{error::Error, fmt, io, sync::Arc};

use russh::{ChannelMsg, client};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::app::stats::ProxyStats;

const TUNNEL_BUFFER_SIZE: usize = 64 * 1024;

#[derive(Debug)]
pub enum TunnelError {
    LocalIo(io::Error),
    Ssh(russh::Error),
}

impl TunnelError {
    pub fn ssh_session_failed(&self) -> bool {
        matches!(self, Self::Ssh(_))
    }
}

impl fmt::Display for TunnelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocalIo(error) => write!(f, "local tunnel I/O failed: {error}"),
            Self::Ssh(error) => write!(f, "SSH tunnel failed: {error}"),
        }
    }
}

impl Error for TunnelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::LocalIo(error) => Some(error),
            Self::Ssh(error) => Some(error),
        }
    }
}

pub async fn pump(
    mut stream: TcpStream,
    channel: &mut russh::Channel<client::Msg>,
    stats: Arc<ProxyStats>,
) -> Result<(), TunnelError> {
    let mut stream_closed = false;
    let mut buf = [0; TUNNEL_BUFFER_SIZE];

    loop {
        tokio::select! {
            read = stream.read(&mut buf), if !stream_closed => {
                match read.map_err(TunnelError::LocalIo)? {
                    0 => {
                        stream_closed = true;
                        channel.eof().await.map_err(TunnelError::Ssh)?;
                    }
                    n => {
                        stats.add_up(n);
                        channel.data(&buf[..n]).await.map_err(TunnelError::Ssh)?;
                    }
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        stats.add_down(data.len());
                        stream.write_all(&data).await.map_err(TunnelError::LocalIo)?;
                    }
                    Some(ChannelMsg::Eof) | None => {
                        if !stream_closed {
                            channel.eof().await.ok();
                        }
                        break;
                    }
                    Some(_) => {}
                }
            }
        }
    }

    Ok(())
}
