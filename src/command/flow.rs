//! The selector flow of a command dialog (`docs/spec/0-ui.md` §2.9, §4.1):
//! which selector is active, where a pick, **Continue**, **Back** and
//! **Enter** move it, and what each selector accepts. Pure data, no GUI:
//! the dialog rebuilds a [`Flow`] from its definition every frame (the
//! counts come from the feature being defined) and keeps only the active
//! selector's key.
//!
//! Rules (Inventor's property panels and dialogs):
//! - one selector is active and takes the clicks in the view;
//! - after a valid pick a single-pick selector moves on by itself to the
//!   next selector (the next empty one first);
//! - Continue goes to the first required selector that is still empty, or
//!   else to the next selector; Back goes to the previous one;
//! - Enter is Continue while a required selector is empty, otherwise OK.

/// What a selector can take from the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PickKind {
    /// Any model face.
    Face,
    /// A planar model face.
    PlanarFace,
    /// A work plane or an origin plane.
    Plane,
    /// A straight model edge.
    StraightEdge,
    /// Any model edge.
    Edge,
    /// A work axis or an origin axis.
    Axis,
    /// A sketch (browser or view).
    Sketch,
    /// A solid body.
    Body,
    /// A closed region of a sketch (a profile).
    Region,
    /// A sketch curve (its connected chain).
    SketchCurve,
    /// A sketch point.
    SketchPoint,
    /// A point on a loft section's outline (Transition: Map Point).
    SectionPoint,
    /// A surface body (Extrude / Revolve with Output Surface).
    Surface,
    /// A model vertex or a work point.
    Point,
}

/// One selector row of a command dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct Selector {
    /// Stable key (the dialog stores the active one).
    pub key: &'static str,
    /// The row's name (Inventor's, English).
    pub label: &'static str,
    /// What the active selector asks for (status bar and cursor hint).
    pub prompt: &'static str,
    /// The kinds it accepts.
    pub accepts: &'static [PickKind],
    /// One item only (a direction, a plane): a pick moves on.
    pub single: bool,
    /// The command cannot finish while it is empty.
    pub required: bool,
    /// How many items it holds.
    pub count: usize,
    /// Its colour: the dot in the row and its highlight in the view.
    pub color: [u8; 3],
}

impl Selector {
    /// Whether it holds anything.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Whether it accepts `k`.
    pub fn takes(&self, k: PickKind) -> bool {
        self.accepts.contains(&k)
    }
}

/// What Enter does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnterAction {
    /// A required selector is still empty: it became active.
    Continue,
    /// Everything required is there: create the feature.
    Ok,
}

/// An entry of the right-click menu of an open command.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    /// OK (enabled when [`Flow::ready`]).
    Ok,
    /// Apply and create new (+).
    Apply,
    /// Continue to the next input.
    Continue,
    /// Back to the previous selector.
    Back,
    /// Cancel (Esc).
    Cancel,
    /// Repeat the last command.
    Repeat,
}

/// The selectors of an open command and the active one.
#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    /// The selectors, top to bottom.
    pub selectors: Vec<Selector>,
    /// The active one (index), if any.
    pub active: Option<usize>,
}

impl Flow {
    /// The flow with the selector `active` active; an unknown or missing
    /// key activates the first empty required selector, or the first one.
    pub fn new(selectors: Vec<Selector>, active: Option<&str>) -> Self {
        let mut f = Self {
            selectors,
            active: None,
        };
        f.active = active
            .and_then(|k| f.index(k))
            .or_else(|| f.first_missing())
            .or(if f.selectors.is_empty() {
                None
            } else {
                Some(0)
            });
        f
    }

    fn index(&self, key: &str) -> Option<usize> {
        self.selectors.iter().position(|s| s.key == key)
    }

    /// The first required selector that is still empty.
    pub fn first_missing(&self) -> Option<usize> {
        self.selectors
            .iter()
            .position(|s| s.required && s.is_empty())
    }

    /// The active selector.
    pub fn current(&self) -> Option<&Selector> {
        self.active.and_then(|i| self.selectors.get(i))
    }

    /// The active selector's key.
    pub fn active_key(&self) -> Option<&'static str> {
        self.current().map(|s| s.key)
    }

    /// Makes the selector `key` active (a click on its row).
    pub fn activate(&mut self, key: &str) {
        if let Some(i) = self.index(key) {
            self.active = Some(i);
        }
    }

    /// Whether every required selector holds something.
    pub fn ready(&self) -> bool {
        self.first_missing().is_none()
    }

    /// Whether the active selector takes `k` (prehighlight shows only
    /// those).
    pub fn accepts(&self, k: PickKind) -> bool {
        self.current().is_some_and(|s| s.takes(k))
    }

    /// The active selector's prompt ("" when none is active).
    pub fn prompt(&self) -> &'static str {
        self.current().map_or("", |s| s.prompt)
    }

    /// A valid pick went into the active selector (whose count the caller
    /// has already updated): a single-pick selector moves on to the next
    /// empty selector after it, or else the next one. Returns whether the
    /// active selector changed.
    pub fn picked(&mut self) -> bool {
        let Some(i) = self.active else { return false };
        if !self.selectors.get(i).is_some_and(|s| s.single) {
            return false;
        }
        let after = (i + 1..self.selectors.len()).find(|&j| self.selectors[j].is_empty());
        match after.or((i + 1 < self.selectors.len()).then_some(i + 1)) {
            Some(j) => {
                self.active = Some(j);
                true
            }
            None => false,
        }
    }

    /// Continue: the first required empty selector (when it is not the
    /// active one), or else the next selector. Returns whether it moved.
    pub fn continue_(&mut self) -> bool {
        if let Some(m) = self.first_missing()
            && self.active != Some(m)
        {
            self.active = Some(m);
            return true;
        }
        match self.active {
            Some(i) if i + 1 < self.selectors.len() => {
                self.active = Some(i + 1);
                true
            }
            None if !self.selectors.is_empty() => {
                self.active = Some(0);
                true
            }
            _ => false,
        }
    }

    /// Back: the previous selector. Returns whether it moved.
    pub fn back(&mut self) -> bool {
        match self.active {
            Some(i) if i > 0 => {
                self.active = Some(i - 1);
                true
            }
            _ => false,
        }
    }

    /// Enter: Continue while a required selector is empty, else OK.
    pub fn enter(&mut self) -> EnterAction {
        if let Some(m) = self.first_missing() {
            self.active = Some(m);
            EnterAction::Continue
        } else {
            EnterAction::Ok
        }
    }

    /// The selector `key` was cleared (`x`): it becomes active, so the next
    /// clicks refill it.
    pub fn cleared(&mut self, key: &str) {
        if let Some(i) = self.index(key) {
            self.selectors[i].count = 0;
            self.active = Some(i);
        }
    }

    /// The standard entries of the right-click menu with whether each is
    /// enabled (`apply`: the command has Apply; `repeat`: a last command).
    pub fn menu(&self, apply: bool, repeat: bool) -> Vec<(MenuItem, bool)> {
        let next = match self.active {
            Some(i) => i + 1 < self.selectors.len() || self.first_missing().is_some_and(|m| m != i),
            None => !self.selectors.is_empty(),
        };
        let mut v = vec![
            (MenuItem::Ok, self.ready()),
            (MenuItem::Continue, next),
            (MenuItem::Back, self.active.is_some_and(|i| i > 0)),
        ];
        if apply {
            v.insert(1, (MenuItem::Apply, self.ready()));
        }
        v.push((MenuItem::Cancel, true));
        if repeat {
            v.push((MenuItem::Repeat, true));
        }
        v
    }
}

/// Colours of the selectors, in order (dot and view highlight).
pub mod colors {
    /// Faces picked for the command (the selection colour in the view).
    pub const FACES: [u8; 3] = [64, 128, 242];
    /// A direction (the arrow in the view).
    pub const DIRECTION: [u8; 3] = [230, 120, 20];
    /// A plane or a parting tool.
    pub const PLANE: [u8; 3] = [30, 160, 160];
    /// Edges (static edges, fixed edges).
    pub const EDGES: [u8; 3] = [170, 80, 200];
    /// Curves that guide a shape (rails, a centre line).
    pub const GUIDES: [u8; 3] = [200, 60, 160];
    /// Points (map points).
    pub const POINTS: [u8; 3] = [220, 170, 30];
}

// ------------------------------------------------- command definitions ---

/// Kinds a Pull Direction takes: a planar face or a plane (their normal),
/// a straight edge or an axis (its direction).
const PULL: &[PickKind] = &[
    PickKind::PlanarFace,
    PickKind::Plane,
    PickKind::StraightEdge,
    PickKind::Axis,
];
const PLANES: &[PickKind] = &[PickKind::PlanarFace, PickKind::Plane];
const PARTING: &[PickKind] = &[PickKind::PlanarFace, PickKind::Plane, PickKind::Sketch];
const FACES: &[PickKind] = &[PickKind::Face];
const EDGES: &[PickKind] = &[PickKind::Edge];

/// Prompts of the Face Draft selectors (status bar: `Face Draft: <prompt>`).
pub mod prompts {
    /// Pull Direction.
    pub const PULL: &str =
        "Select the pull direction (a planar face, a plane, a straight edge or an axis)";
    /// Fixed Plane.
    pub const PLANE: &str = "Select the fixed plane (a planar face or a work or origin plane)";
    /// Parting Line's tool.
    pub const PARTING: &str = "Select the parting tool (a plane, a planar face or a sketch)";
    /// Faces, Fixed Edge.
    pub const FACES_EDGE: &str = "Select faces to draft (click near the edge that stays fixed)";
    /// Faces, other types.
    pub const FACES: &str = "Select faces to draft";
    /// Static edges (Move Parting Line).
    pub const STATIC: &str = "Select the static edges, or Select Boundary";
    /// Shell's faces to remove.
    pub const REMOVE: &str = "Select faces to remove (none: a closed cavity)";
}

/// The selectors of Face Draft for its type (`docs/spec/2.8-draft.md`).
pub fn draft_selectors(d: &crate::part::DraftFeature) -> Vec<Selector> {
    use crate::part::DraftType;
    let pull = Selector {
        key: "pull",
        label: "Pull Direction",
        prompt: prompts::PULL,
        accepts: PULL,
        single: true,
        required: true,
        count: usize::from(d.pull.is_some()),
        color: colors::DIRECTION,
    };
    let faces = Selector {
        key: "faces",
        label: "Faces",
        prompt: if d.draft_type == DraftType::FixedEdge {
            prompts::FACES_EDGE
        } else {
            prompts::FACES
        },
        accepts: FACES,
        single: false,
        required: true,
        count: d.faces.len(),
        color: colors::FACES,
    };
    match d.draft_type {
        DraftType::FixedEdge => vec![pull, faces],
        DraftType::FixedPlane => vec![
            Selector {
                key: "plane",
                label: "Fixed Plane",
                prompt: prompts::PLANE,
                accepts: PLANES,
                single: true,
                required: true,
                count: usize::from(d.plane.is_some()),
                color: colors::PLANE,
            },
            faces,
        ],
        DraftType::PartingLine => {
            let mut v = vec![
                pull,
                Selector {
                    key: "parting",
                    label: "Parting Tool",
                    prompt: prompts::PARTING,
                    accepts: PARTING,
                    single: true,
                    required: true,
                    count: usize::from(d.parting.is_some()),
                    color: colors::PLANE,
                },
                faces,
            ];
            if d.move_parting {
                v.push(Selector {
                    key: "static",
                    label: "Static Edges",
                    prompt: prompts::STATIC,
                    accepts: EDGES,
                    single: false,
                    required: true,
                    count: d.static_edges.len(),
                    color: colors::EDGES,
                });
            }
            v
        }
    }
}

/// The selectors of Shell (`docs/spec/2.6-shell.md`): the faces to remove,
/// optional (none makes a closed cavity).
pub fn shell_selectors(s: &crate::part::ShellFeature) -> Vec<Selector> {
    vec![Selector {
        key: "remove",
        label: "Remove Faces",
        prompt: prompts::REMOVE,
        accepts: FACES,
        single: false,
        required: false,
        count: s.remove.len(),
        color: colors::FACES,
    }]
}

/// Prompts of the Loft selectors (status bar: `Loft: <prompt>`).
pub mod loft_prompts {
    /// Sections.
    pub const SECTIONS: &str = "Select the sections in order (closed regions, planar faces, or a \
                                sketch point at an end; a second click drops one)";
    /// Rails.
    pub const RAILS: &str = "Select rails (sketch curves or edges that touch every section), or OK";
    /// Center Line.
    pub const CENTER: &str = "Select the centre line (a sketch curve or edges)";
    /// Map Point.
    pub const MAP: &str = "Click a point on each section's outline, in section order";
}

const SECTIONS: &[PickKind] = &[
    PickKind::Region,
    PickKind::PlanarFace,
    PickKind::SketchPoint,
];
const GUIDE_CURVES: &[PickKind] = &[PickKind::SketchCurve, PickKind::Edge];
const SECTION_POINTS: &[PickKind] = &[PickKind::SectionPoint];

/// The selectors of Loft (`docs/spec/5.2-loft.md`): Sections, then Rails or
/// the Center Line (Center Line, Area Loft); with `map` (Transition tab,
/// manual mapping) the points of the point set `map` instead.
pub fn loft_selectors(l: &crate::part::LoftFeature, map: Option<usize>) -> Vec<Selector> {
    use crate::part::LoftGuide;
    let mut v = vec![Selector {
        key: "sections",
        label: "Sections",
        prompt: loft_prompts::SECTIONS,
        accepts: SECTIONS,
        single: false,
        required: true,
        count: l.sections.len(),
        color: colors::FACES,
    }];
    match l.guide {
        LoftGuide::Rails => v.push(Selector {
            key: "rails",
            label: "Rails",
            prompt: loft_prompts::RAILS,
            accepts: GUIDE_CURVES,
            single: false,
            required: false,
            count: l.rails.len(),
            color: colors::GUIDES,
        }),
        LoftGuide::CenterLine | LoftGuide::AreaLoft => v.push(Selector {
            key: "center",
            label: if l.guide == LoftGuide::AreaLoft {
                "Area Loft"
            } else {
                "Center Line"
            },
            prompt: loft_prompts::CENTER,
            accepts: GUIDE_CURVES,
            single: true,
            required: true,
            count: usize::from(l.center_line.as_ref().is_some_and(|c| !c.is_empty())),
            color: colors::GUIDES,
        }),
    }
    if let Some(k) = map {
        v.push(Selector {
            key: "map",
            label: "Map Point",
            prompt: loft_prompts::MAP,
            accepts: SECTION_POINTS,
            single: false,
            required: false,
            count: l.map_points.get(k).map_or(0, |m| m.points.len()),
            color: colors::POINTS,
        });
    }
    v
}

const BODIES: &[PickKind] = &[PickKind::Body];
const SPLIT_TOOLS: &[PickKind] = &[PickKind::Plane, PickKind::Sketch, PickKind::Surface];

/// Prompts of the Combine and Split selectors.
pub mod body_prompts {
    /// Combine: Base.
    pub const BASE: &str = "Select the base body";
    /// Combine: Toolbodies.
    pub const TOOLS: &str = "Select the toolbodies (a second click drops one)";
    /// Split: the tool.
    pub const TOOL: &str =
        "Select the split tool (a work or origin plane, a sketch or a surface body)";
    /// Split Face: the faces.
    pub const FACES: &str = "Select the faces to split";
    /// Split: the solid.
    pub const SOLID: &str = "Select the solid";
}

/// The selectors of Combine (`docs/spec/2.13-combine-split.md`): Base, then
/// Toolbodies.
pub fn combine_selectors(c: &crate::part::CombineFeature) -> Vec<Selector> {
    vec![
        Selector {
            key: "base",
            label: "Base",
            prompt: body_prompts::BASE,
            accepts: BODIES,
            single: true,
            required: true,
            count: usize::from(!c.base.is_empty()),
            color: colors::PLANE,
        },
        Selector {
            key: "tools",
            label: "Toolbodies",
            prompt: body_prompts::TOOLS,
            accepts: BODIES,
            single: false,
            required: true,
            count: c.tools.len(),
            color: colors::FACES,
        },
    ]
}

/// The selectors of Split for its method: the tool, then the faces (Split
/// Face) or the solid (required when the part has `many` solids).
pub fn split_selectors(s: &crate::part::SplitFeature, many: bool) -> Vec<Selector> {
    use crate::part::SplitMethod;
    let tool = Selector {
        key: "tool",
        label: "Split Tool",
        prompt: body_prompts::TOOL,
        accepts: SPLIT_TOOLS,
        single: true,
        required: true,
        count: usize::from(s.tool.is_some()),
        color: colors::PLANE,
    };
    let solid = Selector {
        key: "solid",
        label: "Solid",
        prompt: body_prompts::SOLID,
        accepts: BODIES,
        single: true,
        required: many,
        count: usize::from(!s.solid.is_empty()),
        color: colors::EDGES,
    };
    match s.method {
        SplitMethod::SplitFace if s.all_faces => vec![tool, solid],
        SplitMethod::SplitFace => vec![
            tool,
            Selector {
                key: "faces",
                label: "Faces",
                prompt: body_prompts::FACES,
                accepts: FACES,
                single: false,
                required: true,
                count: s.faces.len(),
                color: colors::FACES,
            },
        ],
        SplitMethod::TrimSolid | SplitMethod::SplitSolid => vec![tool, solid],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(
        key: &'static str,
        accepts: &'static [PickKind],
        single: bool,
        required: bool,
        count: usize,
    ) -> Selector {
        Selector {
            key,
            label: key,
            prompt: key,
            accepts,
            single,
            required,
            count,
            color: colors::FACES,
        }
    }

    /// Face Draft, Fixed Edge: Pull Direction, then Faces.
    fn draft(pull: usize, faces: usize) -> Vec<Selector> {
        vec![
            sel(
                "pull",
                &[
                    PickKind::PlanarFace,
                    PickKind::Plane,
                    PickKind::StraightEdge,
                    PickKind::Axis,
                ],
                true,
                true,
                pull,
            ),
            sel("faces", &[PickKind::Face], false, true, faces),
        ]
    }

    #[test]
    fn starts_on_the_first_missing_input() {
        assert_eq!(Flow::new(draft(0, 0), None).active_key(), Some("pull"));
        assert_eq!(Flow::new(draft(1, 0), None).active_key(), Some("faces"));
        // Everything there (Edit Feature): the first selector.
        assert_eq!(Flow::new(draft(1, 2), None).active_key(), Some("pull"));
        // A remembered key wins; an unknown one falls back.
        assert_eq!(
            Flow::new(draft(0, 0), Some("faces")).active_key(),
            Some("faces")
        );
        assert_eq!(
            Flow::new(draft(0, 0), Some("plane")).active_key(),
            Some("pull")
        );
        assert_eq!(Flow::new(Vec::new(), None).active, None);
    }

    #[test]
    fn single_pick_auto_advances() {
        let mut f = Flow::new(draft(0, 0), None);
        f.selectors[0].count = 1;
        assert!(f.picked());
        assert_eq!(f.active_key(), Some("faces"));
        // Faces take many: a pick stays.
        f.selectors[1].count = 1;
        assert!(!f.picked());
        assert_eq!(f.active_key(), Some("faces"));
        // Skips filled selectors to the next empty one.
        let mut f = Flow::new(
            vec![
                sel("a", &[PickKind::Plane], true, true, 1),
                sel("b", &[PickKind::Axis], true, true, 1),
                sel("c", &[PickKind::Face], false, true, 0),
            ],
            Some("a"),
        );
        assert!(f.picked());
        assert_eq!(f.active_key(), Some("c"));
        // The last single selector stays.
        let mut f = Flow::new(vec![sel("a", &[PickKind::Plane], true, true, 1)], None);
        assert!(!f.picked());
        assert_eq!(f.active_key(), Some("a"));
    }

    #[test]
    fn continue_and_back_order() {
        let three = || {
            vec![
                sel("pull", &[PickKind::Plane], true, true, 1),
                sel("tool", &[PickKind::Plane, PickKind::Sketch], true, true, 0),
                sel("faces", &[PickKind::Face], false, true, 0),
            ]
        };
        let mut f = Flow::new(three(), Some("pull"));
        // The first missing required input first.
        assert!(f.continue_());
        assert_eq!(f.active_key(), Some("tool"));
        f.selectors[1].count = 1;
        assert!(f.continue_());
        assert_eq!(f.active_key(), Some("faces"));
        f.selectors[2].count = 3;
        // At the end, nothing missing: no move.
        assert!(!f.continue_());
        assert!(f.back());
        assert_eq!(f.active_key(), Some("tool"));
        assert!(f.back());
        assert!(!f.back());
        assert_eq!(f.active_key(), Some("pull"));
        // Continue from the middle with everything there: the next one.
        f.activate("tool");
        assert!(f.continue_());
        assert_eq!(f.active_key(), Some("faces"));
    }

    #[test]
    fn enter_is_continue_until_ready_then_ok() {
        let mut f = Flow::new(draft(0, 0), None);
        assert_eq!(f.enter(), EnterAction::Continue);
        assert_eq!(f.active_key(), Some("pull"));
        f.selectors[0].count = 1;
        f.activate("pull");
        assert_eq!(f.enter(), EnterAction::Continue);
        assert_eq!(f.active_key(), Some("faces"));
        f.selectors[1].count = 4;
        assert!(f.ready());
        assert_eq!(f.enter(), EnterAction::Ok);
        // An optional empty selector does not hold OK back (Shell: no
        // faces to remove = a closed cavity).
        let mut s = Flow::new(
            vec![sel("remove", &[PickKind::Face], false, false, 0)],
            None,
        );
        assert_eq!(s.enter(), EnterAction::Ok);
    }

    #[test]
    fn clearing_activates_the_selector() {
        let mut f = Flow::new(draft(1, 3), Some("pull"));
        f.cleared("faces");
        assert_eq!(f.active_key(), Some("faces"));
        assert!(!f.ready());
        f.cleared("nothing");
        assert_eq!(f.active_key(), Some("faces"));
    }

    #[test]
    fn accepted_kinds_follow_the_active_selector() {
        let mut f = Flow::new(draft(0, 0), None);
        for k in [
            PickKind::PlanarFace,
            PickKind::Plane,
            PickKind::StraightEdge,
            PickKind::Axis,
        ] {
            assert!(f.accepts(k), "{k:?}");
        }
        assert!(!f.accepts(PickKind::Face));
        assert!(!f.accepts(PickKind::Edge));
        f.activate("faces");
        assert!(f.accepts(PickKind::Face));
        assert!(!f.accepts(PickKind::StraightEdge));
        assert_eq!(f.prompt(), "faces");
    }

    #[test]
    fn menu_entries_and_their_state() {
        let f = Flow::new(draft(0, 0), None);
        let m = f.menu(true, true);
        let get = |k: MenuItem| m.iter().find(|(x, _)| *x == k).map(|(_, on)| *on);
        assert_eq!(get(MenuItem::Ok), Some(false));
        assert_eq!(get(MenuItem::Apply), Some(false));
        assert_eq!(get(MenuItem::Continue), Some(true));
        assert_eq!(get(MenuItem::Back), Some(false));
        assert_eq!(get(MenuItem::Cancel), Some(true));
        assert_eq!(get(MenuItem::Repeat), Some(true));
        let f = Flow::new(draft(1, 1), Some("faces"));
        let m = f.menu(false, false);
        assert_eq!(m.first(), Some(&(MenuItem::Ok, true)));
        assert!(
            !m.iter()
                .any(|(x, _)| *x == MenuItem::Apply || *x == MenuItem::Repeat)
        );
        assert!(m.contains(&(MenuItem::Continue, false)));
        assert!(m.contains(&(MenuItem::Back, true)));
    }
    #[test]
    fn draft_selectors_per_type() {
        use crate::part::{DraftFeature, DraftType, OriginPlane, PlaneRef, PullRef};
        let mut d = DraftFeature::new(0.05);
        let keys = |d: &DraftFeature| draft_selectors(d).iter().map(|s| s.key).collect::<Vec<_>>();
        assert_eq!(keys(&d), ["pull", "faces"]);
        let f = Flow::new(draft_selectors(&d), None);
        assert_eq!(f.active_key(), Some("pull"));
        assert!(f.accepts(PickKind::Axis) && f.accepts(PickKind::StraightEdge));
        assert!(!f.accepts(PickKind::Sketch) && !f.accepts(PickKind::Face));
        d.set_type(DraftType::FixedPlane);
        assert_eq!(keys(&d), ["plane", "faces"]);
        let f = Flow::new(draft_selectors(&d), None);
        assert!(f.accepts(PickKind::Plane) && f.accepts(PickKind::PlanarFace));
        assert!(!f.accepts(PickKind::StraightEdge) && !f.accepts(PickKind::Axis));
        d.set_type(DraftType::PartingLine);
        assert_eq!(keys(&d), ["pull", "parting", "faces"]);
        d.move_parting = true;
        assert_eq!(keys(&d), ["pull", "parting", "faces", "static"]);
        // The pull picked: the parting tool is next.
        d.pull = Some(PullRef::Plane(PlaneRef::Origin(OriginPlane::XY)));
        let mut f = Flow::new(draft_selectors(&d), Some("pull"));
        assert!(f.picked());
        assert_eq!(f.active_key(), Some("parting"));
        assert!(f.accepts(PickKind::Sketch));
        // Enter with the tool, faces and static edges missing: Continue.
        assert_eq!(f.enter(), EnterAction::Continue);
        assert_eq!(f.active_key(), Some("parting"));
        // Shell: nothing required.
        let s = crate::part::ShellFeature::new(2.0);
        let mut f = Flow::new(shell_selectors(&s), None);
        assert!(f.ready());
        assert_eq!(f.enter(), EnterAction::Ok);
        assert!(f.accepts(PickKind::Face) && !f.accepts(PickKind::Edge));
    }

    #[test]
    fn loft_selectors_follow_the_guide() {
        use crate::part::{LoftFeature, LoftGuide, PathRef};
        let mut l = LoftFeature::new(Vec::new());
        let f = Flow::new(loft_selectors(&l, None), None);
        assert_eq!(f.active_key(), Some("sections"));
        assert!(!f.ready());
        assert!(f.accepts(PickKind::Region) && !f.accepts(PickKind::SketchCurve));
        l.guide = LoftGuide::CenterLine;
        let keys: Vec<_> = loft_selectors(&l, None).iter().map(|s| s.key).collect();
        assert_eq!(keys, ["sections", "center"]);
        let mut f = Flow::new(loft_selectors(&l, None), Some("center"));
        assert!(f.accepts(PickKind::SketchCurve));
        l.center_line = Some(PathRef::Edges(Vec::new()));
        assert_eq!(loft_selectors(&l, None)[1].count, 0);
        // Enter with no sections: back to Sections.
        assert_eq!(f.enter(), EnterAction::Continue);
        assert_eq!(f.active_key(), Some("sections"));
        let keys: Vec<_> = loft_selectors(&l, Some(0)).iter().map(|s| s.key).collect();
        assert_eq!(keys, ["sections", "center", "map"]);
    }
}
