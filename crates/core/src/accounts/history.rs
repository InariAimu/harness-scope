//! Append-only observations, optionally shared across devices. Derived anchors are not samples.
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
    // A replay keeps the originally committed time/data, even after sharing was enabled.
    use rusqlite::OptionalExtension;
    let existing: Option<String> = store
        .conn
        .query_row(
            "SELECT payload FROM provider_account_observations WHERE refresh_id=?",
            [refresh_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(existing) = existing {
        return sharing::enqueue(store, &item.account, &serde_json::from_str(&existing)?);
    }
    let data = &item
        .snapshot
        .as_ref()
        .context("Missing successful snapshot")?["data"];
    // `data` is the provider parser's allowlisted result, never a raw response or credential.
    let payload = json!({
        "version":1,"refreshId":refresh_id,"accountId":item.account.id,
        "accountName":item.account.name,"accountRevision":measurement_revision(item),"provider":item.account.provider,
        "observedAtMs":at.timestamp_millis(),"observedAtLocal":local.to_rfc3339_opts(SecondsFormat::Millis,false),
        "timeZone":timezone,"utcOffsetSeconds":local.offset().fix().local_minus_utc(),
        "source":"refresh","data":data,
        "windows":windows(data,&item.account.provider,at.timestamp(),zone)
    });
    // Queue before inserting the history row so its SQLite sequence remains the last row id.
    sharing::enqueue(store, &item.account, &payload)?;
    store.conn.execute(
        "INSERT INTO provider_account_observations(refresh_id,account_id,provider,observed_at_ms,payload) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(refresh_id) DO NOTHING",
        params![refresh_id,item.account.id,item.account.provider,at.timestamp_millis(),serde_json::to_string(&payload)?],
    )?;
    Ok(())
}

pub(super) fn shared_payload(o: &sharing::Observation) -> Result<Value> {
    let zone: chrono_tz::Tz = o.time_zone.parse()?;
    let at =
        DateTime::from_timestamp_millis(o.observed_at_ms).context("Invalid observation time")?;
    let local = at.with_timezone(&zone);
    Ok(
        json!({"version":1,"refreshId":o.id,"accountId":o.account_id,"accountName":o.account_name,
        "accountRevision":o.account_revision,"provider":o.provider,"observedAtMs":o.observed_at_ms,
        "observedAtLocal":local.to_rfc3339_opts(SecondsFormat::Millis,false),"timeZone":o.time_zone,
        "utcOffsetSeconds":local.offset().fix().local_minus_utc(),"source":"refresh","data":o.data,
        "windows":windows(&o.data,&o.provider,at.timestamp(),zone)}),
    )
}

pub(super) fn decorate(store: &Store, row: &mut Value) -> Result<()> {
    use rusqlite::OptionalExtension;
    let shared: Option<(String, String)> = store
        .conn
        .query_row(
            "SELECT subscription_id,device_id FROM subscription_observations WHERE id=?",
            [row["refreshId"].as_str().unwrap_or("")],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((subscription, device)) = shared {
        row["subscriptionId"] = json!(subscription);
        row["deviceId"] = json!(device);
    }
    Ok(())
}

pub(super) const MEMBERSHIP: &str = "(o.account_id=?1 OR EXISTS(SELECT 1 FROM subscription_observations s WHERE s.id=o.refresh_id AND s.subscription_id=?2))";
pub(super) fn between(store: &Store, id: &str, start: i64, end: i64) -> Result<Vec<Value>> {
    let subscription = sharing::subscription_for(store, id)?;
    let mut stmt = store.conn.prepare(&format!("SELECT seq,payload FROM provider_account_observations o WHERE {MEMBERSHIP} AND observed_at_ms>=?3 AND observed_at_ms<=?4 ORDER BY observed_at_ms,refresh_id"))?;
    let rows = stmt
        .query_map(params![id, subscription, start, end], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(seq, p)| {
            let mut p: Value = serde_json::from_str(&p)?;
            p["sequence"] = json!(seq);
            decorate(store, &mut p)?;
            Ok(p)
        })
        .collect()
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
    // The cursor identifies a row; ordering follows event time, including delayed backfills.
    // Deleted accounts remain queryable by ID; deleting settings is not deleting history.
    let subscription = sharing::subscription_for(store, &query.id)?;
    let before_at: i64 = if query.before_sequence.is_some() {
        store.conn.query_row(&format!("SELECT observed_at_ms FROM provider_account_observations o WHERE {MEMBERSHIP} AND seq=?3"),params![query.id,subscription,before],|r|r.get(0)).context("Invalid history cursor")?
    } else {
        i64::MAX
    };
    let mut statement = store.conn.prepare_cached(&format!(
        "SELECT seq,payload FROM provider_account_observations o WHERE {MEMBERSHIP} AND observed_at_ms>=?3 AND observed_at_ms<?4 AND (observed_at_ms,seq)<(?5,?6) ORDER BY observed_at_ms DESC,seq DESC LIMIT ?7"
    ))?;
    let rows = statement
        .query_map(
            params![
                query.id,
                subscription,
                start,
                end,
                before_at,
                before,
                limit + 1
            ],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let has_more = rows.len() > limit as usize;
    let mut observations = Vec::new();
    for (seq, payload) in rows.into_iter().take(limit as usize) {
        let mut row: Value = serde_json::from_str(&payload)?;
        row["sequence"] = json!(seq);
        decorate(store, &mut row)?;
        observations.push(row);
    }
    let next = has_more
        .then(|| observations.last().unwrap()["sequence"].as_i64())
        .flatten();
    Ok(json!({"observations":observations,"nextCursor":next}))
}
