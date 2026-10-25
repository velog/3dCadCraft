//! # History editing (Model Browser)
//!
//! The part's history is [`PartDocument::features`] in order, with the
//! sketches placed between them by [`PartSketch::after_features`]. This
//! module edits it the way Inventor's browser does
//! (`docs/spec/2.16-browser.md`):
//!
//! - **End of Part** ([`PartDocument::eop`]): features at and after it are
//!   rolled back (not built); new features and sketches are inserted there
//!   ([`PartDocument::insert_feature`], [`PartDocument::insert_index`]);
//! - **reorder** ([`PartDocument::move_feature`]): refused when a feature
//!   would come before what it references ([`PartDocument::feature_refs`]);
//!   rolled back when the part no longer builds;
//! - **Suppress / Unsuppress** ([`PartDocument::suppress_feature`]): the
//!   feature and everything that depends on it are skipped by regeneration;
//! - **Delete** with Inventor's options ([`DeleteOptions`]);
//! - **Rename** of the display name (never the id, rule 3).
//!
//! Dependencies are found from persistent names: a feature (or a sketch's
//! placement and projections) references feature `F` when one of its
//! strings is `F`'s id (a body, a pattern source, a work feature) or starts
//! with `"F:"` (a face, edge or vertex of `F`).

use std::collections::{BTreeSet, HashSet};

use super::{Feature, PartDocument, PartError, PartSketch};

/// Inventor's **Delete Features** options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeleteOptions {
    /// Also delete the sketches only the deleted features consume
    /// (otherwise they become free, visible sketches).
    pub consumed_sketches: bool,
    /// Also delete the features and free sketches that reference the
    /// deleted ones (otherwise they stay and become sick).
    pub dependents: bool,
}

impl Default for DeleteOptions {
    /// Inventor's defaults: both on.
    fn default() -> Self {
        Self {
            consumed_sketches: true,
            dependents: true,
        }
    }
}

/// What a delete would remove (for the confirmation dialog).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeletePlan {
    /// Feature indices to remove, ascending.
    pub features: Vec<usize>,
    /// Sketch indices to remove, ascending.
    pub sketches: Vec<usize>,
    /// Consumed sketches kept as free sketches.
    pub freed: Vec<usize>,
}

/// The strings of a JSON value (all nesting levels); `skip` names object
/// keys left out at the top level.
fn strings<'a>(v: &'a serde_json::Value, skip: &[&str], out: &mut Vec<&'a str>) {
    match v {
        serde_json::Value::String(s) => out.push(s),
        serde_json::Value::Array(a) => a.iter().for_each(|x| strings(x, &[], out)),
        serde_json::Value::Object(o) => {
            for (k, x) in o {
                if !skip.contains(&k.as_str()) {
                    strings(x, &[], out);
                }
            }
        }
        _ => {}
    }
}

/// Does the persistent name `s` belong to (or name) feature `id`?
fn names_feature(s: &str, id: &str) -> bool {
    s.strip_prefix(id).is_some_and(|rest| {
        rest.is_empty() || rest.starts_with(|c: char| !c.is_ascii_alphanumeric() && c != '_')
    })
    // A piece of a face divided by Split Face: `<face>~<split id>.<k>`.
    || s.contains(&format!("~{id}."))
}

/// The model references of a sketch: its placement and projections.
fn sketch_json(ps: &PartSketch) -> Vec<serde_json::Value> {
    [
        serde_json::to_value(&ps.placement).unwrap_or_default(),
        serde_json::to_value(&ps.projections).unwrap_or_default(),
    ]
    .into()
}

impl PartDocument {
    /// The End of Part as an index: features `..eop_index()` are built.
    pub fn eop_index(&self) -> usize {
        self.eop
            .unwrap_or(self.features.len())
            .min(self.features.len())
    }

    /// The features built by regeneration: before both the End of Part and
    /// Edit Feature's rollback.
    pub fn build_limit(&self) -> usize {
        self.rollback.unwrap_or(usize::MAX).min(self.eop_index())
    }

    /// Where a new feature or sketch goes in the history: at the End of
    /// Part (Inventor inserts above the EOP marker).
    pub fn insert_index(&self) -> usize {
        self.eop_index()
    }

    /// Index of the feature an `add_*` just inserted (right above the End
    /// of Part) — never assume it is the last one.
    pub fn last_inserted(&self) -> usize {
        self.insert_index().saturating_sub(1)
    }

    /// Is feature `k` below the End of Part (not built)?
    pub fn is_rolled_back(&self, k: usize) -> bool {
        k >= self.eop_index()
    }

    /// Is sketch `index` below the End of Part (made after it)?
    pub fn sketch_rolled_back(&self, index: usize) -> bool {
        let e = self.eop_index();
        self.sketches
            .get(index)
            .is_some_and(|ps| match ps.after_features {
                Some(k) => k > e,
                None => e < self.features.len(),
            })
    }

    /// Is feature `k` suppressed?
    pub fn is_suppressed(&self, k: usize) -> bool {
        self.features.get(k).is_some_and(|f| {
            self.suppressed.contains(f.id()) || self.cond_suppressed.contains(f.id())
        })
    }

    /// Index of the feature with persistent id `id`.
    pub fn feature_index(&self, id: &str) -> Option<usize> {
        self.features.iter().position(|f| f.id() == id)
    }

    /// Inserts `f` at the End of Part (or at the end) **without**
    /// rebuilding and returns its index. Sketches after the insertion point
    /// keep their place relative to the features around them; the End of
    /// Part stays below the new feature. Every `add_*` goes through here.
    pub fn insert_feature(&mut self, f: Feature) -> usize {
        let k = self.insert_index();
        self.features.insert(k, f);
        for ps in &mut self.sketches {
            if let Some(n) = ps.after_features.as_mut()
                && *n > k
            {
                *n += 1;
            }
        }
        if self.eop.is_some() {
            self.eop = Some(k + 1);
        }
        self.normalize_eop();
        k
    }

    /// [`Self::insert_feature`], regenerate, and if the new feature fails
    /// put everything back (rule 5) and return its problem.
    pub(crate) fn insert_feature_checked(&mut self, f: Feature) -> Result<usize, PartError> {
        let id = f.id().to_owned();
        let before = self.history_snapshot();
        let k = self.insert_feature(f);
        self.regenerate()?;
        if let Some(msg) = self.feature_health.get(&id).cloned() {
            self.restore_history(before)?;
            return Err(PartError::Feature(msg));
        }
        Ok(k)
    }

    /// `eop == Some(len)` is the same as no marker: keep one spelling.
    fn normalize_eop(&mut self) {
        if self.eop.is_some_and(|e| e >= self.features.len()) {
            self.eop = None;
        }
    }

    /// **Move EOP**: to `at` (0 = top, `features.len()` = end; Move EOP
    /// Here on feature k = `k + 1`) and rebuilds.
    pub fn set_eop(&mut self, at: usize) -> Result<(), PartError> {
        let before = self.history_snapshot();
        self.eop = Some(at.min(self.features.len()));
        self.normalize_eop();
        if let Err(e) = self.regenerate() {
            self.restore_history(before)?;
            return Err(e);
        }
        Ok(())
    }

    /// **Delete All Features Below EOP**: removes the rolled-back features
    /// and the sketches made after the End of Part.
    pub fn delete_below_eop(&mut self) -> Result<Vec<String>, PartError> {
        let e = self.eop_index();
        let mut names = Vec::new();
        let sketches: Vec<usize> = (0..self.sketches.len())
            .filter(|&i| {
                let users = self.consumers(&self.sketches[i].id);
                self.sketch_rolled_back(i) || (!users.is_empty() && users.iter().all(|&k| k >= e))
            })
            .collect();
        for &i in sketches.iter().rev() {
            names.push(self.sketches.remove(i).name);
        }
        while self.features.len() > e {
            if let Some(f) = self.remove_feature(e) {
                names.push(f.name().to_owned());
            }
        }
        self.eop = None;
        self.regenerate()?;
        Ok(names)
    }

    /// The other features feature `k` references (its faces, edges,
    /// bodies, work features, pattern sources, and those of its sketch's
    /// placement and projections), by index.
    pub fn feature_refs(&self, k: usize) -> Vec<usize> {
        let Some(f) = self.features.get(k) else {
            return Vec::new();
        };
        let fj = serde_json::to_value(f).unwrap_or_default();
        let mut s = Vec::new();
        strings(&fj, &["id", "name", "type", "sketch"], &mut s);
        let sk: Vec<serde_json::Value> = f
            .sketch_ids()
            .into_iter()
            .filter_map(|id| self.sketches.iter().find(|p| p.id == id))
            .flat_map(sketch_json)
            .collect();
        for v in &sk {
            strings(v, &[], &mut s);
        }
        self.matching(&s, Some(k))
    }

    /// The features sketch `index` references (placement, projections).
    pub fn sketch_refs(&self, index: usize) -> Vec<usize> {
        let Some(ps) = self.sketches.get(index) else {
            return Vec::new();
        };
        let vals = sketch_json(ps);
        let mut s = Vec::new();
        for v in &vals {
            strings(v, &[], &mut s);
        }
        self.matching(&s, None)
    }

    fn matching(&self, strings: &[&str], not: Option<usize>) -> Vec<usize> {
        (0..self.features.len())
            .filter(|&j| Some(j) != not)
            .filter(|&j| {
                let id = self.features[j].id();
                strings.iter().any(|s| names_feature(s, id))
            })
            .collect()
    }

    /// Every feature that depends on feature `k`, directly or through
    /// others (ascending).
    pub fn dependents(&self, k: usize) -> Vec<usize> {
        let refs: Vec<Vec<usize>> = (0..self.features.len())
            .map(|j| self.feature_refs(j))
            .collect();
        let mut out: BTreeSet<usize> = BTreeSet::new();
        let mut todo = vec![k];
        while let Some(x) = todo.pop() {
            for (j, r) in refs.iter().enumerate() {
                if j != k && r.contains(&x) && out.insert(j) {
                    todo.push(j);
                }
            }
        }
        out.into_iter().collect()
    }

    /// The features `k` needs, directly or through others (ascending).
    fn requirements(&self, k: usize) -> Vec<usize> {
        let mut out: BTreeSet<usize> = BTreeSet::new();
        let mut todo = vec![k];
        while let Some(x) = todo.pop() {
            for j in self.feature_refs(x) {
                if j != k && out.insert(j) {
                    todo.push(j);
                }
            }
        }
        out.into_iter().collect()
    }

    /// Order problems of the history: a feature before what it references
    /// or before its sketch, a sketch before the features it is placed on.
    fn order_problems(&self) -> BTreeSet<String> {
        let n = self.features.len();
        let mut out = BTreeSet::new();
        for k in 0..n {
            let f = &self.features[k];
            for j in self.feature_refs(k) {
                if j > k {
                    out.insert(format!(
                        "{} uses {}, which would come after it",
                        f.name(),
                        self.features[j].name()
                    ));
                }
            }
            for ps in f
                .sketch_ids()
                .into_iter()
                .filter_map(|id| self.sketches.iter().find(|p| p.id == id))
            {
                if ps.after_features.is_some_and(|p| p > k) {
                    out.insert(format!(
                        "{} uses {}, which would come after it",
                        f.name(),
                        ps.name
                    ));
                }
            }
        }
        for (i, ps) in self.sketches.iter().enumerate() {
            let Some(p) = ps.after_features else {
                continue;
            };
            for j in self.sketch_refs(i) {
                if j >= p {
                    out.insert(format!(
                        "{} lies on {}, which would come after it",
                        ps.name,
                        self.features[j].name()
                    ));
                }
            }
        }
        out
    }

    /// **Reorder** (drag in the browser): moves feature `from` so that it
    /// ends up at index `to`, then rebuilds. Sketches keep their place
    /// between the other features; a sketch only the moved feature uses
    /// moves up with it when needed, and one placed on it moves down with
    /// it. Refused (nothing changes) when a feature would come before what
    /// it references, when the move crosses the End of Part (move the
    /// marker instead), or when the part no longer builds as before.
    pub fn move_feature(&mut self, from: usize, to: usize) -> Result<(), PartError> {
        let n = self.features.len();
        if from >= n || to >= n {
            return Err(PartError::Feature("the feature no longer exists".into()));
        }
        if from == to {
            return Ok(());
        }
        let e = self.eop_index();
        if (from < e) != (to < e) {
            return Err(PartError::Feature(
                "a feature cannot be moved across the End of Part (move the marker instead)".into(),
            ));
        }
        let problems = self.order_problems();
        let before = self.history_snapshot();
        let health = self.health_snapshot();
        let moved_id = self.features[from].id().to_owned();
        let own: HashSet<String> = self.features[from]
            .sketch_ids()
            .into_iter()
            .filter(|s| {
                let c = self.consumers(s);
                c == [from] && !self.sketches.iter().any(|p| p.id == *s && p.shared)
            })
            .map(str::to_owned)
            .collect();
        let on_moved: Vec<bool> = (0..self.sketches.len())
            .map(|i| self.sketch_refs(i).contains(&from))
            .collect();
        let f = self.features.remove(from);
        self.features.insert(to, f);
        for (i, ps) in self.sketches.iter_mut().enumerate() {
            let Some(p) = ps.after_features else {
                continue;
            };
            // Anchored to the features that do not move.
            let c = p - usize::from(from < p);
            let after = to < c || (to == c && from < to);
            let mut q = c + usize::from(after);
            if own.contains(&ps.id) {
                q = q.min(to);
            }
            if on_moved[i] && q <= to {
                q = to + 1;
            }
            ps.after_features = Some(q.min(n));
        }
        let new = self.order_problems();
        if let Some(p) = new.difference(&problems).next() {
            let msg = p.clone();
            self.restore_history(before)?;
            return Err(PartError::Feature(msg));
        }
        if let Err(e) = self.regenerate() {
            self.restore_history(before)?;
            return Err(e);
        }
        if let Some(why) = self.new_problems(&health) {
            self.restore_history(before)?;
            return Err(PartError::Feature(format!(
                "moving {moved_id} breaks the part: {why}"
            )));
        }
        Ok(())
    }

    /// **Suppress Features**: suppresses feature `k` and every feature that
    /// depends on it, then rebuilds. Returns the ids newly suppressed.
    pub fn suppress_feature(&mut self, k: usize) -> Result<Vec<String>, PartError> {
        let Some(f) = self.features.get(k) else {
            return Err(PartError::Feature("the feature no longer exists".into()));
        };
        let mut ids = vec![f.id().to_owned()];
        ids.extend(
            self.dependents(k)
                .into_iter()
                .map(|j| self.features[j].id().to_owned()),
        );
        let before = self.history_snapshot();
        let added: Vec<String> = ids
            .into_iter()
            .filter(|id| self.suppressed.insert(id.clone()))
            .collect();
        if let Err(e) = self.regenerate() {
            self.restore_history(before)?;
            return Err(e);
        }
        Ok(added)
    }

    /// **Unsuppress Features**: unsuppresses feature `k`, the suppressed
    /// features it needs and the suppressed features that depend on it,
    /// then rebuilds. Returns the ids unsuppressed.
    pub fn unsuppress_feature(&mut self, k: usize) -> Result<Vec<String>, PartError> {
        if k >= self.features.len() {
            return Err(PartError::Feature("the feature no longer exists".into()));
        }
        let mut all = vec![k];
        all.extend(self.requirements(k));
        all.extend(self.dependents(k));
        let before = self.history_snapshot();
        let removed: Vec<String> = all
            .into_iter()
            .map(|j| self.features[j].id().to_owned())
            .filter(|id| self.suppressed.remove(id))
            .collect();
        if let Err(e) = self.regenerate() {
            self.restore_history(before)?;
            return Err(e);
        }
        Ok(removed)
    }

    /// What **Delete** of feature `k` removes with `opts`.
    pub fn delete_plan(&self, k: usize, opts: DeleteOptions) -> DeletePlan {
        if k >= self.features.len() {
            return DeletePlan::default();
        }
        let mut feats: BTreeSet<usize> = BTreeSet::from([k]);
        if opts.dependents {
            feats.extend(self.dependents(k));
        }
        let mut sketches = BTreeSet::new();
        let mut freed = BTreeSet::new();
        for (i, ps) in self.sketches.iter().enumerate() {
            let users = self.consumers(&ps.id);
            let consumed = !users.is_empty() && users.iter().all(|u| feats.contains(u));
            if consumed {
                if opts.consumed_sketches {
                    sketches.insert(i);
                } else {
                    freed.insert(i);
                }
            } else if opts.dependents
                && users.is_empty()
                && self.sketch_refs(i).iter().any(|j| feats.contains(j))
            {
                sketches.insert(i);
            }
        }
        DeletePlan {
            features: feats.into_iter().collect(),
            sketches: sketches.into_iter().collect(),
            freed: freed.into_iter().collect(),
        }
    }

    /// **Delete** feature `k` with Inventor's options, then rebuild. Kept
    /// dependants become sick (visible in the browser), never silently
    /// changed. Returns the names removed.
    pub fn delete_features(
        &mut self,
        k: usize,
        opts: DeleteOptions,
    ) -> Result<Vec<String>, PartError> {
        if k >= self.features.len() {
            return Err(PartError::Feature("the feature no longer exists".into()));
        }
        let plan = self.delete_plan(k, opts);
        let before = self.history_snapshot();
        let mut names = Vec::new();
        for &i in &plan.freed {
            let ps = &mut self.sketches[i];
            ps.shared = false;
            ps.visible = true;
        }
        for &j in plan.features.iter().rev() {
            if let Some(f) = self.remove_feature(j) {
                names.push(f.name().to_owned());
            }
        }
        for &i in plan.sketches.iter().rev() {
            names.push(self.sketches.remove(i).name);
        }
        if let Err(e) = self.regenerate() {
            self.restore_history(before)?;
            return Err(e);
        }
        names.reverse();
        Ok(names)
    }

    /// **Rename** feature `k` (display name only; the id and every
    /// reference stay). Names must be unique among the features.
    pub fn rename_feature(&mut self, k: usize, name: &str) -> Result<(), PartError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(PartError::Feature("the name cannot be empty".into()));
        }
        if self
            .features
            .iter()
            .enumerate()
            .any(|(j, f)| j != k && f.name() == name)
        {
            return Err(PartError::Feature(format!(
                "the name {name} is already used"
            )));
        }
        let f = self
            .features
            .get_mut(k)
            .ok_or_else(|| PartError::Feature("the feature no longer exists".into()))?;
        *f.name_mut() = name.to_owned();
        Ok(())
    }

    /// **Rename** sketch `index` (display name only).
    pub fn rename_sketch(&mut self, index: usize, name: &str) -> Result<(), PartError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(PartError::Feature("the name cannot be empty".into()));
        }
        if self
            .sketches
            .iter()
            .enumerate()
            .any(|(j, s)| j != index && s.name == name)
        {
            return Err(PartError::Feature(format!(
                "the name {name} is already used"
            )));
        }
        self.sketches
            .get_mut(index)
            .ok_or(PartError::NoSketch(index))?
            .name = name.to_owned();
        Ok(())
    }

    /// The history state that a failed edit restores.
    fn history_snapshot(&self) -> HistorySnapshot {
        HistorySnapshot {
            features: self.features.clone(),
            sketches: self.sketches.clone(),
            eop: self.eop,
            suppressed: self.suppressed.clone(),
            body: self.body.clone(),
        }
    }

    fn restore_history(&mut self, s: HistorySnapshot) -> Result<(), PartError> {
        self.features = s.features;
        self.sketches = s.sketches;
        self.eop = s.eop;
        self.suppressed = s.suppressed;
        self.body = s.body;
        self.regenerate()
    }

    fn health_snapshot(&self) -> HealthSnapshot {
        HealthSnapshot {
            features: self.feature_health.keys().cloned().collect(),
            sketches: self
                .sketches
                .iter()
                .filter(|s| s.health.is_some())
                .map(|s| s.id.clone())
                .collect(),
        }
    }

    /// The first feature or sketch that is sick now but was not in `old`.
    fn new_problems(&self, old: &HealthSnapshot) -> Option<String> {
        for f in &self.features {
            if !old.features.contains(f.id())
                && let Some(h) = self.feature_health.get(f.id())
            {
                return Some(format!("{}: {h}", f.name()));
            }
        }
        for s in &self.sketches {
            if !old.sketches.contains(&s.id)
                && let Some(h) = &s.health
            {
                return Some(format!("{}: {h}", s.name));
            }
        }
        None
    }
}

struct HistorySnapshot {
    features: Vec<Feature>,
    sketches: Vec<PartSketch>,
    eop: Option<usize>,
    suppressed: BTreeSet<String>,
    body: crate::kernel::BRep,
}

struct HealthSnapshot {
    features: HashSet<String>,
    sketches: HashSet<String>,
}

impl Feature {
    /// The display name, for renaming.
    pub fn name_mut(&mut self) -> &mut String {
        match self {
            Feature::Box(b) => &mut b.name,
            Feature::Extrude(e) => &mut e.name,
            Feature::Revolve(r) => &mut r.name,
            Feature::Work(w) => &mut w.name,
            Feature::Blend(b) => &mut b.name,
            Feature::Shell(s) => &mut s.name,
            Feature::Draft(d) => &mut d.name,
            Feature::Pattern(p) => &mut p.name,
            Feature::Hole(h) => &mut h.name,
            Feature::Thread(t) => &mut t.name,
            Feature::Combine(c) => &mut c.name,
            Feature::Split(sp) => &mut sp.name,
            Feature::Sweep(s) => &mut s.name,
            Feature::Loft(l) => &mut l.name,
            Feature::Coil(c) => &mut c.name,
            Feature::Rib(r) => &mut r.name,
            Feature::SheetFace(f) => &mut f.name,
            Feature::SheetFlange(f) => &mut f.name,
            Feature::SheetConvert(f) => &mut f.name,
        }
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
