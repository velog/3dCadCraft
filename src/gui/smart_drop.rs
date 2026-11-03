//! Smart Drop (`docs/spec/6.3-content-center.md` §3.5, §6.5) and Change Size
//! (§3.4): a family's member resized to the hole or shaft under the cursor, a
//! click places it with its relationships. The choices are GUI-free
//! (`components::fit`); this is the pointer, the toolbar and the commit.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

use super::CadCraftApp;
use super::assembly::AsmTool;
use crate::assembly::PickMesh;
use crate::assembly::relation::{Constraint, ConstraintKind, Geom, GeomRef, OriginItem};
use crate::components::catalog::Catalog;
use crate::components::fit::{self, Fit, Target};
use crate::components::samples::Detail;
use crate::document::{DocRef, DocumentData};
use crate::kernel::{Point3, Real, Transform3, Vector3};
use crate::part::connection::{Connection, ConnectionRole};
use crate::part::hole::{self, Termination};
use crate::part::shell::FaceRef;

/// A place something can land on, worked out once per picked item.
#[derive(Clone, Debug)]
struct Spot {
    /// What was picked: the cursor is still on it while this stays equal.
    key: GeomRef,
    /// What gives the axis.
    axis: GeomRef,
    /// The plane the seat lies on; `None`: a cylinder, a plane comes next.
    plane: Option<GeomRef>,
    /// Picked as a rim with its plane (not a plane chosen afterwards).
    from_rim: bool,
    /// World centre on the plane, and the plane's outward normal.
    center: Point3,
    normal: Vector3,
    radius: Real,
    /// What Smart Drop's choice reads.
    target: Target,
    /// The Hole feature it belongs to and the part: Follow Pattern's reach.
    hole_feature: Option<String>,
    /// Its place among the part's holes (the first of a feature is the source).
    hole_order: Option<usize>,
}

/// One part of what a click places.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Item {
    family: usize,
    member: usize,
    sink: Real,
    flip: bool,
}

/// A hole of a Hole feature on an axis.
#[derive(Clone, Default)]
struct HoleData {
    thread: Option<Real>,
    blind: bool,
    feature: Option<String>,
    order: Option<usize>,
}

/// The tool's state.
pub(super) struct SmartDrop {
    catalog: Catalog,
    family: usize,
    detail: Detail,
    fits: Vec<Fit>,
    /// The size asked for in the toolbar (`None`: the nearest to the target).
    chosen: Option<usize>,
    flip: bool,
    follow_pattern: bool,
    insert_multiple: bool,
    lock_rotation: bool,
    /// A washer under the head, and a washer and a nut on the far side.
    bolted: bool,
    /// What the cursor is on (`member`: `None` when no size fits).
    hover: Option<(Spot, Option<usize>)>,
    /// A cylinder was clicked: the seat's plane comes next.
    cylinder: Option<Spot>,
    /// Meshes by family and member.
    meshes: std::collections::HashMap<(usize, usize), Option<Arc<PickMesh>>>,
    /// A part placed by its own connection, instead of a family.
    single: Option<Single>,
    /// A published family's members: how each lands.
    owned: std::collections::HashMap<usize, Option<Single>>,
}

/// A part with a connection (spec 6.3 §3.5), placed with one Insert.
#[derive(Clone)]
struct Single {
    component: DocRef,
    name: String,
    /// The edge it lands with, in the assembly's reference form below.
    edge: crate::part::blend::EdgeRef,
    /// That edge's circle in part coordinates: centre and outward normal.
    center: Point3,
    normal: Vector3,
}

impl CadCraftApp {
    /// Starts Smart Drop for a family of the window (spec 6.3 §3.5).
    pub(super) fn smart_drop_start(&mut self, catalog: Catalog, family: usize, detail: Detail) {
        let fits = catalog.fits(family);
        self.asm.place = None;
        self.asm.smart = Some(SmartDrop {
            catalog,
            family,
            detail,
            fits,
            chosen: None,
            flip: false,
            follow_pattern: true,
            insert_multiple: true,
            lock_rotation: false,
            bolted: false,
            hover: None,
            cylinder: None,
            meshes: Default::default(),
            single: None,
            owned: Default::default(),
        });
        self.asm.tool = Some(AsmTool::SmartDrop);
        self.asm.sel.clear();
        self.asm.sub.clear();
    }

    /// Place of a part that has a connection on an edge: it lands by it. Any
    /// other part stays with the Place tool already started.
    pub(super) fn smart_drop_start_part(&mut self, r: DocRef) {
        let Some((conn, center, normal)) = self
            .with_part(r, |p| {
                let c = p.connections.iter().find(|c| c.edge.is_some())?;
                let edge = c.edge.clone()?;
                match crate::assembly::relation::resolve_in(
                    p,
                    &GeomRef::Edge {
                        occurrence: 0,
                        sub: Vec::new(),
                        edge,
                    },
                ) {
                    Ok(Geom::Circle { c: at, n, .. }) => Some((c.clone(), at, n)),
                    _ => None,
                }
            })
            .flatten()
        else {
            return;
        };
        let Some(edge) = conn.edge.clone() else {
            return;
        };
        let fit = match conn.role {
            ConnectionRole::Into => Fit::Into {
                d: conn.diameter,
                length: None,
                sink: 0.0,
            },
            ConnectionRole::Over => Fit::Over {
                bore: conn.diameter,
            },
        };
        self.asm.smart = Some(SmartDrop {
            catalog: Catalog::default(),
            family: 0,
            detail: Detail::Standard,
            fits: vec![fit],
            chosen: None,
            flip: false,
            follow_pattern: false,
            insert_multiple: true,
            lock_rotation: false,
            bolted: false,
            hover: None,
            cylinder: None,
            meshes: Default::default(),
            single: Some(Single {
                component: r,
                name: self.asm.place_name.clone(),
                edge,
                center,
                normal,
            }),
            owned: Default::default(),
        });
        self.asm.tool = Some(AsmTool::SmartDrop);
    }

    /// A part of this file opens with its saved body, which has no triangles
    /// (`Project::rebuild_unmeshed_parts`) and no hole list; what Smart Drop reads
    /// needs them, so the part under the cursor is built once. Rebuilding what
    /// was saved is no change of the user's.
    fn drop_prepare(&mut self, g: &GeomRef) {
        let Some((component, _)) = self.geom_part(g) else {
            return;
        };
        if component.library.is_some() {
            return;
        }
        let ready = self
            .with_part(component, |p| p.body.faces().any(|(_, f)| f.mesh.is_some()))
            .unwrap_or(true);
        if ready {
            return;
        }
        let was_saved = self.project_signature_now() == self.saved_sig;
        if let Some(DocumentData::Part(p)) = self
            .project
            .document_mut(component.document)
            .map(|d| &mut d.data)
        {
            let _ = p.regenerate();
        }
        if was_saved {
            self.mark_saved();
        }
    }

    /// The picked item as a place to land on.
    fn drop_spot(&self, g: &GeomRef) -> Option<Spot> {
        let (component, place) = self.geom_part(g)?;
        let local = self.asm_local_geom(g).ok()?;
        let (occurrence, sub) = (g.occurrence(), g.sub().to_vec());
        let cylinder = |f: &FaceRef| {
            self.with_part(component, |p| hole::cylinder_face(&p.body, f).ok())
                .flatten()
        };
        match (g, local) {
            (GeomRef::Edge { edge, .. }, Geom::Circle { c, n, r }) => {
                let refs: Vec<FaceRef> = edge
                    .faces
                    .iter()
                    .map(|name| FaceRef {
                        face: name.clone(),
                        hint: edge.hint,
                    })
                    .collect();
                let wall = refs.iter().find_map(&cylinder);
                // The other face of the edge, when it is a plane, carries the seat;
                // a chamfered shaft's edge has none, so a plane is asked for next.
                let plane = edge.faces.iter().find_map(|name| {
                    let r = GeomRef::Face {
                        occurrence,
                        sub: sub.clone(),
                        name: name.clone(),
                        hint: edge.hint,
                    };
                    matches!(self.asm_local_geom(&r), Ok(Geom::Plane { .. })).then_some(r)
                });
                let world = Geom::Circle { c, n, r }.moved(&place);
                let Geom::Circle { c, n, .. } = world else {
                    return None;
                };
                let h = self.hole_data(component, &Point3::from(c.coords), &place, n);
                Some(Spot {
                    key: g.clone(),
                    axis: g.clone(),
                    from_rim: plane.is_some(),
                    plane,
                    center: c,
                    normal: n,
                    radius: r,
                    target: Target {
                        hole: wall.as_ref().is_none_or(|w| w.internal),
                        diameter: 2.0 * r,
                        thread: h.thread,
                        depth: wall.as_ref().map_or(0.0, |w| w.length),
                        blind: h.blind,
                    },
                    hole_feature: h.feature,
                    hole_order: h.order,
                })
            }
            (GeomRef::Face { name, hint, .. }, Geom::Cylinder { p, d, r }) => {
                let wall = cylinder(&FaceRef {
                    face: name.clone(),
                    hint: *hint,
                });
                let at = place * wall.as_ref().map_or(p, |w| w.start);
                let axis = place * d;
                let h = self.hole_data(component, &at, &place, axis);
                Some(Spot {
                    key: g.clone(),
                    axis: g.clone(),
                    plane: None,
                    from_rim: false,
                    center: at,
                    normal: axis,
                    radius: r,
                    target: Target {
                        hole: wall.as_ref().is_some_and(|w| w.internal),
                        diameter: 2.0 * r,
                        thread: h.thread,
                        depth: wall.as_ref().map_or(0.0, |w| w.length),
                        blind: h.blind,
                    },
                    hole_feature: h.feature,
                    hole_order: h.order,
                })
            }
            _ => None,
        }
    }

    /// A Hole feature's hole on the axis through `at`: its thread, whether it
    /// ends inside, and the feature.
    fn hole_data(
        &self,
        component: DocRef,
        at: &Point3,
        place: &Transform3,
        axis: Vector3,
    ) -> HoleData {
        let local = place.inverse() * at;
        let dir = place.inverse() * axis;
        self.with_part(component, |p| {
            p.holes()
                .iter()
                .enumerate()
                .find(|(_, h)| {
                    let w = h.origin - local;
                    h.direction.cross(&dir).norm() < 1e-6 && h.direction.cross(&w).norm() < 1e-3
                })
                .map(|(k, h)| HoleData {
                    thread: h.spec.thread.as_ref().map(|t| t.major),
                    blind: matches!(h.spec.termination, Termination::Distance),
                    feature: Some(h.feature.clone()),
                    order: Some(k),
                })
        })
        .flatten()
        .unwrap_or_default()
    }

    /// A family member's mesh (a member is written once).
    fn drop_item_mesh(&mut self, family: usize, member: usize) -> Option<Arc<PickMesh>> {
        if let Some(r) = self
            .asm
            .smart
            .as_ref()
            .and_then(|s| s.single.as_ref().map(|x| x.component))
        {
            return self.asm_mesh(r);
        }
        let s = self.asm.smart.as_ref()?;
        if let Some(m) = s.meshes.get(&(family, member)) {
            return m.clone();
        }
        let path = s.catalog.member_file_at(family, member, s.detail).ok();
        let mesh = path.and_then(|p| self.file_mesh(&p));
        if let Some(s) = self.asm.smart.as_mut() {
            s.meshes.insert((family, member), mesh.clone());
        }
        mesh
    }

    /// How a published family's member lands: its connection's edge, read from
    /// the member itself (the edge's size follows the row).
    fn drop_owned(&mut self, member: usize) -> Option<Single> {
        let s = self.asm.smart.as_ref()?;
        let fam = s.catalog.families.get(s.family)?;
        fam.file.as_ref()?;
        if let Some(x) = s.owned.get(&member) {
            return x.clone();
        }
        let name = fam
            .members
            .get(member)
            .map(|m| format!("{} {}", fam.name, m.size))?;
        let path = s.catalog.member_file_at(s.family, member, s.detail).ok();
        let found = path
            .and_then(|p| self.part_file_pub(&p).ok())
            .and_then(|project| {
                project.documents().iter().find_map(|d| match &d.data {
                    DocumentData::Part(p) => own_landing(p, &name),
                    _ => None,
                })
            });
        if let Some(s) = self.asm.smart.as_mut() {
            s.owned.insert(member, found.clone());
        }
        found
    }

    /// What a click places for `member` on `spot`, in order.
    fn drop_items(&self, spot: &Spot, member: usize) -> Vec<Item> {
        let Some(s) = self.asm.smart.as_ref() else {
            return Vec::new();
        };
        let one = vec![Item {
            family: s.family,
            member,
            sink: self.drop_fit(member).map_or(0.0, drop_sink),
            flip: self.drop_flipped(spot, member),
        }];
        let Some(Fit::Into { d, .. }) = self.drop_fit(member) else {
            return one;
        };
        if !s.bolted || !spot.target.hole || spot.target.blind || s.single.is_some() {
            return one;
        }
        let Some(stack) = s.catalog.bolted_stack(s.family, d) else {
            return one;
        };
        let mut t = spot.target;
        t.depth = stack.length_for(spot.target.depth, d);
        let bolt = s
            .chosen
            .or_else(|| fit::choose(&s.fits, &t))
            .unwrap_or(member);
        let depth = spot.target.depth;
        let (h, flip) = (
            stack.washer_h,
            self.asm.smart.as_ref().is_some_and(|x| x.flip),
        );
        vec![
            Item {
                family: stack.washer.0,
                member: stack.washer.1,
                sink: 0.0,
                flip,
            },
            Item {
                family: s.family,
                member: bolt,
                sink: -h,
                flip,
            },
            Item {
                family: stack.washer.0,
                member: stack.washer.1,
                sink: depth,
                flip: !flip,
            },
            Item {
                family: stack.nut.0,
                member: stack.nut.1,
                sink: depth + h,
                flip: !flip,
            },
        ]
    }

    /// Hover: what the cursor is on, which size, drawn see-through in place.
    pub(super) fn smart_drop_view(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        if self.asm.tool != Some(AsmTool::SmartDrop) {
            self.asm.smart = None;
            return;
        }
        let Some(p) = pointer else {
            if let Some(s) = self.asm.smart.as_mut() {
                s.hover = None;
            }
            return;
        };
        let g = self.geom_under_pub(p, rect);
        let waiting_plane = self
            .asm
            .smart
            .as_ref()
            .is_some_and(|s| s.cylinder.is_some());
        if waiting_plane {
            self.drop_plane_view(painter, g, p);
            return;
        }
        let same = self
            .asm
            .smart
            .as_ref()
            .and_then(|s| s.hover.as_ref())
            .is_some_and(|(spot, _)| Some(&spot.key) == g.as_ref());
        if !same {
            if let Some(g) = &g {
                self.drop_prepare(g);
            }
            let spot = g.as_ref().and_then(|g| self.drop_spot(g));
            if let Some(s) = self.asm.smart.as_mut() {
                s.hover = spot.map(|spot| {
                    let member = s.chosen.or_else(|| fit::choose(&s.fits, &spot.target));
                    (spot, member)
                });
            }
        }
        let Some((spot, member)) = self.asm.smart.as_ref().and_then(|s| s.hover.clone()) else {
            return;
        };
        self.draw_spot(painter, &spot, member, p);
    }

    /// The second step after a cylinder: the plane under the cursor.
    fn drop_plane_view(&mut self, painter: &egui::Painter, g: Option<GeomRef>, p: Pos2) {
        let plane = g.and_then(|g| self.asm_world_plane(&g).map(|(at, n)| (g, at, n)));
        let (cyl, member) = {
            let Some(s) = self.asm.smart.as_ref() else {
                return;
            };
            let Some(c) = s.cylinder.clone() else { return };
            let m = s.chosen.or_else(|| fit::choose(&s.fits, &c.target));
            (c, m)
        };
        match plane {
            Some((_, at, n)) => {
                let mut spot = cyl;
                spot.center = spot.center + spot.normal * spot.normal.dot(&(at - spot.center));
                spot.normal = n;
                self.draw_spot(painter, &spot, member, p);
            }
            None => {
                painter.text(
                    p + egui::vec2(16.0, 16.0),
                    egui::Align2::LEFT_TOP,
                    "Now click the plane it sits on",
                    egui::FontId::proportional(13.0),
                    Color32::from_rgb(235, 235, 235),
                );
            }
        }
    }

    /// A face's plane in the assembly's coordinates.
    fn asm_world_plane(&self, g: &GeomRef) -> Option<(Point3, Vector3)> {
        if !matches!(g, GeomRef::Face { .. }) {
            return None;
        }
        let (_, place) = self.geom_part(g)?;
        match self.asm_local_geom(g).ok()?.moved(&place) {
            Geom::Plane { p, n } => Some((p, n)),
            _ => None,
        }
    }

    fn draw_spot(
        &mut self,
        painter: &egui::Painter,
        spot: &Spot,
        member: Option<usize>,
        cursor: Pos2,
    ) {
        let green = Color32::from_rgb(90, 200, 110);
        let red = Color32::from_rgb(235, 80, 70);
        let color = if member.is_some() { green } else { red };
        // The circle it lands on.
        let n = spot.normal;
        let helper = if n.x.abs() < 0.9 {
            Vector3::x()
        } else {
            Vector3::y()
        };
        let u = n.cross(&helper).normalize() * spot.radius;
        let v = n.cross(&u);
        let pts: Vec<Pos2> = (0..=48)
            .filter_map(|i| {
                let a = i as Real / 48.0 * std::f64::consts::TAU;
                let q = spot.center + u * a.cos() + v * a.sin();
                self.camera.project(&q, painter.clip_rect())
            })
            .collect();
        if pts.len() > 2 {
            painter.add(egui::Shape::line(pts, Stroke::new(2.0, color)));
        }
        let Some(member) = member else {
            let what = self
                .asm
                .smart
                .as_ref()
                .and_then(|s| s.catalog.families.get(s.family))
                .map(|f| f.name.clone())
                .unwrap_or_default();
            painter.text(
                cursor + egui::vec2(16.0, 34.0),
                egui::Align2::LEFT_TOP,
                format!("x  No size of {what} fits {:.2} mm", 2.0 * spot.radius),
                egui::FontId::proportional(13.0),
                red,
            );
            return;
        };
        let mut label = self.drop_member_name(member);
        if self.drop_owned(member).is_none()
            && self.asm.smart.as_ref().is_some_and(|s| s.single.is_none())
        {
            let items = self.drop_items(spot, member);
            if items.len() > 1 {
                label = format!("{label}, washers and nut");
                if let Some(bolt) = items.get(1) {
                    label = format!("{}, washers and nut", self.item_name(bolt));
                }
            }
            for item in items {
                if let Some(mesh) = self.drop_item_mesh(item.family, item.member) {
                    let t = fit::seat_transform(spot.center, spot.normal, item.sink, item.flip);
                    super::place::draw_ghost(painter, &self.camera, painter.clip_rect(), &mesh, &t);
                }
            }
        } else if let Some(s) = self.asm.smart.as_ref() {
            let family = s.family;
            if let Some(mesh) = self.drop_item_mesh(family, member) {
                let t = self.drop_transform(spot, member);
                super::place::draw_ghost(painter, &self.camera, painter.clip_rect(), &mesh, &t);
            }
        }
        painter.text(
            cursor + egui::vec2(16.0, 34.0),
            egui::Align2::LEFT_TOP,
            format!("✔  {label}"),
            egui::FontId::proportional(13.0),
            green,
        );
    }

    fn drop_member_name(&self, member: usize) -> String {
        self.asm
            .smart
            .as_ref()
            .and_then(|s| {
                if let Some(x) = &s.single {
                    return Some(x.name.clone());
                }
                let f = s.catalog.families.get(s.family)?;
                let m = f.members.get(member)?;
                Some(format!("{} {}", f.name, m.size))
            })
            .unwrap_or_default()
    }

    fn item_name(&self, item: &Item) -> String {
        self.asm
            .smart
            .as_ref()
            .and_then(|s| {
                let f = s.catalog.families.get(item.family)?;
                let m = f.members.get(item.member)?;
                Some(format!("{} {}", f.name, m.size))
            })
            .unwrap_or_default()
    }

    fn drop_transform(&self, spot: &Spot, member: usize) -> Transform3 {
        let smart = self.asm.smart.as_ref();
        let own = smart.and_then(|s| {
            s.single
                .as_ref()
                .or_else(|| s.owned.get(&member).and_then(Option::as_ref))
        });
        if let Some(x) = own {
            let flip = smart.is_some_and(|s| s.flip);
            return fit::landing(x.center, x.normal, spot.center, spot.normal, flip);
        }
        let sink = self.drop_fit(member).map_or(0.0, drop_sink);
        fit::seat_transform(
            spot.center,
            spot.normal,
            sink,
            self.drop_flipped(spot, member),
        )
    }

    fn drop_fit(&self, member: usize) -> Option<Fit> {
        self.asm.smart.as_ref()?.fits.get(member).copied()
    }

    /// Its seat turned into the face: asked for, or how it goes in. Flip turns
    /// over what it would otherwise do.
    fn drop_flipped(&self, spot: &Spot, member: usize) -> bool {
        let asked = self.asm.smart.as_ref().is_some_and(|s| s.flip);
        let goes = self
            .drop_fit(member)
            .is_some_and(|f| fit::goes_in(f, &spot.target, spot.from_rim));
        asked != goes
    }

    /// A click in the view.
    pub(super) fn smart_drop_click(&mut self, pointer: Option<Pos2>, rect: Rect) {
        let Some(p) = pointer else { return };
        let g = self.geom_under_pub(p, rect);
        if let Some(g) = &g {
            self.drop_prepare(g);
        }
        if self
            .asm
            .smart
            .as_ref()
            .is_some_and(|s| s.cylinder.is_some())
        {
            let Some((g, at, n)) = g.and_then(|g| {
                let (at, n) = self.asm_world_plane(&g)?;
                Some((g, at, n))
            }) else {
                self.file_status = Some(Err("Smart Drop: click a plane for the seat".into()));
                return;
            };
            let Some(mut spot) = self.asm.smart.as_mut().and_then(|s| s.cylinder.take()) else {
                return;
            };
            spot.center = spot.center + spot.normal * spot.normal.dot(&(at - spot.center));
            spot.normal = n;
            spot.plane = Some(g);
            self.drop_commit(spot);
            return;
        }
        let Some(spot) = g.as_ref().and_then(|g| self.drop_spot(g)) else {
            self.file_status = Some(Err(
                "Smart Drop: click a circular edge or a cylindrical face".into(),
            ));
            return;
        };
        if spot.plane.is_none() {
            if self.asm.smart.as_ref().is_some_and(|s| s.single.is_some()) {
                self.file_status = Some(Err(
                    "Place: click the circular edge of a hole or a shaft's end".into(),
                ));
                return;
            }
            if let Some(s) = self.asm.smart.as_mut() {
                s.cylinder = Some(spot);
            }
            return;
        }
        self.drop_commit(spot);
    }

    /// The spots Follow Pattern adds: the other holes of the same Hole
    /// feature, on the same face of the same occurrence.
    fn drop_pattern(&self, spot: &Spot) -> Vec<Spot> {
        let Some(feature) = &spot.hole_feature else {
            return Vec::new();
        };
        let Some((component, _)) = self.geom_part(&spot.axis) else {
            return Vec::new();
        };
        let Some(edges) = self.asm_edges_pub(component) else {
            return Vec::new();
        };
        let (occurrence, sub) = (spot.axis.occurrence(), spot.axis.sub().to_vec());
        let mut out = Vec::new();
        for (edge, _) in edges.iter() {
            let r = GeomRef::Edge {
                occurrence,
                sub: sub.clone(),
                edge: edge.clone(),
            };
            if r == spot.axis {
                continue;
            }
            let Some(other) = self.drop_spot(&r) else {
                continue;
            };
            let same_face = other.plane == spot.plane
                || matches!(
                    (&other.plane, &spot.plane),
                    (Some(GeomRef::Face { name: a, .. }), Some(GeomRef::Face { name: b, .. })) if a == b
                );
            if same_face
                && other.hole_feature.as_ref() == Some(feature)
                && (other.radius - spot.radius).abs() < 1e-6
                && (other.normal - spot.normal).norm() < 1e-6
                && !out
                    .iter()
                    .any(|o: &Spot| (o.center - other.center).norm() < 1e-6)
                && (other.center - spot.center).norm() > 1e-6
            {
                out.push(other);
            }
        }
        out
    }

    /// A part placed by its connection: one Insert of its edge into the rim.
    fn drop_commit_single(&mut self, spot: Spot, x: Single) {
        let fits = self
            .asm
            .smart
            .as_ref()
            .is_some_and(|s| fit::choose(&s.fits, &spot.target).is_some());
        if !fits {
            self.file_status = Some(Err(format!(
                "Place: its connection does not fit {:.2} mm",
                2.0 * spot.radius
            )));
            return;
        }
        if !matches!(spot.axis, GeomRef::Edge { .. }) {
            self.file_status = Some(Err("Place: click a circular edge".into()));
            return;
        }
        let t = self.drop_transform(&spot, 0);
        let (flip, multiple) = self
            .asm
            .smart
            .as_ref()
            .map_or((false, false), |s| (s.flip, s.insert_multiple));
        let mut placed = 0;
        let edge = x.edge.clone();
        let res = self.asm_try("Place", |a| {
            placed = a.place(x.component, &x.name, t, false);
            let mut c = Constraint::new(
                ConstraintKind::Insert,
                spot.axis.clone(),
                GeomRef::Edge {
                    occurrence: placed,
                    sub: Vec::new(),
                    edge,
                },
            );
            c.aligned = flip;
            a.add_constraint(c);
        });
        if let Err(e) = res {
            self.file_status = Some(Err(format!("Place: {e}")));
            return;
        }
        self.asm.sel = vec![placed];
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!("Place: {} landed by its connection", x.name)));
        if multiple {
            if let Some(s) = self.asm.smart.as_mut() {
                s.hover = None;
            }
        } else {
            self.asm.tool = None;
            self.asm.smart = None;
        }
    }

    /// Links the member and places it with its two relationships. All or
    /// nothing (rule 5).
    fn drop_commit(&mut self, spot: Spot) {
        if let Some(x) = self.asm.smart.as_ref().and_then(|s| s.single.clone()) {
            self.drop_commit_single(spot, x);
            return;
        }
        if self.drop_commit_owned(&spot) {
            return;
        }
        let Some((family, detail, member, follow, multiple)) = self.asm.smart.as_ref().map(|s| {
            (
                s.family,
                s.detail,
                s.chosen.or_else(|| fit::choose(&s.fits, &spot.target)),
                s.follow_pattern,
                s.insert_multiple,
            )
        }) else {
            return;
        };
        let Some(member) = member else {
            self.file_status = Some(Err(format!(
                "Smart Drop: no size fits {:.2} mm",
                2.0 * spot.radius
            )));
            return;
        };
        let _ = (family, detail);
        let mut spots = vec![spot];
        let mut pattern = None;
        if follow {
            let more = self.drop_pattern(&spots[0]);
            let whole = self.drop_whole_feature(&spots[0], more.len());
            spots.extend(more);
            // Every hole of the feature, directly in this assembly: the copies
            // follow it from now on. Otherwise plain placements.
            if let Some((occurrence, feature)) = whole.filter(|_| spots.len() > 1) {
                spots.sort_by_key(|s| s.hole_order);
                spots.truncate(1);
                pattern = Some((occurrence, feature));
            }
        }
        let holes = if pattern.is_some() {
            pattern
                .as_ref()
                .and_then(|(_, f)| self.feature_hole_count(&spots[0], f))
                .unwrap_or(1)
        } else {
            spots.len()
        };
        let label = "Smart Drop";
        let items = self.drop_items(&spots[0], member);
        // Every file first: nothing is changed when one cannot be written.
        let mut files = Vec::new();
        for item in &items {
            let found = self
                .asm
                .smart
                .as_ref()
                .map(|s| s.catalog.member_file_at(item.family, item.member, s.detail));
            let path: PathBuf = match found {
                Some(Ok(p)) => p,
                Some(Err(e)) => {
                    self.file_status = Some(Err(format!("Smart Drop: {e}")));
                    return;
                }
                None => return,
            };
            let Some(doc) = self.part_file_pub(&path).ok().and_then(|p| {
                p.documents()
                    .iter()
                    .find(|d| matches!(d.data, DocumentData::Part(_)))
                    .map(|d| d.id)
            }) else {
                self.file_status = Some(Err("Smart Drop: the member's file holds no part".into()));
                return;
            };
            files.push((path, doc, self.item_name(item)));
        }
        self.checkpoint(label);
        let owner = PathBuf::from(self.file_path.trim());
        let mut resolver = crate::document::Resolver::new();
        let mut links = Vec::new();
        for (path, doc, name) in &files {
            match resolver.link(&mut self.project, &owner, path, *doc) {
                Ok(r) => links.push((r, name.clone())),
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Smart Drop: {e}")));
                    return;
                }
            }
        }
        // Lock Rotation holds each part's X axis at its angle to an axis of the
        // target; the target's axis most square to the hole is the steadiest.
        let lock = self.asm.smart.as_ref().is_some_and(|s| s.lock_rotation);
        let reference: Vec<Option<(OriginItem, Vector3)>> = spots
            .iter()
            .map(|spot| {
                if !lock {
                    return None;
                }
                let (_, place) = self.geom_part(&spot.axis)?;
                [
                    (OriginItem::XAxis, Vector3::x()),
                    (OriginItem::YAxis, Vector3::y()),
                    (OriginItem::ZAxis, Vector3::z()),
                ]
                .into_iter()
                .map(|(i, v)| (i, place * v))
                .min_by(|a, b| {
                    a.1.dot(&spot.normal)
                        .abs()
                        .total_cmp(&b.1.dot(&spot.normal).abs())
                })
            })
            .collect();
        let first_name = items
            .get(usize::from(items.len() > 1))
            .map(|i| self.item_name(i))
            .unwrap_or_default();
        let mut placed = Vec::new();
        let count = holes;
        let too_long = spots[0].target.blind
            && items.iter().any(|i| {
                i.family == family && self.asm.smart.as_ref().is_some_and(|s| {
                    matches!(
                        s.fits.get(i.member),
                        Some(Fit::Into { length: Some(l), .. }) if *l > spots[0].target.depth + 0.05
                    )
                })
            });
        let depth = spots[0].target.depth;
        let res = self.asm_try(label, |a| {
            for (spot, axis_ref) in spots.iter().zip(&reference) {
                for (item, (r, name)) in items.iter().zip(&links) {
                    let t = fit::seat_transform(spot.center, spot.normal, item.sink, item.flip);
                    let id = a.place(*r, name, t, false);
                    placed.push(id);
                    let origin = |item: OriginItem| GeomRef::Origin {
                        occurrence: id,
                        sub: Vec::new(),
                        item,
                    };
                    a.add_constraint(Constraint::new(
                        ConstraintKind::Mate,
                        spot.axis.clone(),
                        origin(OriginItem::ZAxis),
                    ));
                    if let Some(plane) = &spot.plane {
                        let mut c = Constraint::new(
                            if item.flip {
                                ConstraintKind::Mate
                            } else {
                                ConstraintKind::Flush
                            },
                            plane.clone(),
                            origin(OriginItem::XyPlane),
                        );
                        c.value = -item.sink;
                        a.add_constraint(c);
                    }
                    if let Some((which, dir)) = axis_ref {
                        let x = t * Vector3::x();
                        let mut c = Constraint::new(
                            ConstraintKind::Angle,
                            GeomRef::Origin {
                                occurrence: spot.axis.occurrence(),
                                sub: spot.axis.sub().to_vec(),
                                item: *which,
                            },
                            origin(OriginItem::XAxis),
                        );
                        c.value = x.dot(dir).clamp(-1.0, 1.0).acos();
                        a.add_constraint(c);
                    }
                }
            }
            if let Some((occurrence, feature)) = &pattern {
                let id = a.next_pattern_id();
                a.patterns.push(crate::assembly::pattern::ComponentPattern {
                    id,
                    name: format!("Component Pattern {id}"),
                    sources: placed.clone(),
                    layout: crate::assembly::pattern::Layout::Feature {
                        occurrence: *occurrence,
                        feature: feature.clone(),
                    },
                    elements: Vec::new(),
                });
            }
        });
        if let Err(e) = res {
            self.history.cancel();
            self.file_status = Some(Err(format!("Smart Drop: {e}")));
            return;
        }
        self.asm.sel = placed;
        self.scene_dirty = true;
        let what = if items.len() > 1 {
            format!("{first_name} with washers and nut")
        } else {
            first_name
        };
        self.file_status = Some(Ok(if too_long {
            format!(
                "Smart Drop: {what} placed; it is longer than the {depth:.1} mm hole, choose another Size or check the depth"
            )
        } else if count == 1 {
            format!("Smart Drop: {what} placed")
        } else {
            format!("Smart Drop: {count} x {what} placed (Follow Pattern)")
        }));
        if multiple {
            if let Some(s) = self.asm.smart.as_mut() {
                s.hover = None;
            }
        } else {
            self.asm.tool = None;
            self.asm.smart = None;
        }
    }

    /// The target's occurrence and Hole feature, when `more` other spots are
    /// all the holes that feature has, directly below this assembly.
    fn drop_whole_feature(&self, spot: &Spot, more: usize) -> Option<(u32, String)> {
        if !spot.axis.sub().is_empty() {
            return None;
        }
        let feature = spot.hole_feature.clone()?;
        let count = self.feature_hole_count(spot, &feature)?;
        (count == more + 1).then_some((spot.axis.occurrence(), feature))
    }

    fn feature_hole_count(&self, spot: &Spot, feature: &str) -> Option<usize> {
        let (component, _) = self.geom_part(&spot.axis)?;
        self.with_part(component, |p| {
            p.holes().iter().filter(|h| h.feature == feature).count()
        })
    }

    /// A published family's member goes in by its connection: one Insert.
    /// `false`: this family is a built-in one.
    fn drop_commit_owned(&mut self, spot: &Spot) -> bool {
        let Some((family, member, detail)) = self.asm.smart.as_ref().and_then(|s| {
            let f = s.catalog.families.get(s.family)?;
            f.file.as_ref()?;
            Some((
                s.family,
                s.chosen.or_else(|| fit::choose(&s.fits, &spot.target)),
                s.detail,
            ))
        }) else {
            return false;
        };
        let Some(member) = member else {
            self.file_status = Some(Err(format!(
                "Smart Drop: no size fits {:.2} mm",
                2.0 * spot.radius
            )));
            return true;
        };
        let Some(mut x) = self.drop_owned(member) else {
            self.file_status = Some(Err("Smart Drop: the member has no connection".into()));
            return true;
        };
        let path = self
            .asm
            .smart
            .as_ref()
            .map(|s| s.catalog.member_file_at(family, member, detail));
        let Some(Ok(path)) = path else {
            self.file_status = Some(Err("Smart Drop: the member's file cannot be written".into()));
            return true;
        };
        let Some(doc) = self.part_file_pub(&path).ok().and_then(|p| {
            p.documents()
                .iter()
                .find(|d| matches!(d.data, DocumentData::Part(_)))
                .map(|d| d.id)
        }) else {
            self.file_status = Some(Err("Smart Drop: the member's file holds no part".into()));
            return true;
        };
        self.checkpoint("Place");
        let owner = PathBuf::from(self.file_path.trim());
        let mut resolver = crate::document::Resolver::new();
        match resolver.link(&mut self.project, &owner, &path, doc) {
            Ok(r) => x.component = r,
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("Smart Drop: {e}")));
                return true;
            }
        }
        self.drop_commit_single(spot.clone(), x);
        true
    }

    /// The toolbar (spec 6.3 §3.5).
    pub(super) fn smart_drop_window(&mut self, ctx: &egui::Context) {
        let Some(mut s) = self.asm.smart.take() else {
            return;
        };
        let mut done = false;
        let mut manual = None;
        let family = s.catalog.families.get(s.family).cloned();
        let w = egui::Window::new("Smart Drop")
            .resizable(false)
            .collapsible(false)
            .default_pos([420.0, 130.0]);
        self.layout
            .place("smart_drop", ctx.content_rect(), false, w)
            .show(ctx, |ui| {
                if let Some(x) = &s.single {
                    ui.label(format!("{}: lands by its connection", x.name));
                } else if let Some(f) = &family {
                    ui.label(format!("{}: {}", f.name, f.category.replace('/', " › ")));
                    let shown = s
                        .chosen
                        .and_then(|m| f.members.get(m))
                        .map_or("Nearest to the target", |m| m.size.as_str())
                        .to_owned();
                    ui.horizontal(|ui| {
                        ui.label("Size");
                        egui::ComboBox::from_id_salt("smart_drop_size")
                            .selected_text(shown)
                            .width(170.0)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(s.chosen.is_none(), "Nearest to the target")
                                    .clicked()
                                {
                                    s.chosen = None;
                                    s.hover = None;
                                }
                                for (i, m) in f.members.iter().enumerate() {
                                    if ui.selectable_label(s.chosen == Some(i), &m.size).clicked() {
                                        s.chosen = Some(i);
                                        s.hover = None;
                                    }
                                }
                            });
                    });
                }
                ui.checkbox(&mut s.flip, "Flip")
                    .on_hover_text("Turn it over: its seat goes into the face");
                ui.add_enabled(
                    s.single.is_none(),
                    egui::Checkbox::new(&mut s.follow_pattern, "Follow Pattern"),
                )
                .on_hover_text("One in every hole of the same Hole feature on this face")
                .on_disabled_hover_text("Only a library family follows a pattern for now");
                ui.checkbox(&mut s.insert_multiple, "Insert Multiple")
                    .on_hover_text("The tool stays after a placement");
                let plain = s.single.is_none();
                ui.add_enabled(plain, egui::Checkbox::new(&mut s.lock_rotation, "Lock Rotation"))
                    .on_hover_text("It cannot turn about its axis");
                let bolts = s
                    .fits
                    .iter()
                    .any(|f| matches!(f, Fit::Into { length: Some(_), .. }));
                let was = s.bolted;
                ui.add_enabled(plain && bolts, egui::Checkbox::new(&mut s.bolted, "Bolted Connection"))
                    .on_hover_text("Through a hole: a washer under the head, a washer and a nut on the far side, and a bolt long enough for them")
                    .on_disabled_hover_text("Only for bolts of a library family");
                if s.bolted != was {
                    s.hover = None;
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .button("Place by Hand")
                        .on_hover_text("Place this member anywhere, without Smart Drop")
                        .clicked()
                    {
                        manual = s
                            .chosen
                            .or_else(|| s.hover.as_ref().and_then(|h| h.1))
                            .or(Some(0));
                    }
                    if ui.button("Done").clicked() {
                        done = true;
                    }
                });
            });
        if manual.is_some()
            && let Some(x) = &s.single
        {
            // Back to the plain Place tool.
            self.asm.tool = Some(AsmTool::Place(x.component));
            return;
        }
        if let Some(m) = manual
            && let Ok(path) = s.catalog.member_file_at(s.family, m, s.detail)
        {
            done = true;
            if let Some(doc) = self.part_file_pub(&path).ok().and_then(|p| {
                p.documents()
                    .iter()
                    .find(|d| matches!(d.data, DocumentData::Part(_)))
                    .map(|d| d.id)
            }) {
                self.place_chosen(path, doc);
                return;
            }
        }
        if done {
            self.asm.tool = None;
            return;
        }
        self.asm.smart = Some(s);
    }
}

impl CadCraftApp {
    /// Connection's OK: the picked circular edge becomes a named connection.
    pub(super) fn connection_ok(&mut self, d: &super::part_planned::PartDialog) -> bool {
        let fail = |app: &mut Self, why: String| {
            app.file_status = Some(Err(format!("Connection: {why}")));
            false
        };
        let Some((super::select::SelItem::Edge(e), _)) = d.items_of("edge").first().cloned() else {
            return fail(self, "select a circular edge".into());
        };
        let part = self.part();
        let Some(edge) = crate::part::blend::edge_ref(&part.body, e) else {
            return fail(self, "the edge has no name".into());
        };
        let geom = crate::assembly::relation::resolve_in(
            part,
            &GeomRef::Edge {
                occurrence: 0,
                sub: Vec::new(),
                edge: edge.clone(),
            },
        );
        let Ok(Geom::Circle { r, .. }) = geom else {
            return fail(self, "select a circular edge".into());
        };
        let typed = d.text(0, "Name").trim().to_owned();
        let name = if typed.is_empty() {
            crate::part::connection::next_name(&part.connections)
        } else {
            typed
        };
        if part.connections.iter().any(|c| c.name == name) {
            return fail(self, format!("{name} already exists"));
        }
        let role = if d.index(0, "Role") == 1 {
            ConnectionRole::Over
        } else {
            ConnectionRole::Into
        };
        self.checkpoint("Connection");
        self.part_mut().connections.push(Connection {
            name: name.clone(),
            role,
            edge: Some(edge),
            diameter: 2.0 * r,
            diameter_column: Some(d.text(0, "Diameter Column").trim().to_owned())
                .filter(|c| !c.is_empty()),
        });
        self.file_status = Some(Ok(format!(
            "Connection: {name} added, {:.2} mm; placing this part now offers what fits",
            2.0 * r
        )));
        true
    }

    /// The part's connections, listed while the command is open.
    pub(super) fn connections_window(&mut self, ctx: &egui::Context) {
        let open = self.part_planned.as_ref().is_some_and(|d| {
            d.kind
                == super::part_planned::Planned::Part(super::part_planned::PartPlanned::Connection)
        });
        if !open {
            return;
        }
        let list = self.part().connections.clone();
        let mut delete = None;
        let w = egui::Window::new("Connections of this part")
            .resizable(false)
            .collapsible(false)
            .default_pos([300.0, 130.0]);
        self.layout
            .place("connections", ctx.content_rect(), false, w)
            .show(ctx, |ui| {
                if list.is_empty() {
                    ui.weak("None yet");
                }
                for (i, c) in list.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "{}: {}, {:.2} mm",
                            c.name,
                            c.role.label(),
                            c.diameter
                        ));
                        if ui.small_button("Delete").clicked() {
                            delete = Some(i);
                        }
                    });
                }
            });
        if let Some(i) = delete {
            self.checkpoint("Connection");
            self.part_mut().connections.remove(i);
        }
    }
}

/// The Change Size window (spec 6.3 §3.4).
pub(super) struct ChangeSize {
    catalog: Catalog,
    family: usize,
    member: usize,
    detail: Detail,
    /// The selected occurrences, all of this family.
    ids: Vec<u32>,
    error: Option<String>,
}

impl CadCraftApp {
    /// The member file of an occurrence, when it is a linked file.
    fn occurrence_file(&self, id: u32) -> Option<PathBuf> {
        let o = self.assembly()?.find(id)?;
        self.library_path(o.component.library?)
    }

    /// Are these occurrences parts of the built-in library? (A cheap check,
    /// asked while a menu is drawn.)
    pub(super) fn change_size_offered(&self, ids: &[u32]) -> bool {
        let Some(root) = crate::components::root_dir() else {
            return false;
        };
        let samples = root.join("Samples");
        !ids.is_empty()
            && ids.iter().all(|&i| {
                self.occurrence_file(i).is_some_and(|p| {
                    let name = samples.file_name();
                    p.starts_with(&samples) || p.components().any(|c| Some(c.as_os_str()) == name)
                })
            })
    }

    /// Change Size: the window for the selected parts, or why not.
    pub(super) fn change_size_start(&mut self, ids: &[u32]) {
        let fail = |app: &mut Self, why: &str| {
            app.file_status = Some(Err(format!("Change Size: {why}")));
        };
        if ids.is_empty() {
            return fail(self, "select a part placed from the library first");
        }
        let Some(root) = crate::components::root_dir() else {
            return fail(self, "no user folder for the library");
        };
        let catalog = Catalog::load(&root);
        let mut found: Option<(usize, usize, Detail)> = None;
        for &id in ids {
            let Some(at) = self
                .occurrence_file(id)
                .and_then(|p| catalog.locate_level(&p))
            else {
                return fail(self, "only parts placed from the library can change size");
            };
            match found {
                Some((f, ..)) if f != at.0 => {
                    return fail(self, "the selected parts belong to different families");
                }
                Some(_) => {}
                None => found = Some(at),
            }
        }
        let Some((family, member, detail)) = found else {
            return;
        };
        self.asm.change_size = Some(ChangeSize {
            catalog,
            family,
            member,
            detail,
            ids: ids.to_vec(),
            error: None,
        });
    }

    /// The window.
    pub(super) fn change_size_window(&mut self, ctx: &egui::Context) {
        let Some(mut c) = self.asm.change_size.take() else {
            return;
        };
        let (mut ok, mut cancel, mut open) = (false, false, true);
        let Some(family) = c.catalog.families.get(c.family).cloned() else {
            return;
        };
        let w = egui::Window::new("Change Size")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size([320.0, 420.0]);
        self.layout
            .place("change_size", ctx.content_rect(), true, w)
            .show(ctx, |ui| {
                egui::Panel::bottom("change_size_buttons").show(ui, |ui| {
                    if let Some(e) = &c.error {
                        ui.colored_label(Color32::from_rgb(235, 80, 70), e);
                    }
                    let offered = c.catalog.levels(c.family, c.member);
                    ui.horizontal(|ui| {
                        ui.label("Detail");
                        for d in Detail::ALL {
                            let on = d
                                == if offered.contains(&c.detail) {
                                    c.detail
                                } else {
                                    Detail::Standard
                                };
                            if ui
                                .add_enabled(
                                    offered.contains(&d),
                                    egui::RadioButton::new(on, d.label()),
                                )
                                .on_disabled_hover_text("This size has no such level")
                                .clicked()
                            {
                                c.detail = d;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ok = ui.button("OK").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
                ui.label(format!("{}: {} selected", family.name, c.ids.len()));
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (k, m) in family.members.iter().enumerate() {
                        if ui.selectable_label(c.member == k, &m.size).clicked() {
                            c.member = k;
                        }
                    }
                });
            });
        if cancel || !open {
            return;
        }
        if ok {
            match self.change_size_apply(&c) {
                Ok(()) => return,
                Err(e) => c.error = Some(e),
            }
        }
        self.asm.change_size = Some(c);
    }

    /// Links the new member and swaps the parts; relationships to the part's
    /// origin hold whatever the size, others may lose their geometry (health).
    fn change_size_apply(&mut self, c: &ChangeSize) -> Result<(), String> {
        let path = c.catalog.member_file_at(c.family, c.member, c.detail)?;
        let project = self.part_file_pub(&path)?;
        let doc = project
            .documents()
            .iter()
            .find(|d| matches!(d.data, DocumentData::Part(_)))
            .map(|d| d.id)
            .ok_or("the member's file holds no part")?;
        let name = c
            .catalog
            .families
            .get(c.family)
            .and_then(|f| Some(format!("{} {}", f.name, f.members.get(c.member)?.size)))
            .unwrap_or_default();
        self.checkpoint("Change Size");
        let owner = PathBuf::from(self.file_path.trim());
        let mut resolver = crate::document::Resolver::new();
        let r = match resolver.link(&mut self.project, &owner, &path, doc) {
            Ok(r) => r,
            Err(e) => {
                self.history.cancel();
                return Err(e.to_string());
            }
        };
        let mut trial = self.assembly().ok_or("no assembly")?.clone();
        if let Err(e) = trial.replace(&c.ids, r, &name) {
            self.history.cancel();
            return Err(e);
        }
        let ids = c.ids.clone();
        if let Err(e) = self.asm_try("Change Size", |a| {
            let _ = a.replace(&ids, r, &name);
        }) {
            self.history.cancel();
            return Err(e);
        }
        self.scene_dirty = true;
        let sick = self
            .cons
            .last
            .sick
            .iter()
            .filter(|(k, _)| {
                self.assembly()
                    .and_then(|a| a.constraint(*k))
                    .is_some_and(|k| ids.iter().any(|&t| k.uses(t)))
            })
            .count();
        self.file_status = Some(Ok(if sick == 0 {
            format!("Change Size: {} now {name}", ids.len())
        } else {
            format!(
                "Change Size: {} now {name}; {sick} relationship(s) lost their geometry (sick)",
                ids.len()
            )
        }));
        Ok(())
    }
}

/// How a part lands by its first connection on an edge: `None` without one.
fn own_landing(p: &crate::part::PartDocument, name: &str) -> Option<Single> {
    let c = p.connections.iter().find(|c| c.edge.is_some())?;
    let edge = c.edge.clone()?;
    let Ok(Geom::Circle { c: center, n, .. }) = crate::assembly::relation::resolve_in(
        p,
        &GeomRef::Edge {
            occurrence: 0,
            sub: Vec::new(),
            edge: edge.clone(),
        },
    ) else {
        return None;
    };
    Some(Single {
        // The link is made when it is placed.
        component: DocRef::local(crate::document::DocumentId(uuid::Uuid::nil())),
        name: name.to_owned(),
        edge,
        center,
        normal: n,
    })
}

/// How far a member's seat lies below the face.
fn drop_sink(fit: Fit) -> Real {
    match fit {
        Fit::Into { sink, .. } => sink,
        _ => 0.0,
    }
}
