//! Slot tools (`docs/spec/2.3-hole.md` §3a): a hole's tool profile carried
//! round an oblong (round ends) or a rectangle (sharp corners) instead of
//! being revolved about the axis. Every band of the profile is one kernel
//! extrusion of the outline — straight walls a prism, a cone (countersink,
//! angled end mill) a tapered prism, whose offset outline keeps the round
//! ends concentric — and the bands of one tool are glued together.

use super::*;

/// The outline of a slot at its centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotOutline {
    /// Half the distance between the end centres (round ends) or half the
    /// length (sharp corners), mm.
    pub half: Real,
    /// Round ends (an oblong) or sharp corners (a rectangle).
    pub round: bool,
}

/// The four sides of an outline, in its loop's order: the end at +x, the side
/// at +y, the end at −x, the side at −y.
pub const SLOT_PARTS: [&str; 4] = ["End1", "Side1", "End2", "Side2"];

/// The outline `r` from its centre line (half the width), counter-clockwise
/// in the plane's (x, y).
fn outline_loop(o: SlotOutline, r: Real) -> sg::Loop2 {
    use std::f64::consts::{FRAC_PI_2, PI};
    let h = o.half;
    let p = |x: Real, y: Real| sg::Vec2::new(x, y);
    let line = |a: sg::Vec2, b: sg::Vec2| sg::Seg2::Line { a, b };
    let segs = if o.round {
        vec![
            sg::Seg2::Arc {
                center: p(h, 0.0),
                radius: r,
                start: -FRAC_PI_2,
                sweep: PI,
            },
            line(p(h, r), p(-h, r)),
            sg::Seg2::Arc {
                center: p(-h, 0.0),
                radius: r,
                start: FRAC_PI_2,
                sweep: PI,
            },
            line(p(-h, -r), p(h, -r)),
        ]
    } else {
        vec![
            line(p(h, -r), p(h, r)),
            line(p(h, r), p(-h, r)),
            line(p(-h, r), p(-h, -r)),
            line(p(-h, -r), p(h, -r)),
        ]
    };
    sg::Loop2 { segs }
}

/// How small a band's outline may get at a tip (a fraction of the width it
/// starts from): an angled end mill ends in a line, which no extrusion can
/// reach, so it stops a hair short of it.
const TIP: Real = 0.01;

/// A slot tool: `profile` is a closed polygon `[r, z]` like
/// [`revolved_tool`]'s (z along `up` from `origin`, from point 0 on the axis
/// above the start plane, round the outer side down, back to the axis), its
/// segment `i` named `names[i]` (`"<role>:<k>"`). The outline runs along
/// `along` (square to `up`). Walls are named `<prefix>:<role><part>:<k>` with
/// the part from [`SLOT_PARTS`], caps `<prefix>:<role>:<k>`.
pub fn slot_tool(
    origin: Point3,
    up: Vector3,
    along: Vector3,
    outline: SlotOutline,
    profile: &[[Real; 2]],
    names: &[&str],
    prefix: &str,
) -> Result<ExactSolid, String> {
    if profile.len() < 3 || names.len() != profile.len() {
        return Err("the slot profile is incomplete".into());
    }
    if profile.iter().flatten().any(|x| !x.is_finite()) || profile.iter().any(|p| p[0] < -1e-9) {
        return Err("the slot profile is not valid".into());
    }
    if !(outline.half.is_finite() && outline.half > 1e-9) {
        return Err("the slot must be longer than it is wide".into());
    }
    let k = up
        .try_normalize(1e-12)
        .ok_or_else(|| "the slot axis has no direction".to_owned())?;
    let u = (along - k * along.dot(&k))
        .try_normalize(1e-12)
        .ok_or_else(|| "the slot direction runs along its axis".to_owned())?;
    let w = k.cross(&u);
    let n = profile.len();
    // The caps (horizontal segments with an end on the axis) by height, and
    // the bands down the outer side.
    let mut caps: Vec<(Real, String)> = Vec::new();
    let mut bands: Vec<([Real; 2], [Real; 2], &str)> = Vec::new();
    let on_axis = |p: [Real; 2]| p[0].abs() < 1e-9;
    for i in 0..n {
        let (a, b) = (profile[i], profile[(i + 1) % n]);
        if on_axis(a) && on_axis(b) {
            continue;
        }
        if (a[1] - b[1]).abs() < 1e-9 {
            caps.push((a[1], names[i].to_owned()));
            continue;
        }
        if a[1] < b[1] {
            return Err("the slot profile must run down its outer side".into());
        }
        bands.push((a, b, names[i]));
    }
    if bands.is_empty() {
        return Err("the slot profile has no wall".into());
    }
    let cap_at = |z: Real| -> Option<String> {
        caps.iter()
            .find(|(c, _)| (c - z).abs() < 1e-9)
            .map(|(_, n)| format!("{prefix}:{n}"))
    };
    let split = |name: &str| -> (String, String) {
        match name.split_once(':') {
            Some((role, rest)) => (role.to_owned(), format!(":{rest}")),
            None => (name.to_owned(), String::new()),
        }
    };
    let mut pieces = Vec::with_capacity(bands.len());
    for (a, b, name) in bands {
        let (role, rest) = split(name);
        let walls: Vec<String> = SLOT_PARTS
            .iter()
            .map(|p| format!("{prefix}:{role}{p}{rest}"))
            .collect();
        let ra = a[0].max(TIP * b[0]);
        let rb = b[0].max(TIP * a[0]);
        let (top, bottom) = (a[1], b[1]);
        // A tip ends in its own small cap, named like its walls.
        let (top_cap, bottom_cap) = (
            cap_at(top),
            cap_at(bottom).or_else(|| (b[0] < 1e-9).then(|| format!("{prefix}:{role}{rest}"))),
        );
        let region = sg::Region2 {
            outer: outline_loop(outline, ra),
            holes: Vec::new(),
        };
        let body = if (ra - rb).abs() < 1e-9 {
            let pl = sg::Plane::new(v3(&origin), d3(&u), d3(&w)).ok_or("degenerate slot plane")?;
            let mut v = guarded("slot", || {
                sk::extrude(&pl, std::slice::from_ref(&region), bottom, top)
            })?;
            if v.len() != 1 {
                return Err("the slot tool is not one solid".into());
            }
            v.remove(0)
        } else {
            let base = sg::Plane::new(v3(&(origin + k * top)), d3(&u), d3(&w))
                .ok_or("degenerate slot plane")?;
            let len = top - bottom;
            let region_b = region.clone();
            guarded("slot", move || {
                sk::extrude_tapered(&base, &region_b, len, -1.0, ((rb - ra) / len).atan())
            })?
        };
        let tapered = (ra - rb).abs() >= 1e-9;
        let face_names = names_from(&body, &[], &|o| match o {
            FaceOrigin::Profile { segment } => walls.get(*segment).cloned(),
            // A prism's start cap lies at its lower offset; a tapered one's at
            // its base (the top here).
            FaceOrigin::Cap { end } => {
                if *end != tapered {
                    top_cap.clone()
                } else {
                    bottom_cap.clone()
                }
            }
            _ => None,
        });
        pieces.push(ExactSolid {
            prism: None,
            sheet: false,
            name: Some(prefix.to_owned()),
            body,
            face_names,
            profiles: Vec::new(),
        });
    }
    let mut it = pieces.into_iter();
    let mut tool = it.next().ok_or("the slot profile has no wall")?;
    for p in it {
        tool = boolean(&tool, &p, ExactOp::Union)?
            .ok_or_else(|| "the slot tool's bands did not join".to_owned())?;
    }
    Ok(tool)
}
