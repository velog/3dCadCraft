//! Representations of an assembly (`docs/spec/3.9-representations.md`): View
//! (what is seen), Position (where the mechanism stands) and Variants
//! (what it consists of). Three independent lists; a change of one leaves the
//! others alone. GUI-free.
//!
//! The occurrences and relationships always hold the state of the active
//! representations, so the view, picking, the solver and the BOM read them as
//! usual. [`Representations::sync`] writes changes of that live state into the
//! active representations; `activate_*` syncs first and then composes the new
//! state. Everything refers to occurrences and relationships by id (never
//! reused).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::pattern::Layout;
use super::relation::ConstraintKind;
use crate::document::iprops::{BomStructure, IProperties};
use crate::document::{AssemblyDocument, DocRef, OccurrenceLooks};
use crate::kernel::{Real, Transform3, Vector3};
use crate::part::expr::{self, Unit};
use crate::part::params::format_value;
use crate::part::variants::{Cell, Row, RowKey, Set, Table, VariantInfo, info_rows, set_info};

/// The list every kind starts with: `[Primary]` has the id 0.
pub const PRIMARY: u32 = 0;

/// The view representation an assembly starts in.
pub const DEFAULT_VIEW: u32 = 1;

/// A map by id as a list of pairs: a document is read through an internally
/// tagged enum, where the JSON keys of a `u32` map come back as text and fail.
mod id_map {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer, T: Serialize>(
        m: &BTreeMap<u32, T>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.collect_seq(m.iter())
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        d: D,
    ) -> Result<BTreeMap<u32, T>, D::Error> {
        Ok(Vec::<(u32, T)>::deserialize(d)?.into_iter().collect())
    }
}

/// What a view representation stores of one occurrence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccDisplay {
    /// Visibility.
    pub visible: bool,
    /// Enabled.
    pub enabled: bool,
    /// Transparent.
    pub transparent: bool,
    /// Appearance overrides.
    #[serde(default, skip_serializing_if = "OccurrenceLooks::is_empty")]
    pub looks: OccurrenceLooks,
}

/// A stored camera: the numbers of the view, free of the GUI's type.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedCamera {
    /// The point looked at.
    pub target: [Real; 3],
    /// Eye to target (mm).
    pub distance: Real,
    /// About world Z (radians).
    pub yaw: Real,
    /// Elevation (radians).
    pub pitch: Real,
    /// About the view axis (radians).
    pub roll: Real,
}

/// A view representation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewRep {
    /// Stable id ([`PRIMARY`] for `[Primary]`).
    pub id: u32,
    /// Its name.
    pub name: String,
    /// The display settings no longer change.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    /// The camera is stored whenever the representation is left.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub auto_camera: bool,
    /// The stored camera.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<SavedCamera>,
    /// Per occurrence id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "id_map")]
    pub display: BTreeMap<u32, OccDisplay>,
}

/// Overrides of one relationship in a positional representation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RelOverride {
    /// The value (mm or radians).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Real>,
    /// Suppressed or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppressed: Option<bool>,
}

/// Overrides of one occurrence in a positional representation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OccOverride {
    /// Grounded or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grounded: Option<bool>,
    /// Position of its origin in assembly coordinates (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<[Real; 3]>,
}

/// A positional representation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PositionRep {
    /// Stable id (never [`PRIMARY`]: `[Primary]` holds no overrides).
    pub id: u32,
    /// Its name.
    pub name: String,
    /// Per relationship id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "id_map")]
    pub relationships: BTreeMap<u32, RelOverride>,
    /// Per occurrence id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "id_map")]
    pub occurrences: BTreeMap<u32, OccOverride>,
    /// Where each occurrence last stood here, so a drawing needs no solver and
    /// the solver starts on the same branch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "id_map")]
    pub poses: BTreeMap<u32, Transform3>,
}

/// A relationship's value in a variant.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RelValue {
    /// Offset (mm) or angle (radians).
    pub value: Real,
    /// A joint holds it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    /// The equation, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
}

/// A variant.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    /// Stable id ([`PRIMARY`] for `[Primary]`).
    pub id: u32,
    /// Its name.
    pub name: String,
    /// Occurrences suppressed here.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub occurrences: BTreeSet<u32>,
    /// Relationships suppressed here.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub relationships: BTreeSet<u32>,
    /// Relationships' values here, by id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "id_map")]
    pub values: BTreeMap<u32, RelValue>,
    /// Component patterns' layouts here, by pattern id: a count that differs makes
    /// or removes copies when the patterns are placed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "id_map")]
    pub patterns: BTreeMap<u32, Layout>,
    /// The part variant each occurrence places here, by occurrence id (absent:
    /// the one active in the part).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "id_map")]
    pub parts: BTreeMap<u32, u32>,
    /// Occurrences counted as Reference in the BOM here.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub references: BTreeSet<u32>,
    /// Its Properties and BOM Structure.
    #[serde(default, skip_serializing_if = "is_default_info")]
    pub info: VariantInfo,
}

fn is_default_info(i: &VariantInfo) -> bool {
    *i == VariantInfo::default()
}

/// What a variant holds besides its suppressions, as the assembly has it now.
fn extras_of(a: &AssemblyDocument) -> Extras {
    Extras {
        values: a
            .constraints
            .iter()
            .map(|c| {
                (
                    c.id,
                    RelValue {
                        value: c.value,
                        locked: c.locked,
                        expr: c.expr.clone(),
                    },
                )
            })
            .collect(),
        patterns: a
            .patterns
            .iter()
            .map(|p| (p.id, p.layout.clone()))
            .collect(),
        parts: a
            .occurrences
            .iter()
            .filter_map(|o| Some((o.id, o.component.variant?)))
            .collect(),
        references: a
            .occurrences
            .iter()
            .filter(|o| o.bom == Some(BomStructure::Reference))
            .map(|o| o.id)
            .collect(),
    }
}

/// See [`extras_of`].
struct Extras {
    values: BTreeMap<u32, RelValue>,
    patterns: BTreeMap<u32, Layout>,
    parts: BTreeMap<u32, u32>,
    references: BTreeSet<u32>,
}

/// In Factory Scope another variant takes the entries that changed.
fn spread_map<T: Clone + PartialEq>(
    other: &mut BTreeMap<u32, T>,
    old: &BTreeMap<u32, T>,
    now: &BTreeMap<u32, T>,
) {
    for (k, v) in now {
        if old.get(k) != Some(v) {
            other.insert(*k, v.clone());
        }
    }
    for k in old.keys() {
        if !now.contains_key(k) {
            other.remove(k);
        }
    }
}

/// `[Primary]`'s values of what a positional representation can override.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PrimaryPosition {
    /// Relationship id → value.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "id_map")]
    pub values: BTreeMap<u32, Real>,
    /// Relationship id → the value is held (a joint can be free).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "id_map")]
    pub locked: BTreeMap<u32, bool>,
    /// Occurrence id → grounded.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "id_map")]
    pub grounded: BTreeMap<u32, bool>,
    /// Occurrence id → placement.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "id_map")]
    pub poses: BTreeMap<u32, Transform3>,
}

/// All three lists of an assembly. Empty in an assembly that has only its
/// `[Primary]` entries, so such a file does not change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Representations {
    /// View representations (`[Primary]` first, once there is another).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<ViewRep>,
    /// The active view representation.
    #[serde(default, skip_serializing_if = "is_primary")]
    pub active_view: u32,
    /// Positional representations (without `[Primary]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub positions: Vec<PositionRep>,
    /// The active positional representation.
    #[serde(default, skip_serializing_if = "is_primary")]
    pub active_position: u32,
    /// `[Primary]`'s values.
    #[serde(default, skip_serializing_if = "is_default")]
    pub primary: PrimaryPosition,
    /// Model variants (`[Primary]` first, once there is another).
    #[serde(default, skip_serializing_if = "Vec::is_empty", alias = "states")]
    pub variants: Vec<Variant>,
    /// The active variant.
    #[serde(default, skip_serializing_if = "is_primary", alias = "active_state")]
    pub active_variant: u32,
    /// Edit Factory Scope: suppressions go to every variant.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub factory_scope: bool,
    /// The highest occurrence id seen, to recognise a new one.
    #[serde(default, skip_serializing_if = "is_primary")]
    pub known: u32,
}

fn yes() -> bool {
    true
}

fn is_yes(v: &bool) -> bool {
    *v
}

fn is_primary(v: &u32) -> bool {
    *v == PRIMARY
}

fn is_default(p: &PrimaryPosition) -> bool {
    *p == PrimaryPosition::default()
}

/// The next `base<k>` not in `names`.
fn next_name<'a>(base: &str, names: impl Iterator<Item = &'a str>) -> String {
    let k = names
        .filter_map(|n| n.strip_prefix(base)?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{base}{}", k + 1)
}

impl Representations {
    /// Only `[Primary]` entries.
    pub fn is_empty(&self) -> bool {
        self.views.is_empty() && self.positions.is_empty() && self.variants.is_empty()
    }

    fn view_of(a: &AssemblyDocument) -> BTreeMap<u32, OccDisplay> {
        a.occurrences
            .iter()
            .map(|o| {
                (
                    o.id,
                    OccDisplay {
                        visible: o.visible,
                        enabled: o.enabled,
                        transparent: o.transparent,
                        looks: o.looks.clone(),
                    },
                )
            })
            .collect()
    }

    fn variant_of(a: &AssemblyDocument) -> (BTreeSet<u32>, BTreeSet<u32>) {
        (
            a.occurrences
                .iter()
                .filter(|o| o.suppressed)
                .map(|o| o.id)
                .collect(),
            a.constraints
                .iter()
                .filter(|c| c.suppressed)
                .map(|c| c.id)
                .collect(),
        )
    }

    fn primary_of(a: &AssemblyDocument) -> PrimaryPosition {
        PrimaryPosition {
            values: a.constraints.iter().map(|c| (c.id, c.value)).collect(),
            locked: a.constraints.iter().map(|c| (c.id, c.locked)).collect(),
            grounded: a.occurrences.iter().map(|o| (o.id, o.grounded)).collect(),
            poses: a.occurrences.iter().map(|o| (o.id, o.transform)).collect(),
        }
    }

    /// The active view representation.
    pub fn active_view(&self) -> Option<&ViewRep> {
        self.views.iter().find(|v| v.id == self.active_view)
    }

    /// The active positional representation; `None` is `[Primary]`.
    pub fn active_position(&self) -> Option<&PositionRep> {
        self.positions.iter().find(|p| p.id == self.active_position)
    }

    /// The active variant.
    pub fn active_variant(&self) -> Option<&Variant> {
        self.variants.iter().find(|s| s.id == self.active_variant)
    }

    /// `[Primary]` is the active positional representation.
    pub fn is_primary_position(&self) -> bool {
        self.active_position == PRIMARY
    }

    fn next_view_id(&self) -> u32 {
        self.views.iter().map(|v| v.id).max().unwrap_or(0) + 1
    }

    fn next_position_id(&self) -> u32 {
        self.positions.iter().map(|p| p.id).max().unwrap_or(0) + 1
    }

    fn next_variant_id(&self) -> u32 {
        self.variants.iter().map(|s| s.id).max().unwrap_or(0) + 1
    }

    /// The view list comes to life: a locked `[Primary]` (everything shown, no
    /// overrides) and an active `Default` with the present display.
    pub fn ensure_views(&mut self, a: &AssemblyDocument) {
        if self.views.is_empty() {
            self.views.push(ViewRep {
                id: PRIMARY,
                name: "Primary".into(),
                locked: true,
                auto_camera: false,
                camera: None,
                display: BTreeMap::new(),
            });
            self.views.push(ViewRep {
                id: DEFAULT_VIEW,
                name: "Default".into(),
                locked: false,
                auto_camera: true,
                camera: None,
                display: Self::view_of(a),
            });
            self.active_view = DEFAULT_VIEW;
        }
    }

    /// The same for the model variants.
    fn ensure_variants(&mut self, a: &AssemblyDocument) {
        if self.variants.is_empty() {
            let (occurrences, relationships) = Self::variant_of(a);
            let x = extras_of(a);
            self.variants.push(Variant {
                id: PRIMARY,
                name: "Primary".into(),
                occurrences,
                relationships,
                values: x.values,
                patterns: x.patterns,
                parts: x.parts,
                references: x.references,
                info: VariantInfo::default(),
            });
        }
    }

    /// Writes the changes of the live state into the active representations;
    /// `true` when something changed. A locked view stores nothing: what was
    /// changed comes back when it is activated again. Cheap: called once a
    /// frame while the assembly has representations.
    pub fn sync(&mut self, a: &AssemblyDocument) -> bool {
        let mut changed = false;
        let top = a.occurrences.iter().map(|o| o.id).max().unwrap_or(0);
        let fresh = top > self.known;
        // Removed things leave every list.
        let occ_ok = |id: &u32| a.find(*id).is_some();
        let rel_ok = |id: &u32| a.constraints.iter().any(|c| c.id == *id);
        for v in &mut self.views {
            let n = v.display.len();
            v.display.retain(|id, _| occ_ok(id));
            changed |= n != v.display.len();
        }
        for p in &mut self.positions {
            let n = p.relationships.len() + p.occurrences.len() + p.poses.len();
            p.relationships.retain(|id, _| rel_ok(id));
            p.occurrences.retain(|id, _| occ_ok(id));
            p.poses.retain(|id, _| occ_ok(id));
            changed |= n != p.relationships.len() + p.occurrences.len() + p.poses.len();
        }
        for s in &mut self.variants {
            let n = s.occurrences.len() + s.relationships.len();
            s.occurrences.retain(occ_ok);
            s.relationships.retain(rel_ok);
            s.values.retain(|id, _| rel_ok(id));
            s.patterns
                .retain(|id, _| a.patterns.iter().any(|p| p.id == *id));
            s.parts.retain(|id, _| occ_ok(id));
            s.references.retain(occ_ok);
            changed |= n != s.occurrences.len() + s.relationships.len();
        }
        // View.
        let active_view = self.active_view;
        if let Some(v) = self.views.iter_mut().find(|v| v.id == active_view)
            && !v.locked
        {
            let now = Self::view_of(a);
            if v.display != now {
                v.display = now;
                changed = true;
            }
        }
        // Position: [Primary] takes everything; another one takes what differs.
        if self.active_position == PRIMARY {
            if !self.positions.is_empty() {
                let now = Self::primary_of(a);
                if self.primary != now {
                    self.primary = now;
                    changed = true;
                }
            }
        } else {
            let primary = self.primary.clone();
            let base_suppressed = self
                .active_variant()
                .map(|s| s.relationships.clone())
                .unwrap_or_default();
            let base_values = self
                .active_variant()
                .map(|s| s.values.clone())
                .unwrap_or_default();
            let active = self.active_position;
            if let Some(p) = self.positions.iter_mut().find(|p| p.id == active) {
                for c in &a.constraints {
                    let e = p.relationships.entry(c.id).or_default();
                    let before = *e;
                    // The variant's value is the base; a relationship new since
                    // [Primary] was stored has none.
                    e.value = base_values
                        .get(&c.id)
                        .map(|v| v.value)
                        .or_else(|| primary.values.get(&c.id).copied())
                        .filter(|v| (*v - c.value).abs() > 1e-12)
                        .map(|_| c.value);
                    let base = base_suppressed.contains(&c.id);
                    e.suppressed = (c.suppressed != base).then_some(c.suppressed);
                    changed |= before != *e;
                }
                p.relationships
                    .retain(|_, r| r.value.is_some() || r.suppressed.is_some());
                for o in &a.occurrences {
                    let e = p.occurrences.entry(o.id).or_default();
                    let before = *e;
                    let base = primary.grounded.get(&o.id).copied().unwrap_or(o.grounded);
                    e.grounded = (o.grounded != base).then_some(o.grounded);
                    changed |= before != *e;
                    if p.poses.get(&o.id) != Some(&o.transform) {
                        p.poses.insert(o.id, o.transform);
                        changed = true;
                    }
                }
                p.occurrences
                    .retain(|_, r| r.grounded.is_some() || r.offset.is_some());
            }
        }
        // Model state: occurrences always; relationships unless a position
        // other than [Primary] takes them as overrides.
        if !self.variants.is_empty() {
            let (occ_now, rel_now) = Self::variant_of(a);
            let primary_position = self.active_position == PRIMARY;
            let factory = self.factory_scope;
            let active = self.active_variant;
            let before = self.variants.iter().find(|s| s.id == active).cloned();
            if let Some(old) = before {
                let rel_now = if primary_position {
                    rel_now
                } else {
                    old.relationships.clone()
                };
                let mut x = extras_of(a);
                if !primary_position {
                    // Values changed in a position are that position's overrides.
                    x.values = old.values.clone();
                }
                if old.occurrences != occ_now
                    || old.relationships != rel_now
                    || old.values != x.values
                    || old.patterns != x.patterns
                    || old.parts != x.parts
                    || old.references != x.references
                {
                    changed = true;
                    for s in &mut self.variants {
                        if s.id == active {
                            s.occurrences = occ_now.clone();
                            s.relationships = rel_now.clone();
                            s.values = x.values.clone();
                            s.patterns = x.patterns.clone();
                            s.parts = x.parts.clone();
                            s.references = x.references.clone();
                        } else if factory {
                            apply_diff(&mut s.occurrences, &old.occurrences, &occ_now);
                            apply_diff(&mut s.relationships, &old.relationships, &rel_now);
                            spread_map(&mut s.values, &old.values, &x.values);
                            spread_map(&mut s.patterns, &old.patterns, &x.patterns);
                            spread_map(&mut s.parts, &old.parts, &x.parts);
                            apply_diff(&mut s.references, &old.references, &x.references);
                        }
                    }
                }
            }
            // Added in Member Scope: only the active state has it.
            if fresh && !factory {
                for o in a
                    .occurrences
                    .iter()
                    .filter(|o| o.id > self.known && a.pattern_of(o.id).is_none())
                {
                    for s in self.variants.iter_mut().filter(|s| s.id != active) {
                        changed |= s.occurrences.insert(o.id);
                    }
                }
            }
        }
        if fresh {
            self.known = top;
            changed = true;
        }
        changed
    }

    /// Composes the live state from the active representations: positions from
    /// the stored poses (the caller solves afterwards).
    pub fn compose(&self, a: &mut AssemblyDocument) {
        if let Some(v) = self.active_view() {
            apply_view(v, a);
        }
        let state = self.active_variant();
        if let Some(s) = state {
            for o in &mut a.occurrences {
                o.suppressed = s.occurrences.contains(&o.id);
            }
            for c in &mut a.constraints {
                c.suppressed = s.relationships.contains(&c.id);
            }
            for p in &mut a.patterns {
                if let Some(l) = s.patterns.get(&p.id) {
                    p.layout = l.clone();
                }
            }
            for o in &mut a.occurrences {
                o.component.variant = s.parts.get(&o.id).copied();
                let reference = s.references.contains(&o.id);
                if reference {
                    o.bom = Some(BomStructure::Reference);
                } else if o.bom == Some(BomStructure::Reference) {
                    o.bom = None;
                }
            }
        }
        // Without positions there is no stored [Primary] to go back to.
        if self.positions.is_empty() {
            apply_values(state, a);
            return;
        }
        for c in &mut a.constraints {
            if let Some(v) = self.primary.values.get(&c.id) {
                c.value = *v;
            }
            if let Some(l) = self.primary.locked.get(&c.id) {
                c.locked = *l;
            }
        }
        apply_values(state, a);
        for o in &mut a.occurrences {
            if let Some(g) = self.primary.grounded.get(&o.id) {
                o.grounded = *g;
            }
            if let Some(t) = self.primary.poses.get(&o.id) {
                o.transform = *t;
            }
        }
        let Some(p) = self.active_position() else {
            return;
        };
        for c in &mut a.constraints {
            if let Some(r) = p.relationships.get(&c.id) {
                if let Some(v) = r.value {
                    c.value = v;
                    // A free joint would not hold it.
                    c.locked = c.kind.is_joint();
                }
                if let Some(s) = r.suppressed {
                    c.suppressed = s;
                }
            }
        }
        for o in &mut a.occurrences {
            if let Some(t) = p.poses.get(&o.id) {
                o.transform = *t;
            }
            if let Some(r) = p.occurrences.get(&o.id) {
                if let Some(g) = r.grounded {
                    o.grounded = g;
                }
                if let Some(q) = r.offset {
                    o.transform = placed_at(&o.transform, q);
                }
            }
        }
    }

    /// New view representation from the present display: active; returns its id.
    pub fn new_view(&mut self, a: &AssemblyDocument) -> u32 {
        self.sync(a);
        self.ensure_views(a);
        let id = self.next_view_id();
        let name = next_name("View", self.views.iter().map(|v| v.name.as_str()));
        self.views.push(ViewRep {
            id,
            name,
            locked: false,
            auto_camera: true,
            camera: None,
            display: Self::view_of(a),
        });
        self.active_view = id;
        id
    }

    /// Copy of a view representation: active; returns its id.
    pub fn copy_view(&mut self, from: u32, a: &mut AssemblyDocument) -> Option<u32> {
        self.sync(a);
        self.ensure_views(a);
        let mut v = self.views.iter().find(|v| v.id == from)?.clone();
        v.id = self.next_view_id();
        v.name = next_name(&v.name, self.views.iter().map(|v| v.name.as_str()));
        v.locked = false;
        let id = v.id;
        self.views.push(v);
        self.active_view = id;
        self.compose(a);
        Some(id)
    }

    /// Activates a view representation.
    pub fn activate_view(&mut self, id: u32, a: &mut AssemblyDocument) -> bool {
        self.sync(a);
        self.ensure_views(a);
        if !self.views.iter().any(|v| v.id == id) {
            return false;
        }
        self.active_view = id;
        self.compose(a);
        true
    }

    /// Deletes a view representation (not `[Primary]`, not the active one).
    pub fn delete_view(&mut self, id: u32) -> bool {
        if id == PRIMARY || id == self.active_view {
            return false;
        }
        let n = self.views.len();
        self.views.retain(|v| v.id != id);
        n != self.views.len()
    }

    /// Copy to Variant: a new active state in which the components hidden
    /// in that view are suppressed; returns its id.
    pub fn view_to_variant(&mut self, from: u32, a: &mut AssemblyDocument) -> Option<u32> {
        self.sync(a);
        self.ensure_views(a);
        let v = self.views.iter().find(|v| v.id == from)?;
        let hidden: BTreeSet<u32> = a
            .occurrences
            .iter()
            .filter(|o| v.display.get(&o.id).is_some_and(|d| !d.visible))
            .map(|o| o.id)
            .collect();
        let id = self.new_variant(a);
        if let Some(s) = self.variants.iter_mut().find(|s| s.id == id) {
            s.occurrences.extend(hidden);
        }
        self.compose(a);
        Some(id)
    }

    /// New positional representation: active; returns its id. It starts as a
    /// copy of `[Primary]`, so it holds no overrides.
    pub fn new_position(&mut self, a: &mut AssemblyDocument) -> u32 {
        self.sync(a);
        // A relationship's own suppression lives in the model variants: an
        // override is what differs from it.
        self.ensure_variants(a);
        if self.positions.is_empty() {
            self.primary = Self::primary_of(a);
        }
        let id = self.next_position_id();
        let name = next_name("Position", self.positions.iter().map(|p| p.name.as_str()));
        let poses = self.primary.poses.clone();
        self.positions.push(PositionRep {
            id,
            name,
            relationships: BTreeMap::new(),
            occurrences: BTreeMap::new(),
            poses,
        });
        self.active_position = id;
        self.compose(a);
        id
    }

    /// Copy of a positional representation: active; returns its id.
    pub fn copy_position(&mut self, from: u32, a: &mut AssemblyDocument) -> Option<u32> {
        self.sync(a);
        let mut p = self.positions.iter().find(|p| p.id == from)?.clone();
        p.id = self.next_position_id();
        p.name = next_name(&p.name, self.positions.iter().map(|p| p.name.as_str()));
        let id = p.id;
        self.positions.push(p);
        self.active_position = id;
        self.compose(a);
        Some(id)
    }

    /// Activates a positional representation; the caller solves.
    pub fn activate_position(&mut self, id: u32, a: &mut AssemblyDocument) -> bool {
        self.sync(a);
        if id != PRIMARY && !self.positions.iter().any(|p| p.id == id) {
            return false;
        }
        self.active_position = id;
        self.compose(a);
        true
    }

    /// Deletes a positional representation (not the active one).
    pub fn delete_position(&mut self, id: u32) -> bool {
        if id == self.active_position {
            return false;
        }
        let n = self.positions.len();
        self.positions.retain(|p| p.id != id);
        n != self.positions.len()
    }

    /// New variant from the present suppressions: active; returns its id.
    pub fn new_variant(&mut self, a: &AssemblyDocument) -> u32 {
        self.sync(a);
        self.ensure_variants(a);
        let id = self.next_variant_id();
        let name = next_name("Variant", self.variants.iter().map(|s| s.name.as_str()));
        let (occurrences, relationships) = Self::variant_of(a);
        let x = extras_of(a);
        let info = self
            .active_variant()
            .map(|v| v.info.clone())
            .unwrap_or_default();
        self.variants.push(Variant {
            id,
            name,
            occurrences,
            relationships,
            values: x.values,
            patterns: x.patterns,
            parts: x.parts,
            references: x.references,
            info,
        });
        self.active_variant = id;
        self.known = self.known.max(a.next_id() - 1);
        id
    }

    /// A copy of variant `from`, active; the caller solves.
    pub fn duplicate_variant(&mut self, from: u32, a: &mut AssemblyDocument) -> Option<u32> {
        self.sync(a);
        self.ensure_variants(a);
        let mut v = self.variants.iter().find(|v| v.id == from)?.clone();
        v.id = self.next_variant_id();
        v.name = next_name(&v.name, self.variants.iter().map(|v| v.name.as_str()));
        let id = v.id;
        self.variants.push(v);
        self.active_variant = id;
        self.compose(a);
        Some(id)
    }

    /// Activates a variant; the caller solves.
    pub fn activate_variant(&mut self, id: u32, a: &mut AssemblyDocument) -> bool {
        self.sync(a);
        self.ensure_variants(a);
        if !self.variants.iter().any(|s| s.id == id) {
            return false;
        }
        self.active_variant = id;
        self.compose(a);
        true
    }

    /// Deletes a variant (not `[Primary]`, not the active one; every scope).
    pub fn delete_variant(&mut self, id: u32) -> bool {
        if id == PRIMARY || id == self.active_variant {
            return false;
        }
        let n = self.variants.len();
        self.variants.retain(|s| s.id != id);
        n != self.variants.len()
    }

    /// The active variant takes a document's Properties (and, in Factory Scope,
    /// every variant the ones that changed); `true` when something changed.
    pub fn take_info(&mut self, props: &IProperties) -> bool {
        let now = VariantInfo::of(props);
        crate::part::variants::take_info(
            &mut self.variants,
            self.active_variant,
            self.factory_scope,
            &now,
            |v| &mut v.info,
            |v| v.id,
        )
    }

    /// Variants without Properties take these (right after a new variant).
    pub fn fill_info(&mut self, props: &IProperties) {
        let now = VariantInfo::of(props);
        for v in &mut self.variants {
            if v.info == VariantInfo::default() {
                v.info = now.clone();
            }
        }
    }

    /// Is `name` free among the given names (case-insensitive), ignoring the
    /// entry itself?
    pub fn name_free<'a>(name: &str, own: &str, mut names: impl Iterator<Item = &'a str>) -> bool {
        let name = name.trim();
        !name.is_empty()
            && !name.eq_ignore_ascii_case("Primary")
            && !names.any(|n| n != own && n.eq_ignore_ascii_case(name))
    }

    /// A copy of the assembly standing in the given representations (`None` =
    /// the active one), for a drawing view: never solves; a position uses its
    /// stored poses.
    pub fn composed(
        &self,
        a: &AssemblyDocument,
        view: Option<u32>,
        position: Option<u32>,
        state: Option<u32>,
    ) -> AssemblyDocument {
        let mut r = self.clone();
        let mut out = a.clone();
        r.sync(&out);
        if let Some(v) = view.filter(|v| r.views.iter().any(|x| x.id == *v)) {
            r.active_view = v;
        }
        if let Some(p) =
            position.filter(|p| *p == PRIMARY || r.positions.iter().any(|x| x.id == *p))
        {
            r.active_position = p;
        }
        if let Some(s) = state.filter(|s| r.variants.iter().any(|x| x.id == *s)) {
            r.active_variant = s;
        }
        r.compose(&mut out);
        out
    }
}

/// The fields of a pattern's layout a cell can change: (field, label, unit, value).
fn layout_fields(l: &Layout) -> Vec<(&'static str, &'static str, Unit, Real)> {
    match l {
        Layout::Rectangular {
            column_count,
            column_spacing,
            row,
            row_count,
            row_spacing,
            ..
        } => {
            let mut f = vec![
                ("column_count", "Columns", Unit::Ul, *column_count as Real),
                (
                    "column_spacing",
                    "Column spacing",
                    Unit::Mm,
                    *column_spacing,
                ),
            ];
            if row.is_some() {
                f.push(("row_count", "Rows", Unit::Ul, *row_count as Real));
                f.push(("row_spacing", "Row spacing", Unit::Mm, *row_spacing));
            }
            f
        }
        Layout::Circular { count, angle, .. } => vec![
            ("count", "Count", Unit::Ul, *count as Real),
            ("angle", "Angle", Unit::Deg, *angle),
        ],
        Layout::Feature { .. } => Vec::new(),
    }
}

/// Sets one field of a layout; a count must be a whole number of at least 1.
fn set_layout_field(l: &mut Layout, field: &str, v: Real) -> Result<(), String> {
    let count = || {
        if v < 1.0 || (v - v.round()).abs() > 1e-9 {
            Err(format!(
                "a count must be a whole number of at least 1, not {v}"
            ))
        } else {
            Ok(v.round() as u32)
        }
    };
    match (l, field) {
        (Layout::Rectangular { column_count, .. }, "column_count") => *column_count = count()?,
        (Layout::Rectangular { column_spacing, .. }, "column_spacing") => *column_spacing = v,
        (Layout::Rectangular { row_count, .. }, "row_count") => *row_count = count()?,
        (Layout::Rectangular { row_spacing, .. }, "row_spacing") => *row_spacing = v,
        (Layout::Circular { count: c, .. }, "count") => *c = count()?,
        (Layout::Circular { angle, .. }, "angle") => *angle = v,
        _ => return Err(format!("the pattern has no {field}")),
    }
    Ok(())
}

/// A value cell's number in internal units.
fn cell_number(cell: &Cell) -> Result<Real, String> {
    let Cell::Value { text, unit, .. } = cell else {
        return Err("a value was expected".to_owned());
    };
    expr::eval_simple(text, *unit, &|_| None)
        .map(|r| r.0)
        .map_err(|e| format!("'{}': {e}", text.trim()))
}

/// Does a relationship have a value a variant can change?
fn has_value(k: ConstraintKind) -> bool {
    use ConstraintKind::*;
    matches!(
        k,
        Mate | Flush | Angle | Insert | Tangent | Rotational | Slider | Cylindrical
    )
}

/// The unit of a relationship's value.
fn value_unit(k: ConstraintKind) -> Unit {
    use ConstraintKind::*;
    if matches!(k, Angle | Rotational) {
        Unit::Deg
    } else {
        Unit::Mm
    }
}

/// The word of "the variant active in the part".
pub const PART_ACTIVE: &str = "(active in the part)";

/// What the Variants window shows of an assembly. `props`: the document's
/// Properties; `part_variants`: a component's own variants (empty: it has none).
pub fn assembly_table(
    a: &AssemblyDocument,
    props: &IProperties,
    part_variants: &dyn Fn(DocRef) -> Vec<(u32, String)>,
) -> Table {
    let r = &a.reps;
    // Before there is a list, one [Primary] stands for what the assembly holds.
    let live = Variant {
        id: PRIMARY,
        name: "Primary".to_owned(),
        occurrences: Representations::variant_of(a).0,
        relationships: Representations::variant_of(a).1,
        values: extras_of(a).values,
        patterns: extras_of(a).patterns,
        parts: extras_of(a).parts,
        references: extras_of(a).references,
        info: VariantInfo::of(props),
    };
    let list: Vec<&Variant> = if r.variants.is_empty() {
        vec![&live]
    } else {
        r.variants.iter().collect()
    };
    let cells = |f: &dyn Fn(&Variant) -> Cell| list.iter().map(|v| f(v)).collect::<Vec<_>>();
    let components = a
        .occurrences
        .iter()
        .map(|o| Row {
            key: RowKey::Occurrence(o.id),
            label: o.name.clone(),
            cells: cells(&|v| Cell::Suppressed(v.occurrences.contains(&o.id))),
        })
        .collect();
    let relationships = a
        .constraints
        .iter()
        .map(|c| Row {
            key: RowKey::Relationship(c.id),
            label: c.name.clone(),
            cells: cells(&|v| Cell::Suppressed(v.relationships.contains(&c.id))),
        })
        .collect();
    let values = a
        .constraints
        .iter()
        .filter(|c| has_value(c.kind))
        .map(|c| {
            let unit = value_unit(c.kind);
            Row {
                key: RowKey::RelValue(c.id),
                label: c.name.clone(),
                cells: cells(&|v| {
                    let x = v.values.get(&c.id);
                    let value = x.map_or(c.value, |x| x.value);
                    Cell::Value {
                        text: x
                            .and_then(|x| x.expr.clone())
                            .unwrap_or_else(|| format_value(value, unit)),
                        unit,
                        value,
                    }
                }),
            }
        })
        .collect();
    let mut patterns = Vec::new();
    for p in &a.patterns {
        for (field, label, unit, _) in layout_fields(&p.layout) {
            patterns.push(Row {
                key: RowKey::Pattern(p.id, field.to_owned()),
                label: format!("{}: {label}", p.name),
                cells: cells(&|v| {
                    let l = v.patterns.get(&p.id).unwrap_or(&p.layout);
                    let value = layout_fields(l)
                        .iter()
                        .find(|f| f.0 == field)
                        .map_or(0.0, |f| f.3);
                    Cell::Value {
                        text: format_value(value, unit),
                        unit,
                        value,
                    }
                }),
            });
        }
    }
    let mut parts = Vec::new();
    for o in a
        .occurrences
        .iter()
        .filter(|o| a.pattern_of(o.id).is_none_or(|p| p.sources.contains(&o.id)))
    {
        let own = part_variants(o.component.base());
        if own.is_empty() {
            continue;
        }
        let mut options = vec![PART_ACTIVE.to_owned()];
        options.extend(own.iter().map(|x| x.1.clone()));
        parts.push(Row {
            key: RowKey::OccVariant(o.id),
            label: o.name.clone(),
            cells: cells(&|v| Cell::Choice {
                options: options.clone(),
                selected: v
                    .parts
                    .get(&o.id)
                    .and_then(|id| own.iter().find(|x| x.0 == *id))
                    .map_or(PART_ACTIVE.to_owned(), |x| x.1.clone()),
            }),
        });
    }
    let references = a
        .occurrences
        .iter()
        .map(|o| Row {
            key: RowKey::OccReference(o.id),
            label: o.name.clone(),
            cells: cells(&|v| Cell::Choice {
                options: vec!["Counted".to_owned(), "Reference".to_owned()],
                selected: if v.references.contains(&o.id) {
                    "Reference"
                } else {
                    "Counted"
                }
                .to_owned(),
            }),
        })
        .collect();
    let infos: Vec<VariantInfo> = list.iter().map(|v| v.info.clone()).collect();
    Table {
        columns: list.iter().map(|v| (v.id, v.name.clone())).collect(),
        active: r.active_variant,
        factory_scope: r.factory_scope,
        sections: vec![
            ("Components".to_owned(), components),
            ("Part Variants".to_owned(), parts),
            ("Relationships".to_owned(), relationships),
            ("Relationship Values".to_owned(), values),
            ("Patterns".to_owned(), patterns),
            ("Properties".to_owned(), info_rows(&infos)),
            ("BOM of Components".to_owned(), references),
        ],
    }
}

/// Checks what [`apply_sets`] cannot refuse half way: every number evaluates.
pub fn check_sets(a: &AssemblyDocument, sets: &[Set]) -> Result<(), String> {
    for s in sets {
        match &s.key {
            RowKey::RelValue(_) => {
                cell_number(&s.cell)?;
            }
            RowKey::Pattern(id, field) => {
                let v = cell_number(&s.cell)?;
                let p = a.patterns.iter().find(|p| p.id == *id);
                let mut l = p.ok_or("the pattern no longer exists")?.layout.clone();
                set_layout_field(&mut l, field, v)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Writes changes of the table: the active variant through the live state, the
/// others into their lists. `part_variants` as in [`assembly_table`]. Call
/// [`check_sets`] first.
pub fn apply_sets(
    a: &mut AssemblyDocument,
    sets: &[Set],
    part_variants: &dyn Fn(DocRef) -> Vec<(u32, String)>,
) {
    let mut r = std::mem::take(&mut a.reps);
    for s in sets {
        let live = s.col == r.active_variant || r.variants.is_empty();
        apply_set(&mut r, a, s, live, part_variants);
    }
    a.reps = r;
}

fn apply_set(
    r: &mut Representations,
    a: &mut AssemblyDocument,
    s: &Set,
    live: bool,
    part_variants: &dyn Fn(DocRef) -> Vec<(u32, String)>,
) {
    let target = r.variants.iter_mut().find(|v| v.id == s.col);
    match (&s.key, &s.cell) {
        (RowKey::Occurrence(id), Cell::Suppressed(on)) => {
            if live {
                if let Some(o) = a.find_mut(*id) {
                    o.suppressed = *on;
                }
            } else if let Some(v) = target {
                toggle(&mut v.occurrences, *id, *on);
            }
        }
        (RowKey::Relationship(id), Cell::Suppressed(on)) => {
            if live {
                if let Some(c) = a.constraint_mut(*id) {
                    c.suppressed = *on;
                }
            } else if let Some(v) = target {
                toggle(&mut v.relationships, *id, *on);
            }
        }
        (RowKey::RelValue(id), cell) => {
            let Ok(value) = cell_number(cell) else {
                return;
            };
            let joint = a.constraint(*id).is_some_and(|c| c.kind.is_joint());
            if live {
                if let Some(c) = a.constraint_mut(*id) {
                    c.value = value;
                    c.locked = joint || c.locked;
                    c.expr = None;
                }
            } else if let Some(v) = target {
                v.values.insert(
                    *id,
                    RelValue {
                        value,
                        locked: joint,
                        expr: None,
                    },
                );
            }
        }
        (RowKey::Pattern(id, field), cell) => {
            let Ok(value) = cell_number(cell) else {
                return;
            };
            if live {
                if let Some(p) = a.patterns.iter_mut().find(|p| p.id == *id) {
                    let _ = set_layout_field(&mut p.layout, field, value);
                }
            } else if let Some(v) = target {
                let base = a
                    .patterns
                    .iter()
                    .find(|p| p.id == *id)
                    .map(|p| p.layout.clone());
                let e = v.patterns.get(id).cloned().or(base);
                if let Some(mut l) = e
                    && set_layout_field(&mut l, field, value).is_ok()
                {
                    v.patterns.insert(*id, l);
                }
            }
        }
        (RowKey::OccVariant(id), Cell::Choice { selected, .. }) => {
            let Some(base) = a.find(*id).map(|o| o.component.base()) else {
                return;
            };
            let chosen = part_variants(base)
                .into_iter()
                .find(|x| x.1 == *selected)
                .map(|x| x.0);
            if live {
                if let Some(o) = a.find_mut(*id) {
                    o.component.variant = chosen;
                }
            } else if let Some(v) = target {
                match chosen {
                    Some(c) => {
                        v.parts.insert(*id, c);
                    }
                    None => {
                        v.parts.remove(id);
                    }
                }
            }
        }
        (RowKey::OccReference(id), Cell::Choice { selected, .. }) => {
            let on = selected == "Reference";
            if live {
                if let Some(o) = a.find_mut(*id) {
                    o.bom = on.then_some(BomStructure::Reference);
                }
            } else if let Some(v) = target {
                toggle(&mut v.references, *id, on);
            }
        }
        (key @ (RowKey::Prop(_) | RowKey::BomStructure), cell) => {
            if let Some(v) = target {
                let _ = set_info(&mut v.info, key, cell);
            }
        }
        _ => {}
    }
}

fn toggle(set: &mut BTreeSet<u32>, id: u32, on: bool) {
    if on {
        set.insert(id);
    } else {
        set.remove(&id);
    }
}

/// A variant's relationship values go on the relationships.
fn apply_values(state: Option<&Variant>, a: &mut AssemblyDocument) {
    let Some(s) = state else {
        return;
    };
    for c in &mut a.constraints {
        if let Some(v) = s.values.get(&c.id) {
            c.value = v.value;
            c.locked = v.locked;
            c.expr = v.expr.clone();
        }
    }
}

/// `set` takes the same additions and removals as went from `old` to `now`.
fn apply_diff(set: &mut BTreeSet<u32>, old: &BTreeSet<u32>, now: &BTreeSet<u32>) {
    for id in now.difference(old) {
        set.insert(*id);
    }
    for id in old.difference(now) {
        set.remove(id);
    }
}

/// `t` with its origin at `q`, its turn kept.
fn placed_at(t: &Transform3, q: [Real; 3]) -> Transform3 {
    let mut out = *t;
    out.translation.vector = Vector3::new(q[0], q[1], q[2]);
    out
}

/// Writes a view's display on the occurrences; `true` when something changed.
/// An occurrence it does not list: visible, unless the view is locked (`[Primary]`
/// lists nothing and shows everything).
fn apply_view(v: &ViewRep, a: &mut AssemblyDocument) -> bool {
    let mut changed = false;
    for o in &mut a.occurrences {
        let d = v.display.get(&o.id);
        let visible = d.map_or(v.id == PRIMARY || !v.locked, |d| d.visible);
        let enabled = d.is_none_or(|d| d.enabled);
        let transparent = d.is_some_and(|d| d.transparent);
        changed |= o.visible != visible || o.enabled != enabled || o.transparent != transparent;
        o.visible = visible;
        o.enabled = enabled;
        o.transparent = transparent;
        let looks = d.map(|d| d.looks.clone()).unwrap_or_default();
        if o.looks != looks {
            o.looks = looks;
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::relation::{Constraint, ConstraintKind, GeomRef, OriginItem};
    use crate::document::{DocRef, DocumentData, Project};

    /// A component to place.
    fn comp() -> DocRef {
        let mut p = Project::new("p");
        DocRef::local(p.add_document("A", DocumentData::Assembly(Default::default())))
    }

    /// Three occurrences; 2 hangs on 1 by a relationship of value 5.
    fn asm() -> AssemblyDocument {
        let mut a = AssemblyDocument::default();
        let r = comp();
        a.place(r, "A", Transform3::identity(), true);
        a.place(r, "B", Transform3::translation(10.0, 0.0, 0.0), false);
        a.place(r, "C", Transform3::translation(20.0, 0.0, 0.0), false);
        let g = |occurrence| GeomRef::Origin {
            occurrence,
            sub: Vec::new(),
            item: OriginItem::XyPlane,
        };
        a.constraints.push(Constraint {
            id: 1,
            name: "Mate:1".into(),
            kind: ConstraintKind::Mate,
            a: g(1),
            b: g(2),
            value: 5.0,
            expr: None,
            aligned: false,
            suppressed: false,
            locked: false,
            min: None,
            max: None,
            resting: false,
            plane: None,
        });
        a
    }

    fn take(a: &mut AssemblyDocument) -> Representations {
        std::mem::take(&mut a.reps)
    }

    #[test]
    fn a_view_keeps_its_display_and_primary_keeps_its_own() {
        let mut a = asm();
        let mut r = take(&mut a);
        let v = r.new_view(&a);
        a.find_mut(2).unwrap().visible = false;
        a.find_mut(3).unwrap().transparent = true;
        r.sync(&a);
        assert!(r.activate_view(PRIMARY, &mut a));
        assert!(a.find(2).unwrap().visible);
        assert!(!a.find(3).unwrap().transparent);
        assert!(r.activate_view(v, &mut a));
        assert!(!a.find(2).unwrap().visible);
        assert!(a.find(3).unwrap().transparent);
        // Primary and the active one cannot be deleted.
        assert!(!r.delete_view(PRIMARY));
        assert!(!r.delete_view(v));
        assert!(r.activate_view(PRIMARY, &mut a));
        assert!(r.delete_view(v));
    }

    #[test]
    fn primary_and_default_start_the_list() {
        let mut a = asm();
        a.find_mut(2).unwrap().visible = false;
        let mut r = take(&mut a);
        let v = r.new_view(&a);
        let names: Vec<&str> = r.views.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["Primary", "Default", "View1"]);
        assert_eq!((r.active_view, v), (v, 2));
        assert!(r.views[0].locked && !r.views[1].locked);
        // [Primary] shows everything, Default what it had.
        r.activate_view(PRIMARY, &mut a);
        assert!(a.find(2).unwrap().visible);
        r.activate_view(DEFAULT_VIEW, &mut a);
        assert!(!a.find(2).unwrap().visible);
        // A copy adds a number to the name.
        let c = r.copy_view(v, &mut a).unwrap();
        assert_eq!(r.views.iter().find(|x| x.id == c).unwrap().name, "View11");
    }

    #[test]
    fn a_locked_view_comes_back_and_hides_what_was_added_when_activated_again() {
        let mut a = asm();
        let mut r = take(&mut a);
        let v = r.new_view(&a);
        r.views.iter_mut().find(|x| x.id == v).unwrap().locked = true;
        // The change is seen, but not stored.
        a.find_mut(2).unwrap().visible = false;
        r.sync(&a);
        assert!(!a.find(2).unwrap().visible);
        assert!(r.active_view().unwrap().display[&2].visible);
        // Added while locked: visible until the view is activated again.
        let id = a.place(comp(), "D", Transform3::identity(), false);
        r.sync(&a);
        assert!(a.find(id).unwrap().visible);
        r.activate_view(DEFAULT_VIEW, &mut a);
        assert!(a.find(id).unwrap().visible);
        r.activate_view(v, &mut a);
        assert!(a.find(2).unwrap().visible);
        assert!(!a.find(id).unwrap().visible);
    }

    #[test]
    fn a_position_overrides_and_primary_is_untouched() {
        let mut a = asm();
        let mut r = take(&mut a);
        let p = r.new_position(&mut a);
        assert!(r.positions[0].relationships.is_empty());
        // An edit in the active position becomes its override.
        a.constraints[0].value = 40.0;
        a.find_mut(3).unwrap().grounded = true;
        a.find_mut(2).unwrap().transform = Transform3::translation(40.0, 0.0, 0.0);
        assert!(r.sync(&a));
        assert_eq!(r.positions[0].relationships[&1].value, Some(40.0));
        assert_eq!(r.positions[0].occurrences[&3].grounded, Some(true));
        // [Primary] has its own values back, and the pose it had.
        assert!(r.activate_position(PRIMARY, &mut a));
        assert_eq!(a.constraints[0].value, 5.0);
        assert!(!a.find(3).unwrap().grounded);
        assert_eq!(
            a.find(2).unwrap().transform,
            Transform3::translation(10.0, 0.0, 0.0)
        );
        // And the position its own.
        assert!(r.activate_position(p, &mut a));
        assert_eq!(a.constraints[0].value, 40.0);
        assert!(a.find(3).unwrap().grounded);
        assert_eq!(
            a.find(2).unwrap().transform,
            Transform3::translation(40.0, 0.0, 0.0)
        );
        // Taking the value back removes the override.
        a.constraints[0].value = 5.0;
        r.sync(&a);
        assert!(!r.positions[0].relationships.contains_key(&1));
    }

    #[test]
    fn a_value_override_holds_a_free_joint_and_primary_frees_it_again() {
        let mut a = asm();
        a.constraints[0].kind = ConstraintKind::Slider;
        let mut r = take(&mut a);
        let p = r.new_position(&mut a);
        r.positions[0].relationships.insert(
            1,
            RelOverride {
                value: Some(40.0),
                suppressed: None,
            },
        );
        r.compose(&mut a);
        assert!(a.constraints[0].locked);
        assert_eq!(a.constraints[0].value, 40.0);
        r.activate_position(PRIMARY, &mut a);
        assert!(!a.constraints[0].locked);
        assert_eq!(a.constraints[0].value, 5.0);
        r.activate_position(p, &mut a);
        assert!(a.constraints[0].locked);
    }

    #[test]
    fn an_offset_override_places_the_origin_and_keeps_the_turn() {
        let mut a = asm();
        let mut r = take(&mut a);
        r.new_position(&mut a);
        r.positions[0].occurrences.insert(
            3,
            OccOverride {
                grounded: Some(true),
                offset: Some([1.0, 2.0, 3.0]),
            },
        );
        r.compose(&mut a);
        let t = a.find(3).unwrap().transform;
        assert_eq!(t.translation.vector, Vector3::new(1.0, 2.0, 3.0));
        // Grounded by its override; the offset stays through sync.
        r.sync(&a);
        assert_eq!(r.positions[0].occurrences[&3].offset, Some([1.0, 2.0, 3.0]));
    }

    #[test]
    fn model_states_member_and_factory_scope() {
        let mut a = asm();
        let mut r = take(&mut a);
        let s1 = r.new_variant(&a);
        a.find_mut(2).unwrap().suppressed = true;
        r.sync(&a);
        assert!(r.activate_variant(PRIMARY, &mut a));
        assert!(!a.find(2).unwrap().suppressed);
        // Factory Scope: one suppress reaches every state.
        r.factory_scope = true;
        a.find_mut(3).unwrap().suppressed = true;
        r.sync(&a);
        assert!(r.activate_variant(s1, &mut a));
        assert!(a.find(2).unwrap().suppressed && a.find(3).unwrap().suppressed);
        assert!(!r.delete_variant(PRIMARY));
        assert!(!r.delete_variant(s1));
    }

    #[test]
    fn a_component_added_in_member_scope_is_suppressed_elsewhere() {
        let mut a = asm();
        let mut r = take(&mut a);
        r.new_variant(&a);
        let id = a.place(comp(), "D", Transform3::identity(), false);
        r.sync(&a);
        assert!(!a.find(id).unwrap().suppressed);
        r.activate_variant(PRIMARY, &mut a);
        assert!(a.find(id).unwrap().suppressed);
    }

    #[test]
    fn a_copy_of_a_pattern_is_not_taken_for_a_new_component() {
        let mut a = with_pattern();
        let mut r = take(&mut a);
        r.new_variant(&a);
        let id = a.place(comp(), "C", Transform3::identity(), false);
        a.patterns[0]
            .elements
            .push(crate::assembly::pattern::Element {
                index: 2,
                occurrences: vec![id],
            });
        r.sync(&a);
        assert!(!r.variants[0].occurrences.contains(&id));
    }

    #[test]
    fn the_three_lists_are_independent() {
        let mut a = asm();
        let mut r = take(&mut a);
        let v = r.new_view(&a);
        a.find_mut(2).unwrap().visible = false;
        let s = r.new_variant(&a);
        a.find_mut(3).unwrap().suppressed = true;
        let p = r.new_position(&mut a);
        a.constraints[0].value = 7.0;
        r.sync(&a);
        // Changing the position leaves view and state.
        r.activate_position(PRIMARY, &mut a);
        assert!(!a.find(2).unwrap().visible && a.find(3).unwrap().suppressed);
        assert_eq!(a.constraints[0].value, 5.0);
        // Changing the view leaves position and state.
        r.activate_position(p, &mut a);
        r.activate_view(PRIMARY, &mut a);
        assert!(a.find(2).unwrap().visible && a.find(3).unwrap().suppressed);
        assert_eq!(a.constraints[0].value, 7.0);
        // Changing the state leaves view and position.
        r.activate_view(v, &mut a);
        r.activate_variant(PRIMARY, &mut a);
        assert!(!a.find(2).unwrap().visible && !a.find(3).unwrap().suppressed);
        assert_eq!(a.constraints[0].value, 7.0);
        assert_eq!(r.active_variant, PRIMARY);
        assert_eq!(s, 1);
    }

    #[test]
    fn a_position_relationship_suppress_is_an_override_not_a_state_change() {
        let mut a = asm();
        let mut r = take(&mut a);
        r.new_variant(&a);
        r.new_position(&mut a);
        a.constraints[0].suppressed = true;
        r.sync(&a);
        assert_eq!(r.positions[0].relationships[&1].suppressed, Some(true));
        assert!(r.active_variant().unwrap().relationships.is_empty());
    }

    #[test]
    fn removed_things_leave_every_list() {
        let mut a = asm();
        let mut r = take(&mut a);
        r.new_view(&a);
        r.new_variant(&a);
        r.new_position(&mut a);
        a.constraints[0].value = 9.0;
        r.sync(&a);
        a.remove(2);
        r.sync(&a);
        assert!(r.views.iter().all(|v| !v.display.contains_key(&2)));
        assert!(!r.positions[0].poses.contains_key(&2));
        assert!(r.positions[0].relationships.is_empty());
    }

    #[test]
    fn a_variant_has_its_own_relationship_values_and_a_position_overrides_them() {
        let mut a = asm();
        let mut r = take(&mut a);
        let v = r.new_variant(&a);
        a.constraints[0].value = 8.0;
        r.sync(&a);
        r.activate_variant(PRIMARY, &mut a);
        assert_eq!(a.constraints[0].value, 5.0);
        r.activate_variant(v, &mut a);
        assert_eq!(a.constraints[0].value, 8.0);
        // A position's override goes over the variant's value, and leaves it.
        let p = r.new_position(&mut a);
        assert_eq!(a.constraints[0].value, 8.0);
        a.constraints[0].value = 30.0;
        r.sync(&a);
        assert_eq!(r.positions[0].relationships[&1].value, Some(30.0));
        assert_eq!(r.active_variant().unwrap().values[&1].value, 8.0);
        r.activate_variant(PRIMARY, &mut a);
        assert_eq!(a.constraints[0].value, 30.0, "the override stays on");
        r.activate_position(PRIMARY, &mut a);
        assert_eq!(a.constraints[0].value, 5.0);
        r.activate_variant(v, &mut a);
        assert_eq!(a.constraints[0].value, 8.0);
        r.activate_position(p, &mut a);
        assert_eq!(a.constraints[0].value, 30.0);
    }

    #[test]
    fn a_free_joint_holds_a_value_set_for_a_variant() {
        let mut a = asm();
        a.constraints[0].kind = ConstraintKind::Slider;
        let mut r = take(&mut a);
        let v = r.new_variant(&a);
        r.activate_variant(PRIMARY, &mut a);
        a.reps = r;
        let none = |_: DocRef| Vec::new();
        let set = Set {
            key: RowKey::RelValue(1),
            col: v,
            cell: Cell::Value {
                text: "40 mm".into(),
                unit: Unit::Mm,
                value: 0.0,
            },
        };
        check_sets(&a, std::slice::from_ref(&set)).unwrap();
        apply_sets(&mut a, &[set], &none);
        assert_eq!(a.constraints[0].value, 5.0);
        let mut r = take(&mut a);
        r.activate_variant(v, &mut a);
        assert_eq!(a.constraints[0].value, 40.0);
        assert!(a.constraints[0].locked);
        r.activate_variant(PRIMARY, &mut a);
        assert!(!a.constraints[0].locked);
    }

    fn with_pattern() -> AssemblyDocument {
        let mut a = asm();
        a.patterns.push(crate::assembly::pattern::ComponentPattern {
            id: 1,
            name: "Component Pattern 1".into(),
            sources: vec![3],
            layout: Layout::Circular {
                axis: GeomRef::Origin {
                    occurrence: 0,
                    sub: Vec::new(),
                    item: OriginItem::ZAxis,
                },
                flip: false,
                count: 4,
                angle: 90f64.to_radians(),
            },
            elements: Vec::new(),
        });
        a
    }

    #[test]
    fn a_variant_has_its_own_pattern_layout_and_bad_counts_are_refused() {
        let mut a = with_pattern();
        let mut r = take(&mut a);
        let v = r.new_variant(&a);
        r.activate_variant(PRIMARY, &mut a);
        a.reps = r;
        let none = |_: DocRef| Vec::new();
        let cell = |text: &str| Cell::Value {
            text: text.into(),
            unit: Unit::Ul,
            value: 0.0,
        };
        let key = RowKey::Pattern(1, "count".into());
        let set = |t: &str| Set {
            key: key.clone(),
            col: v,
            cell: cell(t),
        };
        assert!(check_sets(&a, &[set("2.5")]).is_err());
        assert!(check_sets(&a, &[set("0")]).is_err());
        assert!(check_sets(&a, &[set("1 +")]).is_err());
        check_sets(&a, &[set("8")]).unwrap();
        apply_sets(&mut a, &[set("8")], &none);
        assert_eq!(a.patterns[0].layout.count(), 4);
        let mut r = take(&mut a);
        r.activate_variant(v, &mut a);
        assert_eq!(a.patterns[0].layout.count(), 8);
        r.activate_variant(PRIMARY, &mut a);
        assert_eq!(a.patterns[0].layout.count(), 4);
        a.reps = r;
        let t = assembly_table(&a, &IProperties::default(), &none);
        let rows = &t.sections.iter().find(|s| s.0 == "Patterns").unwrap().1;
        assert_eq!(rows[0].cells[1].text(), "8");
        assert!(Table::differs(&rows[0]));
    }

    #[test]
    fn a_variant_chooses_what_each_occurrence_places_and_how_it_counts() {
        let mut a = asm();
        let lookup = |_: DocRef| vec![(0, "Primary".to_owned()), (7, "Long".to_owned())];
        let mut r = take(&mut a);
        let v = r.new_variant(&a);
        a.reps = r;
        let choice = |t: &str| Cell::Choice {
            options: Vec::new(),
            selected: t.to_owned(),
        };
        let sets = [
            Set {
                key: RowKey::OccVariant(2),
                col: v,
                cell: choice("Long"),
            },
            Set {
                key: RowKey::OccReference(3),
                col: v,
                cell: choice("Reference"),
            },
        ];
        apply_sets(&mut a, &sets, &lookup);
        assert_eq!(a.find(2).unwrap().component.variant, Some(7));
        assert_eq!(a.find(3).unwrap().bom, Some(BomStructure::Reference));
        let mut r = take(&mut a);
        r.sync(&a);
        r.activate_variant(PRIMARY, &mut a);
        assert_eq!(a.find(2).unwrap().component.variant, None);
        assert_eq!(a.find(3).unwrap().bom, None);
        r.activate_variant(v, &mut a);
        assert_eq!(a.find(2).unwrap().component.variant, Some(7));
        assert_eq!(a.find(3).unwrap().bom, Some(BomStructure::Reference));
        a.reps = r;
        let t = assembly_table(&a, &IProperties::default(), &lookup);
        let rows = &t
            .sections
            .iter()
            .find(|s| s.0 == "Part Variants")
            .unwrap()
            .1;
        assert_eq!(rows.len(), a.occurrences.len());
        assert_eq!(rows[1].cells[1].text(), "Long");
        assert_eq!(rows[1].cells[0].text(), PART_ACTIVE);
    }

    #[test]
    fn a_variant_has_its_own_properties() {
        let mut a = asm();
        let mut doc = IProperties::default();
        doc.set("part_number", "ASM-1");
        let mut r = take(&mut a);
        let v = r.new_variant(&a);
        r.fill_info(&doc);
        doc.set("part_number", "ASM-2");
        assert!(r.take_info(&doc));
        assert_eq!(r.active_variant().unwrap().info.get("part_number"), "ASM-2");
        assert_eq!(r.variants[0].info.get("part_number"), "ASM-1");
        // Factory Scope: what changed reaches all.
        r.factory_scope = true;
        doc.set("description", "Pump");
        assert!(r.take_info(&doc));
        assert_eq!(r.variants[0].info.get("description"), "Pump");
        assert_eq!(r.variants[0].info.get("part_number"), "ASM-1");
        assert_eq!(v, r.active_variant);
    }

    #[test]
    fn view_to_variant_suppresses_the_hidden() {
        let mut a = asm();
        let mut r = take(&mut a);
        let v = r.new_view(&a);
        a.find_mut(2).unwrap().visible = false;
        let s = r.view_to_variant(v, &mut a).unwrap();
        assert_eq!(r.active_variant, s);
        assert!(a.find(2).unwrap().suppressed);
        assert!(!a.find(3).unwrap().suppressed);
    }

    #[test]
    fn composed_needs_no_solver_and_leaves_the_assembly() {
        let mut a = asm();
        let mut r = take(&mut a);
        let p = r.new_position(&mut a);
        a.find_mut(2).unwrap().transform = Transform3::translation(77.0, 0.0, 0.0);
        r.sync(&a);
        r.activate_position(PRIMARY, &mut a);
        let c = r.composed(&a, None, Some(p), None);
        assert_eq!(
            c.find(2).unwrap().transform,
            Transform3::translation(77.0, 0.0, 0.0)
        );
        assert_eq!(
            a.find(2).unwrap().transform,
            Transform3::translation(10.0, 0.0, 0.0)
        );
    }

    #[test]
    fn names_count_up_and_stay_unique() {
        assert_eq!(next_name("View", ["View1", "View3"].into_iter()), "View4");
        assert_eq!(next_name("Variant", [].into_iter()), "Variant1");
        assert!(Representations::name_free(
            "Open",
            "x",
            ["x", "Closed"].into_iter()
        ));
        assert!(!Representations::name_free(
            "open",
            "x",
            ["x", "Open"].into_iter()
        ));
        assert!(!Representations::name_free(
            "primary",
            "x",
            ["x"].into_iter()
        ));
        assert!(!Representations::name_free("  ", "x", ["x"].into_iter()));
    }

    #[test]
    fn an_assembly_without_representations_writes_nothing() {
        let a = asm();
        let text = serde_json::to_string(&a).unwrap();
        assert!(!text.contains("reps"), "{text}");
    }

    #[test]
    fn round_trip_through_json() {
        let mut a = asm();
        let mut r = take(&mut a);
        r.new_view(&a);
        r.new_variant(&a);
        r.new_position(&mut a);
        a.reps = r;
        let text = serde_json::to_string(&a).unwrap();
        let b: AssemblyDocument = serde_json::from_str(&text).unwrap();
        assert_eq!(a.reps, b.reps);
    }
}
