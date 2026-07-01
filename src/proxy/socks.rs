use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use anyhow::{Context, Result, bail};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub const REPLY_SUCCEEDED: u8 = 0x00;
pub const REPLY_GENERAL_FAILURE: u8 = 0x05;

pub struct Request {
    pub(crate) host: String,
    pub(crate) port: u16,
}

pub async fn negotiate_no_auth(stream: &mut TcpStream) -> Result<()> {
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

pub async fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut header = [0; 4];
    stream.read_exact(&mut header).await?;
    if header[0] != 5 {
        bail!("unsupported SOCKS request version {}", header[0]);
    }
    if header[1] != 1 {
        write_reply(stream, 0x07).await?;
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
            write_reply(stream, 0x08).await?;
            bail!("unsupported SOCKS address type {atyp}");
        }
    };

    let port = stream.read_u16().await?;
    Ok(Request { host, port })
}

pub async fn write_reply(stream: &mut TcpStream, code: u8) -> Result<()> {
    stream
        .write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0])
        .await
        .context("failed to write SOCKS reply")
}
