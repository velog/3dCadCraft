# Contributing to 3DCadCraft

Thank you for your interest! 3DCadCraft is an open-source parametric 3D CAD
system in pure Rust whose user interface follows Autodesk Inventor, so that
Inventor users can move over easily. Contributions of any size are welcome:
bug reports, test cases, documentation, features.

## Getting started

```bash
git clone https://github.com/velog/3dCadCraft.git
cd 3dCadCraft
cargo run --release                 # the application
cargo test --no-default-features    # all tests without the graphics stack (fast)
cargo xtask ci                      # every check CI runs, in order
```

Windows needs the Visual Studio C++ build tools (the Rust installer offers
them). On Linux the GUI needs the usual X11/Wayland development packages
(see `.github/workflows/ci.yml`).

## Before you start

- **Read [CLAUDE.md](CLAUDE.md).** Despite the name, it is the developer
  guide for humans and AI agents alike: architecture, invariants and the
  pitfalls we have already paid for.
- Look at [docs/ROADMAP.md](docs/ROADMAP.md) (the **Критични** section lists
  the most urgent problems) and [docs/FEATURES.md](docs/FEATURES.md).
- For anything bigger than a bug fix, open an issue first so we can agree on
  the approach.

## The rules that matter most

1. **Clean room.** Never copy code, text, icons or data from Autodesk
   products, and never read or copy GPL/LGPL code (FreeCAD, OpenCASCADE,
   SolveSpace…). Using FreeCAD as a *black-box* reference (running it and
   comparing results) is fine. Anything adapted from a permissively licensed
   project gets a row in [ATTRIBUTION.md](ATTRIBUTION.md).
2. **Spec → code → proof.** Every feature has a spec in `docs/spec/`,
   describing Inventor's behaviour option by option *in our own words*,
   with a status per option. Write or update it before the code.
3. **No `unsafe`, no `unwrap`/`expect`/`panic` in non-test code** (enforced
   by lints). Geometry is compared through `Tolerance`, never with `==`.
4. **Persistent names, never indices.** Model topology is referenced by
   names such as `Extrusion1:Top`; arena ids change on every rebuild.
5. **Failure leaves state unchanged.** A failing operation reports a reason
   and leaves the model as it was.
6. **The file format is additive** (`#[serde(default)]`); see
   [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md) for revisions and fixtures.
7. **Tests with real numbers.** Geometry changes come with measured vs
   expected values (volume, area, faces), at least a few geometry variants,
   and a speed case in `tests/perf.rs` when it can be slow.

## Languages

- Code, identifiers, comments, commit messages: **English**.
- Files in `docs/` are currently written in **Bulgarian** (the project
  started there). English contributions to `docs/` are welcome too; keep
  command and option names in English, as Inventor uses them.

## Pull requests

- Keep a pull request focused on one thing.
- Before opening it: `cargo fmt`, `cargo clippy --all-targets` (no warnings),
  `cargo test` — or simply `cargo xtask ci`.
- Speed tests (`tests/perf.rs`, the timing rows of `tests/matrix.rs`) are
  sensitive to machine load; CI runs them with `CADCRAFT3D_NO_BUDGETS=1`.
  Run them locally on a quiet machine if your change can affect speed.
- Describe what you changed and how you checked it. For visible changes,
  add a screenshot.

## Reporting bugs

The application keeps a session log and an action journal. After a problem,
use **Help → Save Diagnostics…** and attach the zip to the issue. The
journal can be replayed with
`cargo run --example replay -- <journal>` to find the first step that goes
wrong. Do not attach diagnostics of confidential designs: the zip contains
the open project.

## Licence

By contributing you agree that your contributions are licensed under the
same terms as the project: **MIT OR Apache-2.0**, at the user's option.
