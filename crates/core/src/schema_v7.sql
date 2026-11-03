-- Physical observations are local; only canonical usage revisions are synced.
CREATE TABLE IF NOT EXISTS record_sources(device_id TEXT NOT NULL,record_id TEXT NOT NULL,source_path TEXT NOT NULL,PRIMARY KEY(device_id,record_id,source_path));
CREATE INDEX IF NOT EXISTS record_sources_path ON record_sources(source_path);
CREATE TABLE IF NOT EXISTS record_retire_candidates(device_id TEXT NOT NULL,record_id TEXT NOT NULL,PRIMARY KEY(device_id,record_id));
CREATE TABLE IF NOT EXISTS claude_event_bindings(event_id TEXT PRIMARY KEY,record_id TEXT NOT NULL UNIQUE);
