//! **Import** (`docs/spec/7.7a-import.md`, format revision 42): a STEP file's bodies brought into
//! the part as a base feature, the [`BodyOp::Import`](super::BodyOp::Import) variant (Inventor's
//! AnyCAD *Convert Model*).
//!
//! The file is embedded (rule 8: the part regenerates without it). Bodies are named after the
//! feature (`Import1`, `Import1#2` …) by their order in the file, faces by their STEP face entity
//! (`Import1:F123`), so names never depend on the object filters or on a rebuild.
//!
//! **Reference Model** (stage I3): the feature also keeps a [`SourceLink`] to the file. When the
//! file changes, [`ImportOp::updated`] reads it again and matches its faces to the old ones by
//! geometry (STEP has no persistent face ids: a re-export renumbers every entity), so a face that
//! is still there keeps its name (`keep`) and features on it survive; a face with no match gets a
//! name no old face had, and what used the old face becomes sick.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::exact::Fail::{self, User};
use crate::kernel::Real;
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
    /// Reference Model: the file it follows (Convert Model: none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<SourceLink>,
    /// Face entity of the embedded text -> the entity whose name it keeps (after an update).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keep: BTreeMap<u64, u64>,
}

/// The source file of a Reference Model import, as it was when last read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLink {
    /// Its path.
    pub path: String,
    /// Its modification time (seconds since 1970) when read.
    #[serde(default)]
    pub modified: u64,
    /// Its size when read.
    #[serde(default)]
    pub len: u64,
}

impl SourceLink {
    /// The link to `path` as it is now.
    pub fn now(path: &str) -> Self {
        let (modified, len) = Self::stamp(path).unwrap_or_default();
        Self {
            path: path.to_owned(),
            modified,
            len,
        }
    }

    /// The file's time and size, `None` when it cannot be read.
    pub fn stamp(path: &str) -> Option<(u64, u64)> {
        let m = std::fs::metadata(path).ok()?;
        let t = m
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        Some((t, m.len()))
    }

    /// Whether the file changed since it was read (a missing file has not: it cannot update).
    pub fn changed(&self) -> bool {
        Self::stamp(&self.path).is_some_and(|s| s != (self.modified, self.len))
    }
}

/// What an update matched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateReport {
    /// Faces of the new file that kept an old name.
    pub matched: usize,
    /// New faces (no old face like them).
    pub new: usize,
    /// Old faces with no match (what used them becomes sick).
    pub lost: usize,
}

/// The STEP entity of a face name `<body>:F<entity>`.
fn entity_of(name: &str) -> Option<u64> {
    name.rsplit_once(":F")?.1.parse().ok()
}

impl ImportOp {
    /// An import of STEP `text` read from `file`, every object kind on.
    pub fn new(file: impl Into<String>, text: impl Into<Arc<str>>) -> Self {
        Self {
            file: file.into(),
            text: text.into(),
            solids: true,
            surfaces: true,
            link: None,
            keep: BTreeMap::new(),
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
        let warnings = r.warnings;
        let mut kept: Vec<ExactSolid> = Vec::new();
        for b in r.bodies {
            if !(if b.closed { self.solids } else { self.surfaces }) {
                continue;
            }
            kept.push(self.renamed(b.solid));
        }
        if kept.is_empty() {
            return Err(match (total, self.solids) {
                (0, _) => "the file has no bodies the reader could build".into(),
                (_, true) => "the file has no solids (turn on Surfaces?)".into(),
                (_, false) => "the file has no surface bodies (turn on Solids?)".into(),
            });
        }
        Ok((kept, warnings))
    }
}

impl ImportOp {
    /// `s` with the names `keep` gives its faces.
    fn renamed(&self, mut s: ExactSolid) -> ExactSolid {
        if self.keep.is_empty() {
            return s;
        }
        let names = s
            .names()
            .iter()
            .map(|n| {
                n.as_ref().map(|n| match n.rsplit_once(":F") {
                    Some((body, e)) => {
                        match e.parse::<u64>().ok().and_then(|e| self.keep.get(&e)) {
                            Some(k) => format!("{body}:F{k}"),
                            None => n.clone(),
                        }
                    }
                    None => n.clone(),
                })
            })
            .collect();
        s.set_names(names);
        s
    }

    /// The same import read from `text` (the changed file): faces matched to the current ones by
    /// geometry (same body, area, centre, plane normal) keep their names. `id` is the feature's id.
    pub fn updated(&self, id: &str, text: Arc<str>) -> Result<(ImportOp, UpdateReport), String> {
        let (old, _) = self.read(id)?;
        let mut next = ImportOp {
            text,
            keep: BTreeMap::new(),
            ..self.clone()
        };
        let (new, _) = next.read(id)?;
        struct Face {
            body: usize,
            entity: u64,
            sig: exact::FaceSignature,
        }
        let faces = |bodies: &[ExactSolid]| -> Vec<Face> {
            let mut out = Vec::new();
            for (k, b) in bodies.iter().enumerate() {
                let sigs = b.face_signatures(b.size() * 1e-3).unwrap_or_default();
                for (n, sig) in b.names().iter().zip(sigs) {
                    if let Some(e) = n.as_deref().and_then(entity_of) {
                        out.push(Face {
                            body: k,
                            entity: e,
                            sig,
                        });
                    }
                }
            }
            out
        };
        let (of, nf) = (faces(&old), faces(&new));
        let size = old
            .iter()
            .chain(&new)
            .map(ExactSolid::size)
            .fold(1.0, Real::max);
        let near = |a: &exact::FaceSignature, b: &exact::FaceSignature| {
            let area = (a.area - b.area).abs() <= 1e-3 * a.area.max(b.area) + 1e-6 * size * size;
            let centre = (a.centroid - b.centroid).norm() <= 1e-4 * size;
            let normal = match (a.normal, b.normal) {
                (Some(x), Some(y)) => x.dot(&y) > 1.0 - 1e-6,
                (None, None) => true,
                _ => false,
            };
            area && centre && normal
        };
        let mut used = vec![false; of.len()];
        let mut report = UpdateReport::default();
        // Old entity numbers stay reserved: a new face takes one only by matching that face.
        let old_ids: std::collections::BTreeSet<u64> = of.iter().map(|f| f.entity).collect();
        let mut fresh = of.iter().chain(&nf).map(|f| f.entity).max().unwrap_or(0) + 1;
        // Pass 1: the same face (area, centre, normal). Pass 2: a plane that only grew or shrank
        // (same normal and place), when exactly one old plane fits.
        let mut hits: Vec<Option<usize>> = nf
            .iter()
            .map(|f| {
                let i = (0..of.len())
                    .find(|&i| !used[i] && of[i].body == f.body && near(&of[i].sig, &f.sig))?;
                used[i] = true;
                Some(i)
            })
            .collect();
        let same_plane =
            |a: &exact::FaceSignature, b: &exact::FaceSignature| match (a.normal, b.normal) {
                (Some(x), Some(y)) => {
                    x.dot(&y) > 1.0 - 1e-6 && x.dot(&(b.centroid - a.centroid)).abs() <= 1e-4 * size
                }
                _ => false,
            };
        for (k, f) in nf.iter().enumerate() {
            if hits[k].is_some() {
                continue;
            }
            let fits: Vec<usize> = (0..of.len())
                .filter(|&i| !used[i] && of[i].body == f.body && same_plane(&of[i].sig, &f.sig))
                .collect();
            let rivals = nf
                .iter()
                .enumerate()
                .filter(|(j, g)| {
                    hits[*j].is_none() && g.body == f.body && same_plane(&g.sig, &f.sig)
                })
                .count();
            if let [i] = fits.as_slice()
                && rivals == 1
            {
                used[*i] = true;
                hits[k] = Some(*i);
            }
        }
        for (f, hit) in nf.iter().zip(hits) {
            match hit {
                Some(i) => {
                    report.matched += 1;
                    if of[i].entity != f.entity {
                        next.keep.insert(f.entity, of[i].entity);
                    }
                }
                None => {
                    report.new += 1;
                    if old_ids.contains(&f.entity) {
                        next.keep.insert(f.entity, fresh);
                        fresh += 1;
                    }
                }
            }
        }
        report.lost = used.iter().filter(|u| !**u).count();
        if let Some(l) = &mut next.link {
            *l = SourceLink::now(&l.path);
        }
        Ok((next, report))
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

    /// `text` with every entity renumbered (`#n` → `#n+1000`), as a re-export does.
    fn renumbered(text: &str) -> String {
        let mut out = String::with_capacity(text.len() + text.len() / 8);
        let mut it = text.char_indices().peekable();
        while let Some((_, c)) = it.next() {
            out.push(c);
            if c != '#' {
                continue;
            }
            let mut n = String::new();
            while let Some((_, d)) = it.peek().copied().filter(|(_, d)| d.is_ascii_digit()) {
                n.push(d);
                it.next();
            }
            match n.parse::<u64>() {
                Ok(v) => out += &(v + 1000).to_string(),
                Err(_) => out += &n,
            }
        }
        out
    }

    fn box_step(h: f64) -> String {
        let mut p = PartDocument::with_box("Box1", [40.0, 30.0, h]).expect("box");
        p.regenerate().expect("regenerate");
        exact::step_text(&p.exact_bodies.0, "Box").expect("export")
    }

    /// An imported box with a 2 mm fillet on the edge between its bottom and front faces.
    fn linked_with_fillet(text: &str) -> PartDocument {
        let mut p = PartDocument::default();
        let mut op = ImportOp::new("box.step", text);
        op.link = Some(SourceLink::default());
        p.add_body_op(BodyOpFeature::new(BodyOp::Import(op)))
            .expect("import");
        // The edge on the bottom (z = 0) and the front (y = 0): its hint is the edge's middle.
        let edge = p
            .body
            .edges()
            .filter_map(|(e, _)| crate::part::blend::edge_ref(&p.body, e))
            .find(|r| r.hint[1].abs() < 1e-6 && r.hint[2].abs() < 1e-6)
            .expect("the bottom front edge");
        p.add_blend(crate::part::blend::BlendFeature::fillet(vec![edge], 2.0))
            .expect("fillet");
        p
    }

    /// Update of the Import feature (index 0) to `text`.
    fn update(p: &mut PartDocument, text: &str) -> UpdateReport {
        let Some(Feature::BodyOp(b)) = p.features.first().cloned() else {
            panic!("no import");
        };
        let BodyOp::Import(op) = &b.op else {
            panic!("not an import");
        };
        let (next, report) = op.updated(&b.id, text.into()).expect("update");
        let mut nb = b.clone();
        nb.op = BodyOp::Import(next);
        p.replace_feature(0, Feature::BodyOp(nb)).expect("replace");
        report
    }

    #[test]
    fn a_renumbered_file_keeps_every_face_name() {
        let text = box_step(20.0);
        let mut p = linked_with_fillet(&text);
        let before: Vec<_> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
        let v0 = solid_volume(&p);
        let r = update(&mut p, &renumbered(&text));
        assert_eq!((r.matched, r.new, r.lost), (6, 0, 0), "{r:?}");
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        let mut after: Vec<_> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
        let mut b = before.clone();
        b.sort();
        after.sort();
        assert_eq!(after, b);
        assert!((solid_volume(&p) - v0).abs() < 1e-6 * v0);
    }

    #[test]
    fn a_taller_part_keeps_the_faces_that_stay_and_the_fillet() {
        let text = box_step(20.0);
        let mut p = linked_with_fillet(&text);
        // The new file: 5 mm taller, written again (entities renumbered too).
        let r = update(&mut p, &renumbered(&box_step(25.0)));
        // The bottom is the same face; the four sides are the same planes, grown; the top moved.
        assert_eq!((r.matched, r.new, r.lost), (5, 1, 1), "{r:?}");
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        let v = solid_volume(&p);
        let want = 40.0 * 30.0 * 25.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 40.0;
        assert!((v - want).abs() < 1e-3 * want, "{v} vs {want}");
    }

    #[test]
    fn a_link_notices_a_changed_file() {
        let dir = std::env::temp_dir().join(format!("cc_import_link_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("part.step");
        std::fs::write(&f, "a").expect("write");
        let path = f.display().to_string();
        let l = SourceLink::now(&path);
        assert!(!l.changed());
        std::fs::write(&f, "a longer text").expect("write");
        assert!(l.changed());
        let gone = SourceLink {
            path: dir.join("missing.step").display().to_string(),
            ..Default::default()
        };
        assert!(!gone.changed(), "a missing file cannot update");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
