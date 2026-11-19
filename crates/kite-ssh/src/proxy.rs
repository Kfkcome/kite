//! SOCKS5 (RFC 1928 / RFC 1929) and HTTP CONNECT proxy tunnelling for SSH connections.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::Failure;

/// Proxy protocol used to reach the SSH server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyKind {
    Socks5,
    Http,
}

/// Network proxy configuration (SOCKS5 or HTTP CONNECT).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetProxy {
    pub kind: ProxyKind,
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

fn proxy_err(msg: impl Into<String>) -> Failure {
    Failure::Transport(format!("proxy: {}", msg.into()))
}

pub(crate) async fn connect_via_proxy(
    proxy: &NetProxy,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream, Failure> {
    let mut stream = TcpStream::connect((proxy.host.as_str(), proxy.port))
        .await
        .map_err(|e| proxy_err(format!("{}:{}: {e}", proxy.host, proxy.port)))?;
    match proxy.kind {
        ProxyKind::Socks5 => {
            handshake_socks5(&mut stream, proxy, target_host, target_port).await?;
        }
        ProxyKind::Http => {
            handshake_http(&mut stream, proxy, target_host, target_port).await?;
        }
    }
    Ok(stream)
}

async fn handshake_socks5<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    proxy: &NetProxy,
    target_host: &str,
    target_port: u16,
) -> Result<(), Failure> {
    let has_auth = proxy
        .username
        .as_deref()
        .is_some_and(|u| !u.is_empty());
    let greeting: &[u8] = if has_auth {
        &[0x05, 0x02, 0x00, 0x02]
    } else {
        &[0x05, 0x01, 0x00]
    };
    stream
        .write_all(greeting)
        .await
        .map_err(|e| proxy_err(e.to_string()))?;

    let mut chosen = [0u8; 2];
    stream
        .read_exact(&mut chosen)
        .await
        .map_err(|e| proxy_err(e.to_string()))?;
    if chosen[0] != 0x05 {
        return Err(proxy_err("invalid SOCKS5 greeting response"));
    }
    match chosen[1] {
        0x00 => {}
        0x02 if has_auth => {
            let user = proxy.username.as_deref().unwrap_or_default().as_bytes();
            let pass = proxy.password.as_deref().unwrap_or_default().as_bytes();
            let ulen = u8::try_from(user.len())
                .map_err(|_| proxy_err("SOCKS5 username too long"))?;
            let plen = u8::try_from(pass.len())
                .map_err(|_| proxy_err("SOCKS5 password too long"))?;
            let mut auth = Vec::with_capacity(3 + user.len() + pass.len());
            auth.push(0x01);
            auth.push(ulen);
            auth.extend_from_slice(user);
            auth.push(plen);
            auth.extend_from_slice(pass);
            stream
                .write_all(&auth)
                .await
                .map_err(|e| proxy_err(e.to_string()))?;
            let mut resp = [0u8; 2];
            stream
                .read_exact(&mut resp)
                .await
                .map_err(|e| proxy_err(e.to_string()))?;
            if resp[1] != 0x00 {
                return Err(proxy_err("SOCKS5 authentication failed"));
            }
        }
        0xff => return Err(proxy_err("SOCKS5 proxy rejected authentication methods")),
        _ => return Err(proxy_err("SOCKS5 proxy requested unsupported auth method")),
    }

    let mut req = Vec::with_capacity(7 + target_host.len());
    req.extend_from_slice(&[0x05, 0x01, 0x00]);
    match target_host.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            req.push(0x01);
            req.extend_from_slice(&v4.octets());
        }
        Ok(IpAddr::V6(v6)) => {
            req.push(0x04);
            req.extend_from_slice(&v6.octets());
        }
        Err(_) => {
            let host_bytes = target_host.as_bytes();
            let hlen = u8::try_from(host_bytes.len())
                .map_err(|_| proxy_err("target hostname too long for SOCKS5"))?;
            req.push(0x03);
            req.push(hlen);
            req.extend_from_slice(host_bytes);
        }
    }
    req.extend_from_slice(&target_port.to_be_bytes());
    stream
        .write_all(&req)
        .await
        .map_err(|e| proxy_err(e.to_string()))?;

    let mut head = [0u8; 4];
    stream
        .read_exact(&mut head)
        .await
        .map_err(|e| proxy_err(e.to_string()))?;
    if head[0] != 0x05 {
        return Err(proxy_err("invalid SOCKS5 connect response"));
    }
    if head[1] != 0x00 {
        return Err(proxy_err(format!("SOCKS5 connect failed (code {})", head[1])));
    }
    let addr_len = match head[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut l = [0u8; 1];
            stream
                .read_exact(&mut l)
                .await
                .map_err(|e| proxy_err(e.to_string()))?;
            usize::from(l[0])
        }
        _ => return Err(proxy_err("invalid SOCKS5 bound address type")),
    };
    let mut discard = vec![0u8; addr_len + 2];
    stream
        .read_exact(&mut discard)
        .await
        .map_err(|e| proxy_err(e.to_string()))?;
    Ok(())
}

async fn handshake_http<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    proxy: &NetProxy,
    target_host: &str,
    target_port: u16,
) -> Result<(), Failure> {
    let authority = if target_host.contains(':') && !target_host.starts_with('[') {
        format!("[{target_host}]:{target_port}")
    } else {
        format!("{target_host}:{target_port}")
    };
    let mut req = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some(user) = proxy.username.as_deref().filter(|u| !u.is_empty()) {
        let pass = proxy.password.as_deref().unwrap_or_default();
        let creds = encode_base64(format!("{user}:{pass}").as_bytes());
        req.push_str(&format!("Proxy-Authorization: Basic {creds}\r\n"));
    }
    req.push_str("\r\n");
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| proxy_err(e.to_string()))?;

    let mut buf = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    while buf.len() < 8192 {
        let n = stream
            .read(&mut byte)
            .await
            .map_err(|e| proxy_err(e.to_string()))?;
        if n == 0 {
            return Err(proxy_err("HTTP proxy closed connection"));
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let header = String::from_utf8_lossy(&buf);
    let status_line = header.lines().next().unwrap_or_default();
    let code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok());
    match code {
        Some(200) => Ok(()),
        Some(c) => Err(proxy_err(format!("HTTP proxy returned status {c}"))),
        None => Err(proxy_err("invalid HTTP proxy response")),
    }
}

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn socks5_handshake_connects_domain_without_auth() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let proxy = NetProxy {
            kind: ProxyKind::Socks5,
            host: "127.0.0.1".into(),
            port: 7890,
            username: None,
            password: None,
        };
        let srv = tokio::spawn(async move {
            let mut g = [0u8; 3];
            server.read_exact(&mut g).await.unwrap();
            assert_eq!(g, [0x05, 0x01, 0x00]);
            server.write_all(&[0x05, 0x00]).await.unwrap();
            let mut req = [0u8; 4];
            server.read_exact(&mut req).await.unwrap();
            assert_eq!(req, [0x05, 0x01, 0x00, 0x03]);
            let mut len = [0u8; 1];
            server.read_exact(&mut len).await.unwrap();
            let mut rest = vec![0u8; usize::from(len[0]) + 2];
            server.read_exact(&mut rest).await.unwrap();
            assert_eq!(&rest[..usize::from(len[0])], b"example.com");
            assert_eq!(&rest[usize::from(len[0])..], &22u16.to_be_bytes());
            server
                .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await
                .unwrap();
        });
        handshake_socks5(&mut client, &proxy, "example.com", 22)
            .await
            .unwrap();
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn http_connect_sends_basic_auth_and_checks_200() {
        let (mut client, mut server) = tokio::io::duplex(512);
        let proxy = NetProxy {
            kind: ProxyKind::Http,
            host: "127.0.0.1".into(),
            port: 7890,
            username: Some("alice".into()),
            password: Some("secret".into()),
        };
        let srv = tokio::spawn(async move {
            let mut buf = vec![0u8; 256];
            let n = server.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            assert!(req.starts_with("CONNECT 10.0.0.5:22 HTTP/1.1\r\n"));
            assert!(req.contains("Proxy-Authorization: Basic YWxpY2U6c2VjcmV0\r\n"));
            server
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
        });
        handshake_http(&mut client, &proxy, "10.0.0.5", 22)
            .await
            .unwrap();
        srv.await.unwrap();
    }
}
