use super::*;

fn row(minutes: i64) -> Value {
    json!({"usedPercent":12.5,"windowDurationMins":minutes,"resetsAt":1793082977i64})
}
fn account() -> Value {
    json!({"account":{"type":"chatgpt","email":"private@example.test","planType":"prolite"}})
}
#[test]
fn codex_quota_preserves_dynamic_windows_buckets_and_raw_plan_without_credentials() {
    let raw = json!({"accountId":"private-account","accessToken":"secret-token","rateLimits":{"primary":row(300)},"rateLimitsByLimitId":{"codex":{"primary":row(10080),"secondary":null,"credits":{"balance":"0.123456789","hasCredits":true,"unlimited":false,"apiKey":"secret-key"}},"extra":{"limitName":"Extra","primary":row(300),"secondary":row(10080)}}});
    let data = parse(&account(), &raw).unwrap();
    assert_eq!(data["kind"], "codexQuota");
    assert_eq!(data["planType"], "prolite");
    assert_eq!(data["buckets"][0]["windows"].as_array().unwrap().len(), 1);
    assert_eq!(data["buckets"][0]["windows"][0]["durationMinutes"], 10080);
    assert_eq!(data["buckets"][0]["credits"]["balance"], "0.123456789");
    assert_eq!(data["buckets"][1]["windows"].as_array().unwrap().len(), 2);
    assert_eq!(data["identity"].as_str().unwrap().len(), 64);
    for secret in [
        "private-account",
        "private@example.test",
        "secret-token",
        "secret-key",
    ] {
        assert!(!data.to_string().contains(secret));
    }
    let legacy = parse(
        &account(),
        &json!({"rateLimits":{"primary":row(300),"secondary":row(10080)}}),
    )
    .unwrap();
    assert_eq!(legacy["buckets"][0]["windows"].as_array().unwrap().len(), 2);
    let no_windows = parse(
        &account(),
        &json!({"rateLimits":{"primary":null,"secondary":null}}),
    )
    .unwrap();
    assert_eq!(no_windows["buckets"][0]["windows"], json!([]));
}
#[test]
fn codex_invalid_auth_and_quota_never_turn_into_full_or_infinite_allowance() {
    assert!(parse(&json!({"account":{"type":"apiKey"}}), &json!({})).is_err());
    assert!(parse(&account(), &json!({})).is_err());
    for (key, value) in [
        ("usedPercent", json!(-1)),
        ("usedPercent", json!("0")),
        ("windowDurationMins", json!(0)),
        ("resetsAt", json!(null)),
    ] {
        let mut r = row(300);
        r[key] = value;
        assert!(parse(&account(), &json!({"rateLimits":{"primary":r}})).is_err());
    }
    let mut r = row(300);
    r["usedPercent"] = json!(100);
    assert_eq!(
        parse(&account(), &json!({"rateLimits":{"primary":r}})).unwrap()["buckets"][0]["windows"]
            [0]["status"],
        "rate-limited"
    );
}
#[test]
fn local_codex_connection_needs_no_key_and_retains_manual_billing_and_history() {
    let temp = tempfile::tempdir().unwrap();
    let engine = Engine::open(&temp.path().join("db.sqlite")).unwrap();
    let args = json!({"provider":"openai-codex","name":"Codex","codex":{"plan":"pro100"},"links":[{"harness":"codex","provider":"openai"}],"subscription":{"planName":"My subscription","monthlyFee":"87","currency":"USD","periodStart":null,"renewalDate":null}});
    let view = engine
        .execute("provider_account_save", args.clone())
        .unwrap();
    let id = view["account"]["id"].as_str().unwrap();
    assert_eq!(view["keyConfigured"], false);
    assert!(!engine.account_vault.exists(id));
    let raw = json!({"kind":"codexQuota","planType":"prolite","buckets":[]});
    let refreshed = refresh_with(&engine, &engine.account_vault, id, |p, settings| {
        assert_eq!(p, "openai-codex");
        assert_eq!(
            serde_json::from_str::<Settings>(settings).unwrap().plan,
            "pro100"
        );
        Ok(raw.clone())
    })
    .unwrap();
    assert_eq!(refreshed["snapshot"]["data"], raw);
    let mut edited = refreshed["account"].clone();
    edited["codex"]["plan"] = json!("pro500");
    let edited = engine.execute("provider_account_save", edited).unwrap();
    assert_eq!(edited["snapshot"], refreshed["snapshot"]);
    assert_eq!(
        edited["account"]["subscription"],
        view["account"]["subscription"]
    );
    for patch in [
        json!({"apiKey":"not-allowed"}),
        json!({"codex":{"plan":"enterprise"}}),
        json!({"codex":{"plan":"plus","binaryPath":"relative.exe"}}),
        json!({"links":[{"harness":"opencode","provider":"openai"}]}),
    ] {
        let mut invalid = args.clone();
        invalid
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(engine.execute("provider_account_save", invalid).is_err());
    }
    let mut connection = edited["account"].clone();
    connection["codex"]["homePath"] = json!(temp.path().to_string_lossy());
    let changed = engine.execute("provider_account_save", connection).unwrap();
    assert!(changed["snapshot"].is_null());
    let store = engine.read_lock().unwrap();
    assert!(store.pending().unwrap().is_empty());
    assert!(store.local_records().unwrap().is_empty());
}
