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

    cmux::set_status(
        "vouch",
        &format!("review · {} items", items.len()),
        "shield-check",
    );

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
            "cmux" => cmux::notify(
                "vouch",
                &format!("sent {} rejects to source", rejects.len()),
            ),
            "gh" => cmux::notify(
                "vouch",
                &format!("posted {} rejects as PR comment", rejects.len()),
            ),
            _ => {}
        }
    });

    let on_progress: Box<dyn FnMut(usize, usize)> = Box::new(|decided, total| {
        let val = if total > 0 {
            decided as f64 / total as f64
        } else {
            0.0
        };
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
