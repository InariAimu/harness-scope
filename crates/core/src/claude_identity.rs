//! Bind new request identities to existing local facts without replacing quotes.
use crate::{
    model::{ParsedUsage, UsageRecord},
    store::Store,
};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};

pub(crate) fn bind(store: &Store, parsed: &mut ParsedUsage, path: &str) -> Result<()> {
    let event = parsed.id.clone();
    let bound: Option<String> = store
        .conn
        .query_row(
            "SELECT record_id FROM claude_event_bindings WHERE event_id=?",
            [&event],
            |r| r.get(0),
        )
        .optional()?;
    let record_id = if let Some(id) = bound {
        id
    } else {
        // Only a matching observed v6 fact can supply an old identity/quote.
        // A reused gateway message ID must not donate one request's quote to another.
        let payloads = store.conn.prepare("SELECT r.payload FROM records r JOIN record_sources s ON s.device_id=r.device_id AND s.record_id=r.id WHERE s.source_path=?1 AND json_extract(r.payload,'$.harness')='claude' AND json_extract(r.payload,'$.deleted')=0 AND NOT EXISTS(SELECT 1 FROM claude_event_bindings b WHERE b.record_id=r.id) ORDER BY r.id")?
            .query_map([path], |r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
        let candidates = payloads
            .into_iter()
            .map(|p| serde_json::from_str::<UsageRecord>(&p))
            .collect::<Result<Vec<_>, _>>()?;
        candidates
            .into_iter()
            .find(|r| {
                let mut tokens = parsed.tokens.clone();
                tokens.reasoning = r.tokens.reasoning;
                r.native_event_id.as_deref() == Some(&parsed.native_event_id)
                    && r.model == parsed.model
                    && r.tokens == tokens
                    && r.occurred_at == parsed.occurred_at
            })
            .map(|r| r.id)
            .unwrap_or_else(|| event.clone())
    };
    store.conn.execute(
        "INSERT OR IGNORE INTO claude_event_bindings(event_id,record_id) VALUES(?1,?2)",
        params![event, record_id],
    )?;
    parsed.id = record_id;
    // Keep attribution stable across copies. An earlier streaming snapshot in
    // another file cannot roll back the completed request.
    if let Some(old) = store
        .record(&parsed.id)?
        .filter(|r| !r.deleted && r.model == parsed.model)
    {
        if let Some(session) = old.session_id {
            parsed.session_id = session;
        }
        if old.tokens.total() > parsed.tokens.total() {
            parsed.tokens = old.tokens;
            parsed.occurred_at = old.occurred_at;
        }
        parsed.timing = parsed.timing.take().or(old.timing);
        parsed.request_metadata = parsed.request_metadata.take().or(old.request_metadata);
    }
    Ok(())
}
