//! Transactional v4 backfill and a coalesced change index with stable cursor ordering.
use crate::model::{UsageRecord, UsageTiming};
use anyhow::Result;
use rusqlite::{params, Connection};

pub(crate) fn install(conn: &Connection, old_version: i64) -> Result<()> {
    if old_version >= 4 {
        return Ok(());
    }
    let legacy_changes: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('server_changes') WHERE name='payload')",
        [],
        |row| row.get(0),
    )?;
    if legacy_changes {
        // Keep the newest sequence for every identity, including deletion tombstones.
        // Every superseded event has a later replacement, so old cursors remain valid.
        conn.execute_batch("ALTER TABLE server_changes RENAME TO server_changes_v3;
            CREATE TABLE server_changes(seq INTEGER PRIMARY KEY AUTOINCREMENT,device_id TEXT NOT NULL,id TEXT NOT NULL,UNIQUE(device_id,id));
            INSERT INTO server_changes(seq,device_id,id) SELECT MAX(seq),device_id,id FROM server_changes_v3 GROUP BY device_id,id;
            UPDATE sqlite_sequence SET seq=MAX(seq,COALESCE((SELECT seq FROM sqlite_sequence WHERE name='server_changes_v3'),0)) WHERE name='server_changes';
            INSERT INTO sqlite_sequence(name,seq) SELECT 'server_changes',seq FROM sqlite_sequence WHERE name='server_changes_v3' AND NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='server_changes');
            DROP TABLE server_changes_v3;")?;
    }
    let rows = conn.prepare("SELECT r.payload,t.payload FROM records r JOIN record_timings t ON t.device_id=r.device_id AND t.id=r.id WHERE r.device_id=(SELECT json_extract(value,'$') FROM meta WHERE key='device_id')")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?,row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (payload, timing) in rows {
        let mut record: UsageRecord = serde_json::from_str(&payload)?;
        let timing: UsageTiming = serde_json::from_str(&timing)?;
        if record.deleted || record.timing.as_ref() == Some(&timing) {
            continue;
        }
        record.timing = Some(timing);
        record.revision = record
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Revision overflow"))?;
        crate::sync::validate_record(&record)?;
        let payload = serde_json::to_string(&record)?;
        conn.execute(
            "UPDATE records SET revision=?3,payload=?4 WHERE device_id=?1 AND id=?2",
            params![record.device_id, record.id, record.revision, payload],
        )?;
        conn.execute("INSERT INTO outbox(id,revision,payload) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload",params![record.id,record.revision,payload])?;
    }
    Ok(())
}
