//! Boolean identities on random primitives (idea from SolidCraft's kernel
//! fuzz test): V(A∪B) + V(A∩B) = V(A) + V(B), V(A−B) = V(A) − V(A∩B), and
//! every result converts to a valid closed B-rep of ours.

use std::f64::consts::{PI, TAU};

use cadcraft3d::kernel::exact::{self, ExactOp, ExactSolid};
use cadcraft3d::kernel::{Plane, Point3, ProfileSegment, Real, Segment2, Vector3};
use nalgebra::Point2;

/// xorshift64*: reproducible without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> Real {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as Real / (1u64 << 53) as Real
    }
    /// A value in [lo, hi) on a 0.5 mm grid (like real models, which line
    /// faces up; the grid also makes coincident faces happen).
    fn grid(&mut self, lo: Real, hi: Real) -> Real {
        ((lo + (hi - lo) * self.next()) * 2.0).round() / 2.0
    }
}

fn seg(g: Segment2, n: &str) -> ProfileSegment {
    ProfileSegment {
        geometry: g,
        name: n.to_owned(),
    }
}

fn circle(c: Point2<Real>, r: Real) -> Vec<ProfileSegment> {
    vec![
        seg(
            Segment2::Arc {
                c,
                r,
                a0: 0.0,
                sweep: PI,
            },
            "c:0",
        ),
        seg(
            Segment2::Arc {
                c,
                r,
                a0: PI,
                sweep: PI,
            },
            "c:1",
        ),
    ]
}

/// A random primitive and its exact volume.
fn primitive(rng: &mut Rng, name: &str) -> (ExactSolid, Real, &'static str) {
    let o = Point3::new(
        rng.grid(-10.0, 10.0),
        rng.grid(-10.0, 10.0),
        rng.grid(-10.0, 10.0),
    );
    let xy = |z: Real| Plane::new(Point3::new(0.0, 0.0, z), Vector3::z(), Vector3::x()).unwrap();
    match (rng.next() * 4.0) as u32 {
        0 => {
            let s = Vector3::new(
                rng.grid(4.0, 20.0),
                rng.grid(4.0, 20.0),
                rng.grid(4.0, 20.0),
            );
            let b = exact::box_named(o, o + s, name).unwrap();
            (b, s.x * s.y * s.z, "box")
        }
        1 => {
            let (r, h) = (rng.grid(2.0, 10.0), rng.grid(4.0, 20.0));
            let prof = exact::profile(&circle(Point2::new(o.x, o.y), r), &[]).unwrap();
            let b = exact::extrude(&xy(o.z), &prof, 0.0, h, 0.0, name).unwrap();
            (b, PI * r * r * h, "cylinder")
        }
        2 => {
            // A sphere: a half disc on the XZ plane through o, about the vertical axis.
            let r = rng.grid(2.0, 10.0);
            let pl = Plane::new(o, Vector3::y(), Vector3::x()).unwrap();
            let (c, top, bot) = (
                Point2::new(0.0, 0.0),
                Point2::new(0.0, r),
                Point2::new(0.0, -r),
            );
            let pl_up = pl.y_axis().into_inner().z.signum();
            let half = vec![
                seg(Segment2::Line { a: top, b: bot }, "axis:0"),
                seg(
                    Segment2::Arc {
                        c,
                        r,
                        a0: -PI / 2.0,
                        sweep: PI,
                    },
                    "arc:0",
                ),
            ];
            let prof = exact::profile(&half, &[]).unwrap();
            let b = exact::revolve(
                &pl,
                &prof,
                c,
                nalgebra::Vector2::new(0.0, pl_up),
                0.0,
                TAU,
                name,
            )
            .unwrap();
            (b, 4.0 / 3.0 * PI * r * r * r, "sphere")
        }
        _ => {
            let (rmin, rmaj) = (rng.grid(1.0, 4.0), rng.grid(6.0, 12.0));
            let pl = Plane::new(o, Vector3::y(), Vector3::x()).unwrap();
            let up = pl.y_axis().into_inner().z.signum();
            let prof = exact::profile(&circle(Point2::new(rmaj, 0.0), rmin), &[]).unwrap();
            let b = exact::revolve(
                &pl,
                &prof,
                Point2::new(0.0, 0.0),
                nalgebra::Vector2::new(0.0, up),
                0.0,
                TAU,
                name,
            )
            .unwrap();
            (b, 2.0 * PI * PI * rmaj * rmin * rmin, "torus")
        }
    }
}

fn volume(s: &Option<ExactSolid>) -> Real {
    s.as_ref()
        .map_or(0.0, |s| s.measure_full().map_or(Real::NAN, |m| m.volume))
}

/// Our B-rep of `s` validates as closed solids.
fn valid(s: &Option<ExactSolid>) -> Result<(), String> {
    let Some(s) = s else { return Ok(()) };
    let b = exact::to_brep(std::slice::from_ref(s))?;
    for (id, _) in b.solids() {
        b.validate_solid(id).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[test]
fn boolean_identities_hold() {
    let mut problems = Vec::new();
    let mut checked = 0;
    // A pair takes up to a minute (spheres and tori): a few by default,
    // more with BOOLEAN_FUZZ_SEEDS=40 (the long run).
    let seeds: u64 = std::env::var("BOOLEAN_FUZZ_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    for seed in 1..=seeds {
        let t0 = std::time::Instant::now();
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (a, va0, ka) = primitive(&mut rng, "A");
        let (b, vb0, kb) = primitive(&mut rng, "B");
        let (va, vb) = (volume(&Some(a.clone())), volume(&Some(b.clone())));
        // The primitives themselves (tessellated volume vs formula).
        for (v, v0, k) in [(va, va0, ka), (vb, vb0, kb)] {
            if (v - v0).abs() > 2e-3 * v0 {
                problems.push(format!("seed {seed}: {k} volume {v:.3} vs {v0:.3}"));
            }
        }
        eprintln!("seed {seed}: {ka} / {kb}");
        let ops = [ExactOp::Union, ExactOp::Intersect, ExactOp::Cut];
        let r: Vec<Result<Option<ExactSolid>, String>> =
            ops.iter().map(|&op| exact::boolean(&a, &b, op)).collect();
        let (u, i, c) = match (&r[0], &r[1], &r[2]) {
            (Ok(u), Ok(i), Ok(c)) => (u, i, c),
            _ => {
                let errs: Vec<String> =
                    r.iter().filter_map(|x| x.as_ref().err().cloned()).collect();
                problems.push(format!("seed {seed} {ka}/{kb}: {}", errs.join("; ")));
                continue;
            }
        };
        checked += 1;
        eprintln!("  {:.1} s", t0.elapsed().as_secs_f64());
        let (vu, vi, vc) = (volume(u), volume(i), volume(c));
        let tol = 2e-3 * (va + vb);
        if (vu + vi - va - vb).abs() > tol {
            problems.push(format!(
                "seed {seed} {ka}/{kb}: V(A∪B)+V(A∩B) = {:.3}, V(A)+V(B) = {:.3}",
                vu + vi,
                va + vb
            ));
        }
        if (vc - (va - vi)).abs() > tol {
            problems.push(format!(
                "seed {seed} {ka}/{kb}: V(A−B) = {vc:.3}, V(A)−V(A∩B) = {:.3}",
                va - vi
            ));
        }
        for (s, what) in [(u, "union"), (i, "intersection"), (c, "cut")] {
            if let Err(e) = valid(s) {
                problems.push(format!(
                    "seed {seed} {ka}/{kb}: {what} is not a valid B-rep: {e}"
                ));
            }
        }
    }
    println!(
        "boolean identities: {checked} pairs checked, {} problems",
        problems.len()
    );
    for p in &problems {
        println!("  {p}");
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
