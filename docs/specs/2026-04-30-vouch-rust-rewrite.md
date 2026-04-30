# vouch Rust Rewrite — Design Spec

**Date:** 2026-04-30
**Status:** Draft
**Author:** youngjin.lee

---

## 1. Goal

Python(911줄)으로 작성된 vouch를 Rust로 전환한다. 기능 동일성을 보장하되, 단일 바이너리 배포와 타입 안전성을 확보한다. 새 repo(`vouch-rs`)에서 시작하고, 원본 해커톤 repo는 그대로 보존한다.

## 2. Scope

### In-Scope

- Python 12개 모듈의 Rust 1:1 전환
- LLM 백엔드를 Gemini → Claude API로 교체
- 기존 11개 테스트 파일에 대응하는 Rust 테스트 작성
- 단일 바이너리 빌드 (`cargo build --release`)

### Out-of-Scope

- 새로운 기능 추가 (UI 개선, 추가 입력 모드 등)
- CI/CD 파이프라인 구축 (후속 작업)
- 크로스 컴파일 매트릭스 (후속 작업)

## 3. Architecture

### 3.1 파이프라인 (변경 없음)

```
CLI args → diff_input (git/gh subprocess)
         → parser (unified diff → RawHunk)
         → llm::semantic_postprocess (Claude API → SemanticHunk)
         → llm::analyze (Claude API → Analysis)
         → tui (Ratatui interactive review)
         → feedback + cmux (reject delivery)
```

### 3.2 Crate Dependencies

| 용도 | 크레이트 | 비고 |
|------|---------|------|
| CLI 파서 | `clap` | derive macro로 선언적 정의 |
| TUI 프레임워크 | `ratatui` + `crossterm` | immediate mode 렌더링 |
| TUI 텍스트 입력 | `tui-input` | RejectModal용 |
| HTTP 클라이언트 | `reqwest` (blocking) | Claude API 호출 |
| JSON | `serde` + `serde_json` | 직렬화/역직렬화 |
| 해싱 | `sha2` | 캐시 키 생성 |
| 구문 강조 | `syntect` | diff 하이라이팅 |
| 외부 명령 탐색 | `which` | cmux/gh/clipboard 존재 확인 |

### 3.3 비동기 전략

**동기(blocking) 전용.** 파이프라인이 `diff → LLM1 → LLM2 → TUI`로 완전 순차적이며, TUI 실행 중 네트워크 호출이 없으므로 async 런타임 불필요. `reqwest::blocking`을 사용한다.

## 4. Module Design

### 4.1 `src/models.rs`

Python `dataclass` → Rust `struct` + `enum`. `serde` derive로 JSON 직렬화.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Risk { High, Med, Low }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Confidence { Confident, Uncertain, Guess }

#[derive(Debug, Clone, PartialEq)]
pub enum Decision { Accept, Reject }

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
```

### 4.2 `src/parser.rs`

직접 구현. `unidiff` 크레이트 미사용. unified diff 포맷의 `---`, `+++`, `@@` 헤더를 regex로 파싱하고, 라인별 `+`/`-`/` ` 분류. Python `parser.py`(33줄)과 동일한 로직.

**파싱 규칙:**
1. `--- a/path` / `+++ b/path`로 파일 경계 감지
2. `@@ -old_start,old_lines +new_start,new_lines @@`로 hunk 시작
3. 이후 라인은 hunk body — `+`, `-`, ` `(context) 중 하나로 시작
4. 다음 `@@` 또는 `---`까지가 하나의 hunk

### 4.3 `src/diff_input.rs`

`std::process::Command`로 git/gh 서브프로세스 호출. Python과 동일한 4가지 모드:

| 모드 | 명령 |
|------|------|
| uncommitted | `git diff HEAD` |
| commit | `git show --format= <ref>` |
| range | `git diff <a>..<b>` |
| pr | `gh pr diff <number>` |

PR URL 정규식: `https?://github\.com/[^/]+/[^/]+/pull/(\d+)`
Range 정규식: `^[^.\s]+\.\.[^.\s]+$`

### 4.4 `src/cache.rs`

Python과 동일한 SHA256 캐시. `sha2` 크레이트 + `serde_json`.

- 키: `{stage}.{sha256_hex[:16]}.json`
- 디렉토리: `VOUCH_CACHE_DIR` 환경변수 (기본: `fixtures/responses`)
- fallback: `{stage}.json`도 탐색 (데모 호환)

### 4.5 `src/llm.rs`

**Gemini → Claude API 교체.** `reqwest::blocking`으로 Anthropic Messages API 직접 호출.

**환경변수:**
- `ANTHROPIC_API_KEY` — API 키
- `VOUCH_MODEL` — 모델 ID (기본: `claude-sonnet-4-6-20250514`)
- `VOUCH_CACHE_ONLY=1` — 캐시 전용 모드

**API 호출 구조:**
```
POST https://api.anthropic.com/v1/messages
Headers:
  x-api-key: {ANTHROPIC_API_KEY}
  anthropic-version: 2023-06-01
  content-type: application/json
Body:
  model, max_tokens, temperature: 0.1,
  system: (프롬프트),
  messages: [{role: "user", content: (payload)}]
```

**Structured output 전략:** 시스템 프롬프트에 JSON 스키마를 명시하고, 응답에서 JSON 블록을 추출하여 `serde_json::from_str`로 파싱. Gemini의 `response_schema`와 달리, 프롬프트 기반으로 JSON 포맷을 강제한다.

**함수 2개 (Python과 동일):**
- `semantic_postprocess(raw_hunks) -> Vec<SemanticHunk>` — 캐시 → API 호출 → `_build_semantic`
- `analyze(semantic_hunks) -> Vec<Analysis>` — 캐시 → API 호출 → deserialize

### 4.6 `src/feedback.rs`

Python과 동일한 문자열 조합. 두 함수:
- `build_reject_prompt(rejected) -> String` — 에이전트 재시도용 한국어 프롬프트
- `build_pr_review_body(rejected) -> String` — PR 코멘트용 마크다운

### 4.7 `src/cmux.rs`

Python과 동일한 구조. `std::process::Command` + `which` 크레이트.

**함수:**
- `cmux_available() -> bool`
- `discover_source_surface(cli_flag) -> Option<String>`
- `set_status/clear_status/set_progress/notify`
- `send_to_surface(surface, text) -> bool`
- `post_pr_comment(pr, text) -> bool`
- `deliver_reject(text, surface, pr_number) -> String` — 우선순위: gh → cmux → clipboard → stdout

### 4.8 `src/tui.rs`

Ratatui + crossterm. 현재 Textual TUI와 동일한 레이아웃 및 기능:

**레이아웃:**
```
┌──────────────────────────────────────────┐
│ vouch — you vouch, AI helps              │  Header
├────────────────────┬─────────────────────┤
│ Risk│Conf│Intent   │ [b] intent          │
│ 🔴  │ ✅ │ SQL inj │ Risk: 🔴 high       │  Detail header
│ 🟡  │ ⚠️ │ Add auth│ Confidence: ✅      │
│ 🟢  │ ✅ │ Rename  │ Summary: ...        │
│                    ├─────────────────────┤
│   DataTable        │ --- a/file.py       │
│   (좌측 50%)       │ +++ b/file.py       │  Diff scroll
│                    │ @@ -1,3 +1,5 @@     │
│                    │ +new_line            │
├────────────────────┴─────────────────────┤
│ j↓ k↑ a:accept A:all r:reject s:send q  │  Footer
└──────────────────────────────────────────┘
```

**키바인딩:**
| 키 | 액션 |
|----|------|
| j/k | 테이블 행 이동 |
| a | 현재 항목 accept |
| A | low-risk 전체 accept |
| r | reject 모달 열기 |
| s | reject 전송 후 종료 |
| q | 종료 |
| Enter | diff 패인 포커스 |
| Esc | 큐 패인 포커스 |
| `[`/`]` | 분할 비율 조정 |

**마우스:**
- 분할선 드래그 리사이즈 (20%-80% 범위)
- hover 시 분할선 하이라이트 (Color(255, 170, 0))

**RejectModal:**
- 중앙 오버레이 (Clear + Block)
- `tui-input` 위젯으로 텍스트 입력
- Enter 제출, Esc 취소

**diff 구문 강조:**
- `syntect`로 diff 문법 하이라이팅
- `+` 라인 → 초록, `-` 라인 → 빨강, `@@` → 시안

### 4.9 `src/main.rs`

`clap` derive macro로 CLI 정의. Python `cli.py`와 동일한 파이프라인 오케스트레이션.

```rust
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
```

`help` 서브커맨드는 clap의 `long_help` 또는 별도 함수로 구현.

## 5. Test Strategy

Python 11개 테스트 파일을 Rust `#[cfg(test)]` 모듈 + `tests/` 통합 테스트로 1:1 대응.

### 5.1 단위 테스트 (모듈 내 `#[cfg(test)]`)

| Rust 모듈 | 대응 Python 테스트 | 핵심 케이스 |
|-----------|-------------------|------------|
| `models.rs` | `test_models.py` (43줄) | Risk/Confidence enum 직렬화, ReviewItem 생성 |
| `parser.rs` | `test_parser.py` (19줄) | 정상 diff 파싱, 빈 입력, 멀티파일, 바이너리 파일 |
| `diff_input.rs` | `test_diff_input.py` (30줄) | 4가지 모드 resolve, PR URL 파싱, range 정규식 |
| `cache.rs` | `test_cache.py` (25줄) | save→load 왕복, 캐시 미스, fallback |
| `feedback.rs` | `test_feedback.py` (49줄) | reject 프롬프트 포맷, PR body 포맷, 빈 입력 |
| `cmux.rs` | `test_delivery.py` (65줄) | 채널 우선순위, cmux 없을 때 fallback |

### 5.2 LLM 관련 테스트

| Rust 모듈 | 대응 Python 테스트 | 핵심 케이스 |
|-----------|-------------------|------------|
| `llm.rs` | `test_vouch_semantic.py` (205줄) | 캐시된 응답으로 semantic grouping 검증 |
| `llm.rs` | `test_vouch_analysis.py` (185줄) | 캐시된 응답으로 risk/confidence 검증 |
| `llm.rs` | `test_vouch_chunking.py` (172줄) | hunk grouping 로직 (LLM 미호출) |

LLM 테스트는 캐시 fixture(`fixtures/responses/`)를 사용하여 실제 API 호출 없이 검증. 캐시 포맷은 Python 버전과 동일한 JSON 구조를 유지하되, 프롬프트가 달라지므로(Gemini → Claude) **새 fixture를 생성**한다.

### 5.3 통합 테스트

| 파일 | 대응 Python 테스트 | 핵심 케이스 |
|------|-------------------|------------|
| `tests/integration.rs` | `test_integration.py` (113줄) | 4가지 입력 모드 파이프라인 (diff → parse → semantic → analyze) |

fixture repo(`vouch-fixtures`) 활용. `VOUCH_CACHE_ONLY=1`로 LLM 호출 차단.

### 5.4 TUI 테스트

TUI는 자동화 테스트가 어려우므로, 로직만 분리하여 테스트:
- 테이블 데이터 생성 로직
- accept/reject 상태 변경 로직
- accept_all_low 필터 로직
- progress 계산 로직

렌더링 자체는 수동 검증.

## 6. Migration Checklist

전환 완료 기준:

- [ ] `vouch` (인자 없음) — uncommitted diff 리뷰
- [ ] `vouch <commit>` — 단일 커밋 리뷰
- [ ] `vouch <a>..<b>` — 범위 리뷰
- [ ] `vouch --pr <number>` — PR 리뷰
- [ ] `vouch help` — 도움말 출력
- [ ] TUI: j/k 이동, a/A accept, r reject (모달), s 전송, q 종료
- [ ] TUI: 50/50 분할, 마우스 드래그 리사이즈, diff 구문 강조
- [ ] 캐시: save/load/fallback 정상 동작
- [ ] 배포: cmux → gh → clipboard → stdout 우선순위
- [ ] 전체 테스트 통과 (`cargo test`)
