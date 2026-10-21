use super::*;

fn fixture(provider: &str) -> (tempfile::TempDir, Engine, String) {
    let (directory, engine, _vault) = setup();
    let (harness, route) = if provider == "openai-codex" {
        ("codex", "openai")
    } else {
        ("dsh", "opencode-go")
    };
    let mut args = json!({
        "provider":provider,"name":"Weekly fixture",
        "links":[{"harness":harness,"provider":route}],
        "subscription":{"planName":"Manual","monthlyFee":"100","currency":"USD","periodStart":"2020-01-01","renewalDate":"2030-01-01"}
    });
    if provider == "openai-codex" {
        args["codex"] = json!({"plan":"pro100"});
    }
    let view = engine.execute("provider_account_save", args).unwrap();
    let id = view["account"]["id"].as_str().unwrap().to_owned();
    (directory, engine, id)
}

fn quota(provider: &str, reset: i64) -> Value {
    if provider == "openai-codex" {
        json!({"kind":"codexQuota","buckets":[{"id":"codex","windows":[
            {"id":"primary","durationMinutes":300,"resetsAt":reset-60},
            {"id":"secondary","durationMinutes":10080,"resetsAt":reset}
        ]}]})
    } else {
        json!({"kind":"quota","windows":[
            {"id":"rolling","resetsAt":reset-60},
            {"id":"weekly","resetsAt":reset},
            {"id":"monthly","resetsAt":reset+86400}
        ]})
    }
}

fn snapshot(engine: &Engine, id: &str, data: Value) {
    let store = engine.lock().unwrap();
    let mut items = load(&store).unwrap();
    items
        .iter_mut()
        .find(|v| v.account.id == id)
        .unwrap()
        .snapshot = Some(json!({"fetchedAt":Utc::now().timestamp(),"data":data}));
    store.set_meta(META, &items).unwrap();
}

#[test]
fn weekly_usage_uses_official_seconds_and_synced_routes_instead_of_manual_dates() {
    for provider in ["openai-codex", "opencode-go"] {
        let (_directory, engine, id) = fixture(provider);
        let now = Utc::now().timestamp();
        let reset = now + 3617;
        let start = reset - 7 * 86400;
        snapshot(&engine, &id, quota(provider, reset));
        let (harness, route) = if provider == "openai-codex" {
            ("codex", "openai")
        } else {
            ("dsh", "opencode-go")
        };
        let (original, pending);
        {
            let store = engine.lock().unwrap();
            let mut config = store.config().unwrap();
            config.timezone = "America/Los_Angeles".into();
            store.set_meta("config", &config).unwrap();
            let own = store.device_id().unwrap();
            for (device, name, at, price) in [
                (own.as_str(), "local-before", start - 1, "99"),
                (own.as_str(), "local-start", start, "0.1"),
                ("remote", "remote-before", start - 1, "99"),
                ("remote", "remote-start", start, "0.2"),
                ("remote", "remote-now", now, "0.3"),
                (own.as_str(), "future", now + 3000, "99"),
                (own.as_str(), "next-period", reset, "99"),
            ] {
                let mut row = record(device, name, harness, route, Some(price));
                row.occurred_at = at;
                if device == own {
                    store.upsert(row, "fixture").unwrap();
                } else {
                    cache_remote(&store, &row);
                }
            }
            let mut mirror = record(&own, "local-start", harness, route, Some("999"));
            mirror.occurred_at = start;
            cache_remote(&store, &mirror);
            let mut deleted = record("remote", "deleted", harness, route, Some("999"));
            deleted.deleted = true;
            cache_remote(&store, &deleted);
            cache_remote(
                &store,
                &record("remote", "unlinked", harness, "unlinked", Some("999")),
            );
            original = store.local_records().unwrap();
            pending = store.pending_count().unwrap();
        }
        let result = engine
            .execute("provider_account_usage", json!({"id":id,"range":"weekly"}))
            .unwrap();
        assert_eq!(result["startDate"], start);
        assert!(result["endDate"].as_i64().unwrap() >= now);
        assert!(result["endDate"].as_i64().unwrap() < reset);
        assert_eq!(result["weeklyUsageWindow"]["resetsAt"], reset);
        assert_eq!(result["summary"]["totalRequests"], 3);
        assert_eq!(result["summary"]["realTotalTokens"], 600);
        assert_eq!(result["summary"]["totalCost"], "0.6");
        assert_eq!(result["timezone"], "America/Los_Angeles");
        let period = engine
            .execute(
                "provider_account_usage",
                json!({"id":id,"range":"subscription"}),
            )
            .unwrap();
        assert_eq!(period["summary"]["totalRequests"], 5);
        assert_eq!(period["summary"]["totalCost"], "198.6");
        assert!(period["weeklyUsageWindow"].is_null());
        let listed = engine.execute("provider_accounts", json!({})).unwrap();
        assert_eq!(listed[0]["weeklyUsageWindow"], result["weeklyUsageWindow"]);
        let store = engine.lock().unwrap();
        assert_eq!(store.local_records().unwrap(), original);
        assert_eq!(store.pending_count().unwrap(), pending);
    }
}

#[test]
fn weekly_usage_expires_exactly_at_reset_and_adopts_the_next_official_window() {
    let (_directory, engine, _id) = fixture("openai-codex");
    let store = engine.lock().unwrap();
    let mut item = load(&store).unwrap().remove(0);
    let reset = 1793082977;
    item.snapshot = Some(json!({"data":quota("openai-codex", reset)}));
    let start = reset - 604800;
    assert!(weekly_usage_window(&item, start - 1).is_none());
    assert_eq!(weekly_usage_window(&item, start).unwrap().start_date, start);
    assert!(weekly_usage_window(&item, reset - 1).is_some());
    assert!(weekly_usage_window(&item, reset).is_none());
    item.snapshot = Some(json!({"data":quota("openai-codex", reset + 604800)}));
    assert_eq!(weekly_usage_window(&item, reset).unwrap().start_date, reset);
}

#[test]
fn weekly_usage_never_falls_back_to_a_calendar_week_or_manual_period() {
    let (_directory, engine, id) = fixture("openai-codex");
    assert!(engine
        .execute("provider_account_usage", json!({"id":id,"range":"weekly"}))
        .is_err());
    let store = engine.lock().unwrap();
    let mut item = load(&store).unwrap().remove(0);
    let now = 1792479600;
    let reset = now + 3600;
    for data in [
        json!({}),
        quota("opencode-go", reset),
        quota("openai-codex", now),
        quota("openai-codex", now + 604801),
        json!({"kind":"codexQuota","buckets":[{"id":"codex","windows":[{"durationMinutes":300,"resetsAt":reset}]}]}),
        json!({"kind":"codexQuota","buckets":[{"id":"codex","windows":[{"durationMinutes":10080,"resetsAt":reset},{"durationMinutes":10080,"resetsAt":reset}]}]}),
        json!({"kind":"codexQuota","buckets":[{"id":"extra-a","windows":[{"durationMinutes":10080,"resetsAt":reset}]},{"id":"extra-b","windows":[{"durationMinutes":10080,"resetsAt":reset}]}]}),
        json!({"kind":"codexQuota","buckets":[{"id":"codex","windows":[]},{"id":"extra","windows":[{"durationMinutes":10080,"resetsAt":reset}]}]}),
    ] {
        item.snapshot = Some(json!({"data":data}));
        assert!(weekly_usage_window(&item, now).is_none());
    }
    item.snapshot = Some(
        json!({"data":{"kind":"codexQuota","buckets":[{"id":"alternate","windows":[{"durationMinutes":10080,"resetsAt":reset}]}]}}),
    );
    assert_eq!(
        weekly_usage_window(&item, now).unwrap().bucket.as_deref(),
        Some("alternate")
    );
}
