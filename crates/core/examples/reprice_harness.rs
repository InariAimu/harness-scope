//! Back up an existing database and apply verified reference prices to one harness.
use anyhow::Result;
use harness_scope_core::{model::Price, pricing, Engine};
use rusqlite::{backup::Backup, Connection, OpenFlags};
use serde_json::json;
use std::{fs, path::PathBuf, time::Duration};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    anyhow::ensure!(
        args.len() == 4,
        "Usage: reprice_harness DB HARNESS PRICES_JSON BACKUP_DB (close desktop first)"
    );
    let database = PathBuf::from(&args[0]);
    let harness = args[1]
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid harness"))?;
    let price_path = PathBuf::from(&args[2]);
    let backup = PathBuf::from(&args[3]);
    anyhow::ensure!(
        database.is_file() && !backup.exists(),
        "Existing database and new backup path required"
    );
    let entries: Vec<Price> = serde_json::from_slice(&fs::read(price_path)?)?;
    for price in &entries {
        pricing::validate(price)?;
    }
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent)?;
    }
    let original = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut destination = Connection::open(&backup)?;
    Backup::new(&original, &mut destination)?.run_to_completion(
        1000,
        Duration::from_millis(5),
        None,
    )?;
    drop(destination);
    drop(original);
    let engine = Engine::open(&database)?;
    engine.registry.get(harness)?;
    {
        let store = engine.lock()?;
        let mut prices = store.prices()?;
        for entry in entries {
            prices.retain(|p| p.model_id != entry.model_id || p.provider != entry.provider);
            prices.push(entry);
        }
        store.set_meta("prices", &prices)?;
    }
    let changed = engine.execute("reprice_usage", json!({"harness":harness}))?;
    let repeated = engine.execute("reprice_usage", json!({"harness":harness}))?;
    anyhow::ensure!(repeated == 0, "Repeated pricing was not idempotent");
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "harness":harness,"changed":changed,"repeatChanged":repeated,"backup":backup,
            "summary":engine.execute("usage_query",json!({"kind":"summary","appType":harness}))?,
            "models":engine.execute("usage_query",json!({"kind":"models","appType":harness}))?
        }))?
    );
    Ok(())
}
