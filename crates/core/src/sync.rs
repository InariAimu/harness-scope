use crate::{model::*, store::Store};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use rust_decimal::Decimal;
use std::str::FromStr;

pub(crate) fn gzip_json(value: &impl serde::Serialize) -> Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    serde_json::to_writer(&mut encoder, value)?;
    Ok(encoder.finish()?)
}

pub fn validate_record(r: &UsageRecord) -> Result<()> {
    if let Some(t) = &r.timing {
        let sources: (&[&str], &str) = match r.harness.as_str() {
            "codex" => (
                &[
                    "codex_token_usage_record",
                    "codex_model_output_before_tool",
                    "codex_token_count",
                ],
                "codex_item_started_at_ms",
            ),
            "opencode" => (
                &["opencode_time_streamed", "opencode_time_completed"],
                "opencode_content_start",
            ),
            "dsh" => (&["dsh_step_message"], "dsh_stream_delta"),
            _ => anyhow::bail!("Unsupported timing harness"),
        };
        anyhow::ensure!(
            r.grain == "event"
                && r.record_source == "session"
                && (100..=3_600_000).contains(&t.duration_ms)
                && t.first_token_ms
                    .is_none_or(|v| (0..=t.duration_ms).contains(&v))
                && sources.0.contains(&t.duration_source.as_str())
                && t.first_token_ms.is_some() == t.first_token_source.is_some()
                && t.first_token_source
                    .as_deref()
                    .is_none_or(|v| v == sources.1),
            "Invalid timing metadata"
        );
    }
    if let Some(m) = &r.request_metadata {
        anyhow::ensure!(
            r.harness == "codex"
                && r.grain == "event"
                && m.service_tier
                    .as_deref()
                    .is_none_or(RequestMetadata::valid_tier)
                && m.reasoning_effort
                    .as_deref()
                    .is_none_or(RequestMetadata::valid_effort)
                && m.service_tier.is_some() == m.service_tier_source.is_some()
                && m.service_tier_source.as_deref().is_none_or(|s| [
                    "thread_settings",
                    "turn_context"
                ]
                .contains(&s))
                && *m != RequestMetadata::default(),
            "Invalid request metadata"
        );
    }
    anyhow::ensure!(
        r.provider.as_deref().is_none_or(crate::providers::valid),
        "Invalid provider ID"
    );
    anyhow::ensure!(
        r.cost
            .billing_basis
            .as_deref()
            .is_none_or(|v| v == "subscription_usage" || v == "model_reference"),
        "Invalid cost billing basis"
    );
    anyhow::ensure!(
        !r.id.is_empty()
            && r.id.len() <= 512
            && r.device_id.len() <= 128
            && !r.device_id.is_empty(),
        "Invalid identity"
    );
    anyhow::ensure!(
        r.revision > 0 && r.revision < i64::MAX as u64,
        "Invalid revision"
    );
    anyhow::ensure!(
        r.tokens.total() <= 1_000_000_000_000
            && r.tokens.cache_write1h <= r.tokens.cache_write
            && r.tokens.reasoning <= r.tokens.output
            && r.count <= 1_000_000_000,
        "Invalid token/count range"
    );
    anyhow::ensure!(
        r.model.len() <= 512 && r.harness.len() <= 64 && r.parser_version.len() <= 128,
        "Metadata too long"
    );
    anyhow::ensure!(
        r.grain == "event" || r.grain == "legacy_day",
        "Invalid grain"
    );
    if let Some(s) = &r.cost.amount_usd {
        let d = Decimal::from_str(s)?;
        anyhow::ensure!(
            d >= Decimal::ZERO && d < Decimal::from(1_000_000_000u64),
            "Invalid amount"
        );
    }
    if let Some(p) = &r.cost.snapshot {
        crate::pricing::validate(p)?
    }
    if let Some(c) = &r.cost.conversion {
        anyhow::ensure!(
            crate::pricing::CURRENCIES.contains(&c.currency.as_str())
                && c.currency != "USD"
                && c.configured_at >= 0,
            "Invalid original currency"
        );
        let native = Decimal::from_str(&c.original_amount)?;
        anyhow::ensure!(
            native >= Decimal::ZERO && native < Decimal::from(1_000_000_000u64),
            "Invalid native amount"
        );
        anyhow::ensure!(
            r.cost
                .snapshot
                .as_ref()
                .is_some_and(|p| p.currency == c.currency),
            "Currency differs from price snapshot"
        );
        if let Some(rate) = &c.units_per_usd {
            let rate = crate::pricing::validate_rate(rate)?;
            anyhow::ensure!(
                r.cost
                    .amount_usd
                    .as_deref()
                    .is_some_and(|v| Decimal::from_str(v).ok() == Some(native / rate)),
                "Converted amount differs from FX snapshot"
            );
        } else {
            anyhow::ensure!(
                r.cost.amount_usd.is_none(),
                "Missing FX for converted amount"
            );
        }
    } else {
        anyhow::ensure!(
            r.cost.snapshot.as_ref().is_none_or(|p| p.currency == "USD"),
            "Non-USD estimate needs original amount"
        );
    }
    if let Some(c) = &r.cost.context {
        anyhow::ensure!(
            ["peak", "off_peak"].contains(&c.period.as_str())
                && c.timestamp > 0
                && c.time_basis == "log_timestamp"
                && c.calendar_version.len() <= 128,
            "Invalid pricing context"
        );
    }
    Ok(())
}
pub fn push(store: &Store, request: PushRequest) -> Result<PushResponse> {
    anyhow::ensure!(
        [1, 2, 3, 4].contains(&request.protocol_version),
        "Unsupported protocolVersion"
    );
    anyhow::ensure!(
        request.records.len() <= 200 && request.device_name.len() <= 128,
        "Batch too large"
    );
    for r in &request.records {
        anyhow::ensure!(
            r.device_id == request.device_id,
            "Record deviceId differs from batch"
        );
        validate_record(r)?;
        anyhow::ensure!(
            request.protocol_version >= 4 || r.timing.is_none(),
            "Timing requires protocol v4"
        );
        anyhow::ensure!(
            request.protocol_version >= 3 || r.request_metadata.is_none(),
            "Request metadata requires protocol v3"
        );
    }
    store.transaction(||{
        store.conn.execute("INSERT INTO devices(id,name) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET name=excluded.name",params![request.device_id,request.device_name])?;
        let mut accepted=Vec::new();
        for r in request.records{
            let previous:Option<String>=store.conn.query_row("SELECT payload FROM server_records WHERE device_id=?1 AND id=?2",params![r.device_id,r.id],|row|row.get(0)).optional()?;
            let old:Option<UsageRecord>=previous.map(|s|serde_json::from_str(&s)).transpose()?;
            anyhow::ensure!(request.protocol_version >= 4 || old.as_ref().is_none_or(|r|r.timing.is_none()), "Upgrade client to protocol v4 before editing records with timing");
            anyhow::ensure!(request.protocol_version >= 3 || old.as_ref().is_none_or(|r|r.request_metadata.is_none()), "Upgrade client to protocol v3 before editing records with request metadata");
            if let Some(old)=&old{if old.revision==r.revision{anyhow::ensure!(*old==r,"Revision conflict for {}",r.id)}}
            if old.as_ref().is_none_or(|old|r.revision>old.revision){
                let payload=serde_json::to_string(&r)?;
                store.conn.execute("INSERT INTO server_records(device_id,id,revision,payload) VALUES(?1,?2,?3,?4) ON CONFLICT(device_id,id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload",params![r.device_id,r.id,r.revision,payload])?;
                store.conn.execute("INSERT OR REPLACE INTO server_changes(device_id,id) VALUES(?1,?2)",params![r.device_id,r.id])?;
            }
            accepted.push(Ack{id:r.id,revision:r.revision});
        }
        Ok(PushResponse{server_instance_id:store.meta("server_instance_id")?.unwrap(),accepted})
    })
}
pub fn changes(store: &Store, after: i64, limit: usize) -> Result<ChangesResponse> {
    anyhow::ensure!(
        after >= 0 && limit > 0 && limit <= 500,
        "Invalid changes cursor/limit"
    );
    let mut stmt = store
        .conn
        .prepare("SELECT c.seq,r.payload FROM server_changes c JOIN server_records r ON r.device_id=c.device_id AND r.id=c.id WHERE c.seq>?1 ORDER BY c.seq LIMIT ?2")?;
    let rows = stmt
        .query_map(params![after, limit + 1], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let has_more = rows.len() > limit;
    let changes = rows
        .into_iter()
        .take(limit)
        .map(|(seq, p)| {
            Ok(Change {
                seq,
                record: serde_json::from_str(&p)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let next_cursor = changes.last().map(|c| c.seq).unwrap_or(after);
    let mut stmt = store
        .conn
        .prepare("SELECT id,name FROM devices ORDER BY name")?;
    let devices = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ChangesResponse {
        server_instance_id: store.meta("server_instance_id")?.unwrap(),
        changes,
        next_cursor,
        has_more,
        devices,
    })
}
pub fn apply_changes(store: &Store, response: &ChangesResponse) -> Result<()> {
    let own = store.device_id()?;
    store.transaction(||{
        for change in &response.changes{
            let r=&change.record;validate_record(r)?;
            if r.device_id==own{continue}
            let changed=store.conn.execute("INSERT INTO remote_records(device_id,id,revision,payload) VALUES(?1,?2,?3,?4) ON CONFLICT(device_id,id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload WHERE excluded.revision>remote_records.revision",params![r.device_id,r.id,r.revision,serde_json::to_string(r)?])?;
            if changed>0 { store.save_record_timing(&r.device_id,&r.id,r.timing.as_ref())?; }
        }
        store.set_meta("download_cursor",&response.next_cursor)?;
        store.set_meta("remote_instance_id",&response.server_instance_id)?;
        store.set_meta("devices",&response.devices)?;Ok(())
    })
}
