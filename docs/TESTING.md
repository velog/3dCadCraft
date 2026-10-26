# Testing: which tests to run when

The tests come in three tiers (`cargo xtask <tier>`, defined in `xtask/src/main.rs`). Each test runs
**once** per `ci`; the timed tests run alone so that their budgets measure the code, not the machine.

| Tier | What | Time (24 cores, 2026-10-26) |
|---|---|---|
| targeted | `cargo test --no-default-features --lib <module>::` · `cargo test --no-default-features --test <file>` · `cargo test --lib gui::<module>` | seconds |
| `quick` | `fmt --check`, assets, clippy (GUI), every unit test except the heavy ones, the light integration tests, the GUI unit tests | ~40 s with nothing to build, ~3 min after an edit (mostly compiling) |
| `heavy` | the heavy unit tests (`xtask/src/heavy_unit_tests.txt`), each unit test with a time budget alone (`TIMED_UNIT`), the medium integration tests (`MEDIUM`), then each timed binary alone (`TIMED`: `boolean_identity`, `oracle`, `perf_drawing`, `perf`) | ~6–8 min |
| `ci` | `fmt`, assets, clippy with and without the GUI, the `quick` tests, `heavy`, docs | `quick` + `heavy` |

## When

1. **While working**: the targeted tests of the module or file you change, after every edit.
2. **Before a commit**: `cargo xtask quick`.
3. **Before a push** (every finished step is pushed): `cargo xtask ci`. The time budgets of `heavy`
   need an idle machine: when another build or session is running, wait for it (or ask) instead of
   skipping them; never set `CADCRAFT3D_NO_BUDGETS`.
4. A change to the kernel, regeneration, Booleans, blends, hidden line removal or anything with a
   case in `tests/perf.rs` / `tests/perf_drawing.rs`: run `cargo xtask heavy` before the commit too.

## Keeping the tiers right

- A new integration test file is light unless it takes 10 s or more: then add it to `MEDIUM` (or
  `TIMED` when it has time budgets).
- A unit test with a time budget goes into `TIMED_UNIT` (and the heavy list).
- A unit test that takes 3 s or more in a full parallel run goes into `heavy_unit_tests.txt` (exact
  name). Measure with
  `RUSTC_BOOTSTRAP=1 cargo test --no-default-features --lib -- -Z unstable-options --report-time`.
- A test that is not in any list still runs in `quick`: forgetting a list costs time, never coverage.
