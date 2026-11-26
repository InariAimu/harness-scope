use super::Link;
use anyhow::{ensure, Context, Result};
use chrono::DateTime;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub(super) fn linked(links: &[Link]) -> bool {
    links
        .iter()
        .any(|link| link.harness == "antigravity" && link.provider == "antigravity (Session)")
}

pub(super) fn fetch() -> Result<Value> {
    let binary = find_binary().context("Antigravity CLI executable not found")?;
    let version = output(&binary, &["--version"], Duration::from_secs(5))?;
    ensure!(
        version_tuple(std::str::from_utf8(&version)?).is_some_and(|v| v >= [1, 1, 11]),
        "Antigravity CLI 1.1.11 or later is required"
    );
    let args = [
        "--print",
        "/usage",
        "--output-format",
        "json",
        "--print-timeout",
        "15s",
        "--disable-slash-commands=false",
        "--settings",
        r#"{"disableSlashCommands":false}"#,
        "--log-file",
        if cfg!(windows) { "NUL" } else { "/dev/null" },
    ];
    parse_report(&output(&binary, &args, Duration::from_secs(23))?)
}

fn find_binary() -> Option<PathBuf> {
    let standard = if cfg!(windows) {
        dirs::data_local_dir().map(|path| path.join("agy/bin/agy.exe"))
    } else {
        dirs::home_dir().map(|path| path.join(".local/bin/agy"))
    };
    let filename = if cfg!(windows) { "agy.exe" } else { "agy" };
    let path = std::env::var_os("PATH").unwrap_or_default();
    standard
        .into_iter()
        .chain(std::env::split_paths(&path).map(|dir| dir.join(filename)))
        .find(|candidate| candidate.is_absolute() && candidate.is_file())
}

fn version_tuple(output: &str) -> Option<[u32; 3]> {
    let output = output.trim();
    let mut parts = output.strip_prefix('v').unwrap_or(output).split('.');
    let version = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    parts.next().is_none().then_some(version)
}

fn output(binary: &Path, args: &[&str], timeout: Duration) -> Result<Vec<u8>> {
    const LIMIT: u64 = 256 * 1024;
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(std::env::temp_dir())
        .env("AGY_CLI_DISABLE_AUTO_UPDATE", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().context("Cannot start Antigravity CLI")?;
    let stdout = child.stdout.take().expect("piped CLI output");
    let (send, receive) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(LIMIT + 1).read_to_end(&mut bytes);
        let _ = send.send(result.map(|_| bytes));
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                result.context("Cannot wait for Antigravity CLI")?;
                anyhow::bail!("Antigravity CLI query timed out");
            }
        }
    };
    let bytes = receive
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .context("Antigravity CLI output timed out")?
        .context("Cannot read Antigravity CLI output")?;
    ensure!(
        bytes.len() as u64 <= LIMIT,
        "Antigravity CLI output is too large"
    );
    ensure!(
        status.success(),
        "Antigravity CLI query failed; check the local Antigravity login"
    );
    Ok(bytes)
}

fn display_text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
}

fn parse_report(bytes: &[u8]) -> Result<Value> {
    let raw: Value =
        serde_json::from_slice(bytes).context("Invalid Antigravity CLI quota report")?;
    ensure!(
        raw["status"] == "SUCCESS"
            && raw["num_turns"].as_u64() == Some(0)
            && raw["command"]["name"] == "usage",
        "Antigravity CLI did not return a quota report; check the local Antigravity login"
    );
    let groups = raw["command"]["data"]["groups"]
        .as_array()
        .context("Missing Antigravity quota groups")?;
    let mut ids = HashSet::new();
    let mut result = Vec::new();
    for group in groups {
        let (Some(name), Some(buckets)) =
            (display_text(&group["name"]), group["buckets"].as_array())
        else {
            continue;
        };
        let mut windows = Vec::new();
        for bucket in buckets {
            if bucket["disabled"] == true {
                continue;
            }
            let Some(id) = display_text(&bucket["id"]) else {
                continue;
            };
            let Some(remaining) = bucket["remaining_fraction"]
                .as_f64()
                .filter(|n| (0.0..=1.0).contains(n))
            else {
                continue;
            };
            ensure!(ids.insert(id), "Duplicate Antigravity quota window");
            let duration = match bucket["window"].as_str() {
                Some("5h") => Some(300),
                Some("weekly") => Some(10_080),
                _ => None,
            };
            let resets_at = bucket["reset_time"]
                .as_str()
                .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
                .map(|v| v.timestamp())
                .filter(|&v| v > 0);
            windows.push(json!({
                "id": id,
                "name": display_text(&bucket["name"]).unwrap_or(id),
                "durationMinutes": duration,
                "usedPercent": (1.0 - remaining) * 100.0,
                "status": if remaining == 0.0 { "rate-limited" } else { "ok" },
                "resetsAt": resets_at,
            }));
        }
        if let Some(first) = windows.first() {
            result.push(json!({"id": first["id"], "name": name, "windows": windows}));
        }
    }
    ensure!(
        !result.is_empty(),
        "No Antigravity subscription quota was reported"
    );
    Ok(json!({"kind": "antigravityQuota", "groups": result}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_preserves_groups_and_missing_reset_without_retaining_raw_output() {
        let report = json!({
            "status": "SUCCESS",
            "num_turns": 0,
            "response": "unstructured private content",
            "command": {"name": "usage", "data": {"groups": [
                {"name": "Gemini Models", "buckets": [
                    {"id": "gemini-weekly", "window": "weekly", "remaining_fraction": 0.75, "reset_time": "2026-12-03T07:37:52Z"},
                    {"id": "gemini-5h", "window": "5h", "remaining_fraction": 0, "reset_time": "2026-11-26T12:37:52Z"}
                ]},
                {"name": "Claude and GPT models", "buckets": [
                    {"id": "3p-weekly", "window": "weekly", "remaining_fraction": 1, "reset_time": "1970-01-01T00:00:00Z"},
                    {"id": "3p-5h", "window": "5h", "remaining_fraction": 1, "disabled": true},
                    {"id": "missing-fraction", "window": "5h"}
                ]}
            ]}}
        });
        let parsed = parse_report(report.to_string().as_bytes()).unwrap();
        assert_eq!(parsed["groups"].as_array().unwrap().len(), 2);
        let gemini = &parsed["groups"][0]["windows"];
        assert_eq!(gemini[0]["usedPercent"], 25.0);
        assert_eq!(gemini[0]["durationMinutes"], 10_080);
        assert_eq!(gemini[0]["resetsAt"], 1_796_283_472i64);
        assert_eq!(gemini[1]["usedPercent"], 100.0);
        assert_eq!(gemini[1]["status"], "rate-limited");
        let third_party = &parsed["groups"][1]["windows"];
        assert_eq!(third_party.as_array().unwrap().len(), 1);
        assert_eq!(third_party[0]["usedPercent"], 0.0);
        assert!(third_party[0]["resetsAt"].is_null());
        assert!(!parsed.to_string().contains("private content"));

        let mut report = report;
        report["num_turns"] = json!(1);
        assert!(parse_report(report.to_string().as_bytes()).is_err());
        report["num_turns"] = json!(0);
        report["command"]["data"]["groups"][0]["buckets"][1]["id"] = json!("gemini-weekly");
        assert!(parse_report(report.to_string().as_bytes()).is_err());
    }
}
