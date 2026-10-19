//! Native route IDs are evidence of client routing, not verified endpoint ownership.
use crate::model::UsageRecord;

pub fn parse(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|id| valid(id))
        .map(str::to_owned)
}
pub fn valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | ' '))
}
pub fn pricing_id(id: &str) -> &str {
    match id {
        "deepseek" => "deepseek-official",
        other => other,
    }
}
pub fn name(record: &UsageRecord) -> String {
    record
        .provider
        .clone()
        .unwrap_or_else(|| format!("{} (Session)", record.harness))
}
