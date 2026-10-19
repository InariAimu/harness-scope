//! Real-client sync validation; writes require an isolated test server database.
use anyhow::{ensure, Result};
use clap::Parser;
use harness_scope_core::{
    model::{ChangesResponse, Source},
    Engine,
};
use serde_json::json;
use std::{fs, path::PathBuf};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    url: String,
    /// File containing HARNESS_SCOPE_TOKEN=<token>; never put the token in argv.
    #[arg(long)]
    token_file: PathBuf,
    /// Check a production endpoint with an empty local client and no uploads.
    #[arg(long)]
    read_only: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let token_file = fs::read_to_string(args.token_file)?;
    let token = token_file
        .lines()
        .find_map(|line| line.strip_prefix("HARNESS_SCOPE_TOKEN="))
        .ok_or_else(|| anyhow::anyhow!("Token file is missing HARNESS_SCOPE_TOKEN"))?
        .trim()
        .to_owned();
    let directory = tempfile::tempdir()?;
    if args.read_only {
        let engine = Engine::open(&directory.path().join("read-only.db"))?;
        {
            let store = engine.lock()?;
            let mut config = store.config()?;
            config.sources.clear();
            config.server_url = args.url.clone();
            config.server_token = token;
            store.set_meta("config", &config)?;
        }
        let result = engine.sync()?;
        ensure!(result["uploaded"] == 0);
        println!("{}", json!({"url":args.url,"readOnly":true,"sync":result}));
        return Ok(());
    }
    let mut clients = Vec::new();
    for (name, harness, fixture) in [
        (
            "probe-claude",
            "claude",
            include_str!("../../tests/fixtures/claude/session.jsonl").to_owned(),
        ),
        (
            "probe-codex",
            "codex",
            include_str!("../../tests/fixtures/codex/session.jsonl")
                .replace("\"cwd\":", "\"model_provider\":\"openai\",\"cwd\":"),
        ),
    ] {
        let root = directory.path().join(name);
        fs::create_dir(&root)?;
        fs::write(root.join("fixture.jsonl"), fixture)?;
        let engine = Engine::open(&directory.path().join(format!("{name}.db")))?;
        {
            let store = engine.lock()?;
            let mut config = store.config()?;
            config.sources = vec![Source {
                id: name.into(),
                harness: harness.into(),
                root: root.to_string_lossy().into(),
                enabled: true,
            }];
            config.server_url = args.url.clone();
            config.server_token = token.clone();
            config.device_name = name.into();
            store.set_meta("config", &config)?;
        }
        engine.scan(false)?;
        clients.push(engine);
    }
    // Failed authentication must leave the upload queue untouched.
    let pending = clients[1].lock()?.pending_count()?;
    {
        let store = clients[1].lock()?;
        let mut config = store.config()?;
        config.server_token = "incorrect-probe-token-with-no-access".into();
        store.set_meta("config", &config)?;
    }
    ensure!(clients[1].sync().is_err(), "Incorrect token was accepted");
    ensure!(clients[1].lock()?.pending_count()? == pending);
    {
        let store = clients[1].lock()?;
        let mut config = store.config()?;
        config.server_token = token.clone();
        store.set_meta("config", &config)?;
    }
    clients[0].sync()?;
    clients[1].sync()?;
    clients[0].sync()?;
    for engine in &clients {
        let store = engine.lock()?;
        ensure!(store.pending_count()? == 0);
        ensure!(store.records("all")?.len() == 3);
        ensure!(store
            .records("all")?
            .iter()
            .filter(|r| r.harness == "codex")
            .all(|r| r.provider.as_deref() == Some("openai")));
        drop(store);
        let summary = engine.execute("usage_query", json!({"kind":"summary","scope":"all"}))?;
        ensure!(summary["realTotalTokens"] == 410, "Initial total differs");
        engine.sync()?;
        ensure!(engine.lock()?.records("all")?.len() == 3);
    }
    // Corrections must replace an earlier revision rather than count twice.
    {
        let store = clients[0].lock()?;
        let mut record = store.local_records()?.remove(0);
        record.tokens.input += 7;
        store.upsert(record, "synthetic-probe")?;
    }
    clients[0].sync()?;
    clients[1].sync()?;
    for engine in &clients {
        let summary = engine.execute("usage_query", json!({"kind":"summary","scope":"all"}))?;
        ensure!(summary["realTotalTokens"] == 417, "Revision total differs");
        ensure!(engine.lock()?.records("all")?.len() == 3);
        ensure!(engine.lock()?.pending_count()? == 0);
    }
    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut cursor = 0;
    let mut pages = 0;
    loop {
        let response: ChangesResponse = http
            .get(format!("{}/v1/changes", args.url.trim_end_matches('/')))
            .bearer_auth(&token)
            .query(&[("after", cursor), ("limit", 1)])
            .send()?
            .error_for_status()?
            .json()?;
        ensure!(response.changes.len() == 1, "Unexpected test change count");
        ensure!(response.next_cursor > cursor);
        cursor = response.next_cursor;
        pages += 1;
        ensure!(pages <= 4, "Probe requires an isolated test database");
        if !response.has_more {
            break;
        }
    }
    ensure!(pages == 4, "Duplicate uploads appended extra changes");
    let device_ids = clients
        .iter()
        .map(|e| e.lock().and_then(|s| s.device_id()))
        .collect::<Result<Vec<_>>>()?;
    drop(clients);
    for name in ["probe-claude", "probe-codex"] {
        let engine = Engine::open(&directory.path().join(format!("{name}.db")))?;
        engine.sync()?;
        let summary = engine.execute("usage_query", json!({"kind":"summary","scope":"all"}))?;
        ensure!(
            summary["realTotalTokens"] == 417,
            "Client restart lost state"
        );
        ensure!(engine.lock()?.pending_count()? == 0);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "url":args.url,
            "devices":2,
            "records":3,
            "initialTokens":410,
            "revisedTokens":417,
            "pending":0,
            "changePages":pages,
            "checks":["invalid_token_preserves_outbox","bidirectional_sync","repeat_is_idempotent","provider_preserved","revision_replaces_previous","cursor_pagination","client_restart_preserves_state"],
            "deviceIds":device_ids
        }))?
    );
    Ok(())
}
