//! Device-local reader caches. These never substitute for usage parsing or sync facts.
use crate::{
    harness::{files, HarnessAdapter},
    model::Session,
};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
}
impl FileStamp {
    fn read(path: &Path) -> Result<Self> {
        let m = std::fs::metadata(path)?;
        Ok(Self {
            len: m.len(),
            modified: m.modified().ok(),
            created: m.created().ok(),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    file: FileStamp,
    wal: Option<FileStamp>,
}
impl Stamp {
    fn read(path: &Path, sqlite: bool) -> Result<Self> {
        let wal = if sqlite {
            let wal = path.with_file_name(format!(
                "{}-wal",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
            match FileStamp::read(&wal) {
                Ok(stamp) => Some(stamp),
                Err(_) if !wal.exists() => None,
                Err(e) => return Err(e),
            }
        } else {
            None
        };
        Ok(Self {
            file: FileStamp::read(path)?,
            wal,
        })
    }
    fn token(&self) -> String {
        files::hash(format!("{self:?}").as_bytes())
    }
}
type MetaKey = (String, PathBuf);
type MetaEntries = HashMap<MetaKey, (Stamp, Vec<Session>)>;
#[derive(Clone, PartialEq, Eq)]
struct Key {
    harness: String,
    path: PathBuf,
    session: Option<String>,
}
pub struct Transcript {
    pub messages: Vec<Value>,
    sizes: Vec<usize>,
    full: HashMap<(usize, usize, String), String>,
    pub fingerprint: String,
    pub payload_bytes: usize,
    memory_bytes: usize,
}
impl Transcript {
    fn new(mut messages: Vec<Value>, fingerprint: String, session: Option<&str>) -> Self {
        let mut full = HashMap::new();
        for (mi, message) in messages.iter_mut().enumerate() {
            if let Some(blocks) = message["blocks"].as_array_mut() {
                for (bi, block) in blocks.iter_mut().enumerate() {
                    let (field, reference, limit) = match block["type"].as_str() {
                        Some("thinking") => ("text", "full", 400),
                        Some("tool_call") => ("inputPreview", "inputFull", 400),
                        Some("tool_result") => ("preview", "full", 1200),
                        _ => continue,
                    };
                    let Some(text) = block[field].as_str() else {
                        continue;
                    };
                    let preview: String = text.chars().take(limit).collect();
                    if preview.len() == text.len() {
                        continue;
                    }
                    full.insert((mi, bi, field.to_owned()), text.to_owned());
                    block[field] = json!(preview);
                    block[reference] = json!({"kind":"session_block","sessionId":session,"message":mi,"block":bi,"field":field,"fingerprint":fingerprint});
                    if field == "preview" {
                        block["truncated"] = json!(true);
                    }
                }
                if !blocks.is_empty() {
                    message.as_object_mut().unwrap().remove("content");
                }
            }
        }
        let sizes: Vec<usize> = messages
            .iter()
            .map(|m| serde_json::to_vec(m).map(|v| v.len()).unwrap_or(0))
            .collect();
        let payload_bytes = sizes.iter().sum();
        let memory_bytes = payload_bytes + full.values().map(String::len).sum::<usize>();
        Self {
            messages,
            sizes,
            full,
            fingerprint,
            payload_bytes,
            memory_bytes,
        }
    }
    pub fn restored(&self) -> Vec<Value> {
        let mut messages = self.messages.clone();
        for ((mi, bi, field), text) in &self.full {
            let block = &mut messages[*mi]["blocks"][*bi];
            block[field] = json!(text);
            block
                .as_object_mut()
                .unwrap()
                .remove(if field == "inputPreview" {
                    "inputFull"
                } else {
                    "full"
                });
            if field == "preview" {
                block["truncated"] = json!(false);
            }
        }
        messages
    }
    pub fn page(&self, offset: usize, cached: bool, parse_ms: u128) -> Result<Value> {
        anyhow::ensure!(offset <= self.messages.len(), "Invalid transcript offset");
        let mut end = offset;
        let mut bytes = 0;
        while end < self.messages.len() && end - offset < 150 {
            if end > offset && bytes + self.sizes[end] > 256 * 1024 {
                break;
            }
            bytes += self.sizes[end];
            end += 1;
        }
        Ok(
            json!({"header":{"type":"header","total":self.messages.len(),"turns":[],"cached":cached,"parseMs":parse_ms},"messages":self.messages[offset..end],"start":offset,"nextOffset":(end < self.messages.len()).then_some(end),"fingerprint":self.fingerprint,"payloadBytes":self.payload_bytes}),
        )
    }
    pub fn block(&self, reference: &Value, offset: usize, limit: usize) -> Result<Value> {
        anyhow::ensure!(
            reference["kind"] == "session_block" && reference["fingerprint"] == self.fingerprint,
            "Session changed; reload before reading full content"
        );
        let mi = reference["message"]
            .as_u64()
            .context("Missing message index")? as usize;
        let bi = reference["block"].as_u64().context("Missing block index")? as usize;
        let field = reference["field"].as_str().context("Missing block field")?;
        let text = self
            .full
            .get(&(mi, bi, field.to_owned()))
            .context("Unknown full-content reference")?;
        let total = text.chars().count();
        anyhow::ensure!(offset <= total, "Invalid content offset");
        let page: String = text
            .chars()
            .skip(offset)
            .take(limit.clamp(1, 512 * 1024))
            .collect();
        let next = offset + page.chars().count();
        Ok(
            json!({"text":page,"totalLen":total,"truncated":next<total,"nextOffset":(next<total).then_some(next)}),
        )
    }
}
struct Entry {
    key: Key,
    stamp: Stamp,
    transcript: Arc<Transcript>,
}
#[derive(Default)]
pub struct ReaderCache {
    metadata: Mutex<MetaEntries>,
    transcripts: Mutex<VecDeque<Entry>>,
}
pub struct Loaded {
    pub transcript: Arc<Transcript>,
    pub cached: bool,
    pub parse_ms: u128,
}
impl ReaderCache {
    pub fn metadata(&self, adapter: &dyn HarnessAdapter, path: &Path) -> Result<Vec<Session>> {
        let harness = adapter.descriptor().id;
        let key = (harness.to_owned(), path.to_owned());
        let stamp = Stamp::read(path, harness == "opencode")?;
        if let Some((old, rows)) = self
            .metadata
            .lock()
            .map_err(|_| anyhow::anyhow!("Reader cache poisoned"))?
            .get(&key)
        {
            if *old == stamp {
                return Ok(rows.clone());
            }
        }
        let rows = adapter.read_metadata(path)?;
        if stamp == Stamp::read(path, harness == "opencode")? {
            self.metadata
                .lock()
                .map_err(|_| anyhow::anyhow!("Reader cache poisoned"))?
                .insert(key, (stamp, rows.clone()));
        }
        Ok(rows)
    }
    pub fn retain_metadata(&self, keys: &HashSet<MetaKey>) {
        if let Ok(mut entries) = self.metadata.lock() {
            entries.retain(|key, _| keys.contains(key));
        }
    }
    pub fn load(
        &self,
        adapter: &dyn HarnessAdapter,
        path: &Path,
        session: Option<&str>,
        snapshot: Option<&str>,
    ) -> Result<Loaded> {
        let started = Instant::now();
        let key = Key {
            harness: adapter.descriptor().id.to_owned(),
            path: path.to_owned(),
            session: session.map(str::to_owned),
        };
        let stamp = Stamp::read(path, key.harness == "opencode")?;
        // Serialize expensive loads; metadata queries use a separate lock and remain responsive.
        let mut entries = self
            .transcripts
            .lock()
            .map_err(|_| anyhow::anyhow!("Reader cache poisoned"))?;
        if let Some(index) = entries.iter().position(|e| {
            e.key == key
                && match snapshot {
                    Some(token) => e.transcript.fingerprint == token,
                    None => e.stamp == stamp,
                }
        }) {
            let entry = entries.remove(index).unwrap();
            if entry.stamp == stamp || snapshot == Some(entry.transcript.fingerprint.as_str()) {
                let transcript = entry.transcript.clone();
                entries.push_back(entry);
                return Ok(Loaded {
                    transcript,
                    cached: true,
                    parse_ms: started.elapsed().as_millis(),
                });
            }
        }
        let messages = adapter.read_messages(path, session)?;
        // Pin a coherent read for later batches, even if the live log grows meanwhile.
        // A changed stamp never qualifies this snapshot for a fresh, unversioned read.
        let token = if stamp == Stamp::read(path, key.harness == "opencode")? {
            stamp.token()
        } else {
            uuid::Uuid::new_v4().to_string()
        };
        let transcript = Arc::new(Transcript::new(messages, token, session));
        if transcript.memory_bytes <= 96 * 1024 * 1024 {
            entries.push_back(Entry {
                key,
                stamp,
                transcript: transcript.clone(),
            });
            while entries.len() > 8
                || entries
                    .iter()
                    .map(|e| e.transcript.memory_bytes)
                    .sum::<usize>()
                    > 96 * 1024 * 1024
            {
                entries.pop_front();
            }
        }
        Ok(Loaded {
            transcript,
            cached: false,
            parse_ms: started.elapsed().as_millis(),
        })
    }
}
