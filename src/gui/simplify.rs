//! Simplify in the app (spec 2.18 part, 3.11 assembly): the panels' presets, OK and Edit
//! Feature. The part's OK makes a new model state (a variant whose Simplify feature is
//! active), a Simplify feature in the active state, or a new part of the design file that
//! derives this one simplified; the assembly's OK makes such a part from the assembly.

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::part_planned::{PartDialog, PartPlanned, Planned};
use super::select::SelItem;
use crate::document::{DocRef, DocumentData};
use crate::part::derive::DeriveStyle;
use crate::part::simplify::{Envelope, EnvelopeType, Preset, Removal, RemoveMode, SimplifySpec};
use crate::part::{BodyOp, Feature};

/// The feature rows: the combo's label and the label of its size field.
const RULES: [(&str, &str); 5] = [
    ("Holes", "Holes up to"),
    ("Fillets", "Fillets up to"),
    ("Chamfers", "Chamfers up to"),
    ("Pockets", "Pockets up to"),
    ("Embosses", "Embosses up to"),
];

fn mode_of(i: usize) -> RemoveMode {
    match i {
        1 => RemoveMode::All,
        2 => RemoveMode::Range,
        3 => RemoveMode::Preserve,
        _ => RemoveMode::None,
    }
}

fn index_of(m: RemoveMode) -> usize {
    match m {
        RemoveMode::None => 0,
        RemoveMode::All => 1,
        RemoveMode::Range => 2,
        RemoveMode::Preserve => 3,
    }
}

fn preset_of(i: usize) -> Preset {
    match i {
        1 => Preset::Least,
        2 => Preset::Moderate,
        3 => Preset::Most,
        _ => Preset::None,
    }
}

fn type_of(i: usize) -> EnvelopeType {
    match i {
        1 => EnvelopeType::OrientedBox,
        2 => EnvelopeType::Cylinder,
        _ => EnvelopeType::Box,
    }
}

fn type_index(t: EnvelopeType) -> usize {
    match t {
        EnvelopeType::Box => 0,
        EnvelopeType::OrientedBox => 1,
        EnvelopeType::Cylinder => 2,
    }
}

/// Where each group of options sits in the two panels: (tab of envelopes, of exclusion, of
/// features, of the advanced options).
fn tabs(d: &PartDialog) -> (usize, usize, usize, usize) {
    if d.kind == Planned::Asm(AsmPlanned::Simplify) {
        (1, 2, 3, 5)
    } else {
        (0, 0, 0, 0)
    }
}

impl CadCraftApp {
    /// The model's size (its box diagonal, mm): presets scale with it.
    fn simplify_model_size(&self, asm: bool) -> f64 {
        let boxes: Vec<(crate::kernel::Point3, crate::kernel::Point3)> = if asm {
            self.derive_parts(DocRef::local(self.part_id), 0)
                .unwrap_or_default()
                .iter()
                .filter_map(|(p, t, _)| {
                    let (lo, hi) = p.body.bounding_box()?;
                    let a = t.transform_point(&lo);
                    let b = t.transform_point(&hi);
                    Some((a.inf(&b), a.sup(&b)))
                })
                .collect()
        } else {
            self.part().body.bounding_box().into_iter().collect()
        };
        let Some(first) = boxes.first().copied() else {
            return 100.0;
        };
        let (lo, hi) = boxes
            .iter()
            .fold(first, |(lo, hi), (a, b)| (lo.inf(a), hi.sup(b)));
        (hi - lo).norm()
    }

    /// A preset chosen in the panel fills its fields (once per change, then the user's).
    pub(super) fn simplify_preset_follow(&self, d: &mut PartDialog) {
        let asm = d.kind == Planned::Asm(AsmPlanned::Simplify);
        if !asm && d.kind != Planned::Part(PartPlanned::Simplify) {
            return;
        }
        let k = d.index(0, "Preset");
        if d.simplify_preset == Some(k) {
            return;
        }
        let first = d.simplify_preset.is_none();
        d.simplify_preset = Some(k);
        // Opening the panel (or Edit Feature) keeps what it shows.
        if first {
            return;
        }
        let spec = SimplifySpec::preset(preset_of(k), self.simplify_model_size(asm), asm);
        write_spec(d, &spec);
    }

    /// Edit Feature of a Simplify feature: its panel with the feature's options.
    pub(super) fn simplify_edit(&mut self, k: usize) {
        let Some(Feature::BodyOp(b)) = self.part().features.get(k).cloned() else {
            return;
        };
        let BodyOp::Simplify(o) = &b.op else {
            return;
        };
        let labels: Vec<(String, SelItem)> = o
            .spec
            .exclude
            .iter()
            .map(|n| SelItem::Body(n.clone()))
            .chain(
                o.spec
                    .envelope_bodies
                    .iter()
                    .map(|n| SelItem::Body(n.clone())),
            )
            .map(|i| (self.item_label(&i), i))
            .collect();
        self.start_part_planned(PartPlanned::Simplify);
        let Some(d) = self.part_planned.as_mut() else {
            return;
        };
        d.feature = Some(k);
        d.simplify_preset = Some(0);
        write_spec(d, &o.spec);
        d.set_index(0, "Output", 1);
        for (label, item) in labels {
            let key = match &item {
                SelItem::Body(n) if o.spec.exclude.contains(n) => "excluded",
                _ => "envelope",
            };
            d.items.entry(key).or_default().push((item, None));
            d.picks.entry(key).or_default().push(label);
        }
        self.file_status = Some(Ok(format!("Simplify: editing {} (OK rebuilds it)", b.name)));
    }

    /// The options the panel shows, as a [`SimplifySpec`].
    fn simplify_spec(&self, d: &PartDialog) -> SimplifySpec {
        let (te, tx, tf, ta) = tabs(d);
        let asm = d.kind == Planned::Asm(AsmPlanned::Simplify);
        let mut s = SimplifySpec::default();
        s.envelope = if asm {
            match d.index(te, "Replace with Envelopes") {
                1 => Envelope::All,
                2 => Envelope::EachTop,
                3 => Envelope::EachPart,
                _ => Envelope::None,
            }
        } else {
            match d.index(te, "Envelopes") {
                1 => Envelope::All,
                2 => Envelope::EachBody,
                3 => Envelope::Selected,
                _ => Envelope::None,
            }
        };
        s.envelope_type = type_of(d.index(te, "Type"));
        let bodies = |key: &str| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for (item, at) in d.items_of(key) {
                let n = match item {
                    SelItem::Body(b) => Some(b.clone()),
                    SelItem::Face(f) => self.body_of_face(f, *at),
                    _ => None,
                };
                if let Some(n) = n
                    && !out.contains(&n)
                {
                    out.push(n);
                }
            }
            out
        };
        if !asm && s.envelope == Envelope::Selected {
            s.envelope_bodies = bodies("envelope");
        }
        let by_size = if asm {
            d.index(tx, "Exclude parts") == 1
        } else {
            d.index(tx, "Exclude") == 1
        };
        if by_size {
            s.exclude_size = d.num(tx, "Max. Diagonal");
        }
        if asm {
            // First-level components by their names (Component Priority).
            if let Some(a) = self.assembly() {
                for w in d.picks_of("excluded") {
                    let name = w.split(" · ").next().unwrap_or(w);
                    if let Some(o) = a.occurrences.iter().find(|o| o.name == name) {
                        s.exclude.push(o.id.to_string());
                    }
                }
            }
        } else if !by_size {
            s.exclude = bodies("excluded");
        }
        let rule = |combo: &str, num: &str| Removal {
            mode: mode_of(d.index(tf, combo)),
            max: d.num(tf, num).unwrap_or(0.0),
        };
        s.holes = rule(RULES[0].0, RULES[0].1);
        s.fillets = rule(RULES[1].0, RULES[1].1);
        s.chamfers = rule(RULES[2].0, RULES[2].1);
        s.pockets = rule(RULES[3].0, RULES[3].1);
        s.embosses = rule(RULES[4].0, RULES[4].1);
        // Preserve: a face picked stands for the feature that made it.
        for (item, _) in d.items_of("preserve") {
            if let SelItem::Face(f) | SelItem::Feature(f) = item
                && let Some(id) = f.split(':').next().and_then(|x| x.split(['#', '~']).next())
                && !s.preserve.iter().any(|p| p == id)
            {
                s.preserve.push(id.to_owned());
            }
        }
        if asm {
            s.fill_voids = d.check(ta, "Fill internal voids");
            s.remove_internal = d.check(ta, "Remove internal parts");
        } else {
            s.fill_voids = d.check(ta, "Fill internal voids");
            s.remove_internal = d.check(ta, "Remove internal bodies");
        }
        s
    }

    /// The part's Simplify OK. Returns whether the panel closes.
    pub(super) fn simplify_ok(&mut self, d: &PartDialog) -> bool {
        let spec = self.simplify_spec(d);
        if let Err(e) = spec.validate() {
            self.file_status = Some(Err(format!("Simplify: {e}")));
            return false;
        }
        let name = d.text(0, "Name").trim().to_owned();
        // Edit Feature: the same feature with the new options.
        if let Some(k) = d.feature {
            let Some(Feature::BodyOp(mut b)) = self.part().features.get(k).cloned() else {
                return true;
            };
            b.op = BodyOp::Simplify(crate::part::simplify::SimplifyOp { spec });
            self.checkpoint("Edit Simplify");
            return match self.part_mut().replace_feature(k, Feature::BodyOp(b)) {
                Ok(_) => {
                    self.scene_dirty = true;
                    self.file_status = Some(Ok("Simplify: edited".into()));
                    true
                }
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Simplify: {e}")));
                    false
                }
            };
        }
        match d.index(0, "Output") {
            // A new model state.
            0 => {
                self.checkpoint("Simplify");
                let named = (!name.is_empty()).then_some(name.as_str());
                match self.part_mut().add_simplified_variant(spec, named) {
                    Ok((v, id)) => {
                        let vname = self
                            .part()
                            .variants
                            .get(v)
                            .map(|x| x.name.clone())
                            .unwrap_or_default();
                        self.scene_dirty = true;
                        self.file_status = Some(Ok(format!(
                            "Simplify: model state {vname} made ({id}; the others keep every detail)"
                        )));
                        true
                    }
                    Err(e) => {
                        self.history.cancel();
                        self.file_status = Some(Err(format!("Simplify: {e}")));
                        false
                    }
                }
            }
            // A feature in the active model state.
            1 => {
                self.checkpoint("Simplify");
                match self.part_mut().add_simplify(spec) {
                    Ok(id) => {
                        self.scene_dirty = true;
                        self.file_status = Some(Ok(format!("Simplify: {id} added")));
                        true
                    }
                    Err(e) => {
                        self.history.cancel();
                        self.file_status = Some(Err(format!("Simplify: {e}")));
                        false
                    }
                }
            }
            // A new part that follows this one.
            _ => self.simplify_new_part(d, spec, DeriveStyle::Each, false, &name),
        }
    }

    /// The assembly's Simplify OK (a new part of the design file).
    pub(super) fn asm_simplify_ok(&mut self, d: &PartDialog) -> bool {
        let spec = self.simplify_spec(d);
        let style = match d.index(4, "Style") {
            0 => DeriveStyle::SingleMerged,
            1 => DeriveStyle::Single,
            3 => DeriveStyle::Surfaces,
            _ => DeriveStyle::Each,
        };
        let name = d.text(4, "Name").trim().to_owned();
        let reference = d.index(4, "BOM Structure") == 1;
        let ok = self.simplify_new_part(d, spec, style, d.check(5, "Break link"), &name);
        if ok && reference {
            let id = self.project.documents().last().map(|x| x.id);
            if let Some(doc) = id.and_then(|id| self.project.document_mut(id)) {
                doc.properties.bom_structure = crate::document::iprops::BomStructure::Reference;
            }
        }
        ok
    }

    fn simplify_new_part(
        &mut self,
        _d: &PartDialog,
        spec: SimplifySpec,
        style: DeriveStyle,
        break_link: bool,
        name: &str,
    ) -> bool {
        let source = DocRef::local(self.part_id);
        let source_name = self
            .project
            .document(self.part_id)
            .map(|x| x.name.clone())
            .unwrap_or_default();
        let name = if name.is_empty() {
            format!("{source_name}_Simplified")
        } else {
            name.to_owned()
        };
        let Some(parts) = self.derive_parts(source, 0) else {
            self.file_status = Some(Err("Simplify: the model cannot be read".into()));
            return false;
        };
        let src = super::derive::source_of(&parts);
        let made = crate::assembly::make::simplified_part(
            &src,
            source,
            &source_name,
            spec,
            style,
            break_link,
        );
        match made {
            Ok(p) => {
                self.checkpoint("Simplify");
                self.project.add_document(&name, DocumentData::Part(p));
                self.file_status = Some(Ok(format!(
                    "Simplify: {name} made{} (Manage › Documents opens it)",
                    if break_link {
                        ""
                    } else {
                        "; it follows the source on Update"
                    }
                )));
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Simplify: {e}")));
                false
            }
        }
    }
}

/// Writes `spec` into the panel's fields.
fn write_spec(d: &mut PartDialog, spec: &SimplifySpec) {
    let (te, tx, tf, ta) = tabs(d);
    let asm = d.kind == Planned::Asm(AsmPlanned::Simplify);
    if asm {
        d.set_index(
            te,
            "Replace with Envelopes",
            match spec.envelope {
                Envelope::All => 1,
                Envelope::EachTop => 2,
                Envelope::EachPart | Envelope::EachBody => 3,
                _ => 0,
            },
        );
    } else {
        d.set_index(
            te,
            "Envelopes",
            match spec.envelope {
                Envelope::All | Envelope::EachTop => 1,
                Envelope::EachBody | Envelope::EachPart => 2,
                Envelope::Selected => 3,
                Envelope::None => 0,
            },
        );
    }
    d.set_index(te, "Type", type_index(spec.envelope_type));
    let size_label = if asm { "Exclude parts" } else { "Exclude" };
    d.set_index(tx, size_label, usize::from(spec.exclude_size.is_some()));
    if let Some(x) = spec.exclude_size {
        d.set_num(tx, "Max. Diagonal", x);
    }
    for (r, (combo, num)) in [
        spec.holes,
        spec.fillets,
        spec.chamfers,
        spec.pockets,
        spec.embosses,
    ]
    .iter()
    .zip(RULES)
    {
        d.set_index(tf, combo, index_of(r.mode));
        if r.mode == RemoveMode::Range {
            d.set_num(tf, num, r.max);
        }
    }
    d.set_check(ta, "Fill internal voids", spec.fill_voids);
    d.set_check(
        ta,
        if asm {
            "Remove internal parts"
        } else {
            "Remove internal bodies"
        },
        spec.remove_internal,
    );
}
