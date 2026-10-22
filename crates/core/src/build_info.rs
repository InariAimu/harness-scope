/// Build metadata contains only release identifiers, never local paths or configuration.
pub fn current() -> serde_json::Value {
    serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/build-info.json")))
        .expect("valid generated build metadata")
}
