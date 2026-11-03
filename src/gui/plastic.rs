//! The plastic part commands' logic behind their panels (`docs/spec/5.8-plastic-parts.md`): OK
//! turns the planned panel's picks and values into a [`BodyOp`] (`part::plastic`); Edit Feature
//! puts the values back. A sketch pick keeps the clicked point: it says which region, curve or
//! point of that sketch was meant; a sketch picked as a whole (from the browser, a selection)
//! gives its Center Points or its largest region.

use eframe::egui;

use super::CadCraftApp;
use super::part_planned::{PartDialog, PartPlanned};
use super::select::SelItem;
use crate::kernel::{Point3, Real};
use crate::part::plastic::Centers;
use crate::part::plastic::boss::{
    BossHead, BossKind, BossOp, BossRibs, BossThread, HeadStyle, ThreadHole,
};
use crate::part::plastic::grill::{GrillCurves, GrillOp, flow_area, regions_of};
use crate::part::plastic::lip::{LipKind, LipOp};
use crate::part::plastic::rest::{RestExtent, RestLanding, RestOp, RestSide};
use crate::part::plastic::rule_fillet::{FilletRule, RuleFilletOp, RuleRow, RuleSource};
use crate::part::plastic::snap_fit::{SnapFitOp, SnapHook, SnapKind, SnapLoop};
use crate::part::placement::PointRef;
use crate::part::select::feature_id_of_face;
use crate::part::{BodyOp, BodyOpFeature, EdgeRef, FaceRef, Feature, LineRef, PlaneRef, ProfileRef};
use crate::sketch::{EntityId, PointId, Region};

type Picks<'a> = &'a [(SelItem, Option<[f64; 3]>)];

fn rad(v: Option<f64>) -> Real {
    v.unwrap_or(0.0).to_radians()
}

impl CadCraftApp {
    /// The sketch under the cursor for a selector of regions, curves or points: `(sketch)`.
    pub(super) fn plastic_sketch_at(
        &self,
        p: egui::Pos2,
        rect: egui::Rect,
        kinds: &[crate::command::flow::PickKind],
    ) -> Option<usize> {
        use crate::command::flow::PickKind as K;
        use crate::gui::overlay;
        let (o, dir) = self.camera.ray(p, rect)?;
        let part = self.part();
        for (i, ps) in part.sketches.iter().enumerate() {
            if !(ps.visible || self.hidden_sketches_shown()) {
                continue;
            }
            let sk = &ps.sketch;
            if kinds.contains(&K::SketchPoint)
                && overlay::pick_point(&self.camera, rect, sk, p, overlay::POINT_SNAP_PX).is_some()
            {
                return Some(i);
            }
            if kinds.contains(&K::SketchCurve)
                && overlay::pick_entity(&self.camera, rect, sk, p, overlay::CURVE_SNAP_PX).is_some()
            {
                return Some(i);
            }
            if kinds.contains(&K::Region) {
                let n = sk.plane.normal.into_inner();
                let den = dir.dot(&n);
                if den.abs() < 1e-9 {
                    continue;
                }
                let q = o + dir * ((sk.plane.origin - o).dot(&n) / den);
                let uv = sk.plane.project(&q);
                if sk.regions().iter().any(|r| r.contains(uv)) {
                    return Some(i);
                }
            }
        }
        None
    }

    /// The sketch and its plane point of a pick.
    fn plastic_sketch_pick(&self, picks: Picks) -> Option<(String, Vec<Option<crate::sketch::Point2>>)> {
        let part = self.part();
        let mut id = None;
        let mut pts = Vec::new();
        for (item, at) in picks {
            let SelItem::Sketch(i) = item else {
                continue;
            };
            let Some(ps) = part.sketches.get(*i) else {
                continue;
            };
            id.get_or_insert_with(|| ps.id.clone());
            pts.push(at.map(|q| ps.sketch.plane.project(&Point3::from(q))));
        }
        id.map(|s| (s, pts))
    }

    fn sketch_named(&self, id: &str) -> Option<&crate::sketch::Sketch> {
        self.part()
            .sketches
            .iter()
            .find(|s| s.id == id)
            .map(|s| &s.sketch)
    }

    /// The curves of the closed outline a click on `at` means: regions read without curves that
    /// have a free end (centre lines usually cross it without sharing a point with it).
    fn outline_curves(&self, id: &str, at: crate::sketch::Point2) -> Vec<EntityId> {
        let Some(sk) = self.sketch_named(id) else {
            return Vec::new();
        };
        let mut uses: std::collections::HashMap<PointId, usize> = Default::default();
        let ends = |e: &crate::sketch::Entity| match e {
            crate::sketch::Entity::Line { start, end } => vec![*start, *end],
            _ => Vec::new(),
        };
        for (_, e) in sk.entities() {
            for p in ends(e) {
                *uses.entry(p).or_default() += 1;
            }
        }
        let mut clean = sk.clone();
        for (e, ent) in sk.entities() {
            if ends(ent).iter().any(|p| uses.get(p).copied().unwrap_or(0) < 2) {
                let _ = clean.set_style(e, crate::sketch::Style::Construction);
            }
        }
        clean
            .regions()
            .iter()
            .filter(|r| r.contains(at))
            .max_by(|a, b| a.outer.area().abs().total_cmp(&b.outer.area().abs()))
            .map(|r| ProfileRef::of(r, at).sources)
            .unwrap_or_default()
    }

    /// The nearest curve to each click, never one of `keep_off`.
    fn plastic_curves(
        &self,
        id: &str,
        pts: &[Option<crate::sketch::Point2>],
        keep_off: &[EntityId],
    ) -> Vec<EntityId> {
        let Some(sk) = self.sketch_named(id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for at in pts {
            let Some(at) = at else {
                continue;
            };
            let best = sk
                .entities()
                .filter_map(|(e, _)| {
                    let c = sk.curve_of(e)?;
                    let d = c
                        .sample(0.05)
                        .windows(2)
                        .map(|w| segment_distance(*at, w[0], w[1]))
                        .fold(Real::INFINITY, Real::min);
                    Some((d, e))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            // A click on the boundary is no rib: it is dropped, never moved to a curve far away.
            if let Some((_, e)) = best
                && !out.contains(&e)
                && !keep_off.contains(&e)
            {
                out.push(e);
            }
        }
        out
    }

    /// The nearest point to each click (none for a whole sketch: its Center Points).
    fn plastic_points(&self, id: &str, pts: &[Option<crate::sketch::Point2>]) -> Vec<PointId> {
        let Some(sk) = self.sketch_named(id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for at in pts.iter().flatten() {
            let best = sk
                .points()
                .map(|(i, p)| ((p.position - at).norm(), i))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, i)) = best
                && !out.contains(&i)
            {
                out.push(i);
            }
        }
        out
    }

    fn plastic_solids(&self, picks: Picks) -> Vec<String> {
        picks
            .iter()
            .filter_map(|(i, _)| match i {
                SelItem::Body(n) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    fn plastic_line(&self, picks: Picks) -> Option<LineRef> {
        match picks.first() {
            Some((SelItem::Edge(e), _)) => self
                .part()
                .body
                .edge(*e)
                .and_then(|x| x.name.clone())
                .map(LineRef::Edge),
            Some((SelItem::Feature(id), _)) => Some(LineRef::Work(id.clone())),
            _ => None,
        }
    }

    fn plastic_plane(&self, picks: Picks) -> Option<PlaneRef> {
        match picks.first() {
            Some((SelItem::Face(n), _)) => Some(PlaneRef::Face(n.clone())),
            Some((SelItem::Feature(id), _)) => Some(PlaneRef::Work(id.clone())),
            _ => None,
        }
    }

    fn plastic_faces(&self, picks: Picks) -> Vec<FaceRef> {
        picks
            .iter()
            .filter_map(|(item, at)| match item {
                SelItem::Face(f) => at
                    .or_else(|| self.item_point(item).map(|q| [q.x, q.y, q.z]))
                    .map(|hint| FaceRef {
                        face: f.clone(),
                        hint,
                    }),
                _ => None,
            })
            .collect()
    }

    fn feature_ids(&self, picks: Picks) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (item, _) in picks {
            let id = match item {
                SelItem::Feature(id) => Some(id.clone()),
                SelItem::Face(n) => feature_id_of_face(n).map(str::to_owned),
                _ => None,
            };
            if let Some(id) = id
                && !out.contains(&id)
            {
                out.push(id);
            }
        }
        out
    }

    fn plastic_centers(
        &self,
        d: &PartDialog,
        placement_tab: usize,
        old: Option<&Centers>,
    ) -> Result<Centers, String> {
        let picks = d.items_of("centers");
        if d.index(placement_tab, "Placement") == 1 {
            let mut points = Vec::new();
            for (item, _) in picks {
                match item {
                    SelItem::Vertex(n) => points.push(PointRef::Vertex(n.clone())),
                    // Only a work point: the selection at the start can be any feature.
                    SelItem::Feature(id) if self.part().work.points.contains_key(id) => {
                        points.push(PointRef::Work(id.clone()));
                    }
                    _ => {}
                }
            }
            let line = self.plastic_line(d.items_of("direction"));
            if let Some(Centers::Points {
                points: old_pts,
                direction,
            }) = old
            {
                return Ok(Centers::Points {
                    points: if points.is_empty() {
                        old_pts.clone()
                    } else {
                        points
                    },
                    direction: line.unwrap_or_else(|| direction.clone()),
                });
            }
            if points.is_empty() {
                return Err("select the centre points".into());
            }
            let Some(direction) = line else {
                return Err("select the direction".into());
            };
            return Ok(Centers::Points { points, direction });
        }
        match (self.plastic_sketch_pick(picks), old) {
            (Some((id, pts)), _) => {
                let points = self.plastic_points(&id, &pts);
                Ok(Centers::Sketch { sketch: id, points })
            }
            (None, Some(c @ Centers::Sketch { .. })) => Ok(c.clone()),
            _ => Err("select the centres' sketch".into()),
        }
    }

    /// The region clicked in a sketch, among `regions`.
    fn region_at(regions: &[Region], at: crate::sketch::Point2) -> Option<ProfileRef> {
        // The smallest holding the point: an island inside a boundary is the island.
        regions
            .iter()
            .filter(|r| r.contains(at))
            .min_by(|a, b| a.outer.area().abs().total_cmp(&b.outer.area().abs()))
            .map(|r| ProfileRef::of(r, at))
    }

    /// The panel's definition of a plastic command (`old`: the feature being edited).
    pub(super) fn plastic_op_from(
        &self,
        kind: PartPlanned,
        d: &PartDialog,
        old: Option<&BodyOp>,
    ) -> Result<BodyOp, String> {
        match kind {
            PartPlanned::Boss => self.boss_from(d, old),
            PartPlanned::SnapFit => self.snap_from(d, old),
            PartPlanned::Rest => self.rest_from(d, old),
            PartPlanned::Grill => self.grill_from(d, old),
            PartPlanned::RuleFillet => self.rule_from(d, old),
            PartPlanned::Lip => self.lip_from(d, old),
            _ => Err("not a plastic command".into()),
        }
    }

    fn boss_from(&self, d: &PartDialog, old: Option<&BodyOp>) -> Result<BodyOp, String> {
        let prev = match old {
            Some(BodyOp::Boss(o)) => Some(o),
            _ => None,
        };
        let centers = self.plastic_centers(d, 0, prev.map(|o| &o.centers))?;
        let head = d.index(0, "Type") == 0;
        let num = |t, l| d.num(t, l).unwrap_or(0.0);
        let kind = if head {
            BossKind::Head(BossHead {
                style: if d.index(1, "Seat") == 1 {
                    HeadStyle::Countersink
                } else {
                    HeadStyle::Counterbore
                },
                wall: num(1, "Wall Thickness"),
                shank_height: num(1, "Shank Height"),
                clamp_height: num(1, "Clamp Height"),
                shank_diameter: num(1, "Shank Diameter"),
                clamp_diameter: num(1, "Clamp Diameter"),
                head_diameter: num(1, "Head Diameter"),
                countersink: rad(d.num(1, "Countersink Angle")),
                inner_draft: rad(d.num(1, "Inner Draft Angle")),
                shaft_draft: rad(d.num(1, "Shaft Draft Angle")),
                clamp_draft: rad(d.num(1, "Clamp Draft Angle")),
            })
        } else {
            BossKind::Thread(BossThread {
                diameter: num(2, "Thread Diameter"),
                hole: if !d.check(2, "Hole") {
                    ThreadHole::None
                } else {
                    match d.index(2, "Hole Type") {
                        0 => ThreadHole::Depth(num(2, "Thread Hole Depth")),
                        1 => ThreadHole::FullDepth,
                        _ => ThreadHole::Through,
                    }
                },
                hole_diameter: num(2, "Thread Hole Diameter"),
                inner_draft: rad(d.num(2, "Inner Draft Angle")),
            })
        };
        let outer = if head {
            rad(d.num(1, "Outer Draft Angle"))
        } else {
            rad(d.num(2, "Outer Draft Angle"))
        };
        let ribs = d.check(3, "Stiffening Ribs").then(|| BossRibs {
            count: num(3, "Number of Ribs").max(0.0).round() as usize,
            thickness: num(3, "Rib Thickness"),
            draft: rad(d.num(3, "Rib Draft")),
            shoulder: num(3, "Shoulder Length"),
            top_offset: num(3, "Top Offset"),
            shoulder_radius: num(3, "Shoulder Radius"),
            flare: rad(d.num(3, "Flare Angle")),
            rib_radius: num(3, "Rib Radius"),
            blend_radius: num(3, "Blend Radius"),
            start: rad(d.num(3, "Start Angle")),
            space: rad(d.num(3, "Space Angle")),
        });
        let mut solids = self.plastic_solids(d.items_of("solid"));
        if solids.is_empty()
            && let Some(o) = prev
        {
            solids = o.solids.clone();
        }
        Ok(BodyOp::Boss(BossOp {
            centers,
            solids,
            flip: d.check(0, "Flip"),
            offset: num(0, "Offset"),
            kind,
            outer_draft: outer,
            fillet: num(0, "Fillet"),
            ribs,
        }))
    }

    fn snap_from(&self, d: &PartDialog, old: Option<&BodyOp>) -> Result<BodyOp, String> {
        let prev = match old {
            Some(BodyOp::SnapFit(o)) => Some(o),
            _ => None,
        };
        let centers = self.plastic_centers(d, 0, prev.map(|o| &o.centers))?;
        let num = |t, l| d.num(t, l).unwrap_or(0.0);
        let kind = if d.index(0, "Type") == 0 {
            SnapKind::Hook(SnapHook {
                thickness_wall: num(1, "Thickness at Wall"),
                thickness_hook: num(1, "Thickness at Hook"),
                width_wall: num(1, "Width at Wall"),
                width_hook: num(1, "Width at Hook"),
                length: num(1, "Beam Length"),
                tip_length: num(2, "Tip Length"),
                undercut: num(2, "Undercut Depth"),
                hook_length: num(2, "Hook Length"),
                retention: rad(d.num(2, "Retention Angle")),
                insertion: rad(d.num(2, "Insertion Angle")),
            })
        } else {
            SnapKind::Loop(SnapLoop {
                length: num(3, "Clip Length"),
                width: num(3, "Clip Width"),
                thickness_wall: num(3, "Thickness at Wall"),
                thickness_top: num(3, "Thickness at Top"),
                catch_side: num(4, "Side Width"),
                catch_top: num(4, "Top Width"),
                opening: num(4, "Opening Length"),
            })
        };
        let mut solids = self.plastic_solids(d.items_of("solid"));
        if solids.is_empty()
            && let Some(o) = prev
        {
            solids = o.solids.clone();
        }
        Ok(BodyOp::SnapFit(SnapFitOp {
            centers,
            solids,
            flip: d.check(0, "Flip"),
            hook_arrow: d.index(0, "Hook Direction") as u8,
            extend: d.index(0, "Extents") == 0,
            kind,
        }))
    }

    fn rest_from(&self, d: &PartDialog, old: Option<&BodyOp>) -> Result<BodyOp, String> {
        let prev = match old {
            Some(BodyOp::Rest(o)) => Some(o),
            _ => None,
        };
        let (sketch, profiles) = match self.plastic_sketch_pick(d.items_of("profile")) {
            Some((id, pts)) => {
                let regions = self
                    .sketch_named(&id)
                    .map(|s| s.regions())
                    .unwrap_or_default();
                let list: Vec<ProfileRef> = pts
                    .iter()
                    .flatten()
                    .filter_map(|at| Self::region_at(&regions, *at))
                    .collect();
                (id, list)
            }
            None => match prev {
                Some(o) => (o.sketch.clone(), o.profiles.clone()),
                None => return Err("select the profile".into()),
            },
        };
        let num = |t, l| d.num(t, l).unwrap_or(0.0);
        let surface = |key: &str, what: &str| match self.plastic_plane(d.items_of(key)) {
            Some(p) => Ok(p),
            None => Err(format!("select the {what} surface")),
        };
        let extent = match d.index(0, "Platform") {
            1 => RestExtent::Distance(num(0, "Distance")),
            2 => {
                let plane = match (surface("to", "platform's"), prev.map(|o| &o.extent)) {
                    (Ok(p), _) => p,
                    (_, Some(RestExtent::ToSurface(p, _))) => p.clone(),
                    (Err(e), _) => return Err(e),
                };
                RestExtent::ToSurface(plane, num(0, "Offset"))
            }
            _ => RestExtent::ThroughAll,
        };
        let landing = if d.index(1, "Landing") == 1 {
            let plane = match (surface("land", "landing's"), prev.map(|o| &o.landing)) {
                (Ok(p), _) => p,
                (_, Some(RestLanding::ToSurface(p, _))) => p.clone(),
                (Err(e), _) => return Err(e),
            };
            RestLanding::ToSurface(plane, num(1, "Landing Offset"))
        } else {
            RestLanding::Distance(num(1, "Landing Distance"))
        };
        let mut solids = self.plastic_solids(d.items_of("solid"));
        if solids.is_empty()
            && let Some(o) = prev
        {
            solids = o.solids.clone();
        }
        Ok(BodyOp::Rest(RestOp {
            sketch,
            profiles,
            solids,
            flip: d.check(0, "Flip"),
            thickness: num(0, "Thickness"),
            side: match d.index(0, "Thickness Side") {
                1 => RestSide::Outside,
                2 => RestSide::BothSides,
                _ => RestSide::Inside,
            },
            extent,
            landing,
            landing_taper: rad(d.num(1, "Landing Taper")),
            clearance_taper: rad(d.num(1, "Clearance Taper")),
        }))
    }

    fn grill_from(&self, d: &PartDialog, old: Option<&BodyOp>) -> Result<BodyOp, String> {
        let prev = match old {
            Some(BodyOp::Grill(o)) => Some(o),
            _ => None,
        };
        let num = |t, l| d.num(t, l).unwrap_or(0.0);
        let boundary_pick = self.plastic_sketch_pick(d.items_of("boundary"));
        let Some(sketch) = boundary_pick
            .as_ref()
            .map(|b| b.0.clone())
            .or_else(|| prev.map(|o| o.sketch.clone()))
        else {
            return Err("select the boundary profile".into());
        };
        // A rib or spar never takes a curve of the boundary (that would open it).
        let keep_off: Vec<EntityId> = boundary_pick
            .as_ref()
            .and_then(|(id, pts)| {
                pts.iter()
                    .flatten()
                    .next()
                    .map(|at| self.outline_curves(id, *at))
            })
            .unwrap_or_default();
        let curves = |key: &str, thick: Real, height: Real, top: Real, bottom: Real, was| {
            let chosen = self.plastic_sketch_pick(d.items_of(key)).map(|(id, pts)| {
                let off: &[EntityId] = if id == sketch { &keep_off } else { &[] };
                let list = self.plastic_curves(&id, &pts, off);
                (id, list)
            });
            match (chosen, was) {
                (Some((sketch, curves)), _) if !curves.is_empty() => Some(GrillCurves {
                    sketch,
                    curves,
                    thickness: thick,
                    height,
                    top_offset: top,
                    bottom_offset: bottom,
                }),
                (_, Some(w)) => {
                    let w: &GrillCurves = w;
                    Some(GrillCurves {
                        sketch: w.sketch.clone(),
                        curves: w.curves.clone(),
                        thickness: thick,
                        height,
                        top_offset: top,
                        bottom_offset: bottom,
                    })
                }
                _ => None,
            }
        };
        let ribs = curves(
            "ribs",
            num(2, "Thickness"),
            num(2, "Height"),
            num(2, "Top Offset"),
            0.0,
            prev.and_then(|o| o.ribs.as_ref()),
        );
        let spars = curves(
            "spars",
            num(3, "Thickness"),
            0.0,
            num(3, "Top Offset"),
            num(3, "Bottom Offset"),
            prev.and_then(|o| o.spars.as_ref()),
        );
        let mut op = GrillOp {
            sketch: sketch.clone(),
            boundary: prev.and_then(|o| o.boundary.clone()),
            solids: Vec::new(),
            flip: d.check(0, "Flip"),
            thickness: num(0, "Thickness"),
            height: num(0, "Height"),
            outside_height: num(0, "Outside Height"),
            islands: prev.map(|o| o.islands.clone()).unwrap_or_default(),
            island_sketch: prev.and_then(|o| o.island_sketch.clone()),
            island_thickness: num(1, "Thickness"),
            ribs,
            spars,
            draft: rad(d.num(4, "Draft Angle")),
        };
        // Regions are read without the ribs' and spars' curves, so they come after those.
        if let Some((_, pts)) = &boundary_pick
            && let Some(sk) = self.sketch_named(&sketch)
        {
            let regions = regions_of(&op, sk, &sketch);
            // The boundary: the largest region holding the click (an island inside it may be
            // what the click landed on only when the island is its own pick).
            op.boundary = pts.iter().flatten().find_map(|at| {
                regions
                    .iter()
                    .filter(|r| r.contains(*at))
                    .max_by(|a, b| a.outer.area().abs().total_cmp(&b.outer.area().abs()))
                    .map(|r| ProfileRef::of(r, *at))
            });
        }
        if let Some((id, pts)) = self.plastic_sketch_pick(d.items_of("island"))
            && let Some(sk) = self.sketch_named(&id)
        {
            let regions = regions_of(&op, sk, &id);
            op.islands = pts
                .iter()
                .flatten()
                .filter_map(|at| Self::region_at(&regions, *at))
                .collect();
            op.island_sketch = (id != sketch).then_some(id);
        }
        op.solids = self.plastic_solids(d.items_of("solid"));
        if op.solids.is_empty()
            && let Some(o) = prev
        {
            op.solids = o.solids.clone();
        }
        Ok(BodyOp::Grill(op))
    }

    fn rule_from(&self, d: &PartDialog, old: Option<&BodyOp>) -> Result<BodyOp, String> {
        let prev = match old {
            Some(BodyOp::RuleFillet(o)) => Some(o),
            _ => None,
        };
        let mut rows = Vec::new();
        for (t, (src, scope, axis)) in [
            ("src1", "scope1", "axis1"),
            ("src2", "scope2", "axis2"),
            ("src3", "scope3", "axis3"),
        ]
        .into_iter()
        .enumerate()
        {
            let was = prev.and_then(|o| o.rows.get(t));
            let features = d.index(t, "Source") == 0;
            let source = if features {
                let v = self.feature_ids(d.items_of(src));
                if v.is_empty() {
                    match was.map(|r| &r.source) {
                        Some(s @ RuleSource::Features(_)) => Some(s.clone()),
                        _ => None,
                    }
                } else {
                    Some(RuleSource::Features(v))
                }
            } else {
                let v = self.plastic_faces(d.items_of(src));
                if v.is_empty() {
                    match was.map(|r| &r.source) {
                        Some(s @ RuleSource::Faces(_)) => Some(s.clone()),
                        _ => None,
                    }
                } else {
                    Some(RuleSource::Faces(v))
                }
            };
            let Some(source) = source else {
                continue;
            };
            let rule = match d.index(t, "Rule") {
                0 => FilletRule::AllEdges,
                1 => FilletRule::AgainstPart,
                2 => FilletRule::FreeEdges,
                3 => {
                    let mut v = self.feature_ids(d.items_of(scope));
                    if v.is_empty()
                        && let Some(RuleRow {
                            rule: FilletRule::AgainstFeatures(w),
                            ..
                        }) = was
                    {
                        v = w.clone();
                    }
                    FilletRule::AgainstFeatures(v)
                }
                _ => {
                    let line = self.plastic_line(d.items_of(axis)).or_else(|| match was {
                        Some(RuleRow {
                            rule: FilletRule::IncidentEdges { axis, .. },
                            ..
                        }) => Some(axis.clone()),
                        _ => None,
                    });
                    let Some(line) = line else {
                        return Err("Incident Edges: select the axis".into());
                    };
                    FilletRule::IncidentEdges {
                        axis: line,
                        flip: d.check(t, "Flip Axis"),
                        tolerance: rad(d.num(t, "Tolerance")),
                    }
                }
            };
            rows.push(RuleRow {
                source,
                rule,
                radius: d.num(t, "Radius").unwrap_or(1.0),
                all_rounds: d.check(t, "All Rounds"),
                all_fillets: d.check(t, "All Fillets"),
            });
        }
        let picked: Vec<String> = d
            .items_of("exclude")
            .iter()
            .filter_map(|(i, _)| match i {
                SelItem::Face(n) => Some(n.clone()),
                _ => None,
            })
            .collect();
        let exclude_faces = if picked.is_empty() {
            prev.map(|o| o.exclude_faces.clone()).unwrap_or_default()
        } else {
            picked
        };
        Ok(BodyOp::RuleFillet(RuleFilletOp {
            rows,
            exclude_faces,
            remove_merged: d.check(3, "Remove Merged Faces"),
        }))
    }

    fn lip_from(&self, d: &PartDialog, old: Option<&BodyOp>) -> Result<BodyOp, String> {
        let prev = match old {
            Some(BodyOp::Lip(o)) => Some(o),
            _ => None,
        };
        let body = &self.part().body;
        let mut path: Vec<EdgeRef> = d
            .items_of("path")
            .iter()
            .filter_map(|(item, at)| match item {
                SelItem::Edge(e) => {
                    let mut r = crate::part::blend::edge_ref(body, *e)?;
                    if let Some(q) = at {
                        r.hint = *q;
                    }
                    Some(r)
                }
                _ => None,
            })
            .collect();
        if path.is_empty()
            && let Some(o) = prev
        {
            path = o.path.clone();
        }
        let num = |l| d.num(1, l).unwrap_or(0.0);
        let by_pull = d.index(0, "Orientation") == 1;
        let guide = if by_pull {
            None
        } else {
            self.plastic_faces(d.items_of("guide"))
                .into_iter()
                .next()
                .or_else(|| prev.and_then(|o| o.guide.clone()))
        };
        let pull = if by_pull {
            self.plastic_line(d.items_of("pull"))
                .or_else(|| prev.and_then(|o| o.pull.clone()))
        } else {
            None
        };
        let mut solids = self.plastic_solids(d.items_of("solid"));
        if solids.is_empty()
            && let Some(o) = prev
        {
            solids = o.solids.clone();
        }
        Ok(BodyOp::Lip(LipOp {
            kind: if d.index(0, "Type") == 1 {
                LipKind::Groove
            } else {
                LipKind::Lip
            },
            path,
            guide,
            pull,
            solids,
            height: num("Height"),
            width: num("Width"),
            shoulder: num("Shoulder Width"),
            clearance: num("Clearance"),
            inside_angle: rad(d.num(1, "Inside Angle")),
            outside_angle: rad(d.num(1, "Outside Angle")),
        }))
    }

    /// The new faces' edges while a plastic command is open, built on a worker thread from a copy
    /// of the part. `Err("")`: its inputs are not complete yet.
    pub(super) fn plastic_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.part_planned else {
            return Err(String::new());
        };
        let edit = d.feature;
        let old = edit.and_then(|k| match self.part().features.get(k) {
            Some(Feature::BodyOp(b)) => Some(b.clone()),
            _ => None,
        });
        let op = self
            .surface_op_from(d, old.as_ref().map(|b| &b.op))
            .map_err(|_| String::new())?;
        let mut f = old.unwrap_or_else(|| BodyOpFeature::new(op.clone()));
        f.op = op;
        if f.validate().is_err() {
            return Err(String::new());
        }
        let key = format!("plastic|{edit:?}|{:?}", f.op);
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        self.preview_async(key, move || {
            let built = match edit {
                Some(k) => {
                    let id = f.id.clone();
                    trial.replace_feature(k, Feature::BodyOp(f)).map(|_| id)
                }
                None => trial.add_body_op(f),
            };
            built
                .map(|id| {
                    let own = format!("{id}:");
                    super::combine_split::face_edges(&trial, |n| n.starts_with(&own))
                })
                .map_err(|e| e.to_string())
        })
    }

    /// Grill's Flow Area for the panel's current values.
    pub(super) fn grill_flow_line(&self, d: &PartDialog) -> String {
        match self.grill_from(d, None) {
            Ok(BodyOp::Grill(op)) => match flow_area(&op, &self.part().sketches) {
                Ok(a) => format!("Flow Area: {a:.2} mm²"),
                Err(e) => format!("Flow Area: {e}"),
            },
            Ok(_) => String::new(),
            Err(e) => format!("Flow Area: {e}"),
        }
    }
}

fn segment_distance(p: crate::sketch::Point2, a: crate::sketch::Point2, b: crate::sketch::Point2) -> Real {
    let u = b - a;
    let t = ((p - a).dot(&u) / u.dot(&u).max(1e-24)).clamp(0.0, 1.0);
    (a + u * t - p).norm()
}

/// Edit Feature: the feature's values into the panel.
pub(super) fn fill_dialog(d: &mut PartDialog, op: &BodyOp) {
    let deg = Real::to_degrees;
    match op {
        BodyOp::Boss(o) => {
            d.set_index(0, "Placement", usize::from(matches!(o.centers, Centers::Points { .. })));
            d.set_check(0, "Flip", o.flip);
            d.set_num(0, "Offset", o.offset);
            d.set_num(0, "Fillet", o.fillet);
            match &o.kind {
                BossKind::Head(h) => {
                    d.set_index(0, "Type", 0);
                    d.set_index(1, "Seat", usize::from(h.style == HeadStyle::Countersink));
                    d.set_num(1, "Wall Thickness", h.wall);
                    d.set_num(1, "Shank Height", h.shank_height);
                    d.set_num(1, "Clamp Height", h.clamp_height);
                    d.set_num(1, "Shank Diameter", h.shank_diameter);
                    d.set_num(1, "Clamp Diameter", h.clamp_diameter);
                    d.set_num(1, "Head Diameter", h.head_diameter);
                    if h.countersink > 0.0 {
                        d.set_num(1, "Countersink Angle", deg(h.countersink));
                    }
                    d.set_num(1, "Outer Draft Angle", deg(o.outer_draft));
                    d.set_num(1, "Inner Draft Angle", deg(h.inner_draft));
                    d.set_num(1, "Shaft Draft Angle", deg(h.shaft_draft));
                    d.set_num(1, "Clamp Draft Angle", deg(h.clamp_draft));
                }
                BossKind::Thread(t) => {
                    d.set_index(0, "Type", 1);
                    d.set_num(2, "Thread Diameter", t.diameter);
                    d.set_num(2, "Thread Hole Diameter", t.hole_diameter);
                    d.set_num(2, "Outer Draft Angle", deg(o.outer_draft));
                    d.set_num(2, "Inner Draft Angle", deg(t.inner_draft));
                    d.set_check(2, "Hole", t.hole != ThreadHole::None);
                    match t.hole {
                        ThreadHole::Depth(v) => {
                            d.set_index(2, "Hole Type", 0);
                            d.set_num(2, "Thread Hole Depth", v);
                        }
                        ThreadHole::FullDepth => d.set_index(2, "Hole Type", 1),
                        ThreadHole::Through => d.set_index(2, "Hole Type", 2),
                        ThreadHole::None => {}
                    }
                }
            }
            d.set_check(3, "Stiffening Ribs", o.ribs.is_some());
            if let Some(r) = &o.ribs {
                d.set_num(3, "Number of Ribs", r.count as Real);
                d.set_num(3, "Rib Thickness", r.thickness);
                d.set_num(3, "Rib Draft", deg(r.draft));
                d.set_num(3, "Shoulder Length", r.shoulder);
                d.set_num(3, "Top Offset", r.top_offset);
                d.set_num(3, "Shoulder Radius", r.shoulder_radius);
                d.set_num(3, "Flare Angle", deg(r.flare));
                d.set_num(3, "Rib Radius", r.rib_radius);
                d.set_num(3, "Blend Radius", r.blend_radius);
                d.set_num(3, "Start Angle", deg(r.start));
                d.set_num(3, "Space Angle", deg(r.space));
            }
        }
        BodyOp::SnapFit(o) => {
            d.set_index(0, "Placement", usize::from(matches!(o.centers, Centers::Points { .. })));
            d.set_check(0, "Flip", o.flip);
            d.set_index(0, "Hook Direction", o.hook_arrow as usize);
            d.set_index(0, "Extents", usize::from(!o.extend));
            match &o.kind {
                SnapKind::Hook(h) => {
                    d.set_index(0, "Type", 0);
                    d.set_num(1, "Thickness at Wall", h.thickness_wall);
                    d.set_num(1, "Thickness at Hook", h.thickness_hook);
                    d.set_num(1, "Width at Wall", h.width_wall);
                    d.set_num(1, "Width at Hook", h.width_hook);
                    d.set_num(1, "Beam Length", h.length);
                    d.set_num(2, "Tip Length", h.tip_length);
                    d.set_num(2, "Undercut Depth", h.undercut);
                    d.set_num(2, "Hook Length", h.hook_length);
                    d.set_num(2, "Retention Angle", deg(h.retention));
                    d.set_num(2, "Insertion Angle", deg(h.insertion));
                }
                SnapKind::Loop(l) => {
                    d.set_index(0, "Type", 1);
                    d.set_num(3, "Clip Length", l.length);
                    d.set_num(3, "Clip Width", l.width);
                    d.set_num(3, "Thickness at Wall", l.thickness_wall);
                    d.set_num(3, "Thickness at Top", l.thickness_top);
                    d.set_num(4, "Side Width", l.catch_side);
                    d.set_num(4, "Top Width", l.catch_top);
                    d.set_num(4, "Opening Length", l.opening);
                }
            }
        }
        BodyOp::Rest(o) => {
            d.set_check(0, "Flip", o.flip);
            d.set_num(0, "Thickness", o.thickness);
            d.set_index(
                0,
                "Thickness Side",
                match o.side {
                    RestSide::Inside => 0,
                    RestSide::Outside => 1,
                    RestSide::BothSides => 2,
                },
            );
            match &o.extent {
                RestExtent::ThroughAll => d.set_index(0, "Platform", 0),
                RestExtent::Distance(v) => {
                    d.set_index(0, "Platform", 1);
                    d.set_num(0, "Distance", *v);
                }
                RestExtent::ToSurface(_, v) => {
                    d.set_index(0, "Platform", 2);
                    d.set_num(0, "Offset", *v);
                }
            }
            match &o.landing {
                RestLanding::Distance(v) => {
                    d.set_index(1, "Landing", 0);
                    d.set_num(1, "Landing Distance", *v);
                }
                RestLanding::ToSurface(_, v) => {
                    d.set_index(1, "Landing", 1);
                    d.set_num(1, "Landing Offset", *v);
                }
            }
            d.set_num(1, "Landing Taper", deg(o.landing_taper));
            d.set_num(1, "Clearance Taper", deg(o.clearance_taper));
        }
        BodyOp::Grill(o) => {
            d.set_check(0, "Flip", o.flip);
            d.set_num(0, "Thickness", o.thickness);
            d.set_num(0, "Height", o.height);
            d.set_num(0, "Outside Height", o.outside_height);
            d.set_num(1, "Thickness", o.island_thickness);
            if let Some(r) = &o.ribs {
                d.set_num(2, "Thickness", r.thickness);
                d.set_num(2, "Height", r.height);
                d.set_num(2, "Top Offset", r.top_offset);
            }
            if let Some(s) = &o.spars {
                d.set_num(3, "Thickness", s.thickness);
                d.set_num(3, "Top Offset", s.top_offset);
                d.set_num(3, "Bottom Offset", s.bottom_offset);
            }
            d.set_num(4, "Draft Angle", deg(o.draft));
        }
        BodyOp::RuleFillet(o) => {
            for (t, r) in o.rows.iter().enumerate().take(3) {
                d.set_index(t, "Source", usize::from(matches!(r.source, RuleSource::Faces(_))));
                d.set_num(t, "Radius", r.radius);
                d.set_check(t, "All Rounds", r.all_rounds);
                d.set_check(t, "All Fillets", r.all_fillets);
                let k = match &r.rule {
                    FilletRule::AllEdges => 0,
                    FilletRule::AgainstPart => 1,
                    FilletRule::FreeEdges => 2,
                    FilletRule::AgainstFeatures(_) => 3,
                    FilletRule::IncidentEdges {
                        flip, tolerance, ..
                    } => {
                        d.set_check(t, "Flip Axis", *flip);
                        d.set_num(t, "Tolerance", deg(*tolerance));
                        4
                    }
                };
                d.set_index(t, "Rule", k);
            }
            d.set_check(3, "Remove Merged Faces", o.remove_merged);
        }
        BodyOp::Lip(o) => {
            d.set_index(0, "Type", usize::from(o.kind == LipKind::Groove));
            d.set_index(0, "Orientation", usize::from(o.pull.is_some()));
            d.set_num(1, "Height", o.height);
            d.set_num(1, "Width", o.width);
            d.set_num(1, "Shoulder Width", o.shoulder);
            d.set_num(1, "Clearance", o.clearance);
            d.set_num(1, "Inside Angle", deg(o.inside_angle));
            d.set_num(1, "Outside Angle", deg(o.outside_angle));
        }
        _ => {}
    }
}

/// Is the panel one of the plastic commands?
pub(super) fn is_plastic(d: &PartDialog) -> bool {
    matches!(
        d.kind,
        super::part_planned::Planned::Part(
            PartPlanned::Grill
                | PartPlanned::Boss
                | PartPlanned::Rest
                | PartPlanned::SnapFit
                | PartPlanned::RuleFillet
                | PartPlanned::Lip
        )
    )
}

/// The plastic command of a body operation, if it is one.
pub(super) fn command_of(op: &BodyOp) -> Option<PartPlanned> {
    Some(match op {
        BodyOp::Grill(_) => PartPlanned::Grill,
        BodyOp::Boss(_) => PartPlanned::Boss,
        BodyOp::Rest(_) => PartPlanned::Rest,
        BodyOp::SnapFit(_) => PartPlanned::SnapFit,
        BodyOp::RuleFillet(_) => PartPlanned::RuleFillet,
        BodyOp::Lip(_) => PartPlanned::Lip,
        _ => return None,
    })
}

/// A row of a plastic panel that does nothing with the options chosen.
pub(super) fn row_off(d: &PartDialog, kind: PartPlanned, tab: usize, label: &str) -> bool {
    match kind {
        PartPlanned::Boss => {
            let head = d.index(0, "Type") == 0;
            match tab {
                0 => label == "Offset" && d.index(0, "Placement") == 1,
                1 => !head || (label == "Countersink Angle" && d.index(1, "Seat") == 0),
                2 => {
                    let hole = d.check(2, "Hole");
                    head || (!hole
                        && matches!(
                            label,
                            "Thread Hole Diameter" | "Thread Hole Depth" | "Inner Draft Angle"
                        ))
                        || (label == "Thread Hole Depth" && d.index(2, "Hole Type") != 0)
                }
                3 => label != "Stiffening Ribs" && !d.check(3, "Stiffening Ribs"),
                _ => false,
            }
        }
        PartPlanned::SnapFit => {
            let hook = d.index(0, "Type") == 0;
            match tab {
                1 | 2 => !hook,
                3 | 4 => hook,
                _ => false,
            }
        }
        PartPlanned::Rest => match (tab, label) {
            (0, "Distance") => d.index(0, "Platform") != 1,
            (0, "Offset") => d.index(0, "Platform") != 2,
            (1, "Landing Distance") => d.index(1, "Landing") != 0,
            (1, "Landing Offset") => d.index(1, "Landing") != 1,
            _ => false,
        },
        PartPlanned::RuleFillet => match label {
            "Tolerance" | "Flip Axis" => tab < 3 && d.index(tab, "Rule") != 4,
            _ => false,
        },
        _ => false,
    }
}
