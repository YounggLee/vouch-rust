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
    lines.push(
        "거절된 항목 외에는 그대로 유지하고, 위 사유를 직접 해소하는 변경만 적용해줘."
            .to_string(),
    );
    lines.join("\n")
}

pub fn build_pr_review_body(rejected: &[ReviewItem]) -> String {
    if rejected.is_empty() {
        return String::new();
    }
    let sections: Vec<String> = rejected
        .iter()
        .map(|it| {
            let files = it
                .semantic
                .files
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
