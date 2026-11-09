//! Decode-only, request-level distributions. No persisted facts or display modes change.
use crate::{
    query_store::{Row, Selection},
    store::Store,
    timing,
};
use anyhow::{ensure, Context, Result};
use chrono::TimeZone;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

struct Sample {
    generation_ms: i64,
    first_ms: i64,
    estimated: bool,
}

fn sample(row: &Row) -> Option<Sample> {
    if row.grain != "event"
        || row.record_source != "session"
        || row.count != 1
        || row.model_usage.is_some()
    {
        return None;
    }
    let timing = row.timing.as_ref()?;
    let first_ms = timing
        .first_token_ms
        .filter(|ms| (0..=timing.duration_ms).contains(ms))?;
    let estimate = timing::speed_estimate_for_display(timing, row.tokens.output, false);
    if !estimate.eligible || estimate.basis != timing::SpeedBasis::Decode {
        return None;
    }
    Some(Sample {
        generation_ms: estimate.duration_ms?,
        first_ms,
        estimated: timing.duration_source != "lmstudio_prediction_stats",
    })
}

/// Compact, additive totals: calendar heatmaps do not need sorted per-request samples.
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct DailyTotals {
    output_tokens: f64,
    generation_ms: f64,
    first_token_ms: f64,
    sample_count: usize,
    estimated_sample_count: usize,
}

pub(super) fn daily(
    store: &Store,
    selection: &Selection,
    start: i64,
    end: i64,
    timezone: chrono_tz::Tz,
) -> Result<Value> {
    ensure!(
        start >= 0 && start <= end && chrono::DateTime::from_timestamp(end, 0).is_some(),
        "Invalid speed statistics time range"
    );
    let mut days: BTreeMap<String, DailyTotals> = BTreeMap::new();
    let mut selected = 0usize;
    let mut valid = 0usize;
    selection.visit_rows(store, true, false, |row| {
        selected += 1;
        let Some(sample) = sample(&row) else {
            return Ok(());
        };
        let date = timezone
            .timestamp_opt(row.occurred_at, 0)
            .single()
            .context("Invalid speed timestamp")?
            .date_naive()
            .to_string();
        let day = days.entry(date).or_default();
        day.output_tokens += row.tokens.output as f64;
        day.generation_ms += sample.generation_ms as f64;
        day.first_token_ms += sample.first_ms as f64;
        day.sample_count += 1;
        day.estimated_sample_count += usize::from(sample.estimated);
        valid += 1;
        Ok(())
    })?;
    // Missing dates stay absent; the shared calendar paints them as unknown, not zero.
    let days: Vec<Value> = days
        .into_iter()
        .map(|(date, totals)| {
            let mut value = serde_json::to_value(totals).expect("finite daily totals");
            value["date"] = json!(date);
            value
        })
        .collect();
    Ok(json!({"days":days,"selectedRecordCount":selected,"excludedRecordCount":selected-valid}))
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Distribution {
    sample_count: usize,
    mean: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
    central_lower: Option<f64>,
    central_upper: Option<f64>,
    central_sample_count: usize,
}

fn distribution(mut values: Vec<f64>, mean: Option<f64>) -> Distribution {
    if values.is_empty() {
        return Distribution::default();
    }
    values.sort_unstable_by(f64::total_cmp);
    let n = values.len();
    // Discard floor(N / 8) from each tail; keep observed endpoints, not interpolation.
    let lower = values[n / 8];
    let upper = values[n - 1 - n / 8];
    let central_sample_count = values.iter().filter(|&&v| v >= lower && v <= upper).count();
    Distribution {
        sample_count: n,
        mean,
        min: values.first().copied(),
        max: values.last().copied(),
        central_lower: Some(lower),
        central_upper: Some(upper),
        central_sample_count,
    }
}

#[derive(Default)]
struct Accumulator {
    speeds: Vec<f64>,
    firsts: Vec<f64>,
    output: f64,
    generation_ms: f64,
    first_ms: f64,
    estimated_samples: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Statistics {
    speed: Distribution,
    ttft: Distribution,
    estimated_sample_count: usize,
}
impl Accumulator {
    fn add(&mut self, output: u64, generation_ms: i64, first_ms: i64, estimated: bool) {
        self.speeds
            .push(output as f64 * 1000.0 / generation_ms as f64);
        self.firsts.push(first_ms as f64 / 1000.0);
        self.output += output as f64;
        self.generation_ms += generation_ms as f64;
        self.first_ms += first_ms as f64;
        self.estimated_samples += usize::from(estimated);
    }
    fn finish(self) -> Statistics {
        let speed_mean =
            (self.generation_ms > 0.0).then(|| self.output * 1000.0 / self.generation_ms);
        let first_mean =
            (!self.firsts.is_empty()).then(|| self.first_ms / self.firsts.len() as f64 / 1000.0);
        Statistics {
            speed: distribution(self.speeds, speed_mean),
            ttft: distribution(self.firsts, first_mean),
            estimated_sample_count: self.estimated_samples,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bucket {
    start: i64,
    end: i64,
    #[serde(flatten)]
    statistics: Statistics,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    start_date: i64,
    end_date: i64,
    bucket_seconds: i64,
    selected_record_count: usize,
    excluded_record_count: usize,
    summary: Statistics,
    buckets: Vec<Bucket>,
}

fn bucket_seconds(span: i64) -> i64 {
    match span {
        ..=21_601 => 300,
        21_602..=86_401 => 900,
        86_402..=604_801 => 3600,
        604_802..=2_678_401 => 21_600,
        2_678_402..=15_552_001 => 86_400,
        // A fixed duration aligned to UTC; bound "all time" to at most 241 buckets.
        _ => ((span + 239 * 86_400 - 1) / (239 * 86_400)) * 86_400,
    }
}

pub(super) fn trends(
    store: &Store,
    selection: &Selection,
    mut start: i64,
    end: i64,
) -> Result<Value> {
    ensure!(
        start >= 0 && start <= end,
        "Invalid speed statistics time range"
    );
    // Validate before arithmetic/allocation; frontend timestamps are epoch seconds.
    ensure!(
        chrono::DateTime::from_timestamp(end, 0).is_some(),
        "Invalid end timestamp"
    );
    if start == 0 {
        let first: Option<i64> = store.conn.query_row(
            &format!(
                "SELECT MIN(u.occurred_at) FROM usage_index u WHERE {}",
                selection.sql
            ),
            rusqlite::params_from_iter(selection.params.iter()),
            |r| r.get(0),
        )?;
        start = first.unwrap_or_else(|| end.saturating_sub(86_400)).max(0);
    }
    let exclusive_end = end.checked_add(1).context("Invalid end timestamp")?;
    let step = bucket_seconds(exclusive_end - start);
    let aligned = start.div_euclid(step) * step;
    let count = ((end - aligned) / step + 1) as usize;
    ensure!(count <= 241, "Too many speed buckets");
    let mut groups: Vec<Accumulator> = (0..count).map(|_| Accumulator::default()).collect();
    let mut total = Accumulator::default();
    let mut selected = 0;
    selection.visit_rows(store, true, false, |r| {
        selected += 1;
        let Some(sample) = sample(&r) else {
            return Ok(());
        };
        let index = ((r.occurred_at - aligned) / step) as usize;
        groups[index].add(
            r.tokens.output,
            sample.generation_ms,
            sample.first_ms,
            sample.estimated,
        );
        total.add(
            r.tokens.output,
            sample.generation_ms,
            sample.first_ms,
            sample.estimated,
        );
        Ok(())
    })?;
    let summary = total.finish();
    let excluded = selected - summary.speed.sample_count;
    let buckets = groups
        .into_iter()
        .enumerate()
        .map(|(i, group)| {
            let begin = aligned + i as i64 * step;
            Bucket {
                start: begin.max(start),
                end: (begin + step).min(exclusive_end),
                statistics: group.finish(),
            }
        })
        .collect();
    Ok(serde_json::to_value(Response {
        start_date: start,
        end_date: end,
        bucket_seconds: step,
        selected_record_count: selected,
        excluded_record_count: excluded,
        summary,
        buckets,
    })?)
}
