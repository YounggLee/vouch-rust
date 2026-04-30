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
