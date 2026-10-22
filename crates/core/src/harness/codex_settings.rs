//! Bind settings at turn boundaries; a settings change during an in-flight turn
//! belongs to the next turn. Do not infer missing history from current config.
use crate::model::RequestMetadata;
use serde_json::Value;

#[derive(Default)]
pub(super) struct Settings {
    pending: Option<(Option<String>, Option<String>)>,
    active: Option<RequestMetadata>,
    runtime_allowed: bool,
}
impl Settings {
    pub fn observe(&mut self, row: &Value) {
        let p = &row["payload"];
        if row["type"] == "session_meta" {
            self.pending = None;
            self.active = None;
            self.runtime_allowed = false;
        } else if row["type"] == "event_msg" && p["type"] == "task_started" {
            self.active = None;
            self.runtime_allowed = false;
        } else if row["type"] == "event_msg" && p["type"] == "thread_settings_applied" {
            let settings = &p["thread_settings"];
            self.pending = Some((
                settings["model"].as_str().map(str::to_owned),
                tier(&settings["service_tier"]),
            ));
        } else if row["type"] == "turn_context" {
            self.runtime_allowed = p.get("service_tier").is_none();
            let direct = tier(&p["service_tier"]);
            let inherited = self.pending.as_ref().and_then(|(model, tier)| {
                // A different model's retained settings do not describe this turn.
                (model.as_deref() == p["model"].as_str())
                    .then(|| tier.clone())
                    .flatten()
            });
            let source = if direct.is_some() {
                "turn_context"
            } else {
                "thread_settings"
            };
            let service_tier = if p.get("service_tier").is_some() {
                direct
            } else {
                inherited
            };
            let reasoning_effort = p["effort"]
                .as_str()
                .filter(|v| RequestMetadata::valid_effort(v))
                .map(str::to_owned);
            let metadata = RequestMetadata {
                service_tier_source: service_tier.as_ref().map(|_| source.into()),
                service_tier,
                reasoning_effort,
            };
            self.active = (metadata != RequestMetadata::default()).then_some(metadata);
        }
    }
    pub fn metadata(&self, row: &Value, model: &str) -> Option<RequestMetadata> {
        let mut metadata = self.active.clone().unwrap_or_default();
        let runtime = &row["_scopeRuntimeTier"];
        if metadata.service_tier.is_none() && self.runtime_allowed && runtime["model"] == model {
            metadata.service_tier = tier(&runtime["tier"]);
            // The feedback tag is a snapshot of effective local thread settings.
            // Keep the existing wire source; this is not server-confirmed billing.
            metadata.service_tier_source = metadata
                .service_tier
                .as_ref()
                .map(|_| "thread_settings".into());
        }
        (metadata != RequestMetadata::default()).then_some(metadata)
    }
}
fn tier(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|v| *v != "standard" && RequestMetadata::valid_tier(v))
        .map(str::to_owned)
}
