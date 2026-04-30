use std::path::Path;

fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn full_pipeline_sample_diff() {
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
    assert_eq!(resolve_mode(&["HEAD~3..HEAD".into()]).kind, ModeKind::Range);
    assert_eq!(
        resolve_mode(&["--pr".into(), "42".into()]).kind,
        ModeKind::Pr
    );
}

#[test]
fn feedback_roundtrip() {
    use vouch::feedback::{build_pr_review_body, build_reject_prompt};
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
