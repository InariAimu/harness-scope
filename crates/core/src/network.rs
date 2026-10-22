//! One device-local outbound policy for sync, provider APIs, and public catalog downloads.
use crate::{store::Store, Engine};
use anyhow::{Context, Result};
use hyper_util::client::proxy::matcher::Matcher;
use reqwest::{blocking::Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{io::Read, process::Command, time::Duration};

const META: &str = "network_settings";
const LOOPBACK: &str = "localhost,127.0.0.0/8,::1";
pub const MODELS_DEV_URL: &str = "https://models.dev/api.json";

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    System,
    Manual,
    Direct,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub mode: Mode,
    pub proxy_url: String,
}

pub fn load(store: &Store) -> Result<Settings> {
    Ok(store.meta(META)?.unwrap_or_default())
}

impl Settings {
    pub fn validate(&mut self) -> Result<()> {
        self.proxy_url = self.proxy_url.trim().to_owned();
        if self.mode != Mode::Manual {
            // An inactive draft must not block switching away from manual mode,
            // or persist credentials accidentally pasted into a hidden field.
            self.proxy_url.clear();
            return Ok(());
        }
        anyhow::ensure!(
            self.proxy_url.len() <= 2048 && !self.proxy_url.contains(['\r', '\n', '\0']),
            "Invalid proxy address"
        );
        let url = Url::parse(&self.proxy_url).map_err(|_| {
            anyhow::anyhow!(
                "Enter a proxy URL including http://, https://, socks5:// or socks5h://"
            )
        })?;
        anyhow::ensure!(
            matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h")
                && url.host_str().is_some(),
            "Unsupported proxy URL"
        );
        anyhow::ensure!(url.username().is_empty() && url.password().is_none(), "Proxy credentials in the address are not supported; use an unauthenticated local proxy");
        anyhow::ensure!(
            matches!(url.path(), "" | "/") && url.query().is_none() && url.fragment().is_none(),
            "Proxy address must contain only a scheme, host and port"
        );
        self.proxy_url = url.to_string();
        Ok(())
    }

    pub fn client(&self) -> Result<Client> {
        let mut settings = self.clone();
        settings.validate()?;
        let builder = Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none());
        let builder = match settings.mode {
            Mode::System => builder,
            Mode::Direct => builder.no_proxy(),
            Mode::Manual => builder.no_proxy().proxy(
                reqwest::Proxy::all(&settings.proxy_url)?
                    .no_proxy(reqwest::NoProxy::from_string(LOOPBACK)),
            ),
        };
        builder
            .build()
            .context("Cannot initialize outbound connection")
    }

    fn route(&self, target: &str) -> Result<Option<String>> {
        let matcher = match self.mode {
            Mode::System => Matcher::from_system(),
            Mode::Direct => return Ok(None),
            Mode::Manual => Matcher::builder().all(&self.proxy_url).no(LOOPBACK).build(),
        };
        let uri = target.parse().context("Invalid network target")?;
        Ok(matcher.intercept(&uri).map(|proxy| proxy.uri().to_string()))
    }

    /// Configure only this helper's environment; never change the user's global proxy.
    pub fn configure_command(&self, command: &mut Command) -> Result<()> {
        let mut settings = self.clone();
        settings.validate()?;
        match settings.mode {
            Mode::System => {
                // Older Codex builds may only read environment proxies. Bridge the
                // Windows manual proxy for quota requests when no env proxy is set.
                for (scheme, key) in [("http", "HTTP_PROXY"), ("https", "HTTPS_PROXY")] {
                    let inherited = [
                        key.to_owned(),
                        key.to_ascii_lowercase(),
                        "ALL_PROXY".into(),
                        "all_proxy".into(),
                    ]
                    .iter()
                    .any(|key| std::env::var(key).is_ok_and(|v| !v.is_empty()));
                    if !inherited {
                        if let Some(proxy) = settings.route(&format!("{scheme}://chatgpt.com/"))? {
                            command
                                .env(key, &proxy)
                                .env(key.to_ascii_lowercase(), proxy);
                        }
                    }
                }
            }
            Mode::Manual | Mode::Direct => {
                for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"] {
                    command.env_remove(key).env_remove(key.to_ascii_lowercase());
                }
                let (proxy, bypass) = if settings.mode == Mode::Manual {
                    (settings.proxy_url.as_str(), LOOPBACK)
                } else {
                    ("", "*")
                };
                for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
                    command.env(key, proxy).env(key.to_ascii_lowercase(), proxy);
                }
                command.env("NO_PROXY", bypass).env("no_proxy", bypass);
            }
        }
        Ok(())
    }
}

fn view(engine: &Engine, settings: Settings) -> Result<Value> {
    let server = engine.read_lock()?.config()?.server_url;
    let targets = [
        ("sync", server.as_str()),
        ("modelsDev", MODELS_DEV_URL),
        ("deepseek", "https://api.deepseek.com/user/balance"),
        ("opencodeGo", "https://opencode.ai/inference/go/v1/usage"),
        ("codex", "https://chatgpt.com/"),
    ];
    let routes: Vec<Value> = targets
        .into_iter()
        .map(|(id, target)| {
            let configured = !target.is_empty();
            // Matcher strips credentials; do not return raw environment variables.
            let route = configured.then(|| settings.route(target)).transpose();
            let invalid = route.is_err();
            let proxy = route.ok().flatten().flatten();
            json!({"id":id,"configured":configured,"proxy":proxy,"invalid":invalid})
        })
        .collect();
    Ok(json!({"settings":settings,"routes":routes}))
}

pub fn execute(engine: &Engine, name: &str, args: Value) -> Result<Value> {
    match name {
        "network_settings" => {
            let settings = load(&*engine.read_lock()?)?;
            view(engine, settings)
        }
        "save_network_settings" => {
            let mut settings: Settings = serde_json::from_value(args["settings"].clone())?;
            settings.validate()?;
            engine.lock()?.set_meta(META, &settings)?;
            view(engine, settings)
        }
        "test_network_settings" => {
            let mut settings: Settings = serde_json::from_value(args["settings"].clone())?;
            settings.validate()?;
            let started = std::time::Instant::now();
            settings
                .client()?
                .head(MODELS_DEV_URL)
                .send()
                .context("Network test failed; check proxy settings")?
                .error_for_status()?;
            Ok(json!({"elapsedMs":started.elapsed().as_millis()}))
        }
        "fetch_models_dev" => {
            let settings = load(&*engine.read_lock()?)?;
            let response = settings
                .client()?
                .get(MODELS_DEV_URL)
                .send()
                .context("Cannot download models.dev; check network settings")?
                .error_for_status()?;
            let mut bytes = Vec::new();
            response
                .take(16 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() <= 16 * 1024 * 1024,
                "models.dev response is too large"
            );
            let value: Value =
                serde_json::from_slice(&bytes).context("Invalid models.dev response")?;
            anyhow::ensure!(value.is_object(), "Invalid models.dev catalog");
            Ok(value)
        }
        _ => anyhow::bail!("Unknown network command"),
    }
}
