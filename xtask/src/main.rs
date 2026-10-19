//! Project tasks (`cargo xtask <task>`), after SolidCraft's xtask:
//!
//! - `ci`: every check, in order, stopping at the first failure: format,
//!   attribution of assets, clippy (with and without the GUI), tests
//!   (with and without the GUI), docs;
//! - `oracle`: the reference cases with their table (`tests/oracle/`);
//! - `assets`: every image / font / model file in the repository has a row
//!   in `ATTRIBUTION.md`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Runs a command in the repository root, printing it first.
fn run(args: &[&str], env: &[(&str, &str)]) -> bool {
    println!("$ {}", args.join(" "));
    let mut c = Command::new(args[0]);
    c.args(&args[1..]).current_dir(root());
    for (k, v) in env {
        c.env(k, v);
    }
    matches!(c.status(), Ok(s) if s.success())
}

const ASSET_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "ico", "ttf", "otf", "woff", "woff2", "wav", "ogg", "mp3",
    "stl", "step", "stp", "iges", "igs", "obj", "3mf", "glb", "gltf", "hdr", "exr",
];

/// Files with an asset extension that `ATTRIBUTION.md` does not mention.
fn assets() -> bool {
    let root = root();
    let attribution = std::fs::read_to_string(root.join("ATTRIBUTION.md")).unwrap_or_default();
    let mut missing = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let ext = p
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if ASSET_EXT.contains(&ext.as_str()) {
                let rel = p
                    .strip_prefix(&root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                // `ATTRIBUTION.md` writes paths with `/` (Windows lists `\`).
                let dir_rel = p
                    .parent()
                    .and_then(|d| d.strip_prefix(&root).ok())
                    .map(|d| d.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                if !attribution.contains(&rel)
                    && (dir_rel.is_empty() || !attribution.contains(&format!("{dir_rel}/")))
                {
                    missing.push(rel);
                }
            }
        }
    }
    println!("$ assets: {} unattributed", missing.len());
    for m in &missing {
        println!("  {m} has no row in ATTRIBUTION.md");
    }
    missing.is_empty()
}

fn ci() -> bool {
    let clippy = |gui: bool| {
        let mut a = vec!["cargo", "clippy", "--workspace", "--all-targets"];
        if !gui {
            a.push("--no-default-features");
        }
        a.extend(["--", "-D", "warnings"]);
        run(&a, &[])
    };
    run(&["cargo", "fmt", "--all", "--check"], &[])
        && assets()
        && clippy(false)
        && clippy(true)
        && run(&["cargo", "test", "--no-default-features"], &[])
        && run(&["cargo", "test"], &[])
        && run(
            &["cargo", "doc", "--no-default-features", "--no-deps"],
            &[("RUSTDOCFLAGS", "-D warnings")],
        )
}

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let ok = match task.as_str() {
        "ci" => ci(),
        "assets" => assets(),
        "oracle" => run(
            &[
                "cargo",
                "test",
                "--no-default-features",
                "--test",
                "oracle",
                "--",
                "--nocapture",
            ],
            &[],
        ),
        _ => {
            eprintln!("usage: cargo xtask <ci|oracle|assets>");
            false
        }
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
