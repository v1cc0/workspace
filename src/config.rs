use anyhow::Context;
use clap::ValueEnum;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize, Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum WebviewEnv {
    #[default]
    Safe,
    #[value(alias = "x11")]
    Minimal,
    #[value(aliases = ["0", "false", "none"])]
    Off,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub backend: String,
    pub tunnel_strict: bool,
    pub webview_env: WebviewEnv,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backend: "127.0.0.1".to_string(),
            tunnel_strict: false,
            webview_env: WebviewEnv::Safe,
        }
    }
}

impl Config {
    pub fn backend_addr(&self) -> anyhow::Result<SocketAddr> {
        let backend = self.backend.trim();
        let addr = match backend.parse::<IpAddr>() {
            Ok(ip) => SocketAddr::new(ip, 3000),
            Err(_) => backend.parse::<SocketAddr>().context(
                "Backend must be an IP address with an optional port, e.g. 127.0.0.1:3000 or [::1]:3000",
            )?,
        };
        anyhow::ensure!(addr.port() != 0, "Backend port must be between 1 and 65535");
        Ok(addr)
    }

    pub fn with_overrides(
        &self,
        cli_strict: bool,
        cli_webview: Option<WebviewEnv>,
        env_strict: Option<&str>,
        env_webview: Option<&str>,
    ) -> anyhow::Result<Self> {
        let mut config = self.clone();
        config.tunnel_strict = cli_strict
            || env_strict.map_or(self.tunnel_strict, |value| {
                matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
            });
        config.webview_env = match (cli_webview, env_webview) {
            (Some(mode), _) => mode,
            (None, Some(value)) => WebviewEnv::from_str(value, true)
                .map_err(|e| anyhow::anyhow!("Invalid WORKSPACE_WEBVIEW_ENV: {e}"))?,
            (None, None) => self.webview_env,
        };
        Ok(config)
    }
}

pub fn config_path() -> anyhow::Result<PathBuf> {
    ProjectDirs::from("com", "workspace", "workspace")
        .map(|dirs| dirs.config_dir().join("config.toml"))
        .context("Could not locate the user configuration directory")
}

pub fn load_config() -> anyhow::Result<Config> {
    load_config_from(&config_path()?)
}

fn load_config_from(path: &Path) -> anyhow::Result<Config> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(e).with_context(|| format!("Could not read {}", path.display())),
    };
    let config: Config = toml::from_str(&content)
        .with_context(|| format!("Invalid configuration in {}", path.display()))?;
    config.backend_addr()?;
    Ok(config)
}

pub fn save_config(config: &Config) -> anyhow::Result<()> {
    save_config_to(config, &config_path()?)
}

fn save_config_to(config: &Config, path: &Path) -> anyhow::Result<()> {
    config.backend_addr()?;
    let content = toml::to_string_pretty(config)?;
    let parent = path.parent().context("Configuration path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(content.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path)
        .with_context(|| format!("Could not save {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_and_legacy_config_keep_defaults() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("settings/config.toml");
        assert_eq!(load_config_from(&path)?, Config::default());
        assert!(!path.parent().unwrap().exists());

        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, "backend = '192.0.2.1'\n")?;
        let config = load_config_from(&path)?;
        assert_eq!(config.backend, "192.0.2.1");
        assert!(!config.tunnel_strict);
        assert_eq!(config.webview_env, WebviewEnv::Safe);
        Ok(())
    }

    #[test]
    fn settings_round_trip_and_invalid_save_preserves_file() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("settings/config.toml");
        let mut config = Config {
            backend: "192.0.2.1:4443".to_string(),
            tunnel_strict: true,
            webview_env: WebviewEnv::Minimal,
        };
        save_config_to(&config, &path)?;
        assert_eq!(load_config_from(&path)?, config);

        config.webview_env = WebviewEnv::Off;
        save_config_to(&config, &path)?;
        assert_eq!(load_config_from(&path)?, config);
        let saved = fs::read(&path)?;
        config.backend = "https://example.com".to_string();
        assert!(save_config_to(&config, &path).is_err());
        assert_eq!(fs::read(&path)?, saved);
        assert_eq!(fs::read_dir(path.parent().unwrap())?.count(), 1);
        Ok(())
    }

    #[test]
    fn invalid_config_and_write_failure_are_reported() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("config.toml");
        for content in ["[", "webview_env = 'broken'", "backend = 'bad address'"] {
            fs::write(&path, content)?;
            assert!(load_config_from(&path).is_err());
        }
        assert!(save_config_to(&Config::default(), dir.path()).is_err());
        assert_eq!(fs::read_dir(dir.path())?.count(), 1);
        Ok(())
    }

    #[test]
    fn backend_address_validation() {
        for (input, expected) in [
            ("127.0.0.1", "127.0.0.1:3000"),
            (" 192.0.2.1:4443 ", "192.0.2.1:4443"),
            ("::1", "[::1]:3000"),
            ("[::1]:4443", "[::1]:4443"),
        ] {
            let config = Config {
                backend: input.into(),
                ..Config::default()
            };
            assert_eq!(config.backend_addr().unwrap().to_string(), expected);
        }
        for input in [
            "",
            "localhost",
            "https://127.0.0.1",
            "127.0.0.1:0",
            "127.0.0.1:65536",
        ] {
            let config = Config {
                backend: input.into(),
                ..Config::default()
            };
            assert!(config.backend_addr().is_err(), "accepted {input}");
        }
    }

    #[test]
    fn runtime_override_precedence() -> anyhow::Result<()> {
        let saved = Config {
            tunnel_strict: true,
            webview_env: WebviewEnv::Off,
            ..Config::default()
        };
        assert_eq!(saved.with_overrides(false, None, None, None)?, saved);
        let env = saved.with_overrides(false, None, Some("0"), Some("MINIMAL"))?;
        assert!(!env.tunnel_strict);
        assert_eq!(env.webview_env, WebviewEnv::Minimal);
        let cli = saved.with_overrides(true, Some(WebviewEnv::Safe), Some("0"), Some("invalid"))?;
        assert!(cli.tunnel_strict);
        assert_eq!(cli.webview_env, WebviewEnv::Safe);
        assert!(saved
            .with_overrides(false, None, None, Some("invalid"))
            .is_err());
        for (input, expected) in [("x11", WebviewEnv::Minimal), ("false", WebviewEnv::Off)] {
            assert_eq!(
                saved
                    .with_overrides(false, None, None, Some(input))?
                    .webview_env,
                expected
            );
        }
        assert_eq!(saved.webview_env, WebviewEnv::Off);
        Ok(())
    }
}
