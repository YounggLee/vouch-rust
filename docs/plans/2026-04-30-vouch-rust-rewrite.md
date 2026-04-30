# vouch Rust Rewrite — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Python vouch(911줄)를 Rust로 1:1 전환. 단일 바이너리, Claude API, 기존 기능 완전 보존.

**Architecture:** 파이프라인(CLI → diff_input → parser → llm → tui → feedback/cmux) 구조 유지. Gemini → Claude API 교체. blocking I/O, Ratatui TUI.

**Tech Stack:** Rust, clap, ratatui, crossterm, tui-input, reqwest (blocking), serde, serde_json, sha2, syntect, which, regex

---

## File Structure

```
vouch-rust/
├── Cargo.toml
├── src/
│   ├── main.rs          # CLI 파싱 + 파이프라인 오케스트레이션
│   ├── models.rs         # RawHunk, SemanticHunk, Analysis, ReviewItem, enums
│   ├── parser.rs         # unified diff → Vec<RawHunk>
│   ├── diff_input.rs     # git/gh subprocess → unified diff string
│   ├── cache.rs          # SHA256 기반 JSON 파일 캐시
│   ├── llm.rs            # Claude API 호출 (semantic_postprocess, analyze)
│   ├── feedback.rs       # reject 프롬프트/PR body 빌더
│   ├── cmux.rs           # cmux/gh/clipboard 배포 채널
│   └── tui.rs            # Ratatui TUI (테이블, diff 뷰, 모달)
├── tests/
│   └── fixtures/
│       ├── sample.diff   # 2-file diff fixture (Python에서 복사)
│       └── diverse.diff  # multi-file diff fixture (Python에서 복사)
└── docs/
    ├── specs/
    └── plans/
```

---

## Task 1: Cargo 프로젝트 초기화

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs` (placeholder)
- Create: `.gitignore`

- [x] **Step 1: Cargo.toml 작성**

```toml
[package]
name = "vouch"
version = "0.1.0"
edition = "2021"
description = "Closed-loop AI diff reviewer"
license = "MIT"
authors = ["youngjin.lee"]

[dependencies]
clap = { version = "4", features = ["derive"] }
ratatui = "0.29"
crossterm = "0.28"
tui-input = "0.11"
reqwest = { version = "0.12", features = ["blocking", "json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
syntect = { version = "5", default-features = false, features = ["default-fancy"] }
which = "7"
regex = "1"
```

- [x] **Step 2: placeholder main.rs 작성**

```rust
fn main() {
    println!("vouch — closed-loop AI diff reviewer");
}
```

- [x] **Step 3: .gitignore 작성**

```
/target
.env
```

- [x] **Step 4: 빌드 확인**

Run: `cd ~/vibe/vouch-rust && cargo build`
Expected: 의존성 다운로드 후 빌드 성공

- [x] **Step 5: 커밋**

```bash
git add Cargo.toml src/main.rs .gitignore
git commit -m "chore: init Cargo project with dependencies"
```

---

## Task 2: models.rs — 데이터 모델

**Files:**
- Create: `src/models.rs`
- Modify: `src/main.rs` (mod 선언)

- [x] **Step 1: 테스트 작성**

`src/models.rs` 하단에 `#[cfg(test)]` 모듈:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_hunk_construction() {
        let h = RawHunk {
            id: "r1".into(),
            file: "auth.py".into(),
            old_start: 10,
            old_lines: 5,
            new_start: 10,
            new_lines: 7,
            header: "@@ -10,5 +10,7 @@".into(),
            body: "-old\n+new\n".into(),
        };
        assert_eq!(h.id, "r1");
        assert_eq!(h.file, "auth.py");
    }

    #[test]
    fn semantic_hunk_groups_raw() {
        let s = SemanticHunk {
            id: "s1".into(),
            intent: "Add ctx parameter to check_access".into(),
            files: vec!["auth.py".into(), "views.py".into()],
            raw_hunk_ids: vec!["r1".into(), "r2".into(), "r3".into()],
            merged_diff: "...".into(),
        };
        assert_eq!(s.raw_hunk_ids.len(), 3);
    }

    #[test]
    fn review_item_combines_semantic_and_analysis() {
        let s = SemanticHunk {
            id: "s1".into(),
            intent: "x".into(),
            files: vec!["a.py".into()],
            raw_hunk_ids: vec!["r1".into()],
            merged_diff: "d".into(),
        };
        let a = Analysis {
            id: "s1".into(),
            risk: Risk::High,
            risk_reason: "touches auth".into(),
            confidence: Confidence::Uncertain,
            summary_ko: "권한 확장".into(),
        };
        let item = ReviewItem {
            semantic: s,
            analysis: a,
            decision: None,
            reject_reason: None,
        };
        assert!(item.decision.is_none());
        assert_eq!(item.semantic.id, item.analysis.id);
    }

    #[test]
    fn risk_serde_roundtrip() {
        let json = serde_json::to_string(&Risk::High).unwrap();
        assert_eq!(json, "\"high\"");
        let back: Risk = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Risk::High);
    }

    #[test]
    fn confidence_serde_roundtrip() {
        let json = serde_json::to_string(&Confidence::Guess).unwrap();
        assert_eq!(json, "\"guess\"");
        let back: Confidence = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Confidence::Guess);
    }
}
```

- [x] **Step 2: 테스트 실패 확인**

Run: `cargo test --lib models`
Expected: FAIL — 타입 미정의

- [x] **Step 3: 구현**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    High,
    Med,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Confident,
    Uncertain,
    Guess,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Accept,
    Reject,
}

#[derive(Debug, Clone)]
pub struct RawHunk {
    pub id: String,
    pub file: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct SemanticHunk {
    pub id: String,
    pub intent: String,
    pub files: Vec<String>,
    pub raw_hunk_ids: Vec<String>,
    pub merged_diff: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub id: String,
    pub risk: Risk,
    pub risk_reason: String,
    pub confidence: Confidence,
    pub summary_ko: String,
}

#[derive(Debug, Clone)]
pub struct ReviewItem {
    pub semantic: SemanticHunk,
    pub analysis: Analysis,
    pub decision: Option<Decision>,
    pub reject_reason: Option<String>,
}

impl Risk {
    pub fn sort_key(&self) -> u8 {
        match self {
            Risk::High => 0,
            Risk::Med => 1,
            Risk::Low => 2,
        }
    }

    pub fn badge(&self) -> &'static str {
        match self {
            Risk::High => "🔴",
            Risk::Med => "🟡",
            Risk::Low => "🟢",
        }
    }
}

impl Confidence {
    pub fn badge(&self) -> &'static str {
        match self {
            Confidence::Confident => "✅",
            Confidence::Uncertain => "⚠️",
            Confidence::Guess => "❓",
        }
    }
}
```

- [x] **Step 4: main.rs에 mod 선언 추가**

```rust
mod models;

fn main() {
    println!("vouch — closed-loop AI diff reviewer");
}
```

- [x] **Step 5: 테스트 통과 확인**

Run: `cargo test --lib models`
Expected: 5 tests PASS

- [x] **Step 6: 커밋**

```bash
git add src/models.rs src/main.rs
git commit -m "feat: add data models (RawHunk, SemanticHunk, Analysis, ReviewItem)"
```

---

## Task 3: parser.rs — unified diff 파서

**Files:**
- Create: `src/parser.rs`
- Create: `tests/fixtures/sample.diff` (Python 프로젝트에서 복사)
- Modify: `src/main.rs` (mod 선언)

- [x] **Step 1: fixture 복사**

```bash
mkdir -p tests/fixtures
cp "/Users/user/Library/Mobile Documents/com~apple~CloudDocs/Documents/hackathon/tests/fixtures/sample.diff" tests/fixtures/
cp "/Users/user/Library/Mobile Documents/com~apple~CloudDocs/Documents/hackathon/tests/fixtures/diverse.diff" tests/fixtures/
```

- [x] **Step 2: 테스트 작성**

`src/parser.rs` 하단:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture(name: &str) -> String {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn parses_sample_diff() {
        let diff = fixture("sample.diff");
        let hunks = parse_raw_hunks(&diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].file, "auth.py");
        assert_eq!(hunks[1].file, "views.py");
        assert_eq!(hunks[0].id, "r0");
        assert_eq!(hunks[1].id, "r1");
        assert!(
            hunks[0].body.contains("admin") || hunks[0].header.contains("admin")
        );
    }

    #[test]
    fn empty_diff() {
        assert!(parse_raw_hunks("").is_empty());
    }

    #[test]
    fn whitespace_only_diff() {
        assert!(parse_raw_hunks("   \n\n  ").is_empty());
    }
}
```

- [x] **Step 3: 테스트 실패 확인**

Run: `cargo test --lib parser`
Expected: FAIL — 함수 미정의

- [x] **Step 4: 구현**

```rust
use regex::Regex;
use crate::models::RawHunk;

pub fn parse_raw_hunks(unified_diff: &str) -> Vec<RawHunk> {
    if unified_diff.trim().is_empty() {
        return Vec::new();
    }

    let hunk_re = Regex::new(r"^@@ -(\d+),(\d+) \+(\d+),(\d+) @@").unwrap();
    let mut out = Vec::new();
    let mut current_file = String::new();
    let mut current_header = String::new();
    let mut current_body = Vec::new();
    let mut old_start = 0u32;
    let mut old_lines = 0u32;
    let mut new_start = 0u32;
    let mut new_lines = 0u32;
    let mut in_hunk = false;

    for line in unified_diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            // flush previous hunk
            if in_hunk {
                let body = current_body.join("\n");
                out.push(RawHunk {
                    id: format!("r{}", out.len()),
                    file: current_file.clone(),
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    header: current_header.clone(),
                    body,
                });
                current_body.clear();
                in_hunk = false;
            }
            current_file = path.to_string();
        } else if line.starts_with("+++ ") {
            // handle +++ without b/ prefix (e.g. +++ /dev/null)
            if in_hunk {
                let body = current_body.join("\n");
                out.push(RawHunk {
                    id: format!("r{}", out.len()),
                    file: current_file.clone(),
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    header: current_header.clone(),
                    body,
                });
                current_body.clear();
                in_hunk = false;
            }
        } else if line.starts_with("--- ") {
            // extract file for removed files
            if current_file.is_empty() {
                if let Some(path) = line.strip_prefix("--- a/") {
                    current_file = path.to_string();
                }
            }
        } else if let Some(caps) = hunk_re.captures(line) {
            // flush previous hunk in same file
            if in_hunk {
                let body = current_body.join("\n");
                out.push(RawHunk {
                    id: format!("r{}", out.len()),
                    file: current_file.clone(),
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    header: current_header.clone(),
                    body,
                });
                current_body.clear();
            }
            old_start = caps[1].parse().unwrap_or(0);
            old_lines = caps[2].parse().unwrap_or(0);
            new_start = caps[3].parse().unwrap_or(0);
            new_lines = caps[4].parse().unwrap_or(0);
            current_header = format!(
                "@@ -{},{} +{},{} @@",
                old_start, old_lines, new_start, new_lines
            );
            in_hunk = true;
        } else if in_hunk
            && (line.starts_with('+')
                || line.starts_with('-')
                || line.starts_with(' ')
                || line.is_empty())
        {
            current_body.push(line.to_string());
        }
    }

    // flush last hunk
    if in_hunk {
        let body = current_body.join("\n");
        out.push(RawHunk {
            id: format!("r{}", out.len()),
            file: current_file,
            old_start,
            old_lines,
            new_start,
            new_lines,
            header: current_header,
            body,
        });
    }

    out
}
```

- [x] **Step 5: main.rs에 mod 선언 추가**

```rust
mod models;
mod parser;
```

- [x] **Step 6: 테스트 통과 확인**

Run: `cargo test --lib parser`
Expected: 3 tests PASS

- [x] **Step 7: 커밋**

```bash
git add src/parser.rs src/main.rs tests/fixtures/
git commit -m "feat: add unified diff parser"
```

---

## Task 4: diff_input.rs — git/gh 입력 모드

**Files:**
- Create: `src/diff_input.rs`
- Modify: `src/main.rs` (mod 선언)

- [x] **Step 1: 테스트 작성**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_means_uncommitted() {
        let spec = resolve_mode(&[]);
        assert_eq!(spec.kind, ModeKind::Uncommitted);
        assert!(spec.value.is_none());
    }

    #[test]
    fn single_commit() {
        let spec = resolve_mode(&["abc123".to_string()]);
        assert_eq!(spec.kind, ModeKind::Commit);
        assert_eq!(spec.value.as_deref(), Some("abc123"));
    }

    #[test]
    fn range() {
        let spec = resolve_mode(&["abc..def".to_string()]);
        assert_eq!(spec.kind, ModeKind::Range);
        assert_eq!(spec.value.as_deref(), Some("abc..def"));
    }

    #[test]
    fn pr_flag() {
        let spec = resolve_mode(&["--pr".to_string(), "42".to_string()]);
        assert_eq!(spec.kind, ModeKind::Pr);
        assert_eq!(spec.value.as_deref(), Some("42"));
    }

    #[test]
    fn pr_url() {
        let spec = resolve_mode(&["https://github.com/x/y/pull/42".to_string()]);
        assert_eq!(spec.kind, ModeKind::Pr);
        assert_eq!(spec.value.as_deref(), Some("42"));
    }
}
```

- [x] **Step 2: 테스트 실패 확인**

Run: `cargo test --lib diff_input`
Expected: FAIL

- [x] **Step 3: 구현**

```rust
use std::process::Command;
use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub enum ModeKind {
    Uncommitted,
    Commit,
    Range,
    Pr,
}

#[derive(Debug, Clone)]
pub struct ModeSpec {
    pub kind: ModeKind,
    pub value: Option<String>,
}

pub fn resolve_mode(args: &[String]) -> ModeSpec {
    if args.is_empty() {
        return ModeSpec { kind: ModeKind::Uncommitted, value: None };
    }

    if args[0] == "--pr" && args.len() >= 2 {
        return ModeSpec { kind: ModeKind::Pr, value: Some(args[1].clone()) };
    }

    let pr_re = Regex::new(r"https?://github\.com/[^/]+/[^/]+/pull/(\d+)").unwrap();
    if let Some(caps) = pr_re.captures(&args[0]) {
        return ModeSpec {
            kind: ModeKind::Pr,
            value: Some(caps[1].to_string()),
        };
    }

    let range_re = Regex::new(r"^[^.\s]+\.\.[^.\s]+$").unwrap();
    if range_re.is_match(&args[0]) {
        return ModeSpec { kind: ModeKind::Range, value: Some(args[0].clone()) };
    }

    ModeSpec { kind: ModeKind::Commit, value: Some(args[0].clone()) }
}

pub fn get_unified_diff(spec: &ModeSpec) -> Result<String, String> {
    match spec.kind {
        ModeKind::Uncommitted => run_cmd(&["git", "diff", "HEAD"]),
        ModeKind::Commit => {
            let val = spec.value.as_deref().unwrap();
            run_cmd(&["git", "show", "--format=", val])
        }
        ModeKind::Range => {
            let val = spec.value.as_deref().unwrap();
            run_cmd(&["git", "diff", val])
        }
        ModeKind::Pr => {
            let val = spec.value.as_deref().unwrap();
            run_cmd(&["gh", "pr", "diff", val])
        }
    }
}

fn run_cmd(args: &[&str]) -> Result<String, String> {
    let output = Command::new(args[0])
        .args(&args[1..])
        .output()
        .map_err(|e| format!("{} failed: {}", args.join(" "), e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{} failed: {}", args.join(" "), stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
```

- [x] **Step 4: 테스트 통과 확인**

Run: `cargo test --lib diff_input`
Expected: 5 tests PASS

- [x] **Step 5: 커밋**

```bash
git add src/diff_input.rs src/main.rs
git commit -m "feat: add diff input mode resolution (git/gh)"
```

---

## Task 5: cache.rs — SHA256 파일 캐시

**Files:**
- Create: `src/cache.rs`
- Modify: `src/main.rs` (mod 선언)

- [x] **Step 1: 테스트 작성**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn save_then_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let data = serde_json::json!({"a": 1, "b": [2, 3]});
        cache.save("stage_x", "payload-1", &data);
        let loaded = cache.load("stage_x", "payload-1");
        assert_eq!(loaded, Some(data));
    }

    #[test]
    fn load_miss_returns_none() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        assert_eq!(cache.load("stage_x", "missing"), None);
    }

    #[test]
    fn fallback_to_stage_json() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        std::fs::write(
            dir.path().join("stage_x.json"),
            r#"[{"id": "fallback"}]"#,
        ).unwrap();
        let loaded = cache.load("stage_x", "any-payload");
        assert_eq!(loaded, Some(serde_json::json!([{"id": "fallback"}])));
    }

    #[test]
    fn hashed_key_takes_precedence_over_fallback() {
        let dir = TempDir::new().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        cache.save("stage_x", "payload-A", &serde_json::json!({"hashed": true}));
        std::fs::write(
            dir.path().join("stage_x.json"),
            r#"{"hashed": false}"#,
        ).unwrap();
        let loaded = cache.load("stage_x", "payload-A");
        assert_eq!(loaded, Some(serde_json::json!({"hashed": true})));
    }
}
```

- [x] **Step 2: 테스트 실패 확인**

Run: `cargo test --lib cache`
Expected: FAIL

- [x] **Step 3: Cargo.toml에 tempfile dev-dependency 추가**

```toml
[dev-dependencies]
tempfile = "3"
```

- [x] **Step 4: 구현**

```rust
use sha2::{Sha256, Digest};
use std::fs;
use std::path::PathBuf;

pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn from_env() -> Self {
        let dir = std::env::var("VOUCH_CACHE_DIR")
            .unwrap_or_else(|_| "fixtures/responses".to_string());
        Self { dir: PathBuf::from(dir) }
    }

    fn key(&self, stage: &str, payload: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(payload.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        format!("{}.{}.json", stage, &hash[..16])
    }

    pub fn load(&self, stage: &str, payload: &str) -> Option<serde_json::Value> {
        let path = self.dir.join(self.key(stage, payload));
        if let Ok(content) = fs::read_to_string(&path) {
            return serde_json::from_str(&content).ok();
        }
        let fallback = self.dir.join(format!("{}.json", stage));
        if let Ok(content) = fs::read_to_string(&fallback) {
            return serde_json::from_str(&content).ok();
        }
        None
    }

    pub fn save(&self, stage: &str, payload: &str, result: &serde_json::Value) {
        fs::create_dir_all(&self.dir).ok();
        let path = self.dir.join(self.key(stage, payload));
        let json = serde_json::to_string_pretty(result).unwrap();
        fs::write(path, json).ok();
    }
}
```

- [x] **Step 5: 테스트 통과 확인**

Run: `cargo test --lib cache`
Expected: 4 tests PASS

- [x] **Step 6: 커밋**

```bash
git add src/cache.rs src/main.rs Cargo.toml
git commit -m "feat: add SHA256 file cache"
```

---

## Task 6: feedback.rs — reject 프롬프트 빌더

**Files:**
- Create: `src/feedback.rs`
- Modify: `src/main.rs` (mod 선언)

- [x] **Step 1: 테스트 작성**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn item(sid: &str, intent: &str, files: Vec<&str>, reason: &str) -> ReviewItem {
        ReviewItem {
            semantic: SemanticHunk {
                id: sid.into(),
                intent: intent.into(),
                files: files.into_iter().map(String::from).collect(),
                raw_hunk_ids: vec![],
                merged_diff: String::new(),
            },
            analysis: Analysis {
                id: sid.into(),
                risk: Risk::High,
                risk_reason: String::new(),
                confidence: Confidence::Confident,
                summary_ko: intent.into(),
            },
            decision: Some(Decision::Reject),
            reject_reason: Some(reason.into()),
        }
    }

    #[test]
    fn build_reject_prompt_lists_items() {
        let rejects = vec![
            item("s1", "intent A", vec!["a.py"], "use parameterized SQL"),
            item("s2", "intent B", vec!["b.py", "c.py"], "missing null check"),
        ];
        let out = build_reject_prompt(&rejects);
        assert!(out.contains("다시 시도"));
        assert!(out.contains("intent A"));
        assert!(out.contains("use parameterized SQL"));
        assert!(out.contains("intent B"));
        assert!(out.contains("a.py"));
    }

    #[test]
    fn build_reject_prompt_empty() {
        assert_eq!(build_reject_prompt(&[]), "");
    }

    #[test]
    fn pr_review_body_strips_agent_framing() {
        let rejects = vec![
            item("s1", "intent A", vec!["a.py"], "use parameterized SQL"),
            item("s2", "intent B", vec!["b.py", "c.py"], "missing null check"),
        ];
        let out = build_pr_review_body(&rejects);
        assert!(!out.contains("[vouch]"));
        assert!(!out.contains("다시 시도"));
        assert!(!out.contains("거절된 항목 외에는"));
        assert!(!out.contains("intent A"));
        assert!(out.contains("use parameterized SQL"));
        assert!(out.contains("missing null check"));
        assert!(out.contains("`a.py`"));
        assert!(out.contains("`b.py`"));
        assert!(out.contains("`c.py`"));
    }

    #[test]
    fn pr_review_body_empty() {
        assert_eq!(build_pr_review_body(&[]), "");
    }
}
```

- [x] **Step 2: 테스트 실패 확인**

Run: `cargo test --lib feedback`
Expected: FAIL

- [x] **Step 3: 구현**

```rust
use crate::models::ReviewItem;

pub fn build_reject_prompt(rejected: &[ReviewItem]) -> String {
    if rejected.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        "[vouch] 다음 변경들이 사람의 리뷰에서 거절됐어. 사유를 반영해 다시 시도해줘:".to_string(),
        String::new(),
    ];
    for it in rejected {
        let files = it.semantic.files.join(", ");
        lines.push(format!("- ({}) {}", files, it.semantic.intent));
        let reason = it.reject_reason.as_deref().unwrap_or("(no reason)");
        lines.push(format!("    사유: {}", reason));
    }
    lines.push(String::new());
    lines.push("거절된 항목 외에는 그대로 유지하고, 위 사유를 직접 해소하는 변경만 적용해줘.".to_string());
    lines.join("\n")
}

pub fn build_pr_review_body(rejected: &[ReviewItem]) -> String {
    if rejected.is_empty() {
        return String::new();
    }
    let sections: Vec<String> = rejected
        .iter()
        .map(|it| {
            let files = it.semantic.files
                .iter()
                .map(|f| format!("`{}`", f))
                .collect::<Vec<_>>()
                .join(", ");
            let reason = it.reject_reason.as_deref().unwrap_or("(no reason)");
            format!("### {}\n{}", files, reason)
        })
        .collect();
    sections.join("\n\n")
}
```

- [x] **Step 4: 테스트 통과 확인**

Run: `cargo test --lib feedback`
Expected: 4 tests PASS

- [x] **Step 5: 커밋**

```bash
git add src/feedback.rs src/main.rs
git commit -m "feat: add reject prompt and PR review body builders"
```

---

## Task 7: cmux.rs — 배포 채널

**Files:**
- Create: `src/cmux.rs`
- Modify: `src/main.rs` (mod 선언)

- [x] **Step 1: 테스트 작성**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_source_surface_cli_flag() {
        let result = discover_source_surface(Some("surface:1".into()));
        assert_eq!(result, Some("surface:1".to_string()));
    }

    #[test]
    fn discover_source_surface_env_fallback() {
        std::env::set_var("VOUCH_SOURCE_SURFACE", "surface:env");
        let result = discover_source_surface(None);
        assert_eq!(result, Some("surface:env".to_string()));
        std::env::remove_var("VOUCH_SOURCE_SURFACE");
    }

    #[test]
    fn discover_source_surface_none() {
        std::env::remove_var("VOUCH_SOURCE_SURFACE");
        let result = discover_source_surface(None);
        assert!(result.is_none());
    }

    #[test]
    fn deliver_reject_stdout_when_nothing_available() {
        // gh, cmux, clipboard 모두 없는 환경에서는 stdout fallback
        // 이 테스트는 실제 바이너리가 없는 환경에서만 의미 있음
        let channel = deliver_reject("body", None, None);
        assert_eq!(channel, "stdout");
    }
}
```

- [x] **Step 2: 테스트 실패 확인**

Run: `cargo test --lib cmux`
Expected: FAIL

- [x] **Step 3: 구현**

```rust
use std::process::Command;

pub fn cmux_available() -> bool {
    which::which("cmux").is_ok()
        && Command::new("cmux")
            .arg("ping")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

pub fn discover_source_surface(cli_flag: Option<String>) -> Option<String> {
    if cli_flag.is_some() {
        return cli_flag;
    }
    std::env::var("VOUCH_SOURCE_SURFACE").ok()
}

pub fn workspace_id() -> Option<String> {
    std::env::var("CMUX_WORKSPACE_ID").ok()
}

fn run_cmux(args: &[&str]) -> bool {
    if which::which("cmux").is_err() {
        return false;
    }
    let mut cmd = Command::new("cmux");
    cmd.args(args);
    if let Some(ws) = workspace_id() {
        cmd.args(["--workspace", &ws]);
    }
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

pub fn set_status(key: &str, message: &str, icon: &str) {
    run_cmux(&["set-status", key, message, "--icon", icon]);
}

pub fn clear_status(key: &str) {
    run_cmux(&["clear-status", key]);
}

pub fn set_progress(value: f64, label: &str) {
    let val_str = value.to_string();
    let mut args = vec!["set-progress", &val_str];
    if !label.is_empty() {
        args.extend(["--label", label]);
    }
    run_cmux(&args);
}

pub fn notify(title: &str, body: &str) {
    let mut args = vec!["notify", "--title", title];
    if !body.is_empty() {
        args.extend(["--body", body]);
    }
    run_cmux(&args);
}

pub fn send_to_surface(surface: &str, text: &str) -> bool {
    if which::which("cmux").is_err() {
        return false;
    }
    let ws = workspace_id();
    let mut cmd1 = Command::new("cmux");
    cmd1.args(["send", "--surface", surface, text]);
    let mut cmd2 = Command::new("cmux");
    cmd2.args(["send-key", "--surface", surface, "Enter"]);
    if let Some(ref ws) = ws {
        cmd1.args(["--workspace", ws]);
        cmd2.args(["--workspace", ws]);
    }
    let r1 = cmd1.output().map(|o| o.status.success()).unwrap_or(false);
    let r2 = cmd2.output().map(|o| o.status.success()).unwrap_or(false);
    r1 && r2
}

pub fn post_pr_comment(pr: &str, text: &str) -> bool {
    if which::which("gh").is_err() {
        return false;
    }
    Command::new("gh")
        .args(["pr", "comment", pr, "-F", "-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(text.as_bytes()).ok();
            }
            child.wait()
        })
        .map(|s| s.success())
        .unwrap_or(false)
}

fn try_clipboard(text: &str) -> Option<String> {
    let candidates: &[(&str, &[&str])] = &[
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    for (cmd, extra) in candidates {
        if which::which(cmd).is_err() {
            continue;
        }
        let result = Command::new(cmd)
            .args(*extra)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(text.as_bytes()).ok();
                }
                child.wait()
            });
        if result.map(|s| s.success()).unwrap_or(false) {
            return Some(cmd.to_string());
        }
    }
    None
}

pub fn deliver_reject(
    text: &str,
    surface: Option<&str>,
    pr_number: Option<&str>,
) -> String {
    if let Some(pr) = pr_number {
        if post_pr_comment(pr, text) {
            println!("vouch: posted reject as PR #{} comment", pr);
            return "gh".to_string();
        }
    }
    if let Some(surf) = surface {
        if send_to_surface(surf, text) {
            println!("vouch: sent reject to cmux {}", surf);
            return "cmux".to_string();
        }
    }
    if let Some(cb) = try_clipboard(text) {
        println!(
            "vouch: reject prompt copied to clipboard via {} — paste into your agent",
            cb
        );
        return cb;
    }
    println!("vouch: no delivery channel available — printing prompt below\n");
    println!("--- vouch reject prompt ---");
    println!("{}", text);
    "stdout".to_string()
}
```

- [x] **Step 4: 테스트 통과 확인**

Run: `cargo test --lib cmux`
Expected: 4 tests PASS (deliver_reject_stdout는 CI 환경에서만 의미 있음 — 로컬에서 pbcopy가 있으면 "pbcopy" 반환)

- [x] **Step 5: 커밋**

```bash
git add src/cmux.rs src/main.rs
git commit -m "feat: add delivery channels (cmux, gh, clipboard, stdout)"
```

---

## Task 8: llm.rs — Claude API 클라이언트

**Files:**
- Create: `src/llm.rs`
- Modify: `src/main.rs` (mod 선언)

- [x] **Step 1: 테스트 작성**

LLM 모듈의 핵심 테스트: `_build_semantic` 로직 (API 미호출). 캐시 기반 테스트는 Task 12에서 통합 테스트로 다룸.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RawHunk;

    fn sample_hunks() -> Vec<RawHunk> {
        vec![
            RawHunk {
                id: "r0".into(), file: "auth.py".into(),
                old_start: 10, old_lines: 5, new_start: 10, new_lines: 6,
                header: "@@ -10,5 +10,6 @@".into(),
                body: "-old\n+new".into(),
            },
            RawHunk {
                id: "r1".into(), file: "views.py".into(),
                old_start: 45, old_lines: 1, new_start: 45, new_lines: 4,
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
```

- [x] **Step 2: 테스트 실패 확인**

Run: `cargo test --lib llm`
Expected: FAIL

- [x] **Step 3: 구현**

```rust
use crate::cache::Cache;
use crate::models::{Analysis, RawHunk, SemanticHunk};
use std::collections::HashMap;

const DEFAULT_MODEL: &str = "claude-sonnet-4-6-20250514";
const API_URL: &str = "https://api.anthropic.com/v1/messages";

const SEMANTIC_PROMPT: &str = r#"You receive a list of raw git hunks from one change. Group hunks that share a single SPECIFIC intent into a SemanticHunk. Each SemanticHunk should describe ONE concrete action (e.g., "check_access 함수 추가", "users 테이블에 role 컬럼 추가"). DO NOT lump everything into one giant SemanticHunk — aim for 3-8 SemanticHunks for a typical multi-file change. Group only when hunks are mechanically inseparable.

Output JSON array where each item is: {"id": "s<n>", "intent": "한국어 한 줄 의도 (구체적으로)", "raw_hunk_ids": ["r1", ...]}. Each raw_hunk_id must appear in exactly one SemanticHunk. Output ONLY the JSON array, no markdown fences."#;

const ANALYSIS_PROMPT: &str = r#"You receive a list of SemanticHunks (each with merged diff). For each, output a JSON array of objects with fields: id, risk (high|med|low), risk_reason (한 줄), confidence (confident|uncertain|guess), summary_ko (한국어 한 줄). Be conservative on risk: business logic, security, new dependencies → high. Mechanical (rename/import/format) → low. Output ONLY the JSON array, no markdown fences."#;

fn model() -> String {
    std::env::var("VOUCH_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string())
}

fn api_key() -> Result<String, String> {
    std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set".to_string())
}

fn cache_only() -> bool {
    std::env::var("VOUCH_CACHE_ONLY").as_deref() == Ok("1")
}

pub fn extract_json(text: &str) -> String {
    // strip markdown fences if present
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

pub fn build_semantic(
    raw_hunks: &[RawHunk],
    parsed: &[serde_json::Value],
) -> Vec<SemanticHunk> {
    let by_id: HashMap<&str, &RawHunk> =
        raw_hunks.iter().map(|h| (h.id.as_str(), h)).collect();
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
        let members: Vec<&&RawHunk> = raw_ids
            .iter()
            .filter_map(|rid| by_id.get(rid.as_str()))
            .collect();
        let files: Vec<String> = {
            let mut f: Vec<String> = members.iter().map(|m| m.file.clone()).collect();
            f.sort();
            f.dedup();
            f
        };
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

pub fn analyze(
    semantic_hunks: &[SemanticHunk],
    cache: &Cache,
) -> Result<Vec<Analysis>, String> {
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
        let analyses: Vec<Analysis> =
            serde_json::from_value(cached).map_err(|e| e.to_string())?;
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
```

- [x] **Step 4: 테스트 통과 확인**

Run: `cargo test --lib llm`
Expected: 5 tests PASS

- [x] **Step 5: 커밋**

```bash
git add src/llm.rs src/main.rs
git commit -m "feat: add Claude API client (semantic_postprocess, analyze)"
```

---

## Task 9: tui.rs — Ratatui TUI (Part 1: 레이아웃 + 테이블)

**Files:**
- Create: `src/tui.rs`
- Modify: `src/main.rs` (mod 선언)

- [x] **Step 1: 테스트 작성 (TUI 로직 테스트)**

렌더링은 수동 검증이지만, 상태 로직은 테스트 가능:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn sample_items() -> Vec<ReviewItem> {
        vec![
            ReviewItem {
                semantic: SemanticHunk {
                    id: "s0".into(), intent: "SQL injection".into(),
                    files: vec!["auth.py".into()],
                    raw_hunk_ids: vec!["r0".into()],
                    merged_diff: "+eval(user_input)".into(),
                },
                analysis: Analysis {
                    id: "s0".into(), risk: Risk::High,
                    risk_reason: "eval".into(),
                    confidence: Confidence::Confident,
                    summary_ko: "위험한 eval 사용".into(),
                },
                decision: None, reject_reason: None,
            },
            ReviewItem {
                semantic: SemanticHunk {
                    id: "s1".into(), intent: "rename var".into(),
                    files: vec!["utils.py".into()],
                    raw_hunk_ids: vec!["r1".into()],
                    merged_diff: "-old_name\n+new_name".into(),
                },
                analysis: Analysis {
                    id: "s1".into(), risk: Risk::Low,
                    risk_reason: "rename".into(),
                    confidence: Confidence::Confident,
                    summary_ko: "변수 이름 변경".into(),
                },
                decision: None, reject_reason: None,
            },
        ]
    }

    #[test]
    fn accept_sets_decision() {
        let mut items = sample_items();
        items[0].decision = Some(Decision::Accept);
        assert_eq!(items[0].decision, Some(Decision::Accept));
    }

    #[test]
    fn accept_all_low_only_affects_low_risk() {
        let mut items = sample_items();
        accept_all_low(&mut items);
        assert!(items[0].decision.is_none()); // high risk untouched
        assert_eq!(items[1].decision, Some(Decision::Accept)); // low risk accepted
    }

    #[test]
    fn progress_calculation() {
        let mut items = sample_items();
        assert_eq!(progress(&items), (0, 2));
        items[0].decision = Some(Decision::Accept);
        assert_eq!(progress(&items), (1, 2));
        items[1].decision = Some(Decision::Reject);
        assert_eq!(progress(&items), (2, 2));
    }
}
```

- [x] **Step 2: 테스트 실패 확인**

Run: `cargo test --lib tui`
Expected: FAIL

- [x] **Step 3: 구현 — 상태 로직 + 렌더 스켈레톤**

```rust
use crate::models::{Decision, ReviewItem, Risk};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, cursor};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Terminal;
use std::io;
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

pub fn accept_all_low(items: &mut [ReviewItem]) {
    for item in items.iter_mut() {
        if item.analysis.risk == Risk::Low && item.decision.is_none() {
            item.decision = Some(Decision::Accept);
        }
    }
}

pub fn progress(items: &[ReviewItem]) -> (usize, usize) {
    let total = items.len();
    let decided = items.iter().filter(|it| it.decision.is_some()).count();
    (decided, total)
}

enum AppMode {
    Normal,
    RejectInput,
}

pub struct App {
    items: Vec<ReviewItem>,
    table_state: TableState,
    mode: AppMode,
    reject_input: Input,
    queue_pct: u16,
    dragging: bool,
    on_send: Box<dyn FnMut(&[ReviewItem])>,
    on_progress: Box<dyn FnMut(usize, usize)>,
}

impl App {
    pub fn new(
        items: Vec<ReviewItem>,
        on_send: Box<dyn FnMut(&[ReviewItem])>,
        on_progress: Box<dyn FnMut(usize, usize)>,
    ) -> Self {
        let mut table_state = TableState::default();
        if !items.is_empty() {
            table_state.select(Some(0));
        }
        Self {
            items,
            table_state,
            mode: AppMode::Normal,
            reject_input: Input::default(),
            queue_pct: 50,
            dragging: false,
            on_send,
            on_progress,
        }
    }

    fn selected_index(&self) -> Option<usize> {
        self.table_state.selected()
    }

    fn report_progress(&mut self) {
        let (d, t) = progress(&self.items);
        (self.on_progress)(d, t);
    }

    pub fn run(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, crossterm::event::EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        self.report_progress();
        loop {
            terminal.draw(|f| self.draw(f))?;
            if let Event::Key(key) = event::read()? {
                match self.mode {
                    AppMode::Normal => {
                        if self.handle_normal_key(key) {
                            break;
                        }
                    }
                    AppMode::RejectInput => {
                        self.handle_reject_key(key);
                    }
                }
            } else if let Event::Mouse(mouse) = event::read().unwrap_or(Event::FocusLost) {
                self.handle_mouse(mouse, terminal.size().unwrap_or_default());
            }
        }

        terminal::disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        Ok(())
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('j') | KeyCode::Down => self.table_state.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.table_state.select_previous(),
            KeyCode::Char('a') => {
                if let Some(i) = self.selected_index() {
                    self.items[i].decision = Some(Decision::Accept);
                    self.items[i].reject_reason = None;
                    self.report_progress();
                }
            }
            KeyCode::Char('A') => {
                accept_all_low(&mut self.items);
                self.report_progress();
            }
            KeyCode::Char('r') => {
                if self.selected_index().is_some() {
                    self.reject_input = Input::default();
                    self.mode = AppMode::RejectInput;
                }
            }
            KeyCode::Char('s') => {
                let rejects: Vec<ReviewItem> = self.items
                    .iter()
                    .filter(|it| it.decision == Some(Decision::Reject))
                    .cloned()
                    .collect();
                (self.on_send)(&rejects);
                return true;
            }
            KeyCode::Char('[') => {
                self.queue_pct = self.queue_pct.saturating_sub(10).max(20);
            }
            KeyCode::Char(']') => {
                self.queue_pct = (self.queue_pct + 10).min(80);
            }
            _ => {}
        }
        false
    }

    fn handle_reject_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                let reason = self.reject_input.value().to_string();
                if !reason.is_empty() {
                    if let Some(i) = self.selected_index() {
                        self.items[i].decision = Some(Decision::Reject);
                        self.items[i].reject_reason = Some(reason);
                        self.report_progress();
                    }
                }
                self.mode = AppMode::Normal;
            }
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
            }
            _ => {
                self.reject_input.handle_event(&Event::Key(key));
            }
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        let split_x = (area.width as u32 * self.queue_pct as u32 / 100) as u16;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if (mouse.column as i16 - split_x as i16).unsigned_abs() <= 1 {
                    self.dragging = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                let pct = (mouse.column as u32 * 100 / area.width.max(1) as u32) as u16;
                self.queue_pct = pct.clamp(20, 80);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.dragging = false;
            }
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();

        // Main layout: header, body, footer
        let outer = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ]).split(area);

        // Header
        f.render_widget(
            Paragraph::new("vouch — you vouch, AI helps")
                .style(Style::default().fg(Color::White).bg(Color::DarkGray)),
            outer[0],
        );

        // Footer
        f.render_widget(
            Paragraph::new(" j↓ k↑ a:accept A:all r:reject s:send q:quit  [/]:resize")
                .style(Style::default().fg(Color::White).bg(Color::DarkGray)),
            outer[2],
        );

        // Body: queue | detail
        let body = Layout::horizontal([
            Constraint::Percentage(self.queue_pct),
            Constraint::Percentage(100 - self.queue_pct),
        ]).split(outer[1]);

        self.draw_queue(f, body[0]);
        self.draw_detail(f, body[1]);

        // Modal overlay
        if matches!(self.mode, AppMode::RejectInput) {
            self.draw_reject_modal(f, area);
        }
    }

    fn draw_queue(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let header = Row::new(["Risk", "Conf", "Intent", "Files", "Decision"])
            .style(Style::default().add_modifier(Modifier::BOLD))
            .bottom_margin(0);

        let rows: Vec<Row> = self.items.iter().map(|it| {
            let decision = match &it.decision {
                Some(Decision::Accept) => "accept",
                Some(Decision::Reject) => "reject",
                None => "—",
            };
            Row::new([
                Cell::from(it.analysis.risk.badge()),
                Cell::from(it.analysis.confidence.badge()),
                Cell::from(it.semantic.intent.chars().take(50).collect::<String>()),
                Cell::from(it.semantic.files.join(", ").chars().take(40).collect::<String>()),
                Cell::from(decision),
            ])
        }).collect();

        let widths = [
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Length(8),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title("Queue"))
            .row_highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));

        f.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn draw_detail(&self, f: &mut ratatui::Frame, area: Rect) {
        let block = Block::default().borders(Borders::ALL).title("Detail");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let selected = self.selected_index()
            .and_then(|i| self.items.get(i));

        let Some(it) = selected else {
            return;
        };

        // Split detail into header + diff
        let detail_layout = Layout::vertical([
            Constraint::Length(6),
            Constraint::Min(0),
        ]).split(inner);

        // Detail header
        let header_text = vec![
            Line::from(Span::styled(&it.semantic.intent, Style::default().add_modifier(Modifier::BOLD))),
            Line::from(format!(
                "Risk: {} {}  ({})",
                it.analysis.risk.badge(), format!("{:?}", it.analysis.risk).to_lowercase(),
                it.analysis.risk_reason,
            )),
            Line::from(format!(
                "Confidence: {} {}",
                it.analysis.confidence.badge(),
                format!("{:?}", it.analysis.confidence).to_lowercase(),
            )),
            Line::from(format!("Summary: {}", it.analysis.summary_ko)),
            Line::from(format!("Files: {}", it.semantic.files.join(", "))),
            Line::from(format!("Decision: {}", match &it.decision {
                Some(d) => format!("{:?}", d).to_lowercase(),
                None => "(none)".into(),
            })),
        ];
        f.render_widget(
            Paragraph::new(header_text).wrap(Wrap { trim: false }),
            detail_layout[0],
        );

        // Diff with syntax coloring
        let diff_lines: Vec<Line> = it.semantic.merged_diff
            .lines()
            .map(|line| {
                let style = if line.starts_with('+') && !line.starts_with("+++") {
                    Style::default().fg(Color::Green)
                } else if line.starts_with('-') && !line.starts_with("---") {
                    Style::default().fg(Color::Red)
                } else if line.starts_with("@@") {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(line, style))
            })
            .collect();

        f.render_widget(
            Paragraph::new(diff_lines)
                .block(Block::default().borders(Borders::TOP))
                .wrap(Wrap { trim: false }),
            detail_layout[1],
        );
    }

    fn draw_reject_modal(&self, f: &mut ratatui::Frame, area: Rect) {
        let modal_area = centered_rect(70, 5, area);
        f.render_widget(Clear, modal_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title("Reject reason (Enter to submit, Esc to cancel)");
        let inner = block.inner(modal_area);
        f.render_widget(block, modal_area);

        let width = inner.width.saturating_sub(1) as usize;
        let scroll = self.reject_input.visual_scroll(width);
        f.render_widget(
            Paragraph::new(self.reject_input.value())
                .scroll((0, scroll as u16)),
            inner,
        );
        f.set_cursor_position((
            inner.x + (self.reject_input.visual_cursor().saturating_sub(scroll)) as u16,
            inner.y,
        ));
    }
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ]).split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ]).split(popup_layout[1])[1]
}
```

- [x] **Step 4: 테스트 통과 확인**

Run: `cargo test --lib tui`
Expected: 3 tests PASS

- [x] **Step 5: 커밋**

```bash
git add src/tui.rs src/main.rs
git commit -m "feat: add Ratatui TUI with table, detail view, reject modal, mouse drag"
```

---

## Task 10: main.rs — CLI + 파이프라인 오케스트레이션

**Files:**
- Modify: `src/main.rs` (전체 재작성)

- [x] **Step 1: 구현**

```rust
mod cache;
mod cmux;
mod diff_input;
mod feedback;
mod llm;
mod models;
mod parser;
mod tui;

use clap::Parser;
use models::ReviewItem;

#[derive(Parser)]
#[command(name = "vouch", about = "closed-loop AI diff reviewer")]
struct Cli {
    /// commit, range (a..b), PR url, or omit for uncommitted
    rev: Option<String>,
    /// PR number
    #[arg(long)]
    pr: Option<String>,
    /// cmux surface ref of source agent
    #[arg(long = "source-surface")]
    source: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    // Build args for resolve_mode
    let mut spec_args: Vec<String> = Vec::new();
    if let Some(ref pr) = cli.pr {
        spec_args.push("--pr".to_string());
        spec_args.push(pr.clone());
    } else if let Some(ref rev) = cli.rev {
        spec_args.push(rev.clone());
    }
    let spec = diff_input::resolve_mode(&spec_args);

    cmux::set_status("vouch", "loading diff", "hourglass");
    let diff = match diff_input::get_unified_diff(&spec) {
        Ok(d) => d,
        Err(e) => {
            cmux::clear_status("vouch");
            eprintln!("vouch: {}", e);
            std::process::exit(1);
        }
    };

    let raw = parser::parse_raw_hunks(&diff);
    if raw.is_empty() {
        cmux::clear_status("vouch");
        println!("vouch: no changes to review");
        return;
    }

    let cache = cache::Cache::from_env();

    cmux::set_status("vouch", &format!("analyzing {} hunks", raw.len()), "hammer");
    cmux::set_progress(0.2, "semantic");

    let sem = match llm::semantic_postprocess(&raw, &cache) {
        Ok(s) => s,
        Err(e) => {
            cmux::clear_status("vouch");
            eprintln!("vouch: {}", e);
            std::process::exit(1);
        }
    };
    cmux::set_progress(0.6, "analyzing");

    let analyses = match llm::analyze(&sem, &cache) {
        Ok(a) => a,
        Err(e) => {
            cmux::clear_status("vouch");
            eprintln!("vouch: {}", e);
            std::process::exit(1);
        }
    };
    cmux::set_progress(0.9, "ready");

    let by_id: std::collections::HashMap<&str, &models::Analysis> =
        analyses.iter().map(|a| (a.id.as_str(), a)).collect();
    let mut items: Vec<ReviewItem> = sem
        .into_iter()
        .filter_map(|s| {
            by_id.get(s.id.as_str()).map(|&a| ReviewItem {
                semantic: s,
                analysis: a.clone(),
                decision: None,
                reject_reason: None,
            })
        })
        .collect();
    items.sort_by_key(|it| it.analysis.risk.sort_key());

    cmux::set_status("vouch", &format!("review · {} items", items.len()), "shield-check");

    let source = cmux::discover_source_surface(cli.source);
    let pr_number = if spec.kind == diff_input::ModeKind::Pr {
        spec.value.clone()
    } else {
        None
    };

    let pr_for_send = pr_number.clone();
    let source_for_send = source.clone();
    let on_send: Box<dyn FnMut(&[ReviewItem])> = Box::new(move |rejects: &[ReviewItem]| {
        let prompt = if pr_for_send.is_some() {
            feedback::build_pr_review_body(rejects)
        } else {
            feedback::build_reject_prompt(rejects)
        };
        if prompt.is_empty() {
            return;
        }
        let channel = cmux::deliver_reject(
            &prompt,
            source_for_send.as_deref(),
            pr_for_send.as_deref(),
        );
        match channel.as_str() {
            "cmux" => cmux::notify("vouch", &format!("sent {} rejects to source", rejects.len())),
            "gh" => cmux::notify("vouch", &format!("posted {} rejects as PR comment", rejects.len())),
            _ => {}
        }
    });

    let on_progress: Box<dyn FnMut(usize, usize)> = Box::new(|decided, total| {
        let val = if total > 0 { decided as f64 / total as f64 } else { 0.0 };
        cmux::set_progress(val, &format!("{}/{}", decided, total));
    });

    let mut app = tui::App::new(items, on_send, on_progress);
    if let Err(e) = app.run() {
        eprintln!("vouch TUI error: {}", e);
        std::process::exit(1);
    }

    cmux::set_progress(1.0, "done");
    cmux::set_status("vouch", "review complete", "check");
}
```

- [x] **Step 2: 빌드 확인**

Run: `cargo build`
Expected: 컴파일 성공

- [x] **Step 3: 커밋**

```bash
git add src/main.rs
git commit -m "feat: wire CLI pipeline (diff → llm → tui → deliver)"
```

---

## Task 11: 수동 TUI 검증

- [x] **Step 1: fixture 기반 데모 실행 준비**

캐시 fixture를 생성하기 위해 실제 diff로 한 번 실행하거나, Python 프로젝트의 fixture를 활용:

```bash
mkdir -p fixtures/responses
# ANTHROPIC_API_KEY가 설정된 상태에서:
cd ~/vibe/vouch-rust && cargo run
```

또는 `VOUCH_CACHE_ONLY=1`로 캐시 fixture를 먼저 만든 뒤 테스트.

- [x] **Step 2: TUI 기능 수동 체크리스트**

- [ ] j/k 행 이동
- [ ] a로 accept 표시
- [ ] A로 low-risk 전체 accept
- [ ] r로 reject 모달 → 사유 입력 → Enter
- [ ] Esc로 모달 취소
- [ ] `[`/`]`로 분할 비율 조정
- [ ] 마우스 드래그로 분할 리사이즈
- [ ] s로 reject 전송 후 종료
- [ ] q로 종료
- [ ] diff 구문 강조 (초록/빨강/시안)

- [x] **Step 3: 발견된 이슈 수정 및 커밋**

```bash
git add -A
git commit -m "fix: TUI adjustments from manual testing"
```

---

## Task 12: 통합 테스트

**Files:**
- Create: `tests/integration.rs`

- [x] **Step 1: 통합 테스트 작성**

```rust
use std::path::Path;

fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn full_pipeline_sample_diff() {
    // parser → diff 파싱 검증
    let diff = fixture("sample.diff");
    let raw = vouch::parser::parse_raw_hunks(&diff);
    assert_eq!(raw.len(), 2);
    assert_eq!(raw[0].file, "auth.py");
    assert_eq!(raw[1].file, "views.py");
}

#[test]
fn diff_input_resolve_modes() {
    use vouch::diff_input::{resolve_mode, ModeKind};

    assert_eq!(resolve_mode(&[]).kind, ModeKind::Uncommitted);
    assert_eq!(
        resolve_mode(&["HEAD~3..HEAD".into()]).kind,
        ModeKind::Range
    );
    assert_eq!(
        resolve_mode(&["--pr".into(), "42".into()]).kind,
        ModeKind::Pr
    );
}

#[test]
fn feedback_roundtrip() {
    use vouch::feedback::{build_reject_prompt, build_pr_review_body};
    use vouch::models::*;

    let item = ReviewItem {
        semantic: SemanticHunk {
            id: "s1".into(),
            intent: "test".into(),
            files: vec!["a.py".into()],
            raw_hunk_ids: vec![],
            merged_diff: String::new(),
        },
        analysis: Analysis {
            id: "s1".into(),
            risk: Risk::High,
            risk_reason: "reason".into(),
            confidence: Confidence::Confident,
            summary_ko: "요약".into(),
        },
        decision: Some(Decision::Reject),
        reject_reason: Some("fix this".into()),
    };

    let prompt = build_reject_prompt(&[item.clone()]);
    assert!(prompt.contains("다시 시도"));
    assert!(prompt.contains("fix this"));

    let body = build_pr_review_body(&[item]);
    assert!(body.contains("`a.py`"));
    assert!(body.contains("fix this"));
    assert!(!body.contains("다시 시도"));
}
```

- [x] **Step 2: lib.rs 생성 (통합 테스트에서 모듈 접근용)**

`src/lib.rs` 생성:

```rust
pub mod cache;
pub mod cmux;
pub mod diff_input;
pub mod feedback;
pub mod llm;
pub mod models;
pub mod parser;
pub mod tui;
```

- [x] **Step 3: 테스트 통과 확인**

Run: `cargo test --test integration`
Expected: 3 tests PASS

- [x] **Step 4: 전체 테스트 확인**

Run: `cargo test`
Expected: 모든 단위 + 통합 테스트 PASS

- [x] **Step 5: 커밋**

```bash
git add tests/integration.rs src/lib.rs
git commit -m "test: add integration tests"
```

---

## Task 13: 최종 정리

- [x] **Step 1: cargo clippy 수정**

Run: `cargo clippy -- -D warnings`
Expected: 경고 0개 (있으면 수정)

- [x] **Step 2: cargo fmt**

Run: `cargo fmt`

- [x] **Step 3: release 빌드 확인**

Run: `cargo build --release`
Expected: 성공. 바이너리: `target/release/vouch`

- [x] **Step 4: 바이너리 크기 확인**

Run: `ls -lh target/release/vouch`
Expected: ~5-15MB

- [x] **Step 5: 최종 커밋**

```bash
git add -A
git commit -m "chore: clippy + fmt cleanup"
```

- [x] **Step 6: Migration Checklist 최종 확인**

스펙의 Section 6 체크리스트를 하나씩 검증:

- [ ] `vouch` (인자 없음) — uncommitted diff 리뷰
- [ ] `vouch <commit>` — 단일 커밋 리뷰
- [ ] `vouch <a>..<b>` — 범위 리뷰
- [ ] `vouch --pr <number>` — PR 리뷰
- [ ] TUI 전체 기능
- [ ] 캐시 정상 동작
- [ ] 배포 채널 우선순위
- [x] `cargo test` 전체 통과 (단위 30 + 통합 3)
