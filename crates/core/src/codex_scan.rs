//! File-aware Codex collection and atomic rollout-identity migration.
//! All tables here are device-local; ordinary UsageRecord revisions use sync v4.
use crate::{
    harness::{codex::Codex, codex_usage as codex, files, HarnessAdapter, TimelineIndex},
    model::{ParsedUsage, ScanReport, UsageRecord},
    scan_cache::Stamp,
    Engine,
};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Cache {
    version: String,
    #[serde(default)]
    projection_version: u32,
    stamp: Stamp,
    fingerprint: String,
    context: Vec<Value>,
}
struct Input {
    path: PathBuf,
    stamp: Option<Stamp>,
    fingerprint: String,
    context: Vec<Value>,
    lines: Option<Vec<Value>>,
    complete: bool,
}
struct Work {
    path: String,
    fingerprint: String,
    usage: Vec<ParsedUsage>,
    observed_at: HashSet<i64>,
}
#[derive(Default)]
struct Group {
    work: Vec<Work>,
    blocked: bool,
}

pub(crate) fn is_compressed(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == "zst" || ext == "zstd")
}

// Canonical rollout filenames constrain the session and rollout IDs. Unknown
// filename formats must still be read and checked against their metadata.
fn may_match_dependencies(path: &Path, needed: &HashSet<String>) -> bool {
    let name = codex::logical_name(path);
    let stem = name.strip_suffix(".jsonl").unwrap_or(&name);
    let Some(last) = stem
        .len()
        .checked_sub(36)
        .and_then(|at| stem.get(at..))
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
    else {
        return true;
    };
    if needed.contains(&last.to_string()) {
        return true;
    }
    let leading = stem
        .len()
        .checked_sub(73)
        .and_then(|at| stem.get(at..))
        .filter(|pair| pair.as_bytes().get(36) == Some(&b'_'))
        .and_then(|pair| pair.get(..36))
        .and_then(|s| uuid::Uuid::parse_str(s).ok());
    leading.is_some_and(|id| needed.contains(&id.to_string()))
}

pub(crate) fn scan(engine: &Engine, force: bool, report: &mut ScanReport) -> Result<()> {
    let config = engine.lock()?.config()?;
    let mut paths = BTreeSet::new();
    for source in config
        .sources
        .iter()
        .filter(|s| s.enabled && s.harness == "codex")
    {
        match Codex.discover(Path::new(&source.root)) {
            Ok(found) => paths.extend(found),
            Err(e) => report.errors.push(e.to_string()),
        }
    }
    // Compressed rollouts are archived history. Keep already imported facts and
    // cached parent contexts, but do not decompress them during collection.
    paths.retain(|p| !is_compressed(p));
    if paths.is_empty() {
        return Ok(());
    }
    let mut caches: BTreeMap<String, Cache> = BTreeMap::new();
    let mut inputs = Vec::new();
    let mut unavailable = HashSet::new();
    let mut blocked_sessions = HashSet::new();
    for path in paths {
        report.files_scanned += 1;
        let key = path.to_string_lossy().to_string();
        let stamp = Stamp::read(&path).ok();
        let cached: Option<String> = engine
            .lock()?
            .conn
            .query_row(
                "SELECT payload FROM codex_source_cache WHERE path=?",
                [&key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(cache) = cached
            .and_then(|v| serde_json::from_str::<Cache>(&v).ok())
            .filter(|c| c.version == codex::VERSION)
        {
            caches.insert(key.clone(), cache);
        }
        if let Some(c) = caches.get(&key).filter(|c| {
            !force
                && Some(&c.stamp) == stamp.as_ref()
                && !(c.projection_version == 0
                    && codex::identity(&path, &c.context)
                        .is_ok_and(|id| !id.continuation && id.history_base.is_some()))
        }) {
            inputs.push(Input {
                path,
                stamp,
                fingerprint: c.fingerprint.clone(),
                context: c.context.clone(),
                lines: None,
                complete: true,
            });
            continue;
        }
        match Codex.read_usage_source(&path) {
            Ok((lines, fingerprint, errors, complete)) => {
                report.errors.extend(errors);
                let complete = complete && stamp.is_some() && Stamp::read(&path).ok() == stamp;
                let context = codex::context(&lines);
                if complete && codex::identity(&path, &context).is_ok() {
                    let cache = Cache {
                        version: codex::VERSION.into(),
                        projection_version: 1,
                        stamp: stamp.clone().unwrap(),
                        fingerprint: fingerprint.clone(),
                        context: context.clone(),
                    };
                    engine.lock()?.conn.execute("INSERT INTO codex_source_cache(path,payload) VALUES(?1,?2) ON CONFLICT(path) DO UPDATE SET payload=excluded.payload",params![key,serde_json::to_string(&cache)?])?;
                    caches.insert(key, cache);
                } else {
                    if let Some(id) = codex::identity(&path, &context).ok().or_else(|| {
                        caches
                            .get(&key)
                            .and_then(|c| codex::identity(&path, &c.context).ok())
                    }) {
                        blocked_sessions.insert(id.session);
                    }
                    unavailable.insert(key);
                }
                inputs.push(Input {
                    path,
                    stamp,
                    fingerprint,
                    context,
                    lines: Some(lines),
                    complete,
                });
            }
            Err(e) => {
                if let Some(id) = caches
                    .get(&key)
                    .and_then(|c| codex::identity(&path, &c.context).ok())
                {
                    blocked_sessions.insert(id.session);
                }
                unavailable.insert(key);
                report.deferred_files += 1;
                report.errors.push(format!("{}: {e}", path.display()));
            }
        }
    }
    let mut index = TimelineIndex::new();
    let live_paths: HashSet<_> = inputs
        .iter()
        .map(|i| i.path.to_string_lossy().to_string())
        .collect();
    let live_rollouts: HashSet<_> = inputs
        .iter()
        .filter_map(|i| codex::identity(&i.path, &i.context).ok())
        .map(|i| (i.session, i.rollout))
        .collect();
    let needed: HashSet<_> = inputs
        .iter()
        .filter_map(|i| codex::identity(&i.path, &i.context).ok())
        .flat_map(|id| codex::dependencies(&id))
        .collect();
    {
        let s = engine.lock()?;
        let mut stmt = s.conn.prepare("SELECT path FROM codex_source_cache")?;
        let mut payload_query = s
            .conn
            .prepare("SELECT payload FROM codex_source_cache WHERE path=?")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            if !may_match_dependencies(Path::new(&path), &needed) {
                continue;
            }
            let payload: String = payload_query.query_row([&path], |r| r.get(0))?;
            let Some(cache) = serde_json::from_str::<Cache>(&payload)
                .ok()
                .filter(|c| c.version == codex::VERSION)
            else {
                continue;
            };
            let Ok(id) = codex::identity(Path::new(&path), &cache.context) else {
                continue;
            };
            if !needed.contains(&id.session) && !needed.contains(&id.rollout) {
                continue;
            }
            // A moved/compressed source supersedes its cached old path. Retain a
            // deleted parent's context only when there is no live replacement.
            if !live_paths.contains(&path)
                && !unavailable.contains(&path)
                && live_rollouts.contains(&(id.session, id.rollout))
            {
                continue;
            }
            codex::add_context(
                &mut index,
                Path::new(&path),
                &cache.context,
                !unavailable.contains(&path),
            )?;
        }
    }
    for input in inputs.iter().filter(|i| !i.complete) {
        let key = input.path.to_string_lossy().to_string();
        if !caches.contains_key(&key) {
            let _ = codex::add_context(&mut index, &input.path, &input.context, false);
        }
    }
    // Preserve old parent observations when the transcript was intentionally
    // removed. Never let this fallback override a currently unavailable source.
    {
        let s = engine.lock()?;
        let mut stmt = s
            .conn
            .prepare("SELECT payload FROM usage_contexts WHERE id=?")?;
        for id in needed.iter().map(|id| format!("codex:{id}")) {
            if let std::collections::hash_map::Entry::Vacant(e) = index.entry(id) {
                if let Some(payload) = stmt
                    .query_row([e.key()], |r| r.get::<_, String>(0))
                    .optional()?
                {
                    e.insert(serde_json::from_str(&payload)?);
                }
            }
        }
    }
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    let mut runtime = crate::codex_runtime::Reader::default();
    for input in inputs {
        let key = input.path.to_string_lossy().to_string();
        let id = match codex::identity(&input.path, &input.context) {
            Ok(id) => id,
            Err(e) => {
                report.errors.push(format!("{key}: {e}"));
                report.deferred_files += 1;
                continue;
            }
        };
        let group = groups.entry(id.session.clone()).or_default();
        if blocked_sessions.contains(&id.session) {
            group.blocked = true;
        }
        if !input.complete {
            group.blocked = true;
            report.deferred_files += 1;
            continue;
        }
        let deps = codex::dependencies(&id)
            .into_iter()
            .map(|id| {
                let value = index.get(&format!("codex:{id}"));
                json!([id, value])
            })
            .collect::<Vec<_>>();
        let evidence = runtime.read(engine, &input.path, &id.session)?;
        let boundary = input
            .context
            .iter()
            .find(|v| v["type"] == "session_meta")
            .is_some_and(|meta| {
                !meta["payload"]["history_base"]["end_byte_offset"].is_null()
                    || !meta["payload"]["history_base"]["end_ordinal_exclusive"].is_null()
            });
        let fingerprint = format!(
            "{}:{}:{}{}",
            input.fingerprint,
            files::hash(serde_json::to_string(&deps)?.as_bytes()),
            evidence.fingerprint(),
            if !id.continuation && boundary {
                ":parent-boundary-v1"
            } else {
                ""
            }
        );
        let previous: Option<(String, String)> = engine
            .lock()?
            .conn
            .query_row(
                "SELECT fingerprint,parser_version FROM file_state WHERE path=?",
                [&key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if !force && previous == Some((fingerprint.clone(), codex::VERSION.into())) {
            report.skipped += 1;
            continue;
        }
        let mut lines = if let Some(lines) = input.lines {
            lines
        } else {
            let (lines, _, errors, complete) = match Codex.read_usage_source(&input.path) {
                Ok(read) => read,
                Err(e) => {
                    report.errors.push(format!("{key}: {e}"));
                    report.deferred_files += 1;
                    group.blocked = true;
                    continue;
                }
            };
            report.errors.extend(errors);
            if !complete || Stamp::read(&input.path).ok() != input.stamp {
                group.blocked = true;
                report.deferred_files += 1;
                continue;
            }
            lines
        };
        evidence.enrich(&mut lines);
        let result = Codex.parse(&input.path, &lines, &index)?;
        report.errors.extend(result.diagnostics);
        if !result.complete {
            group.blocked = true;
            report.deferred_files += 1;
            continue;
        }
        group.work.push(Work {
            path: key,
            fingerprint,
            usage: result.usage,
            observed_at: lines
                .iter()
                .filter(|v| {
                    v["type"] == "token_usage_record"
                        || (v["type"] == "event_msg" && v["payload"]["type"] == "token_count")
                })
                .filter_map(|v| {
                    v["timestamp"]
                        .as_str()
                        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                        .map(|t| t.timestamp())
                })
                .collect(),
        });
    }
    for (session, group) in groups {
        if group.blocked {
            report.deferred_files += group.work.len() as u64;
            continue;
        }
        if group.work.is_empty() {
            continue;
        }
        match apply(engine, &session, &group.work) {
            Ok((imported, updated, skipped)) => {
                report.imported += imported;
                report.updated += updated;
                report.skipped += skipped;
            }
            Err(e) => {
                report.deferred_files += group.work.len() as u64;
                report
                    .errors
                    .push(format!("Codex session {session}: {e:#}"));
            }
        }
    }
    Ok(())
}
fn key(at: i64, model: &str, tokens: &crate::model::Tokens, provider: Option<&str>) -> String {
    json!([at, model, tokens, provider]).to_string()
}

fn apply(engine: &Engine, session: &str, work: &[Work]) -> Result<(u64, u64, u64)> {
    let s = engine.lock()?;
    let device = s.device_id()?;
    let prices = s.prices()?;
    let pricing = s.pricing_settings()?;
    let manifests: Vec<Value> = s.meta("migration_manifests")?.unwrap_or_default();
    let cutoff = manifests
        .iter()
        .filter(|m| {
            m["harnesses"]
                .as_array()
                .is_some_and(|hs| hs.iter().any(|h| h == "codex"))
        })
        .filter_map(|m| m["cutoff"].as_i64())
        .max()
        .unwrap_or(0);
    let rows=s.conn.prepare("SELECT r.payload,r.source_path FROM records r JOIN usage_index u ON u.origin='local' AND u.device_id=r.device_id AND u.id=r.id WHERE u.harness='codex' AND u.session_id=?1 AND u.record_source='session'")?
        .query_map([session],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<Result<Vec<_>,_>>()?;
    let old: Vec<(UsageRecord, String)> = rows
        .into_iter()
        .map(|(v, p)| Ok((serde_json::from_str(&v)?, p)))
        .collect::<Result<_>>()?;
    // Only this session's legacy records can be candidates for rebinding.
    let mut bound = HashSet::new();
    let mut binding_query = s
        .conn
        .prepare("SELECT 1 FROM codex_event_bindings WHERE record_id=?")?;
    for (record, _) in &old {
        if binding_query.exists([&record.id])? {
            bound.insert(record.id.clone());
        }
    }
    let mut legacy: HashMap<String, Vec<&UsageRecord>> = HashMap::new();
    for (r, _) in &old {
        if !r.deleted && r.parser_version != codex::VERSION && !bound.contains(&r.id) {
            legacy
                .entry(key(
                    r.occurred_at,
                    &r.model,
                    &r.tokens,
                    r.provider.as_deref(),
                ))
                .or_default()
                .push(r);
        }
    }
    let mut events: BTreeMap<String, (ParsedUsage, Vec<String>)> = BTreeMap::new();
    for file in work {
        for p in &file.usage {
            if p.occurred_at < cutoff {
                continue;
            }
            if let Some((previous, paths)) = events.get_mut(&p.id) {
                anyhow::ensure!(
                    previous.occurred_at == p.occurred_at
                        && previous.tokens == p.tokens
                        && previous.model == p.model
                        && previous.provider == p.provider,
                    "Conflicting copies of rollout event {}",
                    p.id
                );
                paths.push(file.path.clone());
            } else {
                events.insert(p.id.clone(), (p.clone(), vec![file.path.clone()]));
            }
        }
    }
    let mut imported = 0;
    let mut updated = 0;
    let mut skipped = 0;
    s.transaction(|| {
        let mut claimed = bound.clone();
        for (event_id, (mut parsed, paths)) in events {
            let binding: Option<String> = s.conn.query_row(
                "SELECT record_id FROM codex_event_bindings WHERE event_id=?",
                [&event_id], |r| r.get(0),
            ).optional()?;
            let record_id = if let Some(id) = binding {
                id
            } else {
                let signature = key(parsed.occurred_at, &parsed.model, &parsed.tokens, parsed.provider.as_deref());
                let matches = legacy.get(&signature).map(|rs| {
                    rs.iter().filter(|r| !claimed.contains(&r.id)).copied().collect::<Vec<_>>()
                }).unwrap_or_default();
                anyhow::ensure!(matches.len() <= 1,
                    "Ambiguous historical event at {}; migration deferred", parsed.occurred_at);
                matches.first().map(|r| r.id.clone()).unwrap_or_else(|| event_id.clone())
            };
            let previous = s.record(&record_id)?;
            if let Some(previous) = &previous {
                anyhow::ensure!(previous.occurred_at == parsed.occurred_at,
                    "Rollout event moved to a different timestamp; collection deferred");
                if parsed.provider.is_none() {
                    parsed.provider = previous.provider.clone();
                }
            }
            claimed.insert(record_id.clone());
            let same = previous.as_ref().filter(|r| {
                r.model == parsed.model && r.tokens == parsed.tokens && r.provider == parsed.provider
            });
            let cost = if let Some(r) = same {
                r.cost.clone()
            } else {
                crate::pricing::resolve_with_settings(&parsed, previous.as_ref(), &prices, &pricing)?
            };
            let record = UsageRecord {
                id: record_id.clone(), native_event_id: Some(parsed.native_event_id),
                session_id: Some(parsed.session_id), device_id: device.clone(),
                harness: "codex".into(), parser_version: codex::VERSION.into(),
                occurred_at: parsed.occurred_at, model: parsed.model, provider: parsed.provider,
                tokens: parsed.tokens, cost, timing: parsed.timing,
                request_metadata: parsed.request_metadata, grain: "event".into(), count: 1,
                record_source: "session".into(), revision: 1, deleted: false,
                status_code: None, legacy_date: None, legacy_timezone: None,
            };
            crate::sync::validate_record(&record)?;
            let timing = record.timing.clone();
            if s.upsert(record, &paths[0])? {
                if previous.is_some() { updated += 1; } else { imported += 1; }
            } else {
                skipped += 1;
            }
            // Restore a missing local projection without revising unchanged
            // synced facts or their frozen prices.
            s.save_record_timing(&device, &record_id, timing.as_ref())?;
            s.conn.execute(
                "INSERT INTO codex_event_bindings(event_id,record_id) VALUES(?1,?2) ON CONFLICT(event_id) DO NOTHING",
                params![event_id, record_id],
            )?;
            for path in paths {
                s.conn.execute("INSERT OR IGNORE INTO codex_event_sources(event_id,source_path) VALUES(?1,?2)",
                    params![event_id, path])?;
            }
        }
        // v5 facts from completely rebuilt sources that have no v6 counterpart
        // are superseded (e.g. replay snapshots). Missing/deleted source history
        // is retained. This one-time migration is not ongoing file truncation.
        let rebuilt: HashMap<_, _> = work.iter().map(|w| (w.path.as_str(), &w.observed_at)).collect();
        for (record, path) in &old {
            if !record.deleted && record.parser_version != codex::VERSION
                && !claimed.contains(&record.id)
                && rebuilt.get(path.as_str()).is_some_and(|observed| observed.contains(&record.occurred_at)) {
                let mut tombstone = record.clone();
                tombstone.deleted = true;
                if s.upsert(tombstone, path)? { updated += 1; }
            }
        }
        for file in work {
            s.conn.execute(
                "INSERT INTO file_state(path,fingerprint,parser_version) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET fingerprint=excluded.fingerprint,parser_version=excluded.parser_version",
                params![file.path, file.fingerprint, codex::VERSION],
            )?;
        }
        Ok(())
    })?;
    Ok((imported, updated, skipped))
}
