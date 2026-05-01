use regex::Regex;
use std::process::Command;

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
        return ModeSpec {
            kind: ModeKind::Uncommitted,
            value: None,
        };
    }

    if args[0] == "--pr" && args.len() >= 2 {
        return ModeSpec {
            kind: ModeKind::Pr,
            value: Some(args[1].clone()),
        };
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
        return ModeSpec {
            kind: ModeKind::Range,
            value: Some(args[0].clone()),
        };
    }

    ModeSpec {
        kind: ModeKind::Commit,
        value: Some(args[0].clone()),
    }
}

pub fn get_unified_diff(spec: &ModeSpec) -> Result<String, String> {
    match spec.kind {
        ModeKind::Uncommitted => {
            ensure_git_repo()?;
            ensure_has_head()?;
            run_cmd(&["git", "diff", "HEAD"])
        }
        ModeKind::Commit => {
            ensure_git_repo()?;
            let val = spec.value.as_deref().unwrap();
            run_cmd(&["git", "show", "--format=", val])
        }
        ModeKind::Range => {
            ensure_git_repo()?;
            let val = spec.value.as_deref().unwrap();
            run_cmd(&["git", "diff", val])
        }
        ModeKind::Pr => {
            let val = spec.value.as_deref().unwrap();
            run_cmd(&["gh", "pr", "diff", val])
        }
    }
}

fn ensure_git_repo() -> Result<(), String> {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "?".to_string());
    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|e| format!("git not available: {}", e))?;
    if !output.status.success() {
        return Err(format!("not inside a git repository (cwd: {})", cwd));
    }
    Ok(())
}

fn ensure_has_head() -> Result<(), String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .output()
        .map_err(|e| format!("git not available: {}", e))?;
    if !output.status.success() {
        return Err(
            "this repository has no commits yet — make at least one commit first".to_string(),
        );
    }
    Ok(())
}

fn run_cmd(args: &[&str]) -> Result<String, String> {
    let output = Command::new(args[0])
        .args(&args[1..])
        .output()
        .map_err(|e| format!("{} failed: {}", args.join(" "), e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{} failed: {}", args.join(" "), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

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
