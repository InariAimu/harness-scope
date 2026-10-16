use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn discover(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        anyhow::bail!("Log directory does not exist: {}", root.display());
    }
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_file()
            && (entry.path().extension().is_some_and(|e| e == "jsonl")
                || entry.path().to_string_lossy().ends_with(".jsonl.zst"))
        {
            paths.push(entry.into_path());
        }
    }
    paths.sort();
    Ok(paths)
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn read(path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
    let bytes = read_bytes(path)?;
    let text = std::str::from_utf8(&bytes).context("Log is not UTF-8")?;
    let mut values = Vec::new();
    let mut errors = Vec::new();
    let parts: Vec<&str> = text.split('\n').collect();
    let mut offset = 0;
    for (i, part) in parts.iter().enumerate() {
        let begin = offset;
        offset += part.len() + 1;
        if part.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(part) {
            Ok(mut v) => {
                if let Some(m) = v.as_object_mut() {
                    m.insert("_scopeOffset".into(), serde_json::json!(begin));
                    m.insert("_scopeLen".into(), serde_json::json!(part.len()));
                }
                values.push(v)
            }
            Err(e) => errors.push(format!(
                "{}:{}: {}{}",
                path.display(),
                i + 1,
                if i + 1 == parts.len() {
                    "Incomplete tail: "
                } else {
                    "Invalid JSON: "
                },
                e
            )),
        }
    }
    let complete = errors.is_empty();
    Ok((values, hash(&bytes), errors, complete))
}
pub fn timestamp(v: &Value) -> Option<i64> {
    v.as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp())
}
pub fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path).with_context(|| format!("Read {}", path.display()))?;
    if path.to_string_lossy().ends_with(".zst") {
        Ok(zstd::stream::decode_all(file)?)
    } else {
        Ok(fs::read(path)?)
    }
}
pub fn number(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

pub fn validate_usage(v: &Value) -> Result<()> {
    for required in ["input_tokens", "output_tokens"] {
        anyhow::ensure!(
            v.get(required).and_then(Value::as_u64).is_some(),
            "Missing or invalid {required}"
        );
    }
    for (key, value) in v.as_object().context("Usage must be an object")? {
        if key.ends_with("tokens") && !value.is_null() {
            anyhow::ensure!(
                value.as_u64().is_some_and(|n| n <= 1_000_000_000_000),
                "Invalid token counter: {key}"
            );
        }
    }
    Ok(())
}
pub fn text(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.into();
    }
    if let Some(xs) = v.as_array() {
        return xs
            .iter()
            .map(|x| x.get("text").and_then(Value::as_str).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}
pub fn allowed_path(root: &Path, path: &Path) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let path = path.canonicalize()?;
    anyhow::ensure!(
        path.starts_with(root) && path.is_file(),
        "Session path is outside configured root"
    );
    Ok(path)
}
