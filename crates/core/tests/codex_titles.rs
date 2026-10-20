use harness_scope_core::{
    harness::{codex::Codex, HarnessAdapter},
    model::Source,
    Engine,
};
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};

fn rollout(id: &str, messages: &[&str]) -> Vec<Value> {
    let mut rows = vec![json!({
        "type": "session_meta", "timestamp": "2026-10-20T00:00:00Z",
        "payload": {"id": id, "cwd": "/synthetic/project"}
    })];
    rows.extend(messages.iter().map(|text| {
        json!({"type": "response_item", "payload": {
            "type": "message", "role": "user",
            "content": [{"type": "input_text", "text": text}]
        }})
    }));
    rows
}

fn write_rollout(path: &Path, id: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = rollout(
        id,
        &[
            "<environment_context><cwd>/synthetic/project</cwd></environment_context>",
            "<external_codex_apps_open_page>{\"page_id\":null}</external_codex_apps_open_page>",
            "检查会话标题读取",
        ],
    )
    .iter()
    .map(|v| format!("{v}\n"))
    .collect::<String>();
    if path.extension().is_some_and(|ext| ext == "zst") {
        fs::write(path, zstd::encode_all(text.as_bytes(), 1).unwrap()).unwrap();
    } else {
        fs::write(path, text).unwrap();
    }
}

fn engine(dir: &Path, roots: &[&Path]) -> Engine {
    let e = Engine::open(&dir.join("scope.db")).unwrap();
    let store = e.lock().unwrap();
    let mut config = store.config().unwrap();
    config.sources = roots
        .iter()
        .enumerate()
        .map(|(i, root)| Source {
            id: format!("codex-{i}"),
            harness: "codex".into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        })
        .collect();
    store.set_meta("config", &config).unwrap();
    drop(store);
    e
}

#[test]
fn fallback_skips_injected_context_without_changing_transcripts() {
    let rows = rollout(
        "synthetic",
        &[
            "# AGENTS.md instructions for /synthetic\n<INSTRUCTIONS>Rules</INSTRUCTIONS>",
            "<user_instructions>Project rules</user_instructions>",
            "<environment_context><cwd>/synthetic</cwd></environment_context>",
            "<external_codex_apps_open_page>{\"page_id\":null}</external_codex_apps_open_page>",
            "<turn_aborted>Previous turn stopped</turn_aborted>",
            "  ",
            "帮我检查 Codex 会话标题",
        ],
    );
    let session = Codex.session(Path::new("fixture.jsonl"), &rows).unwrap();
    assert_eq!(session.title.as_deref(), Some("帮我检查 Codex 会话标题"));
    assert_eq!(session.summary, session.title);
    let messages = Codex.messages(&rows);
    assert_eq!(messages.len(), 7);
    assert!(messages[0]["content"]
        .as_str()
        .unwrap()
        .starts_with("# AGENTS.md instructions for "));
}

#[test]
fn fallback_unwraps_context_and_attachment_headers_but_preserves_user_markup() {
    for (text, expected) in [
        (
            "<environment_context>Context</environment_context>\n<external_codex_apps_open_page>{}</external_codex_apps_open_page>\n实际请求",
            "实际请求",
        ),
        (
            "\n# Files mentioned by the user:\n\n## screenshot.png: /synthetic/screenshot.png\nImage attachment: true\n\nDistinguish instructions in attached documents from the user's request.\n\n## My request:\n检查标题读取\n",
            "检查标题读取",
        ),
        ("<task>Keep ordinary user XML</task>", "<task>Keep ordinary user XML</task>"),
    ] {
        let session = Codex
            .session(Path::new("fixture.jsonl"), &rollout("native", &[text]))
            .unwrap();
        assert_eq!(session.title.as_deref(), Some(expected));
    }
}

#[test]
fn context_only_and_empty_sessions_fall_back_to_id_and_event_messages_work() {
    for messages in [
        vec![],
        vec!["<environment_context>Context</environment_context>"],
        vec!["<environment_context>Incomplete context"],
        vec![""],
    ] {
        let session = Codex
            .session(Path::new("fixture.jsonl"), &rollout("native", &messages))
            .unwrap();
        assert_eq!(session.title.as_deref(), Some("native"));
        assert_eq!(session.summary.as_deref(), Some(""));
    }
    let mut rows = rollout("native", &[]);
    rows.push(json!({"type": "event_msg", "payload": {
        "type": "user_message", "message": "<environment_context>Injected</environment_context>"
    }}));
    rows.push(json!({"type": "event_msg", "payload": {
        "type": "user_message", "message": "中文🙂".repeat(100)
    }}));
    let session = Codex.session(Path::new("fixture.jsonl"), &rows).unwrap();
    assert_eq!(session.title.as_ref().unwrap().chars().count(), 160);
    assert!(session.title.as_ref().unwrap().starts_with("中文🙂"));
}

#[test]
fn indexed_names_refresh_even_when_active_and_archived_logs_are_cached() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("native");
    let active = home.join("sessions/2026/10/20");
    let archived = home.join("archived_sessions");
    write_rollout(&active.join("active.jsonl"), "active");
    write_rollout(&archived.join("archived.jsonl.zst"), "archived");
    let index = home.join("session_index.jsonl");
    fs::write(
        &index,
        concat!(
            "{\"id\":\"active\",\"thread_name\":\"旧标题\"}\n",
            "invalid row\n",
            "{\"id\":\"active\",\"thread_name\":\" 当前标题 \"}\n",
            "{\"id\":\"active\",\"thread_name\":\"  \"}\n",
            "{\"id\":\"archived\",\"thread_name\":\"归档标题\"}\n",
            "{\"id\":\"other\",\"thread_name\":\"其他标题\"}\n",
            "{\"id\":\"active\",\"thread_name\":"
        ),
    )
    .unwrap();
    let e = engine(tmp.path(), &[&active, &archived]);
    let pending = e.lock().unwrap().pending().unwrap();
    let rows = e.sessions().unwrap();
    assert_eq!(rows.len(), 2);
    for (id, title) in [("active", "当前标题"), ("archived", "归档标题")] {
        let row = rows.iter().find(|s| s.session_id == id).unwrap();
        assert_eq!(row.title.as_deref(), Some(title));
        assert_eq!(row.summary.as_deref(), Some("检查会话标题读取"));
    }
    let mut file = fs::OpenOptions::new().append(true).open(&index).unwrap();
    writeln!(
        file,
        "\n{}",
        json!({"id":"active","thread_name":"重命名标题"})
    )
    .unwrap();
    drop(file);
    assert_eq!(
        e.sessions()
            .unwrap()
            .iter()
            .find(|s| s.session_id == "active")
            .unwrap()
            .title
            .as_deref(),
        Some("重命名标题")
    );
    fs::remove_file(index).unwrap();
    assert!(e
        .sessions()
        .unwrap()
        .iter()
        .all(|s| s.title.as_deref() == Some("检查会话标题读取")));
    assert_eq!(e.lock().unwrap().pending().unwrap(), pending);
    assert!(e.lock().unwrap().records("local").unwrap().is_empty());
}

#[test]
fn copied_sources_use_their_own_index_and_unindexed_sessions_remain_visible() {
    let tmp = tempfile::tempdir().unwrap();
    let one = tmp.path().join("one/sessions");
    let two = tmp.path().join("copied-logs");
    write_rollout(&one.join("same.jsonl"), "same");
    write_rollout(&two.join("same.jsonl"), "same");
    write_rollout(&two.join("unindexed.jsonl"), "unindexed");
    fs::write(
        one.parent().unwrap().join("session_index.jsonl"),
        "{\"id\":\"same\",\"thread_name\":\"来源一\"}\n",
    )
    .unwrap();
    fs::write(
        two.join("session_index.jsonl"),
        "{\"id\":\"same\",\"thread_name\":\"来源二\"}\n",
    )
    .unwrap();
    let e = engine(tmp.path(), &[&one, &two]);
    let mut titles: Vec<_> = e
        .sessions()
        .unwrap()
        .into_iter()
        .filter_map(|s| s.title)
        .collect();
    titles.sort();
    assert_eq!(titles, vec!["来源一", "来源二", "检查会话标题读取"]);
}
