use std::sync::Arc;

use anyhow::Result;
use russh::{ChannelMsg, client};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::app::stats::ProxyStats;

const TUNNEL_BUFFER_SIZE: usize = 64 * 1024;

pub async fn pump(
    mut stream: TcpStream,
    channel: &mut russh::Channel<client::Msg>,
    stats: Arc<ProxyStats>,
) -> Result<()> {
    let mut stream_closed = false;
    let mut buf = [0; TUNNEL_BUFFER_SIZE];

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
                    Some(_) => {}
                }
            }
        }
    }

    Ok(())
}
