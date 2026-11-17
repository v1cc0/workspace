#![cfg(target_os = "linux")]

#[allow(dead_code)]
#[path = "../../../src/backend.rs"]
mod backend;

use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Context;
use nsh_app_workspace::browser;
use nsh_core::{JsonValue, prelude::*};
use nsh_platform_linux::{App, AppOptions, AppView, Components, Root, gtk4 as gtk};
use quinn::Endpoint;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use webkit6::prelude::*;

static SOURCE: Mutex<String> = Mutex::new(String::new());
static EVENTS: Mutex<Vec<(String, JsonValue)>> = Mutex::new(Vec::new());

#[component]
fn BrowserFixture() -> impl IntoElement {
    let src = SOURCE.lock().unwrap().clone();
    view().flex_1().child(
        native("workspace.browser")
            .flex_1()
            .prop("src", JsonValue::from(src))
            .on_native_event(|event: &NativeEvent| {
                EVENTS
                    .lock()
                    .unwrap()
                    .push((event.name.to_string(), event.data.clone()));
            }),
    )
}

fn wait_for(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timeout waiting for {what}; events: {:?}",
            EVENTS.lock().unwrap()
        );
        while gtk::glib::MainContext::default().pending() {
            gtk::glib::MainContext::default().iteration(false);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn load(app: &AppView, src: String) {
    *SOURCE.lock().unwrap() = src;
    EVENTS.lock().unwrap().clear();
    app.load(App::Rust(Root::of::<BrowserFixture>()));
}

type Requests = Arc<Mutex<Vec<String>>>;

fn record_error(errors: &Requests, error: anyhow::Error) {
    // Disposing a WebView cancels in-flight sockets, including speculative ones.
    if error.downcast_ref::<std::io::Error>().is_some_and(|error| {
        matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::UnexpectedEof
        )
    }) {
        return;
    }
    errors.lock().unwrap().push(format!("{error:#}"));
}

async fn origin(
    mut socket: impl AsyncRead + AsyncWrite + Unpin,
    requests: Requests,
) -> anyhow::Result<()> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        anyhow::ensure!(bytes.len() < 32 * 1024, "fixture headers too large");
        bytes.push(socket.read_u8().await?);
    }
    let headers = String::from_utf8(bytes)?;
    let target = headers
        .split_whitespace()
        .nth(1)
        .context("request target")?;
    requests.lock().unwrap().push(target.to_owned());
    // Ordinary origin servers require origin-form, not the proxy's absolute-form.
    anyhow::ensure!(
        target.starts_with('/'),
        "origin received absolute-form: {target}"
    );
    if target == "/ws" {
        let key = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("sec-websocket-key")
                    .then(|| value.trim())
            })
            .context("WebSocket key")?;
        let mut checksum = gtk::glib::Checksum::new(gtk::glib::ChecksumType::Sha1).unwrap();
        checksum.update(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes());
        let accept = gtk::glib::base64_encode(&checksum.digest());
        socket.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").as_bytes()).await?;
        let mut frame = [0; 2];
        socket.read_exact(&mut frame).await?;
        anyhow::ensure!(
            frame == [0x81, 0x84],
            "expected a masked four-byte text frame"
        );
        let mut mask = [0; 4];
        let mut payload = [0; 4];
        socket.read_exact(&mut mask).await?;
        socket.read_exact(&mut payload).await?;
        for (byte, mask) in payload.iter_mut().zip(mask) {
            *byte ^= mask;
        }
        anyhow::ensure!(&payload == b"ping", "unexpected WebSocket message");
        socket.write_all(b"\x81\x04pong").await?;
    } else {
        let (status, extra, body) = match target {
            "/start" => ("302 Found", "Location: /page\r\n", ""),
            "/page" => (
                "200 OK",
                "Content-Type: text/html\r\n",
                r#"<!doctype html><meta charset="utf-8"><title>loading</title><link rel="stylesheet" href="/style.css"><script src="/script.js"></script><p>Workspace 代理验证</p><img src="/image.svg" alt="fixture">"#,
            ),
            "/script.js" => (
                "200 OK",
                "Content-Type: text/javascript\r\n",
                r#"const ws = new WebSocket(location.origin.replace('http', 'ws') + '/ws'); ws.onopen = () => ws.send('ping'); ws.onmessage = e => { if (e.data === 'pong') fetch('/resource').then(r => r.text()).then(() => document.title = 'ready-' + location.protocol); };"#,
            ),
            "/style.css" => (
                "200 OK",
                "Content-Type: text/css\r\n",
                "body { color: black; }",
            ),
            "/image.svg" => (
                "200 OK",
                "Content-Type: image/svg+xml\r\n",
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><rect width="1" height="1"/></svg>"#,
            ),
            "/resource" => ("200 OK", "Content-Type: text/plain\r\n", "ok"),
            "/loopback" => (
                "200 OK",
                "Content-Type: text/html\r\n",
                "<title>loopback-ready</title>",
            ),
            _ => (
                "404 Not Found",
                "Content-Type: text/html\r\n",
                "<title>unexpected-origin-request</title>",
            ),
        };
        socket.write_all(format!("HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
    }
    socket.shutdown().await?;
    Ok(())
}

struct Network {
    http: SocketAddr,
    https: SocketAddr,
    proxy: SocketAddr,
    tunnel: backend::TunnelState,
    client: Endpoint,
    server: Endpoint,
    proxy_task: tokio::task::JoinHandle<anyhow::Result<()>>,
    requests: Requests,
    accepts: Arc<AtomicUsize>,
    targets: Requests,
    errors: Requests,
    cert_pem: String,
}

impl Network {
    async fn start() -> anyhow::Result<Self> {
        let cert = rcgen::generate_simple_self_signed(vec![
            "workspace.invalid".into(),
            "localhost".into(),
        ])?;
        let cert_pem = cert.cert.pem();
        let certificate: CertificateDer<'static> = cert.cert.der().clone();
        let key = || PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
        let http_listener = TcpListener::bind("127.0.0.1:0").await?;
        let tls_listener = TcpListener::bind("127.0.0.1:0").await?;
        let http = http_listener.local_addr()?;
        let https = tls_listener.local_addr()?;
        let requests: Requests = Arc::default();
        let accepts = Arc::new(AtomicUsize::new(0));
        let errors: Requests = Arc::default();
        let tls = tokio_rustls::TlsAcceptor::from(Arc::new(
            rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![certificate.clone()], key())?,
        ));
        for (listener, encrypted) in [(http_listener, false), (tls_listener, true)] {
            let requests = requests.clone();
            let accepts = accepts.clone();
            let errors = errors.clone();
            let tls = tls.clone();
            tokio::spawn(async move {
                while let Ok((socket, _)) = listener.accept().await {
                    accepts.fetch_add(1, Ordering::SeqCst);
                    let (requests, errors, tls) = (requests.clone(), errors.clone(), tls.clone());
                    tokio::spawn(async move {
                        let result = async {
                            if encrypted {
                                origin(tls.accept(socket).await?, requests).await
                            } else {
                                origin(socket, requests).await
                            }
                        }
                        .await;
                        if let Err(error) = result {
                            record_error(&errors, error);
                        }
                    });
                }
            });
        }
        let server = Endpoint::server(
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], key())?,
            "127.0.0.1:0".parse()?,
        )?;
        let targets: Requests = Arc::default();
        tokio::spawn({
            let server = server.clone();
            let targets = targets.clone();
            let errors = errors.clone();
            async move {
                let connection = server.accept().await.unwrap().await.unwrap();
                while let Ok((mut send, mut recv)) = connection.accept_bi().await {
                    let targets = targets.clone();
                    let errors = errors.clone();
                    tokio::spawn(async move {
                        let result = async {
                            let mut length = [0; 2];
                            recv.read_exact(&mut length).await?;
                            let length = usize::from(u16::from_be_bytes(length));
                            anyhow::ensure!(
                                length > 0 && length <= 1024,
                                "invalid fixture target length"
                            );
                            let mut target = vec![0; length];
                            recv.read_exact(&mut target).await?;
                            let target = String::from_utf8(target)?;
                            let destination = if target
                                == format!("workspace.invalid:{}", http.port())
                                || target == http.to_string()
                            {
                                http
                            } else if target == format!("workspace.invalid:{}", https.port()) {
                                https
                            } else {
                                anyhow::bail!("unexpected fixture target: {target}");
                            };
                            targets.lock().unwrap().push(target);
                            let socket = TcpStream::connect(destination).await?;
                            send.write_all(&[0]).await?;
                            let (mut read, mut write) = socket.into_split();
                            let upload = async {
                                tokio::io::copy(&mut recv, &mut write).await?;
                                write.shutdown().await
                            };
                            let download = async {
                                tokio::io::copy(&mut read, &mut send).await?;
                                send.finish()?;
                                anyhow::Ok(())
                            };
                            let (up, down) = tokio::join!(upload, download);
                            up?;
                            down?;
                            anyhow::Ok(())
                        }
                        .await;
                        if let Err(error) = result {
                            record_error(&errors, error);
                        }
                    });
                }
            }
        });
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate)?;
        let mut client = Endpoint::client("127.0.0.1:0".parse()?)?;
        client.set_default_client_config(quinn::ClientConfig::with_root_certificates(Arc::new(
            roots,
        ))?);
        let connection = client.connect(server.local_addr()?, "localhost")?.await?;
        let tunnel = backend::TunnelState::default();
        tunnel.set(Some(connection)).await;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let proxy = listener.local_addr()?;
        let proxy_task = tokio::spawn(
            backend::LocalProxy::new(0, tunnel.clone())
                .with_direct_fallback(false)
                .serve(listener),
        );
        Ok(Self {
            http,
            https,
            proxy,
            tunnel,
            client,
            server,
            proxy_task,
            requests,
            accepts,
            targets,
            errors,
            cert_pem,
        })
    }
}

#[test]
#[ignore = "requires a Linux display and WebKitGTK 6; run with GTK_IM_MODULE=fcitx and --ignored"]
fn native_browser_uses_quic_for_http_https_and_fails_closed() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let _ = env_logger::try_init();
    gtk::init()?;
    let runtime = tokio::runtime::Runtime::new()?;
    let mut network = runtime.block_on(Network::start())?;
    let session = browser::session(network.proxy);
    // Trust only this generated fixture certificate in this ephemeral test session.
    session.allow_tls_certificate_for_host(
        &gtk::gio::TlsCertificate::from_pem(&network.cert_pem)?,
        "workspace.invalid",
    );
    let components = Components::default();
    browser::register(&components, &session);
    let app = AppView::new(AppOptions {
        components,
        ..Default::default()
    });
    let window = gtk::Window::builder()
        .title("Workspace browser proxy test")
        .default_width(1000)
        .default_height(700)
        .child(&app.widget())
        .build();
    window.present();
    load(&app, "file:///etc/passwd".to_owned());
    wait_for("unsupported URL rejection", || {
        EVENTS
            .lock()
            .unwrap()
            .iter()
            .any(|(name, _)| name == "error")
    });
    assert_eq!(network.accepts.load(Ordering::SeqCst), 0);
    for (scheme, address) in [("http", network.http), ("https", network.https)] {
        network.requests.lock().unwrap().clear();
        load(
            &app,
            format!("{scheme}://workspace.invalid:{}/start", address.port()),
        );
        let title = format!("ready-{scheme}:");
        wait_for(&title, || {
            let events = EVENTS.lock().unwrap();
            events
                .iter()
                .any(|(name, value)| name == "titleChanged" && value.as_str() == Some(&title))
                && events.iter().any(|(name, _)| name == "loaded")
        });
        for path in [
            "/start",
            "/page",
            "/style.css",
            "/script.js",
            "/image.svg",
            "/ws",
            "/resource",
        ] {
            assert!(
                network.requests.lock().unwrap().iter().any(|p| p == path),
                "missing {scheme} {path}"
            );
        }
        assert!(
            network
                .targets
                .lock()
                .unwrap()
                .iter()
                .any(|target| target == &format!("workspace.invalid:{}", address.port()))
        );
    }
    load(&app, format!("http://{}/loopback", network.http));
    wait_for("proxied loopback page", || {
        EVENTS
            .lock()
            .unwrap()
            .iter()
            .any(|(_, value)| value.as_str() == Some("loopback-ready"))
    });
    assert!(
        network
            .targets
            .lock()
            .unwrap()
            .contains(&network.http.to_string()),
        "WebKit bypassed the proxy for loopback"
    );
    assert!(
        network.errors.lock().unwrap().is_empty(),
        "fixture errors: {:?}",
        network.errors.lock().unwrap()
    );

    let accepts = network.accepts.load(Ordering::SeqCst);
    network.server.close(0_u32.into(), b"P0 disconnect");
    runtime.block_on(async {
        let connection = network.tunnel.get().await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), connection.closed())
            .await
            .unwrap();
        assert!(connection.close_reason().is_some());
    });
    network.requests.lock().unwrap().clear();
    for (scheme, address) in [("http", network.http), ("https", network.https)] {
        load(&app, format!("{scheme}://{address}/no-tunnel"));
        wait_for("strict failure after QUIC disconnect", || {
            EVENTS
                .lock()
                .unwrap()
                .iter()
                .any(|(name, _)| name == "loaded" || name == "error")
        });
        assert!(
            network.requests.lock().unwrap().is_empty(),
            "strict mode connected directly"
        );
        assert_eq!(
            network.accepts.load(Ordering::SeqCst),
            accepts,
            "strict mode opened an origin TCP connection"
        );
    }
    runtime.block_on(network.tunnel.set(None));

    network.proxy_task.abort();
    let _ = runtime.block_on(&mut network.proxy_task);
    load(&app, format!("http://{}/no-proxy", network.http));
    wait_for("proxy failure", || {
        EVENTS
            .lock()
            .unwrap()
            .iter()
            .any(|(name, _)| name == "error")
    });
    assert!(
        network.requests.lock().unwrap().is_empty(),
        "WebKit connected directly after proxy failure"
    );
    assert_eq!(
        network.accepts.load(Ordering::SeqCst),
        accepts,
        "WebKit opened a direct TCP connection"
    );
    window.close();
    network.client.close(0_u32.into(), b"P0 done");
    println!(
        "HTTP, HTTPS, redirects, CSS, images, script, fetch and WS/WSS traversed QUIC; invalid URL, loopback and strict failure checks passed"
    );
    Ok(())
}
