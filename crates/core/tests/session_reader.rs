use harness_scope_core::{
    harness::{Descriptor, HarnessAdapter, ParseResult, Registry, TimelineIndex},
    model::{Session, Source},
    Engine,
};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
struct Spy {
    metadata: Arc<AtomicUsize>,
    body: Arc<AtomicUsize>,
}
impl HarnessAdapter for Spy {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "spy",
            name: "Spy",
            parser_version: "test",
            default_roots: vec![],
            usage: false,
            transcript: true,
            delete: false,
        }
    }
    fn parse(&self, _: &Path, _: &[Value], _: &TimelineIndex) -> anyhow::Result<ParseResult> {
        Ok(ParseResult::default())
    }
    fn session(&self, _: &Path, _: &[Value]) -> Option<Session> {
        None
    }
    fn messages(&self, _: &[Value]) -> Vec<Value> {
        vec![]
    }
    fn read_metadata(&self, path: &Path) -> anyhow::Result<Vec<Session>> {
        self.metadata.fetch_add(1, Ordering::SeqCst);
        let title = std::fs::read_to_string(path)?;
        anyhow::ensure!(!title.contains("invalid"), "Synthetic temporary failure");
        Ok(vec![Session {
            provider_id: "spy".into(),
            session_id: "fixture".into(),
            title: Some(title),
            summary: None,
            project_dir: None,
            created_at: None,
            last_active_at: None,
            source_path: Some(path.to_string_lossy().into()),
            resume_command: None,
            can_delete: false,
        }])
    }
    fn read_messages(&self, _: &Path, session: Option<&str>) -> anyhow::Result<Vec<Value>> {
        self.body.fetch_add(1, Ordering::SeqCst);
        Ok((0..351).map(|i|json!({"id":i.to_string(),"role":"assistant","content":"duplicate legacy text","blocks":[{"type":"tool_result","callId":session,"status":"success","preview":if i==0 {"全🙂文".repeat(25000)} else {"small".into()},"totalLen":75000,"lineCount":1,"truncated":false}]})).collect())
    }
}
fn setup(
    dir: &Path,
) -> (
    Engine,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    std::path::PathBuf,
) {
    let path = dir.join("session.jsonl");
    std::fs::write(&path, "original").unwrap();
    let metadata = Arc::new(AtomicUsize::new(0));
    let body = Arc::new(AtomicUsize::new(0));
    let mut engine = Engine::open(&dir.join("scope.db")).unwrap();
    engine.registry = Registry::new(vec![Arc::new(Spy {
        metadata: metadata.clone(),
        body: body.clone(),
    })]);
    let s = engine.lock().unwrap();
    let mut config = s.config().unwrap();
    config.sources = vec![Source {
        id: "spy-source".into(),
        harness: "spy".into(),
        root: dir.to_string_lossy().into(),
        enabled: true,
    }];
    s.set_meta("config", &config).unwrap();
    drop(s);
    (engine, metadata, body, path)
}
#[test]
fn metadata_cache_invalidates_and_prunes_without_reading_body() {
    let tmp = tempfile::tempdir().unwrap();
    let (e, meta, body, path) = setup(tmp.path());
    assert_eq!(e.sessions().unwrap()[0].title.as_deref(), Some("original"));
    e.sessions().unwrap();
    assert_eq!(meta.load(Ordering::SeqCst), 1);
    std::fs::write(&path, "changed contents").unwrap();
    assert_eq!(
        e.sessions().unwrap()[0].title.as_deref(),
        Some("changed contents")
    );
    assert_eq!(meta.load(Ordering::SeqCst), 2);
    std::fs::remove_file(&path).unwrap();
    assert!(e.sessions().unwrap().is_empty());
    std::fs::write(&path, "returned").unwrap();
    assert_eq!(e.sessions().unwrap().len(), 1);
    assert_eq!(meta.load(Ordering::SeqCst), 3);
    assert_eq!(body.load(Ordering::SeqCst), 0);
}
#[test]
fn failed_metadata_reads_are_not_cached() {
    let tmp = tempfile::tempdir().unwrap();
    let (e, meta, _, path) = setup(tmp.path());
    std::fs::write(path, "invalid").unwrap();
    assert!(e.sessions().unwrap().is_empty());
    assert!(e.sessions().unwrap().is_empty());
    assert_eq!(meta.load(Ordering::SeqCst), 2);
}
#[test]
fn transcript_pages_cache_scope_and_full_unicode_content_preserve_data() {
    let tmp = tempfile::tempdir().unwrap();
    let (e, _, body, path) = setup(tmp.path());
    let args = json!({"providerId":"spy","sourcePath":path,"sessionId":"one"});
    let before = e.lock().unwrap().pending().unwrap();
    let first = e.execute("session_transcript", args.clone()).unwrap();
    assert_eq!(first["header"]["total"], 351);
    assert_eq!(first["header"]["cached"], false);
    assert!(first["messages"].as_array().unwrap().len() <= 150);
    assert!(first["messages"][0].get("content").is_none());
    let reference = first["messages"][0]["blocks"][0]["full"].clone();
    assert_eq!(reference["sessionId"], "one");
    let mut all = first["messages"].as_array().unwrap().clone();
    let mut offset = first["nextOffset"].clone();
    while !offset.is_null() {
        let mut next = args.clone();
        next["offset"] = offset;
        next["fingerprint"] = first["fingerprint"].clone();
        let page = e.execute("session_transcript", next).unwrap();
        assert_eq!(page["header"]["cached"], true);
        all.extend(page["messages"].as_array().unwrap().clone());
        offset = page["nextOffset"].clone();
    }
    assert_eq!(all.len(), 351);
    assert_eq!(body.load(Ordering::SeqCst), 1);
    let mut block_args =
        json!({"providerId":"spy","sourcePath":path,"reference":reference,"limit":128});
    let mut full = String::new();
    let mut offset = 0;
    loop {
        block_args["offset"] = json!(offset);
        let page = e.execute("session_block", block_args.clone()).unwrap();
        full.push_str(page["text"].as_str().unwrap());
        if page["truncated"] == false {
            break;
        }
        offset = page["nextOffset"].as_u64().unwrap();
    }
    assert_eq!(full, "全🙂文".repeat(25000));
    let restored = e.execute("session_messages", args.clone()).unwrap();
    assert_eq!(restored[0]["blocks"][0]["preview"], full);
    assert!(restored[0]["blocks"][0].get("full").is_none());
    let mut other = args.clone();
    other["sessionId"] = json!("two");
    e.execute("session_transcript", other).unwrap();
    assert_eq!(body.load(Ordering::SeqCst), 2);
    let mut bad = block_args.clone();
    bad["reference"]["fingerprint"] = json!("forged");
    assert!(e.execute("session_block", bad).is_err());
    let mut outside = args;
    outside["sourcePath"] = json!(tmp.path().join("../outside.jsonl"));
    assert!(e.execute("session_transcript", outside).is_err());
    assert_eq!(e.lock().unwrap().pending().unwrap(), before);
}
#[test]
fn transcript_changes_invalidate_and_lru_does_not_grow_without_bound() {
    let tmp = tempfile::tempdir().unwrap();
    let (e, _, body, path) = setup(tmp.path());
    let mut args = json!({"providerId":"spy","sourcePath":path,"sessionId":"0"});
    e.execute("session_transcript", args.clone()).unwrap();
    std::fs::write(&path, "updated file with another size").unwrap();
    let refreshed = e.execute("session_transcript", args.clone()).unwrap();
    assert_eq!(refreshed["header"]["cached"], false);
    for id in 1..9 {
        args["sessionId"] = json!(id.to_string());
        e.execute("session_transcript", args.clone()).unwrap();
    }
    args["sessionId"] = json!("0");
    let evicted = e.execute("session_transcript", args).unwrap();
    assert_eq!(evicted["header"]["cached"], false);
    assert_eq!(body.load(Ordering::SeqCst), 11);
}
#[test]
fn compressed_list_reads_only_bounded_header_and_never_decodes_corrupt_tail() {
    use harness_scope_core::harness::codex::Codex;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("session.jsonl.zst");
    let mut text=String::from("{\"type\":\"session_meta\",\"timestamp\":\"2026-10-19T00:00:00Z\",\"payload\":{\"id\":\"native\",\"cwd\":\"/synthetic\"}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"text\":\"Synthetic title\"}]}}\n");
    for _ in 0..100 {
        text.push_str(&format!("{{\"padding\":\"{}\"}}\n", "x".repeat(10000)));
    }
    let mut bytes = zstd::encode_all(text.as_bytes(), 1).unwrap();
    bytes.extend_from_slice(b"corrupt trailing frame");
    std::fs::write(&path, bytes).unwrap();
    let rows = Codex.read_metadata(&path).unwrap();
    assert_eq!(rows[0].session_id, "native");
    assert_eq!(rows[0].title.as_deref(), Some("Synthetic title"));
    assert!(Codex.read_source(&path).is_err());
}

#[test]
fn unicode_cut_at_metadata_budget_does_not_drop_an_existing_session() {
    use harness_scope_core::harness::codex::Codex;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("session.jsonl");
    let text=format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"unicode-budget\"}}}}\n{{\"type\":\"padding\",\"text\":\"{}\"}}\n", "中🙂".repeat(100000));
    std::fs::write(&path, text).unwrap();
    assert_eq!(
        Codex.read_metadata(&path).unwrap()[0].session_id,
        "unicode-budget"
    );
}
#[test]
fn continued_batches_and_blocks_pin_the_original_snapshot_during_append() {
    let tmp = tempfile::tempdir().unwrap();
    let (e, _, body, path) = setup(tmp.path());
    let args = json!({"providerId":"spy","sourcePath":path,"sessionId":"live"});
    let first = e.execute("session_transcript", args.clone()).unwrap();
    let reference = first["messages"][0]["blocks"][0]["full"].clone();
    std::fs::write(&path, "new longer contents during live output").unwrap();
    let fresh = e.execute("session_transcript", args.clone()).unwrap();
    assert_ne!(fresh["fingerprint"], first["fingerprint"]);
    let mut next = args;
    next["offset"] = first["nextOffset"].clone();
    next["fingerprint"] = first["fingerprint"].clone();
    assert_eq!(
        e.execute("session_transcript", next).unwrap()["fingerprint"],
        first["fingerprint"]
    );
    let block = e
        .execute(
            "session_block",
            json!({"providerId":"spy","sourcePath":path,"reference":reference,"limit":12}),
        )
        .unwrap();
    assert_eq!(block["text"], "全🙂文".repeat(4));
    assert_eq!(body.load(Ordering::SeqCst), 2);
}
