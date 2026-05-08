# Switch LLM backend from Anthropic API to Claude CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the direct `reqwest` HTTP call to `api.anthropic.com/v1/messages` with a `claude` CLI subprocess invocation, reusing the user's existing Claude Code authentication (Pro/Max OAuth or `ANTHROPIC_API_KEY`). Apply `--json-schema` only to the `analyze` stage to harden enum field parsing (`risk`, `confidence`).

**Architecture:**
- `src/llm.rs` is the only application file that changes. Two private call helpers replace the single `call_claude`: `call_claude_text` (plain text response, used by `semantic_postprocess`) and `call_claude_structured` (uses `--json-schema`, used by `analyze`). Both spawn `claude -p` via `std::process::Command`, write the user payload to stdin, and parse the `--output-format json` envelope.
- Auth becomes an out-of-band concern — Claude CLI handles it. The `ANTHROPIC_API_KEY` env var check, `api_key()`, and `API_URL` constant are removed.
- For testability, a `VOUCH_CLAUDE_BIN` env var lets tests inject a fake binary (small shell script returning canned JSON). Default is `claude`.
- `reqwest` becomes unused and is removed from `Cargo.toml`.

**Tech Stack:** Rust 2021, `std::process::Command`, `serde_json`. No new dependencies.

---

## File Structure

**Modified:**
- `src/llm.rs` — replace `call_claude` HTTP impl with subprocess; split into text vs structured variants; add response envelope parsing; add Analysis JSON schema constant.
- `Cargo.toml` — remove `reqwest` dependency.

**Unchanged but affected by behavior:**
- `src/cache.rs` — cache shape preserved (still stores `Vec<Analysis>` JSON for `analyze`, `Vec<{id,intent,raw_hunk_ids}>` JSON for `semantic_postprocess`). No code change.
- `src/main.rs` — error messages from llm bubble up unchanged.
- `src/models.rs` — `Risk`/`Confidence` serde representations stay the same (lowercase strings).

**No new files.** All changes are localized to `src/llm.rs` and `Cargo.toml`.

---

## Conventions

- Tests live in the same `mod tests` block at the bottom of `src/llm.rs` (existing convention — see `build_semantic_groups_hunks` etc.).
- Test fake-binary scripts are written to `tempfile::TempDir` paths inside each test, not committed.
- `VOUCH_CLAUDE_BIN` is read once per call (no caching), so tests can set/unset between cases.
- Error strings stay in English with `vouch:` prefix already added by `main.rs`.

---

### Task 1: Add the Analysis JSON schema constant

**Files:**
- Modify: `src/llm.rs` (add new constant near top)

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src/llm.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib llm::tests::analysis_schema_is_object_with_items_array`
Expected: FAIL — `cannot find value 'ANALYSIS_SCHEMA' in this scope`

- [ ] **Step 3: Add the constant**

Add to `src/llm.rs` near the other top-level constants (after `ANALYSIS_PROMPT`):

```rust
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib llm::tests::analysis_schema_is_object_with_items_array`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/llm.rs
git commit -m "feat(llm): add Analysis JSON schema constant for structured output"
```

---

### Task 2: Update ANALYSIS_PROMPT to ask for wrapper object

**Files:**
- Modify: `src/llm.rs:12` — `ANALYSIS_PROMPT` constant

The schema requires the root to be an object (Anthropic tool input_schema constraint), but the current prompt asks for a JSON array. We need to ask for `{"items": [...]}` instead.

- [ ] **Step 1: Update the prompt**

Replace the current `ANALYSIS_PROMPT` string with:

```rust
const ANALYSIS_PROMPT: &str = r#"You receive a list of SemanticHunks (each with merged diff). For each, output an object with field "items" whose value is a JSON array of objects with fields: id, risk (high|med|low), risk_reason (한 줄), confidence (confident|uncertain|guess), summary_ko (한국어 한 줄). Be conservative on risk: business logic, security, new dependencies → high. Mechanical (rename/import/format) → low. Output ONLY the JSON object, no markdown fences."#;
```

- [ ] **Step 2: Verify the test suite still compiles**

Run: `cargo test --lib --no-run`
Expected: builds successfully (no test changes yet — prompts are not asserted in tests)

- [ ] **Step 3: Commit**

```bash
git add src/llm.rs
git commit -m "feat(llm): ask analyze prompt for wrapper object to match schema root"
```

---

### Task 3: Add response envelope parser for `--output-format json`

The CLI's `--output-format json` returns a single line of JSON like:

```json
{"type":"result","subtype":"success","is_error":false,"result":"<text>","structured_output":{...},"total_cost_usd":...}
```

We need a small parser that extracts either the `result` text (text mode) or the `structured_output` object (schema mode), and surfaces `is_error`/`api_error_status` clearly.

**Files:**
- Modify: `src/llm.rs` — add `parse_envelope` private fn

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib llm::tests::envelope`
Expected: FAIL — `cannot find function 'parse_envelope'` and `cannot find type 'Envelope'`

- [ ] **Step 3: Implement the parser**

Add inside `src/llm.rs` (near the other private helpers, above `call_claude`):

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib llm::tests::envelope`
Expected: 4 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/llm.rs
git commit -m "feat(llm): add response envelope parser for claude --output-format json"
```

---

### Task 4: Implement subprocess `call_claude_text` (replaces HTTP `call_claude` for semantic stage)

**Files:**
- Modify: `src/llm.rs` — replace `call_claude` body

The new function spawns `claude -p` with shared flags, pipes the user payload to stdin, parses the envelope, and returns the `result` string.

Shared flags for both helpers:
- `-p` (headless)
- `--model <model>`
- `--system-prompt <system>`
- `--tools ""` (no tool use)
- `--no-session-persistence`
- `--output-format json`

Text variant: no `--json-schema`. Returns `envelope.result`.

- [ ] **Step 1: Write the failing tests**

Add a small helper at the top of `mod tests` to write a fake binary:

```rust
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
```

Then add the test:

```rust
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
```

Add `tempfile` to `[dependencies]`? **No** — it's already in `[dev-dependencies]` (see existing `Cargo.toml`), and the tests run as dev. The use stays inside `#[cfg(test)]`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib llm::tests::call_claude_text`
Expected: FAIL — `cannot find function 'call_claude_text'`

- [ ] **Step 3: Implement `call_claude_text` and the binary lookup helper**

Replace the existing `call_claude` function in `src/llm.rs` with:

```rust
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
```

Also delete from `src/llm.rs`:
- The `API_URL` constant
- The `api_key()` function
- The old `call_claude` function

Update the call site in `semantic_postprocess` (around line 137) from:

```rust
let resp_text = call_claude(SEMANTIC_PROMPT, &payload)?;
```

to:

```rust
let resp_text = call_claude_text(SEMANTIC_PROMPT, &payload)?;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib llm::tests::call_claude_text`
Expected: 2 tests PASS

- [ ] **Step 5: Run full test suite to verify nothing else broke**

Run: `cargo test --lib`
Expected: all existing tests still PASS (extract_json, build_semantic, models tests, cache tests)

- [ ] **Step 6: Commit**

```bash
git add src/llm.rs
git commit -m "feat(llm): swap HTTP API call for claude CLI subprocess (text mode)"
```

---

### Task 5: Implement `call_claude_structured` and wire it into `analyze`

The structured variant adds `--json-schema <ANALYSIS_SCHEMA>` and reads `envelope.structured_output["items"]` instead of `envelope.result`.

**Files:**
- Modify: `src/llm.rs` — add `call_claude_structured`, update `analyze`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
#[test]
fn call_claude_structured_returns_items_array() {
    let dir = tempfile::TempDir::new().unwrap();
    let envelope = r#"{"type":"result","is_error":false,"result":"chatty","structured_output":{"items":[{"id":"s0","risk":"high","risk_reason":"r","confidence":"confident","summary_ko":"요약"}]}}"#;
    let bin = fake_claude_bin(dir.path(), "claude", envelope);
    std::env::set_var("VOUCH_CLAUDE_BIN", &bin);
    let arr = call_claude_structured("sys", "user", "{\"type\":\"object\"}").unwrap();
    std::env::remove_var("VOUCH_CLAUDE_BIN");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], "s0");
    assert_eq!(arr[0]["risk"], "high");
}

#[test]
fn call_claude_structured_errors_when_items_missing() {
    let dir = tempfile::TempDir::new().unwrap();
    let envelope = r#"{"type":"result","is_error":false,"result":"chatty","structured_output":{}}"#;
    let bin = fake_claude_bin(dir.path(), "claude", envelope);
    std::env::set_var("VOUCH_CLAUDE_BIN", &bin);
    let err = call_claude_structured("sys", "user", "{}").unwrap_err();
    std::env::remove_var("VOUCH_CLAUDE_BIN");
    assert!(err.contains("items"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib llm::tests::call_claude_structured`
Expected: FAIL — `cannot find function 'call_claude_structured'`

- [ ] **Step 3: Implement `call_claude_structured`**

Add to `src/llm.rs` next to `call_claude_text`:

```rust
fn call_claude_structured(
    system: &str,
    user: &str,
    schema: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let env = run_claude(system, user, Some(schema))?;
    let items = env.structured_output.get("items").cloned().ok_or_else(|| {
        "claude CLI structured_output missing 'items' field".to_string()
    })?;
    serde_json::from_value::<Vec<serde_json::Value>>(items)
        .map_err(|e| format!("structured_output.items not an array: {}", e))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib llm::tests::call_claude_structured`
Expected: 2 tests PASS

- [ ] **Step 5: Update `analyze` to use the structured variant**

Find this block in `src/llm.rs` (currently around lines 172–175):

```rust
    let resp_text = call_claude(ANALYSIS_PROMPT, &payload)?;
    let json_text = extract_json(&resp_text);
    let analyses: Vec<Analysis> =
        serde_json::from_str(&json_text).map_err(|e| format!("JSON parse error: {}", e))?;
```

Replace with:

```rust
    let items = call_claude_structured(ANALYSIS_PROMPT, &payload, ANALYSIS_SCHEMA)?;
    let analyses: Vec<Analysis> = items
        .into_iter()
        .map(|v| serde_json::from_value::<Analysis>(v))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("Analysis parse error: {}", e))?;
```

- [ ] **Step 6: Run full test suite**

Run: `cargo test --lib`
Expected: all PASS

- [ ] **Step 7: Commit**

```bash
git add src/llm.rs
git commit -m "feat(llm): apply --json-schema to analyze stage for enum field safety"
```

---

### Task 6: Remove the `reqwest` dependency

`reqwest` was only used by the old `call_claude`. Verify nothing else uses it, then drop from `Cargo.toml`.

**Files:**
- Modify: `Cargo.toml` — remove `reqwest = ...` line

- [ ] **Step 1: Verify no remaining `reqwest` usage**

Run: `grep -rn 'reqwest' src/ tests/`
Expected: no matches (or only matches inside comments)

If matches exist, stop and resolve them before continuing.

- [ ] **Step 2: Edit `Cargo.toml`**

Remove this line from `[dependencies]`:

```toml
reqwest = { version = "0.12", features = ["blocking", "json"] }
```

- [ ] **Step 3: Rebuild release to verify**

Run: `cargo build --release`
Expected: builds successfully, `Cargo.lock` updates to drop reqwest and its transitive deps.

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: all PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: drop reqwest dependency now that LLM uses claude CLI"
```

---

### Task 7: Manual smoke test against real `claude` binary

This task is not a unit test — it's a one-time live verification that the wired-up subprocess actually works against the user's authenticated Claude CLI.

**Files:** none (manual verification only).

- [ ] **Step 1: Ensure the cache won't short-circuit**

Run: `rm -rf fixtures/responses` (or set `VOUCH_CACHE_DIR` to a fresh temp dir for this run).

- [ ] **Step 2: Run vouch on uncommitted changes**

Make a trivial edit somewhere (e.g., add a blank line to `README` if one exists, or stage any small change) so vouch has something to review, then run:

```bash
./target/release/vouch
```

Expected: TUI launches, shows grouped semantic units and risk-scored items. No `ANTHROPIC_API_KEY` error.

- [ ] **Step 3: Verify analyze produced valid Risk/Confidence enums**

Inspect the cache file for the analyze stage:

```bash
ls fixtures/responses/analyze.*.json
cat fixtures/responses/analyze.*.json
```

Expected: every `risk` value is one of `high|med|low`; every `confidence` is one of `confident|uncertain|guess`.

- [ ] **Step 4: Test failure path — claude not in PATH**

Run: `VOUCH_CLAUDE_BIN=/nonexistent ./target/release/vouch`
Expected: clean error message like `vouch: failed to spawn claude CLI: ...`. No panic.

- [ ] **Step 5: No commit needed (verification only)**

If any smoke test fails, file findings and revisit Tasks 4–5 before merging.

---

## Self-Review Checklist (run after writing the plan)

- **Spec coverage:** All four user-confirmed decisions are addressed: (a) replace HTTP with CLI → Tasks 4–6; (b) keep Pro/Max OAuth via no `--bare` → flag set in Task 4; (c) `--json-schema` only for analyze → Task 5; (d) parsing path differs by stage → Tasks 4 (text) vs 5 (structured).
- **Placeholder scan:** No "TBD"/"handle errors appropriately"/"similar to" in any task.
- **Type consistency:** `Envelope { is_error, result: Option<String>, structured_output: serde_json::Value }` defined in Task 3 and used unchanged in Tasks 4 and 5. `run_claude` signature in Task 4 (`schema: Option<&str>`) matches usage in Task 5 (`Some(schema)`). `ANALYSIS_SCHEMA` defined in Task 1 referenced in Task 5.
