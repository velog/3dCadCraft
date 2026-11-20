# STEP corpus

STEP files of known shape that `tests/step_corpus.rs` imports through our Import (spec 7.7a) and checks
against their exact volume, area, box and body count.

| Folder / file | From | Licence |
|---|---|---|
| `conformance/` | SolidCraft `tests/step-conformance` (main `61351c67`), generated from formulas by [step-conformance](https://github.com/dan335/step-conformance) (commit 96f30bb); see `conformance/SOURCE.md` | CC0 1.0 |
| `regressions/` | SolidCraft `tests/step-regressions` (main `61351c67`): files that once broke SolidCraft's import, written by SolidCraft or entity by entity from formulas; what each holds: `regressions/SOURCE.md` | MIT OR Apache-2.0 |
| `external-corpus.toml` | SolidCraft `tests/external-corpus.toml` (same commit): the NIST MBE PMI STEP files (US Government work, public domain), hash-checked; the volumes and areas there are SolidCraft's measurements | MIT OR Apache-2.0 (the list); the files are downloaded, never stored here |

Expected values are the generators' formulas, never a run of ours. A case that cannot pass yet goes
into `known_failures.txt` (`<file> <reason>`); one that starts passing fails the test until it is taken out.

External corpus: download the archive named in `external-corpus.toml`, check its `archive_sha256`,
unpack it, then `CADCRAFT3D_EXTERNAL_CORPUS=<folder> cargo test --test step_corpus -- --ignored`.
