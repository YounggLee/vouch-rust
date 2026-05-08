use crate::cache::Cache;
use crate::models::{Analysis, RawHunk, SemanticHunk};
use std::collections::HashMap;

const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

const SEMANTIC_PROMPT: &str = r#"You receive a list of raw git hunks from one change. Group hunks that share a single SPECIFIC intent into a SemanticHunk. Each SemanticHunk should describe ONE concrete action (e.g., "check_access 함수 추가", "users 테이블에 role 컬럼 추가"). DO NOT lump everything into one giant SemanticHunk — aim for 3-8 SemanticHunks for a typical multi-file change. Group only when hunks are mechanically inseparable.

Output JSON array where each item is: {"id": "s<n>", "intent": "한국어 한 줄 의도 (구체적으로)", "raw_hunk_ids": ["r1", ...]}. Each raw_hunk_id must appear in exactly one SemanticHunk. Output ONLY the JSON array, no markdown fences."#;

const ANALYSIS_PROMPT: &str = r#"You receive a list of SemanticHunks (each with merged diff). For each, output an object with field "items" whose value is a JSON array of objects with fields: id, risk (high|med|low), risk_reason (한 줄), confidence (confident|uncertain|guess), summary_ko (한국어 한 줄). Be conservative on risk: business logic, security, new dependencies → high. Mechanical (rename/import/format) → low. Output ONLY the JSON object, no markdown fences."#;

const ANALYSIS_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": {"type": "string"},
          "risk": {"type": "string", "enum": ["high", "med", "low"]},
          "risk_reason": {"type": "string"},
          "confidence": {"type": "string", "enum": ["confident", "uncertain", "guess"]},
          "summary_ko": {"type": "string"}
        },
        "required": ["id", "risk", "risk_reason", "confidence", "summary_ko"]
      }
    }
  },
  "required": ["items"]
}"#;

fn model() -> String {
    std::env::var("VOUCH_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string())
}

fn cache_only() -> bool {
    std::env::var("VOUCH_CACHE_ONLY").as_deref() == Ok("1")
}

pub fn extract_json(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with("```") {
        let start = trimmed.find('\n').map(|i| i + 1).unwrap_or(0);
        let end = trimmed.rfind("```").unwrap_or(trimmed.len());
        return trimmed[start..end].trim().to_string();
    }
    trimmed.to_string()
}

#[derive(Debug)]
struct Envelope {
    is_error: bool,
    result: Option<String>,
    structured_output: serde_json::Value,
}

fn parse_envelope(line: &str) -> Result<Envelope, String> {
    let v: serde_json::Value = serde_json::from_str(line.trim())
        .map_err(|e| format!("claude CLI envelope parse error: {}", e))?;
    Ok(Envelope {
        is_error: v["is_error"].as_bool().unwrap_or(false),
        result: v["result"].as_str().map(String::from),
        structured_output: v["structured_output"].clone(),
    })
}

fn claude_bin() -> String {
    std::env::var("VOUCH_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string())
}

fn run_claude(system: &str, user: &str, schema: Option<&str>) -> Result<Envelope, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(claude_bin());
    cmd.arg("-p")
        .arg("--model")
        .arg(model())
        .arg("--system-prompt")
        .arg(system)
        .arg("--tools")
        .arg("")
        .arg("--no-session-persistence")
        .arg("--output-format")
        .arg("json");
    if let Some(s) = schema {
        cmd.arg("--json-schema").arg(s);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn claude CLI: {}", e))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "claude stdin unavailable".to_string())?
        .write_all(user.as_bytes())
        .map_err(|e| format!("failed to write claude stdin: {}", e))?;

    let out = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for claude: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("claude CLI exited {}: {}", out.status, stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let env = parse_envelope(&stdout)?;
    if env.is_error {
        let msg = env.result.unwrap_or_else(|| "(no result)".to_string());
        return Err(format!("claude CLI error: {}", msg));
    }
    Ok(env)
}

fn call_claude_text(system: &str, user: &str) -> Result<String, String> {
    let env = run_claude(system, user, None)?;
    env.result
        .ok_or_else(|| "claude CLI returned no result text".to_string())
}

pub fn build_semantic(raw_hunks: &[RawHunk], parsed: &[serde_json::Value]) -> Vec<SemanticHunk> {
    let by_id: HashMap<&str, &RawHunk> = raw_hunks.iter().map(|h| (h.id.as_str(), h)).collect();
    let mut out = Vec::new();
    for item in parsed {
        let id = item["id"].as_str().unwrap_or("").to_string();
        let intent = item["intent"].as_str().unwrap_or("").to_string();
        let raw_ids: Vec<String> = item["raw_hunk_ids"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let members: Vec<&RawHunk> = raw_ids
            .iter()
            .filter_map(|rid| by_id.get(rid.as_str()).copied())
            .collect();
        let mut files: Vec<String> = members.iter().map(|m| m.file.clone()).collect();
        files.sort();
        files.dedup();
        let merged = members
            .iter()
            .map(|m| format!("--- {}\n{}\n{}", m.file, m.header, m.body))
            .collect::<Vec<_>>()
            .join("\n\n");
        let actual_ids: Vec<String> = members.iter().map(|m| m.id.clone()).collect();
        out.push(SemanticHunk {
            id,
            intent,
            files,
            raw_hunk_ids: actual_ids,
            merged_diff: merged,
        });
    }
    out
}

pub fn semantic_postprocess(
    raw_hunks: &[RawHunk],
    cache: &Cache,
) -> Result<Vec<SemanticHunk>, String> {
    let payload_obj: Vec<serde_json::Value> = raw_hunks
        .iter()
        .map(|h| {
            serde_json::json!({
                "id": h.id,
                "file": h.file,
                "header": h.header,
                "body": h.body,
            })
        })
        .collect();
    let payload = serde_json::to_string(&payload_obj).unwrap();

    if let Some(cached) = cache.load("semantic_postprocess", &payload) {
        let parsed: Vec<serde_json::Value> =
            serde_json::from_value(cached).map_err(|e| e.to_string())?;
        return Ok(build_semantic(raw_hunks, &parsed));
    }

    if cache_only() {
        return Err("VOUCH_CACHE_ONLY=1 but no cached response".to_string());
    }

    let resp_text = call_claude_text(SEMANTIC_PROMPT, &payload)?;
    let json_text = extract_json(&resp_text);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&json_text).map_err(|e| format!("JSON parse error: {}", e))?;
    cache.save(
        "semantic_postprocess",
        &payload,
        &serde_json::Value::Array(parsed.clone()),
    );
    Ok(build_semantic(raw_hunks, &parsed))
}

pub fn analyze(semantic_hunks: &[SemanticHunk], cache: &Cache) -> Result<Vec<Analysis>, String> {
    let payload_obj: Vec<serde_json::Value> = semantic_hunks
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "intent": s.intent,
                "files": s.files,
                "diff": s.merged_diff,
            })
        })
        .collect();
    let payload = serde_json::to_string(&payload_obj).unwrap();

    if let Some(cached) = cache.load("analyze", &payload) {
        let analyses: Vec<Analysis> = serde_json::from_value(cached).map_err(|e| e.to_string())?;
        return Ok(analyses);
    }

    if cache_only() {
        return Err("VOUCH_CACHE_ONLY=1 but no cached response".to_string());
    }

    let resp_text = call_claude_text(ANALYSIS_PROMPT, &payload)?;
    let json_text = extract_json(&resp_text);
    let analyses: Vec<Analysis> =
        serde_json::from_str(&json_text).map_err(|e| format!("JSON parse error: {}", e))?;
    cache.save(
        "analyze",
        &payload,
        &serde_json::to_value(&analyses).unwrap(),
    );
    Ok(analyses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RawHunk;

    fn fake_claude_bin(dir: &std::path::Path, name: &str, output_json: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        let script = format!("#!/bin/sh\ncat > /dev/null\nprintf '%s' '{}'\n", output_json.replace('\'', "'\\''"));
        std::fs::write(&path, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn call_claude_text_returns_result_field() {
        let dir = tempfile::TempDir::new().unwrap();
        let envelope = r#"{"type":"result","is_error":false,"result":"[{\"id\":\"s0\"}]"}"#;
        let bin = fake_claude_bin(dir.path(), "claude", envelope);
        std::env::set_var("VOUCH_CLAUDE_BIN", &bin);
        let out = call_claude_text("sys", "user").unwrap();
        std::env::remove_var("VOUCH_CLAUDE_BIN");
        assert_eq!(out, "[{\"id\":\"s0\"}]");
    }

    #[test]
    fn call_claude_text_propagates_is_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let envelope = r#"{"type":"result","is_error":true,"result":"Not logged in"}"#;
        let bin = fake_claude_bin(dir.path(), "claude", envelope);
        std::env::set_var("VOUCH_CLAUDE_BIN", &bin);
        let err = call_claude_text("sys", "user").unwrap_err();
        std::env::remove_var("VOUCH_CLAUDE_BIN");
        assert!(err.contains("Not logged in"));
    }

    fn sample_hunks() -> Vec<RawHunk> {
        vec![
            RawHunk {
                id: "r0".into(),
                file: "auth.py".into(),
                old_start: 10,
                old_lines: 5,
                new_start: 10,
                new_lines: 6,
                header: "@@ -10,5 +10,6 @@".into(),
                body: "-old\n+new".into(),
            },
            RawHunk {
                id: "r1".into(),
                file: "views.py".into(),
                old_start: 45,
                old_lines: 1,
                new_start: 45,
                new_lines: 4,
                header: "@@ -45,1 +45,4 @@".into(),
                body: "+def admin():".into(),
            },
        ]
    }

    #[test]
    fn build_semantic_groups_hunks() {
        let raw = sample_hunks();
        let parsed = vec![
            serde_json::json!({
                "id": "s0",
                "intent": "auth 강화",
                "raw_hunk_ids": ["r0"]
            }),
            serde_json::json!({
                "id": "s1",
                "intent": "admin 뷰 추가",
                "raw_hunk_ids": ["r1"]
            }),
        ];
        let result = build_semantic(&raw, &parsed);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "s0");
        assert_eq!(result[0].files, vec!["auth.py"]);
        assert!(result[0].merged_diff.contains("auth.py"));
        assert_eq!(result[1].id, "s1");
        assert_eq!(result[1].files, vec!["views.py"]);
    }

    #[test]
    fn build_semantic_skips_unknown_hunk_ids() {
        let raw = sample_hunks();
        let parsed = vec![serde_json::json!({
            "id": "s0",
            "intent": "test",
            "raw_hunk_ids": ["r0", "r999"]
        })];
        let result = build_semantic(&raw, &parsed);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].raw_hunk_ids, vec!["r0"]);
    }

    #[test]
    fn extract_json_from_markdown_fences() {
        let text = "```json\n[{\"id\": \"s0\"}]\n```";
        let extracted = extract_json(text);
        assert!(extracted.contains("\"id\""));
        let parsed: serde_json::Value = serde_json::from_str(&extracted).unwrap();
        assert!(parsed.is_array());
    }

    #[test]
    fn extract_json_plain() {
        let text = "[{\"id\": \"s0\"}]";
        let extracted = extract_json(text);
        let parsed: serde_json::Value = serde_json::from_str(&extracted).unwrap();
        assert!(parsed.is_array());
    }

    #[test]
    fn envelope_extracts_result_text() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"hello"}"#;
        let env = parse_envelope(line).expect("parse");
        assert_eq!(env.result.as_deref(), Some("hello"));
        assert!(env.structured_output.is_null());
        assert!(!env.is_error);
    }

    #[test]
    fn envelope_extracts_structured_output() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"chatty","structured_output":{"items":[{"id":"s0"}]}}"#;
        let env = parse_envelope(line).expect("parse");
        assert_eq!(env.structured_output["items"][0]["id"], "s0");
    }

    #[test]
    fn envelope_surfaces_errors() {
        let line = r#"{"type":"result","is_error":true,"result":"Not logged in · Please run /login"}"#;
        let env = parse_envelope(line).expect("parse");
        assert!(env.is_error);
        assert_eq!(env.result.as_deref(), Some("Not logged in · Please run /login"));
    }

    #[test]
    fn envelope_handles_invalid_json() {
        let err = parse_envelope("not json at all").unwrap_err();
        assert!(err.contains("envelope"));
    }

    #[test]
    fn analysis_schema_is_object_with_items_array() {
        let schema: serde_json::Value =
            serde_json::from_str(ANALYSIS_SCHEMA).expect("schema must be valid JSON");
        assert_eq!(schema["type"], "object");
        let items = &schema["properties"]["items"];
        assert_eq!(items["type"], "array");
        let item_props = &items["items"]["properties"];
        let risk_enum = item_props["risk"]["enum"]
            .as_array()
            .expect("risk enum");
        let risks: Vec<&str> = risk_enum.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(risks, vec!["high", "med", "low"]);
        let conf_enum = item_props["confidence"]["enum"]
            .as_array()
            .expect("confidence enum");
        let confs: Vec<&str> = conf_enum.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(confs, vec!["confident", "uncertain", "guess"]);
    }
}
