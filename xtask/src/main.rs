//! Project tasks (`cargo xtask <task>`), after SolidCraft's xtask. Tests come
//! in three tiers (`docs/TESTING.md` says which to run when):
//!
//! - `quick` (a few minutes): format, assets, clippy, the unit tests except the
//!   heavy ones (`heavy_unit_tests.txt`), the light integration tests, the GUI
//!   unit tests;
//! - `heavy`: the heavy unit tests, the medium integration tests, then the
//!   timed ones (`TIMED`: perf budgets, Boolean identities, oracle) one binary
//!   at a time so their budgets do not share the machine;
//! - `ci`: every check once: `quick` (and clippy without the GUI), `heavy`,
//!   docs — stopping at the first failure;
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

/// Integration tests that take 10–30 s (run by `heavy`).
const MEDIUM: &[&str] = &[
    "hole_threads",
    "holes",
    "matrix",
    "freecad_cases",
    "freecad_patterns",
    "split_surface",
];

/// Integration tests with time budgets or long runs, each run alone (last).
const TIMED: &[&str] = &["boolean_identity", "oracle", "perf_drawing", "perf"];

/// Unit tests that take 3 s or more under a full parallel run (measured
/// 2026-10-26), one exact name per line.
const HEAVY_UNIT: &str = include_str!("heavy_unit_tests.txt");

fn heavy_unit() -> Vec<&'static str> {
    HEAVY_UNIT
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// The light integration tests: every `tests/*.rs` not in `MEDIUM` / `TIMED`.
fn light() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(root().join("tests"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension()? == "rs")
                .then(|| p.file_stem()?.to_str().map(str::to_owned))
                .flatten()
        })
        .filter(|n| !MEDIUM.contains(&n.as_str()) && !TIMED.contains(&n.as_str()))
        .collect();
    v.sort();
    v
}

fn clippy(gui: bool) -> bool {
    let mut a = vec!["cargo", "clippy", "--workspace", "--all-targets"];
    if !gui {
        a.push("--no-default-features");
    }
    a.extend(["--", "-D", "warnings"]);
    run(&a, &[])
}

/// The tests of the quick tier.
fn quick_tests() -> bool {
    let heavy = heavy_unit();
    let mut unit = vec![
        "cargo",
        "test",
        "--no-default-features",
        "--lib",
        "--",
        "--exact",
    ];
    for h in &heavy {
        unit.extend(["--skip", h]);
    }
    let light = light();
    let mut integ = vec!["cargo", "test", "--no-default-features"];
    for t in &light {
        integ.extend(["--test", t.as_str()]);
    }
    run(&unit, &[]) && run(&integ, &[]) && run(&["cargo", "test", "--lib", "gui::"], &[])
}

fn quick() -> bool {
    run(&["cargo", "fmt", "--all", "--check"], &[]) && assets() && clippy(true) && quick_tests()
}

fn heavy() -> bool {
    let heavy = heavy_unit();
    let mut unit = vec![
        "cargo",
        "test",
        "--no-default-features",
        "--lib",
        "--",
        "--exact",
    ];
    unit.extend(heavy.iter().copied());
    let mut medium = vec!["cargo", "test", "--no-default-features"];
    for t in MEDIUM {
        medium.extend(["--test", t]);
    }
    run(&unit, &[])
        && run(&medium, &[])
        && TIMED.iter().all(|t| {
            run(
                &["cargo", "test", "--no-default-features", "--test", t],
                &[],
            )
        })
}

fn ci() -> bool {
    run(&["cargo", "fmt", "--all", "--check"], &[])
        && assets()
        && clippy(false)
        && clippy(true)
        && quick_tests()
        && heavy()
        && run(
            &["cargo", "doc", "--no-default-features", "--no-deps"],
            &[("RUSTDOCFLAGS", "-D warnings")],
        )
}

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let ok = match task.as_str() {
        "ci" => ci(),
        "quick" => quick(),
        "heavy" => heavy(),
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
            eprintln!("usage: cargo xtask <quick|heavy|ci|oracle|assets>");
            false
        }
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
