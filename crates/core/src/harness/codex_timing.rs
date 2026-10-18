//! Adapted from CC Switch e8880e6 RequestTimer (MIT; third_party/UPSTREAM.md).
//! Boundaries estimate request assembly time; these are never proxy measurements.
use super::files;
use crate::model::UsageTiming;
use serde_json::Value;

#[derive(Default)]
pub(super) struct RequestTimer {
    boundary: Option<i64>,
    last_token: Option<i64>,
    start: Option<i64>,
    last_output: Option<i64>,
    tool_after_output: Option<i64>,
    usage_end: Option<(i64, [u64; 3])>,
    first: Option<i64>,
}
fn millis(v: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|t| t.timestamp_millis())
}
impl RequestTimer {
    pub fn observe(&mut self, v: &Value) {
        let Some(at) = millis(&v["timestamp"]) else {
            return;
        };
        let p = &v["payload"];
        match v["type"].as_str().unwrap_or_default() {
            "turn_context" => self.boundary = self.boundary.max(Some(at)),
            "token_usage_record" => {
                let u = &p["usage"];
                if u["input_tokens"].is_u64() && u["output_tokens"].is_u64() {
                    self.usage_end = Some((
                        at,
                        [
                            files::number(u, "input_tokens"),
                            files::number(u, "cached_input_tokens"),
                            files::number(u, "output_tokens"),
                        ],
                    ));
                }
            }
            "event_msg" => match p["type"].as_str().unwrap_or_default() {
                "task_started" | "turn_aborted" => {
                    *self = Self {
                        boundary: self.boundary.max(Some(at)),
                        last_token: self.last_token,
                        ..Self::default()
                    }
                }
                "item_completed"
                    if matches!(
                        p["item"]["type"].as_str(),
                        Some("Reasoning" | "AgentMessage")
                    ) =>
                {
                    if let Some(first) = p["started_at_ms"].as_i64().filter(|t| *t <= at) {
                        // Old layouts flush output items after the token counter.
                        if self.start.is_none() && self.last_token.is_some_and(|t| at - t <= 100) {
                            return;
                        }
                        self.first = Some(self.first.map_or(first, |old| old.min(first)));
                    }
                }
                _ => {}
            },
            "response_item" => {
                let kind = p["type"].as_str().unwrap_or_default();
                if kind == "message" && p["role"] != "assistant" {
                    self.boundary = self.boundary.max(Some(at));
                } else if kind.ends_with("_output") {
                    self.boundary = self.boundary.max(Some(at));
                    if self.last_output.is_some() {
                        self.tool_after_output = Some(at);
                    }
                } else if kind == "reasoning" || kind == "message" || kind.ends_with("_call") {
                    if self.start.is_none() && self.last_token.is_some_and(|t| at - t <= 100) {
                        return;
                    }
                    if self.start.is_none() {
                        self.start = self.boundary;
                    }
                    self.last_output = Some(at);
                    self.tool_after_output = None;
                }
            }
            _ => {}
        }
    }
    // Called only for a new, nonzero usage event: repeated snapshots aren't requests.
    pub fn finish(&mut self, v: &Value, usage: [u64; 5]) -> Option<UsageTiming> {
        let token_at = millis(&v["timestamp"]);
        let start = self.start.or(self.boundary);
        let record = self
            .usage_end
            .filter(|(_, u)| *u == [usage[0], usage[2], usage[1]])
            .map(|(at, _)| at);
        let waited_for_tool = self
            .tool_after_output
            .is_some_and(|t| token_at.is_none_or(|at| at - t <= 100));
        let (end, source) = if let Some(at) = record {
            (Some(at), "codex_token_usage_record")
        } else if waited_for_tool {
            (self.last_output, "codex_model_output_before_tool")
        } else {
            (token_at, "codex_token_count")
        };
        let first = self.first;
        *self = Self {
            boundary: self.boundary.max(token_at),
            last_token: token_at,
            ..Self::default()
        };
        UsageTiming::estimate(start?, end?, first, source, "codex_item_started_at_ms")
    }
}
