use crate::quic_utils;
use quinn::{Endpoint, ServerConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

const STREAM_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn run_server(port: u16) -> anyhow::Result<()> {
    // 1. Configure Server
    let (certs, key) = quic_utils::generate_self_signed_cert()?;
    let mut server_config = ServerConfig::with_single_cert(certs, key)?;
    server_config.transport_config(Arc::new(quic_utils::configure_transport()));
    
    let addr = format!("0.0.0.0:{}", port).parse()?;
    
    // 2. Bind
    let endpoint = Endpoint::server(server_config, addr)?;
    log::info!("Tunnel Server listening on {}", endpoint.local_addr()?);

    // 3. Accept Connections
    while let Some(conn) = endpoint.accept().await {
        tokio::spawn(async move {
            if let Ok(connection) = conn.await {
                log::info!("Connection from {}", connection.remote_address());
                while let Ok((send, recv)) = connection.accept_bi().await {
                    tokio::spawn(async move {
                        if let Err(e) = handle_stream(send, recv).await {
                            log::debug!("Stream error: {}", e);
                        }
                    });
                }
            }
        });
    }

    Ok(())
}

async fn handle_stream(mut send: quinn::SendStream, mut recv: quinn::RecvStream) -> anyhow::Result<()> {
    // 1. Read Target (with timeout and limit)
    let mut len_buf = [0u8; 2];
    tokio::time::timeout(STREAM_HANDSHAKE_TIMEOUT, recv.read_exact(&mut len_buf)).await??;
    let len = u16::from_be_bytes(len_buf) as usize;
    
    if len > 1024 || len == 0 {
        anyhow::bail!("Invalid target address length: {}", len);
    }
    
    let mut target_buf = vec![0u8; len];
    tokio::time::timeout(STREAM_HANDSHAKE_TIMEOUT, recv.read_exact(&mut target_buf)).await??;
    let target = String::from_utf8(target_buf)?;
    
    log::info!("Connecting to target: {}", target);

    // 2. Connect TCP
    let tcp_stream = match TcpStream::connect(&target).await {
        Ok(s) => {
            send.write_all(&[0]).await?;
            s
        }
        Err(e) => {
            let _ = send.write_all(&[1]).await;
            let _ = send.finish();
            return Err(e.into());
        }
    };
    
    // 3. Bridge
    let (mut tcp_read, mut tcp_write) = tcp_stream.into_split();
    
    let client_to_server = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut recv, &mut tcp_write).await;
        let _ = tcp_write.shutdown().await;
    });
    
    let server_to_client = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut tcp_read, &mut send).await;
        let _ = send.finish();
    });
    
    let _ = tokio::join!(client_to_server, server_to_client);
    
    Ok(())
}
