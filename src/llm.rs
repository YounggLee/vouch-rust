use crate::cache::Cache;
use crate::models::{Analysis, RawHunk, SemanticHunk};
use std::collections::HashMap;

const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const API_URL: &str = "https://api.anthropic.com/v1/messages";

const SEMANTIC_PROMPT: &str = r#"You receive a list of raw git hunks from one change. Group hunks that share a single SPECIFIC intent into a SemanticHunk. Each SemanticHunk should describe ONE concrete action (e.g., "check_access 함수 추가", "users 테이블에 role 컬럼 추가"). DO NOT lump everything into one giant SemanticHunk — aim for 3-8 SemanticHunks for a typical multi-file change. Group only when hunks are mechanically inseparable.

Output JSON array where each item is: {"id": "s<n>", "intent": "한국어 한 줄 의도 (구체적으로)", "raw_hunk_ids": ["r1", ...]}. Each raw_hunk_id must appear in exactly one SemanticHunk. Output ONLY the JSON array, no markdown fences."#;

const ANALYSIS_PROMPT: &str = r#"You receive a list of SemanticHunks (each with merged diff). For each, output a JSON array of objects with fields: id, risk (high|med|low), risk_reason (한 줄), confidence (confident|uncertain|guess), summary_ko (한국어 한 줄). Be conservative on risk: business logic, security, new dependencies → high. Mechanical (rename/import/format) → low. Output ONLY the JSON array, no markdown fences."#;

fn model() -> String {
    std::env::var("VOUCH_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string())
}

fn api_key() -> Result<String, String> {
    std::env::var("ANTHROPIC_API_KEY").map_err(|_| "ANTHROPIC_API_KEY not set".to_string())
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

fn call_claude(system: &str, user_content: &str) -> Result<String, String> {
    let key = api_key()?;
    let body = serde_json::json!({
        "model": model(),
        "max_tokens": 4096,
        "temperature": 0.1,
        "system": system,
        "messages": [{"role": "user", "content": user_content}]
    });

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(API_URL)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("Claude API request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(format!("Claude API error {}: {}", status, body));
    }

    let resp_json: serde_json::Value = resp
        .json()
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    resp_json["content"][0]["text"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| "No text in Claude response".to_string())
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

    let resp_text = call_claude(SEMANTIC_PROMPT, &payload)?;
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

    let resp_text = call_claude(ANALYSIS_PROMPT, &payload)?;
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
}
