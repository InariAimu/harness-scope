use harness_scope_core::{
    model::{Cost, RequestMetadata, Tokens, UsageRecord, UsageTiming},
    query, Engine,
};
use rust_decimal::Decimal;
use serde_json::json;
use std::str::FromStr;

const AT: i64 = 1792500000;

fn record(
    device: &str,
    id: &str,
    tier: Option<&str>,
    output: u64,
    duration: i64,
    first: i64,
) -> UsageRecord {
    UsageRecord {
        device_id: device.into(),
        id: id.into(),
        harness: "codex".into(),
        provider: Some("openai".into()),
        parser_version: "fixture".into(),
        session_id: Some("fixture".into()),
        native_event_id: Some(id.into()),
        occurred_at: AT,
        model: "openai/gpt-fixture".into(),
        grain: "event".into(),
        count: 1,
        tokens: Tokens {
            input: 10,
            output,
            cache_read: 20,
            ..Default::default()
        },
        cost: Cost {
            amount_usd: Some("0.0123456789".into()),
            method: "estimated".into(),
            ..Default::default()
        },
        record_source: "session".into(),
        revision: 1,
        deleted: false,
        status_code: None,
        legacy_date: None,
        legacy_timezone: None,
        request_metadata: tier.map(|tier| RequestMetadata {
            service_tier: Some(tier.into()),
            service_tier_source: Some("thread_settings".into()),
            reasoning_effort: Some("high".into()),
        }),
        timing: Some(UsageTiming {
            duration_ms: duration,
            first_token_ms: Some(first),
            duration_source: "codex_token_count".into(),
            first_token_source: Some("codex_item_started_at_ms".into()),
        }),
    }
}

#[test]
fn modes_keep_weighted_timings_unknowns_and_other_tiers_separate_without_changing_facts() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let s = engine.lock().unwrap();
    let device = s.device_id().unwrap();
    let mut rows = vec![
        record(&device, "standard-one", Some("default"), 400, 2000, 300),
        record(&device, "standard-two", Some("default"), 800, 8000, 700),
        record(&device, "fast", Some("fast"), 1000, 5000, 100),
        record(&device, "priority-small", Some("priority"), 100, 2000, 300),
        record(&device, "unknown", None, 300, 3000, 900),
        record(&device, "other-tier", Some("flex"), 300, 1500, 400),
    ];
    let mut non_codex = record(&device, "gpt-in-opencode", None, 300, 3000, 800);
    non_codex.harness = "opencode".into();
    non_codex.provider = Some("other-route".into());
    non_codex.timing = None;
    rows.push(non_codex);
    let mut claude = record(&device, "unrelated-model", None, 500, 2000, 500);
    claude.model = "claude-fixture".into();
    claude.provider = None;
    claude.harness = "claude".into();
    claude.timing = None;
    rows.push(claude);
    for row in rows {
        s.upsert(row, "synthetic").unwrap();
    }
    let before = s.local_records().unwrap();
    let pending = s.pending().unwrap();
    let version = s.usage_version().unwrap();
    let stats = query::query(&s, "models", &json!({})).unwrap();
    let stats = stats.as_array().unwrap();
    assert_eq!(stats.len(), 5);
    let mode = |name| stats.iter().find(|v| v["requestMode"] == name).unwrap();
    let standard = mode("standard");
    assert_eq!(standard["requestCount"], 2);
    assert_eq!(standard["estSpeedOutputTokens"], 1200);
    assert_eq!(standard["estSpeedDurationMs"], 10000);
    assert_eq!(standard["estSpeedSampleCount"], 2);
    assert_eq!(standard["estFirstTokenMedianMs"], 500.0);
    let fast = mode("fast");
    assert_eq!(fast["requestCount"], 2);
    assert_eq!(fast["estSpeedSampleCount"], 1);
    assert_eq!(fast["estSpeedOutputTokens"], 1000);
    assert_eq!(fast["estFirstTokenMedianMs"], 200.0);
    assert_eq!(mode("unknown")["requestCount"], 2);
    assert_eq!(mode("flex")["requestCount"], 1);
    assert!(stats
        .iter()
        .find(|v| v["model"] == "claude-fixture")
        .unwrap()["requestMode"]
        .is_null());
    let keys: std::collections::HashSet<_> = stats
        .iter()
        .map(|v| v["rowKey"].as_str().unwrap())
        .collect();
    assert_eq!(keys.len(), stats.len());
    let summary = query::query(&s, "summary", &json!({})).unwrap();
    assert_eq!(
        stats
            .iter()
            .map(|v| v["requestCount"].as_u64().unwrap())
            .sum::<u64>(),
        summary["totalRequests"].as_u64().unwrap()
    );
    assert_eq!(
        stats
            .iter()
            .map(|v| v["totalTokens"].as_u64().unwrap())
            .sum::<u64>(),
        summary["realTotalTokens"].as_u64().unwrap()
    );
    assert_eq!(
        stats
            .iter()
            .map(|v| Decimal::from_str(v["totalCost"].as_str().unwrap()).unwrap())
            .sum::<Decimal>(),
        Decimal::from_str(summary["totalCost"].as_str().unwrap()).unwrap()
    );
    assert_eq!(s.local_records().unwrap(), before);
    assert_eq!(s.pending().unwrap(), pending);
    assert_eq!(s.usage_version().unwrap(), version);
    let providers = query::query(&s, "providers", &json!({})).unwrap();
    assert_eq!(
        providers
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["providerId"] == "openai")
            .unwrap()["requestCount"],
        6
    );
}

#[test]
fn device_provider_model_and_time_filters_apply_before_mode_grouping() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let s = engine.lock().unwrap();
    let device = s.device_id().unwrap();
    s.upsert(
        record(&device, "local", Some("default"), 400, 2000, 200),
        "synthetic",
    )
    .unwrap();
    let mut remote = record("remote", "remote", Some("priority"), 800, 3000, 100);
    remote.provider = Some("remote-route".into());
    s.conn
        .execute(
            "INSERT INTO remote_records VALUES(?1,?2,?3,?4)",
            rusqlite::params![
                remote.device_id,
                remote.id,
                remote.revision,
                serde_json::to_string(&remote).unwrap()
            ],
        )
        .unwrap();
    let all = query::query(&s, "models", &json!({"scope":"all"})).unwrap();
    assert_eq!(all.as_array().unwrap().len(), 2);
    for args in [
        json!({}),
        json!({"scope":"all","providerName":"openai"}),
        json!({"scope":"all","appType":"codex","providerName":"openai","model":"openai/gpt-fixture"}),
    ] {
        let rows = query::query(&s, "models", &args).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 1);
        assert_eq!(rows[0]["requestMode"], "standard");
    }
    let only_remote = query::query(&s, "models", &json!({"scope":"remote"})).unwrap();
    assert_eq!(only_remote[0]["requestMode"], "fast");
    assert!(
        query::query(&s, "models", &json!({"scope":"all","startDate":AT+1}))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        query::query(&s, "models", &json!({"model":"different-model"}))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}
