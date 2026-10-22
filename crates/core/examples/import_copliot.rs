//! Enable new VS Code Copilot sessions outside the fixed historical selection. The desktop must be closed first.
use anyhow::{Context, Result};
use harness_scope_core::{
    harness::{
        copliot::ID,
        copliot_deprecated::{Selection, SELECTION_FILE},
    },
    model::Source,
    Engine,
};
use rusqlite::{backup::Backup, Connection, OpenFlags};
use serde_json::json;
use std::{fs, path::PathBuf};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    anyhow::ensure!(
        args.len() == 3,
        "Usage: import_copliot DB CODE_USER_ROOT BACKUP_DB"
    );
    let (database, root, backup) = (&args[0], &args[1], &args[2]);
    anyhow::ensure!(
        root.is_absolute() && root.is_dir(),
        "An absolute VS Code User directory is required"
    );
    anyhow::ensure!(database.is_file(), "Existing application database required");
    anyhow::ensure!(!backup.exists(), "Backup already exists");
    let selection: Selection =
        serde_json::from_slice(&fs::read(root.join(SELECTION_FILE)).context(
            "The historical selection must be fixed before importing new Copilot sessions",
        )?)?;
    selection.validate()?;
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent)?;
    }
    let original = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut backup_connection = Connection::open(backup)?;
    Backup::new(&original, &mut backup_connection)?.run_to_completion(
        1000,
        std::time::Duration::from_millis(5),
        None,
    )?;
    drop(backup_connection);
    drop(original);
    let engine = Engine::open(database)?;
    {
        let store = engine.lock()?;
        let mut config = store.config()?;
        let root = root.to_string_lossy().into_owned();
        if let Some(source) = config
            .sources
            .iter_mut()
            .find(|s| s.harness == ID && s.root == root)
        {
            source.enabled = true;
        } else {
            let mut index = 0;
            while config
                .sources
                .iter()
                .any(|s| s.id == format!("{ID}-{index}"))
            {
                index += 1;
            }
            config.sources.push(Source {
                id: format!("{ID}-{index}"),
                harness: ID.into(),
                root,
                enabled: true,
            });
        }
        store.set_meta("config", &config)?;
    }
    let report = engine.scan_harness(ID, false)?;
    let repeated = engine.scan_harness(ID, false)?;
    let store = engine.lock()?;
    let records = store
        .local_records()?
        .into_iter()
        .filter(|r| r.harness == ID && !r.deleted)
        .collect::<Vec<_>>();
    anyhow::ensure!(
        repeated.imported == 0 && repeated.updated == 0,
        "Repeat import was not idempotent"
    );
    anyhow::ensure!(
        records.iter().all(|r| r.provider.is_some()
            && (r.cost.amount_usd.is_none()
                || (r.tokens.total() > 0
                    && r.cost.method == "estimated"
                    && r.cost.billing_basis.as_deref() == Some("model_reference")))),
        "Unexpected provider/cost projection"
    );
    let summary = harness_scope_core::query::summary(&records);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "harness":ID,"excludedHistoricalSessions":selection.session_ids.len(),"report":report,"repeatReport":repeated,
            "summary":summary,"schemaVersion":store.schema_version()?,
            "pending":store.pending_count()?,"backup":backup
        }))?
    );
    store
        .conn
        .execute_batch("PRAGMA wal_checkpoint(PASSIVE)")
        .context("Checkpoint imported usage")?;
    Ok(())
}
