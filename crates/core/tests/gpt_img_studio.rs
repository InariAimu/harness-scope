use harness_scope_core::{
    harness::{gpt_img_studio::GptImgStudio, HarnessAdapter, TimelineIndex},
    model::*,
    store::Store,
    sync, Engine,
};
use serde_json::{json, Value};
use std::{fs, path::Path};
const FIXTURE: &str = include_str!("../../../tests/fixtures/gpt-img-studio/model-usage.jsonl");
fn rows() -> Vec<Value> {
    FIXTURE
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
fn engine(root: &Path) -> Engine {
    fs::write(root.join("models.jsonl"), FIXTURE).unwrap();
    let e = Engine::open(&root.join("scope.db")).unwrap();
    let s = e.lock().unwrap();
    let mut c = s.config().unwrap();
    c.sources = vec![Source {
        id: "fixture".into(),
        harness: "gpt-img-studio".into(),
        root: root.to_string_lossy().into(),
        enabled: true,
    }];
    c.server_url.clear();
    c.server_token.clear();
    c.timezone = "UTC".into();
    s.set_meta("config", &c).unwrap();
    drop(s);
    e
}
fn scan(e: &Engine) {
    let r = e.scan(false).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
}
fn q(e: &Engine, kind: &str) -> Value {
    e.execute("usage_query", json!({"kind":kind})).unwrap()
}

#[test]
fn model_tokens_frozen_quotes_and_http_request_statistics_stay_separate() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    scan(&e);
    let original = e.lock().unwrap().local_records().unwrap();
    assert_eq!(original.len(), 2);
    for r in &original {
        sync::validate_record(r).unwrap();
        assert_eq!(r.count, 0);
        assert!(r.timing.is_none());
    }
    let s = q(&e, "summary");
    assert_eq!(s["totalRequests"], 0);
    assert_eq!(s["modelUsageCount"], 2);
    assert_eq!(s["realTotalTokens"], 2200);
    assert_eq!(s["totalCost"], "0.01188");
    assert_eq!(s["upperBoundCount"], 1);
    assert_eq!(s["cacheBreakdownKnown"], false);
    assert_eq!(s["cacheHitRateInputTokens"], 1000);
    assert_eq!(s["cacheHitRateReadTokens"], 800);
    assert_eq!(s["cacheHitRate"], 0.8);
    assert!(q(&e, "models")
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["avgCostPerRequest"].is_null()));
    assert_eq!(e.execute("reprice_usage", json!({})).unwrap(), 0);
    scan(&e);
    assert_eq!(e.lock().unwrap().local_records().unwrap(), original);
    let mut http = original
        .iter()
        .find(|r| r.model == "synthetic-language")
        .unwrap()
        .clone();
    http.id = "synthetic-http".into();
    http.harness = "claude".into();
    http.model_usage = None;
    http.count = 1;
    http.cost = Cost {
        amount_usd: Some("1.25".into()),
        method: "reported".into(),
        ..Default::default()
    };
    e.lock().unwrap().upsert(http, "synthetic").unwrap();
    let models = q(&e, "models");
    let language = models
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["model"] == "synthetic-language" && r["appType"] == "all")
        .unwrap();
    assert_eq!(language["avgCostPerRequest"], "1.25");
    assert_eq!(q(&e, "summary")["totalRequests"], 1);
    drop(e);
    let e = Engine::open(&dir.path().join("scope.db")).unwrap();
    scan(&e);
    assert_eq!(q(&e, "summary")["modelUsageCount"], 2);
}

#[test]
fn cache_hit_rate_excludes_unknown_records_and_keeps_known_records_in_the_same_app() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    scan(&e);
    let original = e.lock().unwrap().local_records().unwrap();
    let direct = harness_scope_core::query::summary(&original);
    assert_eq!(direct["cacheHitRate"], 0.8);
    let apps = q(&e, "summaryByApp");
    assert_eq!(apps[0]["summary"]["cacheHitRate"], 0.8);
    assert_eq!(apps[0]["summary"]["realTotalTokens"], 2200);

    let unknown = e
        .execute(
            "usage_query",
            json!({"kind":"summary", "model":"synthetic-image"}),
        )
        .unwrap();
    assert!(unknown["cacheHitRate"].is_null());
    assert_eq!(unknown["cacheHitRateInputTokens"], 0);
    assert_eq!(unknown["realTotalTokens"], 1100);

    let mut http = original
        .iter()
        .find(|r| r.model == "synthetic-language")
        .unwrap()
        .clone();
    http.id = "cache-miss-http".into();
    http.harness = "claude".into();
    http.model_usage = None;
    http.count = 1;
    http.tokens = Tokens {
        input: 100,
        output: 100,
        cache_write: 100,
        ..Default::default()
    };
    let mut local = http.clone();
    local.id = "unknown-local-cache".into();
    local.harness = "lmstudio".into();
    local.tokens = Tokens {
        input: 90_000,
        output: 100,
        ..Default::default()
    };
    let store = e.lock().unwrap();
    store.upsert(http, "synthetic-http").unwrap();
    store.upsert(local, "synthetic-local").unwrap();
    drop(store);

    let mixed = q(&e, "summary");
    assert_eq!(mixed["cacheHitRateInputTokens"], 1200);
    assert_eq!(mixed["cacheHitRateReadTokens"], 800);
    assert!((mixed["cacheHitRate"].as_f64().unwrap() - 2.0 / 3.0).abs() < 1e-12);
    assert_eq!(mixed["realTotalTokens"], 92_600);
    assert_eq!(mixed["totalRequests"], 2);
    assert_eq!(mixed["modelUsageCount"], 2);
    let local = e
        .execute(
            "usage_query",
            json!({"kind":"summary", "appType":"lmstudio"}),
        )
        .unwrap();
    assert!(local["cacheHitRate"].is_null());
    let known = e
        .execute("usage_query", json!({"kind":"summary", "appType":"claude"}))
        .unwrap();
    assert_eq!(known["cacheHitRate"], 0.0);
}

#[test]
fn sqlite_wal_and_jsonl_share_identity_and_unknown_usage_remains_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("studio.sqlite3");
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE model_usage(id TEXT PRIMARY KEY,occurred_at INTEGER,measurement TEXT);").unwrap();
    for v in rows() {
        c.execute(
            "INSERT INTO model_usage VALUES(?1,?2,?3)",
            rusqlite::params![v["id"].as_str(), v["occurredAt"].as_i64(), v.to_string()],
        )
        .unwrap();
    }
    fs::write(dir.path().join("models.jsonl"), FIXTURE).unwrap();
    assert_eq!(GptImgStudio.discover(dir.path()).unwrap(), vec![db.clone()]);
    let (sqlite, _, _, complete) = GptImgStudio.read_source(&db).unwrap();
    assert!(complete);
    assert_eq!(sqlite, rows());
    let mut input = rows();
    input.extend(rows());
    let parsed = GptImgStudio
        .parse(&db, &input, &TimelineIndex::new())
        .unwrap();
    assert!(parsed.complete);
    assert_eq!(parsed.usage.len(), 2);
    input[0]["tokens"]["output"] = json!(101);
    let bad = GptImgStudio
        .parse(&db, &input, &TimelineIndex::new())
        .unwrap();
    assert!(!bad.complete);
    let mut unknown = rows()[1].clone();
    unknown["tokens"] = json!({});
    unknown["textInputTokens"] = Value::Null;
    unknown["imageInputTokens"] = Value::Null;
    unknown["usageAvailable"] = json!(false);
    unknown["estimatedCostUsd"] = Value::Null;
    unknown["estimateStatus"] = json!("unknown");
    let parsed = GptImgStudio
        .parse(&db, &[unknown], &TimelineIndex::new())
        .unwrap();
    assert!(parsed.complete, "{:?}", parsed.diagnostics);
    let m = parsed.usage[0].model_usage.as_ref().unwrap();
    assert!(m.tokens.output.is_none());
    assert!(m.estimated_cost_usd.is_none());
}

#[test]
fn v7_projection_upgrade_preserves_facts_settings_and_outbox() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scope.db");
    let e = engine(dir.path());
    scan(&e);
    let s = e.lock().unwrap();
    let original = s.local_records().unwrap();
    let pending = s.pending().unwrap();
    let config = s.config().unwrap();
    s.set_meta("remote_cursor", &42).unwrap();
    s.conn.execute_batch("DROP TRIGGER usage_index_records_INSERT;DROP TRIGGER usage_index_records_UPDATE;DROP TRIGGER usage_index_remote_records_INSERT;DROP TRIGGER usage_index_remote_records_UPDATE;ALTER TABLE usage_index DROP COLUMN model_usage;PRAGMA user_version=7;").unwrap();
    drop(s);
    drop(e);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 8);
    assert_eq!(s.local_records().unwrap(), original);
    assert_eq!(s.pending().unwrap(), pending);
    assert_eq!(
        serde_json::to_value(s.config().unwrap()).unwrap(),
        serde_json::to_value(config).unwrap()
    );
    assert_eq!(s.meta::<i64>("remote_cursor").unwrap(), Some(42));
    assert_eq!(
        s.conn
            .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    drop(s);
    assert_eq!(
        Store::open(&path).unwrap().local_records().unwrap(),
        original
    );
}

#[test]
fn provider_names_and_api_routes_stay_separate_from_subscription_identity() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    scan(&e);
    let before = e.lock().unwrap().local_records().unwrap();
    let mut input = rows();
    for row in &mut input {
        row["provider"] = json!({"id":"legacy-image","name":"OpenAI","kind":"openai","billingChannel":"api","apiService":"openai-platform"});
    }
    fs::write(
        dir.path().join("models.jsonl"),
        input
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    scan(&e);
    let after = e.lock().unwrap().local_records().unwrap();
    for r in &after {
        assert_eq!(
            r.provider.as_deref(),
            Some("gpt-img-studio:openai-platform:legacy-image")
        );
        let old = before.iter().find(|old| old.id == r.id).unwrap();
        assert_eq!(r.tokens, old.tokens);
        assert_eq!(r.cost, old.cost);
        let mut wrong = r.clone();
        wrong.provider = Some("openai".into());
        assert!(sync::validate_record(&wrong).is_err());
    }
    assert_eq!(
        q(&e, "providers")[0]["providerDisplayName"],
        "OpenAI Platform (API)"
    );
    scan(&e);
    assert_eq!(e.lock().unwrap().local_records().unwrap(), after);
    for row in &mut input {
        row["provider"]
            .as_object_mut()
            .unwrap()
            .remove("apiService");
    }
    let parsed = GptImgStudio
        .parse(
            &dir.path().join("models.jsonl"),
            &input,
            &TimelineIndex::new(),
        )
        .unwrap();
    assert!(parsed.complete);
    let p = parsed.usage[0]
        .model_usage
        .as_ref()
        .unwrap()
        .provider
        .as_ref()
        .unwrap();
    assert_eq!(p.api_service, "unknown");
    assert_eq!(p.display_name(), "OpenAI (API)");
    assert_eq!(
        parsed.usage[0].provider.as_deref(),
        Some("gpt-img-studio:legacy-image")
    );
}
