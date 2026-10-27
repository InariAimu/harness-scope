//! Adapters own formats, not persistence, pricing, networking or application state.
pub mod cherry;
pub mod claude;
mod claude_requests;
pub mod codex;
mod codex_settings;
mod codex_timing;
pub(crate) mod codex_usage;
pub mod copliot;
pub mod copliot_deprecated;
pub mod dsh;
pub mod files;
pub mod opencode;
mod transcript;
mod usage_read;

use crate::model::{ParsedUsage, Session};
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub parser_version: &'static str,
    pub default_roots: Vec<String>,
    pub usage: bool,
    pub transcript: bool,
    pub delete: bool,
}
#[derive(Default)]
pub struct ParseResult {
    pub usage: Vec<ParsedUsage>,
    pub diagnostics: Vec<String>,
    pub complete: bool,
}
pub type TimelineIndex = HashMap<String, Vec<Value>>;

pub trait HarnessAdapter: Send + Sync {
    fn descriptor(&self) -> Descriptor;
    fn allows_price_estimates(&self, _has_tokens: bool) -> bool {
        true
    }
    fn parse(&self, path: &Path, lines: &[Value], index: &TimelineIndex) -> Result<ParseResult>;
    fn session(&self, path: &Path, lines: &[Value]) -> Option<Session>;
    fn messages(&self, lines: &[Value]) -> Vec<Value>;
    fn sessions(&self, path: &Path, events: &[Value]) -> Vec<Session> {
        self.session(path, events).into_iter().collect()
    }
    fn messages_for_session(&self, events: &[Value], _session_id: Option<&str>) -> Vec<Value> {
        self.messages(events)
    }
    fn delete_session(&self, path: &Path, _session_id: &str) -> Result<()> {
        self.delete_source(path)
    }
    fn discover(&self, root: &Path) -> Result<Vec<PathBuf>> {
        files::discover(root)
    }
    /// Override for SQLite/native formats; the engine does not decode a harness format.
    fn read_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        files::read(path)
    }
    fn read_usage_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        self.read_source(path)
    }
    /// Bump when a usage projection changes independently of accounting rules.
    fn usage_reader_version(&self) -> &'static str {
        "usage-read-v2"
    }
    /// Reader projections are independent of complete, hash-verified usage ingestion.
    fn read_metadata(&self, path: &Path) -> Result<Vec<Session>> {
        let (events, _, _, _) = self.read_source(path)?;
        Ok(self.sessions(path, &events))
    }
    /// Apply optional native list metadata once per source, outside the log cache.
    fn enrich_metadata(&self, _root: &Path, _sessions: &mut [Session]) {}
    fn read_messages(&self, path: &Path, session: Option<&str>) -> Result<Vec<Value>> {
        let (events, _, errors, complete) = self.read_source(path)?;
        anyhow::ensure!(
            complete && errors.is_empty(),
            "Session contains incomplete or invalid data; retry after writing finishes"
        );
        Ok(self.messages_for_session(&events, session))
    }
    fn context_id(&self, _events: &[Value]) -> Option<String> {
        None
    }
    fn context_events(&self, _events: &[Value]) -> Vec<Value> {
        Vec::new()
    }
    fn parent_context(&self, _events: &[Value]) -> Option<String> {
        None
    }
    fn delete_source(&self, path: &Path) -> Result<()> {
        anyhow::ensure!(
            self.descriptor().delete,
            "This harness does not support deletion"
        );
        std::fs::remove_file(path)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct Registry {
    adapters: Vec<Arc<dyn HarnessAdapter>>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            adapters: vec![
                Arc::new(claude::Claude),
                Arc::new(codex::Codex),
                Arc::new(opencode::OpenCode),
                Arc::new(cherry::Cherry),
                Arc::new(dsh::Dsh),
                Arc::new(copliot_deprecated::CopliotDeprecated),
                Arc::new(copliot::Copliot),
            ],
        }
    }
}
impl Registry {
    pub fn new(adapters: Vec<Arc<dyn HarnessAdapter>>) -> Self {
        Self { adapters }
    }
    pub fn get(&self, id: &str) -> Result<&dyn HarnessAdapter> {
        self.adapters
            .iter()
            .find(|a| a.descriptor().id == id)
            .map(|a| a.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Unsupported harness: {id}"))
    }
    pub fn descriptors(&self) -> Vec<Descriptor> {
        self.adapters.iter().map(|a| a.descriptor()).collect()
    }
}
