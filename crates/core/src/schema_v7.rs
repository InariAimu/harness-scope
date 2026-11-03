//! v6 -> v7 is additive: usage payloads, frozen quotes, revisions and cursors
//! remain untouched. Source-backed corrections happen only after a complete read.
use anyhow::Result;
use rusqlite::Connection;

pub(crate) fn migrate(conn: &Connection, old_version: i64) -> Result<()> {
    conn.execute_batch(include_str!("schema_v7.sql"))?;
    if old_version < 7 {
        conn.execute_batch("INSERT OR IGNORE INTO record_sources(device_id,record_id,source_path)
            SELECT device_id,id,source_path FROM records
            WHERE json_extract(payload,'$.deleted')=0 AND json_extract(payload,'$.recordSource')='session';
            DELETE FROM file_state WHERE parser_version LIKE 'claude-%' OR parser_version LIKE 'opencode-%' OR parser_version LIKE 'lmstudio-%';
            DELETE FROM usage_read_cache;")?;
    }
    Ok(())
}
