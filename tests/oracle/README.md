# Reference tests (oracle)

The cases are taken from [SolidCraft](https://github.com/bherbruck/solidcraft) `tests/oracle/`
(commit `4687e3b0734e41d761be0d0a2611e5402f088348`, MIT OR Apache-2.0, see
[ATTRIBUTION.md](../../ATTRIBUTION.md)), without `reference.stl`.

Each folder is one case:

- `recipe.json`: the part (sketches in sketch coordinates, features with their parameters,
  faces and edges picked by a point in world space);
- `expected.json`: what the part must measure and where the numbers come from
  (`"source": "analytic"` with the formulas in `method`, or FreeCAD run as a separate program).

Runner: `tests/oracle.rs` (`cargo test --test oracle -- --nocapture` prints the table).
It compares the number of bodies (exactly), volume and area (relative 1e-3), centre of mass and
bounding box (1e-3 × the diagonal), and the number of faces (merged, exactly). Features we do
not have yet mark the case "skipped"; failures not fixed yet are listed in `known_failures.txt`
(with the reason). A listed case that starts to pass also fails the test: remove it from the list.

**Never change `expected.json` to match the program.**
