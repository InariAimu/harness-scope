use harness_scope_core::{
    harness::{
        claude::Claude, codex::Codex, files, Descriptor, HarnessAdapter, ParseResult, Registry,
        TimelineIndex,
    },
    model::{Session, Source},
    Engine,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

struct Spy {
    reads: Arc<AtomicUsize>,
    parses: Arc<AtomicUsize>,
}
impl HarnessAdapter for Spy {
    fn descriptor(&self) -> Descriptor {
        Claude.descriptor()
    }
    fn parse(&self, p: &Path, v: &[Value], i: &TimelineIndex) -> anyhow::Result<ParseResult> {
        self.parses.fetch_add(1, Ordering::SeqCst);
        Claude.parse(p, v, i)
    }
    fn read_usage_source(
        &self,
        p: &Path,
    ) -> anyhow::Result<(Vec<Value>, String, Vec<String>, bool)> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Claude.read_usage_source(p)
    }
    fn session(&self, _: &Path, _: &[Value]) -> Option<Session> {
        None
    }
    fn messages(&self, _: &[Value]) -> Vec<Value> {
        vec![]
    }
}
fn configure(e: &Engine, root: &Path, harness: &str) {
    let s = e.lock().unwrap();
    let mut c = s.config().unwrap();
    c.sources = vec![Source {
        id: "test".into(),
        harness: harness.into(),
        root: root.to_string_lossy().into(),
        enabled: true,
    }];
    s.set_meta("config", &c).unwrap();
}
const CLAUDE: &str = include_str!("../../../tests/fixtures/claude/session.jsonl");
const CODEX: &str = include_str!("../../../tests/fixtures/codex/session.jsonl");

#[test]
fn restart_skips_decoding_and_rewrites_force_and_parser_changes_invalidate() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    fs::create_dir(&root).unwrap();
    let path = root.join("s.jsonl");
    fs::write(&path, CLAUDE).unwrap();
    let db = tmp.path().join("scope.db");
    let mut e = Engine::open(&db).unwrap();
    configure(&e, &root, "claude");
    let reads = Arc::new(AtomicUsize::new(0));
    let parses = Arc::new(AtomicUsize::new(0));
    let registry = Registry::new(vec![Arc::new(Spy {
        reads: reads.clone(),
        parses: parses.clone(),
    })]);
    e.registry = registry.clone();
    assert_eq!(e.scan(false).unwrap().imported, 1);
    drop(e);
    let mut e = Engine::open(&db).unwrap();
    e.registry = registry;
    assert_eq!(e.scan(false).unwrap().skipped, 1);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(
        &path,
        CLAUDE.replace("\"output_tokens\":30", "\"output_tokens\":40"),
    )
    .unwrap();
    // Deliberately preserve size and mtime: the edge digest must catch this rewrite.
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(e.scan(false).unwrap().updated, 1);
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(e.scan(true).unwrap().updated, 0);
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    e.lock()
        .unwrap()
        .conn
        .execute("UPDATE file_state SET parser_version='old'", [])
        .unwrap();
    e.scan(false).unwrap();
    assert_eq!(parses.load(Ordering::SeqCst), 4);
    fs::write(&path, format!("{CLAUDE}{{\"type\":")).unwrap();
    assert_eq!(e.scan(false).unwrap().deferred_files, 1);
    assert_eq!(e.scan(false).unwrap().deferred_files, 1);
    assert_eq!(reads.load(Ordering::SeqCst), 6);
    fs::write(&path, "").unwrap();
    assert_eq!(e.scan(false).unwrap().updated, 1);
    assert!(e.lock().unwrap().local_records().unwrap()[0].deleted);
}

#[test]
fn usage_projection_matches_full_reader_including_timing_and_compression() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, adapter, fixture) in [
        ("codex", &Codex as &dyn HarnessAdapter, CODEX),
        ("claude", &Claude as &dyn HarnessAdapter, CLAUDE),
    ] {
        for compressed in [false, true] {
            let path = tmp.path().join(format!(
                "{name}.jsonl{}",
                if compressed { ".zst" } else { "" }
            ));
            let body = format!(
                "{fixture}{}\n",
                json!({"type":"response_item","timestamp":"2026-10-16T03:00:00Z","payload":{"type":"reasoning","encrypted_content":"large ignored body".repeat(2000)}})
            );
            let bytes = if compressed {
                zstd::stream::encode_all(body.as_bytes(), 1).unwrap()
            } else {
                body.into_bytes()
            };
            fs::write(&path, bytes).unwrap();
            let (full, hash, _, _) = adapter.read_source(&path).unwrap();
            let (projected, p_hash, errors, complete) = adapter.read_usage_source(&path).unwrap();
            assert_eq!(hash, p_hash);
            assert!(complete && errors.is_empty());
            let a = adapter.parse(&path, &full, &TimelineIndex::new()).unwrap();
            let b = adapter
                .parse(&path, &projected, &TimelineIndex::new())
                .unwrap();
            assert_eq!(format!("{:?}", a.usage), format!("{:?}", b.usage));
            assert_eq!(a.complete, b.complete);
            assert!(
                serde_json::to_vec(&projected).unwrap().len()
                    < serde_json::to_vec(&full).unwrap().len() / 2
            );
            fs::write(&path, format!("{fixture}{{\"unused\": [1,]}}\n")).unwrap();
            if !compressed {
                assert!(!adapter.read_usage_source(&path).unwrap().3);
            }
        }
    }
}

#[test]
fn discarded_bodies_still_validate_unicode_numbers_and_json_syntax() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.jsonl");
    for invalid in [
        r#"{"unused":"\uD800"}"#,
        r#"{"unused":[{"x":1e999}]}"#,
        r#"{"unused":[1,]}"#,
        r#"{"unused":{"\uD800":0}}"#,
    ] {
        fs::write(&path, format!("{CODEX}{invalid}\n")).unwrap();
        let full = Codex.read_source(&path).unwrap();
        let projected = Codex.read_usage_source(&path).unwrap();
        assert!(!full.3 && !projected.3, "{invalid}");
        assert_eq!(full.1, projected.1);
    }
}

#[test]
fn parent_arrival_and_changes_invalidate_only_dependent_children() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    fs::create_dir(&root).unwrap();
    let parent = vec![
        json!({"type":"session_meta","timestamp":"2026-10-16T02:00:00Z","payload":{"id":"parent"}}),
        json!({"type":"event_msg","timestamp":"2026-10-16T02:00:01Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":10}}}}),
    ];
    let child = vec![
        json!({"type":"session_meta","timestamp":"2026-10-16T02:01:00Z","payload":{"id":"child","source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent"}}}}}),
        json!({"type":"event_msg","timestamp":"2026-10-16T02:01:01Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":150,"output_tokens":15}}}}),
    ];
    let encode = |v: &[Value]| {
        v.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    };
    let cp = root.join("a-child.jsonl");
    let pp = root.join("z-parent.jsonl");
    fs::write(&cp, encode(&child)).unwrap();
    fs::write(root.join("unrelated.jsonl"), CODEX).unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    configure(&e, &root, "codex");
    assert_eq!(e.scan(false).unwrap().deferred_files, 1);
    fs::write(&pp, encode(&parent)).unwrap();
    assert_eq!(e.scan(false).unwrap().imported, 2);
    let child_record = || {
        e.lock()
            .unwrap()
            .record("codex:child:token:1")
            .unwrap()
            .unwrap()
    };
    assert_eq!(child_record().tokens.input, 50);
    let unrelated_fp: String = e
        .lock()
        .unwrap()
        .conn
        .query_row(
            "SELECT fingerprint FROM file_state WHERE path=?",
            [root.join("unrelated.jsonl").to_string_lossy().to_string()],
            |r| r.get(0),
        )
        .unwrap();
    fs::write(
        &pp,
        encode(&parent).replace("\"input_tokens\":100", "\"input_tokens\":120"),
    )
    .unwrap();
    assert_eq!(e.scan(false).unwrap().updated, 2);
    assert_eq!(child_record().tokens.input, 30);
    let new_fp: String = e
        .lock()
        .unwrap()
        .conn
        .query_row(
            "SELECT fingerprint FROM file_state WHERE path=?",
            [root.join("unrelated.jsonl").to_string_lossy().to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(unrelated_fp, new_fp);
    fs::write(&pp, format!("{}{{", encode(&parent))).unwrap();
    assert_eq!(e.scan(false).unwrap().deferred_files, 2);
    assert_eq!(child_record().tokens.input, 30);
    fs::write(&pp, encode(&parent)).unwrap();
    e.scan(false).unwrap();
    assert_eq!(child_record().tokens.input, 50);
    fs::remove_file(&pp).unwrap();
    assert_eq!(e.scan(false).unwrap().deferred_files, 0); // retained baseline
    assert_eq!(files::read(&cp).unwrap().0.len(), 2);
}

#[test]
fn upgrade_retains_legacy_baselines_after_parent_transcript_deletion() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    fs::create_dir(&root).unwrap();
    let parent: Vec<Value> = CODEX
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let child = json!({"type":"session_meta","timestamp":"2026-10-16T03:00:00Z","payload":{"id":"child","parent_thread_id":"codex-fixture"}});
    fs::write(root.join("child.jsonl"), format!("{child}\n{}\n",json!({"type":"event_msg","timestamp":"2026-10-16T03:00:01Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":160,"output_tokens":45}}}}))).unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    configure(&e, &root, "codex");
    e.lock()
        .unwrap()
        .set_meta(
            "usage_timelines",
            &TimelineIndex::from([("codex:codex-fixture".into(), Codex.context_events(&parent))]),
        )
        .unwrap();
    assert_eq!(e.scan(false).unwrap().imported, 1);
    let record = e
        .lock()
        .unwrap()
        .record("codex:child:token:1")
        .unwrap()
        .unwrap();
    assert_eq!(record.tokens.total(), 15);
    // Subsequent scans no longer parse or rewrite the large legacy metadata blob.
    e.lock()
        .unwrap()
        .conn
        .execute(
            "UPDATE meta SET value='invalid legacy JSON' WHERE key='usage_timelines'",
            [],
        )
        .unwrap();
    assert_eq!(e.scan(false).unwrap().skipped, 1);
}
