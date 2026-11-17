use crate::backend::{LocalProxy, TunnelState};
use crate::quic_utils::{self, SkipServerVerification};
use quinn::{ClientConfig, Endpoint};
use rustls::ClientConfig as RustlsClientConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
#[cfg(target_os = "linux")]
use wry::WebViewBuilderExtUnix;
use wry::{ProxyConfig, ProxyEndpoint, WebViewBuilder}; // Import Unix extension trait

#[cfg(target_os = "linux")]
use gtk::prelude::*;
#[cfg(target_os = "linux")]
use gtk::{Box as GtkBox, Entry, Orientation};

const LOCAL_PROXY_HOST: &str = "127.0.0.1";
const LOCAL_PROXY_PORT: u16 = 8080;
const TUNNEL_RECONNECT_DELAY: Duration = Duration::from_secs(2);

pub async fn run_background_tasks(
    server_addr: SocketAddr,
    tunnel: TunnelState,
    tunnel_strict: bool,
) {
    let proxy =
        LocalProxy::new(LOCAL_PROXY_PORT, tunnel.clone()).with_direct_fallback(!tunnel_strict);

    tokio::spawn(async move {
        if let Err(e) = proxy.run().await {
            log::error!("Local proxy failed: {e}");
        }
    });

    tokio::spawn(async move {
        if let Err(e) = maintain_tunnel(server_addr, tunnel).await {
            log::error!("Tunnel task exited: {e}");
        }
    });

    // Keep runtime alive
    futures::future::pending::<()>().await;
}

pub fn start_browser_window(url: &str) -> anyhow::Result<()> {
    // UI (Main Thread)
    #[cfg(target_os = "linux")]
    {
        let initial_url = if url.is_empty() {
            "https://www.google.com"
        } else {
            url
        };
        log::info!(
            "Opening Browser Window for {} in existing GTK context...",
            initial_url
        );

        // No need for Application::new, Dioxus already initialized GTK
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("Workspace Browser");
        window.set_default_size(800, 600);

        let vbox = GtkBox::new(Orientation::Vertical, 0);
        window.add(&vbox);

        let address_bar = Entry::new();
        address_bar.set_text(initial_url);
        address_bar.set_placeholder_text(Some("Enter URL (e.g. https://example.com)"));
        vbox.pack_start(&address_bar, false, false, 0);

        let webview_container = GtkBox::new(Orientation::Vertical, 0);
        vbox.pack_start(&webview_container, true, true, 0);

        let webview = WebViewBuilder::new()
            .with_url(initial_url)
            .with_proxy_config(ProxyConfig::Http(ProxyEndpoint {
                host: LOCAL_PROXY_HOST.to_string(),
                port: LOCAL_PROXY_PORT.to_string(),
            }))
            .with_devtools(true)
            .build_gtk(&webview_container) // Wry 0.53 style for GTK
            .expect("Failed to initialize WebView");

        let webview = std::rc::Rc::new(webview);
        let webview_for_nav = webview.clone();
        address_bar.connect_activate(move |entry| {
            let mut url = entry.text().to_string();
            url = url.trim().to_string();
            if url.is_empty() {
                return;
            }
            if !url.starts_with("http://") && !url.starts_with("https://") {
                url = format!("https://{}", url);
            }
            if let Err(e) = webview_for_nav.load_url(&url) {
                log::warn!("navigate failed: {e}");
            }
        });

        unsafe {
            gtk::glib::object::ObjectExt::set_data(&window, "wry-webview", webview);
        }

        window.show_all();
        log::info!("Browser Window shown.");
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        log::warn!("Legacy browser window is currently only supported on Linux in this version.");
        Ok(())
    }
}

async fn maintain_tunnel(server_addr: SocketAddr, tunnel: TunnelState) -> anyhow::Result<()> {
    let mut rustls_config = RustlsClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
    rustls_config
        .dangerous()
        .set_certificate_verifier(Arc::new(SkipServerVerification));

    let quic_client_config = quinn::crypto::rustls::QuicClientConfig::try_from(rustls_config)?;
    let mut client_config = ClientConfig::new(Arc::new(quic_client_config));
    client_config.transport_config(Arc::new(quic_utils::configure_transport()));

    let bind_addr = if server_addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let mut endpoint = Endpoint::client(bind_addr.parse()?)?;
    endpoint.set_default_client_config(client_config);

    loop {
        log::info!("Connecting to Tunnel Server at {}", server_addr);
        match endpoint.connect(server_addr, "localhost") {
            Ok(connecting) => match connecting.await {
                Ok(connection) => {
                    log::info!("Tunnel Connected!");
                    tunnel.set(Some(connection.clone())).await;

                    let close = connection.closed().await;
                    log::warn!("Tunnel closed: {close}");
                    tunnel.set(None).await;
                }
                Err(e) => {
                    log::warn!("Tunnel connect failed: {e}");
                    tunnel.set(None).await;
                }
            },
            Err(e) => {
                log::warn!("Tunnel connect failed: {e}");
                tunnel.set(None).await;
            }
        }

        tokio::time::sleep(TUNNEL_RECONNECT_DELAY).await;
    }
}
