CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS records(device_id TEXT NOT NULL,id TEXT NOT NULL,revision INTEGER NOT NULL,payload TEXT NOT NULL,source_path TEXT NOT NULL,PRIMARY KEY(device_id,id));
CREATE INDEX IF NOT EXISTS records_source ON records(source_path);
CREATE TABLE IF NOT EXISTS record_timings(device_id TEXT NOT NULL,id TEXT NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(device_id,id));
CREATE TABLE IF NOT EXISTS outbox(id TEXT PRIMARY KEY,revision INTEGER NOT NULL,payload TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS remote_records(device_id TEXT NOT NULL,id TEXT NOT NULL,revision INTEGER NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(device_id,id));
CREATE TABLE IF NOT EXISTS file_state(path TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,parser_version TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS usage_read_cache(path TEXT PRIMARY KEY,payload TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS usage_contexts(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,payload TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS migration_sources(id TEXT PRIMARY KEY,cutover INTEGER NOT NULL,timezone TEXT NOT NULL,manifest TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS server_records(device_id TEXT NOT NULL,id TEXT NOT NULL,revision INTEGER NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(device_id,id));
CREATE TABLE IF NOT EXISTS server_changes(seq INTEGER PRIMARY KEY AUTOINCREMENT,device_id TEXT NOT NULL,id TEXT NOT NULL,UNIQUE(device_id,id));
CREATE TABLE IF NOT EXISTS devices(id TEXT PRIMARY KEY,name TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS usage_index(origin TEXT NOT NULL,device_id TEXT NOT NULL,id TEXT NOT NULL,harness TEXT NOT NULL,provider TEXT,model TEXT NOT NULL,occurred_at INTEGER NOT NULL,grain TEXT NOT NULL,request_count INTEGER NOT NULL,input_tokens INTEGER NOT NULL,output_tokens INTEGER NOT NULL,cache_read INTEGER NOT NULL,cache_write INTEGER NOT NULL,amount_usd TEXT,status_code INTEGER,record_source TEXT NOT NULL,legacy_date TEXT,legacy_timezone TEXT,cost_converted INTEGER NOT NULL,billing_basis TEXT,session_id TEXT,cost_method TEXT NOT NULL,deleted INTEGER NOT NULL,PRIMARY KEY(origin,device_id,id));
CREATE INDEX IF NOT EXISTS usage_time ON usage_index(occurred_at DESC,id) WHERE deleted=0;
CREATE INDEX IF NOT EXISTS usage_local_time ON usage_index(origin,occurred_at DESC,id) WHERE deleted=0;
CREATE INDEX IF NOT EXISTS usage_app_time ON usage_index(harness,occurred_at DESC,id) WHERE deleted=0;
CREATE INDEX IF NOT EXISTS usage_provider_time ON usage_index(provider,occurred_at DESC,id) WHERE deleted=0;
CREATE INDEX IF NOT EXISTS usage_model_time ON usage_index(model,occurred_at DESC,id) WHERE deleted=0;
CREATE INDEX IF NOT EXISTS usage_session ON usage_index(session_id,harness,occurred_at) WHERE deleted=0;
CREATE INDEX IF NOT EXISTS usage_request ON usage_index((device_id||'|'||id)) WHERE deleted=0;
CREATE INDEX IF NOT EXISTS usage_price_candidates ON usage_index(model,provider,amount_usd) WHERE origin='local' AND deleted=0 AND record_source='session' AND cost_method<>'reported';
CREATE TABLE IF NOT EXISTS usage_state(id INTEGER PRIMARY KEY CHECK(id=1),version INTEGER NOT NULL);
INSERT OR IGNORE INTO usage_state VALUES(1,0);
-- Successful local refreshes and shared history mirrors; separate from request usage facts.
CREATE TABLE IF NOT EXISTS provider_account_observations(seq INTEGER PRIMARY KEY AUTOINCREMENT,refresh_id TEXT NOT NULL UNIQUE,account_id TEXT NOT NULL,provider TEXT NOT NULL,observed_at_ms INTEGER NOT NULL,payload TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS provider_observations_account_seq ON provider_account_observations(account_id,seq DESC);
CREATE INDEX IF NOT EXISTS provider_observations_account_time ON provider_account_observations(account_id,observed_at_ms);
CREATE INDEX IF NOT EXISTS provider_observations_time ON provider_account_observations(observed_at_ms,seq);
-- Shared subscription facts are append-only. Credentials/settings remain device-local.
CREATE TABLE IF NOT EXISTS shared_subscriptions(id TEXT PRIMARY KEY,provider TEXT NOT NULL,name TEXT NOT NULL,pending INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS subscription_observations(id TEXT PRIMARY KEY,subscription_id TEXT NOT NULL,device_id TEXT NOT NULL,payload TEXT NOT NULL,pending INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS subscription_observations_subscription ON subscription_observations(subscription_id,id);
CREATE INDEX IF NOT EXISTS subscription_observations_pending ON subscription_observations(pending,id);
CREATE TABLE IF NOT EXISTS server_subscription_observations(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,payload TEXT NOT NULL);
-- Local rollout observations/aliases never become remote usage facts.
CREATE TABLE IF NOT EXISTS codex_source_cache(path TEXT PRIMARY KEY,payload TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS codex_event_bindings(event_id TEXT PRIMARY KEY,record_id TEXT NOT NULL UNIQUE);
CREATE TABLE IF NOT EXISTS codex_event_sources(event_id TEXT NOT NULL,source_path TEXT NOT NULL,PRIMARY KEY(event_id,source_path));
