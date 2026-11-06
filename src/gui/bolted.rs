//! Bolted Connection's panel (spec `docs/spec/6.2-bolted-connection.md`):
//! the pieces the shared planned panel (`gui/part_planned.rs`) cannot draw
//! from its rows — the selectors each Placement uses, the thread and library
//! lists, the fastener stack (head side, the holes, nut side) and the
//! Calculation tabs' rows, shown disabled until their logic exists.

use std::sync::Arc;

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::panels::plabel;
use super::part_planned::{PartDialog, Planned};
use crate::assembly::bolted::{self, BoltKind};
use crate::components::catalog::Catalog;
use crate::components::fit::Fit;
use crate::part::hole::FastenerFit;
use crate::part::standards::Standards;

/// The tooltip of the Calculation and Fatigue Calculation rows.
const CALC_TIP: &str = "Strength check: comes after the geometry (spec 6.2 §3.7)";
/// The tooltip of what the stack rows will do once the logic exists.
const LOGIC_TIP: &str = "Comes with the command's logic (spec 6.2 §3.4)";

/// What a fastener is in the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    Bolt,
    Washer,
    Nut,
}

impl Role {
    fn of(category: &str) -> Option<Role> {
        let c = category.strip_prefix("Fasteners/")?;
        if c.starts_with("Bolts") {
            Some(Role::Bolt)
        } else if c.starts_with("Washers") {
            Some(Role::Washer)
        } else if c.starts_with("Nuts") {
            Some(Role::Nut)
        } else {
            None
        }
    }
}

/// Which side of the plates a fastener sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    /// Above the start plane: the bolt and the washers under its head.
    Head,
    /// Below the termination: washers and the nut.
    Nut,
}

/// One fastener of the stack: a library family (its member follows from the
/// diameter and, for the bolt, the grip).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct StackItem {
    pub family: usize,
    pub role: Role,
}

/// The panel state of Bolted Connection (`PartDialog::bolt`).
#[derive(Clone, Debug, Default)]
pub(super) struct BoltPanel {
    /// The Component Library, read when the panel opened.
    pub catalog: Option<Arc<Catalog>>,
    /// Head side, from the head down to the first plate (the bolt first).
    pub head: Vec<StackItem>,
    /// Nut side, from the last plate down (washers, then the nut).
    pub nut: Vec<StackItem>,
    /// The side whose library list is open.
    pub adding: Option<Side>,
    /// The axis from the picks (assembly coordinates), or why there is none;
    /// refreshed every frame by [`CadCraftApp::bolt_follow`].
    pub axis: Option<Result<bolted::Axis, String>>,
}

/// Whether `d` is the Bolted Connection panel.
pub(super) fn is_bolted(d: &PartDialog) -> bool {
    d.kind == Planned::Asm(AsmPlanned::BoltedConnection)
}

/// The kind chosen on the Design tab.
pub(super) fn kind(d: &PartDialog) -> BoltKind {
    if d.index(0, "Type") == 1 {
        BoltKind::Blind
    } else {
        BoltKind::ThroughAll
    }
}

/// The selectors Placement and Type have no use for (spec 6.2 §3.2).
pub(super) fn sel_hidden(d: &PartDialog, key: &str) -> bool {
    if !is_bolted(d) {
        return false;
    }
    let placement = d.index(0, "Placement");
    let blind = kind(d) == BoltKind::Blind;
    match key {
        "edge1" | "edge2" => placement != 0,
        "circle" => placement != 1,
        "point" => placement != 2,
        "hole" => placement != 3,
        "end" => blind,
        "blind" => !blind,
        _ => false,
    }
}

/// Whether the placement is complete: start, its reference(s), the end.
pub(super) fn placement_done(d: &PartDialog) -> bool {
    let has = |k: &str| !d.picks_of(k).is_empty();
    let refs = match d.index(0, "Placement") {
        0 => has("edge1") && has("edge2"),
        1 => has("circle"),
        2 => has("point"),
        _ => has("hole"),
    };
    let end = if kind(d) == BoltKind::Blind {
        has("blind")
    } else {
        has("end")
    };
    has("start") && refs && end
}

/// The lists of Bolted Connection's drop-downs that come from the tables.
pub(super) fn combo(d: &PartDialog, ti: usize, label: &str) -> Option<Vec<String>> {
    if !is_bolted(d) || ti != 0 {
        return None;
    }
    let st = Standards::global();
    let own = |v: Vec<&str>| v.into_iter().map(str::to_owned).collect::<Vec<_>>();
    match label {
        "Thread" => Some(own(bolted::thread_types(st))),
        "Diameter" => Some(bolted::sizes(st, &choice(d, "Thread")?)),
        "Standard" => {
            let mut v = vec!["All".to_owned()];
            if let Some(c) = &d.bolt.catalog {
                v.extend(c.standards());
            }
            Some(v)
        }
        "Category" => {
            let mut v = vec!["All".to_owned()];
            if let Some(c) = &d.bolt.catalog {
                for f in &c.families {
                    if Role::of(&f.category).is_some() {
                        let name = category_name(&f.category);
                        if !v.contains(&name) {
                            v.push(name);
                        }
                    }
                }
            }
            Some(v)
        }
        _ => None,
    }
}

/// `Fasteners/Bolts/Hex Head` as `Bolts › Hex Head`.
fn category_name(category: &str) -> String {
    category
        .strip_prefix("Fasteners/")
        .unwrap_or(category)
        .replace('/', " › ")
}

/// The text chosen in drop-down `label` of the Design tab.
pub(super) fn choice(d: &PartDialog, label: &str) -> Option<String> {
    let list = combo(d, 0, label)?;
    list.get(d.index(0, label)).cloned()
}

/// Why row `label` of tab `ti` is disabled (`None`: on).
pub(super) fn row_off(d: &PartDialog, ti: usize, label: &str) -> Option<&'static str> {
    if !is_bolted(d) {
        return None;
    }
    if ti > 0 {
        return Some(CALC_TIP);
    }
    match label {
        "Distance 1" | "Distance 2" if d.index(0, "Placement") != 0 => {
            Some("Linear placement only (spec 6.2 §3.2)")
        }
        "Follow Pattern" if d.index(0, "Placement") != 3 => {
            Some("By Hole on a patterned hole only (spec 6.2 §3.2)")
        }
        _ => None,
    }
}

/// Opens the panel's own state: the library, M8 as the first diameter.
pub(super) fn start(d: &mut PartDialog) {
    if !is_bolted(d) {
        return;
    }
    // Tests never read the user's library folder.
    #[cfg(not(test))]
    {
        d.bolt.catalog = crate::components::root_dir().map(|r| Arc::new(Catalog::load(&r)));
    }
    if let Some(i) = combo(d, 0, "Diameter").and_then(|v| v.iter().position(|s| s == "M8")) {
        d.set_index(0, "Diameter", i);
    }
}

/// The nominal diameter chosen (mm), with its holes.
fn holes(d: &PartDialog) -> Option<(String, bolted::HoleSizes)> {
    let thread = choice(d, "Thread")?;
    let size = choice(d, "Diameter")?;
    let h = bolted::hole_sizes(Standards::global(), &thread, &size, FastenerFit::Normal)?;
    Some((size, h))
}

/// The families that fit the chosen diameter, the Standard and Category
/// filters and `side` (head side: bolts and washers; nut side: washers and
/// nuts).
fn offered(d: &PartDialog, side: Side) -> Vec<(usize, Role)> {
    let (Some(c), Some((_, h))) = (&d.bolt.catalog, holes(d)) else {
        return Vec::new();
    };
    let standard = choice(d, "Standard").filter(|s| s != "All");
    let category = choice(d, "Category").filter(|s| s != "All");
    let dn = h.major;
    let mut v = Vec::new();
    for (i, f) in c.families.iter().enumerate() {
        let Some(role) = Role::of(&f.category) else {
            continue;
        };
        let side_ok = match side {
            Side::Head => role != Role::Nut,
            Side::Nut => role != Role::Bolt,
        };
        if !side_ok
            || standard.as_ref().is_some_and(|s| *s != f.standard)
            || category
                .as_ref()
                .is_some_and(|s| *s != category_name(&f.category))
        {
            continue;
        }
        let fits = c.fits(i).iter().any(|fit| match (role, fit) {
            (
                Role::Bolt,
                Fit::Into {
                    d, length: Some(_), ..
                },
            ) => (d - dn).abs() < 0.01,
            (Role::Nut, Fit::Over { bore }) => (bore - dn).abs() < 0.01,
            (Role::Washer, Fit::Over { bore }) => *bore >= dn - 0.01 && *bore <= dn * 1.2 + 0.2,
            _ => false,
        });
        if fits {
            v.push((i, role));
        }
    }
    v
}

/// The fastener stack (replaces the Click to add a fastener button): the
/// head side, the holes, the nut side.
pub(super) fn stack_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    let done = placement_done(d);
    let blind = kind(d) == BoltKind::Blind;
    side_ui(d, ui, Side::Head, done);
    // The axis found from the picks, or why not.
    match &d.bolt.axis {
        Some(Ok(a)) => {
            plabel(ui, "Grip");
            ui.label(format!("{:.2} mm", a.length));
            ui.end_row();
        }
        Some(Err(e)) => {
            plabel(ui, "");
            ui.add(
                egui::Label::new(
                    RichText::new(format!("Bolted Connection: {e}"))
                        .color(super::theme::tokens().error),
                )
                .wrap(),
            );
            ui.end_row();
        }
        None => {}
    }
    // The holes each plate gets.
    plabel(ui, "Holes");
    match holes(d) {
        Some((size, h)) => {
            let text = if blind {
                format!(
                    "Ø{} clearance (ISO 273 Normal), last component {size} tapped (drill Ø{})",
                    h.clearance, h.tap_drill
                )
            } else {
                format!(
                    "Ø{} clearance through every component (Normal fit)",
                    h.clearance
                )
            };
            ui.vertical(|ui| {
                fit_cell(ui);
                ui.add(egui::Label::new(RichText::new(text).small()).wrap());
                ui.add_enabled(false, egui::Button::new("Modify").small())
                    .on_disabled_hover_text(
                        "Hole type and fit: comes with the logic (spec 6.2 §3.4)",
                    );
            });
        }
        None => {
            ui.label(RichText::new("no clearance hole for this size").weak());
        }
    }
    ui.end_row();
    if !blind {
        side_ui(d, ui, Side::Nut, done);
    }
    if !done {
        plabel(ui, "");
        ui.add(
            egui::Label::new(
                RichText::new("Select the placement first: start plane, reference and termination")
                    .weak()
                    .italics(),
            )
            .wrap(),
        );
        ui.end_row();
    }
}

fn side_ui(d: &mut PartDialog, ui: &mut egui::Ui, side: Side, done: bool) {
    plabel(
        ui,
        if side == Side::Head {
            "Head side"
        } else {
            "Nut side"
        },
    );
    let mut remove = None;
    let mut add = None;
    ui.vertical(|ui| {
        fit_cell(ui);
        let list = match side {
            Side::Head => d.bolt.head.clone(),
            Side::Nut => d.bolt.nut.clone(),
        };
        for (k, item) in list.iter().enumerate() {
            let name = d
                .bolt
                .catalog
                .as_ref()
                .and_then(|c| c.families.get(item.family))
                .map_or_else(String::new, |f| f.name.clone());
            ui.horizontal_wrapped(|ui| {
                ui.label(name);
                ui.add_enabled(false, egui::Button::new("Edit").small())
                    .on_disabled_hover_text(LOGIC_TIP);
                ui.add_enabled(false, egui::Button::new("Change").small())
                    .on_disabled_hover_text(LOGIC_TIP);
                if ui
                    .small_button("x")
                    .on_hover_text("Delete this fastener")
                    .clicked()
                {
                    remove = Some(k);
                }
            });
        }
        let open = d.bolt.adding == Some(side);
        let b = ui.add_enabled(
            done,
            egui::Button::new("Click to add a fastener").selected(open),
        );
        let b = b.on_disabled_hover_text("Select the placement first (spec 6.2 §3.2)");
        if b.clicked() {
            d.bolt.adding = if open { None } else { Some(side) };
        }
        if open && done {
            let found = offered(d, side);
            if found.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new("nothing in the library fits this diameter and filter")
                            .weak(),
                    )
                    .wrap(),
                );
            }
            egui::ScrollArea::vertical()
                .id_salt(("bolt_add", side == Side::Head))
                .max_height(180.0)
                .min_scrolled_height(150.0)
                .show(ui, |ui| {
                    for (f, role) in found {
                        let Some(fam) = d.bolt.catalog.as_ref().and_then(|c| c.families.get(f))
                        else {
                            continue;
                        };
                        let text = format!("{} · {}", fam.name, category_name(&fam.category));
                        let tip = format!("{} ({})", fam.name, category_name(&fam.category));
                        let r = ui.add(egui::Button::selectable(false, text).truncate());
                        if r.on_hover_text(tip).clicked() {
                            add = Some(StackItem { family: f, role });
                        }
                    }
                });
        }
    });
    ui.end_row();
    let list = match side {
        Side::Head => &mut d.bolt.head,
        Side::Nut => &mut d.bolt.nut,
    };
    if let Some(k) = remove {
        list.remove(k);
    }
    if let Some(item) = add {
        insert(list, item);
        d.bolt.adding = None;
    }
}

/// Keeps a grid cell inside the Properties panel (it cannot widen: CLAUDE.md
/// "Properties cannot widen"), so its rows wrap.
fn fit_cell(ui: &mut egui::Ui) {
    let w = (ui.clip_rect().right() - ui.cursor().left() - 6.0).max(80.0);
    ui.set_max_width(w);
}

/// Adds `item` to one side: one bolt (a new one replaces it, first on the
/// head side), washers next to the plates, the nut last.
fn insert(list: &mut Vec<StackItem>, item: StackItem) {
    match item.role {
        Role::Bolt => {
            list.retain(|x| x.role != Role::Bolt);
            list.insert(0, item);
        }
        Role::Nut => list.push(item),
        Role::Washer => {
            // Head side: after the bolt; nut side: before the nut(s).
            let at = list
                .iter()
                .position(|x| x.role == Role::Nut)
                .unwrap_or(list.len());
            list.insert(at, item);
        }
    }
}

impl CadCraftApp {
    /// The first pick of selector `key` in assembly coordinates.
    fn bolt_geom(&self, d: &PartDialog, key: &str) -> Option<crate::assembly::Geom> {
        let r = d.refs.get(key)?.first()?;
        let g = self.asm_local_geom(r).ok()?;
        let a = self.assembly()?;
        Some(match a.find(r.occurrence()) {
            Some(o) => g.moved(&o.transform),
            None => g,
        })
    }

    /// Finds the axis of the open Bolted Connection panel from its picks (once a frame).
    pub(super) fn bolt_follow(&mut self) {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return;
        };
        let axis = (|| {
            let start = self.bolt_geom(d, "start")?;
            let end_key = if kind(d) == BoltKind::Blind {
                "blind"
            } else {
                "end"
            };
            let end = self.bolt_geom(d, end_key)?;
            let reference = match d.index(0, "Placement") {
                0 => {
                    let crate::assembly::Geom::Plane { p, .. } = start else {
                        return Some(Err("the start plane must be planar".to_owned()));
                    };
                    bolted::Reference::Linear {
                        e1: self.bolt_geom(d, "edge1")?,
                        e2: self.bolt_geom(d, "edge2")?,
                        d1: d.num(0, "Distance 1").unwrap_or(10.0),
                        d2: d.num(0, "Distance 2").unwrap_or(10.0),
                        toward: p,
                    }
                }
                1 => bolted::Reference::Through(self.bolt_geom(d, "circle")?),
                2 => bolted::Reference::Through(self.bolt_geom(d, "point")?),
                _ => bolted::Reference::Through(self.bolt_geom(d, "hole")?),
            };
            Some(bolted::axis(&start, &reference, &end))
        })();
        if let Some(d) = self.part_planned.as_mut() {
            d.bolt.axis = axis;
        }
    }

    /// The schematic preview of the open Bolted Connection (spec 6.2 §3.5): the
    /// axis, the hole at both planes, the head above the start plane and, for a
    /// through connection, the nut below the termination.
    pub(super) fn bolt_preview(&self, painter: &egui::Painter, rect: egui::Rect) {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return;
        };
        let (Some(Ok(a)), Some((_, h))) = (&d.bolt.axis, holes(d)) else {
            return;
        };
        let color = super::theme::canvas().hover_edge;
        let thin = egui::Stroke::new(1.0, color);
        let thick = egui::Stroke::new(2.0, color);
        let u = if a.dir.x.abs() < 0.9 {
            a.dir.cross(&crate::kernel::Vector3::x())
        } else {
            a.dir.cross(&crate::kernel::Vector3::y())
        }
        .normalize();
        let v = a.dir.cross(&u);
        let point =
            |t: f64, r: f64, ang: f64| a.at + a.dir * t + (u * ang.cos() + v * ang.sin()) * r;
        let circle = |t: f64, r: f64, s: egui::Stroke| {
            let pts: Vec<egui::Pos2> = (0..=48)
                .filter_map(|k| {
                    let ang = std::f64::consts::TAU * f64::from(k) / 48.0;
                    self.camera.project(&point(t, r, ang), rect)
                })
                .collect();
            if pts.len() > 2 {
                painter.add(egui::Shape::line(pts, s));
            }
        };
        let line = |t0: f64, r0: f64, t1: f64, r1: f64, ang: f64, s: egui::Stroke| {
            if let (Some(p), Some(q)) = (
                self.camera.project(&point(t0, r0, ang), rect),
                self.camera.project(&point(t1, r1, ang), rect),
            ) {
                painter.line_segment([p, q], s);
            }
        };
        let dn = h.major;
        let blind = kind(d) == BoltKind::Blind;
        // The hole through the plates.
        circle(0.0, h.clearance / 2.0, thin);
        circle(a.length, h.clearance / 2.0, thin);
        // The head: 1.6 d across, 0.65 d high, above the start plane.
        let head = -0.65 * dn;
        circle(0.0, 0.8 * dn, thick);
        circle(head, 0.8 * dn, thick);
        // The shank, past the far side by the nut and a little thread (through)
        // or into the tapped hole (blind: 1.5 d).
        let tip = if blind {
            a.length + 1.5 * dn
        } else {
            a.length + 1.1 * dn
        };
        for k in 0..4 {
            let ang = std::f64::consts::FRAC_PI_2 * f64::from(k);
            line(head, 0.8 * dn, 0.0, 0.8 * dn, ang, thick);
            line(0.0, dn / 2.0, tip, dn / 2.0, ang, thin);
        }
        circle(tip, dn / 2.0, thin);
        if !blind {
            let (n0, n1) = (a.length, a.length + 0.8 * dn);
            circle(n0, 0.85 * dn, thick);
            circle(n1, 0.85 * dn, thick);
            for k in 0..4 {
                let ang = std::f64::consts::FRAC_PI_2 * f64::from(k);
                line(n0, 0.85 * dn, n1, 0.85 * dn, ang, thick);
            }
        }
        // The axis, dashed past both ends.
        if let (Some(p), Some(q)) = (
            self.camera.project(&point(head - 0.5 * dn, 0.0, 0.0), rect),
            self.camera.project(&point(tip + 0.5 * dn, 0.0, 0.0), rect),
        ) {
            painter.extend(egui::Shape::dashed_line(&[p, q], thin, 6.0, 4.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(family: usize, role: Role) -> StackItem {
        StackItem { family, role }
    }

    #[test]
    fn the_stack_keeps_one_bolt_first_and_the_nut_last() {
        let mut head = Vec::new();
        insert(&mut head, item(1, Role::Washer));
        insert(&mut head, item(2, Role::Bolt));
        insert(&mut head, item(3, Role::Bolt));
        assert_eq!(head, vec![item(3, Role::Bolt), item(1, Role::Washer)]);
        let mut nut = Vec::new();
        insert(&mut nut, item(5, Role::Nut));
        insert(&mut nut, item(4, Role::Washer));
        assert_eq!(nut, vec![item(4, Role::Washer), item(5, Role::Nut)]);
    }

    #[test]
    fn selectors_follow_placement_and_type() {
        let mut d = PartDialog::new(Planned::Asm(AsmPlanned::BoltedConnection));
        assert!(!sel_hidden(&d, "edge1") && sel_hidden(&d, "circle") && sel_hidden(&d, "blind"));
        d.set_index(0, "Placement", 3);
        d.set_index(0, "Type", 1);
        assert!(sel_hidden(&d, "edge1") && !sel_hidden(&d, "hole"));
        assert!(sel_hidden(&d, "end") && !sel_hidden(&d, "blind"));
        assert!(row_off(&d, 0, "Follow Pattern").is_none());
        assert!(
            row_off(&d, 0, "Distance 1").is_some() && row_off(&d, 1, "Max. Axial Force").is_some()
        );
        assert!(!placement_done(&d));
    }

    #[test]
    fn the_diameter_list_starts_at_m8() {
        let mut d = PartDialog::new(Planned::Asm(AsmPlanned::BoltedConnection));
        start(&mut d);
        assert_eq!(choice(&d, "Thread").as_deref(), Some("ISO Metric profile"));
        assert_eq!(choice(&d, "Diameter").as_deref(), Some("M8"));
        assert_eq!(holes(&d).map(|h| h.1.clearance), Some(9.0));
    }
}
