mod backend;
mod client;
mod config;
mod quic_utils;
mod server;
mod ui;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Run as backend server
    #[arg(long)]
    server: bool,

    /// Server port
    #[arg(long, default_value_t = 3000)]
    port: u16,

    /// Client: disable direct-connect fallback when tunnel is down
    #[arg(long)]
    tunnel_strict: bool,

    /// Client (Linux): WebKitGTK env overrides: safe|minimal|off
    #[arg(long, value_enum, ignore_case = true)]
    webview_env: Option<config::WebviewEnv>,
}

fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let version = env!("CARGO_PKG_VERSION");
    let git_hash = env!("GIT_HASH");
    log::info!("Workspace v{} ({}) starting...", version, git_hash);

    let args = Args::parse();

    if args.server {
        // Run as Backend Server (Async Tokio)
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        rt.block_on(server::run_server(args.port))?;
    } else {
        let saved_config = config::load_config()?;
        let runtime_config = saved_config.with_overrides(
            args.tunnel_strict,
            args.webview_env,
            std::env::var("WORKSPACE_TUNNEL_STRICT").ok().as_deref(),
            std::env::var("WORKSPACE_WEBVIEW_ENV").ok().as_deref(),
        )?;

        #[cfg(target_os = "linux")]
        configure_linux_webview_env(runtime_config.webview_env);

        ui::start_workspace(saved_config, runtime_config)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webview_cli_validates_modes_and_preserves_aliases() {
        for (value, expected) in [
            ("SAFE", config::WebviewEnv::Safe),
            ("minimal", config::WebviewEnv::Minimal),
            ("x11", config::WebviewEnv::Minimal),
            ("off", config::WebviewEnv::Off),
            ("0", config::WebviewEnv::Off),
        ] {
            let args = Args::try_parse_from(["workspace", "--webview-env", value]).unwrap();
            assert_eq!(args.webview_env, Some(expected));
        }
        assert!(Args::try_parse_from(["workspace", "--webview-env", "invalid"]).is_err());
    }
}

#[cfg(target_os = "linux")]
fn configure_linux_webview_env(mode: config::WebviewEnv) {
    if mode == config::WebviewEnv::Off {
        log::info!("WebView env overrides: disabled");
        return;
    }

    set_env_default("GDK_BACKEND", "x11");

    if mode == config::WebviewEnv::Minimal {
        log::info!("WebView env overrides: minimal (GDK_BACKEND only)");
        return;
    }

    log::info!("WebView env overrides: safe");
    set_env_default("LIBGL_ALWAYS_SOFTWARE", "1");
    set_env_default("GDK_GL", "disable");
    set_env_default("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    set_env_default("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    set_env_default("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");
}

#[cfg(target_os = "linux")]
fn set_env_default(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        std::env::set_var(key, value);
    }
}
