//! Request-local observations; never infer boundaries from adjacent turns or tools.
use crate::model::{RequestMetadata, UsageTiming};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

type Key = (String, String);

#[derive(PartialEq, Eq)]
struct Start {
    at: Option<i64>,
    model: Option<String>,
    effort: Option<String>,
}

struct Output {
    reference: Option<String>,
    first: Option<i64>,
    last: Option<i64>,
    ambiguous: bool,
    complete: bool,
}

#[derive(Default)]
pub(super) struct Requests {
    starts: BTreeMap<Key, Option<Start>>,
    outputs: BTreeMap<Key, Output>,
    owners: BTreeMap<Key, BTreeSet<String>>,
}

fn millis(v: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|t| t.timestamp_millis())
}

fn effort(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|s| RequestMetadata::valid_effort(s))
        .map(str::to_owned)
}

impl Requests {
    pub fn observe(&mut self, session: &str, v: &Value) {
        if v["type"] == "api-request" {
            let Some(id) = v["id"].as_str().filter(|s| !s.is_empty()) else {
                return;
            };
            let start = Start {
                at: millis(&v["timestamp"]),
                model: v["params"]["model"].as_str().map(str::to_owned),
                effort: effort(&v["params"]["output_config"]["effort"]),
            };
            self.starts
                .entry((session.into(), id.into()))
                .and_modify(|old| {
                    if old.as_ref() != Some(&start) {
                        *old = None;
                    }
                })
                .or_insert(Some(start));
        } else if v["type"] == "assistant" {
            let Some(id) = v["message"]["id"].as_str() else {
                return;
            };
            let reference = v["requestRef"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            if let Some(reference) = &reference {
                self.owners
                    .entry((session.into(), reference.clone()))
                    .or_default()
                    .insert(id.into());
            }
            let at = millis(&v["timestamp"]);
            let complete = v["message"]["stop_reason"]
                .as_str()
                .is_some_and(|s| !s.is_empty());
            self.outputs
                .entry((session.into(), id.into()))
                .and_modify(|old| {
                    old.ambiguous |= old.reference != reference || at.is_none();
                    old.first = old.first.zip(at).map(|(a, b)| a.min(b));
                    old.last = old.last.zip(at).map(|(a, b)| a.max(b));
                    old.complete |= complete;
                })
                .or_insert(Output {
                    reference,
                    first: at,
                    last: at,
                    ambiguous: at.is_none(),
                    complete,
                });
        }
    }

    fn linked(&self, session: &str, id: &str) -> Option<(&Start, &Output)> {
        let output = self.outputs.get(&(session.into(), id.into()))?;
        if output.ambiguous {
            return None;
        }
        let key = (session.into(), output.reference.clone()?);
        if self.owners.get(&key)?.len() != 1 {
            return None;
        }
        Some((self.starts.get(&key)?.as_ref()?, output))
    }

    pub fn timing(&self, session: &str, id: &str) -> Option<UsageTiming> {
        let (start, output) = self.linked(session, id)?;
        if !output.complete || output.first? < start.at? {
            return None;
        }
        // A timestamp belongs to a completed content block, not a streamed token.
        UsageTiming::estimate(
            start.at?,
            output.last?,
            None,
            "claude_api_request_blocks",
            "",
        )
    }

    pub fn metadata(&self, session: &str, id: &str, v: &Value) -> Option<RequestMetadata> {
        let service_tier = v["message"]["usage"]["service_tier"]
            .as_str()
            .filter(|s| RequestMetadata::valid_tier(s))
            .map(str::to_owned);
        let reasoning_effort = self
            .linked(session, id)
            .and_then(|(start, _)| start.effort.clone())
            .or_else(|| effort(&v["perTurnEffort"]))
            .or_else(|| effort(&v["effort"]));
        let metadata = RequestMetadata {
            service_tier_source: service_tier.as_ref().map(|_| "claude_usage".into()),
            service_tier,
            reasoning_effort,
        };
        (metadata != RequestMetadata::default()).then_some(metadata)
    }
}
