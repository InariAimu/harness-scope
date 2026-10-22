//! Presentation order stays separate from accounts and in-flight quota refreshes.
use super::{load, Stored};
use crate::{store::Store, Engine};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

const META: &str = "provider_sidebar_order_v1";

fn ordered_ids(store: &Store, accounts: &[Stored]) -> Result<Vec<String>> {
    let known: HashSet<_> = accounts.iter().map(|a| a.account.id.as_str()).collect();
    let mut seen = HashSet::new();
    Ok(store
        .meta::<Vec<String>>(META)?
        .unwrap_or_default()
        .into_iter()
        .chain(accounts.iter().map(|a| a.account.id.clone()))
        .filter(|id| known.contains(id.as_str()) && seen.insert(id.clone()))
        .collect())
}

pub(super) fn get(store: &Store) -> Result<Value> {
    Ok(json!(ordered_ids(store, &load(store)?)?))
}

pub(super) fn save(engine: &Engine, args: Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        ids: Vec<String>,
    }
    let input: Input = serde_json::from_value(args)?;
    anyhow::ensure!(input.ids.len() <= 100, "Too many provider account IDs");
    let store = engine.lock()?;
    let accounts = load(&store)?;
    let known: HashSet<_> = accounts.iter().map(|a| a.account.id.as_str()).collect();
    let mut seen = HashSet::new();
    for id in &input.ids {
        anyhow::ensure!(seen.insert(id.as_str()), "Duplicate provider account ID");
        anyhow::ensure!(
            known.contains(id.as_str()),
            "Provider accounts changed; refresh and retry"
        );
    }
    // Accounts created since the editor opened retain their current relative order.
    let mut ids = input.ids.clone();
    ids.extend(
        ordered_ids(&store, &accounts)?
            .into_iter()
            .filter(|id| !seen.contains(id.as_str())),
    );
    store.set_meta(META, &ids)?;
    Ok(json!(ids))
}
