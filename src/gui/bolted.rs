//! Bolted Connection's panel (spec `docs/spec/6.2-bolted-connection.md`):
//! the pieces the shared planned panel (`gui/part_planned.rs`) cannot draw
//! from its rows — the selectors and rows each Placement uses, the thread and
//! library lists, the fastener stack as one column (the bolt, a **+** between
//! every two places, the plates with the hole each gets, the nut side), the
//! smart selectors (a hole's rim gives the start plane and the centre at
//! once), the termination found along the axis, and in the view: every pick in
//! its selector's colour (rule 23), Linear's distances, the bolts of a grid or
//! a bolt circle, the chosen fasteners themselves see-through, the length
//! arrow and the **+** where a fastener can go.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::panels::plabel;
use super::part_planned::{PartDialog, Planned};
use super::theme::{self, Glyph};
use crate::assembly::PickMesh;
use crate::assembly::bolted::{self, BoltKind, Clicked, Placement, Spread};
use crate::assembly::{Geom, GeomRef};
use crate::components::catalog::Catalog;
use crate::components::fit::Fit;
use crate::kernel::{Point3, Vector3};
use crate::part::hole::FastenerFit;
use crate::part::standards::Standards;

/// The tooltip of the Calculation and Fatigue Calculation rows.
const CALC_TIP: &str = "Strength check: comes after the geometry (spec 6.2 §3.7)";
/// Plates closer than this are one stack (mm).
const STACK_TOL: f64 = 0.05;
/// A **+** or the length arrow in the view is hit this close (px).
const GRIP_PX: f32 = 9.0;

pub(super) use crate::assembly::bolted::Role;

/// Which side of the plates a fastener sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    /// Its occurrence path (the top occurrence, then the path below it).
    pub path: Vec<u32>,
    /// Where it starts along the axis (from the start plane).
    pub from: f64,
    /// The axis meets material in it (`false`: it already has a hole there,
    /// which is not cut again).
    pub solid: bool,
    /// Its part, the face the bolt enters it by and where (part coordinates):
    /// the hole written into the part starts there.
    pub component: crate::document::DocRef,
    pub face: Option<String>,
    pub local: Point3,
}

/// The Component Library window opened for the stack: which place, whether
/// it replaces the fastener there, what it lists.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BoltPick {
    pub side: Side,
    pub at: usize,
    /// Change: the fastener at `at` is replaced (else one is inserted there).
    pub replace: bool,
    /// The kinds that may go there.
    pub roles: Vec<Role>,
    /// The category the window opens on (`Fasteners/Bolts`).
    pub category: String,
    /// The family shown first (Change: the current one).
    pub current: Option<String>,
    /// The connection's size (`M8`): the member picked in the window.
    pub size: String,
}

/// One fastener of the preview: the member's triangles at its seat.
#[derive(Clone)]
pub(super) struct Ghost {
    pub file: PathBuf,
    pub mesh: Arc<PickMesh>,
    pub nut_side: bool,
    pub offset: f64,
}

/// What OK would place, for the view and the panel.
#[derive(Clone, Default)]
pub(super) struct View {
    /// Each fastener of one bolt's stack, in stack order.
    pub ghosts: Vec<Ghost>,
    /// The bolt's length.
    pub length: f64,
    /// The lengths of its family that work for this grip.
    pub lengths: Vec<f64>,
    /// The bolt's designation (`ISO 4017 M8 x 35`).
    pub bolt: String,
    /// Each head-side item's seat above the start plane, each nut-side item's
    /// seat below the termination, and where the nut side ends.
    pub head_seats: Vec<f64>,
    pub nut_seats: Vec<f64>,
    pub nut_end: f64,
}

impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "View({} {} mm)", self.bolt, self.length)
    }
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
    /// Every bolt's axis from the picks (assembly coordinates), or why there
    /// is none.
    pub axes: Option<Result<Vec<bolted::Axis>, String>>,
    /// The plates along each bolt, start plane first.
    pub plates: Vec<Vec<PlateRow>>,
    /// The termination selector holds the termination found along the axis
    /// (not the user's pick): it follows the start and the reference.
    pub auto_end: bool,
    /// The inputs [`Self::axes`] was found from (it is found again only when
    /// they change: rule 20).
    pub key: String,
    /// Edit: the connection OK replaces.
    pub editing: Option<u32>,
    /// The bolt length the user chose (`None`: the shortest that works).
    pub length: Option<f64>,
    /// What OK would place, or why it cannot.
    pub view: Option<Result<View, String>>,
    /// The inputs [`Self::view`] was found from.
    pub view_key: String,
    /// The members' triangles, by file (loaded once).
    pub meshes: HashMap<PathBuf, Arc<PickMesh>>,
    /// The library list opened from a **+** in the view: its place and where.
    pub menu: Option<(Side, usize, Pos2)>,
    /// The length arrow held: the length and the pointer when it was pressed.
    pub grip: Option<(f64, Pos2)>,
    /// The panel asks for the Component Library window.
    pub library: Option<BoltPick>,
    /// Follow Pattern was applied to these picks (found again when they change).
    pub followed: Option<String>,
}

impl std::fmt::Debug for Ghost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ghost({})", self.offset)
    }
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

pub(super) fn placement(d: &PartDialog) -> Placement {
    Placement::from_index(d.index(0, "Placement"))
}

/// The clearance fit chosen.
pub(super) fn fit(d: &PartDialog) -> FastenerFit {
    match d.index(0, "Hole Fit") {
        0 => FastenerFit::Close,
        2 => FastenerFit::Loose,
        _ => FastenerFit::Normal,
    }
}

/// How many bolts the references give (spec 6.2 §3.2c).
pub(super) fn spread(d: &PartDialog) -> Spread {
    let count = |label: &str| d.num(0, label).unwrap_or(1.0).round().clamp(1.0, 500.0) as u32;
    match placement(d) {
        Placement::Linear => {
            let c = [count("Count 1"), count("Count 2")];
            if c == [1, 1] {
                Spread::One
            } else {
                Spread::Grid {
                    count: c,
                    spacing: [
                        d.num(0, "Spacing 1").unwrap_or(30.0),
                        d.num(0, "Spacing 2").unwrap_or(30.0),
                    ],
                }
            }
        }
        Placement::Circular => Spread::Circle {
            count: count("Count"),
            radius: d.num(0, "Radius").unwrap_or(40.0),
            angle: d.num(0, "Angle").unwrap_or(0.0).to_radians(),
            span: d.num(0, "Span").unwrap_or(360.0).to_radians(),
        },
        _ => Spread::One,
    }
}

/// The selector holding the termination for the type chosen.
pub(super) fn end_key(d: &PartDialog) -> &'static str {
    if kind(d) == BoltKind::Blind {
        "blind"
    } else {
        "end"
    }
}

/// Selectors that take several picks (a bolt in each).
pub(super) fn multi(d: &PartDialog, key: &str) -> bool {
    match key {
        "point" | "hole" => true,
        "circle" => placement(d) == Placement::Concentric,
        _ => false,
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
        "circle" => !matches!(p, Placement::Concentric | Placement::Circular),
        "point" => p != Placement::OnPoint,
        "hole" => p != Placement::ByHole,
        "end" | "blind" => key != end_key(d),
        _ => false,
    }
}

/// Rows the placement has no use for are not shown (the distances and the
/// grid belong to Linear, the bolt circle to Circular, Follow Pattern to By
/// Hole).
pub(super) fn row_hidden(d: &PartDialog, ti: usize, label: &str) -> bool {
    is_bolted(d)
        && ti == 0
        && match label {
            "Distance 1" | "Distance 2" | "Count 1" | "Spacing 1" | "Count 2" | "Spacing 2" => {
                placement(d) != Placement::Linear
            }
            "Count" | "Radius" | "Angle" | "Span" => placement(d) != Placement::Circular,
            "Follow Pattern" => !matches!(placement(d), Placement::ByHole | Placement::Concentric),
            _ => false,
        }
}

/// The reference selectors of a placement.
pub(super) fn reference_keys(p: Placement) -> &'static [&'static str] {
    match p {
        Placement::Linear => &["edge1", "edge2"],
        Placement::Concentric | Placement::Circular => &["circle"],
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
    let _ = label;
    None
}

/// The family of the library named `name` (`ISO 4017`), if it fits `role`.
fn family_named(c: &Catalog, name: &str, role: Role) -> Option<usize> {
    c.families
        .iter()
        .position(|f| f.name == name && Role::of(&f.category) == Some(role))
}

/// Opens the panel's own state: the library, M8 as the first diameter, and
/// a usual stack (a hex bolt and a washer under its head, a washer and a
/// nut on the far side) when the library has them.
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
    usual_stack(d);
}

/// The usual stack (spec 6.2 §3.4): ISO 4017 + ISO 7089 / ISO 7089 + ISO 4032.
pub(super) fn usual_stack(d: &mut PartDialog) {
    let Some(c) = d.bolt.catalog.clone() else {
        return;
    };
    let item =
        |name: &str, role| family_named(&c, name, role).map(|family| StackItem { family, role });
    let (bolt, washer, nut) = (
        item("ISO 4017", Role::Bolt),
        item("ISO 7089", Role::Washer),
        item("ISO 4032", Role::Nut),
    );
    d.bolt.head = bolt.into_iter().chain(washer.clone()).collect();
    d.bolt.nut = washer.into_iter().chain(nut).collect();
}

/// The nominal diameter chosen (mm), with its holes.
pub(super) fn holes(d: &PartDialog) -> Option<(String, bolted::HoleSizes)> {
    let thread = choice(d, "Thread")?;
    let size = choice(d, "Diameter")?;
    let h = bolted::hole_sizes(Standards::global(), &thread, &size, fit(d))?;
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

/// What may go at place `at` of `side`: a bolt at the top when there is
/// none, washers under the head; washers and nuts on the nut side.
pub(super) fn slot_roles(d: &PartDialog, side: Side, at: usize) -> &'static [Role] {
    match side {
        Side::Head if at == 0 && !d.bolt.head.iter().any(|x| x.role == Role::Bolt) => &[Role::Bolt],
        Side::Head => &[Role::Washer],
        Side::Nut => &[Role::Washer, Role::Nut],
    }
}

/// The families of `roles` that fit the chosen diameter and the Standard and
/// Category filters.
fn offered(d: &PartDialog, roles: &[Role]) -> Vec<(usize, Role)> {
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
        if roles.contains(&role)
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

/// The library window's request for place `at` of `side`.
fn pick_for(d: &PartDialog, side: Side, at: usize, replace: bool) -> BoltPick {
    let roles: Vec<Role> = if replace {
        let list = match side {
            Side::Head => &d.bolt.head,
            Side::Nut => &d.bolt.nut,
        };
        match list.get(at).map(|x| x.role) {
            Some(Role::Bolt) => vec![Role::Bolt],
            Some(_) if side == Side::Head => vec![Role::Washer],
            _ => vec![Role::Washer, Role::Nut],
        }
    } else {
        slot_roles(d, side, at).to_vec()
    };
    let current = if replace {
        let list = match side {
            Side::Head => &d.bolt.head,
            Side::Nut => &d.bolt.nut,
        };
        list.get(at)
            .and_then(|x| d.bolt.catalog.as_ref()?.families.get(x.family))
            .map(|f| f.name.clone())
    } else {
        None
    };
    let category = match roles.as_slice() {
        [Role::Bolt] => "Fasteners/Bolts",
        [Role::Washer] => "Fasteners/Washers",
        [Role::Nut] => "Fasteners/Nuts",
        _ => "Fasteners",
    };
    BoltPick {
        side,
        at,
        replace,
        roles,
        category: category.into(),
        current,
        size: choice(d, "Diameter").unwrap_or_default(),
    }
}

/// Puts library family `name` into the stack where `pick` says.
pub(super) fn use_family(
    d: &mut PartDialog,
    pick: &BoltPick,
    name: &str,
    category: &str,
) -> Result<(), String> {
    let role = Role::of(category).ok_or("that is no bolt, washer or nut")?;
    if !pick.roles.contains(&role) {
        return Err(format!("a {} cannot go there", role.title().to_lowercase()));
    }
    let c = d
        .bolt
        .catalog
        .clone()
        .ok_or("the Component Library could not be read")?;
    let family =
        family_named(&c, name, role).ok_or_else(|| format!("{name} is not in the library"))?;
    let item = StackItem { family, role };
    let list = match pick.side {
        Side::Head => &mut d.bolt.head,
        Side::Nut => &mut d.bolt.nut,
    };
    if role == Role::Bolt {
        d.bolt.length = None;
    }
    if pick.replace && pick.at < list.len() {
        list[pick.at] = item;
    } else {
        insert_at(list, pick.side, pick.at, item);
    }
    d.bolt.adding = None;
    d.bolt.menu = None;
    Ok(())
}

/// What a click in the stack asks for.
enum Act {
    Remove(Side, usize),
    Move(Side, usize, bool),
    Open(Side, usize),
    Add(StackItem),
    Length(Option<f64>),
    Library(BoltPick),
}

/// A row's icon: the library category's picture (rule 23).
fn icon(ui: &mut egui::Ui, category: &str) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
    super::icons::paint_category(ui.painter(), r, category);
}

/// The plates' icon: a section of two plates with the bolt's hole.
fn plate_icon(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
    let p = ui.painter();
    let ink = theme::tokens().text;
    let s = Stroke::new(1.0, ink);
    for (y0, y1) in [(4.0, 9.5), (10.5, 16.0)] {
        let a = Rect::from_min_max(r.min + egui::vec2(1.0, y0), r.min + egui::vec2(19.0, y1));
        p.rect_stroke(a, 0.0, s, egui::StrokeKind::Inside);
        let mut x = a.left() + 2.0;
        while x < a.right() - 1.0 {
            if !(8.0..12.0).contains(&(x - r.left())) {
                p.line_segment(
                    [
                        Pos2::new(x, a.bottom() - 1.0),
                        Pos2::new(x + 2.5, a.top() + 1.0),
                    ],
                    Stroke::new(0.6, ink.gamma_multiply(0.6)),
                );
            }
            x += 3.0;
        }
    }
    p.line_segment(
        [
            r.min + egui::vec2(10.0, 1.0),
            r.min + egui::vec2(10.0, 19.0),
        ],
        Stroke::new(1.0, theme::tokens().accent),
    );
}

/// The fastener stack as one column (spec 6.2 §3.4): the bolt with its
/// length, a **+** at every place a fastener can go, the plates with their
/// holes, the nut side.
pub(super) fn stack_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    let blind = kind(d) == BoltKind::Blind;
    plabel(ui, "Stack");
    let mut act = None;
    ui.vertical(|ui| {
        fit_cell(ui);
        // The head side, from the head down.
        let head = d.bolt.head.clone();
        if !head.iter().any(|x| x.role == Role::Bolt) {
            plus_row(d, ui, Side::Head, 0, "Add a bolt", &mut act);
        }
        for (k, item) in head.iter().enumerate() {
            item_row(d, ui, Side::Head, k, item, &mut act);
            if item.role == Role::Bolt {
                length_row(d, ui, &mut act);
            }
            plus_row(d, ui, Side::Head, k + 1, "Insert a washer here", &mut act);
        }
        plates_ui(d, ui, blind);
        if !blind {
            let nut = d.bolt.nut.clone();
            plus_row(
                d,
                ui,
                Side::Nut,
                0,
                "Insert a washer or a nut here",
                &mut act,
            );
            for (k, item) in nut.iter().enumerate() {
                item_row(d, ui, Side::Nut, k, item, &mut act);
                plus_row(
                    d,
                    ui,
                    Side::Nut,
                    k + 1,
                    "Insert a washer or a nut here",
                    &mut act,
                );
            }
        }
        for p in stack_problems(d) {
            ui.add(egui::Label::new(RichText::new(p).color(theme::tokens().warning)).wrap());
        }
        match &d.bolt.view {
            Some(Err(e)) if placement_done(d) => {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("Bolted Connection: {e}"))
                            .color(theme::tokens().error),
                    )
                    .wrap(),
                );
            }
            Some(Ok(v)) => {
                let n = d
                    .bolt
                    .axes
                    .as_ref()
                    .and_then(|a| a.as_ref().ok())
                    .map_or(1, Vec::len);
                let text = if n > 1 {
                    format!("OK places {n} bolts {}", v.bolt)
                } else {
                    format!("OK places {}", v.bolt)
                };
                ui.label(RichText::new(text).color(theme::tokens().ok));
            }
            _ if !placement_done(d) => {
                ui.add(
                    egui::Label::new(
                        RichText::new("Pick the start plane, the reference and the termination")
                            .weak()
                            .italics(),
                    )
                    .wrap(),
                );
            }
            _ => {}
        }
    });
    ui.end_row();
    if let Some(a) = act {
        apply_act(d, a);
    }
}

/// Carries out a click of the stack.
fn apply_act(d: &mut PartDialog, act: Act) {
    fn list(d: &mut PartDialog, side: Side) -> &mut Vec<StackItem> {
        match side {
            Side::Head => &mut d.bolt.head,
            Side::Nut => &mut d.bolt.nut,
        }
    }
    match act {
        Act::Remove(side, k) => {
            if k < list(d, side).len() {
                list(d, side).remove(k);
            }
            d.bolt.adding = None;
        }
        Act::Move(side, k, up) => {
            move_item(list(d, side), k, up);
        }
        Act::Open(side, at) => {
            d.bolt.adding = if d.bolt.adding == Some((side, at)) {
                None
            } else {
                Some((side, at))
            };
        }
        Act::Add(item) => {
            let (side, at) = d
                .bolt
                .adding
                .or(d.bolt.menu.map(|m| (m.0, m.1)))
                .unwrap_or((Side::Head, 0));
            if item.role == Role::Bolt {
                d.bolt.length = None;
            }
            insert_at(list(d, side), side, at, item);
            d.bolt.adding = None;
            d.bolt.menu = None;
        }
        Act::Length(l) => d.bolt.length = l,
        Act::Library(p) => d.bolt.library = Some(p),
    }
}

/// A **+** between two places, and the library list under it when open.
fn plus_row(
    d: &PartDialog,
    ui: &mut egui::Ui,
    side: Side,
    at: usize,
    tip: &str,
    act: &mut Option<Act>,
) {
    let open = d.bolt.adding == Some((side, at));
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let r = theme::glyph(ui, Glyph::Plus).on_hover_text(tip);
        if r.clicked() {
            *act = Some(Act::Open(side, at));
        }
        let w = (ui.available_width() - 6.0).max(10.0);
        let (line, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
        let c = if open {
            theme::tokens().accent
        } else {
            theme::tokens().text.gamma_multiply(0.25)
        };
        ui.painter().line_segment(
            [line.left_center(), line.right_center()],
            Stroke::new(1.0, c),
        );
    });
    if open {
        if ui
            .button("Library...")
            .on_hover_text(
                "Choose in the Component Library window (search, categories, standards, preview)",
            )
            .clicked()
        {
            *act = Some(Act::Library(pick_for(d, side, at, false)));
        }
        if let Some(item) = library_list(d, ui, slot_roles(d, side, at), ("panel", side, at)) {
            *act = Some(Act::Add(item));
        }
    }
}

/// One fastener: its icon, role and family, ↑ ↓ ✕.
fn item_row(
    d: &PartDialog,
    ui: &mut egui::Ui,
    side: Side,
    k: usize,
    item: &StackItem,
    act: &mut Option<Act>,
) {
    let list = match side {
        Side::Head => &d.bolt.head,
        Side::Nut => &d.bolt.nut,
    };
    let fam = d
        .bolt
        .catalog
        .as_ref()
        .and_then(|c| c.families.get(item.family));
    let name = fam.map_or_else(String::new, |f| f.name.clone());
    let category = fam.map_or("", |f| f.category.as_str());
    ui.horizontal(|ui| {
        fit_cell(ui);
        icon(ui, category);
        let bolt = item.role == Role::Bolt;
        let up_ok = !bolt && k > 0 && list[k - 1].role != Role::Bolt;
        let down_ok = !bolt && k + 1 < list.len();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::glyph(ui, Glyph::Close)
                .on_hover_text("Delete this fastener")
                .clicked()
            {
                *act = Some(Act::Remove(side, k));
            }

            if ui
                .add_enabled_ui(down_ok, |ui| theme::glyph(ui, Glyph::Down))
                .inner
                .on_hover_text("Move down")
                .clicked()
            {
                *act = Some(Act::Move(side, k, false));
            }
            if ui
                .add_enabled_ui(up_ok, |ui| theme::glyph(ui, Glyph::Up))
                .inner
                .on_hover_text("Move up")
                .clicked()
            {
                *act = Some(Act::Move(side, k, true));
            }
            if theme::glyph(ui, Glyph::Edit)
                .on_hover_text(format!(
                    "Change this {} (another standard or type) in the Component Library",
                    item.role.title().to_lowercase()
                ))
                .clicked()
            {
                *act = Some(Act::Library(pick_for(d, side, k, true)));
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                // The name is the Change button: the Component Library window.
                let r = ui
                    .add(
                        egui::Button::new(RichText::new(name.clone()).strong())
                            .frame(false)
                            .truncate(),
                    )
                    .on_hover_text(format!(
                        "{}: {name} ({}). Click: change it in the Component Library (search, categories, preview)",
                        item.role.title(),
                        category_name(category)
                    ));
                if r.clicked() {
                    *act = Some(Act::Library(pick_for(d, side, k, true)));
                }
            });
        });
    });
}

/// Under the bolt: the lengths that work for this grip (auto = the shortest).
fn length_row(d: &PartDialog, ui: &mut egui::Ui, act: &mut Option<Act>) {
    let Some(Ok(v)) = &d.bolt.view else {
        return;
    };
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.label("Length");
        let shown = match d.bolt.length {
            Some(l) => format!("{l:.0} mm"),
            None => format!("auto ({:.0} mm)", v.length),
        };
        egui::ComboBox::from_id_salt("bolt_length")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(d.bolt.length.is_none(), "auto (the shortest)")
                    .clicked()
                {
                    *act = Some(Act::Length(None));
                }
                for l in &v.lengths {
                    let on = d.bolt.length.is_some_and(|x| (x - l).abs() < 1e-6);
                    if ui.selectable_label(on, format!("{l:.0} mm")).clicked() {
                        *act = Some(Act::Length(Some(*l)));
                    }
                }
            })
            .response
            .on_hover_text("The bolt's length; the arrow on its end in the view drags it");
    });
}

/// The plates the first bolt passes with the hole each gets, the grip.
fn plates_ui(d: &PartDialog, ui: &mut egui::Ui, blind: bool) {
    let hs = holes(d);
    ui.horizontal(|ui| {
        fit_cell(ui);
        plate_icon(ui);
        ui.vertical(|ui| match (&d.bolt.axes, d.bolt.plates.first()) {
            (Some(Ok(a)), Some(plates)) => {
                let n = plates.len();
                for (k, p) in plates.iter().enumerate() {
                    let last = blind && k + 1 == n;
                    let hole = match (&hs, p.solid, last) {
                        (_, false, _) => "existing hole".to_owned(),
                        (Some((size, h)), true, true) => {
                            format!("{size} tapped, drill Ø{}", h.tap_drill)
                        }
                        (Some((_, h)), true, false) => format!("Ø{} clearance", h.clearance),
                        (None, ..) => "no hole for this size".to_owned(),
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{}  {:.2} mm · {hole}", p.name, p.thickness))
                                .small(),
                        )
                        .wrap(),
                    );
                }
                let bolts = a.len();
                let grip = a.first().map_or(0.0, |x| x.length);
                ui.label(if bolts > 1 {
                    format!("Grip {grip:.2} mm · {bolts} bolts")
                } else {
                    format!("Grip {grip:.2} mm")
                });
            }
            (Some(Err(e)), _) => {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("Bolted Connection: {e}"))
                            .color(theme::tokens().error),
                    )
                    .wrap(),
                );
            }
            _ => {
                ui.add(egui::Label::new(RichText::new("Plates: pick the placement").weak()).wrap());
            }
        });
    });
}

/// The library list of the families of `roles` that fit; the one clicked.
fn library_list(
    d: &PartDialog,
    ui: &mut egui::Ui,
    roles: &[Role],
    id: impl std::hash::Hash + std::fmt::Debug,
) -> Option<StackItem> {
    let found = offered(d, roles);
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
        .id_salt(egui::Id::new(id))
        .max_height(200.0)
        .show(ui, |ui| {
            for (f, role) in found {
                let Some(fam) = d.bolt.catalog.as_ref().and_then(|c| c.families.get(f)) else {
                    continue;
                };
                let r = ui
                    .horizontal(|ui| {
                        icon(ui, &fam.category);
                        ui.add(
                            egui::Button::selectable(
                                false,
                                format!("{} · {}", fam.name, category_name(&fam.category)),
                            )
                            .truncate(),
                        )
                    })
                    .inner;
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

/// The two directions square to `dir`: a stable side and the third.
fn sides(dir: &Vector3) -> (Vector3, Vector3) {
    let u = if dir.x.abs() < 0.9 {
        dir.cross(&Vector3::x())
    } else {
        dir.cross(&Vector3::y())
    }
    .normalize();
    (u, dir.cross(&u))
}

// ---- the view ------------------------------------------------------------------

impl CadCraftApp {
    /// The first pick of selector `key` in assembly coordinates.
    pub(super) fn bolt_geom(&self, d: &PartDialog, key: &str) -> Option<Geom> {
        let r = d.refs.get(key)?.first()?;
        self.bolt_world(r)
    }

    pub(super) fn bolt_world(&self, r: &GeomRef) -> Option<Geom> {
        let g = self.asm_local_geom(r).ok()?;
        let a = self.assembly()?;
        Some(match a.find(r.occurrence()) {
            Some(o) => g.moved(&o.transform),
            None => g,
        })
    }

    /// The references of the placement chosen, one per pick (`None`: not
    /// picked yet).
    fn bolt_references(&self, d: &PartDialog, start: &Geom) -> Option<Vec<bolted::Reference>> {
        let through = |key: &str| -> Option<Vec<bolted::Reference>> {
            let v: Vec<_> = d
                .refs
                .get(key)?
                .iter()
                .filter_map(|r| self.bolt_world(r))
                .map(bolted::Reference::Through)
                .collect();
            (!v.is_empty()).then_some(v)
        };
        Some(match placement(d) {
            Placement::Linear => {
                let Geom::Plane { p, .. } = *start else {
                    return None;
                };
                vec![bolted::Reference::Linear {
                    e1: self.bolt_geom(d, "edge1")?,
                    e2: self.bolt_geom(d, "edge2")?,
                    d1: d.num(0, "Distance 1").unwrap_or(10.0),
                    d2: d.num(0, "Distance 2").unwrap_or(10.0),
                    // The side of the edges the start face was picked on.
                    toward: d
                        .refs
                        .get("start")
                        .and_then(|v| v.first())
                        .and_then(|r| self.bolt_pick_point(r))
                        .unwrap_or(p),
                }]
            }
            Placement::Circular => through("circle")?.into_iter().take(1).collect(),
            Placement::Concentric => through("circle")?,
            Placement::OnPoint => through("point")?,
            Placement::ByHole => through("hole")?,
        })
    }

    /// Where a face was picked (assembly coordinates).
    fn bolt_pick_point(&self, r: &GeomRef) -> Option<Point3> {
        let GeomRef::Face {
            occurrence, hint, ..
        } = r
        else {
            return None;
        };
        let a = self.assembly()?;
        let local = Point3::from(*hint);
        Some(match a.find(*occurrence) {
            Some(o) => o.transform * local,
            None => local,
        })
    }

    /// The inputs the axes depend on, as text (found again only when it changes).
    fn bolt_key(d: &PartDialog) -> String {
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
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
            spread(d),
            choice(d, "Diameter"),
            d.bolt.auto_end,
            d.picks_of(end_key(d)).len(),
        )
    }

    /// What the preview depends on beyond the axes: the stack, the length, the fit.
    fn bolt_view_key(d: &PartDialog) -> String {
        format!(
            "{}|{:?}|{:?}|{:?}|{:?}|{:?}",
            d.bolt.key,
            d.bolt.head,
            d.bolt.nut,
            d.bolt.length,
            fit(d),
            d.bolt.axes
        )
    }

    /// Finds the axes, the plates along each and (when the termination is
    /// not the user's) the termination, when the panel's inputs changed; then
    /// what OK would place.
    pub(super) fn bolt_follow(&mut self) {
        // The stack asked for the Component Library window.
        if let Some(pick) = self
            .part_planned
            .as_mut()
            .filter(|d| is_bolted(d))
            .and_then(|d| d.bolt.library.take())
        {
            self.library_for_bolt(pick);
        }
        self.bolt_follow_pattern();
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return;
        };
        let key = Self::bolt_key(d);
        if key != d.bolt.key {
            self.bolt_find_axes();
        }
        let Some(d) = self.part_planned.as_ref() else {
            return;
        };
        let vkey = Self::bolt_view_key(d);
        if vkey != d.bolt.view_key {
            let view = self.bolt_view(d);
            if let Some(d) = self.part_planned.as_mut() {
                // The members' triangles are kept for the next change.
                if let Ok(v) = &view {
                    for g in &v.ghosts {
                        d.bolt.meshes.insert(g.file.clone(), g.mesh.clone());
                    }
                }
                d.bolt.view = Some(view);
                d.bolt.view_key = vkey;
                // A length that no longer works goes back to auto.
                if let (Some(l), Some(Ok(v))) = (d.bolt.length, &d.bolt.view)
                    && !v.lengths.iter().any(|x| (x - l).abs() < 1e-6)
                {
                    d.bolt.length = None;
                }
            }
        }
    }

    fn bolt_find_axes(&mut self) {
        let Some(d) = self.part_planned.as_ref() else {
            return;
        };
        let end_key = end_key(d);
        let user_end = d.picks_of(end_key).len() == 1 && !d.bolt.auto_end;
        let probe = holes(d).map_or(6.0, |(_, h)| {
            (h.clearance / 2.0 + 0.75 * h.major) / 2.0 + 1.0
        });
        let start = self.bolt_geom(d, "start");
        let spread = spread(d);
        let refs = start.as_ref().and_then(|s| self.bolt_references(d, s));
        // Where the first bolt enters: the termination is found along it.
        let entry = match (&start, &refs) {
            (Some(s), Some(r)) => Some(
                r.first()
                    .ok_or_else(|| "pick the reference".to_owned())
                    // The first bolt itself (a bolt circle's is off its centre).
                    .and_then(|r| bolted::entries(s, r, &spread))
                    .and_then(|v| v.first().copied().ok_or_else(|| "no bolt".to_owned())),
            ),
            _ => None,
        };
        // The components a bolt may pass (not the fasteners of connections
        // already made: a nut's washer would read as one more plate).
        let fasteners: Vec<u32> = self
            .assembly()
            .map(|a| {
                a.bolts
                    .iter()
                    .flat_map(|b| b.fasteners.iter().map(|f| f.occurrence))
                    .collect()
            })
            .unwrap_or_default();
        let mut parts = self.pick_parts();
        parts.retain(|p| !fasteners.contains(&p.top));
        let meshes: Vec<_> = parts.iter().map(|p| self.asm_mesh(p.component)).collect();
        let list: Vec<_> = parts
            .iter()
            .zip(&meshes)
            .filter_map(|(p, m)| m.as_ref().map(|m| (m.triangles(), p.place)))
            .collect();
        let index: Vec<usize> = (0..parts.len()).filter(|i| meshes[*i].is_some()).collect();
        let rows = |a: &bolted::Axis| -> (Vec<PlateRow>, Vec<bolted::Plate>) {
            let s = bolted::stack_along(&list, a, probe, STACK_TOL);
            let rows = s
                .iter()
                .map(|p| {
                    let part = &parts[index[p.part]];
                    PlateRow {
                        name: self
                            .occurrence_label_pub(part.top)
                            .map(|l| l.split(" · ").next().unwrap_or_default().to_owned())
                            .unwrap_or_default(),
                        thickness: p.to - p.from,
                        path: std::iter::once(part.top)
                            .chain(part.sub.iter().copied())
                            .collect(),
                        from: p.from,
                        solid: bolted::solid_on_axis(&list, a, p),
                        component: part.component,
                        face: self.face_name_pub(part, p.entry),
                        local: part.place.inverse() * (a.at + a.dir * p.from),
                    }
                })
                .collect();
            (rows, s)
        };
        let mut found_end = None;
        if let Some(Ok(a)) = &entry {
            let (_, s) = rows(a);
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
        let end = if user_end {
            d.refs.get(end_key).and_then(|v| v.first()).cloned()
        } else {
            found_end.clone()
        };
        let axes = match (entry, &start, &refs, end.and_then(|e| self.bolt_world(&e))) {
            (Some(Err(e)), ..) => Some(Err(e)),
            (Some(Ok(_)), Some(s), Some(r), Some(e)) => Some(bolted::axes(s, r, &spread, &e)),
            _ => None,
        };
        let plates: Vec<Vec<PlateRow>> = match &axes {
            Some(Ok(v)) => v.iter().map(|a| rows(a).0).collect(),
            _ => Vec::new(),
        };
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
        d.bolt.axes = axes;
        // With the found termination in: the same inputs next frame.
        d.bolt.key = Self::bolt_key(d);
    }

    /// The Component Library window's Use: the family goes into the stack.
    pub(super) fn bolt_use_family(&mut self, pick: &BoltPick, name: &str, category: &str) {
        let Some(d) = self.part_planned.as_mut().filter(|d| is_bolted(d)) else {
            self.file_status = Some(Err("Bolted Connection: the panel is closed".into()));
            return;
        };
        if let Err(e) = use_family(d, pick, name, category) {
            self.file_status = Some(Err(format!("Bolted Connection: {e}")));
        }
    }

    /// Follow Pattern (spec 6.2 §3.2): every hole of the first one's size on
    /// its face becomes a reference too (a bolt in each); found again when
    /// the first pick changes, and only the first stays when it is turned off.
    fn bolt_follow_pattern(&mut self) {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return;
        };
        let key = match placement(d) {
            Placement::ByHole => "hole",
            Placement::Concentric => "circle",
            _ => return,
        };
        let on = d.check(0, "Follow Pattern");
        let first = d.refs.get(key).and_then(|v| v.first()).cloned();
        let state = on.then(|| format!("{key}|{first:?}"));
        if state == d.bolt.followed {
            return;
        }
        let Some(first) = first else {
            if let Some(d) = self.part_planned.as_mut() {
                d.bolt.followed = state;
            }
            return;
        };
        let label = self.geom_label(&first);
        let mut all = vec![(label, first.clone())];
        if on {
            all.extend(self.bolt_same_holes(&first));
        }
        let Some(d) = self.part_planned.as_mut() else {
            return;
        };
        d.clear_ref(key);
        for (l, g) in all {
            d.add_ref(key, l, g);
        }
        d.bolt.followed = state;
    }

    /// The other holes of `first`'s size whose rims lie in the plane of its
    /// rim (a rim: its circle; a wall: the rim on the start plane), one edge
    /// per hole.
    fn bolt_same_holes(&self, first: &GeomRef) -> Vec<(String, GeomRef)> {
        let d = self.part_planned.as_ref();
        let plane = d.and_then(|d| self.bolt_geom(d, "start"));
        let (c0, n0, r0) = match (self.bolt_world(first), plane) {
            (Some(Geom::Circle { c, n, r }), _) => (c, n, r),
            (Some(Geom::Cylinder { p, d: ax, r }), Some(Geom::Plane { p: q, n })) => {
                let t = (q - p).dot(&n) / ax.dot(&n);
                (p + ax * t, n, r)
            }
            _ => return Vec::new(),
        };
        let mut out: Vec<(String, GeomRef)> = Vec::new();
        let mut centres = vec![c0];
        for part in self.pick_parts() {
            let Some(edges) = self.asm_edges_pub(part.component) else {
                continue;
            };
            for (e, pts) in edges.iter() {
                if pts.len() <= 2 {
                    continue;
                }
                let g = GeomRef::Edge {
                    occurrence: part.top,
                    sub: part.sub.clone(),
                    edge: e.clone(),
                };
                let Some(Geom::Circle { c, n, r }) = self.bolt_world(&g) else {
                    continue;
                };
                let same = (r - r0).abs() < 1e-3
                    && n.cross(&n0).norm() < 1e-3
                    && (c - c0).dot(&n0).abs() < 1e-3;
                if same && !centres.iter().any(|x| (x - c).norm() < 1e-3) {
                    centres.push(c);
                    out.push((self.geom_label(&g), g));
                }
            }
        }
        out
    }

    /// What OK would place: the plan's members as see-through triangles.
    fn bolt_view(&self, d: &PartDialog) -> Result<View, String> {
        let plan = self.bolted_plan_pub(d)?;
        let mut ghosts = Vec::new();
        for (file, nut_side, offset) in &plan.0 {
            let mesh = match d.bolt.meshes.get(file) {
                Some(m) => m.clone(),
                None => {
                    let p = self.part_file_pub(file)?;
                    let part = p
                        .documents()
                        .iter()
                        .find_map(|x| match &x.data {
                            crate::document::DocumentData::Part(part) => Some(part),
                            _ => None,
                        })
                        .ok_or("the member's file holds no part")?;
                    Arc::new(PickMesh::of(part))
                }
            };
            ghosts.push(Ghost {
                file: file.clone(),
                mesh,
                nut_side: *nut_side,
                offset: *offset,
            });
        }
        let mut v = plan.1;
        v.ghosts = ghosts;
        Ok(v)
    }

    /// What the active selector would take under the cursor (rule 19): an
    /// edge selector sees edges only, the termination faces only; whatever
    /// the route refuses is not offered.
    fn bolt_candidate(&self, pos: Pos2, rect: Rect) -> Option<(GeomRef, bolted::Route)> {
        let d = self.part_planned.as_ref().filter(|d| is_bolted(d))?;
        let active = d.active?;
        let straight = |p: &[Point3]| p.len() == 2;
        let round = |p: &[Point3]| p.len() > 2;
        let p = placement(d);
        let g = match active {
            "edge1" | "edge2" => self.geom_under_with(pos, rect, &straight, false),
            "end" | "blind" => self.face_under_pub(pos, rect),
            "point" => self
                .asm_vertex_under_pub(pos, rect)
                .or_else(|| self.geom_under_with(pos, rect, &round, false)),
            "circle" | "hole" => self.geom_under_with(pos, rect, &round, true),
            _ if p == Placement::Linear => self.geom_under_with(pos, rect, &|_| true, true),
            _ => self.geom_under_with(pos, rect, &round, true),
        }?;
        let what = self.bolt_clicked(&g)?;
        let edge1 = !d.picks_of("edge1").is_empty();
        let route = bolted::route(p, active, what, edge1)?;
        Some((g, route))
    }

    /// What the routing makes of `g`.
    fn bolt_clicked(&self, g: &GeomRef) -> Option<Clicked> {
        Some(match (g, self.bolt_world(g)?) {
            (GeomRef::Face { .. }, Geom::Plane { .. }) => Clicked::PlanarFace,
            (GeomRef::Face { .. }, Geom::Cylinder { .. }) => Clicked::RoundFace,
            (GeomRef::Edge { .. }, Geom::Circle { .. }) => Clicked::CircularEdge,
            (GeomRef::Edge { .. }, Geom::Axis { .. }) => Clicked::StraightEdge,
            (_, Geom::Point { .. } | Geom::Frame { .. }) => Clicked::Point,
            _ => return None,
        })
    }

    /// The smart selectors (spec 6.2 §3.2): a click in the view while the
    /// Bolted Connection panel is open. `true`: the click was used (or
    /// ignored: it was nothing the active selector takes).
    pub(super) fn bolt_click(&mut self, pos: Pos2, rect: Rect) -> bool {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return false;
        };
        // A + in the view: the library list for that place.
        if let Some((side, at, _)) = self
            .bolt_plus_spots(rect)
            .into_iter()
            .find(|s| s.2.distance(pos) <= GRIP_PX)
        {
            if let Some(d) = self.part_planned.as_mut() {
                d.bolt.menu = Some((side, at, pos));
            }
            return true;
        }
        if d.bolt.menu.is_some() {
            if let Some(d) = self.part_planned.as_mut() {
                d.bolt.menu = None;
            }
            return true;
        }
        if d.active.is_none() {
            return false;
        }
        if let Some((g, _)) = self.bolt_candidate(pos, rect) {
            self.bolt_take(g, Some((pos, rect)));
        }
        true
    }

    /// A face or an edge given to the open Bolted Connection (a click at `at`,
    /// or the selection it was started from): the smart selectors' route.
    pub(super) fn bolt_take(&mut self, g: GeomRef, at: Option<(Pos2, Rect)>) -> bool {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return false;
        };
        let Some(active) = d.active else {
            return false;
        };
        let geom = self.bolt_world(&g);
        let Some(what) = self.bolt_clicked(&g) else {
            let w = self.geom_label(&g);
            self.file_status = Some(Err(format!("Bolted Connection: {w} cannot be used here")));
            return true;
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
            match self.bolt_edge_face(&g, &geom, at) {
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
        let mut stay = false;
        for (k, (label, r)) in route.keys.iter().zip(labels) {
            let key = if *k == "start*" { "start" } else { k };
            if multi(d, key) {
                // A selector of several picks stays active for the next one
                // (the termination is found by itself; Next goes on).
                d.add_ref(key, label, r);
                d.set_active(key);
                stay = true;
            } else {
                d.set_ref(key, label, r);
            }
            if key == "end" || key == "blind" {
                d.bolt.auto_end = false;
            }
        }
        if !stay {
            d.activate_first_empty();
        }
        true
    }

    /// The planar face an edge bounds, for the start plane.
    fn bolt_edge_face(
        &self,
        g: &GeomRef,
        geom: &Option<Geom>,
        at: Option<(Pos2, Rect)>,
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
        if let Some((pos, rect)) = at
            && let Some(f) = self.face_under_pub(pos, rect)
            && matches!(&f, GeomRef::Face { name, .. } if edge.faces.contains(name))
            && plane_of(&f).is_some()
        {
            return Some(f);
        }
        let view = at
            .and_then(|(pos, rect)| self.camera.ray(pos, rect))
            .map(|(_, d)| d);
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
                let score = |n: &Vector3| match (geom, view) {
                    (Some(Geom::Circle { n: c, .. }), _) => n.dot(c).abs(),
                    (_, Some(v)) => -n.dot(&v),
                    _ => 0.0,
                };
                score(&a.1).total_cmp(&score(&b.1))
            })
            .map(|x| x.0)
    }

    /// The first bolt's axis and its stack's seats, when known.
    fn bolt_first(&self) -> Option<(bolted::Axis, Option<View>)> {
        let d = self.part_planned.as_ref().filter(|d| is_bolted(d))?;
        let a = *d.bolt.axes.as_ref()?.as_ref().ok()?.first()?;
        let v = d.bolt.view.as_ref().and_then(|v| v.as_ref().ok()).cloned();
        Some((a, v))
    }

    /// A direction square to `dir` that points right on the screen.
    fn bolt_screen_side(&self, dir: &Vector3) -> Vector3 {
        let (back, _) = self.camera.frame();
        let s = dir.cross(&back);
        if s.norm() > 1e-6 {
            s.normalize()
        } else {
            sides(dir).0
        }
    }

    /// The **+** of the view (spec 6.2 §3.5): each place a fastener can go,
    /// beside the first bolt's axis, on the screen.
    fn bolt_plus_spots(&self, rect: Rect) -> Vec<(Side, usize, Pos2)> {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return Vec::new();
        };
        let Some((a, view)) = self.bolt_first() else {
            return Vec::new();
        };
        let dn = holes(d).map_or(8.0, |h| h.1.major);
        let side = self.bolt_screen_side(&a.dir) * (1.4 * dn + 3.0);
        let mut v: Vec<(Side, usize, f64)> = Vec::new();
        let blind = kind(d) == BoltKind::Blind;
        let has_bolt = d.bolt.head.iter().any(|x| x.role == Role::Bolt);
        match &view {
            Some(w) => {
                // Under each head-side item; the bolt when there is none.
                for (k, h) in w.head_seats.iter().enumerate() {
                    v.push((Side::Head, k + 1, -h));
                }
                if !blind {
                    for (k, t) in w.nut_seats.iter().enumerate() {
                        v.push((Side::Nut, k, a.length + t));
                    }
                    v.push((Side::Nut, w.nut_seats.len(), a.length + w.nut_end));
                }
            }
            None => {
                let n = d.bolt.head.len();
                v.push((Side::Head, if has_bolt { n } else { 0 }, 0.0));
                if !blind {
                    v.push((Side::Nut, d.bolt.nut.len(), a.length));
                }
            }
        }
        v.into_iter()
            .filter_map(|(s, k, t)| {
                let p = a.at + a.dir * t + side;
                Some((s, k, self.camera.project(&p, rect)?))
            })
            .collect()
    }

    /// The bolt's end and the arrow's end on the screen (the length grip).
    fn bolt_grip_ends(&self, rect: Rect) -> Option<(Pos2, Pos2, f64)> {
        let (a, v) = self.bolt_first()?;
        let v = v?;
        let seat = a.at - a.dir * v.head_seats.first().copied().unwrap_or(0.0);
        let tip = seat + a.dir * v.length;
        let p = self.camera.project(&tip, rect)?;
        let q = self.camera.project(&(tip + a.dir * 1.0), rect)?;
        let px_per_mm = p.distance(q) as f64;
        let dir = (q - p).normalized();
        Some((p, p + dir * 34.0, px_per_mm))
    }

    /// The length arrow's drag (`true` while it is held: the view does not
    /// orbit or box-select).
    pub(super) fn bolt_grip(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
    ) -> bool {
        let Some(ends) = self.bolt_grip_ends(rect) else {
            return false;
        };
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return false;
        };
        let held = d.bolt.grip;
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
        {
            let (p, q, _) = ends;
            let near = seg_dist(press, p, q) <= GRIP_PX;
            let length = d
                .bolt
                .view
                .as_ref()
                .and_then(|v| v.as_ref().ok())
                .map(|v| v.length);
            if near && let (Some(l), Some(d)) = (length, self.part_planned.as_mut()) {
                d.bolt.grip = Some((l, press));
                return true;
            }
            return false;
        }
        let Some((l0, press)) = held else {
            return false;
        };
        if !response.dragged() {
            if let Some(d) = self.part_planned.as_mut() {
                d.bolt.grip = None;
            }
            return false;
        }
        let Some(now) = response.interact_pointer_pos() else {
            return true;
        };
        let (p, q, px_per_mm) = ends;
        let along = (now - press).dot((q - p).normalized()) as f64;
        let want = l0 + along / px_per_mm.max(1e-6);
        let lengths = d
            .bolt
            .view
            .as_ref()
            .and_then(|v| v.as_ref().ok())
            .map(|v| v.lengths.clone())
            .unwrap_or_default();
        let best = lengths
            .iter()
            .copied()
            .min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs()));
        if let (Some(l), Some(d)) = (best, self.part_planned.as_mut())
            && d.bolt.length != Some(l)
        {
            d.bolt.length = Some(l);
        }
        true
    }

    /// The library list opened from a **+** in the view.
    pub(super) fn bolt_view_menu(&mut self, ctx: &egui::Context) {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return;
        };
        let Some((side, at, pos)) = d.bolt.menu else {
            return;
        };
        let roles = slot_roles(d, side, at);
        let mut chosen = None;
        let mut close = false;
        egui::Area::new(egui::Id::new("bolt_view_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos + egui::vec2(10.0, 0.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(300.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Insert here").strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::glyph(ui, Glyph::Close).clicked() {
                                close = true;
                            }
                        });
                    });
                    chosen = library_list(d, ui, roles, ("view", side, at));
                });
            });
        if let Some(d) = self.part_planned.as_mut() {
            if let Some(item) = chosen {
                apply_act(d, Act::Add(item));
            } else if close {
                d.bolt.menu = None;
            }
        }
    }

    /// Bolted Connection in the view (rule 23): every pick in its selector's
    /// colour, what a click would take, Linear's distances, the bolt circle,
    /// each bolt's fasteners (the members themselves, see-through), the
    /// length arrow and the **+** where a fastener can go.
    pub(super) fn bolt_preview(&self, painter: &egui::Painter, rect: Rect, pointer: Option<Pos2>) {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_bolted(d)) else {
            return;
        };
        // The picks, each in its selector's colour.
        for key in [
            "start", "edge1", "edge2", "circle", "point", "hole", "end", "blind",
        ] {
            if sel_hidden(d, key) {
                continue;
            }
            let col = d.sel_color(key);
            for g in d.refs.get(key).into_iter().flatten() {
                self.draw_geom(painter, rect, g, col);
                if let Some(Geom::Point { p } | Geom::Frame { p, .. }) = self.bolt_world(g)
                    && let Some(s) = self.camera.project(&p, rect)
                {
                    painter.circle_filled(s, 4.5, col);
                }
            }
        }
        // What a click would take, in the active selector's colour.
        if d.bolt.grip.is_none()
            && d.bolt.menu.is_none()
            && let Some(p) = pointer
            && let Some((g, _)) = self.bolt_candidate(p, rect)
            && let Some(key) = d.active
        {
            let col = d.sel_color(key);
            self.draw_geom(painter, rect, &g, col.gamma_multiply(0.75));
        }
        let Some(Ok(axes)) = &d.bolt.axes else {
            return;
        };
        let Some((_, h)) = holes(d) else {
            return;
        };
        self.bolt_dimensions(painter, rect, d, axes);
        let view = d.bolt.view.as_ref().and_then(|v| v.as_ref().ok());
        for a in axes {
            match view {
                Some(v) => self.bolt_ghosts(painter, rect, a, v),
                None => self.bolt_schematic(painter, rect, a, &h, kind(d) == BoltKind::Blind),
            }
            // The axis, dashed.
            let tip = a.length + 1.5 * h.major;
            if let (Some(p), Some(q)) = (
                self.camera.project(&(a.at - a.dir * (2.0 * h.major)), rect),
                self.camera.project(&(a.at + a.dir * tip), rect),
            ) {
                let c = theme::canvas().hover_edge;
                painter.extend(egui::Shape::dashed_line(
                    &[p, q],
                    Stroke::new(1.0, c),
                    6.0,
                    4.0,
                ));
            }
        }
        // The length arrow on the first bolt.
        if let Some((p, q, _)) = self.bolt_grip_ends(rect) {
            let hot =
                d.bolt.grip.is_some() || pointer.is_some_and(|c| seg_dist(c, p, q) <= GRIP_PX);
            let c = if hot {
                theme::canvas().hover_edge
            } else {
                Color32::from_rgb(230, 170, 40)
            };
            arrow(painter, p, q, c);
            if let (true, Some(v)) = (hot, view) {
                painter.text(
                    q + egui::vec2(8.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    format!("{:.0} mm", v.length),
                    egui::FontId::proportional(13.0),
                    c,
                );
            }
        }
        // The + where a fastener can go.
        for (_, _, s) in self.bolt_plus_spots(rect) {
            let hot = pointer.is_some_and(|c| c.distance(s) <= GRIP_PX);
            let c = if hot {
                theme::canvas().hover_edge
            } else {
                Color32::from_rgb(70, 150, 230)
            };
            painter.circle_filled(s, 7.0, Color32::from_black_alpha(140));
            painter.circle_stroke(s, 7.0, Stroke::new(1.2, c));
            painter.line_segment(
                [s - egui::vec2(4.0, 0.0), s + egui::vec2(4.0, 0.0)],
                Stroke::new(1.6, c),
            );
            painter.line_segment(
                [s - egui::vec2(0.0, 4.0), s + egui::vec2(0.0, 4.0)],
                Stroke::new(1.6, c),
            );
        }
    }

    /// Linear's two distances as dimensions from each edge to the first bolt,
    /// the grid's and the bolt circle's bolts as small crosses.
    fn bolt_dimensions(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        d: &PartDialog,
        axes: &[bolted::Axis],
    ) {
        let Some(first) = axes.first() else {
            return;
        };
        let font = egui::FontId::proportional(13.0);
        match placement(d) {
            Placement::Linear => {
                for (key, label, row) in
                    [("edge1", "d1", "Distance 1"), ("edge2", "d2", "Distance 2")]
                {
                    let Some(Geom::Axis { p, d: u }) = self.bolt_geom(d, key) else {
                        continue;
                    };
                    let foot = p + u * (first.at - p).dot(&u);
                    let col = d.sel_color(key);
                    if let (Some(a), Some(b)) = (
                        self.camera.project(&foot, rect),
                        self.camera.project(&first.at, rect),
                    ) {
                        painter.line_segment([a, b], Stroke::new(1.2, col));
                        arrow_head(painter, b, a, col);
                        arrow_head(painter, a, b, col);
                        let value = d.num(0, row).unwrap_or(0.0);
                        let mid = a + (b - a) * 0.5;
                        painter.text(
                            mid + egui::vec2(0.0, -10.0),
                            egui::Align2::CENTER_CENTER,
                            format!("{label} = {value:.2}"),
                            font.clone(),
                            col,
                        );
                    }
                }
            }
            Placement::Circular => {
                let Spread::Circle { radius, .. } = spread(d) else {
                    return;
                };
                let n = -first.dir;
                let plane = Geom::Plane { p: first.at, n };
                let Some(Ok(centre)) = self
                    .bolt_geom(d, "circle")
                    .map(|g| bolted::entry(&plane, &bolted::Reference::Through(g)))
                else {
                    return;
                };
                let o = centre.at;
                let x = bolted::plane_x(&n);
                let y = n.cross(&x);
                let pts: Vec<Pos2> = (0..=72)
                    .filter_map(|k| {
                        let t = std::f64::consts::TAU * f64::from(k) / 72.0;
                        self.camera
                            .project(&(o + (x * t.cos() + y * t.sin()) * radius), rect)
                    })
                    .collect();
                let col = d.sel_color("circle");
                painter.extend(egui::Shape::dashed_line(
                    &pts,
                    Stroke::new(1.0, col),
                    5.0,
                    4.0,
                ));
                if let (Some(a), Some(b)) = (
                    self.camera.project(&o, rect),
                    self.camera.project(&first.at, rect),
                ) {
                    painter.line_segment([a, b], Stroke::new(1.0, col));
                    painter.text(
                        a + (b - a) * 0.5 + egui::vec2(0.0, -10.0),
                        egui::Align2::CENTER_CENTER,
                        format!("R {radius:.2}"),
                        font.clone(),
                        col,
                    );
                }
            }
            _ => {}
        }
        // Each bolt's entry.
        let col = theme::canvas().hover_edge;
        for a in axes {
            if let Some(s) = self.camera.project(&a.at, rect) {
                painter.line_segment(
                    [s - egui::vec2(5.0, 5.0), s + egui::vec2(5.0, 5.0)],
                    Stroke::new(1.5, col),
                );
                painter.line_segment(
                    [s - egui::vec2(5.0, -5.0), s + egui::vec2(5.0, -5.0)],
                    Stroke::new(1.5, col),
                );
            }
        }
    }

    /// The members of one bolt's stack at their seats, see-through.
    fn bolt_ghosts(&self, painter: &egui::Painter, rect: Rect, a: &bolted::Axis, v: &View) {
        let bottom = a.at + a.dir * a.length;
        for g in &v.ghosts {
            let (z, seat) = if g.nut_side {
                (a.dir, bottom + a.dir * g.offset)
            } else {
                (-a.dir, a.at - a.dir * g.offset)
            };
            let t = crate::components::fit::seat_transform(seat, z, 0.0, false);
            super::place::draw_ghost(painter, &self.camera, rect, &g.mesh, &t);
        }
    }

    /// Before the fasteners are known: the hole at both planes, a head and a
    /// nut outline.
    fn bolt_schematic(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        a: &bolted::Axis,
        h: &bolted::HoleSizes,
        blind: bool,
    ) {
        let color = theme::canvas().hover_edge;
        let thin = Stroke::new(1.0, color);
        let thick = Stroke::new(2.0, color);
        let (u, v) = sides(&a.dir);
        let point =
            |t: f64, r: f64, ang: f64| a.at + a.dir * t + (u * ang.cos() + v * ang.sin()) * r;
        let circle = |t: f64, r: f64, s: Stroke| {
            let pts: Vec<Pos2> = (0..=48)
                .filter_map(|k| {
                    let ang = std::f64::consts::TAU * f64::from(k) / 48.0;
                    self.camera.project(&point(t, r, ang), rect)
                })
                .collect();
            if pts.len() > 2 {
                painter.add(egui::Shape::line(pts, s));
            }
        };
        let dn = h.major;
        circle(0.0, h.clearance / 2.0, thin);
        circle(a.length, h.clearance / 2.0, thin);
        circle(0.0, 0.8 * dn, thick);
        circle(-0.65 * dn, 0.8 * dn, thick);
        if !blind {
            circle(a.length, 0.85 * dn, thick);
            circle(a.length + 0.8 * dn, 0.85 * dn, thick);
        }
    }
}

/// The screen distance from `p` to the segment `a`–`b`.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// An arrow from `a` to `b`.
fn arrow(painter: &egui::Painter, a: Pos2, b: Pos2, c: Color32) {
    painter.line_segment([a, b], Stroke::new(2.0, c));
    arrow_head(painter, a, b, c);
}

/// An arrow head at `b`, pointing away from `a`.
fn arrow_head(painter: &egui::Painter, a: Pos2, b: Pos2, c: Color32) {
    let d = (b - a).normalized();
    let n = egui::vec2(-d.y, d.x);
    painter.add(egui::Shape::convex_polygon(
        vec![b, b - d * 9.0 + n * 4.0, b - d * 9.0 - n * 4.0],
        c,
        Stroke::NONE,
    ));
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
        assert!(!row_hidden(&d, 0, "Count 2") && row_hidden(&d, 0, "Radius"));
        assert_eq!(spread(&d), Spread::One);
        d.set_num(0, "Count 1", 3.0);
        assert!(matches!(spread(&d), Spread::Grid { count: [3, 1], .. }));
        d.set_index(0, "Placement", Placement::Concentric.index());
        assert_eq!(shown(&d), vec!["start", "circle", "end"]);
        assert!(row_hidden(&d, 0, "Distance 2") && row_hidden(&d, 0, "Count 1"));
        assert!(multi(&d, "circle"));
        d.set_index(0, "Placement", Placement::Circular.index());
        assert_eq!(shown(&d), vec!["start", "circle", "end"]);
        assert!(!row_hidden(&d, 0, "Radius") && !multi(&d, "circle"));
        assert!(matches!(spread(&d), Spread::Circle { count: 4, .. }));
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

    /// Use case 11: what the stack still lacks is said in the panel; each
    /// place offers only what may go there.
    #[test]
    fn use_case_the_stack_says_what_is_missing() {
        let mut d = dialog();
        assert!(stack_problems(&d).is_empty());
        assert_eq!(slot_roles(&d, Side::Head, 0), &[Role::Bolt]);
        d.bolt.head.push(item(0, Role::Washer));
        assert_eq!(
            stack_problems(&d),
            vec!["Head side: add a bolt", "Nut side: add a nut (Through All)"]
        );
        d.bolt.head.insert(0, item(1, Role::Bolt));
        assert_eq!(slot_roles(&d, Side::Head, 0), &[Role::Washer]);
        assert_eq!(slot_roles(&d, Side::Nut, 0), &[Role::Washer, Role::Nut]);
        d.set_index(0, "Type", 1);
        assert!(stack_problems(&d).is_empty());
    }

    #[test]
    fn the_diameter_list_starts_at_m8() {
        let mut d = dialog();
        assert_eq!(choice(&d, "Thread").as_deref(), Some("ISO Metric profile"));
        assert_eq!(choice(&d, "Diameter").as_deref(), Some("M8"));
        assert_eq!(holes(&d).map(|h| h.1.clearance), Some(9.0));
        // The fit row changes the clearance hole (ISO 273 fine / coarse).
        d.set_index(0, "Hole Fit", 0);
        assert_eq!(holes(&d).map(|h| h.1.clearance), Some(8.4));
        d.set_index(0, "Hole Fit", 2);
        assert_eq!(holes(&d).map(|h| h.1.clearance), Some(10.0));
    }
}
