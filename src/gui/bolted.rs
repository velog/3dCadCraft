//! Bolted Connection's panel (spec `docs/spec/6.2-bolted-connection.md`):
//! the pieces the shared planned panel (`gui/part_planned.rs`) cannot draw
//! from its rows — the selectors and rows each Placement uses, the thread and
//! library lists, the fastener stack (head side, the plates and their holes,
//! nut side: reordered, inserted between, checked), the smart selectors
//! (a hole's rim gives the start plane and the centre at once), the
//! termination found along the axis, the schematic preview, and the
//! Calculation tabs' rows, shown disabled until their logic exists.

use std::sync::Arc;

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::panels::plabel;
use super::part_planned::{PartDialog, Planned};
use super::theme::{self, Glyph};
use crate::assembly::bolted::{self, BoltKind, Clicked, Placement};
use crate::assembly::{Geom, GeomRef};
use crate::components::catalog::Catalog;
use crate::components::fit::Fit;
use crate::part::hole::FastenerFit;
use crate::part::standards::Standards;

/// The tooltip of the Calculation and Fatigue Calculation rows.
const CALC_TIP: &str = "Strength check: comes after the geometry (spec 6.2 §3.7)";
/// The tooltip of what the stack rows will do once the logic exists.
const LOGIC_TIP: &str = "they come with the command's logic (spec 6.2 §3.4)";
/// Plates closer than this are one stack (mm).
const STACK_TOL: f64 = 0.05;

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

    fn title(self) -> &'static str {
        match self {
            Role::Bolt => "Bolt",
            Role::Washer => "Washer",
            Role::Nut => "Nut",
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

/// A plate the bolt passes, for the panel.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlateRow {
    /// The occurrence's name.
    pub name: String,
    /// Its thickness along the axis.
    pub thickness: f64,
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
    /// The library list open: the side and where the new fastener goes.
    pub adding: Option<(Side, usize)>,
    /// The axis from the picks (assembly coordinates), or why there is none.
    pub axis: Option<Result<bolted::Axis, String>>,
    /// The plates along the axis, start plane first.
    pub plates: Vec<PlateRow>,
    /// The termination selector holds the termination found along the axis
    /// (not the user's pick): it follows the start and the reference.
    pub auto_end: bool,
    /// The inputs [`Self::axis`] was found from (it is found again only when
    /// they change: rule 20).
    pub key: String,
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

fn placement(d: &PartDialog) -> Placement {
    Placement::from_index(d.index(0, "Placement"))
}

/// The selector holding the termination for the type chosen.
fn end_key(d: &PartDialog) -> &'static str {
    if kind(d) == BoltKind::Blind {
        "blind"
    } else {
        "end"
    }
}

/// The selectors Placement and Type have no use for (spec 6.2 §3.2).
pub(super) fn sel_hidden(d: &PartDialog, key: &str) -> bool {
    if !is_bolted(d) {
        return false;
    }
    let p = placement(d);
    match key {
        "edge1" | "edge2" => p != Placement::Linear,
        "circle" => p != Placement::Concentric,
        "point" => p != Placement::OnPoint,
        "hole" => p != Placement::ByHole,
        "end" | "blind" => key != end_key(d),
        _ => false,
    }
}

/// Rows the placement has no use for are not shown (the distances belong to
/// Linear, Follow Pattern to By Hole).
pub(super) fn row_hidden(d: &PartDialog, ti: usize, label: &str) -> bool {
    is_bolted(d)
        && ti == 0
        && match label {
            "Distance 1" | "Distance 2" => placement(d) != Placement::Linear,
            "Follow Pattern" => placement(d) != Placement::ByHole,
            _ => false,
        }
}

/// The reference selectors of a placement.
fn reference_keys(p: Placement) -> &'static [&'static str] {
    match p {
        Placement::Linear => &["edge1", "edge2"],
        Placement::Concentric => &["circle"],
        Placement::OnPoint => &["point"],
        Placement::ByHole => &["hole"],
    }
}

/// Whether the placement is complete: start, its reference(s), the end.
pub(super) fn placement_done(d: &PartDialog) -> bool {
    let has = |k: &str| !d.picks_of(k).is_empty();
    has("start") && reference_keys(placement(d)).iter().all(|k| has(k)) && has(end_key(d))
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
        "Follow Pattern" => Some("Comes with the logic: needs a hole of a pattern (spec 6.2 §3.2)"),
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

/// Whether family `f` of role `role` fits nominal diameter `dn`.
fn family_fits(c: &Catalog, f: usize, role: Role, dn: f64) -> bool {
    c.fits(f).iter().any(|fit| match (role, fit) {
        (
            Role::Bolt,
            Fit::Into {
                d, length: Some(_), ..
            },
        ) => (d - dn).abs() < 0.01,
        (Role::Nut, Fit::Over { bore }) => (bore - dn).abs() < 0.01,
        (Role::Washer, Fit::Over { bore }) => *bore >= dn - 0.01 && *bore <= dn * 1.2 + 0.2,
        _ => false,
    })
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
    let mut v = Vec::new();
    for (i, f) in c.families.iter().enumerate() {
        let Some(role) = Role::of(&f.category) else {
            continue;
        };
        let side_ok = match side {
            Side::Head => role != Role::Nut,
            Side::Nut => role != Role::Bolt,
        };
        if side_ok
            && standard.as_ref().is_none_or(|s| *s == f.standard)
            && category
                .as_ref()
                .is_none_or(|s| *s == category_name(&f.category))
            && family_fits(c, i, role, h.major)
        {
            v.push((i, role));
        }
    }
    v
}

// ---- the order of the stack ------------------------------------------------

/// Puts `item` at `pos` of one side and returns where it went: one bolt (a
/// new one replaces it) always first on the head side, under its head the
/// washers; on the nut side anything in any order (washer, nut, a jam nut).
pub(super) fn insert_at(
    list: &mut Vec<StackItem>,
    side: Side,
    pos: usize,
    item: StackItem,
) -> usize {
    if item.role == Role::Bolt {
        list.retain(|x| x.role != Role::Bolt);
        list.insert(0, item);
        return 0;
    }
    let first =
        usize::from(side == Side::Head && list.first().is_some_and(|x| x.role == Role::Bolt));
    let at = pos.clamp(first, list.len());
    list.insert(at, item);
    at
}

/// Moves item `k` one place up (towards the head / the plates) or down:
/// nothing goes above the bolt and the bolt does not move.
pub(super) fn move_item(list: &mut [StackItem], k: usize, up: bool) -> bool {
    let Some(j) = (if up { k.checked_sub(1) } else { Some(k + 1) }) else {
        return false;
    };
    if j >= list.len() || list[k].role == Role::Bolt || list[j].role == Role::Bolt {
        return false;
    }
    list.swap(k, j);
    true
}

/// Where the stack is not a working joint yet, as the panel says it.
pub(super) fn stack_problems(d: &PartDialog) -> Vec<String> {
    let mut v = Vec::new();
    let blind = kind(d) == BoltKind::Blind;
    let any = !d.bolt.head.is_empty() || !d.bolt.nut.is_empty();
    if any && !d.bolt.head.iter().any(|x| x.role == Role::Bolt) {
        v.push("Head side: add a bolt".to_owned());
    }
    if any && !blind && !d.bolt.nut.iter().any(|x| x.role == Role::Nut) {
        v.push("Nut side: add a nut (Through All)".to_owned());
    }
    if let (Some(c), Some((size, h))) = (&d.bolt.catalog, holes(d)) {
        for x in d
            .bolt
            .head
            .iter()
            .chain(if blind { &[][..] } else { &d.bolt.nut[..] })
        {
            if !family_fits(c, x.family, x.role, h.major) {
                let name = c.families.get(x.family).map_or("", |f| f.name.as_str());
                v.push(format!("{name} has no {size}: choose another or delete it"));
            }
        }
    }
    v
}

// ---- the panel ---------------------------------------------------------------

/// The fastener stack (in place of the Click to add a fastener button): the
/// head side, the plates with their holes, the nut side, what is missing.
pub(super) fn stack_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    let done = placement_done(d);
    let blind = kind(d) == BoltKind::Blind;
    side_ui(d, ui, Side::Head, done);
    // The plates the bolt passes, or why the axis cannot be found.
    plabel(ui, "Plates");
    ui.vertical(|ui| {
        fit_cell(ui);
        match &d.bolt.axis {
            Some(Ok(a)) => {
                for (k, p) in d.bolt.plates.iter().enumerate() {
                    let last = blind && k + 1 == d.bolt.plates.len();
                    ui.label(
                        RichText::new(format!(
                            "{}  {:.2} mm{}",
                            p.name,
                            p.thickness,
                            if last { "  (tapped)" } else { "" }
                        ))
                        .small(),
                    );
                }
                ui.label(format!("Grip {:.2} mm", a.length));
            }
            Some(Err(e)) => {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("Bolted Connection: {e}"))
                            .color(theme::tokens().error),
                    )
                    .wrap(),
                );
            }
            None => {
                ui.add(
                    egui::Label::new(
                        RichText::new("Pick the start plane and the reference").weak(),
                    )
                    .wrap(),
                );
            }
        }
        match holes(d) {
            Some((size, h)) => {
                let text = if blind {
                    format!(
                        "Holes: Ø{} clearance (Normal fit), the last one {size} tapped (drill Ø{})",
                        h.clearance, h.tap_drill
                    )
                } else {
                    format!(
                        "Holes: Ø{} clearance through each (Normal fit)",
                        h.clearance
                    )
                };
                ui.add(egui::Label::new(RichText::new(text).small()).wrap());
                ui.add_enabled(false, egui::Button::new("Modify").small())
                    .on_disabled_hover_text(
                        "Hole type and fit: comes with the logic (spec 6.2 §3.4)",
                    );
            }
            None => {
                ui.label(RichText::new("no clearance hole for this size").weak());
            }
        }
    });
    ui.end_row();
    if !blind {
        side_ui(d, ui, Side::Nut, done);
    }
    let problems = stack_problems(d);
    if !done || !problems.is_empty() {
        plabel(ui, "");
        ui.vertical(|ui| {
            fit_cell(ui);
            if !done {
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Fasteners come after the placement: start plane, reference, termination",
                        )
                        .weak()
                        .italics(),
                    )
                    .wrap(),
                );
            }
            for p in problems {
                ui.add(
                    egui::Label::new(RichText::new(p).color(theme::tokens().warning)).wrap(),
                );
            }
        });
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
    enum Act {
        Remove(usize),
        Move(usize, bool),
        Open(usize),
        Add(StackItem),
    }
    let mut act = None;
    ui.vertical(|ui| {
        fit_cell(ui);
        let list = match side {
            Side::Head => d.bolt.head.clone(),
            Side::Nut => d.bolt.nut.clone(),
        };
        let open = d.bolt.adding.filter(|a| a.0 == side).map(|a| a.1);
        let n = list.len();
        for (k, item) in list.iter().enumerate() {
            let name = d
                .bolt
                .catalog
                .as_ref()
                .and_then(|c| c.families.get(item.family))
                .map_or_else(String::new, |f| f.name.clone());
            // "+" inserts towards the plates: under it on the head side,
            // above it on the nut side (a washer between the plate and the nut).
            let ins = if side == Side::Head { k + 1 } else { k };
            if side == Side::Nut
                && open == Some(ins)
                && let Some(item) = library_list(d, ui, side)
            {
                act = Some(Act::Add(item));
            }
            ui.horizontal(|ui| {
                fit_cell(ui);
                let bolt = item.role == Role::Bolt;
                let up_ok = !bolt && k > 0 && list[k - 1].role != Role::Bolt;
                let down_ok = !bolt && k + 1 < n;
                // The buttons on the right, the name in the rest.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::glyph(ui, Glyph::Close)
                        .on_hover_text("Delete this fastener")
                        .clicked()
                    {
                        act = Some(Act::Remove(k));
                    }
                    let tip = if side == Side::Head {
                        "Insert a fastener under this one"
                    } else {
                        "Insert a fastener above this one (towards the plates)"
                    };
                    if theme::glyph(ui, Glyph::Plus).on_hover_text(tip).clicked() {
                        act = Some(Act::Open(ins));
                    }
                    if ui
                        .add_enabled_ui(down_ok, |ui| theme::glyph(ui, Glyph::Down))
                        .inner
                        .on_hover_text("Move down")
                        .clicked()
                    {
                        act = Some(Act::Move(k, false));
                    }
                    if ui
                        .add_enabled_ui(up_ok, |ui| theme::glyph(ui, Glyph::Up))
                        .inner
                        .on_hover_text("Move up")
                        .clicked()
                    {
                        act = Some(Act::Move(k, true));
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(format!("{} {name}", item.role.title())).truncate(),
                        )
                        .on_hover_text(format!(
                            "{name}. Edit (size, length) and Change: {LOGIC_TIP}"
                        ));
                    });
                });
            });
            if side == Side::Head
                && open == Some(ins)
                && ins < n
                && let Some(item) = library_list(d, ui, side)
            {
                act = Some(Act::Add(item));
            }
        }
        // Inventor's row at the end of the side: next to the plates on the
        // head side, farthest from them on the nut side.
        let b = ui
            .add_enabled(
                done,
                egui::Button::new("Click to add a fastener").selected(open == Some(n)),
            )
            .on_disabled_hover_text("Select the placement first (spec 6.2 §3.2)");
        if b.clicked() {
            act = Some(Act::Open(n));
        }
        if open == Some(n)
            && done
            && let Some(item) = library_list(d, ui, side)
        {
            act = Some(Act::Add(item));
        }
    });
    ui.end_row();
    let Some(act) = act else {
        return;
    };
    let list = match side {
        Side::Head => &mut d.bolt.head,
        Side::Nut => &mut d.bolt.nut,
    };
    match act {
        Act::Remove(k) => {
            list.remove(k);
            d.bolt.adding = None;
        }
        Act::Move(k, up) => {
            move_item(list, k, up);
        }
        Act::Open(at) => {
            d.bolt.adding = if d.bolt.adding == Some((side, at)) {
                None
            } else {
                Some((side, at))
            };
        }
        Act::Add(item) => {
            let at = d.bolt.adding.map_or(list.len(), |a| a.1);
            insert_at(list, side, at, item);
            d.bolt.adding = None;
        }
    }
}

/// The library list of the families that fit; the one clicked.
fn library_list(d: &PartDialog, ui: &mut egui::Ui, side: Side) -> Option<StackItem> {
    let found = offered(d, side);
    if found.is_empty() {
        ui.add(
            egui::Label::new(
                RichText::new("nothing in the library fits this diameter and filter").weak(),
            )
            .wrap(),
        );
    }
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .id_salt(("bolt_add", side == Side::Head))
        .max_height(180.0)
        .min_scrolled_height(150.0)
        .show(ui, |ui| {
            for (f, role) in found {
                let Some(fam) = d.bolt.catalog.as_ref().and_then(|c| c.families.get(f)) else {
                    continue;
                };
                let text = format!("{} · {}", fam.name, category_name(&fam.category));
                let r = ui.add(egui::Button::selectable(false, text).truncate());
                if r.on_hover_text(format!("{} ({})", fam.name, category_name(&fam.category)))
                    .clicked()
                {
                    chosen = Some(StackItem { family: f, role });
                }
            }
        });
    chosen
}

/// Keeps a grid cell inside the Properties panel (it cannot widen: CLAUDE.md
/// "Properties cannot widen"), so its rows wrap.
fn fit_cell(ui: &mut egui::Ui) {
    let w = (ui.clip_rect().right() - ui.cursor().left() - 6.0).max(80.0);
    ui.set_max_width(w);
}

// ---- the view ------------------------------------------------------------------

impl CadCraftApp {
    /// The first pick of selector `key` in assembly coordinates.
    fn bolt_geom(&self, d: &PartDialog, key: &str) -> Option<Geom> {
        let r = d.refs.get(key)?.first()?;
        self.bolt_world(r)
    }

    fn bolt_world(&self, r: &GeomRef) -> Option<Geom> {
        let g = self.asm_local_geom(r).ok()?;
        let a = self.assembly()?;
        Some(match a.find(r.occurrence()) {
            Some(o) => g.moved(&o.transform),
            None => g,
        })
    }

    /// The reference of the placement chosen (`None`: not picked yet).
    fn bolt_reference(&self, d: &PartDialog, start: &Geom) -> Option<bolted::Reference> {
        Some(match placement(d) {
            Placement::Linear => {
                let Geom::Plane { p, .. } = *start else {
                    return None;
                };
                bolted::Reference::Linear {
                    e1: self.bolt_geom(d, "edge1")?,
                    e2: self.bolt_geom(d, "edge2")?,
                    d1: d.num(0, "Distance 1").unwrap_or(10.0),
                    d2: d.num(0, "Distance 2").unwrap_or(10.0),
                    toward: p,
                }
            }
            Placement::Concentric => bolted::Reference::Through(self.bolt_geom(d, "circle")?),
            Placement::OnPoint => bolted::Reference::Through(self.bolt_geom(d, "point")?),
            Placement::ByHole => bolted::Reference::Through(self.bolt_geom(d, "hole")?),
        })
    }

    /// The inputs the axis depends on, as text (found again only when it changes).
    fn bolt_key(d: &PartDialog) -> String {
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            kind(d),
            placement(d),
            d.refs.get("start"),
            reference_keys(placement(d))
                .iter()
                .map(|k| d.refs.get(k))
                .collect::<Vec<_>>(),
            d.refs.get(end_key(d)),
            d.num(0, "Distance 1"),
            d.num(0, "Distance 2"),
            choice(d, "Diameter"),
            d.bolt.auto_end,
            d.picks_of(end_key(d)).len(),
        )
    }

    /// Finds the axis, the plates along it and (when the termination is not
    /// the user's) the termination, when the panel's inputs changed.
    pub(super) fn bolt_follow(&mut self) {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return;
        };
        let key = Self::bolt_key(d);
        if key == d.bolt.key {
            return;
        }
        let end_key = end_key(d);
        let user_end = d.picks_of(end_key).len() == 1 && !d.bolt.auto_end;
        let probe = holes(d).map_or(6.0, |(_, h)| {
            (h.clearance / 2.0 + 0.75 * h.major) / 2.0 + 1.0
        });
        let start = self.bolt_geom(d, "start");
        let entry = start.as_ref().and_then(|s| {
            self.bolt_reference(d, s)
                .map(|r| bolted::entry(s, &r).map(|a| (a, r)))
        });
        // The plates along the axis (the found termination needs them).
        let mut plates = Vec::new();
        let mut found_end = None;
        if let Some(Ok((a, _))) = &entry {
            let parts = self.pick_parts();
            let meshes: Vec<_> = parts.iter().map(|p| self.asm_mesh(p.component)).collect();
            let list: Vec<_> = parts
                .iter()
                .zip(&meshes)
                .filter_map(|(p, m)| m.as_ref().map(|m| (m.triangles(), p.place)))
                .collect();
            let index: Vec<usize> = (0..parts.len()).filter(|i| meshes[*i].is_some()).collect();
            let s = bolted::stack_along(&list, a, probe, STACK_TOL);
            for p in &s {
                let part = &parts[index[p.part]];
                plates.push(PlateRow {
                    name: self
                        .occurrence_label_pub(part.top)
                        .map(|l| l.split(" · ").next().unwrap_or_default().to_owned())
                        .unwrap_or_default(),
                    thickness: p.to - p.from,
                });
            }
            if !user_end && let Some((i, face, t)) = bolted::auto_end(&s, kind(d)) {
                // `i` is the plate's part: an index into `list`.
                let part = &parts[index[i]];
                let local = part.place.inverse() * (a.at + a.dir * t);
                if let Some(name) = self.face_name_pub(part, face) {
                    found_end = Some(GeomRef::Face {
                        occurrence: part.top,
                        sub: part.sub.clone(),
                        name,
                        hint: [local.x, local.y, local.z],
                    });
                }
            }
        }
        let label = found_end
            .as_ref()
            .map(|g| format!("{} (auto)", self.geom_label(g)));
        let Some(d) = self.part_planned.as_mut() else {
            return;
        };
        if !user_end {
            match (found_end, label) {
                (Some(g), Some(l)) => {
                    d.set_ref(end_key, l, g);
                    d.bolt.auto_end = true;
                    if d.active == Some(end_key) {
                        d.activate_first_empty();
                    }
                }
                _ if d.bolt.auto_end => {
                    d.clear_ref(end_key);
                    d.bolt.auto_end = false;
                }
                _ => {}
            }
        }
        d.bolt.plates = plates;
        let end = d.refs.get(end_key).and_then(|v| v.first()).cloned();
        let axis = match (entry, start, end) {
            (Some(Err(e)), ..) => Some(Err(e)),
            (Some(Ok((_, r))), Some(s), Some(e)) => {
                let end = self.bolt_world(&e);
                end.map(|e| bolted::axis(&s, &r, &e))
            }
            _ => None,
        };
        let Some(d) = self.part_planned.as_mut() else {
            return;
        };
        d.bolt.axis = axis;
        d.bolt.key = Self::bolt_key(d);
    }

    /// The smart selectors (spec 6.2 §3.2): a click in the view while the
    /// Bolted Connection panel waits. `true`: the click was used (or refused
    /// with a message).
    pub(super) fn bolt_click(&mut self, pos: egui::Pos2, rect: egui::Rect) -> bool {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return false;
        };
        let Some(active) = d.active else {
            return false;
        };
        let Some(g) = self.geom_under_pub(pos, rect) else {
            return true;
        };
        let geom = self.bolt_world(&g);
        let what = match (&g, &geom) {
            (GeomRef::Face { .. }, Some(Geom::Plane { .. })) => Clicked::PlanarFace,
            (GeomRef::Face { .. }, Some(Geom::Cylinder { .. })) => Clicked::RoundFace,
            (GeomRef::Edge { .. }, Some(Geom::Circle { .. })) => Clicked::CircularEdge,
            (GeomRef::Edge { .. }, Some(Geom::Axis { .. })) => Clicked::StraightEdge,
            _ => {
                let w = self.geom_label(&g);
                self.file_status = Some(Err(format!("Bolted Connection: {w} cannot be used here")));
                return true;
            }
        };
        let edge1 = !d.picks_of("edge1").is_empty();
        let Some(route) = bolted::route(placement(d), active, what, edge1) else {
            let w = self.geom_label(&g);
            let asked = d
                .flow()
                .current()
                .map_or(String::new(), |s| s.label.to_lowercase());
            self.file_status = Some(Err(format!("Bolted Connection: {w} cannot be the {asked}")));
            return true;
        };
        // An edge clicked for the start plane: its planar face (the one under
        // the cursor when it is one of the edge's, else the one square to a
        // circle / facing the eye).
        let start = if route.keys.first() == Some(&"start*") {
            match self.bolt_edge_face(&g, &geom, pos, rect) {
                Some(f) => Some(f),
                None => {
                    self.file_status = Some(Err(
                        "Bolted Connection: that edge bounds no planar face".into(),
                    ));
                    return true;
                }
            }
        } else {
            None
        };
        let labels: Vec<(String, GeomRef)> = route
            .keys
            .iter()
            .map(|k| match (*k, &start) {
                ("start*", Some(f)) => (self.geom_label(f), f.clone()),
                _ => (self.geom_label(&g), g.clone()),
            })
            .collect();
        let Some(d) = self.part_planned.as_mut() else {
            return true;
        };
        if let Some(p) = route.placement {
            d.set_index(0, "Placement", p.index());
        }
        for (k, (label, r)) in route.keys.iter().zip(labels) {
            let key = if *k == "start*" { "start" } else { k };
            if key == "point" {
                d.add_ref(key, label, r);
            } else {
                d.set_ref(key, label, r);
            }
            if key == "end" || key == "blind" {
                d.bolt.auto_end = false;
            }
        }
        if active != "point" {
            d.activate_first_empty();
        }
        true
    }

    /// The planar face an edge bounds, for the start plane.
    fn bolt_edge_face(
        &self,
        g: &GeomRef,
        geom: &Option<Geom>,
        pos: egui::Pos2,
        rect: egui::Rect,
    ) -> Option<GeomRef> {
        let GeomRef::Edge {
            occurrence,
            sub,
            edge,
        } = g
        else {
            return None;
        };
        let plane_of = |f: &GeomRef| match self.bolt_world(f) {
            Some(Geom::Plane { n, .. }) => Some(n),
            _ => None,
        };
        if let Some(f) = self.face_under_pub(pos, rect)
            && matches!(&f, GeomRef::Face { name, .. } if edge.faces.contains(name))
            && plane_of(&f).is_some()
        {
            return Some(f);
        }
        let view = self.camera.ray(pos, rect).map(|(_, d)| d);
        edge.faces
            .iter()
            .map(|name| GeomRef::Face {
                occurrence: *occurrence,
                sub: sub.clone(),
                name: name.clone(),
                hint: edge.hint,
            })
            .filter_map(|f| plane_of(&f).map(|n| (f, n)))
            .max_by(|a, b| {
                let score = |n: &crate::kernel::Vector3| match (geom, view) {
                    (Some(Geom::Circle { n: c, .. }), _) => n.dot(c).abs(),
                    (_, Some(v)) => -n.dot(&v),
                    _ => 0.0,
                };
                score(&a.1).total_cmp(&score(&b.1))
            })
            .map(|x| x.0)
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
        let color = theme::canvas().hover_edge;
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

    fn dialog() -> PartDialog {
        let mut d = PartDialog::new(Planned::Asm(AsmPlanned::BoltedConnection));
        start(&mut d);
        d
    }

    /// Use case 8: the head side keeps one bolt first; washers go under it
    /// wherever they are inserted; a second bolt replaces the first.
    #[test]
    fn use_case_head_side_order() {
        let mut head = Vec::new();
        assert_eq!(
            insert_at(&mut head, Side::Head, 0, item(1, Role::Washer)),
            0
        );
        assert_eq!(insert_at(&mut head, Side::Head, 1, item(2, Role::Bolt)), 0);
        // A washer inserted above the bolt lands just under it.
        assert_eq!(
            insert_at(&mut head, Side::Head, 0, item(3, Role::Washer)),
            1
        );
        assert_eq!(insert_at(&mut head, Side::Head, 9, item(4, Role::Bolt)), 0);
        assert_eq!(
            head,
            vec![
                item(4, Role::Bolt),
                item(3, Role::Washer),
                item(1, Role::Washer)
            ]
        );
        // Nothing moves above the bolt, the bolt does not move.
        assert!(!move_item(&mut head, 1, true));
        assert!(!move_item(&mut head, 0, false));
        assert!(move_item(&mut head, 1, false));
        assert_eq!(head[1], item(1, Role::Washer));
        assert!(!move_item(&mut head, 2, false));
    }

    /// Use case 9: the nut side in any order — a washer between the plate
    /// and the nut, a jam nut after it, a washer inserted between the two nuts.
    #[test]
    fn use_case_nut_side_insert_between_and_reorder() {
        let mut nut = Vec::new();
        insert_at(&mut nut, Side::Nut, 0, item(10, Role::Nut));
        insert_at(&mut nut, Side::Nut, 0, item(11, Role::Washer));
        insert_at(&mut nut, Side::Nut, 2, item(12, Role::Nut));
        assert_eq!(insert_at(&mut nut, Side::Nut, 2, item(13, Role::Washer)), 2);
        assert_eq!(
            nut.iter().map(|x| x.family).collect::<Vec<_>>(),
            vec![11, 10, 13, 12]
        );
        assert!(move_item(&mut nut, 0, false));
        assert!(move_item(&mut nut, 3, true));
        assert_eq!(
            nut.iter().map(|x| x.family).collect::<Vec<_>>(),
            vec![10, 11, 12, 13]
        );
        assert!(!move_item(&mut nut, 0, true));
    }

    /// Use case 10: the panel follows the placement — only its selectors and
    /// rows are shown, Next goes through them in order, each a colour of its own.
    #[test]
    fn use_case_each_placement_shows_its_inputs() {
        let mut d = dialog();
        let shown = |d: &PartDialog| d.flow().selectors.iter().map(|s| s.key).collect::<Vec<_>>();
        assert_eq!(shown(&d), vec!["start", "edge1", "edge2", "end"]);
        assert!(!row_hidden(&d, 0, "Distance 1") && row_hidden(&d, 0, "Follow Pattern"));
        d.set_index(0, "Placement", Placement::Concentric.index());
        assert_eq!(shown(&d), vec!["start", "circle", "end"]);
        assert!(row_hidden(&d, 0, "Distance 2"));
        d.set_index(0, "Placement", Placement::OnPoint.index());
        assert_eq!(shown(&d), vec!["start", "point", "end"]);
        d.set_index(0, "Placement", Placement::ByHole.index());
        d.set_index(0, "Type", 1);
        assert_eq!(shown(&d), vec!["start", "hole", "blind"]);
        assert!(!row_hidden(&d, 0, "Follow Pattern"));
        let colours: Vec<_> = d.flow().selectors.iter().map(|s| s.color).collect();
        for (i, c) in colours.iter().enumerate() {
            assert!(!colours[..i].contains(c), "{colours:?}");
        }
        assert!(row_off(&d, 1, "Max. Axial Force").is_some());
        assert!(!placement_done(&d));
    }

    /// Use case 11: what the stack still lacks is said in the panel.
    #[test]
    fn use_case_the_stack_says_what_is_missing() {
        let mut d = dialog();
        assert!(stack_problems(&d).is_empty());
        d.bolt.head.push(item(0, Role::Washer));
        assert_eq!(
            stack_problems(&d),
            vec!["Head side: add a bolt", "Nut side: add a nut (Through All)"]
        );
        d.set_index(0, "Type", 1);
        assert_eq!(stack_problems(&d), vec!["Head side: add a bolt"]);
    }

    #[test]
    fn the_diameter_list_starts_at_m8() {
        let d = dialog();
        assert_eq!(choice(&d, "Thread").as_deref(), Some("ISO Metric profile"));
        assert_eq!(choice(&d, "Diameter").as_deref(), Some("M8"));
        assert_eq!(holes(&d).map(|h| h.1.clearance), Some(9.0));
    }
}
