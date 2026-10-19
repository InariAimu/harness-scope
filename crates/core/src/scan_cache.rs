//! Persistent read checkpoints, separate from committed usage facts.
use crate::harness::{files, HarnessAdapter};
use crate::{harness::TimelineIndex, store::Store};
use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::SystemTime,
};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    edges: String,
}
impl FileStamp {
    fn read(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let m = file.metadata()?;
        // Guard against edge rewrites and replacements on filesystems with coarse
        // timestamps. Size is essential on Windows while a writer holds a log open.
        let mut edges = Vec::with_capacity(8192);
        (&mut file).take(4096).read_to_end(&mut edges)?;
        if m.len() > 4096 {
            file.seek(SeekFrom::Start(m.len().saturating_sub(4096)))?;
            file.take(4096).read_to_end(&mut edges)?;
        }
        Ok(Self {
            len: m.len(),
            modified: m.modified()?,
            created: m.created().ok(),
            edges: files::hash(&edges),
        })
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Stamp {
    file: FileStamp,
    wal: Option<FileStamp>,
}
impl Stamp {
    pub fn read(path: &Path) -> Result<Self> {
        let wal = path.with_file_name(format!(
            "{}-wal",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        Ok(Self {
            file: FileStamp::read(path)?,
            wal: if wal.exists() {
                Some(FileStamp::read(&wal)?)
            } else {
                None
            },
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Checkpoint {
    stamp: Stamp,
    version: String,
    pub fingerprint: String,
    pub context_id: Option<String>,
    pub parent: Option<String>,
}
impl Checkpoint {
    pub fn matches(&self, stamp: &Stamp, adapter: &dyn HarnessAdapter) -> bool {
        self.stamp == *stamp && self.version == Self::version(adapter)
    }
    fn version(adapter: &dyn HarnessAdapter) -> String {
        format!("usage-read-v2:{}", adapter.descriptor().parser_version)
    }
    pub fn new(
        stamp: Stamp,
        adapter: &dyn HarnessAdapter,
        fingerprint: String,
        events: &[Value],
    ) -> Self {
        Self {
            stamp,
            version: Self::version(adapter),
            fingerprint,
            context_id: adapter.context_id(events),
            parent: adapter.parent_context(events),
        }
    }
}

pub(crate) fn save_context(store: &Store, id: &str, events: &[Value]) -> Result<()> {
    let payload = serde_json::to_string(events)?;
    store.conn.execute(
        "INSERT INTO usage_contexts(id,fingerprint,payload) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET fingerprint=excluded.fingerprint,payload=excluded.payload",
        params![id, files::hash(payload.as_bytes()), payload],
    )?;
    Ok(())
}

pub(crate) fn migrate_contexts(store: &Store) -> Result<()> {
    if store
        .meta::<bool>("usage_contexts_migrated")?
        .unwrap_or(false)
    {
        return Ok(());
    }
    // Preserve baselines for parent transcripts that have already been deleted.
    let old: TimelineIndex = store.meta("usage_timelines")?.unwrap_or_default();
    store.transaction(|| {
        for (id, events) in old {
            save_context(store, &id, &events)?;
        }
        store.set_meta("usage_contexts_migrated", &true)
    })
}
