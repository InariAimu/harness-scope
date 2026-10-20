//! Local Codex login supplies quota. Reference dollar budgets never alter billing records.
use super::*;
use std::path::{Path, PathBuf};
pub(super) mod calibration;
pub(crate) mod catalog;
mod reference;
mod transport;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub plan: String,
    #[serde(default)]
    pub binary_path: Option<String>,
    #[serde(default)]
    pub home_path: Option<String>,
}
pub(super) fn validate(settings: &Settings) -> Result<()> {
    multiplier(&settings.plan)?;
    for path in [&settings.binary_path, &settings.home_path]
        .into_iter()
        .flatten()
    {
        anyhow::ensure!(
            path.len() <= 4096
                && Path::new(path).is_absolute()
                && !path.contains(['\n', '\r', '\0']),
            "Codex paths must be absolute paths"
        );
    }
    Ok(())
}
fn multiplier(plan: &str) -> Result<u32> {
    match plan {
        "plus" => Ok(1),
        "pro100" => Ok(5),
        "pro200" => Ok(10),
        "pro500" => Ok(25),
        _ => anyhow::bail!("Choose Plus, Pro 100, Pro 200 or Pro 500"),
    }
}
fn binary(settings: &Settings) -> Result<PathBuf> {
    if let Some(path) = &settings.binary_path {
        let path = PathBuf::from(path);
        anyhow::ensure!(
            path.is_file(),
            "Codex executable not found; configure its absolute path"
        );
        return Ok(path);
    }
    let mut candidates = vec![];
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join("Codex/versions/current/resources/codex.exe"));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for path in std::env::split_paths(&paths) {
            candidates.push(path.join(if cfg!(windows) { "codex.exe" } else { "codex" }));
            #[cfg(windows)]
            for package in ["codex", "codex-win32-x64", "codex-win32-arm64"] {
                for arch in ["x86_64", "aarch64"] {
                    candidates.push(path.join(format!("node_modules/@openai/{package}/vendor/{arch}-pc-windows-msvc/codex/codex.exe")));
                }
            }
        }
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .context("Codex executable not found; configure its absolute path")
}
pub(super) fn fetch(settings: &Settings) -> Result<Value> {
    validate(settings)?;
    let (account, limits) = transport::query(&binary(settings)?, settings.home_path.as_deref())?;
    parse(&account, &limits)
}
fn parse(account: &Value, limits: &Value) -> Result<Value> {
    anyhow::ensure!(
        matches!(
            account["account"]["type"].as_str(),
            Some("chatgpt" | "chatgptAuthTokens")
        ),
        "Codex subscription requires a local ChatGPT login; sign in with Codex first"
    );
    let short = |v: &Value| v.as_str().filter(|s| s.len() <= 128).map(str::to_owned);
    let plan = short(&account["account"]["planType"]);
    let mut raw = vec![];
    if let Some(buckets) = limits["rateLimitsByLimitId"]
        .as_object()
        .filter(|m| !m.is_empty())
    {
        anyhow::ensure!(buckets.len() <= 32, "Too many Codex quota buckets");
        for (id, bucket) in buckets {
            raw.push((id.clone(), bucket));
        }
    } else if limits["rateLimits"].is_object() {
        raw.push((
            short(&limits["rateLimits"]["limitId"]).unwrap_or_else(|| "codex".into()),
            &limits["rateLimits"],
        ));
    }
    anyhow::ensure!(!raw.is_empty(), "Codex returned no subscription quota");
    let mut buckets = Vec::new();
    for (id, bucket) in raw {
        anyhow::ensure!(
            !id.is_empty() && id.len() <= 128,
            "Invalid Codex quota bucket"
        );
        let mut windows = Vec::new();
        for key in ["primary", "secondary"] {
            let row = &bucket[key];
            if row.is_null() {
                continue;
            }
            let used = row["usedPercent"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 10000.0)
                .context("Invalid Codex quota percentage")?;
            let minutes = row["windowDurationMins"]
                .as_i64()
                .filter(|v| *v > 0 && *v <= 527040)
                .context("Invalid Codex quota window")?;
            let resets = row["resetsAt"]
                .as_i64()
                .filter(|v| *v > 0)
                .context("Invalid Codex reset time")?;
            windows.push(json!({"id":key,"usedPercent":used,"durationMinutes":minutes,"resetsAt":resets,"status":if used>=100.0 {"rate-limited"} else {"ok"}}));
        }
        let credits=bucket["credits"].as_object().map(|c|json!({"hasCredits":c.get("hasCredits").and_then(Value::as_bool),"unlimited":c.get("unlimited").and_then(Value::as_bool),"balance":c.get("balance").and_then(Value::as_str).filter(|s|s.len()<=80).and_then(|s|Decimal::from_str(s).ok()).filter(|v|*v>=Decimal::ZERO).map(|v|v.normalize().to_string())}));
        buckets.push(json!({"id":id,"name":short(&bucket["limitName"]),"planType":short(&bucket["planType"]).or_else(||plan.clone()),"windows":windows,"credits":credits}));
    }
    // Hash upstream identity; never persist emails, IDs, tokens, or arbitrary response fields.
    let identity = limits["accountId"]
        .as_str()
        .or_else(|| account["account"]["email"].as_str())
        .map(|s| {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(s.as_bytes()))
        });
    Ok(
        json!({"kind":"codexQuota","planType":plan,"identity":identity,"identitySource":if limits["accountId"].as_str().is_some_and(|s|!s.is_empty()){"accountId"}else{"email"},"buckets":buckets}),
    )
}
pub(super) fn estimate(engine: &Engine, args: &Value) -> Result<Value> {
    reference::calculate(engine, args)
}

#[cfg(test)]
mod tests;
