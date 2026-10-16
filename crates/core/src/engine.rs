use crate::{
    harness::{files, Registry, TimelineIndex},
    migration,
    model::*,
    pricing, query,
    store::Store,
    sync,
};
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

pub struct Engine {
    pub store: Mutex<Store>,
    pub registry: Registry,
    scan_lock: Mutex<()>,
    sync_lock: Mutex<()>,
    database_path: PathBuf,
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
            store: Mutex::new(store),
            registry,
            scan_lock: Mutex::new(()),
            sync_lock: Mutex::new(()),
            database_path: path.to_path_buf(),
        })
    }
    pub fn lock(&self) -> Result<MutexGuard<'_, Store>> {
        self.store
            .lock()
            .map_err(|_| anyhow::anyhow!("Database lock poisoned"))
    }
    pub fn execute(&self, name: &str, args: Value) -> Result<Value> {
        match name {
            "snapshot" => {
                let s = self.lock()?;
                let mut config = serde_json::to_value(s.config()?)?;
                let token = !s.config()?.server_token.is_empty();
                config["serverToken"] = json!("");
                Ok(
                    json!({"deviceId":s.device_id()?,"config":config,"tokenConfigured":token,"harnesses":self.registry.descriptors(),"pending":s.pending_count()?,"devices":s.meta::<Vec<Value>>("devices")?.unwrap_or_default(),"lastScanAt":s.meta::<i64>("last_scan_at")?,"lastScan":s.meta::<ScanReport>("last_scan")?,"lastSyncAt":s.meta::<i64>("last_sync_at")?,"lastSyncError":s.meta::<Option<String>>("last_sync_error")?.flatten(),"migrations":s.meta::<Vec<Value>>("migration_manifests")?.unwrap_or_default()}),
                )
            }
            "save_config" => {
                let mut c: Config = serde_json::from_value(args["config"].clone())?;
                c.timezone.parse::<chrono_tz::Tz>()?;
                anyhow::ensure!(
                    c.scan_interval_seconds >= 10 && c.scan_interval_seconds <= 86400,
                    "Scan interval must be 10–86400 seconds"
                );
                anyhow::ensure!(c.device_name.len() <= 128, "Device name too long");
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
                if c.server_token.is_empty() && !args["clearToken"].as_bool().unwrap_or(false) {
                    c.server_token = old.server_token
                }
                if old.server_url != c.server_url {
                    s.transaction(|| {
                        s.conn.execute("DELETE FROM remote_records", [])?;
                        s.set_meta("download_cursor", &0i64)?;
                        s.set_meta("remote_instance_id", &String::new())?;
                        s.requeue_all()
                    })?
                }
                s.set_meta("config", &c)?;
                Ok(json!(true))
            }
            "scan" => Ok(serde_json::to_value(self.scan(false)?)?),
            "rescan" => Ok(serde_json::to_value(self.scan(true)?)?),
            "usage_query" => query::query(
                &*self.lock()?,
                args["kind"].as_str().context("Missing query kind")?,
                &args,
            ),
            "last_scan" => Ok(json!(self.lock()?.meta::<i64>("last_scan_at")?)),
            "prices" => Ok(json!(self.lock()?.prices()?)),
            "save_prices" => self.save_prices(args),
            "delete_price" => {
                let s = self.lock()?;
                let mut prices = s.prices()?;
                prices.retain(|p| p.model_id != args["modelId"].as_str().unwrap_or(""));
                s.set_meta("prices", &prices)?;
                Ok(json!(true))
            }
            "models_config" => {
                let s = self.lock()?;
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
            "session_messages" => {
                let path = self.session_path(&args)?;
                let adapter = self
                    .registry
                    .get(args["providerId"].as_str().context("Missing harness")?)?;
                let (lines, _, errors, _) = adapter.read_source(&path)?;
                anyhow::ensure!(
                    errors.is_empty(),
                    "Session contains incomplete or invalid JSON; retry after writing finishes"
                );
                Ok(json!(
                    adapter.messages_for_session(&lines, args["sessionId"].as_str())
                ))
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
        let config = self.lock()?.config()?;
        let mut report = ScanReport::default();
        let mut inputs = Vec::new();
        let mut paths = HashSet::new();
        // Keep only usage timelines locally so deleting an old transcript does not
        // destroy the baseline needed to interpret an existing child session.
        let mut index: TimelineIndex = self.lock()?.meta("usage_timelines")?.unwrap_or_default();
        for source in config.sources.iter().filter(|s| s.enabled) {
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
                match adapter.read_source(&path) {
                    Ok((lines, fingerprint, errors, complete)) => {
                        if let Some(id) = adapter.context_id(&lines) {
                            index.insert(id, adapter.context_events(&lines));
                        }
                        report.errors.extend(errors);
                        inputs.push((source.harness.clone(), path, lines, fingerprint, complete));
                    }
                    Err(e) => report.errors.push(format!("{}: {e}", path.display())),
                }
            }
        }
        let ancestry = files::hash(
            inputs
                .iter()
                .map(|(_, _, _, f, _)| f.as_str())
                .collect::<String>()
                .as_bytes(),
        );
        for (harness, path, lines, fingerprint, read_complete) in inputs {
            report.files_scanned += 1;
            let adapter = self.registry.get(&harness)?;
            let descriptor = adapter.descriptor();
            let key = path.to_string_lossy().to_string();
            let fingerprint = if adapter.uses_ancestry() {
                format!("{fingerprint}:{ancestry}")
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
                for parsed in result.usage{
                    if parsed.occurred_at<cutoff{continue}
                    active.insert(parsed.id.clone());let old=s.record(&parsed.id)?;
                    let cost=if let Some(old)=&old{
                        if old.tokens==parsed.tokens&&old.model==parsed.model&&old.cost.amount_usd.is_some(){old.cost.clone()}
                        else if let Some(price)=old.cost.snapshot.as_ref().filter(|_|old.model==parsed.model){pricing::estimate(&parsed.tokens,price)?}
                        else if let Some(price)=pricing::find(&prices,&parsed.model){pricing::estimate(&parsed.tokens,price)?}else{Cost::default()}
                    }else if let Some(amount)=parsed.reported_cost{Cost{amount_usd:Some(amount),method:"reported".into(),..Default::default()}}
                    else if let Some(price)=pricing::find(&prices,&parsed.model){pricing::estimate(&parsed.tokens,price)?}else{Cost::default()};
                    let r=UsageRecord{device_id:device.clone(),id:parsed.id,harness:harness.clone(),parser_version:descriptor.parser_version.into(),session_id:Some(parsed.session_id),native_event_id:Some(parsed.native_event_id),occurred_at:parsed.occurred_at,model:parsed.model,grain:"event".into(),count:1,tokens:parsed.tokens,cost,record_source:"session".into(),revision:1,deleted:false,status_code:None,legacy_date:None,legacy_timezone:None};
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
        s.set_meta("usage_timelines", &index)?;
        s.set_meta("last_scan_at", &chrono::Utc::now().timestamp_millis())?;
        s.set_meta("last_scan", &report)?;
        Ok(report)
    }
    fn save_prices(&self, args: Value) -> Result<Value> {
        let entries = args["entries"].as_array().context("Missing prices")?;
        let s = self.lock()?;
        s.transaction(|| {
            let mut prices = s.prices()?;
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
                let p: Price = serde_json::from_value(v)?;
                pricing::validate(&p)?;
                anyhow::ensure!(
                    ["models_dev", "manual"].contains(&p.source.as_str()),
                    "Invalid imported price source"
                );
                prices.retain(|old| old.model_id != p.model_id);
                prices.push(p)
            }
            s.set_meta("prices", &prices)?;
            for mut r in s.local_records()?.into_iter().filter(|r| {
                !r.deleted && r.cost.amount_usd.is_none() && r.record_source == "session"
            }) {
                if let Some(p) = pricing::find(&prices, &r.model) {
                    r.cost = pricing::estimate(&r.tokens, p)?;
                    let path: String = s.conn.query_row(
                        "SELECT source_path FROM records WHERE device_id=?1 AND id=?2",
                        params![r.device_id, r.id],
                        |row| row.get(0),
                    )?;
                    s.upsert(r, &path)?;
                }
            }
            Ok(json!(entries.len()))
        })
    }
    pub fn sessions(&self) -> Result<Vec<Session>> {
        let config = self.lock()?.config()?;
        let mut sessions = Vec::new();
        let mut visited = HashSet::new();
        for source in &config.sources {
            let adapter = self.registry.get(&source.harness)?;
            if !Path::new(&source.root).exists() {
                continue;
            }
            for path in adapter.discover(Path::new(&source.root))? {
                if !visited.insert(path.clone()) {
                    continue;
                }
                if let Ok((lines, _, _, _)) = adapter.read_source(&path) {
                    sessions.extend(adapter.sessions(&path, &lines));
                }
            }
        }
        sessions.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
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
        anyhow::ensure!(
            config.sources.iter().any(|s| s.enabled
                && s.harness == args["providerId"].as_str().unwrap_or("")
                && files::allowed_path(Path::new(&s.root), &path).is_ok()),
            "Enable collection for this source before deleting a session"
        );
        let adapter = self
            .registry
            .get(args["providerId"].as_str().unwrap_or(""))?;
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
            Ok(_) => {
                s.set_meta("last_sync_at", &chrono::Utc::now().timestamp_millis())?;
                s.set_meta("last_sync_error", &Option::<String>::None)?
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
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let mut cursor = self.lock()?.meta::<i64>("download_cursor")?.unwrap_or(0);
        let mut first: ChangesResponse = client
            .get(format!("{base}/v1/changes"))
            .query(&[("after", cursor), ("limit", 500)])
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
                s.conn.execute("DELETE FROM remote_records", [])?;
                s.set_meta("download_cursor", &0i64)?;
                s.set_meta("remote_instance_id", &first.server_instance_id)?;
                s.requeue_all()
            })?;
            cursor = 0;
            drop(s);
            first = client
                .get(format!("{base}/v1/changes"))
                .query(&[("after", cursor), ("limit", 500)])
                .bearer_auth(&config.server_token)
                .send()?
                .error_for_status()?
                .json()?;
        }
        sync::apply_changes(&*self.lock()?, &first)?;
        let mut uploaded = 0;
        loop {
            let records = self.lock()?.pending()?;
            if records.is_empty() {
                break;
            }
            let expected: HashSet<_> = records.iter().map(|r| (r.id.clone(), r.revision)).collect();
            let response: PushResponse = client
                .post(format!("{base}/v1/push"))
                .bearer_auth(&config.server_token)
                .json(&PushRequest {
                    protocol_version: 1,
                    device_id: device.clone(),
                    device_name: config.device_name.clone(),
                    records,
                })
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
        }
        let mut downloaded = first.changes.len();
        loop {
            cursor = self.lock()?.meta::<i64>("download_cursor")?.unwrap_or(0);
            let response: ChangesResponse = client
                .get(format!("{base}/v1/changes"))
                .query(&[("after", cursor), ("limit", 500)])
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
        Ok(json!({"uploaded":uploaded,"downloaded":downloaded}))
    }
    pub fn start_background(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        std::thread::spawn(move || loop {
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
            std::thread::sleep(Duration::from_secs(interval));
        });
    }
}
