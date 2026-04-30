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

pub fn deliver_reject(text: &str, surface: Option<&str>, pr_number: Option<&str>) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_source_surface_cli_flag() {
        let result = discover_source_surface(Some("surface:1".into()));
        assert_eq!(result, Some("surface:1".to_string()));
    }

    #[test]
    fn deliver_reject_falls_back_when_nothing_routes() {
        // gh/cmux는 surface/pr_number 없이 호출 시 시도하지 않음.
        // clipboard가 있는 환경(macOS의 pbcopy 등)에서는 그 채널 이름이 반환됨.
        // 어떤 환경에서도 기대 가능한 채널 중 하나를 반환해야 한다.
        let channel = deliver_reject("body", None, None);
        assert!(
            ["stdout", "pbcopy", "wl-copy", "xclip", "xsel"].contains(&channel.as_str()),
            "unexpected channel: {}",
            channel
        );
    }
}
