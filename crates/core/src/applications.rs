use crate::store::Store;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Local preferences only: never remove usage facts or change sync payloads.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct ApplicationSettings {
    pub collection_enabled: bool,
    pub visible: bool,
}

impl Default for ApplicationSettings {
    fn default() -> Self {
        Self {
            collection_enabled: true,
            visible: true,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Patch {
    pub collection_enabled: Option<bool>,
    pub visible: Option<bool>,
}

pub(crate) type Settings = BTreeMap<String, ApplicationSettings>;

pub(crate) fn load(store: &Store) -> Result<Settings> {
    Ok(store.meta("application_settings")?.unwrap_or_default())
}

pub(crate) fn get(settings: &Settings, harness: &str) -> ApplicationSettings {
    settings.get(harness).copied().unwrap_or_default()
}
