//! Variants of a part (`docs/spec/3.9-representations.md` §3.3; Inventor: Model
//! States): one part, several named sets of suppressed features and parameter
//! equations, one of them active. GUI-free.
//!
//! As in an assembly, the part always holds the active variant's state, so
//! regeneration and every reader stay as they are; [`PartDocument::variants_sync`]
//! writes changes of the part into the active variant. Features are named by id
//! and parameters by name (rules 2, 3).
//!
//! [`Table`] is what the Variants window shows, for parts and assemblies alike:
//! one row per item, one column per variant, and the corrections that reduce to
//! a list of [`Set`]s.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::expr::Unit;
use super::params::ParamOwner;
use super::{PartDocument, PartError};
use crate::document::iprops::{BomStructure, IProperties};
use crate::kernel::Real;

/// `[Primary]` has the id 0.
pub const PRIMARY: u32 = 0;

/// The Properties a variant has its own values of.
pub const VARIANT_PROPS: [(&str, &str); 3] = [
    ("part_number", "Part Number"),
    ("stock_number", "Stock Number"),
    ("description", "Description"),
];

/// A BOM Structure's words, for a cell.
pub const BOM_WORDS: [(BomStructure, &str); 5] = [
    (BomStructure::Normal, "Normal"),
    (BomStructure::Phantom, "Phantom"),
    (BomStructure::Reference, "Reference"),
    (BomStructure::Purchased, "Purchased"),
    (BomStructure::Inseparable, "Inseparable"),
];

/// A BOM Structure from its word.
pub fn bom_of_word(word: &str) -> Option<BomStructure> {
    BOM_WORDS.iter().find(|w| w.1 == word).map(|w| w.0)
}

/// The word of a BOM Structure.
pub fn word_of_bom(b: BomStructure) -> &'static str {
    BOM_WORDS
        .iter()
        .find(|w| w.0 == b)
        .map_or("Normal", |w| w.1)
}

/// A variant's Properties and BOM Structure.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VariantInfo {
    /// Values of [`VARIANT_PROPS`], by key; empty ones are absent.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, String>,
    /// How the document counts in a BOM.
    #[serde(default, skip_serializing_if = "is_normal")]
    pub bom: BomStructure,
}

fn is_normal(b: &BomStructure) -> bool {
    *b == BomStructure::Normal
}

impl VariantInfo {
    /// What a document's Properties hold of it.
    pub fn of(p: &IProperties) -> Self {
        Self {
            props: VARIANT_PROPS
                .iter()
                .filter(|(k, _)| !p.get(k).is_empty())
                .map(|(k, _)| ((*k).to_owned(), p.get(k).to_owned()))
                .collect(),
            bom: p.bom_structure,
        }
    }

    /// Writes it into a document's Properties.
    pub fn write(&self, p: &mut IProperties) {
        for (k, _) in VARIANT_PROPS {
            p.set(k, self.props.get(k).map_or("", String::as_str));
        }
        p.bom_structure = self.bom;
    }

    /// The Property `key`, empty when absent.
    pub fn get(&self, key: &str) -> &str {
        self.props.get(key).map_or("", String::as_str)
    }

    /// Sets the Property `key`; an empty text removes it.
    pub fn set(&mut self, key: &str, text: &str) {
        let t = text.trim();
        if t.is_empty() {
            self.props.remove(key);
        } else {
            self.props.insert(key.to_owned(), t.to_owned());
        }
    }
}

/// The table's rows of a variant's Properties, one column per entry of `infos`.
pub fn info_rows(infos: &[VariantInfo]) -> Vec<Row> {
    let mut rows: Vec<Row> = VARIANT_PROPS
        .iter()
        .map(|(k, label)| Row {
            key: RowKey::Prop((*k).to_owned()),
            label: (*label).to_owned(),
            cells: infos
                .iter()
                .map(|i| Cell::Text(i.get(k).to_owned()))
                .collect(),
        })
        .collect();
    rows.push(Row {
        key: RowKey::BomStructure,
        label: "BOM Structure".to_owned(),
        cells: infos
            .iter()
            .map(|i| Cell::Choice {
                options: BOM_WORDS.iter().map(|w| w.1.to_owned()).collect(),
                selected: word_of_bom(i.bom).to_owned(),
            })
            .collect(),
    });
    rows
}

/// Writes one cell of a Properties row into `info`; `false` when it is no such row.
pub fn set_info(info: &mut VariantInfo, key: &RowKey, cell: &Cell) -> Result<bool, String> {
    match (key, cell) {
        (RowKey::Prop(k), Cell::Text(t)) => {
            info.set(k, t);
            Ok(true)
        }
        (RowKey::BomStructure, Cell::Choice { selected, .. }) => {
            info.bom =
                bom_of_word(selected).ok_or_else(|| format!("'{selected}' is no BOM Structure"))?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// One variant of a part.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PartVariant {
    /// Stable id ([`PRIMARY`] for `[Primary]`).
    pub id: u32,
    /// Its name.
    pub name: String,
    /// Features suppressed here, by id.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub suppressed: BTreeSet<String>,
    /// Every parameter's equation here, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    /// Its material (`None`: the Generic one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Its Properties and BOM Structure.
    #[serde(default, skip_serializing_if = "is_default_info")]
    pub info: VariantInfo,
}

fn is_default_info(i: &VariantInfo) -> bool {
    *i == VariantInfo::default()
}

/// The variants of a part. Empty in a part with only its `[Primary]`, so such a
/// file does not change.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartVariants {
    /// `[Primary]` first, once there is another.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list: Vec<PartVariant>,
    /// The active one.
    #[serde(default, skip_serializing_if = "is_primary")]
    pub active: u32,
    /// Edit Factory Scope: changes go to every variant.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub factory_scope: bool,
    /// The features seen so far, to recognise a new one.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub known: BTreeSet<String>,
}

fn is_primary(v: &u32) -> bool {
    *v == PRIMARY
}

impl PartVariants {
    /// Only `[Primary]`.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The variant `id`.
    pub fn get(&self, id: u32) -> Option<&PartVariant> {
        self.list.iter().find(|v| v.id == id)
    }

    fn get_mut(&mut self, id: u32) -> Option<&mut PartVariant> {
        self.list.iter_mut().find(|v| v.id == id)
    }

    /// Is `name` free for the variant called `own`?
    pub fn name_free(&self, name: &str, own: &str) -> bool {
        let name = name.trim();
        !name.is_empty()
            && !name.eq_ignore_ascii_case("Primary")
            && !self
                .list
                .iter()
                .any(|v| v.name != own && v.name.eq_ignore_ascii_case(name))
    }
}

/// The next `base<k>` not among `names`.
pub fn next_name<'a>(base: &str, names: impl Iterator<Item = &'a str>) -> String {
    let k = names
        .filter_map(|n| n.strip_prefix(base)?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{base}{}", k + 1)
}

impl PartDocument {
    /// Every parameter's equation or value as typed, by name (physical
    /// properties are results, not inputs).
    fn parameter_equations(&self) -> BTreeMap<String, String> {
        self.parameter_table()
            .into_iter()
            .filter(|r| {
                !matches!(
                    r.owner,
                    ParamOwner::Physical(_) | ParamOwner::Reference { .. }
                )
            })
            .map(|r| {
                let text = r.equation();
                (r.name, text)
            })
            .collect()
    }

    fn ensure_variants(&mut self) {
        if self.variants.is_empty() {
            let v = PartVariant {
                id: PRIMARY,
                name: "Primary".into(),
                suppressed: self.suppressed.clone(),
                params: self.parameter_equations(),
                material: self.material.clone(),
                info: VariantInfo::default(),
            };
            self.variants.list.push(v);
            self.variants.known = self.features.iter().map(|f| f.id().to_owned()).collect();
        }
    }

    /// Writes changes of the part into the active variant (and, in Factory
    /// Scope, into all); `true` when something changed. Removed features and
    /// parameters leave every variant; a feature added in Member Scope is
    /// suppressed in the others.
    pub fn variants_sync(&mut self) -> bool {
        if self.variants.is_empty() {
            return false;
        }
        let live = self.parameter_equations();
        let suppressed = self.suppressed.clone();
        let material = self.material.clone();
        let features: BTreeSet<String> = self.features.iter().map(|f| f.id().to_owned()).collect();
        let vs = &mut self.variants;
        let active = vs.active;
        let factory = vs.factory_scope;
        let mut changed = false;
        let fresh: Vec<String> = features.difference(&vs.known).cloned().collect();
        if !fresh.is_empty() || features.len() != vs.known.len() {
            vs.known = features.clone();
            changed = true;
        }
        let Some(old) = vs.get(active).cloned() else {
            return changed;
        };
        for v in &mut vs.list {
            if v.id == active {
                continue;
            }
            let n = v.suppressed.len() + v.params.len();
            v.suppressed.retain(|f| features.contains(f));
            v.params.retain(|p, _| live.contains_key(p));
            changed |= n != v.suppressed.len() + v.params.len();
            // A new parameter starts with the value it has now.
            for (p, e) in &live {
                if !v.params.contains_key(p) {
                    v.params.insert(p.clone(), e.clone());
                    changed = true;
                }
            }
            if factory {
                for f in suppressed.difference(&old.suppressed) {
                    changed |= v.suppressed.insert(f.clone());
                }
                for f in old.suppressed.difference(&suppressed) {
                    changed |= v.suppressed.remove(f);
                }
                for (p, e) in &live {
                    if old.params.get(p).is_some_and(|o| o != e) {
                        v.params.insert(p.clone(), e.clone());
                        changed = true;
                    }
                }
                if old.material != material && v.material != material {
                    v.material = material.clone();
                    changed = true;
                }
            } else {
                for f in &fresh {
                    changed |= v.suppressed.insert(f.clone());
                }
            }
        }
        if let Some(v) = vs.get_mut(active)
            && (v.suppressed != suppressed || v.params != live || v.material != material)
        {
            v.suppressed = suppressed;
            v.params = live;
            v.material = material;
            changed = true;
        }
        changed
    }

    /// The active variant takes a document's Properties (and, in Factory Scope,
    /// every variant the ones that changed); `true` when something changed.
    pub fn variants_take_info(&mut self, props: &IProperties) -> bool {
        let now = VariantInfo::of(props);
        take_info(
            &mut self.variants.list,
            self.variants.active,
            self.variants.factory_scope,
            &now,
            |v| &mut v.info,
            |v| v.id,
        )
    }

    /// The Properties and BOM Structure of the active variant.
    pub fn active_variant_info(&self) -> Option<&VariantInfo> {
        self.variants.get(self.variants.active).map(|v| &v.info)
    }

    /// Variants without Properties take these (right after a new variant: the
    /// list started without the document's Properties).
    pub fn variants_fill_info(&mut self, props: &IProperties) {
        let now = VariantInfo::of(props);
        for v in &mut self.variants.list {
            if v.info == VariantInfo::default() {
                v.info = now.clone();
            }
        }
    }

    /// Writes one Properties or Material cell of variant `col`.
    pub fn set_variant_info(
        &mut self,
        col: u32,
        key: &RowKey,
        cell: &Cell,
    ) -> Result<(), PartError> {
        let bad = PartError::Feature;
        if let (RowKey::Material, Cell::Text(name)) = (key, cell) {
            let lib = crate::materials::with_user();
            let name = name.trim();
            let chosen = (!name.is_empty()).then_some(name);
            if col == self.variants.active || self.variants.is_empty() {
                return self.set_material(chosen, lib).map_err(bad);
            }
            // Checked on a copy: the material is copied into the part only when used.
            self.clone().set_material(chosen, lib).map_err(bad)?;
            if let Some(v) = self.variants.get_mut(col) {
                v.material = chosen.map(str::to_owned);
            }
            return Ok(());
        }
        let v = self
            .variants
            .get_mut(col)
            .ok_or_else(|| bad("the variant no longer exists".to_owned()))?;
        set_info(&mut v.info, key, cell).map_err(bad)?;
        Ok(())
    }

    /// A new variant from the part as it is: active; returns its id.
    pub fn new_variant(&mut self) -> u32 {
        self.variants_sync();
        self.ensure_variants();
        let id = self.variants.list.iter().map(|v| v.id).max().unwrap_or(0) + 1;
        let name = next_name(
            "Variant",
            self.variants.list.iter().map(|v| v.name.as_str()),
        );
        let info = self
            .variants
            .get(self.variants.active)
            .map(|v| v.info.clone())
            .unwrap_or_default();
        let v = PartVariant {
            id,
            name,
            suppressed: self.suppressed.clone(),
            params: self.parameter_equations(),
            material: self.material.clone(),
            info,
        };
        self.variants.list.push(v);
        self.variants.active = id;
        id
    }

    /// A copy of variant `from`, active; returns its id. Fails like
    /// [`PartDocument::activate_variant`].
    pub fn duplicate_variant(&mut self, from: u32) -> Result<u32, PartError> {
        self.variants_sync();
        self.ensure_variants();
        let Some(mut v) = self.variants.get(from).cloned() else {
            return Err(PartError::Feature("the variant no longer exists".into()));
        };
        let id = self.variants.list.iter().map(|v| v.id).max().unwrap_or(0) + 1;
        v.id = id;
        v.name = next_name(&v.name, self.variants.list.iter().map(|v| v.name.as_str()));
        self.variants.list.push(v);
        self.activate_variant(id)?;
        Ok(id)
    }

    /// Makes variant `id` the active one and rebuilds the part. A failure
    /// leaves the part as it was (rule 5).
    pub fn activate_variant(&mut self, id: u32) -> Result<(), PartError> {
        self.variants_sync();
        self.ensure_variants();
        let before = self.clone();
        let result = self.activate_inner(id);
        if result.is_err() {
            *self = before;
        }
        result
    }

    fn activate_inner(&mut self, id: u32) -> Result<(), PartError> {
        let Some(v) = self.variants.get(id).cloned() else {
            return Err(PartError::Feature("the variant no longer exists".into()));
        };
        self.suppressed = v.suppressed;
        if self.material != v.material {
            self.set_material(v.material.as_deref(), crate::materials::with_user())
                .map_err(PartError::Feature)?;
        }
        let live = self.parameter_equations();
        for (p, e) in &v.params {
            if live.get(p).is_some_and(|now| now != e) {
                self.set_parameter_expr(p, e)?;
            }
        }
        self.variants.active = id;
        self.regenerate()?;
        Ok(())
    }

    /// Deletes a variant (not `[Primary]`, not the active one; every scope).
    pub fn delete_variant(&mut self, id: u32) -> bool {
        if id == PRIMARY || id == self.variants.active {
            return false;
        }
        let n = self.variants.list.len();
        self.variants.list.retain(|v| v.id != id);
        n != self.variants.list.len()
    }

    /// Suppresses a feature (with what depends on it) or brings it back (with
    /// what it needs) in variant `col`.
    pub fn set_variant_suppressed(
        &mut self,
        col: u32,
        feature: &str,
        on: bool,
    ) -> Result<(), PartError> {
        let k = self
            .feature_index(feature)
            .ok_or_else(|| PartError::Feature(format!("the part has no feature '{feature}'")))?;
        if col == self.variants.active || self.variants.is_empty() {
            if on {
                self.suppress_feature(k)?;
            } else {
                self.unsuppress_feature(k)?;
            }
            return Ok(());
        }
        let related: Vec<String> = if on {
            self.dependents(k)
        } else {
            self.requirements(k)
        }
        .into_iter()
        .map(|j| self.features[j].id().to_owned())
        .collect();
        let v = self
            .variants
            .get_mut(col)
            .ok_or_else(|| PartError::Feature("the variant no longer exists".into()))?;
        for f in std::iter::once(feature.to_owned()).chain(related) {
            if on {
                v.suppressed.insert(f);
            } else {
                v.suppressed.remove(&f);
            }
        }
        Ok(())
    }

    /// Sets a parameter's equation in variant `col`; a text that does not
    /// evaluate is refused whichever variant it is for.
    pub fn set_variant_param(&mut self, col: u32, name: &str, text: &str) -> Result<(), PartError> {
        if col == self.variants.active || self.variants.is_empty() {
            return self.set_parameter_expr(name, text);
        }
        // Checked on a copy of the part standing in that variant.
        let mut trial = self.clone();
        trial.activate_variant(col)?;
        trial.set_parameter_expr(name, text)?;
        let v = self
            .variants
            .get_mut(col)
            .ok_or_else(|| PartError::Feature("the variant no longer exists".into()))?;
        v.params.insert(name.to_owned(), text.trim().to_owned());
        Ok(())
    }

    /// What the Variants window shows; `props` are the document's Properties.
    pub fn variants_table(&self, props: &IProperties) -> Table {
        let vs = &self.variants;
        let one = PartVariant {
            id: PRIMARY,
            name: "Primary".into(),
            suppressed: self.suppressed.clone(),
            params: self.parameter_equations(),
            material: self.material.clone(),
            info: VariantInfo::of(props),
        };
        let list: Vec<&PartVariant> = if vs.is_empty() {
            vec![&one]
        } else {
            vs.list.iter().collect()
        };
        let features = self
            .features
            .iter()
            .map(|f| Row {
                key: RowKey::Feature(f.id().to_owned()),
                label: f.name().to_owned(),
                cells: list
                    .iter()
                    .map(|v| Cell::Suppressed(v.suppressed.contains(f.id())))
                    .collect(),
            })
            .collect();
        let parameters = self
            .parameter_table()
            .into_iter()
            .filter(|r| {
                !matches!(
                    r.owner,
                    ParamOwner::Physical(_) | ParamOwner::Reference { .. }
                )
            })
            .map(|r| {
                let now = r.equation();
                Row {
                    key: RowKey::Param(r.name.clone()),
                    label: r.name.clone(),
                    cells: list
                        .iter()
                        .map(|v| Cell::Value {
                            text: v
                                .params
                                .get(&r.name)
                                .cloned()
                                .unwrap_or_else(|| now.clone()),
                            unit: r.unit,
                            value: r.value,
                        })
                        .collect(),
                }
            })
            .collect();
        Table {
            columns: list.iter().map(|v| (v.id, v.name.clone())).collect(),
            active: vs.active,
            factory_scope: vs.factory_scope,
            sections: vec![
                ("Features".into(), features),
                ("Parameters".into(), parameters),
                ("Properties".into(), {
                    let infos: Vec<VariantInfo> = list.iter().map(|v| v.info.clone()).collect();
                    let mut rows = info_rows(&infos);
                    rows.push(Row {
                        key: RowKey::Material,
                        label: "Material".to_owned(),
                        cells: list
                            .iter()
                            .map(|v| Cell::Text(v.material.clone().unwrap_or_default()))
                            .collect(),
                    });
                    rows
                }),
            ],
        }
    }
}

/// The active entry of `list` takes `now`; in Factory Scope the others take what
/// changed. `true` when something changed.
pub fn take_info<T>(
    list: &mut [T],
    active: u32,
    factory: bool,
    now: &VariantInfo,
    info: impl Fn(&mut T) -> &mut VariantInfo,
    id: impl Fn(&T) -> u32,
) -> bool {
    let Some(old) = list
        .iter_mut()
        .find(|t| id(t) == active)
        .map(|t| info(t).clone())
    else {
        return false;
    };
    if old == *now {
        return false;
    }
    for t in list.iter_mut() {
        if id(t) == active {
            *info(t) = now.clone();
        } else if factory {
            let i = info(t);
            for (k, _) in VARIANT_PROPS {
                if old.get(k) != now.get(k) {
                    i.set(k, now.get(k));
                }
            }
            if old.bom != now.bom {
                i.bom = now.bom;
            }
        }
    }
    true
}

/// What a row of the table stands for.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RowKey {
    /// An assembly's occurrence.
    Occurrence(u32),
    /// An assembly's relationship.
    Relationship(u32),
    /// A part's feature, by id.
    Feature(String),
    /// A part's parameter, by name.
    Param(String),
    /// A Property, by key ([`VARIANT_PROPS`]).
    Prop(String),
    /// The document's BOM Structure.
    BomStructure,
    /// A part's material.
    Material,
    /// The value of an assembly's relationship.
    RelValue(u32),
    /// A value of a component pattern: its id, the field.
    Pattern(u32, String),
    /// The variant of the part an occurrence places.
    OccVariant(u32),
    /// An occurrence counted as Reference in the BOM.
    OccReference(u32),
}

/// One cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    /// Suppressed or included.
    Suppressed(bool),
    /// A parameter's equation.
    Value {
        /// As typed.
        text: String,
        /// Its unit.
        unit: Unit,
        /// The value it has now (for the field).
        value: Real,
    },
    /// Free text (a Property, a material's name).
    Text(String),
    /// One of a list.
    Choice {
        /// What can be chosen.
        options: Vec<String>,
        /// What is chosen.
        selected: String,
    },
}

impl Cell {
    /// The same setting, whatever the field's current number says.
    pub fn same(&self, other: &Cell) -> bool {
        match (self, other) {
            (Cell::Suppressed(a), Cell::Suppressed(b)) => a == b,
            (Cell::Value { text: a, .. }, Cell::Value { text: b, .. }) => a == b,
            (Cell::Text(a), Cell::Text(b)) => a == b,
            (Cell::Choice { selected: a, .. }, Cell::Choice { selected: b, .. }) => a == b,
            _ => false,
        }
    }

    /// As in the CSV text and the search.
    pub fn text(&self) -> String {
        match self {
            Cell::Suppressed(true) => "Suppressed".into(),
            Cell::Suppressed(false) => "Included".into(),
            Cell::Value { text, .. } | Cell::Text(text) => text.clone(),
            Cell::Choice { selected, .. } => selected.clone(),
        }
    }
}

/// One row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// What it stands for.
    pub key: RowKey,
    /// Its name.
    pub label: String,
    /// One cell per column.
    pub cells: Vec<Cell>,
}

/// One change of a cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Set {
    /// The row.
    pub key: RowKey,
    /// The variant's id.
    pub col: u32,
    /// The new setting.
    pub cell: Cell,
}

/// The whole table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    /// (variant id, name), `[Primary]` first.
    pub columns: Vec<(u32, String)>,
    /// The active variant.
    pub active: u32,
    /// Edit Factory Scope.
    pub factory_scope: bool,
    /// (title, rows).
    pub sections: Vec<(String, Vec<Row>)>,
}

impl Table {
    fn col_index(&self, id: u32) -> Option<usize> {
        self.columns.iter().position(|c| c.0 == id)
    }

    fn rows(&self) -> impl Iterator<Item = &Row> {
        self.sections.iter().flat_map(|s| s.1.iter())
    }

    /// Does the row differ between the variants?
    pub fn differs(row: &Row) -> bool {
        row.cells.windows(2).any(|w| !w[0].same(&w[1]))
    }

    /// Every variant takes this row's setting from variant `from`.
    pub fn fill_row(&self, key: &RowKey, from: u32) -> Vec<Set> {
        self.spread(std::slice::from_ref(key), from, None)
    }

    /// Variant `to` takes the settings of variant `from`: all rows, or `keys`.
    pub fn copy_column(&self, from: u32, to: u32, keys: Option<&[RowKey]>) -> Vec<Set> {
        let all: Vec<RowKey> = self.rows().map(|r| r.key.clone()).collect();
        self.spread(keys.unwrap_or(&all), from, Some(to))
    }

    fn spread(&self, keys: &[RowKey], from: u32, to: Option<u32>) -> Vec<Set> {
        let Some(f) = self.col_index(from) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for r in self.rows().filter(|r| keys.contains(&r.key)) {
            for (c, (id, _)) in self.columns.iter().enumerate() {
                if c != f && to.is_none_or(|t| t == *id) && !r.cells[c].same(&r.cells[f]) {
                    out.push(Set {
                        key: r.key.clone(),
                        col: *id,
                        cell: r.cells[f].clone(),
                    });
                }
            }
        }
        out
    }

    /// `keys` set to `suppressed` in variant `col` (rows of values are left alone).
    pub fn set_suppressed(&self, keys: &[RowKey], col: u32, suppressed: bool) -> Vec<Set> {
        let Some(c) = self.col_index(col) else {
            return Vec::new();
        };
        self.rows()
            .filter(|r| keys.contains(&r.key))
            .filter(|r| matches!(r.cells[c], Cell::Suppressed(s) if s != suppressed))
            .map(|r| Set {
                key: r.key.clone(),
                col,
                cell: Cell::Suppressed(suppressed),
            })
            .collect()
    }

    /// The table as CSV: a heading, then `section,item,cell…`.
    pub fn to_csv(&self) -> String {
        let field = |s: &str| {
            if s.contains([',', '"', '\n']) {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_owned()
            }
        };
        let mut out = String::from("Section,Item");
        for (_, n) in &self.columns {
            out.push(',');
            out.push_str(&field(n));
        }
        out.push('\n');
        for (title, rows) in &self.sections {
            for r in rows {
                out.push_str(&field(title));
                out.push(',');
                out.push_str(&field(&r.label));
                for c in &r.cells {
                    out.push(',');
                    out.push_str(&field(&c.text()));
                }
                out.push('\n');
            }
        }
        out
    }

    /// What a CSV text changes: only cells that differ; columns and rows are
    /// found by name. An unknown variant or item, or a word that is no setting,
    /// refuses the whole text.
    pub fn from_csv(&self, text: &str) -> Result<Vec<Set>, String> {
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let head = csv_fields(lines.next().ok_or("the text is empty")?);
        if head.len() < 3 {
            return Err("the heading needs Section, Item and at least one variant".into());
        }
        let mut cols = Vec::new();
        for name in &head[2..] {
            let n = name.trim();
            let c = self
                .col_index_named(n)
                .ok_or_else(|| format!("there is no variant '{n}'"))?;
            cols.push(c);
        }
        let mut out = Vec::new();
        for line in lines {
            let f = csv_fields(line);
            let (Some(title), Some(item)) = (f.first(), f.get(1)) else {
                return Err(format!("line '{line}' has no item"));
            };
            let row = self
                .sections
                .iter()
                .filter(|s| s.0 == title.trim())
                .flat_map(|s| s.1.iter())
                .find(|r| r.label == item.trim())
                .ok_or_else(|| format!("there is no {} '{}'", title.trim(), item.trim()))?;
            for (k, c) in cols.iter().enumerate() {
                let Some(t) = f.get(k + 2) else { continue };
                let cell = match (&row.cells[*c], t.trim()) {
                    (Cell::Suppressed(_), "Suppressed") => Cell::Suppressed(true),
                    (Cell::Suppressed(_), "Included") => Cell::Suppressed(false),
                    (Cell::Suppressed(_), other) => {
                        return Err(format!(
                            "{}: '{other}' is neither Suppressed nor Included",
                            row.label
                        ));
                    }
                    (Cell::Value { unit, value, .. }, other) => Cell::Value {
                        text: other.to_owned(),
                        unit: *unit,
                        value: *value,
                    },
                    (Cell::Text(_), other) => Cell::Text(other.to_owned()),
                    (Cell::Choice { options, .. }, other) => {
                        if !options.iter().any(|o| o == other) {
                            return Err(format!(
                                "{}: '{other}' is none of {}",
                                row.label,
                                options.join(", ")
                            ));
                        }
                        Cell::Choice {
                            options: options.clone(),
                            selected: other.to_owned(),
                        }
                    }
                };
                if !row.cells[*c].same(&cell) {
                    out.push(Set {
                        key: row.key.clone(),
                        col: self.columns[*c].0,
                        cell,
                    });
                }
            }
        }
        Ok(out)
    }

    fn col_index_named(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.1 == name)
    }
}

/// The fields of one CSV line (double quotes as in spreadsheets).
fn csv_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{ExtrudeFeature, OriginPlane, ProfileRef, SketchOptions, SketchPlacement};
    use crate::sketch::Point2;

    fn box_part() -> PartDocument {
        PartDocument::with_box("Box1", [20.0, 10.0, 5.0]).unwrap()
    }

    /// A box and a second solid on top of it (an extrusion that can be suppressed).
    fn with_boss() -> PartDocument {
        let mut part = box_part();
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
            .unwrap();
        part.sketches[i]
            .sketch
            .add_circle(Point2::new(4.0, 4.0), 2.0)
            .unwrap();
        let regions = part.sketches[i].sketch.regions();
        part.add_extrude(
            i,
            ExtrudeFeature::new(
                vec![ProfileRef::of(&regions[0], regions[0].interior_point())],
                30.0,
            ),
        )
        .unwrap();
        part
    }

    #[test]
    fn a_variant_keeps_its_parameters_and_primary_its_own() {
        let mut p = box_part();
        let v0 = p.body.bounding_box().unwrap().1.x;
        assert!((v0 - 20.0).abs() < 1e-6, "{v0}");
        let name = p.parameter_table()[0].name.clone();
        let before = p.parameter(&name).unwrap().value;
        let id = p.new_variant();
        assert_eq!(p.variants.list[1].name, "Variant1");
        p.set_parameter_expr(&name, "33 mm").unwrap();
        p.variants_sync();
        assert_eq!(p.parameter(&name).unwrap().value, 33.0);
        p.activate_variant(PRIMARY).unwrap();
        assert_eq!(p.parameter(&name).unwrap().value, before);
        p.activate_variant(id).unwrap();
        assert_eq!(p.parameter(&name).unwrap().value, 33.0);
        // [Primary] and the active one stay.
        assert!(!p.delete_variant(PRIMARY));
        assert!(!p.delete_variant(id));
    }

    #[test]
    fn a_suppressed_feature_changes_the_body_per_variant() {
        let mut p = with_boss();
        let k = p.features.len() - 1;
        let id = p.features[k].id().to_owned();
        let high = p.body.bounding_box().unwrap().1.z;
        let v = p.new_variant();
        p.set_variant_suppressed(v, &id, true).unwrap();
        p.variants_sync();
        let low = p.body.bounding_box().unwrap().1.z;
        assert!(low < high - 1.0, "{low} {high}");
        p.activate_variant(PRIMARY).unwrap();
        assert!((p.body.bounding_box().unwrap().1.z - high).abs() < 1e-6);
        // Set from another variant, not active: nothing moves now.
        p.set_variant_suppressed(v, &id, false).unwrap();
        assert!((p.body.bounding_box().unwrap().1.z - high).abs() < 1e-6);
        p.activate_variant(v).unwrap();
        assert!((p.body.bounding_box().unwrap().1.z - high).abs() < 1e-6);
    }

    #[test]
    fn a_bad_equation_is_refused_for_any_variant_and_changes_nothing() {
        let mut p = box_part();
        let name = p.parameter_table()[0].name.clone();
        let v = p.new_variant();
        p.activate_variant(PRIMARY).unwrap();
        let copy = p.variants.clone();
        assert!(p.set_variant_param(v, &name, "1 +").is_err());
        assert_eq!(p.variants, copy);
        assert_eq!(p.variants.active, PRIMARY);
        // A good one is kept without touching the active variant.
        let now = p.parameter(&name).unwrap().value;
        p.set_variant_param(v, &name, "44 mm").unwrap();
        assert_eq!(p.parameter(&name).unwrap().value, now);
        p.activate_variant(v).unwrap();
        assert_eq!(p.parameter(&name).unwrap().value, 44.0);
    }

    #[test]
    fn factory_scope_reaches_every_variant() {
        let mut p = box_part();
        let name = p.parameter_table()[0].name.clone();
        let v = p.new_variant();
        p.variants.factory_scope = true;
        p.set_parameter_expr(&name, "50 mm").unwrap();
        p.variants_sync();
        p.activate_variant(PRIMARY).unwrap();
        assert_eq!(p.parameter(&name).unwrap().value, 50.0);
        p.activate_variant(v).unwrap();
        assert_eq!(p.parameter(&name).unwrap().value, 50.0);
    }

    #[test]
    fn a_feature_added_in_member_scope_is_suppressed_elsewhere() {
        let mut p = box_part();
        let n = p.features.len();
        let v = p.new_variant();
        let mut q = with_boss();
        std::mem::swap(&mut q.variants, &mut p.variants);
        // The same part with one more feature, added while the variant was active.
        let added = q.features[n].id().to_owned();
        q.variants_sync();
        assert!(!q.variants.get(v).unwrap().suppressed.contains(&added));
        assert!(q.variants.get(PRIMARY).unwrap().suppressed.contains(&added));
    }

    #[test]
    fn the_table_shows_differences_and_its_corrections_reduce_to_sets() {
        let mut p = with_boss();
        let id = p.features[p.features.len() - 1].id().to_owned();
        let v = p.new_variant();
        p.set_variant_suppressed(v, &id, true).unwrap();
        p.variants_sync();
        let t = p.variants_table(&IProperties::default());
        assert_eq!(t.columns.len(), 2);
        assert_eq!(t.active, v);
        let key = RowKey::Feature(id.clone());
        let row = t.sections[0].1.iter().find(|r| r.key == key).unwrap();
        assert!(Table::differs(row));
        assert!(!Table::differs(&t.sections[0].1[0]));
        // Same in all variants: only the other column changes.
        let fill = t.fill_row(&key, v);
        assert_eq!(fill.len(), 1);
        assert_eq!(
            (fill[0].col, &fill[0].cell),
            (PRIMARY, &Cell::Suppressed(true))
        );
        // Back to [Primary]: the copy of one column.
        let back = t.copy_column(PRIMARY, v, None);
        assert_eq!(back.len(), 1);
        assert_eq!((back[0].col, &back[0].cell), (v, &Cell::Suppressed(false)));
        // Nothing to do for a bulk that already holds.
        assert!(
            t.set_suppressed(std::slice::from_ref(&key), v, true)
                .is_empty()
        );
        assert_eq!(t.set_suppressed(&[key], PRIMARY, true).len(), 1);
    }

    #[test]
    fn csv_goes_round_and_refuses_what_it_does_not_know() {
        let mut p = with_boss();
        let v = p.new_variant();
        let t = p.variants_table(&IProperties::default());
        let text = t.to_csv();
        assert!(t.from_csv(&text).unwrap().is_empty(), "{text}");
        // A changed cell is one Set.
        let label = t.sections[0].1[1].label.clone();
        let changed = text.replacen(
            &format!("Features,{label},Included,Included"),
            &format!("Features,{label},Included,Suppressed"),
            1,
        );
        let sets = t.from_csv(&changed).unwrap();
        assert_eq!(sets.len(), 1);
        assert_eq!((sets[0].col, &sets[0].cell), (v, &Cell::Suppressed(true)));
        assert!(t.from_csv("Section,Item,Nope\n").is_err());
        assert!(
            t.from_csv("Section,Item,Primary\nFeatures,Nope,Included\n")
                .is_err()
        );
        let bad = format!("Section,Item,Primary\nFeatures,{label},Maybe\n");
        assert!(t.from_csv(&bad).is_err());
        assert_eq!(csv_fields("a,\"b,c\",\"d\"\"e\""), ["a", "b,c", "d\"e"]);
    }

    #[test]
    fn a_variant_has_its_own_material_and_properties() {
        let mut p = box_part();
        let lib = crate::materials::with_user();
        let steel = lib
            .materials
            .iter()
            .map(|m| m.name.clone())
            .find(|n| n != "Generic")
            .unwrap();
        let mut doc = IProperties::default();
        doc.set("part_number", "A-100");
        let v = p.new_variant();
        p.variants_fill_info(&doc);
        assert_eq!(
            p.variants.get(PRIMARY).unwrap().info.get("part_number"),
            "A-100"
        );
        // The active one follows the document.
        doc.set("part_number", "A-200");
        doc.bom_structure = BomStructure::Purchased;
        assert!(p.variants_take_info(&doc));
        assert_eq!(p.active_variant_info().unwrap().get("part_number"), "A-200");
        assert_eq!(
            p.variants.get(PRIMARY).unwrap().info.get("part_number"),
            "A-100"
        );
        assert_eq!(
            p.variants.get(PRIMARY).unwrap().info.bom,
            BomStructure::Normal
        );
        // Material, from the table's cell.
        p.set_variant_info(v, &RowKey::Material, &Cell::Text(steel.clone()))
            .unwrap();
        p.variants_sync();
        assert_eq!(p.material.as_deref(), Some(steel.as_str()));
        p.activate_variant(PRIMARY).unwrap();
        assert_eq!(p.material, None);
        p.activate_variant(v).unwrap();
        assert_eq!(p.material.as_deref(), Some(steel.as_str()));
        // Not in the library: refused, for any variant.
        let before = p.variants.clone();
        assert!(
            p.set_variant_info(PRIMARY, &RowKey::Material, &Cell::Text("Nothing".into()))
                .is_err()
        );
        assert_eq!(p.variants, before);
        // The table lists them; a word that is no BOM Structure is refused.
        let t = p.variants_table(&doc);
        let props = &t.sections[2].1;
        assert_eq!(props[0].cells[0], Cell::Text("A-100".into()));
        assert!(
            t.from_csv("Section,Item,Primary\nProperties,BOM Structure,Maybe\n")
                .is_err()
        );
        assert_eq!(
            t.from_csv("Section,Item,Primary\nProperties,BOM Structure,Phantom\n")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn a_part_without_variants_writes_nothing_and_they_round_trip() {
        let mut p = box_part();
        assert!(!serde_json::to_string(&p).unwrap().contains("variants"));
        p.new_variant();
        p.variants_sync();
        let text = serde_json::to_string(&p).unwrap();
        let q: PartDocument = serde_json::from_str(&text).unwrap();
        assert_eq!(p.variants, q.variants);
    }
}
