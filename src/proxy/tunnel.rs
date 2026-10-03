use std::{error::Error, fmt, io};

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

/// Copy data both ways until the remote side finishes.
///
/// The two directions run concurrently so a full SSH window in one direction
/// cannot stall the other.
pub async fn pump(
    mut stream: TcpStream,
    channel: russh::Channel<client::Msg>,
    stats: &ProxyStats,
) -> Result<(), TunnelError> {
    let (mut from_remote, to_remote) = channel.split();
    let (mut local_read, mut local_write) = stream.split();

    let upload = async {
        let mut buf = vec![0; TUNNEL_BUFFER_SIZE];
        loop {
            match local_read
                .read(&mut buf)
                .await
                .map_err(TunnelError::LocalIo)?
            {
                0 => return to_remote.eof().await.map_err(TunnelError::Ssh),
                n => {
                    stats.add_up(n);
                    to_remote.data(&buf[..n]).await.map_err(TunnelError::Ssh)?;
                }
            }
        }
    };
    let download = async {
        while let Some(msg) = from_remote.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    stats.add_down(data.len());
                    local_write
                        .write_all(&data)
                        .await
                        .map_err(TunnelError::LocalIo)?;
                }
                ChannelMsg::Eof => break,
                _ => {}
            }
        }
        Ok(())
    };
    tokio::pin!(upload, download);

    let result = tokio::select! {
        result = &mut download => result,
        result = &mut upload => match result {
            Ok(()) => download.await,
            Err(error) => Err(error),
        },
    };
    // russh does not close channels on drop; release it on the server too.
    let _ = to_remote.close().await;
    result
}
