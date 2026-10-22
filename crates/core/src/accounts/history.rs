//! Append-only, device-local observations. Derived window anchors are not samples.
use super::*;
use chrono::{DateTime, Offset, SecondsFormat};
use rusqlite::params;

fn local_time(at: i64, zone: chrono_tz::Tz) -> Option<String> {
    DateTime::from_timestamp(at, 0).map(|at| {
        at.with_timezone(&zone)
            .to_rfc3339_opts(SecondsFormat::Secs, false)
    })
}

fn window(
    view: &Value,
    bucket: Option<&str>,
    provider: &str,
    at: i64,
    zone: chrono_tz::Tz,
) -> Value {
    let id = view["id"].as_str().unwrap_or("");
    let reported_minutes = view["durationMinutes"]
        .as_i64()
        .filter(|m| *m > 0 && *m <= 527040);
    let (minutes, duration_source) = match reported_minutes {
        Some(value) => (
            Some(value),
            Some(if custom::is_provider(provider) {
                "customQuery"
            } else {
                "provider"
            }),
        ),
        None if provider == "opencode-go" => match id {
            // https://docs.opencode.ai/docs/go/#usage-limits
            "rolling" => (Some(300), Some("providerRule")),
            "weekly" => (Some(10080), Some("providerRule")),
            // A month is not necessarily 30 days; the API supplies no start/duration.
            _ => (None, None),
        },
        _ => (None, None),
    };
    let reset = view["resetsAt"].as_i64().filter(|r| *r > 0);
    let start = reset
        .zip(minutes)
        .and_then(|(r, m)| r.checked_sub(m * 60))
        .filter(|s| *s >= 0 && DateTime::from_timestamp(*s, 0).is_some());
    let active = start.zip(reset).is_some_and(|(s, r)| s <= at && at < r);
    let known = matches!(view["status"].as_str(), Some("ok" | "rate-limited"));
    let remaining = known
        .then(|| Decimal::from_str(&view["usedPercent"].to_string()).ok())
        .flatten()
        .filter(|v| *v >= Decimal::ZERO)
        .map(|used| {
            (Decimal::from(100) - used)
                .clamp(Decimal::ZERO, Decimal::from(100))
                .normalize()
                .to_string()
        });
    let full_at = (active && known && remaining.is_some())
        .then_some(start)
        .flatten();
    json!({
        "bucketId":bucket,"windowId":id,"usedPercent":view["usedPercent"],
        "remainingPercent":remaining,"status":view["status"],
        "durationMinutes":minutes,"durationSource":duration_source,
        "resetsAt":reset,"resetsAtLocal":reset.and_then(|r| local_time(r, zone)),
        "startsAt":start,"startsAtLocal":start.and_then(|s| local_time(s, zone)),
        "activeAtSample":active,
        "fullQuotaAnchor":full_at.map(|s|json!({"at":s,"atLocal":local_time(s,zone),"remainingPercent":"100","source":"resetMinusDuration","observed":false}))
    })
}

fn windows(data: &Value, provider: &str, at: i64, zone: chrono_tz::Tz) -> Vec<Value> {
    match data["kind"].as_str() {
        Some("customUsage") => data["items"].as_array().into_iter().flatten().map(|item| {
            let view = json!({"id":item["id"],"usedPercent":item["usedPercent"].as_str().and_then(|s|s.parse::<f64>().ok()),
                "status":if item["usedPercent"].is_null() { "unknown" } else { "ok" },
                "resetsAt":item["resetsAt"],
                "durationMinutes":if item["windowType"] == "fixed" { item["durationMinutes"].clone() } else { Value::Null }});
            window(&view, None, provider, at, zone)
        }).collect(),
        Some("quota") => data["windows"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|w| window(w, None, provider, at, zone))
            .collect(),
        Some("codexQuota") => data["buckets"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|b| {
                b["windows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(move |w| window(w, b["id"].as_str(), provider, at, zone))
            })
            .collect(),
        _ => vec![],
    }
}

/// The caller commits this and the latest snapshot in the same transaction.
pub(super) fn record(
    store: &Store,
    item: &Stored,
    at: DateTime<Utc>,
    timezone: &str,
) -> Result<()> {
    let zone: chrono_tz::Tz = timezone.parse().context("Invalid observation time zone")?;
    let local = at.with_timezone(&zone);
    let refresh_id = item
        .request_id
        .as_deref()
        .context("Missing refresh identity")?;
    let data = &item
        .snapshot
        .as_ref()
        .context("Missing successful snapshot")?["data"];
    // `data` is the provider parser's allowlisted result, never a raw response or credential.
    let payload = json!({
        "version":1,"refreshId":refresh_id,"accountId":item.account.id,
        "accountName":item.account.name,"accountRevision":item.revision,"provider":item.account.provider,
        "observedAtMs":at.timestamp_millis(),"observedAtLocal":local.to_rfc3339_opts(SecondsFormat::Millis,false),
        "timeZone":timezone,"utcOffsetSeconds":local.offset().fix().local_minus_utc(),
        "source":"refresh","data":data,
        "windows":windows(data,&item.account.provider,at.timestamp(),zone)
    });
    store.conn.execute(
        "INSERT INTO provider_account_observations(refresh_id,account_id,provider,observed_at_ms,payload) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(refresh_id) DO NOTHING",
        params![refresh_id,item.account.id,item.account.provider,at.timestamp_millis(),serde_json::to_string(&payload)?],
    )?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Query {
    id: String,
    start_at_ms: Option<i64>,
    end_at_ms: Option<i64>,
    before_sequence: Option<i64>,
    limit: Option<u32>,
}

pub(super) fn query(store: &Store, args: Value) -> Result<Value> {
    let query: Query = serde_json::from_value(args)?;
    uuid::Uuid::parse_str(&query.id).context("Invalid account ID")?;
    let limit = query.limit.unwrap_or(100);
    anyhow::ensure!(
        (1..=500).contains(&limit),
        "History limit must be between 1 and 500"
    );
    let start = query.start_at_ms.unwrap_or(i64::MIN);
    let end = query.end_at_ms.unwrap_or(i64::MAX);
    let before = query.before_sequence.unwrap_or(i64::MAX);
    anyhow::ensure!(start < end && before > 0, "Invalid history range or cursor");
    // Stable append order and a sequence cursor preserve same-millisecond samples.
    // Deleted accounts remain queryable by ID; deleting settings is not deleting history.
    let mut statement = store.conn.prepare_cached(
        "SELECT seq,payload FROM provider_account_observations WHERE account_id=?1 AND observed_at_ms>=?2 AND observed_at_ms<?3 AND seq<?4 ORDER BY seq DESC LIMIT ?5",
    )?;
    let rows = statement
        .query_map(params![query.id, start, end, before, limit + 1], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let has_more = rows.len() > limit as usize;
    let mut observations = Vec::new();
    for (seq, payload) in rows.into_iter().take(limit as usize) {
        let mut row: Value = serde_json::from_str(&payload)?;
        row["sequence"] = json!(seq);
        observations.push(row);
    }
    let next = has_more
        .then(|| observations.last().unwrap()["sequence"].as_i64())
        .flatten();
    Ok(json!({"observations":observations,"nextCursor":next}))
}
