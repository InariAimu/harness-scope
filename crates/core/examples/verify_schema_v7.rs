//! Offline verification on an explicitly supplied copy under repository output/.
//! No background workers, account refresh or network synchronization are started.
use anyhow::{Context, Result};
use harness_scope_core::{model::UsageRecord, Engine};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let database = PathBuf::from(args.next().context("database copy required")?).canonicalize()?;
    let report_path = PathBuf::from(args.next().context("report path required")?);
    anyhow::ensure!(args.next().is_none(), "unexpected argument");
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../output")
        .canonicalize()?;
    anyhow::ensure!(
        database.starts_with(&output),
        "Only database copies inside repository output/ are accepted"
    );
    let conn = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    anyhow::ensure!(version == 6, "Expected an untouched v6 copy");
    let before = conn
        .prepare("SELECT payload FROM records ORDER BY device_id,id")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let meta = conn
        .prepare("SELECT key,value FROM meta ORDER BY key")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    drop(conn);
    let e = Engine::open(&database)?;
    let after = e
        .lock()?
        .conn
        .prepare("SELECT payload FROM records ORDER BY device_id,id")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    anyhow::ensure!(before == after, "DDL migration changed historical payloads");
    for (key, value) in &meta {
        let actual: String =
            e.lock()?
                .conn
                .query_row("SELECT value FROM meta WHERE key=?", [key], |r| r.get(0))?;
        anyhow::ensure!(
            &actual == value,
            "DDL migration changed existing metadata: {key}"
        );
    }
    let before: Vec<UsageRecord> = before
        .into_iter()
        .map(|p| serde_json::from_str(&p))
        .collect::<Result<_, _>>()?;
    let mut scans = Vec::new();
    for harness in ["claude", "opencode", "lmstudio", "codex"] {
        let a = e.scan_harness(harness, false)?;
        let first = e.lock()?.local_records()?;
        let b = e.scan_harness(harness, false)?;
        let second = e.lock()?.local_records()?;
        scans.push(json!({"harness":harness,"imported":a.imported,"updated":a.updated,"deferred":a.deferred_files,"errors":a.errors.len(),"repeatImported":b.imported,"repeatUpdated":b.updated,"repeatUnchanged":first==second}));
    }
    let final_rows = e.lock()?.local_records()?;
    let by_id: BTreeMap<_, _> = final_rows
        .iter()
        .map(|r| ((&r.device_id, &r.id), r))
        .collect();
    let mut missing = 0;
    let mut changed_quotes = 0;
    let mut retired = 0;
    for old in &before {
        let Some(new) = by_id.get(&(&old.device_id, &old.id)) else {
            missing += 1;
            continue;
        };
        if !old.deleted && new.deleted {
            retired += 1;
        }
        if old.cost.amount_usd.is_some()
            && old.model == new.model
            && old.tokens == new.tokens
            && old.provider == new.provider
            && old.cost.method != "reported"
            && old.cost != new.cost
        {
            changed_quotes += 1;
        }
    }
    let s = e.lock()?;
    let check: String = s.conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    let report = json!({"schemaBefore":version,"schemaAfter":s.schema_version()?,"ddlPayloadsUnchanged":true,"ddlExistingMetadataUnchanged":true,"historicalRows":before.len(),"missingHistoricalRows":missing,"changedFrozenQuotes":changed_quotes,"retiredDuplicatesOrInvalidated":retired,"scans":scans,"quickCheck":check});
    fs::write(report_path, serde_json::to_string_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    anyhow::ensure!(
        missing == 0 && changed_quotes == 0 && check == "ok",
        "History/quote verification failed"
    );
    Ok(())
}
