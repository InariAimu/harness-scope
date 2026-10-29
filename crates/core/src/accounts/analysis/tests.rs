use super::*;
use crate::model::{Cost, Tokens};

struct Fixture {
    _dir: tempfile::TempDir,
    engine: Engine,
    id: String,
    start: i64,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
        let account = engine.execute("provider_account_save",json!({"provider":"custom","name":"Mixed","links":[{"harness":"codex","provider":"relay"}],"custom":{"enabled":false,"baseUrl":"","script":"","timeoutSeconds":10}})).unwrap();
        let id = account["account"]["id"].as_str().unwrap().to_owned();
        {
            let s = engine.lock().unwrap();
            let mut price = s.prices().unwrap()[0].clone();
            price.model_id = "fixture".into();
            price.provider = Some("relay".into());
            price.source = "manual".into();
            price.currency = "USD".into();
            price.pricing_rule = None;
            price.go_tariff = None;
            price.input_cost_per_million = "1".into();
            price.output_cost_per_million = "0".into();
            price.cache_read_cost_per_million = "0.1".into();
            price.cache_creation_cost_per_million = "0".into();
            s.set_meta("prices", &vec![price]).unwrap();
        }
        Self {
            _dir: dir,
            engine,
            id,
            start: Utc::now().timestamp() - 3600,
        }
    }
    fn append(&self, offset: i64, used: i64) -> i64 {
        let s = self.engine.lock().unwrap();
        let mut item = load(&s)
            .unwrap()
            .into_iter()
            .find(|i| i.account.id == self.id)
            .unwrap();
        item.request_id = Some(uuid::Uuid::new_v4().to_string());
        item.snapshot = Some(
            json!({"data":{"kind":"customUsage","items":[{"id":"weekly","planName":"Shared","unit":"USD","total":"100","usedPercent":used.to_string(),"resetsAt":self.start+86400,"durationMinutes":10080,"windowType":"fixed"}]}}),
        );
        history::record(
            &s,
            &item,
            chrono::DateTime::from_timestamp(self.start + offset, 0).unwrap(),
            "Asia/Shanghai",
        )
        .unwrap();
        s.conn.last_insert_rowid()
    }
    fn usage(&self, id: &str, offset: i64, input: u64, cache: u64) -> UsageRecord {
        let s = self.engine.lock().unwrap();
        let row = UsageRecord {
            timing: None,
            request_metadata: None,
            device_id: s.device_id().unwrap(),
            id: id.into(),
            provider: Some("relay".into()),
            harness: "codex".into(),
            parser_version: "fixture".into(),
            session_id: None,
            native_event_id: None,
            occurred_at: self.start + offset,
            model: "fixture".into(),
            grain: "event".into(),
            count: 1,
            tokens: Tokens {
                input,
                cache_read: cache,
                ..Default::default()
            },
            cost: Cost {
                amount_usd: Some("0".into()),
                ..Default::default()
            },
            record_source: "session".into(),
            revision: 1,
            deleted: false,
            status_code: None,
            legacy_date: None,
            legacy_timezone: None,
        };
        s.upsert(row.clone(), "fixture").unwrap();
        row
    }
    fn estimate(&self, a: i64, b: i64) -> Value {
        self.engine.execute("provider_account_estimate",json!({"id":self.id,"firstSequence":a,"lastSequence":b,"bucketId":null,"windowId":"weekly"})).unwrap()
    }
    fn patch_observation(&self, seq: i64, patch: impl FnOnce(&mut Value)) {
        let s = self.engine.lock().unwrap();
        let payload: String = s
            .conn
            .query_row(
                "SELECT payload FROM provider_account_observations WHERE seq=?",
                [seq],
                |r| r.get(0),
            )
            .unwrap();
        let mut value: Value = serde_json::from_str(&payload).unwrap();
        patch(&mut value);
        s.conn
            .execute(
                "UPDATE provider_account_observations SET payload=?1 WHERE seq=?2",
                params![value.to_string(), seq],
            )
            .unwrap();
    }
}

#[test]
fn infers_capacity_from_actual_interval_repricing_zero_reported_cost_and_cache_mix() {
    let f = Fixture::new();
    let a = f.append(0, 20);
    let b = f.append(1800, 30);
    f.usage("at-start", 0, 999_000_000, 0);
    f.usage("inside", 600, 4_000_000, 10_000_000);
    f.usage("after-end", 1801, 999_000_000, 0);
    let before = f.engine.lock().unwrap().usage_version().unwrap();
    let result = f.estimate(a, b);
    assert_eq!(result["status"], "ready");
    assert_eq!(result["linkedUsd"], "5");
    assert_eq!(result["totalEquivalentUsd"], "50");
    assert_eq!(result["remainingEquivalentUsd"], "35");
    assert_eq!(result["requests"], 1);
    assert_eq!(f.engine.lock().unwrap().usage_version().unwrap(), before);
    assert!(f
        .engine
        .lock()
        .unwrap()
        .local_records()
        .unwrap()
        .iter()
        .all(|r| r.cost.amount_usd.as_deref() == Some("0")));
}

#[test]
fn cross_device_usage_counts_once_and_unpriced_usage_blocks_the_result() {
    let f = Fixture::new();
    let a = f.append(0, 20);
    let b = f.append(1800, 30);
    let row = f.usage("local", 600, 5_000_000, 0);
    {
        let s = f.engine.lock().unwrap();
        for (device, id) in [
            (row.device_id.as_str(), "local"),
            ("other-device", "remote"),
        ] {
            let mut remote = row.clone();
            remote.device_id = device.into();
            remote.id = id.into();
            s.conn
                .execute(
                    "INSERT INTO remote_records(device_id,id,revision,payload) VALUES(?1,?2,1,?3)",
                    params![device, id, serde_json::to_string(&remote).unwrap()],
                )
                .unwrap();
        }
    }
    assert_eq!(f.estimate(a, b)["linkedUsd"], "10");
    let mut row = f.usage("unpriced", 700, 100, 0);
    row.model = "missing-price".into();
    f.engine.lock().unwrap().upsert(row, "fixture").unwrap();
    let blocked = f.estimate(a, b);
    assert_eq!(blocked["reason"], "unpriced_usage");
    assert!(blocked["totalEquivalentUsd"].is_null());
}

#[test]
fn skips_regressing_counters_but_still_rejects_account_boundaries() {
    let f = Fixture::new();
    let a = f.append(0, 20);
    f.append(300, 15);
    let b = f.append(1800, 30);
    f.usage("inside", 600, 5_000_000, 0);
    let result = f.estimate(a, b);
    assert_eq!(result["status"], "ready", "{result}");
    assert_eq!(result["ignoredObservations"], 1);
    assert_eq!(result["totalEquivalentUsd"], "50");
    f.patch_observation(a, |v| v["accountRevision"] = json!("old"));
    assert_eq!(f.estimate(a, b)["reason"], "different_window");
}

#[test]
fn replenishment_blocks_cross_interval_estimates_but_keeps_both_intervals_usable() {
    let f = Fixture::new();
    let old_start = f.append(0, 95);
    let old_end = f.append(300, 99);
    let new_start = f.append(600, 49);
    let new_end = f.append(900, 50);
    f.append(1200, 47); // Small delayed-device regression is still ignored.
    let latest = f.append(1800, 51);
    f.usage("old", 150, 4_000_000, 0);
    f.usage("new", 750, 1_000_000, 0);
    assert_eq!(f.estimate(old_start, old_end)["status"], "ready");
    for end in [new_start, new_end, latest] {
        let rejected = f.estimate(old_end, end);
        assert_eq!(rejected["reason"], "quota_adjusted");
        assert!(rejected["totalEquivalentUsd"].is_null());
    }
    let result = f.estimate(new_start, new_end);
    assert_eq!(result["status"], "ready", "{result}");
    assert_eq!(result["linkedUsd"], "1");
    assert_eq!(result["totalEquivalentUsd"], "100");
    assert_eq!(result["remainingEquivalentUsd"], "50");
    assert_eq!(f.estimate(new_start, latest)["ignoredObservations"], 1);
    // Even after consumption exceeds the old high-water mark, this is a boundary.
    let caught_up = f.append(2400, 100);
    assert_eq!(f.estimate(old_end, caught_up)["reason"], "quota_adjusted");
}

#[test]
fn adjustment_threshold_is_inclusive_and_does_not_turn_small_regressions_into_boundaries() {
    let f = Fixture::new();
    let a = f.append(0, 99);
    let small = f.append(300, 80);
    let boundary = f.append(600, 79);
    assert_eq!(f.estimate(a, small)["reason"], "quota_decreased");
    assert_eq!(f.estimate(a, boundary)["reason"], "quota_adjusted");
}

#[test]
fn short_flat_rolling_unknown_and_expired_samples_do_not_produce_capacity() {
    let f = Fixture::new();
    let a = f.append(0, 20);
    let short = f.append(10, 30);
    assert_eq!(f.estimate(a, short)["reason"], "insufficient_span");
    let flat = f.append(1800, 20);
    assert_eq!(f.estimate(short, flat)["reason"], "quota_decreased");
    f.patch_observation(short, |v| v["windows"][0]["usedPercent"] = json!(20));
    assert_eq!(f.estimate(a, flat)["reason"], "insufficient_delta");
    f.patch_observation(flat, |v| {
        v["data"]["items"][0]["windowType"] = json!("rolling")
    });
    assert_eq!(f.estimate(a, flat)["reason"], "unsupported_window");
    f.patch_observation(flat, |v| {
        v["data"]["items"][0]["windowType"] = json!("fixed");
        v["windows"][0]["status"] = json!("unknown")
    });
    assert_eq!(f.estimate(a, flat)["reason"], "unknown_quota");
}

#[test]
fn account_edits_and_cross_account_sequence_selection_are_rejected() {
    let f = Fixture::new();
    let a = f.append(0, 20);
    let b = f.append(1800, 30);
    {
        let s = f.engine.lock().unwrap();
        let mut items = load(&s).unwrap();
        items[0].revision = "changed".into();
        s.set_meta(META, &items).unwrap();
    }
    assert_eq!(f.estimate(a, b)["reason"], "account_changed");
    assert!(f.engine.execute("provider_account_estimate",json!({"id":uuid::Uuid::new_v4().to_string(),"firstSequence":a,"lastSequence":b,"windowId":"weekly"})).is_err());
}

#[test]
fn codex_requires_verified_identity_and_a_mapped_bucket_before_pricing() {
    let f = Fixture::new();
    let a = f.append(0, 20);
    let b = f.append(1800, 30);
    f.usage("inside", 600, 5_000_000, 0);
    {
        let s = f.engine.lock().unwrap();
        let mut items = load(&s).unwrap();
        items[0].account.provider = "openai-codex".into();
        items[0].account.custom = None;
        items[0].account.codex = Some(codex::Settings {
            plan: "plus".into(),
            binary_path: None,
            home_path: None,
        });
        s.set_meta(META, &items).unwrap();
        let mut prices = s.prices().unwrap();
        prices
            .iter_mut()
            .filter(|p| p.model_id == "fixture")
            .for_each(|p| p.provider = None);
        s.set_meta("prices", &prices).unwrap();
    }
    for seq in [a, b] {
        f.patch_observation(seq, |v| {
            v["provider"] = json!("openai-codex");
            v["windows"][0]["bucketId"] = json!("codex");
            v["data"] = json!({"kind":"codexQuota","identity":"a".repeat(64),"identitySource":"email","planType":"plus","buckets":[{"id":"codex","planType":"plus"}]});
        });
    }
    let query = |bucket: &str| {
        f.engine.execute("provider_account_estimate",json!({"id":f.id,"firstSequence":a,"lastSequence":b,"bucketId":bucket,"windowId":"weekly"})).unwrap()
    };
    assert_eq!(query("codex")["reason"], "unverified_identity");
    for seq in [a, b] {
        f.patch_observation(seq, |v| v["data"]["identitySource"] = json!("accountId"));
    }
    assert_eq!(query("codex")["totalEquivalentUsd"], "50");
    f.patch_observation(b, |v| {
        let reset = v["windows"][0]["resetsAt"].as_i64().unwrap();
        v["windows"][0]["resetsAt"] = json!(reset + 1);
    });
    assert_eq!(query("codex")["totalEquivalentUsd"], "50");
    f.patch_observation(b, |v| {
        let reset = v["windows"][0]["resetsAt"].as_i64().unwrap();
        v["windows"][0]["resetsAt"] = json!(reset + 1);
    });
    assert_eq!(query("codex")["reason"], "different_window");
    f.patch_observation(b, |v| {
        let reset = v["windows"][0]["resetsAt"].as_i64().unwrap();
        v["windows"][0]["resetsAt"] = json!(reset - 2);
    });
    for seq in [a, b] {
        f.patch_observation(seq, |v| {
            v["windows"][0]["bucketId"] = json!("extra");
            v["data"]["buckets"][0]["id"] = json!("extra")
        });
    }
    assert_eq!(query("extra")["reason"], "unsupported_bucket");
}
