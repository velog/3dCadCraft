//! One selection model (`docs/spec/0.9a-selection-filters.md`): everything
//! is pickable by default, the user switches categories off ([`Filter`]),
//! chooses what a click on model geometry resolves to ([`Priority`],
//! Inventor's Select Priority), and inside a command only what the active
//! selector accepts *and* the user filter lets through can be picked
//! ([`effective`]). Pure data, no GUI: the viewport asks this module and
//! the Shift + right-click menu is built from [`shift_menu`].

use serde::{Deserialize, Serialize};

use super::flow::PickKind;

/// What a pickable thing is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Category {
    /// A face of a body.
    Face,
    /// A model edge.
    Edge,
    /// A named model vertex.
    Vertex,
    /// A whole feature (Features priority).
    Feature,
    /// A solid body (Bodies priority).
    SolidBody,
    /// A surface body (Bodies priority).
    SurfaceBody,
    /// A visible sketch (a click on one of its curves).
    Sketch,
    /// A point, curve or region of a sketch (command inputs).
    SketchEntity,
    /// A work plane.
    WorkPlane,
    /// A work axis.
    WorkAxis,
    /// A work point.
    WorkPoint,
    /// The origin planes and axes.
    Origin,
}

impl Category {
    /// Every category, in menu order.
    pub const ALL: [Category; 12] = [
        Category::Face,
        Category::Edge,
        Category::Vertex,
        Category::Feature,
        Category::SolidBody,
        Category::SurfaceBody,
        Category::Sketch,
        Category::SketchEntity,
        Category::WorkPlane,
        Category::WorkAxis,
        Category::WorkPoint,
        Category::Origin,
    ];

    /// The menu label (English, as the rest of the UI).
    pub fn label(self) -> &'static str {
        match self {
            Category::Face => "Faces",
            Category::Edge => "Edges",
            Category::Vertex => "Vertices",
            Category::Feature => "Features",
            Category::SolidBody => "Solid Bodies",
            Category::SurfaceBody => "Surface Bodies",
            Category::Sketch => "Sketches",
            Category::SketchEntity => "Sketch Entities",
            Category::WorkPlane => "Work Planes",
            Category::WorkAxis => "Work Axes",
            Category::WorkPoint => "Work Points",
            Category::Origin => "Origin",
        }
    }

    fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// A set of categories (a bit mask). Saved as the list of categories that
/// are switched **off**, so a category added later is on by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Filter(u16);

const ALL_BITS: u16 = (1 << Category::ALL.len()) - 1;

impl Default for Filter {
    fn default() -> Self {
        Self::all()
    }
}

impl Filter {
    /// Everything (Inventor's Select All, the default).
    pub const fn all() -> Self {
        Self(ALL_BITS)
    }

    /// Nothing.
    pub const fn none() -> Self {
        Self(0)
    }

    /// Exactly these categories.
    pub fn of(cats: &[Category]) -> Self {
        Self(cats.iter().fold(0, |m, c| m | c.bit()))
    }

    /// Whether `c` passes.
    pub fn has(self, c: Category) -> bool {
        self.0 & c.bit() != 0
    }

    /// Switches `c` on or off.
    pub fn set(&mut self, c: Category, on: bool) {
        if on {
            self.0 |= c.bit();
        } else {
            self.0 &= !c.bit();
        }
    }

    /// Both filters.
    pub fn and(self, o: Filter) -> Filter {
        Filter(self.0 & o.0)
    }

    /// Nothing passes.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Everything passes.
    pub fn is_all(self) -> bool {
        self.0 == ALL_BITS
    }

    /// The categories that pass, in menu order.
    pub fn categories(self) -> Vec<Category> {
        Category::ALL.into_iter().filter(|c| self.has(*c)).collect()
    }
}

impl Serialize for Filter {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let off: Vec<Category> = Category::ALL
            .into_iter()
            .filter(|c| !self.has(*c))
            .collect();
        off.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Filter {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let off = Vec::<Category>::deserialize(d)?;
        let mut f = Filter::all();
        for c in off {
            f.set(c, false);
        }
        Ok(f)
    }
}

/// What a click on a face, an edge or a vertex selects (Inventor: Select
/// Priority in the Select drop-down of the part environment).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Priority {
    /// The face, edge or vertex itself (the default).
    #[default]
    FacesEdges,
    /// The feature that made it.
    Features,
    /// Its body.
    Bodies,
    /// Sketches win over model geometry at the same spot.
    SketchFeatures,
}

impl Priority {
    /// Every priority, in menu order.
    pub const ALL: [Priority; 4] = [
        Priority::FacesEdges,
        Priority::Features,
        Priority::Bodies,
        Priority::SketchFeatures,
    ];

    /// Inventor's name.
    pub fn label(self) -> &'static str {
        match self {
            Priority::FacesEdges => "Select Faces and Edges",
            Priority::Features => "Select Features",
            Priority::Bodies => "Select Bodies",
            Priority::SketchFeatures => "Select Sketch Features",
        }
    }
}

/// How a selection box decides what it takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BoxMode {
    /// Left → right: window; right → left: crossing (Inventor).
    #[default]
    ByDirection,
    /// Always window (only what is fully inside).
    Window,
    /// Always crossing (also what it touches).
    Crossing,
}

impl BoxMode {
    /// Every mode, in menu order.
    pub const ALL: [BoxMode; 3] = [BoxMode::ByDirection, BoxMode::Window, BoxMode::Crossing];

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            BoxMode::ByDirection => "Window / Crossing by Direction",
            BoxMode::Window => "Always Window",
            BoxMode::Crossing => "Always Crossing",
        }
    }

    /// Whether a box dragged from x `a` to x `b` is a window.
    pub fn is_window(self, a: f32, b: f32) -> bool {
        match self {
            BoxMode::ByDirection => b >= a,
            BoxMode::Window => true,
            BoxMode::Crossing => false,
        }
    }
}

/// The persisted selection settings (`AppOptions::selection`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectSettings {
    /// Select Priority.
    pub priority: Priority,
    /// The user filter (categories switched off are saved; everything else
    /// is on).
    pub filter: Filter,
    /// How a box selects.
    pub box_mode: BoxMode,
    /// Enable Prehighlight.
    pub prehighlight: bool,
}

impl Default for SelectSettings {
    fn default() -> Self {
        Self {
            priority: Priority::default(),
            filter: Filter::all(),
            box_mode: BoxMode::default(),
            prehighlight: true,
        }
    }
}

/// The categories a selector kind can be satisfied by.
pub fn kind_categories(k: PickKind) -> &'static [Category] {
    use Category as C;
    match k {
        PickKind::Face | PickKind::PlanarFace => &[C::Face],
        PickKind::Plane => &[C::WorkPlane, C::Origin],
        PickKind::StraightEdge | PickKind::Edge => &[C::Edge],
        PickKind::Axis => &[C::WorkAxis, C::Origin],
        PickKind::Sketch => &[C::Sketch],
        PickKind::Body => &[C::SolidBody, C::SurfaceBody],
        PickKind::Region
        | PickKind::SketchCurve
        | PickKind::SketchPoint
        | PickKind::SectionPoint => &[C::SketchEntity],
        PickKind::Point => &[C::Vertex, C::WorkPoint],
    }
}

/// The categories some selector kinds accept together.
pub fn kinds_filter(kinds: &[PickKind]) -> Filter {
    let mut f = Filter::none();
    for k in kinds {
        for c in kind_categories(*k) {
            f.set(*c, true);
        }
    }
    f
}

/// What may be picked this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effective {
    /// What passes.
    pub filter: Filter,
    /// The user filter hid everything the selector takes, so it was ignored
    /// (the status bar says so).
    pub overridden: bool,
}

/// The selector's kinds (`None`: no command, the idle view) ∩ the user
/// filter. An empty intersection inside a command falls back to the
/// selector's kinds alone, so a filter never locks a command up.
pub fn effective(selector: Option<&[PickKind]>, user: Filter) -> Effective {
    match selector {
        None => Effective {
            filter: user,
            overridden: false,
        },
        Some(kinds) => {
            let sel = kinds_filter(kinds);
            let both = sel.and(user);
            if both.is_empty() && !sel.is_empty() {
                Effective {
                    filter: sel,
                    overridden: true,
                }
            } else {
                Effective {
                    filter: both,
                    overridden: false,
                }
            }
        }
    }
}

/// What a raw pick (`raw`: the geometry found under the cursor) selects
/// under `priority`, or `None` when the filter hides it. `surface`: the
/// geometry belongs to a surface body. The raw category must pass; faces,
/// edges and vertices then resolve to their feature or body when that
/// priority is on and its category passes, otherwise to themselves.
pub fn resolve(raw: Category, priority: Priority, f: Filter, surface: bool) -> Option<Category> {
    if !f.has(raw) {
        return None;
    }
    let model = matches!(raw, Category::Face | Category::Edge | Category::Vertex);
    if model {
        match priority {
            Priority::Features if f.has(Category::Feature) => return Some(Category::Feature),
            Priority::Bodies => {
                let b = if surface {
                    Category::SurfaceBody
                } else {
                    Category::SolidBody
                };
                return f.has(b).then_some(b);
            }
            _ => {}
        }
    }
    Some(raw)
}

/// Pick order under the cursor (smaller first): work point > vertex >
/// work axis > sketch > edge > face > work plane > origin (`0-ui` §3.2);
/// with Sketch Features priority sketches come first.
pub fn rank(c: Category, priority: Priority) -> u8 {
    if priority == Priority::SketchFeatures
        && matches!(c, Category::Sketch | Category::SketchEntity)
    {
        return 0;
    }
    match c {
        Category::WorkPoint => 1,
        Category::Vertex => 2,
        Category::WorkAxis => 3,
        Category::SketchEntity | Category::Sketch => 4,
        Category::Edge => 5,
        Category::Face | Category::Feature | Category::SolidBody | Category::SurfaceBody => 6,
        Category::WorkPlane => 7,
        Category::Origin => 8,
    }
}

/// One thing under the cursor: its category and its order within the
/// category (screen distance or depth, nearest first).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    /// Its category.
    pub cat: Category,
    /// Screen distance or depth (smaller first).
    pub dist: f32,
    /// On a surface body (for Bodies priority).
    pub surface: bool,
}

/// The candidates that pass, resolved by `priority` and in pick order
/// (stable for equal rank and distance). Returns `(index, resolved)`.
pub fn filter_candidates(
    cands: &[Candidate],
    priority: Priority,
    f: Filter,
) -> Vec<(usize, Category)> {
    let mut out: Vec<(usize, Category)> = cands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| resolve(c.cat, priority, f, c.surface).map(|r| (i, r)))
        .collect();
    out.sort_by(|a, b| {
        let (ca, cb) = (&cands[a.0], &cands[b.0]);
        rank(ca.cat, priority)
            .cmp(&rank(cb.cat, priority))
            .then(ca.dist.total_cmp(&cb.dist))
    });
    out
}

/// What a selection box collects under a priority and a filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoxTakes {
    /// Faces (Faces and Edges / Sketch Features priority).
    pub faces: bool,
    /// Edges (same priorities).
    pub edges: bool,
    /// Named vertices (same priorities).
    pub vertices: bool,
    /// Whole features whose faces are inside (Features priority).
    pub features: bool,
    /// Solid bodies (Bodies priority).
    pub solids: bool,
    /// Surface bodies (Bodies priority).
    pub surfaces: bool,
    /// Visible sketches (any priority).
    pub sketches: bool,
}

/// What a box takes: the model categories that pass the filter, resolved
/// by the priority as a click would (`resolve`), plus sketches.
pub fn box_takes(priority: Priority, f: Filter) -> BoxTakes {
    let model = matches!(priority, Priority::FacesEdges | Priority::SketchFeatures);
    let via_faces = |c| f.has(Category::Face) && resolve(Category::Face, priority, f, c).is_some();
    BoxTakes {
        faces: model && f.has(Category::Face),
        edges: model && f.has(Category::Edge),
        vertices: model && f.has(Category::Vertex),
        features: priority == Priority::Features
            && resolve(Category::Face, priority, f, false) == Some(Category::Feature),
        solids: priority == Priority::Bodies && via_faces(false),
        surfaces: priority == Priority::Bodies && via_faces(true),
        sketches: f.has(Category::Sketch),
    }
}

/// Where the Shift + right-click menu opens.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuContext {
    /// The idle part view.
    Idle {
        /// Something is selected.
        selection: bool,
        /// The last selected item is an edge or a face (Tangencies).
        tangent_source: bool,
        /// An edge is selected or a face is under the cursor (Loop).
        loop_source: bool,
        /// Previous Selection has a set.
        previous: bool,
        /// More than one candidate under the cursor (Select Other).
        overlapping: bool,
    },
    /// A command is open; its active selector takes `kinds`.
    Command {
        /// The active selector's kinds.
        kinds: Vec<PickKind>,
        /// More than one candidate under the cursor.
        overlapping: bool,
    },
    /// Editing a sketch.
    Sketch {
        /// Something is selected in the sketch.
        selection: bool,
    },
}

/// An entry of the Shift + right-click menu.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    /// Cycle through the candidates under the cursor.
    SelectOther,
    /// The loop of the selected edge (or of the face under the cursor).
    SelectLoop,
    /// The tangent chain of the selected edge or face.
    SelectTangencies,
    /// Everything not selected.
    SelectInvert,
    /// Everything that passes the filter.
    SelectAll,
    /// The set before the last change.
    PreviousSelection,
    /// Nothing selected.
    ClearSelection,
    /// The box mode radio group.
    Box(BoxMode),
    /// A filter check box.
    Filter(Category),
    /// "Select All Filters" (back to everything).
    AllFilters,
    /// A priority radio.
    Priority(Priority),
    /// Enable Prehighlight.
    Prehighlight,
}

/// The entries of the Shift + right-click menu with whether each is
/// enabled, for its context. Inside a command only what the active
/// selector can take is offered as a filter.
pub fn shift_menu(ctx: &MenuContext) -> Vec<(MenuEntry, bool)> {
    let boxes = BoxMode::ALL.map(|b| (MenuEntry::Box(b), true));
    let mut v = Vec::new();
    match ctx {
        MenuContext::Idle {
            selection,
            tangent_source,
            loop_source,
            previous,
            overlapping,
        } => {
            v.push((MenuEntry::SelectOther, *overlapping));
            v.push((MenuEntry::SelectLoop, *loop_source));
            v.push((MenuEntry::SelectTangencies, *tangent_source));
            v.push((MenuEntry::SelectInvert, true));
            v.push((MenuEntry::SelectAll, true));
            v.push((MenuEntry::PreviousSelection, *previous));
            v.push((MenuEntry::ClearSelection, *selection));
            v.extend(boxes);
            v.extend(Category::ALL.map(|c| (MenuEntry::Filter(c), true)));
            v.push((MenuEntry::AllFilters, true));
            v.extend(Priority::ALL.map(|p| (MenuEntry::Priority(p), true)));
        }
        MenuContext::Command { kinds, overlapping } => {
            v.push((MenuEntry::SelectOther, *overlapping));
            v.extend(boxes);
            let takes = kinds_filter(kinds);
            v.extend(
                takes
                    .categories()
                    .into_iter()
                    .map(|c| (MenuEntry::Filter(c), true)),
            );
            if !takes.is_empty() {
                v.push((MenuEntry::AllFilters, true));
            }
        }
        MenuContext::Sketch { selection } => {
            v.push((MenuEntry::SelectAll, true));
            v.push((MenuEntry::SelectInvert, true));
            v.push((MenuEntry::ClearSelection, *selection));
            v.extend(boxes);
        }
    }
    v.push((MenuEntry::Prehighlight, true));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use Category as C;

    #[test]
    fn default_is_everything_and_roundtrips() {
        let f = Filter::default();
        assert!(f.is_all());
        assert!(C::ALL.iter().all(|c| f.has(*c)));
        let mut g = f;
        g.set(C::Edge, false);
        g.set(C::Origin, false);
        let json = serde_json::to_string(&g).unwrap_or_default();
        assert_eq!(json, r#"["Edge","Origin"]"#);
        let back: Filter = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(back, g);
        let s = SelectSettings::default();
        let txt = serde_json::to_string(&s).unwrap_or_default();
        let s2: SelectSettings = serde_json::from_str(&txt).unwrap_or_default();
        assert_eq!(s, s2);
        // Missing keys keep their defaults.
        let s3: SelectSettings = serde_json::from_str("{}").unwrap_or_default();
        assert_eq!(s3, SelectSettings::default());
    }

    /// Selector kinds × user filter: at least five combinations.
    #[test]
    fn selector_kinds_intersect_the_user_filter() {
        let all = Filter::all();
        // 1. Idle: the user filter alone.
        let mut no_edges = all;
        no_edges.set(C::Edge, false);
        assert_eq!(effective(None, no_edges).filter, no_edges);
        // 2. Faces selector, everything allowed: faces only.
        let e = effective(Some(&[PickKind::Face]), all);
        assert_eq!(e.filter, Filter::of(&[C::Face]));
        assert!(!e.overridden);
        // 3. Pull Direction (planar face, plane, straight edge, axis) without
        // edges: faces, planes, axes, origin.
        let pull = [
            PickKind::PlanarFace,
            PickKind::Plane,
            PickKind::StraightEdge,
            PickKind::Axis,
        ];
        let e = effective(Some(&pull), no_edges);
        assert!(e.filter.has(C::Face) && e.filter.has(C::WorkPlane) && e.filter.has(C::WorkAxis));
        assert!(!e.filter.has(C::Edge) && !e.filter.has(C::Vertex));
        // 4. Plane selector, origin hidden: work planes only.
        let mut no_origin = all;
        no_origin.set(C::Origin, false);
        let e = effective(Some(&[PickKind::Plane]), no_origin);
        assert_eq!(e.filter, Filter::of(&[C::WorkPlane]));
        // 5. Edge selector with edges filtered out: the selector wins and
        // the override is reported.
        let e = effective(Some(&[PickKind::Edge]), no_edges);
        assert_eq!(e.filter, Filter::of(&[C::Edge]));
        assert!(e.overridden);
        // 6. Body selector, surface bodies off: solids only.
        let mut no_surf = all;
        no_surf.set(C::SurfaceBody, false);
        let e = effective(Some(&[PickKind::Body]), no_surf);
        assert_eq!(e.filter, Filter::of(&[C::SolidBody]));
        // 7. Hole (sketch point, edge, planar face), only faces allowed.
        let e = effective(
            Some(&[PickKind::SketchPoint, PickKind::Edge, PickKind::PlanarFace]),
            Filter::of(&[C::Face]),
        );
        assert_eq!(e.filter, Filter::of(&[C::Face]));
        // 8. A command with no active selector picks nothing.
        let e = effective(Some(&[]), all);
        assert!(e.filter.is_empty() && !e.overridden);
    }

    #[test]
    fn priority_resolves_model_picks() {
        let all = Filter::all();
        assert_eq!(
            resolve(C::Face, Priority::FacesEdges, all, false),
            Some(C::Face)
        );
        assert_eq!(
            resolve(C::Edge, Priority::Features, all, false),
            Some(C::Feature)
        );
        assert_eq!(
            resolve(C::Vertex, Priority::Bodies, all, false),
            Some(C::SolidBody)
        );
        assert_eq!(
            resolve(C::Face, Priority::Bodies, all, true),
            Some(C::SurfaceBody)
        );
        // Work features and sketches stay themselves.
        assert_eq!(
            resolve(C::WorkPlane, Priority::Features, all, false),
            Some(C::WorkPlane)
        );
        assert_eq!(
            resolve(C::Sketch, Priority::Bodies, all, false),
            Some(C::Sketch)
        );
        // Features off under Features priority: the face itself.
        let mut f = all;
        f.set(C::Feature, false);
        assert_eq!(
            resolve(C::Face, Priority::Features, f, false),
            Some(C::Face)
        );
        // Surface bodies off under Bodies priority: nothing.
        let mut f = all;
        f.set(C::SurfaceBody, false);
        assert_eq!(resolve(C::Face, Priority::Bodies, f, true), None);
        // The raw category filtered: nothing.
        let mut f = all;
        f.set(C::Edge, false);
        assert_eq!(resolve(C::Edge, Priority::FacesEdges, f, false), None);
    }

    #[test]
    fn pick_order_and_select_other_over_mixed_categories() {
        let c = |cat, dist| Candidate {
            cat,
            dist,
            surface: false,
        };
        let cands = [
            c(C::Origin, 0.0),
            c(C::Face, 10.0),
            c(C::Face, 5.0),
            c(C::Edge, 3.0),
            c(C::Sketch, 2.0),
            c(C::WorkPlane, 1.0),
            c(C::Vertex, 4.0),
            c(C::WorkPoint, 6.0),
            c(C::WorkAxis, 1.0),
        ];
        let order: Vec<Category> = filter_candidates(&cands, Priority::FacesEdges, Filter::all())
            .into_iter()
            .map(|(i, _)| cands[i].cat)
            .collect();
        assert_eq!(
            order,
            vec![
                C::WorkPoint,
                C::Vertex,
                C::WorkAxis,
                C::Sketch,
                C::Edge,
                C::Face,
                C::Face,
                C::WorkPlane,
                C::Origin
            ]
        );
        // Nearest face first.
        let faces: Vec<f32> = filter_candidates(&cands, Priority::FacesEdges, Filter::all())
            .into_iter()
            .filter(|(i, _)| cands[*i].cat == C::Face)
            .map(|(i, _)| cands[i].dist)
            .collect();
        assert_eq!(faces, vec![5.0, 10.0]);
        // Sketch Features priority: the sketch first.
        let first = filter_candidates(&cands, Priority::SketchFeatures, Filter::all())[0];
        assert_eq!(cands[first.0].cat, C::Sketch);
        // Filters remove categories from the cycle; Features priority
        // resolves the model picks.
        let f = Filter::of(&[C::Face, C::Edge, C::Feature, C::Origin]);
        let r: Vec<Category> = filter_candidates(&cands, Priority::Features, f)
            .into_iter()
            .map(|(_, r)| r)
            .collect();
        assert_eq!(r, vec![C::Feature, C::Feature, C::Feature, C::Origin]);
    }

    /// Box selection honours the filter and the priority.
    #[test]
    fn box_takes_follow_filter_and_priority() {
        let all = Filter::all();
        let t = box_takes(Priority::FacesEdges, all);
        assert!(t.faces && t.edges && t.vertices && t.sketches);
        assert!(!t.features && !t.solids);
        let mut f = all;
        f.set(C::Edge, false);
        f.set(C::Sketch, false);
        let t = box_takes(Priority::FacesEdges, f);
        assert!(t.faces && !t.edges && !t.sketches && t.vertices);
        let t = box_takes(Priority::Features, all);
        assert!(t.features && !t.faces && !t.edges);
        // Features off under Features priority: nothing by faces.
        let mut f = all;
        f.set(C::Feature, false);
        assert!(!box_takes(Priority::Features, f).features);
        let mut f = all;
        f.set(C::SurfaceBody, false);
        let t = box_takes(Priority::Bodies, f);
        assert!(t.solids && !t.surfaces && !t.faces);
        assert_eq!(
            box_takes(Priority::Bodies, Filter::none()),
            BoxTakes::default()
        );
    }

    #[test]
    fn box_modes() {
        assert!(BoxMode::ByDirection.is_window(10.0, 20.0));
        assert!(!BoxMode::ByDirection.is_window(20.0, 10.0));
        assert!(BoxMode::Window.is_window(20.0, 10.0));
        assert!(!BoxMode::Crossing.is_window(10.0, 20.0));
    }

    #[test]
    fn shift_menu_follows_the_context() {
        let idle = shift_menu(&MenuContext::Idle {
            selection: false,
            tangent_source: false,
            loop_source: true,
            previous: false,
            overlapping: true,
        });
        let has =
            |v: &[(MenuEntry, bool)], e: &MenuEntry| v.iter().find(|(x, _)| x == e).map(|x| x.1);
        assert_eq!(has(&idle, &MenuEntry::SelectOther), Some(true));
        assert_eq!(has(&idle, &MenuEntry::SelectLoop), Some(true));
        assert_eq!(has(&idle, &MenuEntry::SelectTangencies), Some(false));
        assert_eq!(has(&idle, &MenuEntry::ClearSelection), Some(false));
        assert_eq!(has(&idle, &MenuEntry::PreviousSelection), Some(false));
        assert_eq!(
            has(&idle, &MenuEntry::Priority(Priority::Bodies)),
            Some(true)
        );
        assert!(
            C::ALL
                .iter()
                .all(|c| has(&idle, &MenuEntry::Filter(*c)).is_some())
        );
        assert_eq!(has(&idle, &MenuEntry::Box(BoxMode::Crossing)), Some(true));
        // In a command: only the selector's categories, no priorities, no
        // selection-set tools.
        let cmd = shift_menu(&MenuContext::Command {
            kinds: vec![PickKind::Edge, PickKind::Axis],
            overlapping: false,
        });
        let filters: Vec<Category> = cmd
            .iter()
            .filter_map(|(e, _)| match e {
                MenuEntry::Filter(c) => Some(*c),
                _ => None,
            })
            .collect();
        assert_eq!(filters, vec![C::Edge, C::WorkAxis, C::Origin]);
        assert_eq!(has(&cmd, &MenuEntry::SelectOther), Some(false));
        assert!(has(&cmd, &MenuEntry::SelectAll).is_none());
        assert!(has(&cmd, &MenuEntry::Priority(Priority::Features)).is_none());
        assert_eq!(has(&cmd, &MenuEntry::Prehighlight), Some(true));
        // In a sketch: the sketch's own set tools, no 3D filters.
        let sk = shift_menu(&MenuContext::Sketch { selection: true });
        assert_eq!(has(&sk, &MenuEntry::SelectAll), Some(true));
        assert_eq!(has(&sk, &MenuEntry::ClearSelection), Some(true));
        assert!(sk.iter().all(|(e, _)| !matches!(e, MenuEntry::Filter(_))));
        assert!(has(&sk, &MenuEntry::SelectOther).is_none());
    }
}
