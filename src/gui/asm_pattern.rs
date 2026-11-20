//! Pattern Component in the assembly (`docs/spec/3.0-assembly-commands.md`
//! §3.7): the planned panel's Rectangular and Circular tabs become an
//! [`assembly::pattern::Layout`](crate::assembly::pattern::Layout); the copies
//! show see-through while the panel is open, OK adds the pattern (one Undo
//! step), and the browser lists it with its elements (Edit, Delete).
//! The Placements tab (spec 3.7a) copies them to picked places: hole rims,
//! points, faces; its sources turn see-through while places are picked.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::icons::Icon;
use super::part_planned::{PartDialog, Planned};
use crate::assembly::pattern::{self, Anchor, ComponentPattern, Layout, Lookup, PlacePick};
use crate::assembly::relation::{self, Geom, GeomRef};
use crate::document::AssemblyDocument;
use crate::kernel::Transform3;

/// Tabs of the panel.
const ASSOCIATIVE: usize = 0;
const RECTANGULAR: usize = 1;
const CIRCULAR: usize = 2;
const PLACEMENTS: usize = 3;

/// The tag of a frame member's pick in the Components selector (a member
/// of a frame subassembly, patterned inside it: spec 6.1 §3.14).
pub(super) const MEMBER_TAG: &str = "frame member";

/// What the browser asked for.
pub(super) enum PatternCmd {
    Select(u32),
    SelectElement(u32, u32),
    SelectOccurrence(u32),
    Edit(u32),
    Delete(u32),
}

impl CadCraftApp {
    /// A reference's geometry in its occurrence's coordinates, looked up in
    /// assembly `a` (a part inside a subassembly is moved by its place
    /// there, spec 3.1a; parts of this file or linked part files).
    pub(super) fn asm_local_in(&self, a: &AssemblyDocument, r: &GeomRef) -> Result<Geom, String> {
        if let GeomRef::Origin {
            occurrence: relation::ASSEMBLY,
            item,
            ..
        } = r
        {
            return Ok(item.geom());
        }
        let linked = self.linked();
        let n = crate::assembly::tree::nested(&self.project, &linked, a, r.occurrence(), r.sub())?;
        let name = &n.name;
        let g = self
            .with_part(n.component, |p| relation::resolve_in(p, r))
            .ok_or_else(|| format!("{name}: its part file was not found"))?
            .map_err(|e| format!("{name}: {e}"))?;
        Ok(if r.sub().is_empty() {
            g
        } else {
            g.moved(&n.at)
        })
    }

    /// The feature pattern `id` of occurrence `occ`'s part (Associative).
    fn feature_instances_in(
        &self,
        a: &AssemblyDocument,
        occ: u32,
        id: &str,
    ) -> Result<Vec<(u32, Transform3)>, String> {
        let o = a.find(occ).ok_or("the component no longer exists")?;
        let name = o.name.clone();
        self.with_part(o.component, |p| pattern::feature_instances(p, id))
            .ok_or_else(|| format!("{name}: its part file was not found"))?
            .map_err(|e| format!("{name}: {e}"))
    }

    /// Places the copies of every pattern of `a` from their sources.
    pub(super) fn place_patterns(&self, a: &mut AssemblyDocument) -> Result<(), String> {
        if a.patterns.is_empty() {
            return Ok(());
        }
        let snapshot = a.clone();
        let local = |r: &GeomRef| self.asm_local_in(&snapshot, r);
        let feature = |o: u32, id: &str| self.feature_instances_in(&snapshot, o, id);
        let report = pattern::place_report(
            a,
            &Lookup {
                local: &local,
                feature: &feature,
            },
        );
        // The active assembly's patterns: their health for the browser.
        if self.assembly().is_some_and(|cur| {
            cur.patterns.iter().map(|p| p.id).collect::<Vec<_>>()
                == a.patterns.iter().map(|p| p.id).collect::<Vec<_>>()
        }) {
            *self.asm.pattern_health.borrow_mut() = report.iter().cloned().collect();
        }
        let errors: Vec<String> = report
            .into_iter()
            .map(|(id, e)| {
                let name = a
                    .patterns
                    .iter()
                    .find(|p| p.id == id)
                    .map_or("Component Pattern", |p| p.name.as_str());
                format!("{name}: {e}")
            })
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Why pattern `id` is sick, if it is.
    fn pattern_sick(&self, id: u32) -> Option<String> {
        self.asm.pattern_health.borrow().get(&id).cloned()
    }

    /// On opening an assembly (or coming back to it from a part): pattern
    /// copies follow what changed meanwhile — an Associative pattern's
    /// count, a moved direction edge. Not an Undo step (Inventor updates the
    /// same way).
    pub(super) fn follow_patterns(&mut self) {
        let Some(mut a) = self.assembly().filter(|a| !a.patterns.is_empty()).cloned() else {
            return;
        };
        if let Err(e) = self.place_patterns(&mut a) {
            log::warn!("{e}");
        }
        let changed = self.assembly().is_some_and(|cur| {
            cur.occurrences.len() != a.occurrences.len()
                || cur
                    .occurrences
                    .iter()
                    .zip(&a.occurrences)
                    .any(|(x, y)| x.id != y.id || x.transform != y.transform)
        });
        if changed && let Some(cur) = self.assembly_mut() {
            *cur = a;
            self.scene_dirty = true;
        }
    }

    /// A frame member's pick for the Components selector, when the frame
    /// is a subassembly of the active assembly.
    pub(super) fn pattern_member_label(&self, id: u32) -> Option<String> {
        self.frame_occ()?;
        let o = self.frame_asm()?.find(id)?;
        Some(format!("{} · {MEMBER_TAG}", o.name))
    }

    /// The frame members the Components selector holds (ids of the frame
    /// assembly; empty unless the frame is a subassembly here).
    fn pattern_members(&self, d: &PartDialog) -> Vec<u32> {
        if self.frame_occ().is_none() {
            return Vec::new();
        }
        let Some(a) = self.frame_asm() else {
            return Vec::new();
        };
        d.picks_of("components")
            .iter()
            .filter(|w| w.ends_with(MEMBER_TAG))
            .filter_map(|w| {
                let name = w.split(" · ").next().unwrap_or(w);
                a.occurrences.iter().find(|o| o.name == name).map(|o| o.id)
            })
            .collect()
    }

    /// The frame member's world placement (active assembly coordinates).
    fn member_world(&self, id: u32) -> Option<Transform3> {
        let t = self.frame_asm()?.find(id)?.transform;
        Some(self.frame_place() * t)
    }

    /// The occurrences the panel's Components selector holds.
    fn pattern_sources(&self, d: &PartDialog) -> Vec<u32> {
        let Some(a) = self.assembly() else {
            return Vec::new();
        };
        d.picks_of("components")
            .iter()
            .filter(|w| !w.ends_with(MEMBER_TAG))
            .filter_map(|w| {
                let name = w.split(" · ").next().unwrap_or(w);
                a.occurrences.iter().find(|o| o.name == name).map(|o| o.id)
            })
            .collect()
    }

    /// The open Pattern panel at Placements, when one of its geometry
    /// selectors is active.
    fn placements_dialog(&self) -> Option<&PartDialog> {
        self.part_planned.as_ref().filter(|d| {
            d.kind == Planned::Asm(AsmPlanned::Pattern)
                && d.tab() == PLACEMENTS
                && d.active.is_some_and(|k| k != "components")
        })
    }

    /// Whether the Placements tab is picking geometry.
    pub(super) fn pattern_places_active(&self) -> bool {
        self.placements_dialog().is_some()
    }

    /// The sources drawn see-through and left out of picking while the
    /// Placements tab picks geometry (spec 3.7a §3).
    pub(super) fn pattern_see_through(&self) -> Vec<u32> {
        let Some(d) = self.placements_dialog() else {
            return Vec::new();
        };
        let mut ids = self.pattern_sources(d);
        // Editing: the pattern's own copies stand in the holes too.
        if let Some(p) = d.edit.and_then(|id| {
            self.assembly()
                .and_then(|a| a.patterns.iter().find(|p| p.id == id))
        }) {
            ids.extend(
                p.elements
                    .iter()
                    .flat_map(|e| e.occurrences.iter().copied()),
            );
        }
        ids
    }

    /// A reference's geometry in assembly coordinates.
    fn pattern_world(&self, r: &GeomRef) -> Result<Geom, String> {
        let a = self.assembly().ok_or("no assembly")?;
        let g = self.asm_local_in(a, r)?;
        Ok(match a.find(r.occurrence()) {
            Some(o) => g.moved(&o.transform),
            None => g,
        })
    }

    /// The Placements tab's reference and places.
    fn placements_of(&self, d: &PartDialog) -> Result<Layout, String> {
        let r = |key: &str| d.refs.get(key).and_then(|v| v.first()).cloned();
        let point = r("ref_point").ok_or("select the start point")?;
        let reference = Anchor {
            plane: r("ref_plane"),
            point,
            orient: r("ref_orient"),
        };
        let picks: Vec<(GeomRef, PlacePick)> = d
            .refs
            .get("places")
            .map(|v| {
                v.iter()
                    .filter_map(|g| {
                        let k = PlacePick::of(&self.pattern_world(g).ok()?)?;
                        Some((g.clone(), k))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let places = pattern::places_from_picks(&picks)?;
        if places.is_empty() {
            return Err("pick at least one place".into());
        }
        Ok(Layout::Placements { reference, places })
    }

    /// A click in the view while the Placements tab picks geometry: the
    /// reference and the places (spec 3.7a §2). `true`: the click was used.
    pub(super) fn pattern_place_click(&mut self, pos: egui::Pos2, rect: egui::Rect) -> bool {
        if !self.pattern_places_active() {
            return false;
        }
        if let Some(g) = self.pattern_place_candidate(pos, rect) {
            self.pattern_place_take(g);
        }
        true
    }

    /// What a click at `pos` would give the active Placements selector
    /// (rule 19: only what it takes).
    fn pattern_place_candidate(&self, pos: egui::Pos2, rect: egui::Rect) -> Option<GeomRef> {
        let active = self.placements_dialog()?.active?;
        let straight = |p: &[crate::kernel::Point3]| p.len() == 2;
        let g = match active {
            "ref_orient" => self
                .geom_under_with(pos, rect, &straight, false)
                .or_else(|| self.asm_vertex_under_pub(pos, rect))
                .or_else(|| self.geom_under_pub(pos, rect)),
            "ref_plane" => self.geom_under_pub(pos, rect),
            _ => self
                .asm_vertex_under_pub(pos, rect)
                .or_else(|| self.geom_under_pub(pos, rect)),
        }?;
        let k = PlacePick::of(&self.pattern_world(&g).ok()?)?;
        let ok = match active {
            "ref_plane" => matches!(k, PlacePick::Plane | PlacePick::Rim),
            "ref_point" => matches!(k, PlacePick::Point | PlacePick::Rim),
            _ => true,
        };
        ok.then_some(g)
    }

    /// The Placements tab in the view (rule 23): every pick in its
    /// selector's colour, each place's number at its point, what a click
    /// would take.
    pub(super) fn pattern_place_overlay(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        pointer: Option<egui::Pos2>,
    ) {
        let Some(d) = self
            .part_planned
            .as_ref()
            .filter(|d| d.kind == Planned::Asm(AsmPlanned::Pattern) && d.tab() == PLACEMENTS)
        else {
            return;
        };
        let centre = |g: &GeomRef| match self.pattern_world(g).ok()? {
            Geom::Point { p } | Geom::Frame { p, .. } => Some(p),
            Geom::Circle { c, .. } | Geom::Sphere { c, .. } => Some(c),
            _ => None,
        };
        for key in ["ref_plane", "ref_point", "ref_orient", "places"] {
            let col = d.sel_color(key);
            for g in d.refs.get(key).into_iter().flatten() {
                self.draw_geom(painter, rect, g, col);
                if let Some(p) = centre(g)
                    && let Some(s) = self.camera.project(&p, rect)
                {
                    painter.circle_filled(s, 4.5, col);
                }
            }
        }
        // Each place's number beside its point.
        if let Ok(Layout::Placements { places, .. }) = self.placements_of(d) {
            for (k, a) in places.iter().enumerate() {
                if let Some(p) = centre(&a.point)
                    && let Some(s) = self.camera.project(&p, rect)
                {
                    painter.text(
                        s + egui::vec2(7.0, -7.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("{}", k + 1),
                        egui::FontId::proportional(13.0),
                        d.sel_color("places"),
                    );
                }
            }
        }
        if let (Some(p), Some(key)) = (pointer, d.active)
            && let Some(g) = self.pattern_place_candidate(p, rect)
        {
            self.draw_geom(painter, rect, &g, d.sel_color(key).gamma_multiply(0.75));
        }
    }

    /// Gives `g` to the Placements tab's active selector.
    pub(super) fn pattern_place_take(&mut self, g: GeomRef) {
        let fail = |app: &mut Self, e: &str| {
            app.file_status = Some(Err(format!("Pattern Component: {e}")));
        };
        let Some(active) = self.placements_dialog().and_then(|d| d.active) else {
            return;
        };
        let Some(kind) = self.pattern_world(&g).ok().as_ref().and_then(PlacePick::of) else {
            return fail(self, "that geometry cannot be used here");
        };
        let label = self.geom_label(&g);
        let n_places = self
            .placements_dialog()
            .and_then(|d| self.placements_of(d).ok())
            .map_or(0, |l| l.count() as usize - 1);
        let Some(d) = self.part_planned.as_mut() else {
            return;
        };
        let set = |d: &mut PartDialog, key: &'static str, w: String, g: GeomRef| {
            d.picks.insert(key, vec![w]);
            d.refs.insert(key, vec![g]);
        };
        match (active, kind) {
            ("ref_plane", PlacePick::Plane) => {
                set(d, "ref_plane", label, g);
                d.active = Some("ref_point");
            }
            // A rim: plane and point at once.
            ("ref_plane" | "ref_point", PlacePick::Rim) => {
                d.picks.remove("ref_plane");
                d.refs.remove("ref_plane");
                set(d, "ref_point", label, g);
                d.active = Some("ref_orient");
            }
            ("ref_point", PlacePick::Point) => {
                set(d, "ref_point", label, g);
                d.active = Some("ref_orient");
            }
            ("ref_orient", PlacePick::Edge) => {
                set(d, "ref_orient", label, g);
                d.active = Some("places");
            }
            // Orientation is optional: a place clicked while it waits is
            // the first place (spec 3.7a §2).
            ("places" | "ref_orient", k) => {
                d.active = Some("places");
                if k == PlacePick::Edge && n_places == 0 {
                    return fail(self, "pick a place first; an edge turns the last place");
                }
                let w = match k {
                    PlacePick::Rim | PlacePick::Point => {
                        format!("Place {} · {label}", n_places + 1)
                    }
                    PlacePick::Plane => format!("Plane · {label}"),
                    PlacePick::Edge => format!("Turn place {n_places} · {label}"),
                };
                d.picks.entry("places").or_default().push(w);
                d.refs.entry("places").or_default().push(g);
            }
            (_, _) => {
                let asked = d
                    .flow()
                    .current()
                    .map_or(String::new(), |s| s.label.to_lowercase());
                fail(self, &format!("{label} cannot be the {asked}"));
            }
        }
    }

    /// The sources and layout the panel describes.
    fn pattern_layout(&self, d: &PartDialog) -> Result<(Vec<u32>, Layout), String> {
        let a = self.assembly().ok_or("no assembly")?;
        let sources = self.pattern_sources(d);
        let members = self.pattern_members(d);
        if !members.is_empty() && !sources.is_empty() {
            return Err("frame members are patterned apart from other components".into());
        }
        let in_frame = !members.is_empty();
        let sources = if in_frame { members } else { sources };
        if sources.is_empty() {
            return Err("select the components to pattern".into());
        }
        if in_frame && d.tab() == ASSOCIATIVE {
            return Err(
                "an Associative pattern of frame members is made in the frame assembly".into(),
            );
        }
        let r = |key: &str| d.refs.get(key).and_then(|v| v.first()).cloned();
        let count = |v: Option<f64>| v.unwrap_or(1.0).round().max(1.0) as u32;
        let layout = match d.tab() {
            ASSOCIATIVE => {
                // A face, or an edge (either of its faces), of the pattern.
                let (occurrence, names) = match r("feature") {
                    Some(GeomRef::Face {
                        occurrence, name, ..
                    }) => (occurrence, vec![name]),
                    Some(GeomRef::Edge {
                        sub: _,
                        occurrence,
                        edge,
                    }) => (occurrence, edge.faces.to_vec()),
                    _ => return Err("select a face of a feature pattern of a part".into()),
                };
                let o = a.find(occurrence).ok_or("the component no longer exists")?;
                let feature = self
                    .with_part(o.component, |p| {
                        names.iter().find_map(|n| pattern::pattern_of_face(p, n))
                    })
                    .flatten()
                    .ok_or_else(|| {
                        format!("{}: that face belongs to no feature pattern", o.name)
                    })?;
                if sources.contains(&occurrence) {
                    return Err("the part with the feature pattern cannot follow it".into());
                }
                Layout::Feature {
                    occurrence,
                    feature,
                }
            }
            RECTANGULAR => Layout::Rectangular {
                column: r("column").ok_or("select the column direction")?,
                column_flip: d.check(RECTANGULAR, "Flip column"),
                column_count: count(d.num(RECTANGULAR, "Column Count")),
                column_spacing: d.num(RECTANGULAR, "Column Spacing").unwrap_or(0.0),
                row: r("row"),
                row_flip: d.check(RECTANGULAR, "Flip row"),
                row_count: count(d.num(RECTANGULAR, "Row Count")),
                row_spacing: d.num(RECTANGULAR, "Row Spacing").unwrap_or(0.0),
            },
            CIRCULAR => Layout::Circular {
                axis: r("axis").ok_or("select the rotation axis")?,
                flip: d.check(CIRCULAR, "Flip"),
                count: count(d.num(CIRCULAR, "Count")),
                angle: d.num(CIRCULAR, "Angle").unwrap_or(0.0).to_radians(),
            },
            PLACEMENTS => self.placements_of(d)?,
            _ => return Err("no such tab".into()),
        };
        if !matches!(layout, Layout::Feature { .. }) && layout.count() < 2 {
            return Err("the count makes no copies".into());
        }
        if layout.count() > 1000 {
            return Err(format!("{} elements are more than 1000", layout.count()));
        }
        Ok((sources, layout))
    }

    /// OK of Pattern Component: adds (or changes) the pattern; false with
    /// the reason in the status bar when the panel is not complete.
    pub(super) fn pattern_ok(&mut self, d: &PartDialog) -> bool {
        let fail = |app: &mut Self, e: String| {
            app.file_status = Some(Err(format!("Pattern Component: {e}")));
            false
        };
        let (sources, layout) = match self.pattern_layout(d) {
            Ok(v) => v,
            Err(e) => return fail(self, e),
        };
        if !self.pattern_members(d).is_empty() {
            if d.edit.is_some() {
                return fail(
                    self,
                    "a pattern of frame members is edited in the frame assembly".into(),
                );
            }
            return match self.pattern_members_ok(sources, &layout) {
                Ok(msg) => {
                    self.asm.pattern_preview = None;
                    self.scene_dirty = true;
                    self.file_status = Some(Ok(msg));
                    true
                }
                Err(e) => fail(self, e),
            };
        }
        let Some(mut a) = self.assembly().cloned() else {
            return false;
        };
        let snapshot = a.clone();
        let local = |r: &GeomRef| self.asm_local_in(&snapshot, r);
        let feature = |o: u32, id: &str| self.feature_instances_in(&snapshot, o, id);
        let local = Lookup {
            local: &local,
            feature: &feature,
        };
        let done = match d.edit {
            Some(id) => match a.patterns.iter_mut().find(|p| p.id == id) {
                Some(p) => {
                    p.layout = layout;
                    p.sources = sources;
                    pattern::place(&mut a, &local).map(|_| id)
                }
                None => Err("the pattern no longer exists".into()),
            },
            None => a.add_pattern(sources, layout, &local),
        };
        let id = match done {
            Ok(id) => id,
            Err(e) => return fail(self, e),
        };
        let label = if d.edit.is_some() {
            "Edit Component Pattern"
        } else {
            "Pattern Component"
        };
        self.checkpoint(label);
        let (name, n) = a
            .patterns
            .iter()
            .find(|p| p.id == id)
            .map_or((String::new(), 0), |p| {
                (p.name.clone(), p.elements.len() + 1)
            });
        if let Some(cur) = self.assembly_mut() {
            *cur = a;
        }
        self.asm.pattern_preview = None;
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!("{name}: {n} elements")));
        true
    }

    /// OK of a pattern of frame members made from the parent (spec 6.1
    /// §3.14): the layout in the frame assembly's terms, the pattern added
    /// there, one Undo step; nothing changes on an error.
    fn pattern_members_ok(&mut self, sources: Vec<u32>, layout: &Layout) -> Result<String, String> {
        let linked = self.frame_linked();
        let mut trial = self.project.clone();
        let down = self.frame_layout_down(&mut trial, &linked, layout)?;
        let doc = self.frame_doc();
        let mut a = match trial.document(doc).map(|x| &x.data) {
            Some(crate::document::DocumentData::Assembly(a)) => a.clone(),
            _ => return Err("the frame assembly is missing".into()),
        };
        let snapshot = a.clone();
        let local = |r: &GeomRef| self.asm_local_in(&snapshot, r);
        let feature = |o: u32, id: &str| self.feature_instances_in(&snapshot, o, id);
        let look = Lookup {
            local: &local,
            feature: &feature,
        };
        let id = a.add_pattern(sources, down, &look)?;
        let (name, n) = a
            .patterns
            .iter()
            .find(|p| p.id == id)
            .map_or((String::new(), 0), |p| {
                (p.name.clone(), p.elements.len() + 1)
            });
        if let Some(x) = trial.document_mut(doc) {
            x.data = crate::document::DocumentData::Assembly(a);
        }
        self.frame_commit("Pattern Component", trial);
        Ok(format!("{name}: {n} elements of frame members"))
    }

    /// The copies the open Pattern panel would make (source, motion), for
    /// the see-through preview; none while the panel is incomplete.
    pub(super) fn pattern_preview_items(&self) -> Vec<(u32, Transform3)> {
        let Some(d) = self
            .part_planned
            .as_ref()
            .filter(|d| d.kind == Planned::Asm(AsmPlanned::Pattern))
        else {
            return Vec::new();
        };
        let Some(a) = self.assembly() else {
            return Vec::new();
        };
        let Ok((sources, layout)) = self.pattern_layout(d) else {
            return Vec::new();
        };
        let in_frame = !self.pattern_members(d).is_empty();
        let source_at = |id: u32| {
            if in_frame {
                self.member_world(id)
            } else {
                a.find(id).map(|o| o.transform)
            }
        };
        let at = |id: u32| a.find(id).map(|o| o.transform);
        let local = |r: &GeomRef| self.asm_local_in(a, r);
        let feature = |o: u32, id: &str| self.feature_instances_in(a, o, id);
        let look = Lookup {
            local: &local,
            feature: &feature,
        };
        let found = match &layout {
            Layout::Placements { reference, places } => {
                pattern::placement_motions(reference, places, &look, &at).map(|m| m.0)
            }
            l => pattern::motions(l, &look, &at),
        };
        let Ok(moves) = found else {
            return Vec::new();
        };
        // Editing: the pattern's own copies are already drawn (at
        // Placements they are see-through and may move: all are previewed).
        let placements = matches!(layout, Layout::Placements { .. });
        let skip = d.edit.filter(|_| !placements).map_or(0, |id| {
            a.patterns
                .iter()
                .find(|p| p.id == id)
                .map_or(0, |p| p.elements.len())
        });
        moves
            .iter()
            .skip(skip)
            .flat_map(|&(_, m)| {
                sources
                    .iter()
                    .filter_map(|&s| source_at(s).map(|t| (s, m * t)))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Called every frame in an assembly: rebuilds the scene when the
    /// Pattern panel's copies change.
    pub(super) fn pattern_preview_poll(&mut self) {
        // Place numbers follow a pick dropped with x.
        if let Some(d) = self.part_planned.as_mut()
            && d.kind == Planned::Asm(AsmPlanned::Pattern)
            && let Some(list) = d.picks.get_mut("places")
        {
            relabel_places(list);
        }
        let items = self.pattern_preview_items();
        let through = self.pattern_see_through();
        let key =
            (!items.is_empty() || !through.is_empty()).then(|| format!("{items:?}{through:?}"));
        if key != self.asm.pattern_preview {
            self.asm.pattern_preview = key;
            self.scene_dirty = true;
        }
    }

    /// The preview copies, see-through (scene rebuild).
    pub(super) fn pattern_preview_scene(&self, g: &mut super::renderer::SceneGeometry) {
        if self.asm.pattern_preview.is_none() {
            return;
        }
        let Some(a) = self.assembly() else {
            return;
        };
        // Frame members: their parts are in the frame assembly.
        let in_frame = self
            .part_planned
            .as_ref()
            .is_some_and(|d| !self.pattern_members(d).is_empty());
        for (s, t) in self.pattern_preview_items() {
            let o = if in_frame {
                self.frame_asm().and_then(|f| f.find(s))
            } else {
                a.find(s)
            };
            let Some(o) = o else {
                continue;
            };
            self.with_part(o.component, |p| {
                super::material::add_part_meshes_seen(
                    g,
                    p,
                    Some(&t),
                    Some([0.3, 0.6, 1.0]),
                    Some(0.35),
                );
            });
        }
    }

    /// Opens the Pattern panel on pattern `id` (browser Edit).
    pub(super) fn edit_pattern(&mut self, id: u32) {
        let Some(p) = self
            .assembly()
            .and_then(|a| a.patterns.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        let mut d = PartDialog::new(AsmPlanned::Pattern);
        d.edit = Some(id);
        let names: Vec<String> = p
            .sources
            .iter()
            .filter_map(|&s| self.occurrence_label_pub(s))
            .collect();
        d.picks.insert("components", names);
        let take = |d: &mut PartDialog, key: &'static str, g: &GeomRef| {
            d.picks.insert(key, vec![self.geom_label(g)]);
            d.refs.insert(key, vec![g.clone()]);
        };
        match &p.layout {
            Layout::Rectangular {
                column,
                column_flip,
                column_count,
                column_spacing,
                row,
                row_flip,
                row_count,
                row_spacing,
            } => {
                d.set_tab(RECTANGULAR);
                take(&mut d, "column", column);
                if let Some(r) = row {
                    take(&mut d, "row", r);
                }
                d.set_check(RECTANGULAR, "Flip column", *column_flip);
                d.set_num(RECTANGULAR, "Column Count", f64::from(*column_count));
                d.set_num(RECTANGULAR, "Column Spacing", *column_spacing);
                d.set_check(RECTANGULAR, "Flip row", *row_flip);
                d.set_num(RECTANGULAR, "Row Count", f64::from(*row_count));
                d.set_num(RECTANGULAR, "Row Spacing", *row_spacing);
            }
            Layout::Feature {
                occurrence,
                feature,
            } => {
                d.set_tab(ASSOCIATIVE);
                // A face of one of the pattern's copies stands for it.
                let prefix = format!("{feature}#");
                let face = self
                    .assembly()
                    .and_then(|a| a.find(*occurrence))
                    .and_then(|o| {
                        self.with_part(o.component, |p| {
                            p.body
                                .faces()
                                .filter_map(|(_, f)| f.name.clone())
                                .find(|n| n.starts_with(&prefix))
                        })
                        .flatten()
                    });
                if let Some(name) = face {
                    let g = GeomRef::Face {
                        sub: Vec::new(),
                        occurrence: *occurrence,
                        name,
                        hint: [0.0; 3],
                    };
                    d.picks
                        .insert("feature", vec![format!("{feature} · feature pattern")]);
                    d.refs.insert("feature", vec![g]);
                }
            }
            Layout::Placements { reference, places } => {
                d.set_tab(PLACEMENTS);
                if let Some(g) = &reference.plane {
                    take(&mut d, "ref_plane", g);
                }
                take(&mut d, "ref_point", &reference.point);
                if let Some(g) = &reference.orient {
                    take(&mut d, "ref_orient", g);
                }
                let kind = |g: &GeomRef| {
                    self.pattern_world(g)
                        .ok()
                        .as_ref()
                        .and_then(PlacePick::of)
                        .unwrap_or(PlacePick::Point)
                };
                let mut n = 0;
                for (g, k) in pattern::picks_of_places(places, &kind) {
                    let label = self.geom_label(&g);
                    let w = match k {
                        PlacePick::Rim | PlacePick::Point => {
                            n += 1;
                            format!("Place {n} · {label}")
                        }
                        PlacePick::Plane => format!("Plane · {label}"),
                        PlacePick::Edge => format!("Turn place {n} · {label}"),
                    };
                    d.picks.entry("places").or_default().push(w);
                    d.refs.entry("places").or_default().push(g);
                }
            }
            Layout::Circular {
                axis,
                flip,
                count,
                angle,
            } => {
                d.set_tab(CIRCULAR);
                take(&mut d, "axis", axis);
                d.set_check(CIRCULAR, "Flip", *flip);
                d.set_num(CIRCULAR, "Count", f64::from(*count));
                d.set_num(CIRCULAR, "Angle", angle.to_degrees());
            }
        }
        d.active = None;
        self.asm.tool = None;
        self.part_planned = Some(d);
    }

    /// Deletes pattern `id` with its copies (one Undo step).
    pub(super) fn delete_pattern(&mut self, id: u32) {
        let Some(mut a) = self.assembly().cloned() else {
            return;
        };
        let Some(p) = a.remove_pattern(id) else {
            return;
        };
        self.checkpoint("Delete Component Pattern");
        if let Some(cur) = self.assembly_mut() {
            *cur = a;
        }
        self.asm.sel.clear();
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!("{}: deleted with its copies", p.name)));
    }

    /// A pattern's node in the browser: its elements (Element:1 = the
    /// sources) with their components.
    pub(super) fn pattern_rows(
        &mut self,
        ui: &mut egui::Ui,
        p: &ComponentPattern,
        cmds: &mut Vec<PatternCmd>,
    ) {
        let names = |ids: &[u32]| -> Vec<(u32, String)> {
            ids.iter()
                .filter_map(|&i| {
                    self.assembly()
                        .and_then(|a| a.find(i))
                        .map(|o| (i, o.name.clone()))
                })
                .collect()
        };
        let mut elements = vec![(1, names(&p.sources))];
        elements.extend(p.elements.iter().map(|e| (e.index, names(&e.occurrences))));
        let all: Vec<u32> = elements
            .iter()
            .flat_map(|e| e.1.iter().map(|x| x.0))
            .collect();
        let on = !all.is_empty() && all.iter().all(|i| self.asm.sel.contains(i));
        super::panels::tree_fold(ui, ui.make_persistent_id(("asm_pattern", p.id)), false)
            .show_header(ui, |ui| {
                super::panels::tree_icon(ui, Icon::AsmPattern);
                let sick = self.pattern_sick(p.id);
                let mut text = RichText::new(&p.name);
                if sick.is_some() {
                    text = text.color(super::theme::tokens().error);
                }
                let r = ui.selectable_label(on, text);
                let r = match &sick {
                    Some(e) => r.on_hover_text(format!("Sick: {e}")),
                    None => r,
                };
                if r.clicked() {
                    cmds.push(PatternCmd::Select(p.id));
                }
                if r.double_clicked() {
                    cmds.push(PatternCmd::Edit(p.id));
                }
                r.context_menu(|ui| {
                    if ui.button("Edit").clicked() {
                        cmds.push(PatternCmd::Edit(p.id));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        cmds.push(PatternCmd::Delete(p.id));
                        ui.close();
                    }
                });
            })
            .body(|ui| {
                for (index, occs) in elements {
                    let ids: Vec<u32> = occs.iter().map(|x| x.0).collect();
                    let on = !ids.is_empty() && ids.iter().all(|i| self.asm.sel.contains(i));
                    super::panels::tree_fold(
                        ui,
                        ui.make_persistent_id(("asm_pattern_el", p.id, index)),
                        false,
                    )
                    .show_header(ui, |ui| {
                        super::panels::tree_icon(ui, Icon::AsmPattern);
                        let r = ui.selectable_label(on, format!("Element:{index}"));
                        if r.clicked() {
                            cmds.push(PatternCmd::SelectElement(p.id, index));
                        }
                    })
                    .body(|ui| {
                        for (id, name) in occs {
                            ui.horizontal(|ui| {
                                super::panels::tree_icon(ui, Icon::Part);
                                let r = ui.selectable_label(self.asm.sel.contains(&id), name);
                                if r.clicked() {
                                    cmds.push(PatternCmd::SelectOccurrence(id));
                                }
                                if r.hovered() {
                                    self.asm.hover = Some(id);
                                }
                                r.context_menu(|ui| {
                                    self.asm.menu_at = None;
                                    if !self.asm.sel.contains(&id) {
                                        self.asm.sel = vec![id];
                                    }
                                    let sel = self.asm.sel.clone();
                                    self.occurrence_menu(ui, &sel);
                                });
                            });
                        }
                    });
                }
            });
    }

    /// Runs a browser command of a pattern.
    pub(super) fn pattern_cmd(&mut self, c: PatternCmd) {
        let Some(a) = self.assembly() else {
            return;
        };
        match c {
            PatternCmd::Select(id) => {
                if let Some(p) = a.patterns.iter().find(|p| p.id == id) {
                    self.asm.sel = p
                        .sources
                        .iter()
                        .chain(p.elements.iter().flat_map(|e| e.occurrences.iter()))
                        .copied()
                        .filter(|&i| i != 0)
                        .collect();
                }
            }
            PatternCmd::SelectElement(id, index) => {
                if let Some(p) = a.patterns.iter().find(|p| p.id == id) {
                    self.asm.sel = if index == 1 {
                        p.sources.clone()
                    } else {
                        p.elements
                            .iter()
                            .filter(|e| e.index == index)
                            .flat_map(|e| e.occurrences.iter().copied())
                            .filter(|&i| i != 0)
                            .collect()
                    };
                }
            }
            PatternCmd::SelectOccurrence(id) => self.asm.sel = vec![id],
            PatternCmd::Edit(id) => self.edit_pattern(id),
            PatternCmd::Delete(id) => self.delete_pattern(id),
        }
        self.scene_dirty = true;
    }

    /// The pattern copy's note in the component's Properties.
    pub(super) fn pattern_note(&self, ui: &mut egui::Ui, id: u32) {
        let Some(a) = self.assembly() else {
            return;
        };
        if let Some(p) = a.pattern_of(id) {
            let what = if a.is_element(id) {
                "a copy placed by"
            } else {
                "a source of"
            };
            ui.label(RichText::new(format!("{what} {}", p.name)).weak());
        }
    }
}

/// The Places list's labels numbered again (`Place n · …`, `Turn place n ·
/// …`) after a pick was dropped.
fn relabel_places(list: &mut [String]) {
    let mut n = 0;
    for w in list.iter_mut() {
        let Some((head, rest)) = w.split_once(" · ") else {
            continue;
        };
        let new = if head.starts_with("Place ") {
            n += 1;
            format!("Place {n} · {rest}")
        } else if head.starts_with("Turn place ") {
            format!("Turn place {n} · {rest}")
        } else {
            continue;
        };
        if *w != new {
            *w = new;
        }
    }
}

#[cfg(test)]
mod tests {
    /// Dropping place 1 renumbers the places after it and their turns.
    #[test]
    fn place_labels_follow_a_dropped_pick() {
        let mut list: Vec<String> = [
            "Place 2 · Plate:1 edge",
            "Plane · Plate:1 face",
            "Place 3 · Plate:1 vertex",
            "Turn place 3 · Plate:1 edge",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        super::relabel_places(&mut list);
        assert_eq!(
            list,
            [
                "Place 1 · Plate:1 edge",
                "Plane · Plate:1 face",
                "Place 2 · Plate:1 vertex",
                "Turn place 2 · Plate:1 edge",
            ]
        );
    }
}
