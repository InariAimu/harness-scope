//! Device-local query projection. Original JSON remains the revision/sync authority.
use crate::{
    model::{RequestMetadata, Tokens, UsageTiming},
    store::Store,
};
use anyhow::Result;
use rusqlite::{params_from_iter, types::Value as SqlValue, Connection};
use serde_json::Value;

pub(crate) const COLUMNS: &str = "origin,device_id,id,harness,provider,model,occurred_at,grain,request_count,input_tokens,output_tokens,cache_read,cache_write,amount_usd,status_code,record_source,legacy_date,legacy_timezone,cost_converted,billing_basis,session_id,cost_method,deleted,request_metadata";

pub(crate) fn install(conn: &Connection, old_version: i64) -> Result<()> {
    if old_version < 3 {
        conn.execute_batch("ALTER TABLE usage_index ADD COLUMN request_metadata TEXT;")?;
    }
    for (table, origin) in [("records", "local"), ("remote_records", "remote")] {
        let values = |p: &str, device: &str, id: &str| {
            format!("'{origin}',{device},{id},json_extract({p},'$.harness'),json_extract({p},'$.provider'),json_extract({p},'$.model'),json_extract({p},'$.occurredAt'),json_extract({p},'$.grain'),json_extract({p},'$.count'),json_extract({p},'$.tokens.input'),json_extract({p},'$.tokens.output'),json_extract({p},'$.tokens.cacheRead'),json_extract({p},'$.tokens.cacheWrite'),json_extract({p},'$.cost.amountUsd'),json_extract({p},'$.statusCode'),json_extract({p},'$.recordSource'),json_extract({p},'$.legacyDate'),json_extract({p},'$.legacyTimezone'),json_extract({p},'$.cost.conversion.unitsPerUsd') IS NOT NULL,json_extract({p},'$.cost.billingBasis'),json_extract({p},'$.sessionId'),json_extract({p},'$.cost.method'),json_extract({p},'$.deleted')")
        };
        let values = |p: &str, device: &str, id: &str| {
            format!(
                "{},json_extract({p},'$.requestMetadata')",
                values(p, device, id)
            )
        };
        let new = values("NEW.payload", "NEW.device_id", "NEW.id");
        for (operation, remove) in [("INSERT", "".to_owned()), ("UPDATE", format!("DELETE FROM usage_index WHERE origin='{origin}' AND device_id=OLD.device_id AND id=OLD.id;"))] {
            if old_version < 3 { conn.execute_batch(&format!("DROP TRIGGER IF EXISTS usage_index_{table}_{operation};"))?; }
            conn.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS usage_index_{table}_{operation} AFTER {operation} ON {table} BEGIN {remove} INSERT OR REPLACE INTO usage_index({COLUMNS}) SELECT {new}; UPDATE usage_state SET version=version+1 WHERE id=1; END;"))?;
        }
        conn.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS usage_index_{table}_DELETE AFTER DELETE ON {table} BEGIN DELETE FROM usage_index WHERE origin='{origin}' AND device_id=OLD.device_id AND id=OLD.id; UPDATE usage_state SET version=version+1 WHERE id=1; END;"))?;
        if old_version < 3 {
            conn.execute_batch(&format!(
                "INSERT OR REPLACE INTO usage_index({COLUMNS}) SELECT {} FROM {table};",
                values("payload", "device_id", "id")
            ))?;
        }
    }
    if old_version < 3 {
        conn.execute_batch("UPDATE usage_state SET version=version+1 WHERE id=1;")?;
    }
    for operation in ["INSERT", "UPDATE", "DELETE"] {
        conn.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS usage_timing_{operation} AFTER {operation} ON record_timings BEGIN UPDATE usage_state SET version=version+1 WHERE id=1; END;"))?;
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) struct Row {
    pub request_metadata: Option<RequestMetadata>,
    pub harness: String,
    pub provider: Option<String>,
    pub model: String,
    pub occurred_at: i64,
    pub grain: String,
    pub count: u64,
    pub tokens: Tokens,
    pub amount: Option<String>,
    pub status_code: Option<u16>,
    pub record_source: String,
    pub legacy_date: Option<String>,
    pub legacy_timezone: Option<String>,
    pub converted: bool,
    pub billing_basis: Option<String>,
    pub timing: Option<UsageTiming>,
}
impl Row {
    pub fn provider_name(&self) -> String {
        self.provider
            .clone()
            .unwrap_or_else(|| format!("{} (Session)", self.harness))
    }
}

pub(crate) struct Selection {
    pub sql: String,
    pub params: Vec<SqlValue>,
}
impl Selection {
    pub fn new(store: &Store, args: &Value, start: i64, end: i64) -> Result<Self> {
        let own = store.device_id()?;
        let scope = args["scope"].as_str().unwrap_or("local");
        let mut selection = Self {
            sql: "u.deleted=0 AND u.occurred_at>=? AND u.occurred_at<=?".into(),
            params: vec![start.into(), end.into()],
        };
        if scope == "local" || scope == own {
            selection.sql.push_str(" AND u.origin='local'");
        } else {
            selection
                .sql
                .push_str(" AND (u.origin='local' OR u.device_id<>?)");
            selection.params.push(own.into());
        }
        if scope != "local" && scope != "all" {
            selection.equal("u.device_id", scope);
        }
        let filter = args.get("filters").unwrap_or(args);
        if let Some(app) = filter["appType"].as_str().filter(|s| *s != "all") {
            selection.equal("u.harness", app);
        }
        if let Some(model) = filter["model"].as_str() {
            selection.equal("u.model", model);
        }
        if let Some(provider) = filter["providerName"].as_str() {
            selection.sql.push_str(" AND (u.provider=? OR (u.provider IS NULL AND (u.harness=? OR u.harness||' (Session)'=?)))");
            selection.params.extend([
                provider.to_owned().into(),
                provider.to_owned().into(),
                provider.to_owned().into(),
            ]);
        }
        if let Some(status) = filter["statusCode"].as_u64() {
            if status > u16::MAX as u64 {
                selection.sql.push_str(" AND 0");
            } else {
                selection.sql.push_str(" AND u.status_code=?");
                selection.params.push((status as i64).into());
            }
        }
        if let Some(session) = args["sessionId"].as_str() {
            selection.equal("u.session_id", session);
        }
        Ok(selection)
    }
    pub fn equal(&mut self, column: &str, value: &str) {
        self.sql.push_str(&format!(" AND {column}=?"));
        self.params.push(value.to_owned().into());
    }
    pub fn rows(&self, store: &Store, with_timing: bool) -> Result<Vec<Row>> {
        self.select_rows(store, with_timing, false)
    }
    pub fn rows_with_metadata(&self, store: &Store) -> Result<Vec<Row>> {
        self.select_rows(store, false, true)
    }
    fn select_rows(
        &self,
        store: &Store,
        with_timing: bool,
        with_metadata: bool,
    ) -> Result<Vec<Row>> {
        let join = if with_timing {
            "LEFT JOIN record_timings t ON t.device_id=u.device_id AND t.id=u.id"
        } else {
            ""
        };
        let timing = if with_timing { "t.payload" } else { "NULL" };
        let metadata = if with_metadata {
            "u.request_metadata"
        } else {
            "NULL"
        };
        let mut stmt = store.conn.prepare_cached(&format!("SELECT u.harness,u.provider,u.model,u.occurred_at,u.grain,u.request_count,u.input_tokens,u.output_tokens,u.cache_read,u.cache_write,u.amount_usd,u.status_code,u.record_source,u.legacy_date,u.legacy_timezone,u.cost_converted,u.billing_basis,{timing},{metadata} FROM usage_index u {join} WHERE {}", self.sql))?;
        let rows = stmt
            .query_map(params_from_iter(self.params.iter()), |r| {
                Ok(Row {
                    harness: r.get(0)?,
                    provider: r.get(1)?,
                    model: r.get(2)?,
                    occurred_at: r.get(3)?,
                    grain: r.get(4)?,
                    count: r.get(5)?,
                    tokens: Tokens {
                        input: r.get(6)?,
                        output: r.get(7)?,
                        cache_read: r.get(8)?,
                        cache_write: r.get(9)?,
                        ..Default::default()
                    },
                    amount: r.get(10)?,
                    status_code: r.get(11)?,
                    record_source: r.get(12)?,
                    legacy_date: r.get(13)?,
                    legacy_timezone: r.get(14)?,
                    converted: r.get(15)?,
                    billing_basis: r.get(16)?,
                    timing: None,
                    request_metadata: None,
                })
                .and_then(|mut row| {
                    let timing: Option<String> = r.get(17)?;
                    let metadata: Option<String> = r.get(18)?;
                    row.request_metadata = metadata
                        .map(|s| serde_json::from_str(&s))
                        .transpose()
                        .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            18,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                    row.timing = timing
                        .map(|s| serde_json::from_str(&s))
                        .transpose()
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                17,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?;
                    Ok(row)
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
