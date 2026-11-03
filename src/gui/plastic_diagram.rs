//! The live picture of a plastic command's panel (`docs/spec/5.8-plastic-parts.md`): the section of
//! what the command builds, to scale, from the panel's current values, with its sizes as dimensions.
//! A click on a dimension's text edits the same value as the field below (rule 13: the same field).

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, vec2};

use super::part_planned::{PartDialog, PartPlanned, Planned};
use super::valuefield::ExprValue;

type P = [f64; 2];

/// A value shown in the picture.
struct Dim {
    /// Tab of its field.
    tab: usize,
    /// The field's label, shown too.
    label: &'static str,
    /// What it measures; equal ends: only its text, in the row under the section.
    a: P,
    b: P,
    /// The dimension line's distance from `a`–`b`, in pixels, to the left of a → b.
    off: f32,
    /// The field's suffix.
    unit: &'static str,
}

/// A section: what is there already, what the command makes, and its sizes.
#[derive(Default)]
struct Pic {
    old: Vec<Vec<P>>,
    new: Vec<Vec<P>>,
    /// Dashed lines (an axis, the sketch's plane).
    dashed: Vec<(P, P)>,
    /// Plain words on the picture.
    notes: Vec<(P, &'static str)>,
    /// Marked points (the path).
    marks: Vec<P>,
    /// A line over the picture: why this tab's values have no effect now.
    hint: Option<&'static str>,
    dims: Vec<Dim>,
}

impl Pic {
    fn dim(&mut self, field: (usize, &'static str), unit: &'static str, a: P, b: P, off: f32) {
        self.dims.push(Dim {
            tab: field.0,
            label: field.1,
            a,
            b,
            off,
            unit,
        });
    }

    /// A value with no line of its own (an angle, a size that is zero now).
    fn chip(&mut self, field: (usize, &'static str), unit: &'static str) {
        self.dim(field, unit, [0.0; 2], [0.0; 2], 0.0);
    }

    fn length(&mut self, field: (usize, &'static str), a: P, b: P, off: f32) {
        self.dim(field, " mm", a, b, off);
    }

    fn angle(&mut self, field: (usize, &'static str)) {
        self.chip(field, "°");
    }
}

fn tan_deg(v: f64) -> f64 {
    v.to_radians().tan()
}

fn lip(d: &PartDialog) -> Pic {
    let n = |l| d.num(1, l).unwrap_or(0.0);
    let (s, w, h, c) = (n("Shoulder Width"), n("Width"), n("Height"), n("Clearance"));
    let (ti, to) = (tan_deg(n("Inside Angle")), tan_deg(n("Outside Angle")));
    let groove = d.index(0, "Type") == 1;
    // The wall round the bead is cut short: the bead's sizes are the point of the picture.
    let rim = s + w + w.max(0.5);
    let deep = if groove {
        (h + c) * 1.5
    } else {
        c + h.max(0.5)
    };
    let mut p = Pic::default();
    if groove {
        let dp = h + c;
        let (x0, x1) = (s - dp * ti, s + w + dp * to);
        p.old.push(vec![
            [0.0, -deep],
            [rim, -deep],
            [rim, 0.0],
            [x1, 0.0],
            [s + w, -dp],
            [s, -dp],
            [x0, 0.0],
            [0.0, 0.0],
        ]);
        p.new
            .push(vec![[x0, 0.0], [s, -dp], [s + w, -dp], [x1, 0.0]]);
        p.length((1, "Height"), [x1, 0.0], [x1, -h], -14.0);
        p.length((1, "Width"), [s, 0.0], [s + w, 0.0], 14.0);
    } else {
        let mut wall = vec![[0.0, -deep], [rim, -deep], [rim, -c]];
        if c > 0.0 {
            wall.push([s + w, -c]);
        }
        wall.push([s + w, 0.0]);
        wall.push([0.0, 0.0]);
        p.old.push(wall);
        p.new.push(vec![
            [s, 0.0],
            [s + w, 0.0],
            [s + w - h * to, h],
            [s + h * ti, h],
        ]);
        p.length((1, "Height"), [s + w, 0.0], [s + w, h], -16.0);
        p.length((1, "Width"), [s, h], [s + w, h], 14.0);
    }
    if s > 0.0 {
        p.length((1, "Shoulder Width"), [0.0, 0.0], [s, 0.0], 12.0);
    } else {
        p.chip((1, "Shoulder Width"), " mm");
    }
    if c > 0.0 && !groove {
        p.length((1, "Clearance"), [rim, 0.0], [rim, -c], -12.0);
    } else {
        p.chip((1, "Clearance"), " mm");
    }
    p.angle((1, "Inside Angle"));
    p.angle((1, "Outside Angle"));
    p.marks.push([0.0, 0.0]);
    p.notes.push(([0.0, -deep], "Path"));
    p.notes.push(([rim * 0.6, -deep], "Rim"));
    p
}

/// A boss's section is symmetric: `half` is its right side from the axis, top first.
fn both_sides(half: &[P]) -> Vec<P> {
    let mut v: Vec<P> = half.iter().map(|q| [-q[0], q[1]]).collect();
    v.reverse();
    v.extend_from_slice(half);
    v
}

fn boss(d: &PartDialog) -> Pic {
    let head = d.index(0, "Type") == 0;
    let mut p = Pic::default();
    let hd = |l| d.num(1, l).unwrap_or(0.0);
    let th = |l| d.num(2, l).unwrap_or(0.0);
    let (rs, rc, rh) = (
        hd("Shank Diameter") / 2.0,
        hd("Clamp Diameter") / 2.0,
        hd("Head Diameter") / 2.0,
    );
    let (sh, ch) = (hd("Shank Height"), hd("Clamp Height"));
    let r0 = if head {
        rh + hd("Wall Thickness")
    } else {
        th("Thread Diameter") / 2.0
    };
    let tab_draft = if head { 1 } else { 2 };
    let tan_o = tan_deg(d.num(tab_draft, "Outer Draft Angle").unwrap_or(0.0));
    // The height is not known before the wall is reached: a typical one, for the picture.
    let e = (r0 * 3.0)
        .max(sh + ch + 4.0)
        .max(th("Thread Hole Depth") + 2.0);
    let wall = (r0 * 0.5).max(1.5);
    let far = r0 * 2.6;
    let r1 = r0 + e * tan_o;
    p.old.push(vec![
        [-far, -e],
        [-r1, -e],
        [r1, -e],
        [far, -e],
        [far, -e - wall],
        [-far, -e - wall],
    ]);
    if head {
        let top = sh + ch;
        let tan_i = tan_deg(hd("Inner Draft Angle"));
        let rf = rh + (e + wall - sh) * tan_i;
        let rs0 = rs + top * tan_deg(hd("Shaft Draft Angle"));
        let countersink = d.index(1, "Seat") == 1;
        // The material of the section's right half: the post between its outside and the cut.
        let right: Vec<P> = if countersink {
            let rise = (rh - rs) / (hd("Countersink Angle").to_radians() / 2.0).tan().max(1e-3);
            vec![
                [rs0, 0.0],
                [r0, 0.0],
                [r1, -e],
                [rf, -e - wall],
                [rh, -sh - rise],
                [rs, -sh],
            ]
        } else {
            let rc_top = rc - ch * tan_deg(hd("Clamp Draft Angle"));
            vec![
                [rs0, 0.0],
                [r0, 0.0],
                [r1, -e],
                [rf, -e - wall],
                [rh, -sh],
                [rc, -sh],
                [rc_top, -top],
                [rs, -top],
            ]
        };
        let left: Vec<P> = right.iter().map(|q| [-q[0], q[1]]).collect();
        p.new.push(right);
        p.new.push(left);
        if d.tab() == 1 {
            p.length(
                (1, "Head Diameter"),
                [-rh, -e - wall],
                [rh, -e - wall],
                -14.0,
            );
            p.length((1, "Shank Diameter"), [-rs0, 0.0], [rs0, 0.0], 14.0);
            p.length((1, "Wall Thickness"), [rh, -e * 0.6], [r0, -e * 0.6], 12.0);
            p.length((1, "Shank Height"), [-r0, 0.0], [-r0, -sh], 14.0);
            if countersink {
                p.angle((1, "Countersink Angle"));
            } else {
                p.length((1, "Clamp Diameter"), [-rc, -top], [rc, -top], -12.0);
                p.chip((1, "Clamp Height"), " mm");
                p.angle((1, "Clamp Draft Angle"));
            }
            p.angle((1, "Outer Draft Angle"));
            p.angle((1, "Inner Draft Angle"));
            p.angle((1, "Shaft Draft Angle"));
        }
    } else {
        let rh2 = th("Thread Hole Diameter") / 2.0;
        let hole = d.check(2, "Hole");
        let depth = match d.index(2, "Hole Type") {
            0 => th("Thread Hole Depth"),
            1 => e,
            _ => e + wall,
        };
        let tan_i = tan_deg(th("Inner Draft Angle"));
        if hole {
            let r_top = rh2 + depth * tan_i;
            let right = vec![
                [r_top, 0.0],
                [r0, 0.0],
                [r1, -e],
                [r1, -e - wall * 0.25],
                [rh2, -e - wall * 0.25],
                [rh2, -depth],
            ];
            let left: Vec<P> = right.iter().map(|q| [-q[0], q[1]]).collect();
            p.new.push(right);
            p.new.push(left);
            if d.tab() == 2 {
                p.length(
                    (2, "Thread Hole Diameter"),
                    [-rh2, -depth],
                    [rh2, -depth],
                    -12.0,
                );
                if d.index(2, "Hole Type") == 0 {
                    p.length((2, "Thread Hole Depth"), [0.0, 0.0], [0.0, -depth], 0.0);
                }
                p.angle((2, "Inner Draft Angle"));
            }
        } else {
            p.new
                .push(both_sides(&[[r0, 0.0], [r1, -e], [r1, -e - wall * 0.25]]));
        }
        if d.tab() == 2 {
            p.length((2, "Thread Diameter"), [-r0, 0.0], [r0, 0.0], 14.0);
            p.angle((2, "Outer Draft Angle"));
        }
    }
    if d.tab() == 3 || d.tab() == 0 {
        // A rib in the section's plane, on the right.
        if d.check(3, "Stiffening Ribs") {
            let rib = |l| d.num(3, l).unwrap_or(0.0);
            let (z0, sl) = (rib("Top Offset"), rib("Shoulder Length"));
            let fl = tan_deg(rib("Flare Angle"));
            let at = |z: f64| r0 + z * tan_o;
            p.new.push(vec![
                [at(z0), -z0],
                [at(z0) + sl, -z0],
                [at(e) + sl + (e - z0) * fl, -e],
                [at(e), -e],
            ]);
            if d.tab() == 3 {
                p.length(
                    (3, "Shoulder Length"),
                    [at(z0), -z0],
                    [at(z0) + sl, -z0],
                    12.0,
                );
                p.length((3, "Top Offset"), [-r0, 0.0], [-r0, -z0], 14.0);
                p.angle((3, "Flare Angle"));
                p.chip((3, "Rib Thickness"), " mm");
                p.chip((3, "Number of Ribs"), "");
                p.angle((3, "Rib Draft"));
            }
        }
    }
    if d.tab() == 0 {
        let off = d.num(0, "Offset").unwrap_or(0.0);
        if off > 0.0 {
            p.length((0, "Offset"), [-r0, -off], [-r0, 0.0], 14.0);
        } else {
            p.chip((0, "Offset"), " mm");
        }
        p.dashed.push(([-far, -off], [far, -off]));
        p.notes.push(([far, -off], "Sketch"));
        p.chip((0, "Fillet"), " mm");
    }
    p.dashed.push(([0.0, r0 * 0.4], [0.0, -e - wall * 1.2]));
    p
}

fn rest(d: &PartDialog) -> Pic {
    let mut p = Pic::default();
    let t = d.num(0, "Thickness").unwrap_or(1.0);
    let w = (t * 10.0).max(14.0);
    let side = d.index(0, "Thickness Side");
    // The wall's outline against the sketched one at 0 and w.
    let (o, i) = match side {
        0 => (0.0, t),
        1 => (-t, 0.0),
        _ => (-t / 2.0, t / 2.0),
    };
    let plate = 3.0;
    let ld = d.num(1, "Landing Distance").unwrap_or(0.0);
    let land = if d.index(1, "Landing") == 0 { ld } else { 0.0 };
    let (sketch, top) = (7.0, 7.0 + land);
    let tl = tan_deg(d.num(1, "Landing Taper").unwrap_or(0.0));
    let platform = d.index(0, "Platform");
    let bottom = match platform {
        1 => top - d.num(0, "Distance").unwrap_or(2.0),
        2 => (top - t - 1.5).max(0.5),
        _ => -plate / 2.0,
    };
    let g = (top - bottom) * tl;
    let far = 5.0;
    p.old
        .push(vec![[-far, -plate], [i, -plate], [i, 0.0], [-far, 0.0]]);
    p.old.push(vec![
        [w - i, -plate],
        [w + far, -plate],
        [w + far, 0.0],
        [w - i, 0.0],
    ]);
    p.new
        .push(vec![[o, top], [w - o, top], [w - o, top - t], [o, top - t]]);
    p.new.push(vec![
        [o, top - t],
        [i, top - t],
        [i + g, bottom],
        [o + g, bottom],
    ]);
    p.new.push(vec![
        [w - i, top - t],
        [w - o, top - t],
        [w - o - g, bottom],
        [w - i - g, bottom],
    ]);
    p.dashed.push(([-far, sketch], [w + far, sketch]));
    p.notes.push(([w + far, sketch], "Sketch"));
    if d.tab() == 0 {
        p.length(
            (0, "Thickness"),
            [o, top - t * 2.0],
            [i, top - t * 2.0],
            10.0,
        );
        match platform {
            1 => p.length((0, "Distance"), [w - o, top], [w - o, bottom], 14.0),
            2 => p.chip((0, "Offset"), " mm"),
            _ => {}
        }
    } else {
        if d.index(1, "Landing") == 0 {
            if ld > 0.0 {
                p.length((1, "Landing Distance"), [-far, sketch], [-far, top], -12.0);
            } else {
                p.chip((1, "Landing Distance"), " mm");
            }
        } else {
            p.chip((1, "Landing Offset"), " mm");
        }
        p.angle((1, "Landing Taper"));
        p.angle((1, "Clearance Taper"));
    }
    p
}

fn grill(d: &PartDialog) -> Pic {
    let mut p = Pic::default();
    let n = |t, l| d.num(t, l).unwrap_or(0.0);
    let (t, h, oh) = (n(0, "Thickness"), n(0, "Height"), n(0, "Outside Height"));
    let tan = tan_deg(n(4, "Draft Angle"));
    let w = 2.0 * t + 10.0;
    let (top, bot) = (oh, oh - h);
    let plate = 2.0;
    let far = 2.0;
    p.old
        .push(vec![[-far, -plate], [t, -plate], [t, 0.0], [-far, 0.0]]);
    p.old.push(vec![
        [w - t, -plate],
        [w + far, -plate],
        [w + far, 0.0],
        [w - t, 0.0],
    ]);
    let g = h * tan;
    p.new
        .push(vec![[0.0, top], [t, top], [t + g, bot], [-g, bot]]);
    p.new
        .push(vec![[w - t, top], [w, top], [w + g, bot], [w - t - g, bot]]);
    let bar = |c: f64, wd: f64, y0: f64, y1: f64| {
        let g = (y0 - y1).abs() * tan;
        vec![
            [c - wd / 2.0, y0],
            [c + wd / 2.0, y0],
            [c + wd / 2.0 + g, y1],
            [c - wd / 2.0 - g, y1],
        ]
    };
    let (rt, rh, ro) = (n(2, "Thickness"), n(2, "Height"), n(2, "Top Offset"));
    let (st, so, bo) = (n(3, "Thickness"), n(3, "Top Offset"), n(3, "Bottom Offset"));
    let (r0, r1) = (top - ro, top - ro - rh);
    let (s0, s1) = (r0 - so, r1 - bo);
    let (cr, cs, ci) = (t + 5.0, t + 2.0, t + 8.0);
    p.new.push(bar(cr, rt, r0, r1));
    if s0 > s1 {
        p.new.push(bar(cs, st, s0, s1));
    }
    p.new.push(bar(ci, 2.0, top, bot));
    p.notes.push(([cr, r1], "Rib"));
    p.notes.push(([cs, s1], "Spar"));
    p.notes.push(([ci, bot], "Island"));
    match d.tab() {
        0 => {
            p.length((0, "Thickness"), [0.0, top], [t, top], 12.0);
            p.length((0, "Height"), [w, top], [w, bot], 14.0);
            if oh > 0.0 {
                p.length((0, "Outside Height"), [-far, 0.0], [-far, oh], -12.0);
            } else {
                p.chip((0, "Outside Height"), " mm");
            }
        }
        1 => p.chip((1, "Thickness"), " mm"),
        2 => {
            p.length(
                (2, "Thickness"),
                [cr - rt / 2.0, r0],
                [cr + rt / 2.0, r0],
                12.0,
            );
            p.length(
                (2, "Height"),
                [cr + rt / 2.0, r0],
                [cr + rt / 2.0, r1],
                14.0,
            );
            if ro > 0.0 {
                p.length((2, "Top Offset"), [cr - rt, top], [cr - rt, r0], 12.0);
            } else {
                p.chip((2, "Top Offset"), " mm");
            }
        }
        3 => {
            p.length(
                (3, "Thickness"),
                [cs - st / 2.0, s0],
                [cs + st / 2.0, s0],
                12.0,
            );
            p.chip((3, "Top Offset"), " mm");
            p.chip((3, "Bottom Offset"), " mm");
        }
        _ => p.angle((4, "Draft Angle")),
    }
    p
}

fn snap_hook(d: &PartDialog) -> Pic {
    let mut p = Pic::default();
    let b = |l| d.num(1, l).unwrap_or(0.0);
    let k = |l| d.num(2, l).unwrap_or(0.0);
    let (tw, th, l) = (
        b("Thickness at Wall"),
        b("Thickness at Hook"),
        b("Beam Length"),
    );
    let (tip, u, hl) = (k("Tip Length"), k("Undercut Depth"), k("Hook Length"));
    let rise = |a: f64| {
        if (a - 90.0).abs() < 1e-9 {
            0.0
        } else {
            u / a.to_radians().tan()
        }
    };
    let half = |z: f64| (tw + (th - tw) * z / l.max(1e-6)) / 2.0;
    let z_r = l - hl;
    let z_land = z_r + rise(k("Retention Angle"));
    let z_tip = l - tip;
    let z_ramp = z_tip - rise(k("Insertion Angle"));
    let top_x = half(z_r) + u;
    p.old.push(vec![
        [-tw * 2.5, -2.0],
        [top_x + tw * 2.0, -2.0],
        [top_x + tw * 2.0, 0.0],
        [-tw * 2.5, 0.0],
    ]);
    p.new.push(vec![
        [-tw / 2.0, 0.0],
        [tw / 2.0, 0.0],
        [half(z_r), z_r],
        [top_x, z_land],
        [top_x, z_ramp],
        [half(z_tip), z_tip],
        [th / 2.0, l],
        [-th / 2.0, l],
    ]);
    // The beam from the front: its widths.
    let (ww, wh) = (b("Width at Wall"), b("Width at Hook"));
    let c = top_x + tw * 2.0 + ww.max(wh) / 2.0 + 2.0;
    if d.tab() == 1 {
        p.new.push(vec![
            [c - ww / 2.0, 0.0],
            [c + ww / 2.0, 0.0],
            [c + wh / 2.0, l],
            [c - wh / 2.0, l],
        ]);
        p.length(
            (1, "Width at Wall"),
            [c - ww / 2.0, 0.0],
            [c + ww / 2.0, 0.0],
            -14.0,
        );
        p.length(
            (1, "Width at Hook"),
            [c - wh / 2.0, l],
            [c + wh / 2.0, l],
            12.0,
        );
        p.length(
            (1, "Thickness at Wall"),
            [-tw / 2.0, 0.0],
            [tw / 2.0, 0.0],
            -14.0,
        );
        p.length(
            (1, "Thickness at Hook"),
            [-th / 2.0, l],
            [th / 2.0, l],
            12.0,
        );
        p.length((1, "Beam Length"), [-tw / 2.0, 0.0], [-th / 2.0, l], -14.0);
    } else if d.tab() == 2 {
        p.length((2, "Tip Length"), [top_x, z_tip], [top_x, l], -12.0);
        p.length((2, "Undercut Depth"), [half(z_r), z_r], [top_x, z_r], -12.0);
        p.length((2, "Hook Length"), [-th / 2.0, z_r], [-th / 2.0, l], -14.0);
        p.angle((2, "Retention Angle"));
        p.angle((2, "Insertion Angle"));
    }
    p
}

fn snap_loop(d: &PartDialog) -> Pic {
    let mut p = Pic::default();
    let c = |l| d.num(3, l).unwrap_or(0.0);
    let k = |l| d.num(4, l).unwrap_or(0.0);
    let (l, w) = (c("Clip Length"), c("Clip Width"));
    let (cs, ct, op) = (k("Side Width"), k("Top Width"), k("Opening Length"));
    p.old.push(vec![
        [-w * 0.9, -2.0],
        [w * 0.9, -2.0],
        [w * 0.9, 0.0],
        [-w * 0.9, 0.0],
    ]);
    let hw = w / 2.0;
    let (y1, y0) = (l - ct, l - ct - op);
    let hole = hw - cs;
    // The clip from the front, in four strips round its opening.
    p.new.push(vec![[-hw, 0.0], [hw, 0.0], [hw, y0], [-hw, y0]]);
    p.new
        .push(vec![[-hw, y0], [-hole, y0], [-hole, y1], [-hw, y1]]);
    p.new.push(vec![[hole, y0], [hw, y0], [hw, y1], [hole, y1]]);
    p.new.push(vec![[-hw, y1], [hw, y1], [hw, l], [-hw, l]]);
    if d.tab() == 3 {
        p.length((3, "Clip Width"), [-hw, l], [hw, l], 12.0);
        p.length((3, "Clip Length"), [-hw, 0.0], [-hw, l], 14.0);
        p.chip((3, "Thickness at Wall"), " mm");
        p.chip((3, "Thickness at Top"), " mm");
    } else {
        p.length(
            (4, "Side Width"),
            [hole, (y0 + y1) / 2.0],
            [hw, (y0 + y1) / 2.0],
            -12.0,
        );
        p.length((4, "Top Width"), [hw, y1], [hw, l], -14.0);
        p.length((4, "Opening Length"), [-hw, y0], [-hw, y1], 14.0);
    }
    p
}

/// Why the values of the tab on show do nothing with the options chosen.
fn unused_tab(d: &PartDialog, k: PartPlanned) -> Option<&'static str> {
    match (k, d.tab()) {
        (PartPlanned::Boss, 1) if d.index(0, "Type") != 0 => {
            Some("Not used: Type on the Shape tab is Thread")
        }
        (PartPlanned::Boss, 2) if d.index(0, "Type") != 1 => {
            Some("Not used: Type on the Shape tab is Head")
        }
        (PartPlanned::Boss, 3) if !d.check(3, "Stiffening Ribs") => {
            Some("Turn on Stiffening Ribs below to use these")
        }
        (PartPlanned::SnapFit, 1 | 2) if d.index(0, "Type") != 0 => {
            Some("Not used: Type on the Shape tab is Loop")
        }
        (PartPlanned::SnapFit, 3 | 4) if d.index(0, "Type") != 1 => {
            Some("Not used: Type on the Shape tab is Hook")
        }
        _ => None,
    }
}

/// The picture of the panel's command, if it has one.
fn picture(d: &PartDialog) -> Option<Pic> {
    let Planned::Part(k) = d.kind else {
        return None;
    };
    let hint = unused_tab(d, k);
    let mut pic = picture_of(d, k)?;
    pic.hint = hint;
    Some(pic)
}

fn picture_of(d: &PartDialog, k: PartPlanned) -> Option<Pic> {
    Some(match k {
        PartPlanned::Lip => lip(d),
        PartPlanned::Boss => boss(d),
        PartPlanned::Rest => rest(d),
        PartPlanned::Grill => grill(d),
        PartPlanned::SnapFit => {
            if d.index(0, "Type") == 0 {
                snap_hook(d)
            } else {
                snap_loop(d)
            }
        }
        _ => return None,
    })
}

/// Triangles of a simple polygon (ear clipping; the outlines here have few points).
fn triangles(poly: &[Pos2]) -> Vec<[Pos2; 3]> {
    let area: f32 = (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    let turn = |a: Pos2, b: Pos2, c: Pos2| (b - a).x * (c - a).y - (b - a).y * (c - a).x;
    let mut idx: Vec<usize> = (0..poly.len()).collect();
    let mut out = Vec::new();
    let mut guard = 0;
    while idx.len() > 2 && guard < 200 {
        guard += 1;
        let n = idx.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (
                poly[idx[(i + n - 1) % n]],
                poly[idx[i]],
                poly[idx[(i + 1) % n]],
            );
            if turn(a, b, c) * area <= 0.0 {
                return false;
            }
            !idx.iter().any(|&j| {
                let q = poly[j];
                q != a && q != b && q != c && {
                    let (x, y, z) = (turn(a, b, q), turn(b, c, q), turn(c, a, q));
                    x * area >= 0.0 && y * area >= 0.0 && z * area >= 0.0
                }
            })
        });
        let Some(i) = ear else {
            break;
        };
        out.push([
            poly[idx[(i + n - 1) % n]],
            poly[idx[i]],
            poly[idx[(i + 1) % n]],
        ]);
        idx.remove(i);
    }
    out
}

fn fill(painter: &egui::Painter, poly: &[Pos2], color: Color32, edge: Color32) {
    let mut mesh = egui::Mesh::default();
    for t in triangles(poly) {
        let k = mesh.vertices.len() as u32;
        for q in t {
            mesh.colored_vertex(q, color);
        }
        mesh.add_triangle(k, k + 1, k + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
    let mut closed = poly.to_vec();
    if let Some(first) = poly.first() {
        closed.push(*first);
    }
    painter.add(egui::Shape::line(closed, Stroke::new(1.0, edge)));
}

/// Draws the picture of the panel's command under its tabs.
pub(super) fn show(d: &mut PartDialog, ui: &mut egui::Ui) {
    let Some(pic) = picture(d) else {
        return;
    };
    let w = ui.available_width().max(200.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, 230.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, Color32::from_rgb(28, 31, 37));
    // To scale: the whole section in the room the dimensions leave.
    let all = pic.old.iter().chain(&pic.new).flatten().copied().chain(
        pic.dims
            .iter()
            .filter(|x| x.a != x.b)
            .flat_map(|x| [x.a, x.b]),
    );
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for q in all {
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    // The values without a line of their own take a strip at the bottom.
    let strip = 44.0;
    let mut room = rect.shrink2(vec2(46.0, 26.0));
    room.max.y = (room.max.y - strip + 8.0).max(room.min.y + 40.0);
    let mut row = Pos2::new(rect.left() + 4.0, rect.bottom() - strip);
    let scale = ((room.width() as f64 / (hi[0] - lo[0]).max(1e-6))
        .min(room.height() as f64 / (hi[1] - lo[1]).max(1e-6))) as f32;
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let at = |q: P| {
        room.center()
            + vec2(
                (q[0] - mid[0]) as f32 * scale,
                -((q[1] - mid[1]) as f32) * scale,
            )
    };
    let screen = |v: &[P]| v.iter().map(|q| at(*q)).collect::<Vec<Pos2>>();
    for s in &pic.old {
        fill(
            &painter,
            &screen(s),
            Color32::from_rgb(82, 88, 98),
            Color32::from_rgb(140, 146, 156),
        );
    }
    for s in &pic.new {
        fill(
            &painter,
            &screen(s),
            Color32::from_rgb(214, 124, 44),
            Color32::from_rgb(250, 180, 100),
        );
    }
    for (a, b) in &pic.dashed {
        painter.add(egui::Shape::dashed_line(
            &[at(*a), at(*b)],
            Stroke::new(1.0, Color32::from_gray(170)),
            5.0,
            4.0,
        ));
    }
    for q in &pic.marks {
        painter.circle_filled(at(*q), 4.0, Color32::from_rgb(80, 150, 250));
    }
    let small = egui::FontId::proportional(11.0);
    let mut words: Vec<Rect> = Vec::new();
    for (q, text) in &pic.notes {
        let galley =
            painter.layout_no_wrap((*text).to_owned(), small.clone(), Color32::from_gray(225));
        let place = Rect::from_center_size(
            at(*q) + vec2(0.0, 3.0 + galley.size().y / 2.0),
            galley.size(),
        );
        // A narrow section has no room for every word.
        if words.iter().any(|w| w.expand(2.0).intersects(place)) {
            continue;
        }
        words.push(place);
        painter.galley(place.min, galley, Color32::from_gray(225));
    }
    if let Some(h) = pic.hint {
        painter.text(
            rect.center_top() + vec2(0.0, 5.0),
            egui::Align2::CENTER_TOP,
            h,
            small.clone(),
            Color32::from_rgb(240, 190, 90),
        );
    }
    let ink = Color32::from_rgb(150, 210, 255);
    let line = Stroke::new(1.0, ink);
    let mut clicked = None;
    let mut labels: Vec<Rect> = Vec::new();
    for dim in &pic.dims {
        // What does nothing with the options chosen has no value to show.
        if let Planned::Part(k) = d.kind
            && super::plastic::row_off(d, k, dim.tab, dim.label)
        {
            continue;
        }
        let (a, b) = (at(dim.a), at(dim.b));
        let along = b - a;
        let lined = dim.a != dim.b;
        let mut place = a;
        if lined && along.length() > 0.5 {
            let dir = along.normalized();
            // The left of a → b, on the screen (y down).
            let left = vec2(dir.y, -dir.x);
            let (p, q) = (a + left * dim.off, b + left * dim.off);
            if dim.off != 0.0 {
                painter.line_segment([a, p + left * 2.0 * dim.off.signum()], line);
                painter.line_segment([b, q + left * 2.0 * dim.off.signum()], line);
            }
            painter.line_segment([p, q], line);
            for (end, s) in [(p, 1.0), (q, -1.0)] {
                let tip = dir * 6.0 * s;
                painter.line_segment([end, end + tip + left * 2.5], line);
                painter.line_segment([end, end + tip - left * 2.5], line);
            }
            place = p + (q - p) / 2.0 + left * 9.0 * dim.off.signum();
        }
        let value = d.num(dim.tab, dim.label).unwrap_or(0.0);
        let text = format!(
            "{} {}{}",
            dim.label,
            (value * 1000.0).round() / 1000.0,
            dim.unit
        );
        let galley = painter.layout_no_wrap(text, small.clone(), ink);
        let size = galley.size() + vec2(6.0, 2.0);
        let mut chip = if lined {
            Rect::from_center_size(place, size)
        } else {
            if row.x + size.x > rect.right() - 2.0 {
                row = Pos2::new(rect.left() + 4.0, row.y + size.y + 3.0);
            }
            let r = Rect::from_min_size(row, size);
            row.x += size.x + 4.0;
            r
        };
        // Inside the picture, and off the labels drawn before it.
        chip = chip.translate(vec2(
            (rect.left() + 2.0 - chip.left()).max(0.0)
                + (rect.right() - 2.0 - chip.right()).min(0.0),
            (rect.top() + 2.0 - chip.top()).max(0.0)
                + (rect.bottom() - 2.0 - chip.bottom()).min(0.0),
        ));
        let mut tries = 0;
        while tries < 8 && labels.iter().any(|l| l.intersects(chip)) {
            chip = chip.translate(vec2(0.0, chip.height() + 1.0));
            tries += 1;
        }
        labels.push(chip);
        let id = ui.id().with(("dim", dim.tab, dim.label));
        let r = ui
            .interact(chip, id, Sense::click())
            .on_hover_text(format!("{}: click to change", dim.label));
        painter.rect_filled(
            chip,
            2.0,
            if r.hovered() {
                Color32::from_rgb(60, 92, 140)
            } else {
                Color32::from_rgb(32, 44, 62)
            },
        );
        painter.galley(chip.min + vec2(3.0, 1.0), galley, ink);
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if r.clicked() {
            clicked = Some((dim.tab, dim.label, dim.unit, chip));
        }
    }
    if let Some((tab, label, unit, chip)) = clicked {
        d.diagram_edit = Some((tab, label, unit, chip, 2));
    }
    edit_box(d, ui, rect);
    ui.label(
        RichText::new("Click a dimension to change it")
            .small()
            .weak(),
    );
    ui.separator();
}

/// The box over a clicked dimension: the same field as the panel's row.
fn edit_box(d: &mut PartDialog, ui: &mut egui::Ui, frame: Rect) {
    let Some((tab, label, unit, chip, stage)) = d.diagram_edit else {
        return;
    };
    let w = 110.0_f32.min(frame.width());
    let x = chip.left().clamp(frame.left(), frame.right() - w);
    let place = Rect::from_min_size(Pos2::new(x, chip.top() - 2.0), vec2(w, 24.0));
    let Some(v) = d.num_mut(tab, label) else {
        d.diagram_edit = None;
        return;
    };
    // Over the picture, taking no room in the panel's layout.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(place));
    let field = child.next_auto_id();
    let text = field.with("te");
    child.add(ExprValue::new(v).suffix(unit));
    // Two frames to open: the focus, then the whole text selected so typing replaces it.
    match stage {
        2 => child.ctx().memory_mut(|m| m.request_focus(text)),
        1 => {
            let mut st = egui::TextEdit::load_state(child.ctx(), text).unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(usize::MAX / 2),
            )));
            st.store(child.ctx(), text);
        }
        _ => {}
    }
    // Enter, Esc or a click elsewhere take the focus away: the field has its value by then.
    let done = stage < 2 && !child.ctx().memory(|m| m.has_focus(text));
    d.diagram_edit = (!done).then_some((tab, label, unit, chip, stage.saturating_sub(1)));
}
