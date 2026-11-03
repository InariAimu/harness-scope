//! Model-level measurements are not HTTP requests. Nullable source facts and
//! source price snapshots remain authoritative; Tokens is only a sum projection.
use crate::model::{Cost, Tokens};
use anyhow::{ensure, Result};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, str::FromStr};

pub const CAPABILITY: &str = "model_usage_v2";

/// Connection metadata captured by the producer, never credentials or a full URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelProvider {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// Historical records without endpoint evidence remain unknown.
    pub api_service: String,
}
impl ModelProvider {
    pub fn route(&self) -> String {
        if self.api_service == "unknown" {
            format!("gpt-img-studio:{}", self.id)
        } else {
            format!("gpt-img-studio:{}:{}", self.api_service, self.id)
        }
    }
    pub fn display_name(&self) -> String {
        if self.api_service == "openai-platform" {
            if self.name.eq_ignore_ascii_case("openai") {
                "OpenAI Platform (API)".into()
            } else {
                format!("{} · OpenAI Platform (API)", self.name)
            }
        } else {
            format!("{} (API)", self.name)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelTokens {
    pub total_input: Option<u64>,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    pub reasoning: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourcePriceSnapshot {
    pub source: String,
    pub currency: String,
    pub unit: String,
    pub rates: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ModelProvider>,
    pub source_device_id: String,
    pub request_id: String,
    pub operation: String,
    pub kind: String,
    pub usage_available: bool,
    pub cache_breakdown_known: bool,
    pub cache_write_applicable: bool,
    pub tokens: ModelTokens,
    pub text_input_tokens: Option<u64>,
    pub image_input_tokens: Option<u64>,
    pub estimated_cost_usd: Option<String>,
    pub estimate_status: String,
    pub pricing_snapshot: Option<SourcePriceSnapshot>,
    pub status_code: Option<u16>,
    /// Whole HTTP duration, not model decoding time and never used for tok/s.
    pub request_duration_ms: Option<i64>,
}

impl ModelUsage {
    pub fn projection(&self) -> Tokens {
        let t = &self.tokens;
        Tokens {
            input: if self.cache_breakdown_known {
                t.input.unwrap_or(0)
            } else {
                t.total_input.unwrap_or(0)
            },
            output: t.output.unwrap_or(0),
            cache_read: if self.cache_breakdown_known {
                t.cache_read.unwrap_or(0)
            } else {
                0
            },
            cache_write: if self.cache_breakdown_known {
                t.cache_write.unwrap_or(0)
            } else {
                0
            },
            reasoning: t.reasoning.unwrap_or(0),
            ..Default::default()
        }
    }

    pub fn cost(&self, model: &str, occurred_at: i64) -> Cost {
        Cost {
            amount_usd: self.estimated_cost_usd.clone(),
            method: if self.estimated_cost_usd.is_some() {
                "source_estimated"
            } else {
                "unknown"
            }
            .into(),
            price_source: "gpt-img-studio".into(),
            pricing_model: Some(model.into()),
            priced_at: occurred_at,
            rule_version: "model_usage_v1".into(),
            billing_basis: Some("model_reference".into()),
            ..Default::default()
        }
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(p) = &self.provider {
            ensure!(
                crate::providers::valid(&p.id)
                    && crate::providers::valid(&p.route())
                    && !p.name.trim().is_empty()
                    && p.name.len() <= 256
                    && !p.name.chars().any(char::is_control)
                    && ["openai", "deepseek", "opencode", "custom"].contains(&p.kind.as_str())
                    && ["openai-platform", "other", "unknown"].contains(&p.api_service.as_str()),
                "Invalid model provider snapshot"
            );
        }
        for id in [&self.source_device_id, &self.request_id] {
            ensure!(
                !id.is_empty()
                    && id.len() <= 128
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_:".contains(c)),
                "Invalid model usage identity"
            );
        }
        ensure!(
            ["image", "refine", "naming"].contains(&self.operation.as_str()),
            "Invalid model usage operation"
        );
        ensure!(
            ["image", "language"].contains(&self.kind.as_str()),
            "Invalid model usage kind"
        );
        let t = &self.tokens;
        for v in [
            t.total_input,
            t.input,
            t.output,
            t.cache_read,
            t.cache_write,
            t.reasoning,
            self.text_input_tokens,
            self.image_input_tokens,
        ]
        .into_iter()
        .flatten()
        {
            ensure!(v <= 1_000_000_000_000, "Invalid model token count");
        }
        ensure!(
            self.usage_available == (t.total_input.is_some() && t.output.is_some()),
            "Invalid usage availability"
        );
        if let Some(total) = t.total_input {
            ensure!(
                t.cache_read
                    .unwrap_or(0)
                    .saturating_add(t.cache_write.unwrap_or(0))
                    <= total,
                "Cache exceeds input"
            );
            if let (Some(text), Some(image)) = (self.text_input_tokens, self.image_input_tokens) {
                ensure!(
                    text.checked_add(image) == Some(total),
                    "Modality input differs from total"
                );
            }
        }
        if let Some(reasoning) = t.reasoning {
            ensure!(
                t.output.is_some_and(|v| reasoning <= v),
                "Reasoning exceeds output"
            );
        }
        if self.cache_breakdown_known {
            ensure!(
                t.cache_read.is_some() && (!self.cache_write_applicable || t.cache_write.is_some()),
                "Missing known cache breakdown"
            );
            if let Some(total) = t.total_input {
                ensure!(
                    t.input.is_some_and(|v| v
                        .checked_add(t.cache_read.unwrap())
                        .and_then(|n| n.checked_add(t.cache_write.unwrap_or(0)))
                        == Some(total)),
                    "Input breakdown differs from total"
                );
            }
        }
        ensure!(
            self.request_duration_ms
                .is_none_or(|v| (0..=3_600_000).contains(&v)),
            "Invalid request duration"
        );
        ensure!(
            ["estimated", "upper_bound", "unknown"].contains(&self.estimate_status.as_str()),
            "Invalid source estimate status"
        );
        ensure!(
            (self.estimate_status == "unknown") == self.estimated_cost_usd.is_none(),
            "Source estimate availability differs"
        );
        if let Some(amount) = &self.estimated_cost_usd {
            let amount = Decimal::from_str(amount)?;
            ensure!(
                self.usage_available
                    && self.pricing_snapshot.is_some()
                    && amount >= Decimal::ZERO
                    && amount < Decimal::from(1_000_000_000u64),
                "Invalid source estimate"
            );
        }
        if let Some(p) = &self.pricing_snapshot {
            ensure!(
                ["openai_reference", "provider_config"].contains(&p.source.as_str())
                    && p.currency == "USD"
                    && p.unit == "per_million_tokens",
                "Invalid source price provenance"
            );
            for s in [&p.as_of, &p.service_tier, &p.context_tier]
                .into_iter()
                .flatten()
            {
                ensure!(
                    s.len() <= 64 && !s.chars().any(char::is_control),
                    "Invalid source price metadata"
                );
            }
            for (key, value) in &p.rates {
                ensure!(
                    [
                        "input",
                        "cachedInput",
                        "cacheWrite",
                        "imageInput",
                        "cachedImageInput",
                        "output"
                    ]
                    .contains(&key.as_str()),
                    "Unknown source price rate"
                );
                let rate = Decimal::from_str(value)?;
                ensure!(
                    rate >= Decimal::ZERO && rate <= Decimal::from(1_000_000_000u64),
                    "Invalid source price rate"
                );
            }
        }
        Ok(())
    }
}
