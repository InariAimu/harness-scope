use super::files;
use crate::model::Session;
use serde_json::{json, Value};
use std::path::Path;

pub fn session(harness: &str, path: &Path, lines: &[Value]) -> Option<Session> {
    let meta = lines.iter().find(|v| v["type"] == "session_meta");
    let id = if harness == "codex" {
        meta?.pointer("/payload/id")?.as_str()?.to_owned()
    } else {
        lines
            .iter()
            .find_map(|v| v["sessionId"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into()
            })
    };
    let messages = if harness == "codex" {
        codex(lines)
    } else {
        claude(lines)
    };
    let first = messages
        .iter()
        .find(|m| m["role"] == "user")
        .map(|m| files::text(&m["content"]))
        .unwrap_or_default();
    let summary: String = first.chars().take(160).collect();
    let times: Vec<i64> = lines
        .iter()
        .filter_map(|v| files::timestamp(&v["timestamp"]))
        .collect();
    let project = meta
        .and_then(|m| m.pointer("/payload/cwd"))
        .and_then(Value::as_str)
        .or_else(|| lines.iter().find_map(|v| v["cwd"].as_str()));
    // Session IDs are emitted by the harness, but still quote them as shell arguments.
    let resume = if id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Some(format!(
            "{} {} {}",
            if harness == "claude" {
                "claude"
            } else {
                "codex"
            },
            if harness == "claude" {
                "--resume"
            } else {
                "resume"
            },
            id
        ))
    } else {
        None
    };
    Some(Session {
        provider_id: harness.into(),
        session_id: id.clone(),
        title: Some(if summary.is_empty() {
            id
        } else {
            summary.clone()
        }),
        summary: Some(summary),
        project_dir: project.map(str::to_owned),
        created_at: times.iter().min().map(|t| t * 1000),
        last_active_at: times.iter().max().map(|t| t * 1000),
        source_path: Some(path.to_string_lossy().into()),
        resume_command: resume,
        can_delete: true,
    })
}
pub(super) fn call(id: &str, name: &str, input: &Value) -> Value {
    let input_text = if let Some(s) = input.as_str() {
        s.to_owned()
    } else {
        serde_json::to_string_pretty(input).unwrap_or_default()
    };
    let lower = name.to_lowercase();
    let kind = if lower.contains("bash") || lower.contains("exec") || lower.contains("shell") {
        "shell"
    } else if lower.contains("read") {
        "read"
    } else if lower.contains("edit") || lower.contains("patch") {
        "edit"
    } else if lower.contains("search") || lower.contains("grep") {
        "search"
    } else if lower.contains("agent") {
        "agent"
    } else {
        "other"
    };
    json!({"type":"tool_call","id":id,"rawName":name,"kind":kind,"title":name,"inputPreview":input_text,"inputTotalLen":input_text.chars().count()})
}
pub(super) fn result(id: &str, content: &Value, error: bool) -> Value {
    let text = if content.is_string() || content.is_array() {
        files::text(content)
    } else {
        content.to_string()
    };
    json!({"type":"tool_result","callId":id,"status":if error{"error"}else{"unknown"},"preview":text,"totalLen":text.chars().count(),"lineCount":text.lines().count(),"truncated":false})
}
pub fn claude(lines: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for v in lines {
        let role = v["type"].as_str().unwrap_or("");
        if role != "assistant" && role != "user" {
            continue;
        }
        let m = &v["message"];
        let mut blocks = Vec::new();
        let content = &m["content"];
        if let Some(xs) = content.as_array() {
            for (block_index, b) in xs.iter().enumerate() {
                match b["type"].as_str().unwrap_or("") {
                    "text" => {
                        blocks.push(json!({"type":"text","text":b["text"].as_str().unwrap_or("")}))
                    }
                    "thinking" => blocks.push(
                        json!({"type":"thinking","text":b["thinking"].as_str().unwrap_or("")}),
                    ),
                    "image" => {
                        if let Some(line) = v["_scopeOffset"].as_u64() {
                            blocks.push(json!({"type":"image","image":{"source":{"kind":"inline","content":{"kind":"jsonl","offset":line,"len":v["_scopeLen"],"pointer":format!("/message/content/{block_index}/source")}},"mediaType":b["source"]["media_type"].as_str().unwrap_or("image/png"),"size":b["source"]["data"].as_str().map(|s|s.len()*3/4).unwrap_or(0)}}))
                        }
                    }
                    "tool_use" => blocks.push(call(
                        b["id"].as_str().unwrap_or(""),
                        b["name"].as_str().unwrap_or("tool"),
                        &b["input"],
                    )),
                    "tool_result" => blocks.push(result(
                        b["tool_use_id"].as_str().unwrap_or(""),
                        &b["content"],
                        b["is_error"].as_bool().unwrap_or(false),
                    )),
                    _ => {}
                }
            }
        } else if let Some(s) = content.as_str() {
            blocks.push(json!({"type":"text","text":s}))
        }
        if !blocks.is_empty() {
            out.push(json!({"role":role,"content":files::text(content),"id":v["uuid"].as_str(),"ts":files::timestamp(&v["timestamp"]).map(|t|t*1000),"blocks":blocks,"meta":{"model":m["model"],"inputTokens":m["usage"]["input_tokens"],"outputTokens":m["usage"]["output_tokens"]}}))
        }
    }
    out
}
pub fn codex(lines: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for (i, v) in lines.iter().enumerate() {
        if v["type"] != "response_item" {
            continue;
        }
        let p = &v["payload"];
        let kind = p["type"].as_str().unwrap_or("");
        let (role, blocks) = match kind {
            "message" => (p["role"].as_str().unwrap_or("assistant"), codex_blocks(v)),
            "function_call" | "custom_tool_call" => (
                "assistant",
                vec![call(
                    p["call_id"].as_str().unwrap_or(""),
                    p["name"].as_str().unwrap_or("tool"),
                    p.get("arguments")
                        .or_else(|| p.get("input"))
                        .unwrap_or(&Value::Null),
                )],
            ),
            "function_call_output" | "custom_tool_call_output" => (
                "tool",
                vec![result(
                    p["call_id"].as_str().unwrap_or(""),
                    &p["output"],
                    false,
                )],
            ),
            "reasoning" => (
                "assistant",
                vec![
                    json!({"type":"thinking","text":files::text(&p["summary"]),"redacted":p["encrypted_content"].is_string()}),
                ],
            ),
            _ => continue,
        };
        out.push(json!({"role":role,"content":files::text(&p["content"]),"id":format!("item-{i}"),"ts":files::timestamp(&v["timestamp"]).map(|t|t*1000),"blocks":blocks}));
    }
    out
}
fn codex_blocks(row: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();
    if let Some(content) = row["payload"]["content"].as_array() {
        for (i, item) in content.iter().enumerate() {
            if let Some(text) = item["text"].as_str() {
                blocks.push(json!({"type":"text","text":text}))
            }
            if item["type"] == "input_image"
                && item["image_url"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("data:image/"))
                && !row["_scopeOffset"].is_null()
            {
                blocks.push(json!({"type":"image","image":{"source":{"kind":"inline","content":{"kind":"jsonl","offset":row["_scopeOffset"],"len":row["_scopeLen"],"pointer":format!("/payload/content/{i}/image_url")}},"mediaType":"image/png","size":item["image_url"].as_str().map(|s|s.len()*3/4).unwrap_or(0)}}));
            }
        }
    } else {
        blocks.push(json!({"type":"text","text":files::text(&row["payload"]["content"])}))
    }
    blocks
}
