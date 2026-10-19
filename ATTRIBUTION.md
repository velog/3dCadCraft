# Attribution

Code and data adapted or used from other projects. All are under licences compatible with
this project's `MIT OR Apache-2.0`.

| What | Source | Licence | How it is used |
|---|---|---|---|
| `solidcraft-kernel`, `solidcraft-geom` | SolidCraft, https://github.com/bherbruck/solidcraft (commit `4687e3b0734e41d761be0d0a2611e5402f088348`) | MIT OR Apache-2.0 | dependency: exact B-rep operations (extrude, revolve, Booleans, fillet, chamfer, …), behind `kernel::exact` |
| `truck-*` crates | truck, https://github.com/ricosjp/truck (RICOS Co. Ltd.), with SolidCraft's fixes (`vendor/` in SolidCraft) | Apache-2.0 | dependency of `solidcraft-kernel` (via `[patch.crates-io]`) |
| Shell Outside / Both as an outward offset less a cavity (open faces pushed out by 2t + 1) | SolidCraft `crates/doc/src/eval.rs` (`FeatureKind::Shell`, same commit) | MIT OR Apache-2.0 | rewritten in `kernel::exact::shell` |
| Reference-test cases `tests/oracle/*/recipe.json`, `expected.json` | SolidCraft `tests/oracle/` (same commit) | MIT OR Apache-2.0 | copied unchanged; run by our `tests/oracle.rs` |
| Ideas: `cargo xtask ci`/`assets`, panic-ban lints and `clippy.toml`, Boolean-identity fuzz test | SolidCraft `xtask/`, `clippy.toml`, `crates/kernel/src/fuzz.rs` | MIT OR Apache-2.0 | rewritten for this project (`xtask/`, `src/lib.rs`, `tests/boolean_identity.rs`) |
