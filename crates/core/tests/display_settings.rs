use chrono::TimeZone;
use harness_scope_core::{model::Source, Engine};
use serde_json::{json, Value};

fn save(e: &Engine, patch: Value) -> Value {
    e.execute("save_usage_display", json!({"patch":patch}))
        .unwrap()
}

#[test]
fn old_database_defaults_and_display_preferences_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scope.db");
    let e = Engine::open(&path).unwrap();
    assert_eq!(e.execute("usage_display", json!({})).unwrap(), json!({}));
    save(
        &e,
        json!({"currency":"CNY","exchangeRates":{"CNY":7.25},"sidebarTodayItems":["cost","requests"],"requestLogColumns":{"statusCode":true,"app":false}}),
    );
    // Upgrading an existing display-settings record adds only the new preference.
    let upgraded = save(&e, json!({"trendChartType":"area"}));
    assert_eq!(upgraded["currency"], "CNY");
    drop(e);
    let e = Engine::open(&path).unwrap();
    let value = e.execute("usage_display", json!({})).unwrap();
    assert_eq!(value["currency"], "CNY");
    assert_eq!(value["exchangeRates"]["CNY"], 7.25);
    assert_eq!(value["sidebarTodayItems"], json!(["cost", "requests"]));
    assert_eq!(value["requestLogColumns"]["app"], false);
    assert_eq!(value["trendChartType"], "area");
    assert_eq!(
        save(&e, json!({"trendChartType":"bar"}))["trendChartType"],
        "bar"
    );
}

#[test]
fn patches_preserve_other_fields_rates_and_allow_explicit_resets() {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::open(&dir.path().join("scope.db")).unwrap();
    save(
        &e,
        json!({"currency":"CNY","exchangeRates":{"CNY":7.25},"sidebarTodayItems":["tokens"]}),
    );
    save(&e, json!({"exchangeRates":{"EUR":0.9}}));
    save(&e, json!({"trendChartType":"area"}));
    let value = save(&e, json!({"requestLogColumns":{"app":false}}));
    assert_eq!(value["currency"], "CNY");
    assert_eq!(value["exchangeRates"], json!({"CNY":7.25,"EUR":0.9}));
    let value = save(&e, json!({"sidebarTodayItems":[],"requestLogColumns":{}}));
    assert_eq!(value["sidebarTodayItems"], json!([]));
    assert_eq!(value["requestLogColumns"], json!({}));
    assert_eq!(value["exchangeRates"]["CNY"], 7.25);
    assert_eq!(value["trendChartType"], "area");
}

#[test]
fn invalid_patches_are_rejected_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::open(&dir.path().join("scope.db")).unwrap();
    let before = save(&e, json!({"currency":"EUR","exchangeRates":{"EUR":0.9}}));
    for patch in [
        json!({"currency":"BAD"}),
        json!({"exchangeRates":{"CNY":0}}),
        json!({"currency":"CNY","exchangeRates":{"EUR":-1}}),
        json!({"exchangeRates":{"USD":7}}),
        json!({"sidebarTodayItems":["tokens","requests","cost"]}),
        json!({"sidebarTodayItems":["tokens","tokens"]}),
        json!({"requestLogColumns":{"time":false}}),
        json!({"serverToken":"private"}),
        json!({"currency":"CNY","trendChartType":"line"}),
        json!({"trendChartType":""}),
        json!({"trendChartType":42}),
    ] {
        assert!(e
            .execute("save_usage_display", json!({"patch":patch}))
            .is_err());
        assert_eq!(e.execute("usage_display", json!({})).unwrap(), before);
    }
}

#[test]
fn presentation_changes_do_not_reprice_revise_or_requeue_usage() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    std::fs::create_dir(&logs).unwrap();
    std::fs::write(
        logs.join("session.jsonl"),
        include_str!("../../../tests/fixtures/codex/session.jsonl"),
    )
    .unwrap();
    let e = Engine::open(&dir.path().join("scope.db")).unwrap();
    {
        let s = e.lock().unwrap();
        let mut config = s.config().unwrap();
        config.sources = vec![Source {
            id: "fixture".into(),
            harness: "codex".into(),
            root: logs.to_string_lossy().into(),
            enabled: true,
        }];
        s.set_meta("config", &config).unwrap();
    }
    e.scan(false).unwrap();
    let (records, pending, config) = {
        let s = e.lock().unwrap();
        (
            s.local_records().unwrap(),
            s.pending().unwrap(),
            serde_json::to_value(s.config().unwrap()).unwrap(),
        )
    };
    save(
        &e,
        json!({"currency":"JPY","exchangeRates":{"JPY":158},"sidebarTodayItems":["tokens","requests"],"requestLogColumns":{"cost":false},"trendChartType":"area"}),
    );
    let s = e.lock().unwrap();
    assert_eq!(records, s.local_records().unwrap());
    assert_eq!(pending, s.pending().unwrap());
    assert_eq!(config, serde_json::to_value(s.config().unwrap()).unwrap());
    assert!(!serde_json::to_string(&s.pending().unwrap())
        .unwrap()
        .contains("exchangeRates"));
}

#[test]
fn sidebar_today_uses_the_configured_timezone_and_local_device() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    std::fs::create_dir(&logs).unwrap();
    std::fs::write(
        logs.join("session.jsonl"),
        include_str!("../../../tests/fixtures/codex/session.jsonl"),
    )
    .unwrap();
    let e = Engine::open(&dir.path().join("scope.db")).unwrap();
    let zone: chrono_tz::Tz = "Pacific/Kiritimati".parse().unwrap();
    {
        let s = e.lock().unwrap();
        let mut config = s.config().unwrap();
        config.timezone = zone.to_string();
        config.sources = vec![Source {
            id: "fixture".into(),
            harness: "codex".into(),
            root: logs.to_string_lossy().into(),
            enabled: true,
        }];
        s.set_meta("config", &config).unwrap();
    }
    e.scan(false).unwrap();
    {
        let s = e.lock().unwrap();
        let mut record = s.local_records().unwrap().remove(0);
        s.conn.execute("DELETE FROM records", []).unwrap();
        let today = chrono::Utc::now().with_timezone(&zone).date_naive();
        let start = zone
            .from_local_datetime(&today.and_hms_opt(0, 0, 0).unwrap())
            .earliest()
            .unwrap()
            .timestamp();
        record.id = "today-local".into();
        record.occurred_at = start + 1;
        s.upsert(record.clone(), "fixture").unwrap();
        record.id = "yesterday-local".into();
        record.occurred_at = start - 1;
        s.upsert(record.clone(), "fixture").unwrap();
        record.id = "today-remote".into();
        record.device_id = "remote".into();
        record.occurred_at = start + 1;
        s.conn
            .execute(
                "INSERT INTO remote_records VALUES(?1,?2,?3,?4)",
                rusqlite::params![
                    record.device_id,
                    record.id,
                    1,
                    serde_json::to_string(&record).unwrap()
                ],
            )
            .unwrap();
    }
    let local = e
        .execute(
            "usage_query",
            json!({"kind":"todaySummary","scope":"local"}),
        )
        .unwrap();
    assert_eq!(local["totalRequests"], 1);
    let all = e
        .execute("usage_query", json!({"kind":"todaySummary","scope":"all"}))
        .unwrap();
    assert_eq!(all["totalRequests"], 2);
}
