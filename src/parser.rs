use crate::models::RawHunk;
use regex::Regex;

pub fn parse_raw_hunks(unified_diff: &str) -> Vec<RawHunk> {
    if unified_diff.trim().is_empty() {
        return Vec::new();
    }

    // git diff omits ",N" when N=1, so the comma+count groups are optional.
    let hunk_re = Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@").unwrap();
    let mut out = Vec::new();
    let mut current_file = String::new();
    let mut current_header = String::new();
    let mut current_body: Vec<String> = Vec::new();
    let mut old_start = 0u32;
    let mut old_lines = 0u32;
    let mut new_start = 0u32;
    let mut new_lines = 0u32;
    let mut in_hunk = false;

    let flush = |out: &mut Vec<RawHunk>,
                 current_file: &str,
                 current_header: &str,
                 current_body: &mut Vec<String>,
                 old_start: u32,
                 old_lines: u32,
                 new_start: u32,
                 new_lines: u32| {
        let body = current_body.join("\n");
        out.push(RawHunk {
            id: format!("r{}", out.len()),
            file: current_file.to_string(),
            old_start,
            old_lines,
            new_start,
            new_lines,
            header: current_header.to_string(),
            body,
        });
        current_body.clear();
    };

    for line in unified_diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            if in_hunk {
                flush(
                    &mut out,
                    &current_file,
                    &current_header,
                    &mut current_body,
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                );
                in_hunk = false;
            }
            current_file = path.to_string();
        } else if line.starts_with("+++ ") {
            // e.g. "+++ /dev/null"
            if in_hunk {
                flush(
                    &mut out,
                    &current_file,
                    &current_header,
                    &mut current_body,
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                );
                in_hunk = false;
            }
        } else if line.starts_with("--- ") {
            if current_file.is_empty() {
                if let Some(path) = line.strip_prefix("--- a/") {
                    current_file = path.to_string();
                }
            }
        } else if let Some(caps) = hunk_re.captures(line) {
            if in_hunk {
                flush(
                    &mut out,
                    &current_file,
                    &current_header,
                    &mut current_body,
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                );
            }
            old_start = caps[1].parse().unwrap_or(0);
            old_lines = caps
                .get(2)
                .map(|m| m.as_str().parse().unwrap_or(1))
                .unwrap_or(1);
            new_start = caps[3].parse().unwrap_or(0);
            new_lines = caps
                .get(4)
                .map(|m| m.as_str().parse().unwrap_or(1))
                .unwrap_or(1);
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

    if in_hunk {
        flush(
            &mut out,
            &current_file,
            &current_header,
            &mut current_body,
            old_start,
            old_lines,
            new_start,
            new_lines,
        );
    }

    out
}

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
        assert!(hunks[0].body.contains("admin") || hunks[0].header.contains("admin"));
    }

    #[test]
    fn empty_diff() {
        assert!(parse_raw_hunks("").is_empty());
    }

    #[test]
    fn whitespace_only_diff() {
        assert!(parse_raw_hunks("   \n\n  ").is_empty());
    }

    #[test]
    fn parses_single_line_hunk_without_comma() {
        let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1,2 @@\n hello\n+world\n";
        let hunks = parse_raw_hunks(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file, "a.txt");
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].old_lines, 1);
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[0].new_lines, 2);
        assert!(hunks[0].body.contains("+world"));
    }
}
