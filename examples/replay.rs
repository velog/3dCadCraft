//! Replays an action journal (`docs/spec/0.14-diagnostics.md`):
//!
//! ```text
//! cargo run --example replay -- path/to/session-…-journal.jsonl
//! cargo run --example replay -- journal.jsonl --part 12 out.3dcadcraft
//! ```
//!
//! Prints every recorded state with what the session's regeneration gave
//! and what the replay gives, and marks the first step that differs (exit
//! code 1). `--part N FILE` saves the part as it was at state N as a project.

use std::path::PathBuf;
use std::process::ExitCode;

use cadcraft3d::diagnostics::replay;
use cadcraft3d::document::{DocumentData, Project};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first().map(PathBuf::from) else {
        eprintln!("usage: replay JOURNAL [--part N OUT.3dcadcraft]");
        return ExitCode::from(2);
    };
    if let Some(k) = args.iter().position(|a| a == "--part") {
        let (Some(n), Some(out)) = (
            args.get(k + 1).and_then(|n| n.parse::<u64>().ok()),
            args.get(k + 2),
        ) else {
            eprintln!("usage: replay JOURNAL --part N OUT.3dcadcraft");
            return ExitCode::from(2);
        };
        return match replay::part_at(&path, n) {
            Ok(part) => {
                let mut p = Project::new(format!("Replay of state {n}"));
                p.add_document("Part1", DocumentData::Part(part));
                match p.save(out.as_ref()) {
                    Ok(()) => {
                        println!("state {n} saved to {out}");
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("{out}: {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let steps = match replay::replay(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let fmt = |o: &Option<cadcraft3d::diagnostics::journal::Outcome>| match o {
        Some(o) => format!(
            "vol {:.3} faces {} sick {}",
            o.volume,
            o.faces,
            if o.health.is_empty() {
                "-".to_owned()
            } else {
                format!("{:?}", o.health)
            }
        ),
        None => "-".into(),
    };
    let first = replay::first_difference(&steps).map(|s| s.line);
    for s in &steps {
        let mark = if Some(s.line) == first {
            ">>"
        } else if s.differs() {
            "! "
        } else {
            "  "
        };
        println!(
            "{mark} line {:>5} state {:>4} {} {:<24} recorded: {} | replayed: {}",
            s.line,
            s.seq,
            s.time,
            s.label,
            fmt(&s.recorded),
            fmt(&s.replayed)
        );
        if let Some(e) = &s.error {
            println!("     error: {e}");
        }
        if let Some(d) = &s.difference {
            println!("     differs: {d}");
        }
    }
    match replay::first_difference(&steps) {
        Some(s) => {
            println!(
                "first difference: line {} (state {}, '{}')",
                s.line, s.seq, s.label
            );
            ExitCode::FAILURE
        }
        None => {
            println!("{} states replayed, all as recorded", steps.len());
            ExitCode::SUCCESS
        }
    }
}
