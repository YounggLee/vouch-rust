# vouch

Closed-loop AI diff reviewer. git diff를 의미 단위로 묶고 각 단위의 위험도를 LLM으로 평가한 뒤, 터미널 UI에서 accept/reject를 결정해 결과를 다시 작업자(예: 다른 에이전트)에게 돌려보내는 도구.

## 요구사항

- Rust 1.75+ (Cargo)
- [`claude` CLI](https://docs.claude.com/en/docs/claude-code) — 인증된 상태 (`claude auth login` 또는 `ANTHROPIC_API_KEY`)
- macOS 또는 Linux

## 설치

```bash
git clone https://github.com/YounggLee/vouch-rust.git
cd vouch-rust
cargo install --path .
```

또는 빌드만:

```bash
cargo build --release
./target/release/vouch --help
```

## 사용법

git 저장소 안에서 실행:

```bash
vouch                  # 워킹 트리의 미커밋 변경
vouch <commit>         # 특정 커밋
vouch a..b             # 커밋 범위
vouch <PR-url>         # GitHub PR URL
vouch --pr 123         # PR 번호
```

추가 옵션:

```bash
vouch --source-surface <ref>   # cmux 통합 (작업자에게 reject 사유 전달)
```

### 동작 흐름

1. git diff를 raw hunk로 파싱
2. **semantic** 단계 — LLM이 hunk를 의미 단위(`SemanticHunk`)로 그룹화
3. **analyze** 단계 — 각 단위의 risk(high/med/low), confidence, 한국어 요약을 LLM이 생성 (JSON schema로 enum 출력 보장)
4. TUI에서 accept/reject 결정
5. reject 사유는 stdout 또는 cmux surface로 전달

## TUI 키바인딩

| 키 | 동작 |
|---|---|
| `j` / `k`, `↑` / `↓` | 항목 이동 / 디테일 스크롤 |
| `Enter` | 디테일 패널 포커스 |
| `Esc` | 큐 패널 포커스 |
| `PgUp` / `PgDn`, `Home` / `End` | 디테일 패널 빠른 스크롤 |
| `a` | 현재 항목 accept |
| `A` | risk=low 항목 일괄 accept |
| `r` | 현재 항목 reject (사유 입력) |
| `s` | reject 항목들 작업자에게 전송 후 종료 |
| `[` / `]` | 패널 너비 비율 조정 |
| `q` | 종료 |

## 환경변수

| 변수 | 기본값 | 설명 |
|---|---|---|
| `VOUCH_MODEL` | `claude-sonnet-4-6` | LLM 모델. `claude` CLI가 인식하는 모델명/별칭 사용 |
| `VOUCH_CLAUDE_BIN` | `claude` | `claude` 실행 파일 경로 (PATH에 없을 때) |
| `VOUCH_CACHE_DIR` | `~/.cache/vouch/responses` | LLM 응답 캐시 디렉터리 |
| `VOUCH_CACHE_ONLY` | (unset) | `1`이면 캐시 미스 시 LLM 호출 없이 에러 — 결정론적 재실행/테스트용 |
| `VOUCH_SOURCE_SURFACE` | (unset) | cmux 작업자 surface ref (auto-discovery 폴백) |

## 캐시

LLM 응답은 입력 페이로드 SHA-256 해시 기반으로 디스크에 저장됩니다. 같은 diff에 대한 재실행은 LLM을 다시 호출하지 않습니다. 캐시를 비우려면:

```bash
rm -rf ~/.cache/vouch
```

## 인증 모델

vouch는 LLM 호출을 `claude` CLI에 위임합니다. 인증은 CLI가 관리하므로 vouch에서 별도 API 키를 설정할 필요가 없습니다:

- Claude Pro/Max 사용자: `claude auth login`으로 OAuth 인증
- API 키 사용자: `ANTHROPIC_API_KEY` 환경변수 (CLI가 자동 인식)

## License

MIT
