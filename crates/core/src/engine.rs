use crate::{
    harness::{files, Registry, TimelineIndex},
    migration,
    model::*,
    pricing, query,
    scan_cache::{Checkpoint, Stamp},
    store::Store,
    sync,
};
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::{hash_map::Entry, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::Duration,
};

fn validated_device_name(value: &str) -> Result<String> {
    let name = value.trim();
    anyhow::ensure!(!name.is_empty(), "Device name cannot be empty");
    anyhow::ensure!(name.len() <= 128, "Device name too long");
    anyhow::ensure!(
        !name.chars().any(char::is_control),
        "Device name cannot contain control characters"
    );
    Ok(name.to_owned())
}

pub struct Engine {
    pub store: Mutex<Store>,
    pub registry: Registry,
    scan_lock: Mutex<()>,
    sync_lock: Mutex<()>,
    database_path: PathBuf,
    reader: crate::sessions::ReaderCache,
    query_reader: Mutex<Store>,
    background_wake: Arc<(Mutex<u64>, Condvar)>,
    pub(crate) account_vault: crate::account_credentials::Vault,
    pub(crate) account_polling: Arc<crate::accounts::Polling>,
}
impl Engine {
    pub fn open(path: &Path) -> Result<Self> {
        let store = Store::open(path)?;
        let registry = Registry::default();
        let mut config = store.config()?;
        if config.sources.is_empty() {
            for descriptor in registry.descriptors() {
                for (i, root) in descriptor.default_roots.iter().enumerate() {
                    config.sources.push(Source {
                        id: format!("{}-{i}", descriptor.id),
                        harness: descriptor.id.into(),
                        root: root.clone(),
                        enabled: Path::new(root).exists(),
                    })
                }
            }
            store.set_meta("config", &config)?
        }
        Ok(Self {
            account_vault: crate::account_credentials::Vault::new(path),
            account_polling: Default::default(),
            query_reader: Mutex::new(Store::reader(path)?),
            background_wake: Arc::new((Mutex::new(0), Condvar::new())),
            store: Mutex::new(store),
            registry,
            scan_lock: Mutex::new(()),
            sync_lock: Mutex::new(()),
            database_path: path.to_path_buf(),
            reader: Default::default(),
        })
    }
    pub fn lock(&self) -> Result<MutexGuard<'_, Store>> {
        self.store
            .lock()
            .map_err(|_| anyhow::anyhow!("Database lock poisoned"))
    }
    pub(crate) fn read_lock(&self) -> Result<MutexGuard<'_, Store>> {
        self.query_reader
            .lock()
            .map_err(|_| anyhow::anyhow!("Query lock poisoned"))
    }
    fn wake_collection(&self) {
        let (epoch, wake) = &*self.background_wake;
        if let Ok(mut epoch) = epoch.lock() {
            *epoch = epoch.wrapping_add(1);
            wake.notify_all();
        }
    }
    pub fn execute(&self, name: &str, args: Value) -> Result<Value> {
        match name {
            "icon_library" | "set_entity_icon" | "import_entity_icon" | "delete_icon_asset" => {
                crate::icons::execute(self, name, args)
            }
            "network_settings"
            | "save_network_settings"
            | "test_network_settings"
            | "fetch_models_dev" => crate::network::execute(self, name, args),
            "provider_accounts"
            | "shared_subscriptions"
            | "provider_sidebar_order"
            | "save_provider_sidebar_order"
            | "provider_account_history"
            | "provider_account_estimate"
            | "provider_account_test_query"
            | "provider_session_sources"
            | "provider_account_routes"
            | "provider_account_save"
            | "provider_account_delete"
            | "provider_account_refresh"
            | "provider_account_activity"
            | "go_usage_estimate"
            | "go_forecast"
            | "codex_subscription_estimate"
            | "go_simulate"
            | "provider_account_usage" => {
                crate::accounts::execute(self, &self.account_vault, name, args)
            }
            "snapshot" => {
                let s = self.read_lock()?;
                let mut config = serde_json::to_value(s.config()?)?;
                let application_settings = crate::applications::load(&s)?;
                let token = !s.config()?.server_token.is_empty();
                config["serverToken"] = json!("");
                Ok(
                    json!({"appBuild":crate::build_info::current(),"syncProtocolVersion":4,"deviceId":s.device_id()?,"config":config,"applicationSettings":application_settings,"usageVersion":s.usage_version()?,"localSchemaVersion":s.schema_version()?,"remoteSchemaVersion":s.meta::<Option<u32>>("remote_schema_version")?.flatten(),"remoteSchemaCheckedAt":s.meta::<Option<i64>>("remote_schema_checked_at")?.flatten(),"tokenConfigured":token,"harnesses":self.registry.descriptors(),"pending":s.pending_count()?,"devices":s.meta::<Vec<Value>>("devices")?.unwrap_or_default(),"lastScanAt":s.meta::<i64>("last_scan_at")?,"lastScan":s.meta::<ScanReport>("last_scan")?,"lastSyncAt":s.meta::<i64>("last_sync_at")?,"lastSyncError":s.meta::<Option<String>>("last_sync_error")?.flatten(),"migrations":s.meta::<Vec<Value>>("migration_manifests")?.unwrap_or_default()}),
                )
            }
            "save_application_settings" => {
                let harness = args["harness"].as_str().context("Missing harness")?;
                self.registry.get(harness)?;
                let patch: crate::applications::Patch =
                    serde_json::from_value(args["patch"].clone())?;
                anyhow::ensure!(
                    patch.collection_enabled.is_some() || patch.visible.is_some(),
                    "Missing application settings"
                );
                // Keep deletion's collect-before-remove sequence atomic with respect
                // to this gate; an in-progress scan finishes before collection changes.
                let _scan = if patch.collection_enabled.is_some() {
                    Some(
                        self.scan_lock
                            .lock()
                            .map_err(|_| anyhow::anyhow!("Scan lock poisoned"))?,
                    )
                } else {
                    None
                };
                let s = self.lock()?;
                let mut settings = crate::applications::load(&s)?;
                let current = settings.entry(harness.to_owned()).or_default();
                let previous = *current;
                if let Some(enabled) = patch.collection_enabled {
                    current.collection_enabled = enabled;
                }
                if let Some(visible) = patch.visible {
                    current.visible = visible;
                }
                let updated = *current;
                s.set_meta("application_settings", &settings)?;
                if previous.collection_enabled != updated.collection_enabled {
                    self.wake_collection();
                }
                Ok(json!(updated))
            }
            "rename_device" => {
                let name =
                    validated_device_name(args["name"].as_str().context("Missing device name")?)?;
                let s = self.lock()?;
                let mut config = s.config()?;
                config.device_name = name.clone();
                s.set_meta("config", &config)?;
                Ok(json!(name))
            }
            "save_config" => {
                let mut c: Config = serde_json::from_value(args["config"].clone())?;
                let accent = c.accent_color.as_bytes();
                anyhow::ensure!(
                    accent.len() == 7
                        && accent[0] == b'#'
                        && accent[1..].iter().all(u8::is_ascii_hexdigit),
                    "Accent color must be a six-digit HEX color (#RRGGBB)"
                );
                c.accent_color.make_ascii_lowercase();
                anyhow::ensure!(
                    matches!(
                        c.interface_skin.as_str(),
                        "none" | "deepseek" | "claude" | "gpt" | "gemini" | "grok"
                    ),
                    "Unsupported interface skin"
                );
                anyhow::ensure!(
                    c.skin_transparency <= 100 && c.skin_mask_strength <= 100,
                    "Skin transparency and veil strength must be between 0 and 100"
                );
                c.timezone.parse::<chrono_tz::Tz>()?;
                anyhow::ensure!(
                    c.scan_interval_seconds >= 5 && c.scan_interval_seconds <= 86400,
                    "Scan interval must be 5–86400 seconds"
                );
                c.device_name = validated_device_name(&c.device_name)?;
                let mut ids = HashSet::new();
                for source in &c.sources {
                    self.registry.get(&source.harness)?;
                    anyhow::ensure!(
                        Path::new(&source.root).is_absolute(),
                        "Log root must be absolute"
                    );
                    anyhow::ensure!(ids.insert(source.id.clone()), "Duplicate source ID");
                }
                let s = self.lock()?;
                let old = s.config()?;
                let collection_changed = old.scan_interval_seconds != c.scan_interval_seconds
                    || old
                        .sources
                        .iter()
                        .map(|s| (&s.id, &s.harness, &s.root, s.enabled))
                        .ne(c
                            .sources
                            .iter()
                            .map(|s| (&s.id, &s.harness, &s.root, s.enabled)));
                if c.server_token.is_empty() && !args["clearToken"].as_bool().unwrap_or(false) {
                    c.server_token = old.server_token
                }
                if old.server_url != c.server_url {
                    s.transaction(|| {
                        s.conn.execute(
                            "DELETE FROM record_timings WHERE device_id<>?1",
                            [s.device_id()?],
                        )?;
                        s.conn.execute("DELETE FROM remote_records", [])?;
                        s.set_meta("download_cursor", &0i64)?;
                        s.set_meta("remote_instance_id", &String::new())?;
                        s.set_meta("remote_schema_version", &Option::<u32>::None)?;
                        s.set_meta("remote_schema_checked_at", &Option::<i64>::None)?;
                        s.requeue_all()
                    })?
                }
                s.set_meta("config", &c)?;
                if collection_changed {
                    self.wake_collection();
                }
                Ok(json!(true))
            }
            "scan" => Ok(serde_json::to_value(self.scan(false)?)?),
            "rescan" => Ok(serde_json::to_value(self.scan(true)?)?),
            "usage_query" => {
                let s = self.read_lock()?;
                s.transaction(|| {
                    query::query(
                        &s,
                        args["kind"].as_str().context("Missing query kind")?,
                        &args,
                    )
                })
            }
            "last_scan" => Ok(json!(self.read_lock()?.meta::<i64>("last_scan_at")?)),
            "usage_display" => Ok(json!(self
                .read_lock()?
                .meta::<UsageDisplaySettings>("usage_display")?
                .unwrap_or_default())),
            "save_usage_display" => {
                let patch: UsageDisplaySettings = serde_json::from_value(args["patch"].clone())?;
                let s = self.lock()?;
                let mut display = s
                    .meta::<UsageDisplaySettings>("usage_display")?
                    .unwrap_or_default();
                display.apply(patch)?;
                s.set_meta("usage_display", &display)?;
                Ok(json!(display))
            }
            "prices" => Ok(json!(self.read_lock()?.prices()?)),
            "codex_allowance_catalog" => crate::accounts::codex::catalog::info(
                &*self.read_lock()?,
                &self.database_path.with_extension("codex-allowances.json"),
            ),
            "save_codex_allowance_catalog" => crate::accounts::codex::catalog::save(
                &*self.lock()?,
                &self.database_path.with_extension("codex-allowances.json"),
                args,
            ),
            "go_catalog" => pricing::go_catalog::info(
                &*self.read_lock()?,
                &self.database_path.with_extension("opencode-go.json"),
            ),
            "save_go_catalog" => pricing::go_catalog::save(
                &*self.lock()?,
                &self.database_path.with_extension("opencode-go.json"),
                args,
            ),
            "models_needing_prices" => {
                let s = self.read_lock()?;
                let prices = s.prices()?;
                let mut stmt=s.conn.prepare_cached("SELECT model,provider,MAX(amount_usd IS NULL),harness,MAX(input_tokens+output_tokens+cache_read+cache_write)>0 FROM usage_index WHERE origin='local' AND deleted=0 AND record_source='session' AND cost_method<>'reported' AND (harness NOT IN ('copliot-deprecated','copliot') OR input_tokens+output_tokens+cache_read+cache_write>0) GROUP BY model,provider,harness")?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, bool>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, bool>(4)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                let models: std::collections::BTreeSet<_> = rows
                    .into_iter()
                    .filter(|(_, _, _, harness, has_tokens)| {
                        self.permits_price_estimates(harness, *has_tokens)
                    })
                    .filter(|(model, provider, unknown, _, _)| {
                        *unknown
                            || pricing::find_for_provider(&prices, model, provider.as_deref())
                                .is_none()
                    })
                    .map(|(model, _, _, _, _)| model)
                    .collect();
                Ok(json!(models))
            }
            "pricing_settings" => Ok(json!(self.read_lock()?.pricing_settings()?)),
            "save_pricing_settings" => {
                let patch: PricingSettings = serde_json::from_value(args["patch"].clone())?;
                pricing::validate_settings(&patch)?;
                let s = self.lock()?;
                s.transaction(|| {
                    let mut settings = s.pricing_settings()?;
                    settings.exchange_rates.extend(patch.exchange_rates);
                    settings.holiday_calendars.extend(patch.holiday_calendars);
                    settings.updated_at = chrono::Utc::now().timestamp();
                    pricing::validate_settings(&settings)?;
                    s.set_meta("pricing_settings", &settings)?;
                    self.reprice(&s, false, None, None)?;
                    Ok(json!(settings))
                })
            }
            "reprice_usage" => {
                let s = self.lock()?;
                s.transaction(|| {
                    Ok(json!(self.reprice(
                        &s,
                        true,
                        args["model"].as_str(),
                        args["harness"].as_str()
                    )?))
                })
            }
            "save_prices" => self.save_prices(args, false),
            "save_prices_detailed" => self.save_prices(args, true),
            "delete_price" => {
                let s = self.lock()?;
                let mut prices = s.prices()?;
                let provider = args["provider"].as_str().map(crate::providers::pricing_id);
                anyhow::ensure!(
                    provider != Some("opencode-go")
                        || pricing::go_catalog::find(
                            &pricing::go_catalog::load(&s)?,
                            args["modelId"].as_str().unwrap_or("")
                        )
                        .is_none(),
                    "OpenCode Go pricing is managed by local Go rules"
                );
                anyhow::ensure!(
                    !(pricing::deepseek::canonical(args["modelId"].as_str().unwrap_or(""))
                        .is_some()
                        && matches!(provider, Some("deepseek-official" | "opencode-go"))),
                    "Built-in provider pricing cannot be deleted"
                );
                prices.retain(|p| {
                    p.model_id != args["modelId"].as_str().unwrap_or("")
                        || p.provider.as_deref() != provider
                });
                s.set_meta("prices", &prices)?;
                Ok(json!(true))
            }
            "models_config" => {
                let s = self.read_lock()?;
                Ok(
                    json!({"config":s.config()?.models_dev_sync,"configPath":self.database_path.canonicalize()?.to_string_lossy()}),
                )
            }
            "save_models_config" => {
                let s = self.lock()?;
                let mut c = s.config()?;
                c.models_dev_sync = args["config"].clone();
                s.set_meta("config", &c)?;
                Ok(json!({"config":c.models_dev_sync,"configPath":"harness-scope.db"}))
            }
            "record_models_sync" => {
                let s = self.lock()?;
                let mut c = s.config()?;
                if !args["syncedAt"].is_null() {
                    c.models_dev_sync["lastSyncAt"] = args["syncedAt"].clone()
                }
                c.models_dev_sync["lastSyncError"] = args["error"].clone();
                s.set_meta("config", &c)?;
                Ok(Value::Null)
            }
            "sessions" => Ok(json!(self.sessions()?)),
            "session_messages" | "session_transcript" | "session_block" => {
                let path = self.session_path(&args)?;
                let adapter = self
                    .registry
                    .get(args["providerId"].as_str().context("Missing harness")?)?;
                let session = if name == "session_block" {
                    args["reference"]["sessionId"].as_str()
                } else {
                    args["sessionId"].as_str()
                };
                let snapshot = if name == "session_block" {
                    args["reference"]["fingerprint"].as_str()
                } else {
                    args["fingerprint"].as_str()
                };
                let loaded = self.reader.load(adapter, &path, session, snapshot)?;
                if name == "session_messages" {
                    return Ok(json!(loaded.transcript.restored()));
                }
                let offset: usize = args
                    .get("offset")
                    .map(|v| v.as_u64().context("Invalid content offset"))
                    .transpose()?
                    .unwrap_or(0)
                    .try_into()?;
                if name == "session_block" {
                    let limit = args
                        .get("limit")
                        .map(|v| v.as_u64().context("Invalid content limit"))
                        .transpose()?
                        .unwrap_or(64 * 1024)
                        .min(512 * 1024) as usize;
                    return loaded.transcript.block(&args["reference"], offset, limit);
                }
                if let Some(fingerprint) = args.get("fingerprint") {
                    anyhow::ensure!(
                        fingerprint == &loaded.transcript.fingerprint,
                        "Session changed; reload transcript"
                    );
                }
                loaded
                    .transcript
                    .page(offset, loaded.cached, loaded.parse_ms)
            }
            "delete_session" => self.delete_session(&args),
            "open_data_folder" => self.execute(
                "reveal_session_path",
                json!({"path":self.database_path.canonicalize()?.to_string_lossy()}),
            ),
            "session_image" => {
                let path = self.session_path(&args)?;
                let reference = &args["image"]["source"]["content"];
                anyhow::ensure!(
                    reference["kind"] == "jsonl",
                    "Only inline JSONL images are supported"
                );
                let bytes = files::read_bytes(&path)?;
                let offset = reference["offset"]
                    .as_u64()
                    .context("Invalid image offset")? as usize;
                let len = reference["len"].as_u64().context("Invalid image length")? as usize;
                let end = offset
                    .checked_add(len)
                    .context("Image reference overflow")?;
                let row: Value = serde_json::from_slice(
                    bytes.get(offset..end).context("Invalid image reference")?,
                )?;
                let pointer = reference["pointer"]
                    .as_str()
                    .context("Invalid image pointer")?;
                let image = row.pointer(pointer).context("Image not found")?;
                let url = if let Some(s) = image.as_str() {
                    s.to_owned()
                } else {
                    let mime = image["media_type"].as_str().context("Missing image type")?;
                    let data = image["data"].as_str().context("Missing image data")?;
                    format!("data:{mime};base64,{data}")
                };
                anyhow::ensure!(
                    url.len() <= 14 * 1024 * 1024
                        && [
                            "data:image/png;base64,",
                            "data:image/jpeg;base64,",
                            "data:image/webp;base64,",
                            "data:image/gif;base64,"
                        ]
                        .iter()
                        .any(|prefix| url.starts_with(prefix)),
                    "Unsupported image type or size"
                );
                Ok(json!(url))
            }
            "export_text" => {
                let path = Path::new(args["path"].as_str().context("Missing export path")?);
                anyhow::ensure!(
                    path.is_absolute() && path.extension().is_some_and(|s| s == "md"),
                    "Export requires an absolute .md path"
                );
                std::fs::write(
                    path,
                    args["content"].as_str().context("Missing export content")?,
                )?;
                Ok(json!(true))
            }
            "open_external" => {
                let url = reqwest::Url::parse(args["url"].as_str().context("Missing URL")?)?;
                anyhow::ensure!(
                    ["https", "http"].contains(&url.scheme()),
                    "Only HTTP(S) links can be opened"
                );
                #[cfg(target_os = "windows")]
                std::process::Command::new("rundll32.exe")
                    .arg("url.dll,FileProtocolHandler")
                    .arg(url.as_str())
                    .spawn()?;
                #[cfg(target_os = "macos")]
                std::process::Command::new("open")
                    .arg(url.as_str())
                    .spawn()?;
                #[cfg(target_os = "linux")]
                std::process::Command::new("xdg-open")
                    .arg(url.as_str())
                    .spawn()?;
                Ok(json!(true))
            }
            "reveal_session_path" => {
                let path = Path::new(args["path"].as_str().context("Missing path")?);
                anyhow::ensure!(path.is_absolute() && path.exists(), "Path does not exist");
                #[cfg(target_os = "windows")]
                std::process::Command::new("explorer.exe")
                    .arg(format!("/select,{}", path.display()))
                    .spawn()?;
                #[cfg(target_os = "macos")]
                std::process::Command::new("open")
                    .arg("-R")
                    .arg(path)
                    .spawn()?;
                #[cfg(target_os = "linux")]
                std::process::Command::new("xdg-open")
                    .arg(path.parent().unwrap_or(path))
                    .spawn()?;
                Ok(json!(true))
            }
            "migration_preview" => {
                let input: migration::MigrationInput = serde_json::from_value(args)?;
                Ok(migration::preview(&*self.lock()?, &input)?.value)
            }
            "migration_commit" => {
                let _scan = self
                    .scan_lock
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Scan lock poisoned"))?;
                let input = serde_json::from_value(args)?;
                migration::commit(&*self.lock()?, &input)
            }
            "sync" => self.sync(),
            _ => anyhow::bail!("Unknown command: {name}"),
        }
    }
    pub fn scan(&self, force: bool) -> Result<ScanReport> {
        let _guard = self
            .scan_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Scan lock poisoned"))?;
        self.scan_unlocked(force)
    }
    fn scan_unlocked(&self, force: bool) -> Result<ScanReport> {
        self.scan_selected(force, None)
    }
    /// Import one configured harness without scanning unrelated logs.
    pub fn scan_harness(&self, harness: &str, force: bool) -> Result<ScanReport> {
        self.registry.get(harness)?;
        let _guard = self
            .scan_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Scan lock poisoned"))?;
        self.scan_selected(force, Some(harness))
    }
    fn scan_selected(&self, force: bool, selected: Option<&str>) -> Result<ScanReport> {
        let config = self.lock()?.config()?;
        let settings = crate::applications::load(&*self.lock()?)?;
        let mut report = ScanReport::default();
        let mut inputs = Vec::new();
        let mut paths = HashSet::new();
        crate::scan_cache::migrate_contexts(&*self.lock()?)?;
        if selected.is_none_or(|h| h == "codex")
            && crate::applications::get(&settings, "codex").collection_enabled
        {
            crate::codex_scan::scan(self, force, &mut report)?;
        }
        let mut index = TimelineIndex::new();
        let mut unavailable = HashSet::new();
        for source in config.sources.iter().filter(|s| {
            s.enabled
                && crate::applications::get(&settings, &s.harness).collection_enabled
                && s.harness != "codex"
                && selected.is_none_or(|h| h == s.harness)
        }) {
            let adapter = self.registry.get(&source.harness)?;
            let found = match adapter.discover(Path::new(&source.root)) {
                Ok(xs) => xs,
                Err(e) => {
                    report.errors.push(e.to_string());
                    continue;
                }
            };
            for path in found {
                if !paths.insert(path.clone()) {
                    continue;
                }
                let key = path.to_string_lossy().to_string();
                let stamp = Stamp::read(&path).ok();
                let cached: Option<String> = self
                    .lock()?
                    .conn
                    .query_row(
                        "SELECT payload FROM usage_read_cache WHERE path=?",
                        [&key],
                        |r| r.get(0),
                    )
                    .optional()?;
                let cached = cached.and_then(|v| serde_json::from_str::<Checkpoint>(&v).ok());
                if let Some(cached) = cached
                    .as_ref()
                    .filter(|c| !force && stamp.as_ref().is_some_and(|s| c.matches(s, adapter)))
                {
                    inputs.push((
                        source.harness.clone(),
                        path,
                        None,
                        cached.fingerprint.clone(),
                        true,
                        cached.parent.clone(),
                        stamp,
                    ));
                    continue;
                }
                if let Some(id) = cached.and_then(|c| c.context_id) {
                    unavailable.insert(id);
                }
                match adapter.read_usage_source(&path) {
                    Ok((lines, fingerprint, errors, complete)) => {
                        let complete =
                            complete && stamp.is_some() && Stamp::read(&path).ok() == stamp;
                        if let Some(id) = adapter.context_id(&lines) {
                            if complete {
                                crate::scan_cache::save_context(
                                    &*self.lock()?,
                                    &id,
                                    &adapter.context_events(&lines),
                                )?;
                                unavailable.remove(&id);
                            } else {
                                unavailable.insert(id);
                            }
                        }
                        if complete {
                            let checkpoint = Checkpoint::new(
                                stamp.as_ref().unwrap().clone(),
                                adapter,
                                fingerprint.clone(),
                                &lines,
                            );
                            self.lock()?.conn.execute(
                                "INSERT INTO usage_read_cache(path,payload) VALUES(?1,?2) ON CONFLICT(path) DO UPDATE SET payload=excluded.payload",
                                params![key, serde_json::to_string(&checkpoint)?]
                            )?;
                        }
                        report.errors.extend(errors);
                        let parent = adapter.parent_context(&lines);
                        inputs.push((
                            source.harness.clone(),
                            path,
                            Some(lines),
                            fingerprint,
                            complete,
                            parent,
                            stamp,
                        ));
                    }
                    Err(e) => {
                        report.files_scanned += 1;
                        report.deferred_files += 1;
                        report.errors.push(format!("{}: {e}", path.display()));
                    }
                }
            }
        }
        for (harness, path, lines, fingerprint, read_complete, parent, stamp) in inputs {
            report.files_scanned += 1;
            let adapter = self.registry.get(&harness)?;
            let descriptor = adapter.descriptor();
            let key = path.to_string_lossy().to_string();
            let parent_fingerprint: Option<String> = if let Some(parent) = &parent {
                if unavailable.contains(parent) {
                    None
                } else {
                    self.lock()?
                        .conn
                        .query_row(
                            "SELECT fingerprint FROM usage_contexts WHERE id=?",
                            [parent],
                            |r| r.get(0),
                        )
                        .optional()?
                }
            } else {
                None
            };
            let fingerprint = if parent.is_some() {
                format!(
                    "{fingerprint}:{}",
                    parent_fingerprint.as_deref().unwrap_or("missing")
                )
            } else {
                fingerprint
            };
            let s = self.lock()?;
            let previous: Option<(String, String)> = s
                .conn
                .query_row(
                    "SELECT fingerprint,parser_version FROM file_state WHERE path=?",
                    [&key],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if !force
                && read_complete
                && previous == Some((fingerprint.clone(), descriptor.parser_version.into()))
            {
                report.skipped += 1;
                continue;
            }
            drop(s);
            if let Some(parent) = parent.filter(|_| parent_fingerprint.is_some()) {
                if let Entry::Vacant(entry) = index.entry(parent) {
                    let payload: String = self.lock()?.conn.query_row(
                        "SELECT payload FROM usage_contexts WHERE id=?",
                        [entry.key()],
                        |r| r.get(0),
                    )?;
                    entry.insert(serde_json::from_str(&payload)?);
                }
            }
            let lines = if let Some(lines) = lines {
                lines
            } else {
                match adapter.read_usage_source(&path) {
                    Ok((lines, _, errors, complete))
                        if complete && Stamp::read(&path).ok() == stamp =>
                    {
                        report.errors.extend(errors);
                        lines
                    }
                    Ok((_, _, errors, _)) => {
                        report.errors.extend(errors);
                        report.deferred_files += 1;
                        continue;
                    }
                    Err(e) => {
                        report.errors.push(format!("{key}: {e}"));
                        report.deferred_files += 1;
                        continue;
                    }
                }
            };
            let result = match adapter.parse(&path, &lines, &index) {
                Ok(r) => r,
                Err(e) => {
                    report.errors.push(format!("{key}: {e}"));
                    report.deferred_files += 1;
                    continue;
                }
            };
            report.errors.extend(result.diagnostics);
            let complete = read_complete && result.complete;
            if !complete {
                report.deferred_files += 1
            }
            let s = self.lock()?;
            let device = s.device_id()?;
            let prices = s.prices()?;
            let pricing_settings = s.pricing_settings()?;
            let manifests: Vec<Value> = s.meta("migration_manifests")?.unwrap_or_default();
            let cutoff = manifests
                .iter()
                .filter(|m| {
                    m["harnesses"]
                        .as_array()
                        .is_some_and(|hs| hs.iter().any(|h| h == &harness))
                })
                .filter_map(|m| m["cutoff"].as_i64())
                .max()
                .unwrap_or(0);
            s.transaction(||{
                let mut active=HashSet::new();
                for mut parsed in result.usage{
                    if parsed.occurred_at<cutoff{continue}
                    active.insert(parsed.id.clone());let old=s.record(&parsed.id)?;
                    if parsed.provider.is_none() { parsed.provider = old.as_ref().and_then(|r| r.provider.clone()); }
                    let metadata_upgrade=old.as_ref().filter(|r| {
                        let mut billing_tokens = parsed.tokens.clone();
                        if harness=="claude" { billing_tokens.reasoning=r.tokens.reasoning; }
                        let upgrading=(harness=="codex" && r.parser_version!="codex-v5-request-settings")
                            || (harness=="claude" && r.parser_version!=descriptor.parser_version);
                        upgrading && r.model==parsed.model && r.tokens==billing_tokens && r.provider==parsed.provider
                    });
                    let mut cost=if !adapter.allows_price_estimates(parsed.tokens.total()>0) { Cost { rule_version:"unknown".into(), ..Default::default() } } else if let Some(old)=metadata_upgrade { old.cost.clone() } else { pricing::resolve_with_settings(&parsed,old.as_ref(),&prices,&pricing_settings)? };
                    if matches!(harness.as_str(), "copliot-deprecated" | "copliot") && cost.method=="estimated" { cost.billing_basis=Some("model_reference".into()); }
                    if let Some(old)=old.as_ref().filter(|r|harness=="codex" && r.model==parsed.model && r.tokens==parsed.tokens && r.provider==parsed.provider) {
                        // Overlapping log copies may visit the upgraded record again.
                        // A quote's timestamp alone is not a new price.
                        let quoted_at=cost.priced_at;
                        cost.priced_at=old.cost.priced_at;
                        if cost!=old.cost {cost.priced_at=quoted_at;}
                    }
                    s.save_timing(&parsed.id, parsed.timing.as_ref())?;
                    let r=UsageRecord{
        timing: parsed.timing.clone(),
        request_metadata: parsed.request_metadata,provider:parsed.provider,device_id:device.clone(),id:parsed.id,harness:harness.clone(),parser_version:descriptor.parser_version.into(),session_id:Some(parsed.session_id),native_event_id:Some(parsed.native_event_id),occurred_at:parsed.occurred_at,model:parsed.model,grain:"event".into(),count:1,tokens:parsed.tokens,cost,record_source:"session".into(),revision:1,deleted:false,status_code:None,legacy_date:None,legacy_timezone:None};
                    sync::validate_record(&r)?;
                    if s.upsert(r,&key)?{if old.is_some(){report.updated+=1}else{report.imported+=1}}else{report.skipped+=1}
                }
                if complete{
                    let mut stmt=s.conn.prepare("SELECT payload FROM records WHERE source_path=?1")?;
                    let stored=stmt.query_map([&key],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
                    for payload in stored{let mut old:UsageRecord=serde_json::from_str(&payload)?;if !old.deleted&&old.record_source=="session"&&!active.contains(&old.id){old.deleted=true;s.upsert(old,&key)?;report.updated+=1}}
                    s.conn.execute("INSERT INTO file_state(path,fingerprint,parser_version) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET fingerprint=excluded.fingerprint,parser_version=excluded.parser_version",params![key,fingerprint,descriptor.parser_version])?;
                }Ok(())
            })?;
        }
        let s = self.lock()?;
        s.set_meta("last_scan_at", &chrono::Utc::now().timestamp_millis())?;
        s.set_meta("last_scan", &report)?;
        Ok(report)
    }
    fn save_prices(&self, args: Value, detailed: bool) -> Result<Value> {
        let entries = args["entries"].as_array().context("Missing prices")?;
        let s = self.lock()?;
        s.transaction(|| {
            let mut prices = s.prices()?;
            let mut saved = 0;
            for value in entries {
                let mut v = value.clone();
                if v.get("source").is_none() {
                    v["source"] = json!("manual")
                }
                if v.get("sourceKey").is_none() {
                    v["sourceKey"] = Value::Null
                }
                if v.get("fetchedAt").is_none() {
                    v["fetchedAt"] = json!(chrono::Utc::now().timestamp())
                }
                let mut p: Price = serde_json::from_value(v)?;
                anyhow::ensure!(
                    p.go_tariff.is_none(),
                    "Go catalog snapshots cannot be imported as flat prices"
                );
                p.provider = p
                    .provider
                    .as_deref()
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(crate::providers::pricing_id)
                    .map(str::to_owned);
                pricing::validate(&p)?;
                if p.provider.as_deref() == Some("opencode-go")
                    && pricing::go_catalog::find(&pricing::go_catalog::load(&s)?, &p.model_id)
                        .is_some()
                {
                    anyhow::ensure!(
                        p.source == "models_dev",
                        "OpenCode Go pricing is managed by local Go rules"
                    );
                    continue;
                }
                anyhow::ensure!(
                    ["models_dev", "manual"].contains(&p.source.as_str()),
                    "Invalid imported price source"
                );
                // A flat models.dev import must never replace the official time-of-use tariff.
                if pricing::deepseek::canonical(&p.model_id).is_some()
                    && matches!(
                        p.provider.as_deref(),
                        Some("deepseek-official" | "opencode-go")
                    )
                {
                    anyhow::ensure!(
                        p.source == "models_dev",
                        "Official DeepSeek pricing is managed by the built-in rule"
                    );
                    continue;
                }
                anyhow::ensure!(
                    p.pricing_rule.is_none(),
                    "Manual/imported prices cannot install a pricing rule"
                );
                if p.source == "models_dev"
                    && prices.iter().any(|old| {
                        old.model_id == p.model_id
                            && old.provider == p.provider
                            && old.source == "manual"
                    })
                {
                    continue;
                }
                prices.retain(|old| old.model_id != p.model_id || old.provider != p.provider);
                prices.push(p);
                saved += 1;
            }
            s.set_meta("prices", &prices)?;
            let usage_changed = self.reprice(&s, false, None, None)?;
            Ok(if detailed {
                json!({"saved":saved,"usageChanged":usage_changed})
            } else {
                json!(saved)
            })
        })
    }
    /// Local session estimates only. Reported amounts, migrations and remote facts are immutable here.
    fn permits_price_estimates(&self, harness: &str, has_tokens: bool) -> bool {
        self.registry
            .get(harness)
            .map(|a| a.allows_price_estimates(has_tokens))
            .unwrap_or(true)
    }
    fn reprice(
        &self,
        s: &Store,
        include_estimated: bool,
        model: Option<&str>,
        harness: Option<&str>,
    ) -> Result<u64> {
        let prices = s.prices()?;
        let settings = s.pricing_settings()?;
        let mut changed = 0;
        for mut r in s.local_records()?.into_iter().filter(|r| {
            !r.deleted
                && harness.is_none_or(|h| r.harness == h)
                && self.permits_price_estimates(&r.harness, r.tokens.total() > 0)
                && r.record_source == "session"
                && r.grain == "event"
                && r.cost.method != "reported"
                && (include_estimated || r.cost.amount_usd.is_none())
                && model
                    .is_none_or(|m| pricing::deepseek::canonical(&r.model).unwrap_or(&r.model) == m)
        }) {
            if let Some(price) =
                pricing::find_for_provider(&prices, &r.model, r.provider.as_deref())
            {
                let mut cost = pricing::estimate_at(&r.tokens, price, r.occurred_at, &settings)?;
                if matches!(r.harness.as_str(), "copliot-deprecated" | "copliot")
                    && cost.method == "estimated"
                {
                    cost.billing_basis = Some("model_reference".into());
                }
                // Do not erase a known estimate when a rate or historical calendar is missing.
                if r.cost.amount_usd.is_some() && cost.amount_usd.is_none() {
                    continue;
                }
                let mut comparison = cost.clone();
                comparison.priced_at = r.cost.priced_at;
                if comparison == r.cost {
                    continue;
                }
                r.cost = cost;
                let path: String = s.conn.query_row(
                    "SELECT source_path FROM records WHERE device_id=?1 AND id=?2",
                    params![r.device_id, r.id],
                    |row| row.get(0),
                )?;
                sync::validate_record(&r)?;
                if s.upsert(r, &path)? {
                    changed += 1;
                }
            }
        }
        Ok(changed)
    }
    pub fn sessions(&self) -> Result<Vec<Session>> {
        let config = self.lock()?.config()?;
        let settings = crate::applications::load(&*self.lock()?)?;
        let mut sessions = Vec::new();
        let mut visited = HashSet::new();
        let mut keys = HashSet::new();
        for source in &config.sources {
            let adapter = self.registry.get(&source.harness)?;
            if !Path::new(&source.root).exists() {
                continue;
            }
            let mut source_sessions = Vec::new();
            for path in adapter.discover(Path::new(&source.root))? {
                if !visited.insert(path.clone()) {
                    continue;
                }
                keys.insert((source.harness.clone(), path.clone()));
                if let Ok(rows) = self.reader.metadata(adapter, &path) {
                    source_sessions.extend(rows.into_iter().map(|mut session| {
                        session.can_delete = adapter.descriptor().delete
                            && crate::applications::get(&settings, &source.harness)
                                .collection_enabled;
                        session
                    }));
                }
            }
            adapter.enrich_metadata(Path::new(&source.root), &mut source_sessions);
            sessions.extend(source_sessions);
        }
        self.reader.retain_metadata(&keys);
        sessions.sort_by_key(|session| std::cmp::Reverse(session.last_active_at));
        Ok(sessions)
    }
    fn session_path(&self, args: &Value) -> Result<PathBuf> {
        let harness = args["providerId"].as_str().context("Missing harness")?;
        let path = Path::new(
            args["sourcePath"]
                .as_str()
                .context("Missing session source")?,
        );
        let config = self.lock()?.config()?;
        for source in config.sources.iter().filter(|s| s.harness == harness) {
            if let Ok(path) = files::allowed_path(Path::new(&source.root), path) {
                return Ok(path);
            }
        }
        anyhow::bail!("Session is outside configured log roots")
    }
    fn delete_session(&self, args: &Value) -> Result<Value> {
        let _scan = self
            .scan_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Scan lock poisoned"))?;
        let path = self.session_path(args)?;
        let config = self.lock()?.config()?;
        let settings = crate::applications::load(&*self.lock()?)?;
        anyhow::ensure!(
            crate::applications::get(&settings, args["providerId"].as_str().unwrap_or(""))
                .collection_enabled,
            "Enable collection for this application before deleting a session"
        );
        anyhow::ensure!(
            config.sources.iter().any(|s| s.enabled
                && s.harness == args["providerId"].as_str().unwrap_or("")
                && files::allowed_path(Path::new(&s.root), &path).is_ok()),
            "Enable collection for this source before deleting a session"
        );
        let adapter = self
            .registry
            .get(args["providerId"].as_str().unwrap_or(""))?;
        anyhow::ensure!(adapter.descriptor().delete, "This harness does not support reliable deletion; delete sessions in the original application");
        let (lines, hash_before, errors, complete) = adapter.read_source(&path)?;
        anyhow::ensure!(
            complete && errors.is_empty(),
            "Incomplete session cannot be deleted"
        );
        let adapter = self
            .registry
            .get(args["providerId"].as_str().unwrap_or(""))?;
        let session = adapter
            .sessions(&path, &lines)
            .into_iter()
            .find(|s| Some(s.session_id.as_str()) == args["sessionId"].as_str())
            .context("Unknown session")?;
        anyhow::ensure!(
            session.session_id == args["sessionId"].as_str().unwrap_or(""),
            "Session ID mismatch"
        );
        let modified = std::fs::metadata(&path)?.modified()?;
        anyhow::ensure!(
            modified.elapsed().unwrap_or_default() >= Duration::from_secs(60),
            "Session was recently written; retry after it becomes idle"
        );
        let report = self.scan_unlocked(true)?;
        anyhow::ensure!(
            report.errors.is_empty() && report.deferred_files == 0,
            "Collection is incomplete; deletion cancelled"
        );
        anyhow::ensure!(
            adapter.read_source(&path)?.1 == hash_before,
            "Session changed while collecting; deletion cancelled"
        );
        adapter.delete_session(&path, &session.session_id)?;
        Ok(json!(true))
    }
    pub fn sync(&self) -> Result<Value> {
        let _guard = self
            .sync_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Sync lock poisoned"))?;
        let result = self.sync_inner();
        let s = self.lock()?;
        match &result {
            Ok(report) => {
                s.set_meta("last_sync_at", &chrono::Utc::now().timestamp_millis())?;
                s.set_meta("last_sync_error", &report["warning"].as_str())?
            }
            Err(e) => s.set_meta("last_sync_error", &format!("{e:#}"))?,
        }
        result
    }
    fn sync_inner(&self) -> Result<Value> {
        let (config, device) = {
            let s = self.lock()?;
            (s.config()?, s.device_id()?)
        };
        anyhow::ensure!(
            !config.server_url.trim().is_empty() && !config.server_token.is_empty(),
            "Configure server URL and token first"
        );
        let url = reqwest::Url::parse(&config.server_url)?;
        anyhow::ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(
                        url.host_str(),
                        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
                    )),
            "Use HTTPS for remote sync servers"
        );
        let base = config.server_url.trim_end_matches('/');
        let network = crate::network::load(&*self.read_lock()?)?;
        let client = network.client()?;
        let health: Value = client
            .get(format!("{base}/health"))
            .bearer_auth(&config.server_token)
            .send()?
            .error_for_status()?
            .json()?;
        {
            let s = self.lock()?;
            // A server URL change during the request must not label the new server
            // with the previous server's schema. Older servers leave it unknown.
            if s.config()?.server_url == config.server_url {
                let schema = health["schemaVersion"]
                    .as_u64()
                    .and_then(|v| u32::try_from(v).ok())
                    .filter(|v| *v > 0);
                s.transaction(|| {
                    s.set_meta("remote_schema_version", &schema)?;
                    s.set_meta(
                        "remote_schema_checked_at",
                        &chrono::Utc::now().timestamp_millis(),
                    )
                })?;
            }
        }
        anyhow::ensure!(health["protocolVersion"].as_u64().is_some_and(|v| v >= 4), "Sync server must be upgraded to protocol v4 to preserve timing and request metadata; pending records have been retained");
        let claude_metadata = health["recordCapabilities"]
            .as_array()
            .is_some_and(|caps| caps.iter().any(|v| v == "claude_request_timing_v2"));
        let mut cursor = self.lock()?.meta::<i64>("download_cursor")?.unwrap_or(0);
        let mut first: ChangesResponse = client
            .get(format!("{base}/v1/changes"))
            .query(&[("after", cursor), ("limit", 500), ("protocolVersion", 4)])
            .bearer_auth(&config.server_token)
            .send()?
            .error_for_status()?
            .json()?;
        let instance = self
            .lock()?
            .meta::<String>("remote_instance_id")?
            .unwrap_or_default();
        if instance != first.server_instance_id {
            let s = self.lock()?;
            s.transaction(|| {
                s.conn.execute(
                    "DELETE FROM record_timings WHERE device_id<>?1",
                    [s.device_id()?],
                )?;
                s.conn.execute("DELETE FROM remote_records", [])?;
                s.set_meta("download_cursor", &0i64)?;
                s.set_meta("remote_instance_id", &first.server_instance_id)?;
                s.requeue_all()
            })?;
            cursor = 0;
            drop(s);
            first = client
                .get(format!("{base}/v1/changes"))
                .query(&[("after", cursor), ("limit", 500), ("protocolVersion", 4)])
                .bearer_auth(&config.server_token)
                .send()?
                .error_for_status()?
                .json()?;
        }
        sync::apply_changes(&*self.lock()?, &first)?;
        let mut uploaded = 0;
        // Publish a renamed (or new) device even when there are no usage records to upload.
        let mut publish_name = !first.devices.iter().any(|d| {
            d["id"].as_str() == Some(device.as_str())
                && d["name"].as_str() == Some(config.device_name.as_str())
        });
        loop {
            let records = self.lock()?.pending_compatible(claude_metadata)?;
            if records.is_empty() && !publish_name {
                break;
            }
            let expected: HashSet<_> = records.iter().map(|r| (r.id.clone(), r.revision)).collect();
            let request = PushRequest {
                protocol_version: 4,
                device_id: device.clone(),
                device_name: config.device_name.clone(),
                records,
            };
            let response: PushResponse = client
                .post(format!("{base}/v1/push"))
                .bearer_auth(&config.server_token)
                .header("Content-Type", "application/json")
                .header("Content-Encoding", "gzip")
                .body(sync::gzip_json(&request)?)
                .send()?
                .error_for_status()?
                .json()?;
            anyhow::ensure!(
                response.server_instance_id == first.server_instance_id,
                "Server changed during upload; retry"
            );
            anyhow::ensure!(
                response
                    .accepted
                    .iter()
                    .map(|a| (a.id.clone(), a.revision))
                    .collect::<HashSet<_>>()
                    == expected,
                "Server returned an incomplete or invalid acknowledgment"
            );
            uploaded += response.accepted.len();
            self.lock()?.ack(&response.accepted)?;
            publish_name = false;
        }
        let mut downloaded = first.changes.len();
        loop {
            cursor = self.lock()?.meta::<i64>("download_cursor")?.unwrap_or(0);
            let response: ChangesResponse = client
                .get(format!("{base}/v1/changes"))
                .query(&[("after", cursor), ("limit", 500), ("protocolVersion", 4)])
                .bearer_auth(&config.server_token)
                .send()?
                .error_for_status()?
                .json()?;
            anyhow::ensure!(
                response.server_instance_id == first.server_instance_id,
                "Server changed during download; retry"
            );
            downloaded += response.changes.len();
            anyhow::ensure!(
                !response.has_more || response.next_cursor > cursor,
                "Server download cursor did not advance"
            );
            sync::apply_changes(&*self.lock()?, &response)?;
            if !response.has_more {
                break;
            }
        }
        let deferred = if claude_metadata {
            0
        } else {
            self.lock()?.pending_claude_metadata_count()?
        };
        let subscription_capable = health["recordCapabilities"].as_array().is_some_and(|caps| {
            caps.iter()
                .any(|v| v == crate::accounts::sharing::CAPABILITY)
        });
        let subscription_sync = if subscription_capable {
            crate::accounts::sharing::sync(
                self,
                &client,
                base,
                &config.server_token,
                &first.server_instance_id,
            )?
        } else {
            Value::Null
        };
        let subscription_pending = crate::accounts::sharing::pending_count(&*self.lock()?)?;
        let mut warnings = Vec::new();
        if deferred > 0 {
            warnings.push(format!("Sync server needs Claude request metadata support; {deferred} Claude revisions retained locally. Other records continue syncing."));
        }
        if !subscription_capable && subscription_pending > 0 {
            warnings.push("Sync server needs subscription_history_v1 support; subscription history retained locally. Upgrade the server and proxy routes.".into());
        }
        let warning = (!warnings.is_empty()).then(|| warnings.join(" "));
        Ok(
            json!({"uploaded":uploaded,"downloaded":downloaded,"deferredClaude":deferred,"warning":warning,"subscriptionSync":subscription_sync,"subscriptionPending":subscription_pending}),
        )
    }
    pub fn start_background(self: &Arc<Self>) {
        crate::accounts::start_background(self);
        let weak = Arc::downgrade(self);
        let wake = self.background_wake.clone();
        std::thread::spawn(move || loop {
            let generation = match wake.0.lock() {
                Ok(v) => *v,
                Err(_) => break,
            };
            let Some(engine) = weak.upgrade() else { break };
            let interval = engine
                .lock()
                .and_then(|s| s.config())
                .map(|c| c.scan_interval_seconds)
                .unwrap_or(60);
            let _ = engine.scan(false);
            if engine
                .lock()
                .and_then(|s| s.config())
                .is_ok_and(|c| !c.server_url.is_empty() && !c.server_token.is_empty())
            {
                let _ = engine.sync();
            }
            drop(engine);
            let Ok(epoch) = wake.0.lock() else { break };
            let _ = wake
                .1
                .wait_timeout_while(epoch, Duration::from_secs(interval), |epoch| {
                    *epoch == generation
                });
        });
    }
}
