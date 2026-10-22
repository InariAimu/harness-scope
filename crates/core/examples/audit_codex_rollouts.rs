//! Run the production Codex scanner against an explicitly supplied database copy.
//! No background worker, account refresh or network synchronization is started.
use anyhow::{Context, Result};
use harness_scope_core::Engine;
use serde_json::json;
use std::{env, fs, path::PathBuf};

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let database = PathBuf::from(args.next().context("database copy path required")?);
    let report_path = PathBuf::from(args.next().context("report path required")?);
    anyhow::ensure!(args.next().is_none(), "unexpected argument");
    let engine = Engine::open(&database)?;
    let before = engine.lock()?.local_records()?;
    let first = engine.scan_harness("codex", true)?;
    let after = engine.lock()?.local_records()?;
    let second = engine.scan_harness("codex", true)?;
    let final_records = engine.lock()?.local_records()?;
    let report = json!({
        "first": first, "second": second,
        "beforeActive": before.iter().filter(|r| !r.deleted).count(),
        "afterActive": after.iter().filter(|r| !r.deleted).count(),
        "nonCodexUnchanged": before.iter().filter(|r| r.harness != "codex").eq(after.iter().filter(|r| r.harness != "codex")),
        "repeatUnchanged": after == final_records,
        "schemaVersion": engine.lock()?.schema_version()?,
    });
    fs::write(report_path, serde_json::to_string_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
