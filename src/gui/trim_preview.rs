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
    /// Sketch curves (their polylines in the sketch plane): the side of the nearest segment.
    Curves(crate::kernel::Plane, Vec<[nalgebra::Point2<Real>; 2]>),
}

impl Side {
    fn of(&self, q: &Point3) -> Real {
        match self {
            Side::Plane(pl) => (q - pl.origin).dot(&pl.normal.into_inner()),
            Side::Sheet(s) => s.signed(q),
            Side::Curves(pl, segs) => {
                let p = pl.project(q);
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
                (!segs.is_empty()).then(|| Side::Curves(sk.plane.clone(), segs))
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
