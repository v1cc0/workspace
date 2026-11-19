use quinn::{Connection, RecvStream, SendStream};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use url::Url;

const MAX_INITIAL_REQUEST_BYTES: usize = 32 * 1024;
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TUNNEL_ACK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Default)]
pub struct TunnelState {
    inner: Arc<RwLock<Option<Connection>>>,
}

impl TunnelState {
    pub async fn set(&self, conn: Option<Connection>) {
        *self.inner.write().await = conn;
    }

    pub async fn get(&self) -> Option<Connection> {
        self.inner.read().await.clone()
    }
}

pub struct LocalProxy {
    tunnel: TunnelState,
    direct_fallback: bool,
}

impl LocalProxy {
    pub fn new(tunnel: TunnelState, direct_fallback: bool) -> Self {
        Self {
            tunnel,
            direct_fallback,
        }
    }

    pub async fn serve(self, listener: TcpListener) -> anyhow::Result<()> {
        log::info!("Local HTTP Proxy listening on {}", listener.local_addr()?);
        loop {
            let (socket, _) = listener.accept().await?;
            let tunnel = self.tunnel.clone();
            let direct_fallback = self.direct_fallback;
            tokio::spawn(async move {
                if let Err(e) = handle_proxy_conn(socket, tunnel, direct_fallback).await {
                    log::warn!("Proxy connection failed: {}", e);
                }
            });
        }
    }
}

struct ParsedRequest {
    method: String,
    path: String,
    header_len: usize,
    bytes: Vec<u8>,
    host_header: Option<String>,
}

async fn handle_proxy_conn(
    mut client_socket: TcpStream,
    tunnel: TunnelState,
    direct_fallback: bool,
) -> anyhow::Result<()> {
    let request = read_initial_request(&mut client_socket).await?;
    let target = target_from_request(&request)?;

    if let Some(conn) = tunnel.get().await {
        match conn.open_bi().await {
            Ok((mut send, mut recv)) => {
                match send_target_and_wait_ack(&mut send, &mut recv, &target).await {
                    Ok(0) => return proxy_via_tunnel(client_socket, request, send, recv).await,
                    Ok(_) => {
                        if direct_fallback {
                            return proxy_direct(client_socket, request, &target).await;
                        }
                        write_http_error(
                            &mut client_socket,
                            "502 Bad Gateway",
                            "Upstream connect failed",
                        )
                        .await?;
                        return Ok(());
                    }
                    Err(e) => {
                        log::warn!("Tunnel handshake failed for {target}: {e}");
                        if direct_fallback {
                            return proxy_direct(client_socket, request, &target).await;
                        }
                        write_http_error(
                            &mut client_socket,
                            "503 Service Unavailable",
                            "Tunnel unavailable",
                        )
                        .await?;
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                log::warn!("Tunnel stream open failed for {target}: {e}");
                if direct_fallback {
                    return proxy_direct(client_socket, request, &target).await;
                }
                write_http_error(
                    &mut client_socket,
                    "503 Service Unavailable",
                    "Tunnel unavailable",
                )
                .await?;
                return Ok(());
            }
        }
    }

    if !direct_fallback {
        write_http_error(
            &mut client_socket,
            "503 Service Unavailable",
            "Tunnel not connected",
        )
        .await?;
        return Ok(());
    }

    proxy_direct(client_socket, request, &target).await
}

async fn send_target_and_wait_ack(
    send: &mut SendStream,
    recv: &mut RecvStream,
    target: &str,
) -> anyhow::Result<u8> {
    let target_bytes = target.as_bytes();
    if target_bytes.len() > 1024 {
        anyhow::bail!("Target address too long: {}", target_bytes.len());
    }

    let len = (target_bytes.len() as u16).to_be_bytes();
    tokio::time::timeout(TUNNEL_ACK_TIMEOUT, send.write_all(&len)).await??;
    tokio::time::timeout(TUNNEL_ACK_TIMEOUT, send.write_all(target_bytes)).await??;

    let mut ack = [0u8; 1];
    let ack_res = tokio::time::timeout(TUNNEL_ACK_TIMEOUT, recv.read_exact(&mut ack)).await;
    match ack_res {
        Ok(Ok(())) => Ok(ack[0]),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => anyhow::bail!("tunnel ack timeout"),
    }
}

async fn read_initial_request(socket: &mut TcpStream) -> anyhow::Result<ParsedRequest> {
    let mut bytes = Vec::with_capacity(4096);
    let mut buf = [0u8; 4096];
    let (method, path, header_len, host_header) = loop {
        let n = socket.read(&mut buf).await?;
        if n == 0 {
            anyhow::bail!("client closed before sending request");
        }
        bytes.extend_from_slice(&buf[..n]);
        if bytes.len() > MAX_INITIAL_REQUEST_BYTES {
            anyhow::bail!("request headers too large");
        }

        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = httparse::Request::new(&mut headers);
        let status = req.parse(&bytes)?;
        if !status.is_complete() {
            continue;
        }

        let mut header_len = status.unwrap();
        let method = req.method.unwrap_or("").to_string();
        let path = req.path.unwrap_or("").to_string();

        let host_header = req
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("Host"))
            .map(|h| {
                std::str::from_utf8(h.value)
                    .unwrap_or("")
                    .trim()
                    .to_string()
            })
            .filter(|h| !h.is_empty());

        if !method.eq_ignore_ascii_case("CONNECT") {
            let url = if path.starts_with("http://")
                || path.starts_with("https://")
                || path.starts_with("ws://")
                || path.starts_with("wss://")
            {
                Some(Url::parse(&path)?)
            } else {
                None
            };
            let origin_target = url.as_ref().map_or(path.as_str(), |url| {
                &url[url::Position::BeforePath..url::Position::AfterQuery]
            });
            let upgrade = req
                .headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("Upgrade"))
                && req.headers.iter().any(|h| {
                    h.name.eq_ignore_ascii_case("Connection")
                        && h.value
                            .split(|b| *b == b',')
                            .any(|token| token.trim_ascii().eq_ignore_ascii_case(b"upgrade"))
                });
            let mut forwarded = format!(
                "{method} {origin_target} HTTP/1.{}\r\n",
                req.version.unwrap_or(1)
            )
            .into_bytes();
            for header in req.headers.iter() {
                if header.name.eq_ignore_ascii_case("Proxy-Connection")
                    || header.name.eq_ignore_ascii_case("Proxy-Authorization")
                    || (!upgrade && header.name.eq_ignore_ascii_case("Connection"))
                {
                    continue;
                }
                forwarded.extend_from_slice(header.name.as_bytes());
                forwarded.extend_from_slice(b": ");
                forwarded.extend_from_slice(header.value);
                forwarded.extend_from_slice(b"\r\n");
            }
            // One HTTP request per connection; upgraded streams stay bidirectional.
            if !upgrade {
                forwarded.extend_from_slice(b"Connection: close\r\n");
            }
            forwarded.extend_from_slice(b"\r\n");
            let forwarded_header_len = forwarded.len();
            forwarded.extend_from_slice(&bytes[header_len..]);
            bytes = forwarded;
            header_len = forwarded_header_len;
        }

        break (method, path, header_len, host_header);
    };

    Ok(ParsedRequest {
        method,
        path,
        header_len,
        bytes,
        host_header,
    })
}

fn target_from_request(request: &ParsedRequest) -> anyhow::Result<String> {
    if request.method.eq_ignore_ascii_case("CONNECT") {
        return Ok(request.path.clone());
    }

    if request.path.starts_with("http://")
        || request.path.starts_with("https://")
        || request.path.starts_with("ws://")
        || request.path.starts_with("wss://")
    {
        let url = Url::parse(&request.path)?;
        let host = url.host_str().unwrap_or("");
        if host.is_empty() {
            anyhow::bail!("no host in url");
        }
        let port = url.port_or_known_default().unwrap_or(80);
        return Ok(format!("{host}:{port}"));
    }

    let host = request
        .host_header
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no host header"))?
        .trim();
    if host.contains(':') {
        Ok(host.to_string())
    } else {
        Ok(format!("{host}:80"))
    }
}

async fn proxy_via_tunnel(
    client_socket: TcpStream,
    request: ParsedRequest,
    mut send: SendStream,
    mut recv: RecvStream,
) -> anyhow::Result<()> {
    let mut client_socket = client_socket;
    let is_connect = request.method.eq_ignore_ascii_case("CONNECT");
    if is_connect {
        client_socket
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
    }

    let initial_payload = if is_connect {
        request.bytes[request.header_len..].to_vec()
    } else {
        request.bytes
    };

    let (mut client_read, mut client_write) = client_socket.into_split();

    let client_to_upstream = async {
        if !initial_payload.is_empty() {
            send.write_all(&initial_payload).await?;
        }
        let _ = tokio::io::copy(&mut client_read, &mut send).await?;
        let _ = send.finish();
        Ok::<(), std::io::Error>(())
    };

    let upstream_to_client = async {
        let _ = tokio::io::copy(&mut recv, &mut client_write).await?;
        Ok::<(), std::io::Error>(())
    };

    let _ = tokio::try_join!(client_to_upstream, upstream_to_client)?;
    Ok(())
}

async fn proxy_direct(
    client_socket: TcpStream,
    request: ParsedRequest,
    target: &str,
) -> anyhow::Result<()> {
    let connect_res =
        tokio::time::timeout(UPSTREAM_CONNECT_TIMEOUT, TcpStream::connect(target)).await;
    let upstream = match connect_res {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            let mut client_socket = client_socket;
            write_http_error(
                &mut client_socket,
                "502 Bad Gateway",
                &format!("Connect failed: {e}"),
            )
            .await?;
            return Ok(());
        }
        Err(_) => {
            let mut client_socket = client_socket;
            write_http_error(&mut client_socket, "504 Gateway Timeout", "Connect timeout").await?;
            return Ok(());
        }
    };

    let mut client_socket = client_socket;
    let is_connect = request.method.eq_ignore_ascii_case("CONNECT");
    if is_connect {
        client_socket
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
    }

    let initial_payload = if is_connect {
        request.bytes[request.header_len..].to_vec()
    } else {
        request.bytes
    };

    let (mut client_read, mut client_write) = client_socket.into_split();
    let (mut upstream_read, mut upstream_write) = upstream.into_split();

    let client_to_upstream = async {
        if !initial_payload.is_empty() {
            upstream_write.write_all(&initial_payload).await?;
        }
        let _ = tokio::io::copy(&mut client_read, &mut upstream_write).await?;
        upstream_write.shutdown().await?;
        Ok::<(), std::io::Error>(())
    };

    let upstream_to_client = async {
        let _ = tokio::io::copy(&mut upstream_read, &mut client_write).await?;
        Ok::<(), std::io::Error>(())
    };

    let _ = tokio::try_join!(client_to_upstream, upstream_to_client)?;
    Ok(())
}

async fn write_http_error(socket: &mut TcpStream, status: &str, body: &str) -> anyhow::Result<()> {
    let body_bytes = body.as_bytes();
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    socket.write_all(headers.as_bytes()).await?;
    socket.write_all(body_bytes).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn absolute_requests_reach_the_origin_with_path_headers_and_body() -> anyhow::Result<()> {
        let origin = TcpListener::bind("127.0.0.1:0").await?;
        let address = origin.local_addr()?;
        let proxy = TcpListener::bind("127.0.0.1:0").await?;
        let proxy_address = proxy.local_addr()?;
        let proxy_task = tokio::spawn(LocalProxy::new(TunnelState::default(), true).serve(proxy));
        let origin_task = tokio::spawn(async move {
            let (mut socket, _) = origin.accept().await?;
            let mut request = Vec::new();
            socket.read_to_end(&mut request).await?;
            assert!(request.starts_with(b"POST /a%2Fb?query=%E4%B8%AD HTTP/1.1\r\n"));
            assert!(
                request
                    .windows(19)
                    .any(|part| part == b"Connection: close\r\n")
            );
            assert!(!request.windows(16).any(|part| part == b"Proxy-Connection"));
            assert!(
                !request
                    .windows(19)
                    .any(|part| part == b"Proxy-Authorization")
            );
            assert!(request.ends_with(b"\r\n\r\n\x00\xff"));
            assert!(
                request
                    .windows(22)
                    .any(|part| part == b"X-Fixture: preserved\r\n")
            );
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await?;
            anyhow::Ok(())
        });
        let mut client = TcpStream::connect(proxy_address).await?;
        client.write_all(format!("POST http://{address}/a%2Fb?query=%E4%B8%AD HTTP/1.1\r\nHost: {address}\r\nX-Fixture: preserved\r\nContent-Length: 2\r\nProxy-Authorization: Basic ZHVtbXk=\r\nProxy-Connection: keep-alive\r\nConnection: keep-alive\r\n\r\n").as_bytes()).await?;
        client.write_all(b"\x00\xff").await?;
        client.shutdown().await?;
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut response)).await??;
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert!(response.ends_with(b"\r\n\r\nok"));
        origin_task.await??;
        proxy_task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn strict_mode_never_connects_directly_without_a_tunnel() -> anyhow::Result<()> {
        for method in ["GET", "CONNECT"] {
            let upstream = std::net::TcpListener::bind("127.0.0.1:0")?;
            upstream.set_nonblocking(true)?;
            let target = upstream.local_addr()?;
            let request = if method == "CONNECT" {
                format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n")
            } else {
                format!("GET http://{target}/probe HTTP/1.1\r\nHost: {target}\r\n\r\n")
            };
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let mut client = TcpStream::connect(listener.local_addr()?).await?;
            let handler = tokio::spawn(async move {
                let (socket, _) = listener.accept().await?;
                handle_proxy_conn(socket, TunnelState::default(), false).await
            });
            client.write_all(request.as_bytes()).await?;
            let mut response = String::new();
            tokio::time::timeout(Duration::from_secs(2), client.read_to_string(&mut response))
                .await??;
            handler.await??;
            assert!(response.starts_with("HTTP/1.1 503 Service Unavailable"));
            assert_eq!(
                upstream.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock,
                "{method} bypassed the disconnected tunnel"
            );
        }
        Ok(())
    }
}
