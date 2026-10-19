# Attribution

Code and data adapted or used from other projects. All are under licences compatible with
this project's `MIT OR Apache-2.0`.

| What | Source | Licence | How it is used |
|---|---|---|---|
| `solidcraft-kernel`, `solidcraft-geom` | SolidCraft, https://github.com/bherbruck/solidcraft (commit `4687e3b0734e41d761be0d0a2611e5402f088348`) | MIT OR Apache-2.0 | dependency: exact B-rep operations (extrude, revolve, Booleans, fillet, chamfer, …), behind `kernel::exact` |
| `truck-*` crates | truck, https://github.com/ricosjp/truck (RICOS Co. Ltd.), with SolidCraft's fixes (`vendor/` in SolidCraft) | Apache-2.0 | dependency of `solidcraft-kernel` (via `[patch.crates-io]`) |
| Shell Outside / Both as an outward offset less a cavity (open faces pushed out by 2t + 1) | SolidCraft `crates/doc/src/eval.rs` (`FeatureKind::Shell`, same commit) | MIT OR Apache-2.0 | rewritten in `kernel::exact::shell` |
| Hole tools of revolution cut stage by stage (seat, then the bore), seams turned per tool; the modeled thread groove (ISO 68-1 trapezoid from inside the bore to the major diameter, P/8 root flat) swept by `sweep_helix_mesh` | SolidCraft `crates/doc/src/eval.rs` (`hole_tools_and_prisms`, `thread_groove`), `crates/kernel/src/helix.rs` (same commit) | MIT OR Apache-2.0 | rewritten in `part::exact::hole`, `part::hole::ThreadSpec::internal_groove` / `external_groove`, `kernel::exact::{revolved_tool, helix_tool, cut_threads}` |
| Standards tables `data/standards/*.csv` | values of the public standards ISO 261/262/724/273, ISO 4762/10642, DIN 974-1, ASME B1.1/B18.2.8/B18.3 as published in handbooks and manufacturers' tables (sources in each file's header); computed columns from the thread formulas | facts, no licence | never taken from a CAD program's data files |
| Reference-test cases `tests/oracle/*/recipe.json`, `expected.json` | SolidCraft `tests/oracle/` (same commit) | MIT OR Apache-2.0 | copied unchanged; run by our `tests/oracle.rs` |
| Ideas: `cargo xtask ci`/`assets`, panic-ban lints and `clippy.toml`, Boolean-identity fuzz test | SolidCraft `xtask/`, `clippy.toml`, `crates/kernel/src/fuzz.rs` | MIT OR Apache-2.0 | rewritten for this project (`xtask/`, `src/lib.rs`, `tests/boolean_identity.rs`) |
