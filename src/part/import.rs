//! **Import** (`docs/spec/7.7a-import.md`, format revision 42): a STEP file's bodies brought into
//! the part as a base feature, the [`BodyOp::Import`](super::BodyOp::Import) variant (Inventor's
//! AnyCAD *Convert Model*).
//!
//! The file is embedded (rule 8: the part regenerates without it). Bodies are named after the
//! feature (`Import1`, `Import1#2` …) by their order in the file, faces by their STEP face entity
//! (`Import1:F123`), so names never depend on the object filters or on a rebuild.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::exact::Fail::{self, User};
use crate::kernel::exact::{self, ExactSolid};

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// **Import** (Convert Model).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportOp {
    /// The source file's path (shown in the panel; not read again).
    #[serde(default)]
    pub file: String,
    /// The embedded STEP text (shared by clones of the part).
    pub text: Arc<str>,
    /// Object Filters: closed bodies as solids.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub solids: bool,
    /// Object Filters: open shells as surface bodies.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub surfaces: bool,
}

impl ImportOp {
    /// An import of STEP `text` read from `file`, every object kind on.
    pub fn new(file: impl Into<String>, text: impl Into<Arc<str>>) -> Self {
        Self {
            file: file.into(),
            text: text.into(),
            solids: true,
            surfaces: true,
        }
    }

    /// The file's name without its folder.
    pub fn file_name(&self) -> &str {
        self.file
            .rsplit(['/', '\\'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("the embedded file")
    }

    /// Refuses a definition with nothing to read.
    pub fn validate(&self) -> Result<(), String> {
        if self.text.trim().is_empty() {
            return Err("choose a STEP file (Browse...)".into());
        }
        if !self.solids && !self.surfaces {
            return Err("turn on Solids or Surfaces".into());
        }
        Ok(())
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        format!("Import of {}", self.file_name())
    }

    /// The bodies the filters keep (and the reader's warnings).
    pub fn read(&self, id: &str) -> Result<(Vec<ExactSolid>, Vec<String>), String> {
        let r = exact::step_bodies(&self.text, id)?;
        let total = r.bodies.len();
        let kept: Vec<ExactSolid> = r
            .bodies
            .into_iter()
            .filter(|b| if b.closed { self.solids } else { self.surfaces })
            .map(|b| b.solid)
            .collect();
        if kept.is_empty() {
            return Err(match (total, self.solids) {
                (0, _) => "the file has no bodies the reader could build".into(),
                (_, true) => "the file has no solids (turn on Surfaces?)".into(),
                (_, false) => "the file has no surface bodies (turn on Solids?)".into(),
            });
        }
        Ok((kept, r.warnings))
    }
}

/// Import after the solids `cur` and the surface bodies `sheets` built so far: every body after
/// it.
pub(super) fn import(
    op: &ImportOp,
    id: &str,
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    op.validate().map_err(User)?;
    let (made, _) = op.read(id).map_err(User)?;
    Ok(cur.iter().chain(sheets).cloned().chain(made).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{BodyOp, BodyOpFeature, Feature, PartDocument};

    /// A filleted box exported to STEP.
    fn step_of_part() -> (String, PartDocument) {
        let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).expect("box");
        p.regenerate().expect("regenerate");
        let text = exact::step_text(&p.exact_bodies.0, "Box").expect("export");
        (text, p)
    }

    fn solid_volume(p: &PartDocument) -> f64 {
        p.exact_bodies
            .0
            .iter()
            .filter(|s| !s.is_open())
            .filter_map(|s| s.volume_at(1e-3))
            .sum()
    }

    #[test]
    fn an_exported_part_comes_back_with_its_volume_and_faces() {
        let (text, src) = step_of_part();
        let mut p = PartDocument::default();
        p.add_body_op(BodyOpFeature::new(BodyOp::Import(ImportOp::new(
            "C:\\models\\box.step",
            text.as_str(),
        ))))
        .expect("import");
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        let v = solid_volume(&p);
        assert!((v - 24000.0).abs() < 1e-3 * 24000.0, "volume {v}");
        assert_eq!(p.exact_bodies.0.len(), 1);
        let names = p.exact_bodies.0[0].names();
        assert_eq!(names.len(), src.exact_bodies.0[0].names().len());
        assert!(
            names
                .iter()
                .all(|n| n.as_deref().is_some_and(|n| n.starts_with("Import1:F"))),
            "{names:?}"
        );
        // Every face has its own STEP entity.
        let mut uniq: Vec<_> = names.iter().flatten().collect();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 6);
        // The body shows in the display B-rep under the same names.
        assert_eq!(
            p.body
                .faces()
                .filter(|(_, f)| f
                    .name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("Import1:F")))
                .count(),
            6
        );
    }

    #[test]
    fn names_stay_through_a_rebuild_and_a_save() {
        let (text, _) = step_of_part();
        let mut p = PartDocument::default();
        p.add_body_op(BodyOpFeature::new(BodyOp::Import(ImportOp::new(
            "box.stp",
            text.as_str(),
        ))))
        .expect("import");
        let before = p.exact_bodies.0[0].names().to_vec();
        let json = serde_json::to_string(&p).expect("save");
        let mut back: PartDocument = serde_json::from_str(&json).expect("load");
        back.regenerate().expect("regenerate");
        assert_eq!(back.exact_bodies.0[0].names(), before.as_slice());
        let Some(Feature::BodyOp(b)) = back.features.first() else {
            panic!("no import feature");
        };
        let BodyOp::Import(op) = &b.op else {
            panic!("not an import");
        };
        assert_eq!(&*op.text, text.as_str());
        assert_eq!(op.file_name(), "box.stp");
    }

    #[test]
    fn features_build_on_imported_faces() {
        let (text, _) = step_of_part();
        let mut p = PartDocument::default();
        p.add_body_op(BodyOpFeature::new(BodyOp::Import(ImportOp::new(
            "box.step",
            text.as_str(),
        ))))
        .expect("import");
        // A fillet on an imported edge (between two of its faces).
        let edge = p
            .body
            .edges()
            .find_map(|(e, _)| crate::part::blend::edge_ref(&p.body, e))
            .expect("an edge between two named faces");
        assert!(
            edge.faces.iter().all(|f| f.starts_with("Import1:F")),
            "{edge:?}"
        );
        let before = solid_volume(&p);
        let mut q = p.clone();
        q.build_cache = Default::default();
        q.add_blend(crate::part::blend::BlendFeature::fillet(vec![edge], 2.0))
            .expect("fillet");
        let after = solid_volume(&q);
        assert!(after < before - 1.0, "{before} -> {after}");
        assert!(q.feature_health.is_empty(), "{:?}", q.feature_health);
    }

    #[test]
    fn a_bad_file_leaves_the_part_unchanged() {
        let mut p = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).expect("box");
        p.regenerate().expect("regenerate");
        let n = p.features.len();
        let r = p.add_body_op(BodyOpFeature::new(BodyOp::Import(ImportOp::new(
            "x.step",
            "this is not STEP",
        ))));
        assert!(r.is_err());
        assert_eq!(p.features.len(), n);
        let off = ImportOp {
            solids: false,
            surfaces: false,
            ..ImportOp::new("x.step", "ISO-10303-21;")
        };
        assert!(off.validate().is_err());
    }

    #[test]
    fn the_filters_pick_solids_or_surfaces() {
        let (text, _) = step_of_part();
        let op = ImportOp {
            solids: false,
            ..ImportOp::new("box.step", text.as_str())
        };
        let e = op.read("Import1").expect_err("no surfaces in a closed box");
        assert!(e.contains("no surface bodies"), "{e}");
    }
}
