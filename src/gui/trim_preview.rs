//! Trim's preview (`docs/spec/5.4a-surface-workflow.md` T1): once the cutting tool is picked,
//! the side of the surface that OK would remove is drawn see-through red — the side under the
//! cursor while *Remove* is still empty, then the picked one (Invert Selection swaps them), as
//! Inventor highlights the areas to remove before OK.
//!
//! The sides are told from the display triangles (each triangle by its centre against the
//! tool), so the preview is instant; the kernel's cut runs only at OK.

use eframe::egui;

use super::CadCraftApp;
use super::part_planned::{PartPlanned, Planned};
use super::select::SelItem;
use crate::kernel::{Point3, Real};
use crate::part::PlaneRef;

/// Which side of the cutting tool a point lies on (the sign matters).
pub(super) enum Side {
    /// A plane: the signed distance.
    Plane(crate::kernel::Plane),
    /// A surface body: the signed distance to its triangles.
    Sheet(crate::kernel::exact::SheetSide),
    /// Sketch curves (their polylines in the sketch plane): the side of the nearest segment,
    /// or for closed loops (`true`) inside (positive) or outside — the curves of a rectangle
    /// run whichever way they were drawn.
    Curves(crate::kernel::Plane, Vec<[nalgebra::Point2<Real>; 2]>, bool),
}

impl Side {
    fn of(&self, q: &Point3) -> Real {
        match self {
            Side::Plane(pl) => (q - pl.origin).dot(&pl.normal.into_inner()),
            Side::Sheet(s) => s.signed(q),
            Side::Curves(pl, segs, closed) => {
                let p = pl.project(q);
                if *closed {
                    let mut inside = false;
                    let mut best = Real::MAX;
                    for [a, b] in segs {
                        let ab = b - a;
                        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * ab.x / ab.y {
                            inside = !inside;
                        }
                        let t = ((p - a).dot(&ab) / ab.norm_squared().max(1e-24)).clamp(0.0, 1.0);
                        best = best.min((a + ab * t - p).norm());
                    }
                    return if inside { best } else { -best };
                }
                let mut best: Option<(Real, Real)> = None;
                for [a, b] in segs {
                    let ab = b - a;
                    let t = ((p - a).dot(&ab) / ab.norm_squared().max(1e-24)).clamp(0.0, 1.0);
                    let d = (a + ab * t - p).norm();
                    if best.is_none_or(|(x, _)| d < x) {
                        best = Some((d, ab.x * (p.y - a.y) - ab.y * (p.x - a.x)));
                    }
                }
                best.map_or(0.0, |(_, s)| s)
            }
        }
    }
}

/// The part of triangle `t` on the `gone` side of `side` (positive when `gone`), cut where the
/// side value changes sign (linear between the corners: exact for a plane), as triangles.
fn clip_triangle(
    t: [Point3; 3],
    side: impl Fn(&Point3) -> Real,
    gone: bool,
    out: &mut Vec<[Point3; 3]>,
) {
    let v: Vec<Real> = t
        .iter()
        .map(|q| if gone { side(q) } else { -side(q) })
        .collect();
    if v.iter().all(|x| *x >= 0.0) {
        out.push(t);
        return;
    }
    if v.iter().all(|x| *x <= 0.0) {
        return;
    }
    // Sutherland–Hodgman against "value >= 0".
    let mut poly: Vec<Point3> = Vec::with_capacity(4);
    for k in 0..3 {
        let (p, q) = (t[k], t[(k + 1) % 3]);
        let (a, b) = (v[k], v[(k + 1) % 3]);
        if a >= 0.0 {
            poly.push(p);
        }
        if (a >= 0.0) != (b >= 0.0) {
            let s = a / (a - b);
            poly.push(p + (q - p) * s);
        }
    }
    for k in 1..poly.len().saturating_sub(1) {
        out.push([poly[0], poly[k], poly[k + 1]]);
    }
}

fn centre(t: &[Point3; 3]) -> Point3 {
    Point3::from((t[0].coords + t[1].coords + t[2].coords) / 3.0)
}

/// Pieces of a triangle soup: triangles on the same side (`side`) sharing a corner belong to one
/// piece. Returns each triangle's piece number.
fn pieces(tris: &[[Point3; 3]], side: impl Fn(&[Point3; 3]) -> bool) -> Vec<usize> {
    let key = |p: &Point3| {
        (
            (p.x * 1e4).round() as i64,
            (p.y * 1e4).round() as i64,
            (p.z * 1e4).round() as i64,
        )
    };
    let sides: Vec<bool> = tris.iter().map(&side).collect();
    let mut by_corner: std::collections::HashMap<(i64, i64, i64), Vec<usize>> =
        std::collections::HashMap::new();
    for (k, t) in tris.iter().enumerate() {
        for p in t {
            by_corner.entry(key(p)).or_default().push(k);
        }
    }
    let mut label = vec![usize::MAX; tris.len()];
    let mut next = 0;
    for s in 0..tris.len() {
        if label[s] != usize::MAX {
            continue;
        }
        label[s] = next;
        let mut stack = vec![s];
        while let Some(k) = stack.pop() {
            for p in &tris[k] {
                for &j in by_corner.get(&key(p)).into_iter().flatten() {
                    if label[j] == usize::MAX && sides[j] == sides[k] {
                        label[j] = next;
                        stack.push(j);
                    }
                }
            }
        }
        next += 1;
    }
    label
}

impl CadCraftApp {
    /// The cutting tool picked in the Trim panel, as a side test.
    fn trim_tool_side(&self) -> Option<Side> {
        let d = self.part_planned.as_ref()?;
        let (item, at) = d.items_of("tool").first()?.clone();
        let part = self.part();
        let sheet = |name: &str| {
            part.exact_bodies
                .0
                .iter()
                .find(|s| s.is_open() && s.name.as_deref() == Some(name))
                .and_then(|s| s.triangles().ok())
                .map(|t| Side::Sheet(crate::kernel::exact::SheetSide::new(t)))
        };
        let plane = |r: &PlaneRef| {
            crate::part::placement::plane_of(r, &part.body, &part.work)
                .ok()
                .map(Side::Plane)
        };
        match &item {
            SelItem::Face(f) => match self.body_of_face(f, at) {
                Some(b)
                    if part
                        .exact_bodies
                        .0
                        .iter()
                        .any(|s| s.is_open() && s.name.as_deref() == Some(b.as_str())) =>
                {
                    sheet(&b)
                }
                _ => plane(&PlaneRef::Face(f.clone())),
            },
            SelItem::Body(b) => sheet(b),
            SelItem::Feature(id) if part.work.planes.contains_key(id) => {
                plane(&PlaneRef::Work(id.clone()))
            }
            SelItem::Sketch(i) => {
                let ps = part.sketches.get(*i)?;
                let sk = &ps.sketch;
                let segs: Vec<[nalgebra::Point2<Real>; 2]> = sk
                    .entities()
                    .filter(|(e, _)| {
                        !ps.is_reference(*e) && sk.style(*e) == crate::sketch::Style::Normal
                    })
                    .flat_map(|(e, _)| {
                        let pts = super::overlay::polyline(sk, e);
                        pts.windows(2).map(|w| [w[0], w[1]]).collect::<Vec<_>>()
                    })
                    .collect();
                // Closed when every end meets exactly one other end.
                let key = |p: &nalgebra::Point2<Real>| {
                    ((p.x * 1e5).round() as i64, (p.y * 1e5).round() as i64)
                };
                let mut ends: std::collections::HashMap<(i64, i64), usize> = Default::default();
                for [a, b] in &segs {
                    *ends.entry(key(a)).or_default() += 1;
                    *ends.entry(key(b)).or_default() += 1;
                }
                let closed = ends.values().all(|n| n % 2 == 0);
                (!segs.is_empty()).then(|| Side::Curves(sk.plane.clone(), segs, closed))
            }
            _ => None,
        }
    }

    /// Updates the Trim preview for this frame (`hover`: the cursor, while Remove is empty):
    /// the triangles to remove, rebuilt only when the surface, the tool or the side changes.
    pub(super) fn update_trim_preview(&mut self, hover: Option<(egui::Pos2, egui::Rect)>) {
        // The tool's side test, made once per tool (a surface's triangles are meshed for it).
        let tool_key = self
            .part_planned
            .as_ref()
            .filter(|d| d.kind == Planned::Part(PartPlanned::TrimSurface))
            .and_then(|d| d.items_of("tool").first())
            .map(|(item, _)| format!("{item:?}"));
        let Some(tool_key) = tool_key else {
            self.trim_side = None;
            if self.trim_preview.take().is_some() {
                self.scene_dirty = true;
            }
            return;
        };
        if self.trim_side.as_ref().map(|(k, _)| k) != Some(&tool_key) {
            self.trim_side = self
                .trim_tool_side()
                .map(|s| (tool_key.clone(), std::rc::Rc::new(s)));
        }
        let Some(side) = self.trim_side.as_ref().map(|(_, s)| s.clone()) else {
            return;
        };
        // Several areas picked: only the pieces under them (Inventor's Remove of many areas).
        if let Some((key, surface, picks, invert)) = self.trim_pieces_key()
            && self.trim_preview.as_ref().map(|(k, _)| k) != Some(&key)
        {
            let tris = self.trim_pieces_preview(&surface, &picks, invert, &side);
            self.trim_preview = Some((key, tris));
            self.scene_dirty = true;
            return;
        }
        if self.trim_pieces_key().is_some() {
            return;
        }
        let want = self.trim_preview_key(hover, &side);
        if self.trim_preview.as_ref().map(|(k, _)| k) == want.as_ref().map(|(k, ..)| k) {
            return;
        }
        self.trim_preview = want.map(|(key, surface, gone)| {
            let body = &self.part().body;
            let opts = crate::kernel::TessellationOptions::default();
            let mut tris = Vec::new();
            for f in crate::part::select::faces_of_body(body, &surface) {
                let Ok(m) = body.face_mesh(f, &opts) else {
                    continue;
                };
                for t in &m.triangles {
                    let corners = t.map(|i| m.positions[i as usize]);
                    clip_triangle(corners, |q| side.of(q), gone, &mut tris);
                }
            }
            (key, tris)
        });
        self.scene_dirty = true;
    }

    /// With two areas or more picked: (key, surface, the points, invert).
    fn trim_pieces_key(&self) -> Option<(String, String, Vec<Point3>, bool)> {
        let d = self.part_planned.as_ref()?;
        let picks = d.items_of("remove");
        if picks.len() < 2 {
            return None;
        }
        let (first, at0) = picks.first()?;
        let surface = match first {
            SelItem::Face(f) => self.body_of_face(f, *at0)?,
            SelItem::Body(b) => b.clone(),
            _ => return None,
        };
        let pts: Vec<Point3> = picks
            .iter()
            .filter_map(|(_, at)| at.map(Point3::from))
            .collect();
        let invert = d.check(0, "Invert Selection");
        Some((format!("{surface}|{pts:?}|{invert}"), surface, pts, invert))
    }

    /// The triangles of the pieces under `picks` (cut at the tool; every other piece with
    /// `invert`).
    fn trim_pieces_preview(
        &self,
        surface: &str,
        picks: &[Point3],
        invert: bool,
        side: &Side,
    ) -> Vec<[Point3; 3]> {
        let body = &self.part().body;
        let opts = crate::kernel::TessellationOptions::default();
        // The surface's triangles cut at the tool, both sides.
        let mut tris = Vec::new();
        for f in crate::part::select::faces_of_body(body, surface) {
            let Ok(m) = body.face_mesh(f, &opts) else {
                continue;
            };
            for t in &m.triangles {
                let corners = t.map(|i| m.positions[i as usize]);
                clip_triangle(corners, |q| side.of(q), true, &mut tris);
                clip_triangle(corners, |q| side.of(q), false, &mut tris);
            }
        }
        let label = pieces(&tris, |t| side.of(&centre(t)) > 0.0);
        let nearest = |q: &Point3| {
            tris.iter()
                .enumerate()
                .map(|(k, t)| (k, (centre(t) - q).norm()))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(k, _)| label[k])
        };
        let chosen: Vec<usize> = picks.iter().filter_map(nearest).collect();
        tris.iter()
            .zip(&label)
            .filter(|(_, l)| chosen.contains(l) != invert)
            .map(|(t, _)| *t)
            .collect()
    }

    /// What the preview shows: (key, surface, the removed side: `true` = positive).
    fn trim_preview_key(
        &self,
        hover: Option<(egui::Pos2, egui::Rect)>,
        side: &Side,
    ) -> Option<(String, String, bool)> {
        let d = self.part_planned.as_ref()?;
        let invert = d.check(0, "Invert Selection");
        let (item, at) = match d.items_of("remove").first() {
            Some((item, Some(at))) => (item.clone(), Point3::from(*at)),
            Some((_, None)) => return None,
            None => {
                let (p, rect) = hover?;
                if d.flow().current().map(|s| s.key) != Some("remove") {
                    return None;
                }
                let item = self.part_planned_item_at(p, rect)?;
                let at = self.click_point(&item, p, rect)?;
                (item, at)
            }
        };
        let surface = match &item {
            SelItem::Face(f) => self.body_of_face(f, Some([at.x, at.y, at.z]))?,
            SelItem::Body(b) => b.clone(),
            _ => return None,
        };
        let s0 = side.of(&at);
        if s0.abs() < 1e-9 {
            return None;
        }
        let gone = (s0 > 0.0) != invert;
        let key = format!("{surface}|{gone}");
        Some((key, surface, gone))
    }

    /// The preview's triangles into the scene (see-through red, after everything else).
    pub(super) fn add_trim_preview(&self, g: &mut super::renderer::SceneGeometry) {
        let Some((_, tris)) = &self.trim_preview else {
            return;
        };
        let mut pos = Vec::with_capacity(tris.len() * 3);
        let mut nor = Vec::with_capacity(tris.len() * 3);
        let mut idx = Vec::with_capacity(tris.len());
        for [a, b, c] in tris {
            let n = (b - a)
                .cross(&(c - a))
                .try_normalize(1e-18)
                .unwrap_or_else(crate::kernel::Vector3::z);
            let k = pos.len() as u32;
            pos.extend([*a, *b, *c]);
            nor.extend([n, n, n]);
            idx.push([k, k + 1, k + 2]);
        }
        g.add_mesh_transparent(&pos, &nor, &idx, [0.9, 0.15, 0.1, 0.55]);
    }
}
