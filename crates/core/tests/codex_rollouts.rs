//! Regression contracts for CC Switch rollout accounting and our sync migration.
use harness_scope_core::{
    harness::{codex::Codex, HarnessAdapter, TimelineIndex},
    model::{Cost, ParsedUsage, PushRequest, Source, UsageRecord},
    store::Store,
    sync, Engine,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

const THREAD: &str = "00000000-0000-4000-8000-000000000001";
const PAGE: &str = "00000000-0000-4000-8000-000000000002";
const CHILD: &str = "00000000-0000-4000-8000-000000000003";
fn first_name() -> String {
    format!("rollout-2026-10-22T01-00-00-{THREAD}.jsonl")
}
fn next_name() -> String {
    format!("rollout-2026-10-22T02-00-00-{THREAD}_{PAGE}.jsonl")
}
fn meta(id: &str) -> Value {
    json!({"type":"session_meta","timestamp":"2026-10-22T00:00:00Z","payload":{"id":id,"model":"gpt-5.4","model_provider":"openai"}})
}
fn usage(second: u32, total: Option<u64>, last: Option<u64>) -> Value {
    json!({"type":"event_msg","timestamp":format!("2026-10-22T01:00:{second:02}Z"),"payload":{"type":"token_count","info":{"total_token_usage":total.map(|n|json!({"input_tokens":n,"output_tokens":0})),"last_token_usage":last.map(|n|json!({"input_tokens":n,"output_tokens":0}))}}})
}
fn continuation() -> Value {
    let mut v = meta(THREAD);
    v["payload"]["history_mode"] = json!("paginated");
    v["payload"]["history_base"] = json!({"thread_id":THREAD});
    v
}
fn write(path: &Path, rows: &[Value]) {
    fs::write(
        path,
        rows.iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .unwrap();
}
fn setup() -> (tempfile::TempDir, Engine, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("logs");
    fs::create_dir(&root).unwrap();
    let e = Engine::open(&dir.path().join("scope.db")).unwrap();
    {
        let s = e.lock().unwrap();
        let mut c = s.config().unwrap();
        c.sources = vec![Source {
            id: "fixture".into(),
            harness: "codex".into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        }];
        s.set_meta("config", &c).unwrap();
    }
    (dir, e, root)
}
fn scan(e: &Engine, force: bool) -> harness_scope_core::model::ScanReport {
    let r = e.scan_harness("codex", force).unwrap();
    assert!(r.errors.is_empty(), "{r:?}");
    assert_eq!(r.deferred_files, 0, "{r:?}");
    r
}
fn index(segments: &[(PathBuf, Vec<Value>)]) -> TimelineIndex {
    let mut index = TimelineIndex::new();
    for (p, rows) in segments {
        index
            .entry(format!("codex:{THREAD}"))
            .or_default()
            .push(json!({"_codexPath":p,"events":Codex.context_events(rows),"available":true}));
    }
    index
}
fn parsed(path: &Path, rows: &[Value], index: &TimelineIndex) -> Vec<ParsedUsage> {
    let r = Codex.parse(path, rows, index).unwrap();
    assert!(r.complete, "{:?}", r.diagnostics);
    r.usage
}
#[test]
fn separate_rollouts_union_and_compressed_moves_survive_restart_and_truncation() {
    let (dir, e, root) = setup();
    let a = root.join(first_name());
    let b = root.join(next_name());
    let first = vec![
        meta(THREAD),
        usage(1, Some(10), Some(10)),
        usage(2, Some(30), Some(20)),
    ];
    let next = vec![continuation(), usage(3, Some(60), Some(30))];
    write(&a, &first);
    write(&b, &next);
    assert_eq!(scan(&e, false).imported, 3);
    let before = e.lock().unwrap().records("local").unwrap();
    assert_eq!(before.iter().map(|r| r.tokens.total()).sum::<u64>(), 60);
    assert!(before
        .iter()
        .all(|r| r.session_id.as_deref() == Some(THREAD)));
    e.lock()
        .unwrap()
        .conn
        .execute("DELETE FROM outbox", [])
        .unwrap();
    assert_eq!(scan(&e, true).updated, 0);
    drop(e);
    let e = Engine::open(&dir.path().join("scope.db")).unwrap();
    assert_eq!(scan(&e, false).imported, 0);
    let compressed = b.with_extension("jsonl.zst");
    fs::write(
        &compressed,
        zstd::stream::encode_all(fs::read(&b).unwrap().as_slice(), 1).unwrap(),
    )
    .unwrap();
    assert_eq!(scan(&e, true).updated, 0);
    fs::remove_file(&b).unwrap();
    assert_eq!(scan(&e, false).updated, 0);
    let archive = root.join("archive");
    fs::create_dir(&archive).unwrap();
    write(&archive.join(first_name()), &first);
    scan(&e, false);
    write(&a, &first[..2]);
    scan(&e, false);
    assert_eq!(e.lock().unwrap().records("local").unwrap(), before);
    assert_eq!(e.lock().unwrap().pending_count().unwrap(), 0);
}
#[test]
fn compressed_only_sources_are_not_scanned_even_when_forced() {
    let (_dir, e, root) = setup();
    let path = root.join(first_name());
    write(&path, &[meta(THREAD), usage(1, Some(10), Some(10))]);
    fs::write(
        path.with_extension("jsonl.zst"),
        zstd::stream::encode_all(fs::read(&path).unwrap().as_slice(), 1).unwrap(),
    )
    .unwrap();
    fs::remove_file(&path).unwrap();
    let compressed = path.with_extension("jsonl.zst");
    let sessions = e.sessions().unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(!sessions[0].can_delete);
    assert!(e
        .execute(
            "delete_session",
            json!({
                "providerId":"codex", "sourcePath":compressed, "sessionId":THREAD
            })
        )
        .is_err());
    assert!(compressed.exists());
    for force in [false, true] {
        let report = scan(&e, force);
        assert_eq!(report.files_scanned, 0);
        assert_eq!(report.imported, 0);
        assert!(e.lock().unwrap().records("local").unwrap().is_empty());
    }
}

/// Opt-in timing on an SQLite backup placed under output/, never the live DB.
#[test]
#[ignore]
fn profile_codex_scan_on_database_copy() {
    let copy = PathBuf::from(
        std::env::var_os("HARNESS_SCOPE_PROFILE_COPY").expect("backup path required"),
    );
    let output = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../output")
        .canonicalize()
        .unwrap();
    assert!(copy.canonicalize().unwrap().starts_with(output));
    let engine = Engine::open(&copy).unwrap();
    for pass in 1..=2 {
        let start = std::time::Instant::now();
        let r = engine.scan_harness("codex", false).unwrap();
        println!(
            "pass={pass} ms={} files={} imported={} updated={} deferred={} errors={}",
            start.elapsed().as_millis(),
            r.files_scanned,
            r.imported,
            r.updated,
            r.deferred_files,
            r.errors.len()
        );
    }
}
#[test]
fn cached_compressed_parent_still_supplies_continuation_baseline() {
    let (_dir, e, root) = setup();
    let first = root.join(first_name());
    write(&first, &[meta(THREAD), usage(1, Some(100), Some(100))]);
    assert_eq!(scan(&e, false).imported, 1);
    fs::write(
        first.with_extension("jsonl.zst"),
        zstd::stream::encode_all(fs::read(&first).unwrap().as_slice(), 1).unwrap(),
    )
    .unwrap();
    fs::remove_file(&first).unwrap();
    write(
        &root.join(next_name()),
        &[continuation(), usage(2, Some(120), None)],
    );
    let report = scan(&e, false);
    assert_eq!(report.files_scanned, 1);
    assert_eq!(report.imported, 1);
    assert_eq!(
        e.lock()
            .unwrap()
            .records("local")
            .unwrap()
            .iter()
            .map(|r| r.tokens.input)
            .sum::<u64>(),
        120
    );
    assert_eq!(scan(&e, true).updated, 0);
}
#[test]
fn lane_replays_and_zero_events_do_not_shift_ordinals_but_last_only_requests_count() {
    let mut rows = vec![meta(THREAD), usage(0, Some(0), Some(0))];
    for (at, total, last, lane) in [
        (1, 100, 10, "a"),
        (2, 100, 10, "b"),
        (3, 20, 5, "b"),
        (4, 100, 10, "a"),
        (5, 105, 5, "a"),
    ] {
        let mut u = usage(at, Some(total), Some(last));
        u["payload"]["rate_limits"] = json!({"limit_id":lane});
        rows.push(u);
    }
    rows.extend([usage(6, None, Some(5)), usage(7, None, Some(5))]);
    let records = parsed(Path::new(&first_name()), &rows, &TimelineIndex::new());
    assert_eq!(records.len(), 5);
    assert_eq!(
        records.iter().map(|r| r.tokens.input).collect::<Vec<_>>(),
        vec![10, 5, 5, 5, 5]
    );
    for (n, r) in records.iter().enumerate() {
        assert_eq!(r.id, format!("codex:v6:{THREAD}:token:{}", n + 1));
    }
}
#[test]
fn continuation_total_only_uses_previous_high_water_and_missing_baselines_defer() {
    let a = vec![
        meta(THREAD),
        usage(1, Some(100), None),
        usage(2, Some(80), None),
    ];
    let mut b = vec![continuation(), usage(3, Some(120), None)];
    let context = index(&[(first_name().into(), a)]);
    let path = PathBuf::from(next_name());
    assert_eq!(parsed(&path, &b, &context)[0].tokens.input, 20);
    assert!(
        !Codex
            .parse(&path, &b, &TimelineIndex::new())
            .unwrap()
            .complete
    );
    b[1] = usage(3, Some(15), None);
    assert_eq!(parsed(&path, &b, &context)[0].tokens.input, 15);
    b[0]["payload"]["forked_from_id"] = json!("missing-parent");
    b[1] = usage(3, Some(120), Some(20));
    assert_eq!(parsed(&path, &b, &TimelineIndex::new())[0].tokens.input, 20);
}
#[test]
fn parent_history_merges_pages_rejects_conflicting_copies_and_checks_precise_cutoff() {
    let first = vec![meta(THREAD), usage(1, Some(100), Some(100))];
    let mut next = vec![
        continuation(),
        usage(2, Some(150), Some(50)),
        json!({"type":"event_msg","timestamp":"2026-10-22T01:00:02.100000001Z","payload":{"type":"task_complete"}}),
    ];
    let mut child = vec![
        meta(CHILD),
        first[1].clone(),
        next[1].clone(),
        usage(3, Some(170), None),
    ];
    child[0]["timestamp"] = json!("2026-10-22T01:00:02.100000001Z");
    child[0]["payload"]["forked_from_id"] = json!(THREAD);
    let path = PathBuf::from(format!("rollout-{CHILD}.jsonl"));
    let context = index(&[
        (next_name().into(), next.clone()),
        (first_name().into(), first.clone()),
    ]);
    let out = parsed(&path, &child, &context);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].tokens.input, 20);
    assert!(out[0].id.ends_with("token:3"));
    child[0]["timestamp"] = json!("2026-10-22T01:00:02.100000002Z");
    assert!(!Codex.parse(&path, &child, &context).unwrap().complete);
    child[0]["timestamp"] = json!("2026-10-22T01:00:02Z");
    next[1] = usage(2, Some(151), Some(51));
    let mut conflict = context;
    let other = index(&[(PathBuf::from("copy").join(next_name()), next)]);
    conflict
        .get_mut(&format!("codex:{THREAD}"))
        .unwrap()
        .extend(other[&format!("codex:{THREAD}")].clone());
    assert!(!Codex.parse(&path, &child, &conflict).unwrap().complete);
}

#[test]
fn explicit_fork_boundary_excludes_later_parent_events_and_remains_idempotent() {
    let (dir, e, root) = setup();
    let parent = root.join(first_name());
    let prefix = vec![
        meta(THREAD),
        usage(1, Some(100), Some(100)),
        usage(2, Some(150), Some(50)),
    ];
    write(&parent, &prefix);
    let bytes = fs::metadata(&parent).unwrap().len();
    let mut parent_rows = prefix.clone();
    parent_rows.push(usage(3, Some(200), Some(50)));
    write(&parent, &parent_rows);
    let mut child_meta = meta(CHILD);
    child_meta["timestamp"] = json!("2026-10-22T01:00:10Z");
    child_meta["payload"]["forked_from_id"] = json!(THREAD);
    child_meta["payload"]["history_mode"] = json!("paginated");
    child_meta["payload"]["history_base"] =
        json!({"thread_id":THREAD,"end_ordinal_exclusive":3,"end_byte_offset":bytes});
    let child = root.join(format!("rollout-2026-10-22T01-00-10-{CHILD}.jsonl"));
    // The new event has the same signature as a parent event after the boundary.
    let child_rows = vec![
        child_meta,
        prefix[1].clone(),
        prefix[2].clone(),
        usage(11, Some(200), Some(50)),
    ];
    write(&child, &child_rows);
    assert_eq!(scan(&e, false).imported, 4);
    let before = e.lock().unwrap().records("local").unwrap();
    let child_records = before
        .iter()
        .filter(|r| r.session_id.as_deref() == Some(CHILD))
        .collect::<Vec<_>>();
    assert_eq!(child_records.len(), 1);
    assert_eq!(child_records[0].tokens.input, 50);
    assert_eq!(child_records[0].native_event_id.as_deref(), Some("token:3"));
    assert_eq!(scan(&e, true).updated, 0);
    drop(e);
    let e = Engine::open(&dir.path().join("scope.db")).unwrap();
    assert_eq!(scan(&e, false).imported, 0);
    assert_eq!(e.lock().unwrap().records("local").unwrap(), before);
}

#[test]
fn explicit_fork_boundary_validates_offsets_and_supports_total_only_child() {
    let (_dir, e, root) = setup();
    let parent = root.join(first_name());
    let rows = vec![
        meta(THREAD),
        usage(1, Some(100), Some(100)),
        usage(2, Some(150), Some(50)),
    ];
    write(&parent, &rows);
    let bytes = fs::metadata(&parent).unwrap().len();
    let mut child_meta = meta(CHILD);
    child_meta["timestamp"] = json!("2026-10-22T01:00:10Z");
    child_meta["payload"]["forked_from_id"] = json!(THREAD);
    child_meta["payload"]["history_base"] =
        json!({"thread_id":THREAD,"end_ordinal_exclusive":3,"end_byte_offset":bytes});
    let child = root.join(format!("rollout-{CHILD}.jsonl"));
    let child_usage = usage(11, Some(170), None);
    write(&child, &[child_meta.clone(), child_usage.clone()]);
    assert_eq!(scan(&e, false).imported, 3);
    let before = e.lock().unwrap().records("local").unwrap();
    assert_eq!(
        before
            .iter()
            .find(|r| r.session_id.as_deref() == Some(CHILD))
            .unwrap()
            .tokens
            .input,
        20
    );
    for bad in [
        json!({"thread_id":THREAD,"end_ordinal_exclusive":2,"end_byte_offset":bytes}),
        json!({"thread_id":THREAD,"end_ordinal_exclusive":3,"end_byte_offset":bytes-1}),
        json!({"thread_id":THREAD,"end_ordinal_exclusive":3,"end_byte_offset":bytes+1}),
        json!({"thread_id":CHILD,"end_ordinal_exclusive":3,"end_byte_offset":bytes}),
        json!({"thread_id":THREAD,"end_ordinal_exclusive":"3","end_byte_offset":bytes}),
    ] {
        let mut bad_meta = child_meta.clone();
        bad_meta["payload"]["history_base"] = bad;
        write(&child, &[bad_meta, child_usage.clone()]);
        let report = e.scan_harness("codex", false).unwrap();
        assert!(report.deferred_files > 0 && !report.errors.is_empty());
        assert_eq!(e.lock().unwrap().records("local").unwrap(), before);
    }
    write(&child, &[child_meta, child_usage]);
    assert_eq!(scan(&e, false).updated, 0);
    assert_eq!(e.lock().unwrap().records("local").unwrap(), before);
}

#[test]
fn explicit_fork_boundary_suppresses_supplemental_usage_in_inherited_replay() {
    let (_dir, e, root) = setup();
    let parent = root.join(first_name());
    let zero = usage(1, Some(100), Some(0));
    write(&parent, &[meta(THREAD), zero.clone()]);
    let bytes = fs::metadata(&parent).unwrap().len();
    let mut child_meta = meta(CHILD);
    child_meta["timestamp"] = json!("2026-10-22T01:00:10Z");
    child_meta["payload"]["forked_from_id"] = json!(THREAD);
    child_meta["payload"]["history_base"] =
        json!({"thread_id":THREAD,"end_ordinal_exclusive":2,"end_byte_offset":bytes});
    let child = root.join(format!("rollout-{CHILD}.jsonl"));
    let supplement = json!({"type":"token_usage_record","timestamp":"2026-10-22T01:00:01Z","payload":{"response_id":"replayed","usage":{"input_tokens":10,"output_tokens":1}}});
    write(
        &child,
        &[child_meta, supplement, zero, usage(11, Some(120), Some(20))],
    );
    assert_eq!(scan(&e, false).imported, 1);
    let records = e.lock().unwrap().records("local").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].tokens.input, 20);
    assert!(!records[0]
        .native_event_id
        .as_deref()
        .unwrap()
        .starts_with("usage:"));
}

#[test]
fn legacy_fork_read_cache_is_refreshed_before_boundary_validation() {
    let (_dir, e, root) = setup();
    let parent = root.join(first_name());
    write(&parent, &[meta(THREAD), usage(1, Some(100), Some(100))]);
    let bytes = fs::metadata(&parent).unwrap().len();
    let mut child_meta = meta(CHILD);
    child_meta["timestamp"] = json!("2026-10-22T01:00:10Z");
    child_meta["payload"]["forked_from_id"] = json!(THREAD);
    child_meta["payload"]["history_base"] =
        json!({"thread_id":THREAD,"end_ordinal_exclusive":2,"end_byte_offset":bytes});
    let child = root.join(format!("rollout-{CHILD}.jsonl"));
    write(&child, &[child_meta, usage(11, Some(120), Some(20))]);
    assert_eq!(scan(&e, false).imported, 2);
    let before = e.lock().unwrap().records("local").unwrap();
    {
        let s = e.lock().unwrap();
        let path = child.to_string_lossy();
        let payload: String = s
            .conn
            .query_row(
                "SELECT payload FROM codex_source_cache WHERE path=?1",
                [path.as_ref()],
                |r| r.get(0),
            )
            .unwrap();
        let mut cache: Value = serde_json::from_str(&payload).unwrap();
        cache.as_object_mut().unwrap().remove("projection_version");
        let meta = cache["context"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|v| v["type"] == "session_meta")
            .unwrap();
        meta["payload"]["history_base"]
            .as_object_mut()
            .unwrap()
            .remove("end_byte_offset");
        meta["payload"]["history_base"]
            .as_object_mut()
            .unwrap()
            .remove("end_ordinal_exclusive");
        s.conn
            .execute(
                "UPDATE codex_source_cache SET payload=?1 WHERE path=?2",
                rusqlite::params![cache.to_string(), path.as_ref()],
            )
            .unwrap();
        s.conn
            .execute("DELETE FROM file_state WHERE path=?1", [path.as_ref()])
            .unwrap();
    }
    let report = scan(&e, false);
    assert_eq!((report.imported, report.updated), (0, 0));
    assert_eq!(e.lock().unwrap().records("local").unwrap(), before);
    let s = e.lock().unwrap();
    let payload: String = s
        .conn
        .query_row(
            "SELECT payload FROM codex_source_cache WHERE path=?1",
            [child.to_string_lossy().as_ref()],
            |r| r.get(0),
        )
        .unwrap();
    let cache: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(cache["projection_version"], 1);
    assert!(cache["context"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["payload"]["history_base"]["end_byte_offset"] == bytes));
}

#[test]
fn conflicting_parent_copies_defer_boundary_without_guessing() {
    let (_dir, e, root) = setup();
    let parent = root.join(first_name());
    let rows = [meta(THREAD), usage(1, Some(100), Some(100))];
    write(&parent, &rows);
    let bytes = fs::metadata(&parent).unwrap().len();
    let copy_dir = root.join("copy");
    fs::create_dir(&copy_dir).unwrap();
    write(
        &copy_dir.join(first_name()),
        &[meta(THREAD), usage(1, Some(101), Some(101))],
    );
    let mut child_meta = meta(CHILD);
    child_meta["timestamp"] = json!("2026-10-22T01:00:10Z");
    child_meta["payload"]["forked_from_id"] = json!(THREAD);
    child_meta["payload"]["history_base"] =
        json!({"thread_id":THREAD,"end_ordinal_exclusive":2,"end_byte_offset":bytes});
    let child = root.join(format!("rollout-{CHILD}.jsonl"));
    write(&child, &[child_meta, usage(11, Some(120), Some(20))]);
    let report = e.scan_harness("codex", false).unwrap();
    assert!(report.deferred_files > 0 && !report.errors.is_empty());
    assert!(e
        .lock()
        .unwrap()
        .records("local")
        .unwrap()
        .iter()
        .all(|r| r.session_id.as_deref() != Some(CHILD)));
}

#[test]
fn compressed_only_parent_cannot_validate_a_new_fork_boundary() {
    let (_dir, e, root) = setup();
    let parent = root.join(first_name());
    write(&parent, &[meta(THREAD), usage(1, Some(100), Some(100))]);
    let bytes = fs::metadata(&parent).unwrap().len();
    assert_eq!(scan(&e, false).imported, 1);
    let before = e.lock().unwrap().records("local").unwrap();
    fs::write(
        parent.with_extension("jsonl.zst"),
        zstd::stream::encode_all(fs::read(&parent).unwrap().as_slice(), 1).unwrap(),
    )
    .unwrap();
    fs::remove_file(&parent).unwrap();
    let mut child_meta = meta(CHILD);
    child_meta["timestamp"] = json!("2026-10-22T01:00:10Z");
    child_meta["payload"]["forked_from_id"] = json!(THREAD);
    child_meta["payload"]["history_base"] =
        json!({"thread_id":THREAD,"end_ordinal_exclusive":2,"end_byte_offset":bytes});
    let child = root.join(format!("rollout-{CHILD}.jsonl"));
    write(&child, &[child_meta, usage(11, Some(120), Some(20))]);
    let report = e.scan_harness("codex", false).unwrap();
    assert!(report.deferred_files > 0 && !report.errors.is_empty());
    assert_eq!(e.lock().unwrap().records("local").unwrap(), before);
}

#[test]
fn total_only_fork_defers_without_a_known_parent_cumulative_baseline() {
    let (_dir, e, root) = setup();
    let parent = root.join(first_name());
    write(&parent, &[meta(THREAD)]);
    let bytes = fs::metadata(&parent).unwrap().len();
    let mut child_meta = meta(CHILD);
    child_meta["timestamp"] = json!("2026-10-22T01:00:10Z");
    child_meta["payload"]["forked_from_id"] = json!(THREAD);
    child_meta["payload"]["history_base"] =
        json!({"thread_id":THREAD,"end_ordinal_exclusive":1,"end_byte_offset":bytes});
    let child = root.join(format!("rollout-{CHILD}.jsonl"));
    write(&child, &[child_meta.clone(), usage(11, Some(120), None)]);
    let r = e.scan_harness("codex", false).unwrap();
    assert!(r.deferred_files > 0 && !r.errors.is_empty());
    assert!(e.lock().unwrap().records("local").unwrap().is_empty());
    write(&child, &[child_meta, usage(11, Some(120), Some(20))]);
    assert_eq!(scan(&e, false).imported, 1);
    assert_eq!(
        e.lock().unwrap().records("local").unwrap()[0].tokens.input,
        20
    );
}

#[test]
fn total_only_fork_uses_full_parent_snapshot_when_only_part_is_replayed() {
    let (_dir, e, root) = setup();
    let parent = root.join(first_name());
    let earlier = usage(1, Some(100), None);
    write(
        &parent,
        &[meta(THREAD), earlier.clone(), usage(2, Some(150), None)],
    );
    let bytes = fs::metadata(&parent).unwrap().len();
    let mut child_meta = meta(CHILD);
    child_meta["timestamp"] = json!("2026-10-22T01:00:10Z");
    child_meta["payload"]["forked_from_id"] = json!(THREAD);
    child_meta["payload"]["history_base"] =
        json!({"thread_id":THREAD,"end_ordinal_exclusive":3,"end_byte_offset":bytes});
    let child = root.join(format!("rollout-{CHILD}.jsonl"));
    write(&child, &[child_meta, earlier, usage(11, Some(170), None)]);
    assert_eq!(scan(&e, false).imported, 3);
    let s = e.lock().unwrap();
    let records = s.records("local").unwrap();
    let child = records
        .iter()
        .find(|r| r.session_id.as_deref() == Some(CHILD))
        .unwrap();
    assert_eq!(child.tokens.input, 20);
}
#[test]
fn compaction_is_separate_from_token_ordinals_and_projection_preserves_physical_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(first_name());
    let record = |id: Value| json!({"type":"token_usage_record","timestamp":"2026-10-22T01:00:01Z","payload":{"response_id":id,"usage":{"input_tokens":12,"output_tokens":2}}});
    let mut counted = usage(2, Some(12), Some(12));
    counted["payload"]["info"]["last_token_usage"]["output_tokens"] = json!(2);
    let rows = [
        meta(THREAD),
        json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"Vendor/Raw-Model"}}}),
        record(json!("matched")),
        counted,
        record(Value::Null),
        usage(3, Some(12), Some(0)),
        usage(4, Some(25), Some(13)),
        record(json!("orphan")),
    ];
    fs::write(
        &path,
        format!(
            "\n{}",
            rows.iter().map(|r| format!("{r}\n")).collect::<String>()
        ),
    )
    .unwrap();
    let full = Codex.read_source(&path).unwrap().0;
    let projected = Codex.read_usage_source(&path).unwrap().0;
    let a = parsed(&path, &full, &TimelineIndex::new());
    let b = parsed(&path, &projected, &TimelineIndex::new());
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    assert_eq!(a.len(), 3);
    assert_eq!(a[1].native_event_id, "usage:line6");
    assert_eq!(a[2].native_event_id, "token:2");
    assert!(a.iter().all(|r| r.model == "Vendor/Raw-Model"));
    assert_eq!(a[1].tokens.total(), 14);
}
#[test]
fn incomplete_or_conflicting_pages_defer_the_whole_session_without_checkpoints() {
    let (_dir, e, root) = setup();
    let a = root.join(first_name());
    let b = root.join(next_name());
    let first = vec![meta(THREAD), usage(1, Some(10), Some(10))];
    write(&a, &first);
    fs::write(&b, format!("{}\n{{", continuation())).unwrap();
    let r = e.scan_harness("codex", false).unwrap();
    assert_eq!(r.imported, 0);
    assert_eq!(r.deferred_files, 2);
    assert!(e.lock().unwrap().local_records().unwrap().is_empty());
    write(&b, &[continuation(), usage(2, Some(20), Some(10))]);
    assert_eq!(scan(&e, false).imported, 2);
    let archive = root.join("copy");
    fs::create_dir(&archive).unwrap();
    write(
        &archive.join(first_name()),
        &[meta(THREAD), usage(1, Some(15), Some(15))],
    );
    let before = e.lock().unwrap().local_records().unwrap();
    assert!(e.scan_harness("codex", false).unwrap().deferred_files > 0);
    assert_eq!(e.lock().unwrap().local_records().unwrap(), before);
}
fn old_record(e: &Engine, p: ParsedUsage, ordinal: u32) -> UsageRecord {
    UsageRecord {
        id: format!("codex:{THREAD}:token:{ordinal}"),
        native_event_id: Some(format!("token:{ordinal}")),
        session_id: Some(p.session_id),
        device_id: e.lock().unwrap().device_id().unwrap(),
        harness: "codex".into(),
        parser_version: "codex-v5-request-settings".into(),
        occurred_at: p.occurred_at,
        model: p.model,
        provider: p.provider,
        tokens: p.tokens,
        cost: Cost::default(),
        timing: None,
        request_metadata: None,
        grain: "event".into(),
        count: 1,
        record_source: "session".into(),
        revision: 1,
        deleted: false,
        status_code: None,
        legacy_date: None,
        legacy_timezone: None,
    }
}
fn legacy_fixture() -> (tempfile::TempDir, Engine, PathBuf, Vec<UsageRecord>) {
    let (dir, e, root) = setup();
    let a = root.join(first_name());
    let b = root.join(next_name());
    let first = vec![
        meta(THREAD),
        usage(1, Some(10), Some(10)),
        usage(2, Some(30), Some(20)),
    ];
    let next = vec![
        continuation(),
        usage(3, Some(60), Some(30)),
        usage(4, Some(60), Some(30)),
    ];
    write(&a, &first);
    write(&b, &next);
    let old_a = parsed(&a, &first, &TimelineIndex::new());
    let old_b = parsed(&b, &next, &TimelineIndex::new());
    // v5 reused ordinal 1 across files, so b overwrote a's first event.
    let mut records = vec![
        old_record(&e, old_b[0].clone(), 1),
        old_record(&e, old_a[1].clone(), 2),
    ];
    let phantom = parsed(
        &a,
        &[meta(THREAD), usage(4, Some(60), Some(30))],
        &TimelineIndex::new(),
    );
    records.push(old_record(&e, phantom[0].clone(), 3));
    let missing = parsed(
        &a,
        &[meta(THREAD), usage(5, Some(7), Some(7))],
        &TimelineIndex::new(),
    );
    records.push(old_record(&e, missing[0].clone(), 4));
    {
        let s = e.lock().unwrap();
        for (r, path) in records
            .iter()
            .zip([b.clone(), a, b, root.join("removed.jsonl")])
        {
            s.upsert(r.clone(), &path.to_string_lossy()).unwrap();
        }
    }
    (dir, e, root, records)
}
#[test]
fn historical_collision_repair_preserves_ids_costs_and_syncs_revisions_without_duplicates() {
    let (dir, e, _root, old) = legacy_fixture();
    let server = Store::open(&dir.path().join("server.db")).unwrap();
    let send = |e: &Engine| {
        let s = e.lock().unwrap();
        let request = PushRequest {
            protocol_version: 4,
            device_id: s.device_id().unwrap(),
            device_name: "test".into(),
            records: s.pending().unwrap(),
        };
        let response = sync::push(&server, request.clone()).unwrap();
        sync::push(&server, request).unwrap();
        s.ack(&response.accepted).unwrap();
    };
    send(&e);
    e.lock()
        .unwrap()
        .conn
        .execute_batch("PRAGMA user_version=4")
        .unwrap();
    drop(e);
    let e = Engine::open(&dir.path().join("scope.db")).unwrap();
    assert_eq!(e.lock().unwrap().schema_version().unwrap(), 6);
    let r = scan(&e, false);
    assert_eq!(r.imported, 1);
    assert_eq!(r.updated, 3);
    {
        let s = e.lock().unwrap();
        for r in &old[..2] {
            let new = s.record(&r.id).unwrap().unwrap();
            assert_eq!(new.cost, r.cost);
            assert_eq!(new.tokens, r.tokens);
            assert_eq!(new.revision, r.revision + 1);
        }
        assert!(s.record(&old[2].id).unwrap().unwrap().deleted);
        assert_eq!(s.record(&old[3].id).unwrap().unwrap(), old[3]);
        assert_eq!(
            s.records("local")
                .unwrap()
                .iter()
                .map(|r| r.tokens.total())
                .sum::<u64>(),
            67
        );
    }
    send(&e);
    let remote = Store::open(&dir.path().join("remote.db")).unwrap();
    sync::apply_changes(&remote, &sync::changes(&server, 0, 500).unwrap()).unwrap();
    let mut expected = e.lock().unwrap().records("local").unwrap();
    expected.sort_by(|a, b| a.id.cmp(&b.id));
    let mut actual = remote.records("all").unwrap();
    actual.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(actual, expected);
    assert_eq!(scan(&e, true).updated, 0);
    assert_eq!(e.lock().unwrap().pending_count().unwrap(), 0);
    assert_eq!(
        remote
            .conn
            .query_row("SELECT COUNT(*) FROM codex_event_bindings", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn failed_checkpoint_rolls_back_records_aliases_outbox_and_usage_version() {
    let (_dir, e, _root, _) = legacy_fixture();
    let before = e.lock().unwrap().local_records().unwrap();
    let pending = e.lock().unwrap().pending().unwrap();
    let version = e.lock().unwrap().usage_version().unwrap();
    e.lock().unwrap().conn.execute_batch("CREATE TRIGGER reject_checkpoint BEFORE INSERT ON file_state BEGIN SELECT RAISE(ABORT,'fixture checkpoint failure'); END;").unwrap();
    let r = e.scan_harness("codex", false).unwrap();
    assert_eq!(r.imported, 0);
    assert_eq!(r.updated, 0);
    assert_eq!(r.deferred_files, 2);
    {
        let s = e.lock().unwrap();
        assert_eq!(s.local_records().unwrap(), before);
        assert_eq!(s.pending().unwrap(), pending);
        assert_eq!(s.usage_version().unwrap(), version);
        assert_eq!(
            s.conn
                .query_row("SELECT COUNT(*) FROM codex_event_bindings", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        s.conn
            .execute_batch("DROP TRIGGER reject_checkpoint")
            .unwrap();
    }
    assert_eq!(scan(&e, false).imported, 1);
}

#[test]
fn legacy_facts_outside_a_truncated_source_are_retained_during_migration() {
    let (_dir, e, root, old) = legacy_fixture();
    write(
        &root.join(first_name()),
        &[meta(THREAD), usage(1, Some(10), Some(10))],
    );
    scan(&e, false);
    assert_eq!(
        e.lock().unwrap().record(&old[1].id).unwrap().unwrap(),
        old[1]
    );
    assert!(
        !e.lock()
            .unwrap()
            .record(&old[3].id)
            .unwrap()
            .unwrap()
            .deleted
    );
    assert_eq!(scan(&e, true).updated, 0);
}
