use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Instant,
};
const MAX_LINE: u64 = 1024 * 1024;
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub(super) fn query(binary: &Path, home: Option<&str>) -> Result<(Value, Value)> {
    let mut command = Command::new(binary);
    command
        .args([
            "app-server",
            "--listen",
            "stdio://",
            "-c",
            "analytics.enabled=false",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(home) = home {
        anyhow::ensure!(Path::new(home).is_dir(), "Codex home directory not found");
        command.env("CODEX_HOME", home);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW for the background helper.
    }
    let mut process = Process(
        command
            .spawn()
            .map_err(|_| anyhow::anyhow!("Cannot start Codex account query"))?,
    );
    let mut input = process.0.stdin.take().context("Cannot open Codex input")?;
    let output = process
        .0
        .stdout
        .take()
        .context("Cannot open Codex output")?;
    let (send, receive) = mpsc::sync_channel::<Result<Value>>(16);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(output);
        let mut total = 0usize;
        loop {
            let mut line = String::new();
            let result = (|| -> Result<Option<Value>> {
                let size = Read::by_ref(&mut reader)
                    .take(MAX_LINE + 1)
                    .read_line(&mut line)?;
                if size == 0 {
                    return Ok(None);
                }
                total += size;
                anyhow::ensure!(
                    size <= MAX_LINE as usize && total <= 4 * MAX_LINE as usize,
                    "Codex response exceeded size limit"
                );
                Ok(Some(
                    serde_json::from_str(&line)
                        .map_err(|_| anyhow::anyhow!("Invalid Codex response"))?,
                ))
            })();
            match result {
                Ok(None) => break,
                Ok(Some(value)) => {
                    if send.send(Ok(value)).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = send.send(Err(anyhow::anyhow!("Invalid or oversized Codex response")));
                    break;
                }
            }
        }
    });
    exchange(&mut input, receive)
}

fn exchange(
    input: &mut impl Write,
    receive: mpsc::Receiver<Result<Value>>,
) -> Result<(Value, Value)> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut send_message = |message: Value| -> Result<()> {
        writeln!(input, "{message}").context("Cannot write Codex request")?;
        input.flush().context("Cannot flush Codex request")
    };
    let response = |id: i64| -> Result<Value> {
        loop {
            let value = receive
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| {
                    anyhow::anyhow!("Codex account query timed out; check local login")
                })??;
            if value["id"].as_i64() == Some(id) {
                anyhow::ensure!(
                    value.get("error").is_none(),
                    "Codex account query failed; check local ChatGPT login and Codex version"
                );
                return value
                    .get("result")
                    .cloned()
                    .context("Invalid Codex account response");
            }
            // These methods never require tools, model turns, or authentication mutation requests.
            anyhow::ensure!(Instant::now() < deadline, "Codex account query timed out");
        }
    };
    send_message(
        json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"harness_scope","title":"Harness Scope","version":env!("CARGO_PKG_VERSION")}}}),
    )?;
    response(1)?;
    send_message(json!({"method":"initialized","params":{}}))?;
    send_message(json!({"id":2,"method":"account/read","params":{"refreshToken":false}}))?;
    let account = response(2)?;
    anyhow::ensure!(
        matches!(
            account["account"]["type"].as_str(),
            Some("chatgpt" | "chatgptAuthTokens")
        ),
        "Codex subscription requires a local ChatGPT login; sign in with Codex first"
    );
    send_message(json!({"id":3,"method":"account/rateLimits/read"}))?;
    let limits = response(3)?;
    Ok((account, limits))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_exchange_only_initializes_and_reads_without_starting_turns_or_refreshing_tokens() {
        let (sender, receiver) = mpsc::channel();
        for value in [
            json!({"id":1,"result":{}}),
            json!({"method":"account/updated","params":{"planType":"prolite"}}),
            json!({"id":2,"result":{"account":{"type":"chatgpt","planType":"prolite"}}}),
            json!({"id":3,"result":{"rateLimits":{"primary":null}}}),
        ] {
            sender.send(Ok(value)).unwrap();
        }
        let mut input = Vec::new();
        let (account, quota) = exchange(&mut input, receiver).unwrap();
        assert_eq!(account["account"]["planType"], "prolite");
        assert!(quota["rateLimits"].is_object());
        let sent: Vec<Value> = String::from_utf8(input)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            sent.iter()
                .map(|v| v["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "initialize",
                "initialized",
                "account/read",
                "account/rateLimits/read"
            ]
        );
        assert_eq!(sent[2]["params"]["refreshToken"], false);
    }
    #[test]
    fn upstream_errors_and_api_key_logins_never_leak_tokens_or_query_subscription_as_api_usage() {
        for account in [
            json!({"id":2,"error":{"code":401,"message":"secret-access-token"}}),
            json!({"id":2,"result":{"account":{"type":"apiKey"}}}),
        ] {
            let (sender, receiver) = mpsc::channel();
            sender.send(Ok(json!({"id":1,"result":{}}))).unwrap();
            sender.send(Ok(account)).unwrap();
            let mut input = Vec::new();
            let error = exchange(&mut input, receiver).unwrap_err().to_string();
            assert!(!error.contains("secret-access-token"));
            assert!(!String::from_utf8(input)
                .unwrap()
                .contains("account/rateLimits/read"));
        }
    }
}
