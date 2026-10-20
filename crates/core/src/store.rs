use crate::model::*;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::{path::Path, time::Duration};

pub struct Store {
    pub conn: Connection,
}
impl Store {
    pub(crate) fn reader(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        Ok(Self { conn })
    }
    pub fn usage_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT version FROM usage_state WHERE id=1", [], |r| {
                r.get(0)
            })?)
    }
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        anyhow::ensure!(
            version <= 3,
            "Database schema is newer than this application"
        );
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let migration = (|| -> Result<()> {
            conn.execute_batch(include_str!("schema.sql"))?;
            crate::query_store::install(&conn, version)?;
            conn.execute_batch("PRAGMA user_version=3;")?;
            Ok(())
        })();
        if let Err(error) = migration {
            conn.execute_batch("ROLLBACK")?;
            return Err(error);
        }
        conn.execute_batch("COMMIT")?;
        let store = Self { conn };
        if store.meta::<String>("device_id")?.is_none() {
            store.set_meta("device_id", &uuid::Uuid::new_v4().to_string())?
        }
        if store.meta::<String>("server_instance_id")?.is_none() {
            store.set_meta("server_instance_id", &uuid::Uuid::new_v4().to_string())?
        }
        if store.meta::<Config>("config")?.is_none() {
            store.set_meta("config", &Config::default())?
        }
        Ok(store)
    }
    pub fn meta<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let value: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key=?", [key], |r| r.get(0))
            .optional()?;
        value
            .map(|s| serde_json::from_str(&s).context("Invalid stored metadata"))
            .transpose()
    }
    pub fn set_meta<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.conn.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,serde_json::to_string(value)?])?;
        Ok(())
    }
    pub fn device_id(&self) -> Result<String> {
        Ok(self.meta("device_id")?.unwrap())
    }
    pub fn config(&self) -> Result<Config> {
        Ok(self.meta("config")?.unwrap())
    }
    pub fn prices(&self) -> Result<Vec<Price>> {
        let mut prices = self.meta("prices")?.unwrap_or_else(crate::pricing::builtin);
        crate::pricing::deepseek::install(&mut prices);
        crate::pricing::go_catalog::install(&mut prices, &crate::pricing::go_catalog::load(self)?);
        Ok(prices)
    }
    pub fn pricing_settings(&self) -> Result<PricingSettings> {
        Ok(self
            .meta("pricing_settings")?
            .unwrap_or_else(crate::pricing::deepseek::default_settings))
    }
    pub fn record(&self, id: &str) -> Result<Option<UsageRecord>> {
        let p: Option<String> = self
            .conn
            .query_row(
                "SELECT payload FROM records WHERE device_id=?1 AND id=?2",
                params![self.device_id()?, id],
                |r| r.get(0),
            )
            .optional()?;
        p.map(|p| serde_json::from_str(&p).map_err(Into::into))
            .transpose()
    }
    /// Call under a transaction when coordinating multiple facts and scan state.
    pub fn upsert(&self, mut record: UsageRecord, source_path: &str) -> Result<bool> {
        if let Some(old) = self.record(&record.id)? {
            record.revision = old.revision;
            if record == old {
                return Ok(false);
            }
            record.revision += 1;
        } else {
            record.revision = 1
        }
        let payload = serde_json::to_string(&record)?;
        self.conn.execute("INSERT INTO records(device_id,id,revision,payload,source_path) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(device_id,id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload,source_path=excluded.source_path",params![record.device_id,record.id,record.revision,payload,source_path])?;
        self.conn.execute("INSERT INTO outbox(id,revision,payload) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload",params![record.id,record.revision,payload])?;
        Ok(true)
    }
    pub fn local_records(&self) -> Result<Vec<UsageRecord>> {
        self.read_records("SELECT payload FROM records")
    }
    pub fn save_timing(&self, id: &str, timing: Option<&UsageTiming>) -> Result<()> {
        let device = self.device_id()?;
        if let Some(timing) = timing {
            self.conn.execute("INSERT INTO record_timings(device_id,id,payload) VALUES(?1,?2,?3) ON CONFLICT(device_id,id) DO UPDATE SET payload=excluded.payload WHERE record_timings.payload<>excluded.payload", params![device,id,serde_json::to_string(timing)?])?;
        } else {
            self.conn.execute(
                "DELETE FROM record_timings WHERE device_id=?1 AND id=?2",
                params![device, id],
            )?;
        }
        Ok(())
    }
    pub fn timings(&self) -> Result<std::collections::BTreeMap<(String, String), UsageTiming>> {
        let mut stmt = self
            .conn
            .prepare("SELECT device_id,id,payload FROM record_timings")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(device, id, payload)| Ok(((device, id), serde_json::from_str(&payload)?)))
            .collect()
    }
    fn read_records(&self, sql: &str) -> Result<Vec<UsageRecord>> {
        let mut stmt = self.conn.prepare(sql)?;
        let xs = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        xs.into_iter()
            .map(|p| serde_json::from_str(&p).map_err(Into::into))
            .collect()
    }
    pub fn records(&self, scope: &str) -> Result<Vec<UsageRecord>> {
        let own = self.device_id()?;
        let mut local = self.local_records()?;
        if scope != "local" && scope != own {
            local.extend(
                self.read_records("SELECT payload FROM remote_records")?
                    .into_iter()
                    .filter(|r| r.device_id != own),
            )
        }
        Ok(local
            .into_iter()
            .filter(|r| !r.deleted && (scope == "all" || scope == "local" || r.device_id == scope))
            .collect())
    }
    pub fn pending(&self) -> Result<Vec<UsageRecord>> {
        self.read_records("SELECT payload FROM outbox ORDER BY id LIMIT 200")
    }
    pub fn ack(&self, acks: &[Ack]) -> Result<()> {
        for ack in acks {
            self.conn.execute(
                "DELETE FROM outbox WHERE id=?1 AND revision=?2",
                params![ack.id, ack.revision],
            )?;
        }
        Ok(())
    }
    pub fn pending_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0))?)
    }
    pub fn requeue_all(&self) -> Result<()> {
        self.conn.execute("INSERT INTO outbox(id,revision,payload) SELECT id,revision,payload FROM records WHERE device_id=?1 ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload",[self.device_id()?])?;
        Ok(())
    }
    pub fn transaction<T>(&self, run: impl FnOnce() -> Result<T>) -> Result<T> {
        self.conn.execute_batch("SAVEPOINT operation")?;
        match run() {
            Ok(value) => {
                self.conn.execute_batch("RELEASE operation")?;
                Ok(value)
            }
            Err(e) => {
                let _ = self
                    .conn
                    .execute_batch("ROLLBACK TO operation; RELEASE operation");
                Err(e)
            }
        }
    }
}
