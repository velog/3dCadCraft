//! Materials and appearances of a part (`docs/spec/2.15-materials.md`).
//!
//! The part has a material ([`PartDocument::material`], `None` = Generic)
//! and an appearance ([`PartDocument::appearance`], `None` = "As
//! Material"). [`Looks`] holds the overrides — by **persistent name**, never
//! by arena id (rule 2): bodies by `Solid::name`, features by id, faces by
//! name plus a point on the face — and the document copies of every
//! material and appearance used, so a file opens the same everywhere.
//!
//! Precedence (Inventor): **Face > Feature > Body > Part > Material**,
//! where the material is the body's own, else the part's ([`Resolver`]).
//!
//! Everything here is part data: per-occurrence colours of an assembly
//! belong to the assembly (rule 8).

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::PartDocument;
use crate::kernel::{BRep, FaceId, Point3, Real, SolidId};
use crate::materials::{self, Appearance, GENERIC, Library, Material};

/// Material and appearance of one body (both optional overrides).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BodyStyle {
    /// The body's material (`None`: the part's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// The body's appearance (`None`: the part's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<String>,
}

impl BodyStyle {
    fn is_empty(&self) -> bool {
        self.material.is_none() && self.appearance.is_none()
    }
}

/// A face's appearance: the face named `face` nearest `hint` (a Boolean
/// can split one face into pieces of the same name).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceLook {
    /// Persistent face name.
    pub face: String,
    /// A point on the face (model coordinates) when it was assigned.
    pub hint: [Real; 3],
    /// Appearance name.
    pub appearance: String,
}

/// Overrides and document copies (all optional, additive file data).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Looks {
    /// Per body (by persistent body name).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bodies: BTreeMap<String, BodyStyle>,
    /// Per feature (by feature id): the faces the feature made.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub features: BTreeMap<String, String>,
    /// Per face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceLook>,
    /// Document copies of the materials in use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<Material>,
    /// Document copies of the appearances in use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub appearances: Vec<Appearance>,
}

impl Looks {
    /// Nothing stored (keeps old files byte-identical).
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What receives an appearance (or a Clear).
#[derive(Clone, Debug, PartialEq)]
pub enum LookTarget {
    /// The whole part.
    Part,
    /// A body by persistent name.
    Body(String),
    /// A feature by id.
    Feature(String),
    /// A face by persistent name and a point on it.
    Face {
        /// Persistent face name.
        face: String,
        /// Point on the face.
        hint: [Real; 3],
    },
}

/// Which level an appearance came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookSource {
    /// A face override.
    Face,
    /// A feature override.
    Feature,
    /// A body override.
    Body,
    /// The part's appearance.
    Part,
    /// The material's own appearance (no override).
    Material,
}

/// Which overrides Clear removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClearWhat {
    /// Face overrides.
    pub faces: bool,
    /// Feature overrides.
    pub features: bool,
    /// Body appearance overrides.
    pub bodies: bool,
    /// The part's appearance (back to As Material).
    pub part: bool,
}

impl ClearWhat {
    /// Every level.
    pub const ALL: ClearWhat = ClearWhat {
        faces: true,
        features: true,
        bodies: true,
        part: true,
    };
}

/// Resolves names to definitions and faces to their appearance.
pub struct Resolver<'a> {
    part: &'a PartDocument,
    lib: &'a Library,
}

impl<'a> Resolver<'a> {
    /// Definitions come from the document copies first, then the built-in
    /// library.
    pub fn new(part: &'a PartDocument) -> Self {
        Self {
            part,
            lib: materials::builtin(),
        }
    }

    /// An appearance by name (document → library → the fallback look).
    pub fn appearance(&self, name: &str) -> Appearance {
        self.find_appearance(name)
            .cloned()
            .unwrap_or_else(Appearance::fallback)
    }

    fn find_appearance(&self, name: &str) -> Option<&'a Appearance> {
        self.part
            .looks
            .appearances
            .iter()
            .find(|a| a.name == name)
            .or_else(|| self.lib.appearance(name))
    }

    /// A material by name (document → library); `None` for unknown names.
    pub fn find_material(&self, name: &str) -> Option<&'a Material> {
        self.part
            .looks
            .materials
            .iter()
            .find(|m| m.name == name)
            .or_else(|| self.lib.material(name))
    }

    /// The name of a body's material (its own, else the part's, else
    /// Generic).
    pub fn body_material_name(&self, body: Option<&str>) -> &'a str {
        body.and_then(|b| self.part.looks.bodies.get(b))
            .and_then(|s| s.material.as_deref())
            .or(self.part.material.as_deref())
            .unwrap_or(GENERIC)
    }

    /// A body's material and whether its name was known (an unknown name
    /// gives Generic's values).
    pub fn body_material(&self, body: Option<&str>) -> (Material, bool) {
        let name = self.body_material_name(body);
        match self.find_material(name) {
            Some(m) => (m.clone(), true),
            None => {
                let mut g = Library::generic();
                g.name = name.to_owned();
                (g, false)
            }
        }
    }

    /// A body's appearance: body > part > its material's.
    pub fn body_look(&self, body: Option<&str>) -> (Appearance, LookSource) {
        if let Some(a) = body
            .and_then(|b| self.part.looks.bodies.get(b))
            .and_then(|s| s.appearance.as_deref())
        {
            return (self.appearance(a), LookSource::Body);
        }
        if let Some(a) = self.part.appearance.as_deref() {
            return (self.appearance(a), LookSource::Part);
        }
        let (m, _) = self.body_material(body);
        (self.appearance(&m.appearance), LookSource::Material)
    }

    /// The appearance of a face named `face` (if any) of body `body`:
    /// face (the entry `face_entry` chose for it) > feature > body > part
    /// > material.
    pub fn face_look(
        &self,
        body: Option<&str>,
        face: Option<&str>,
        face_entry: Option<&str>,
    ) -> (Appearance, LookSource) {
        if let Some(a) = face_entry {
            return (self.appearance(a), LookSource::Face);
        }
        if let Some(a) = face
            .and_then(super::select::feature_id_of_face)
            .and_then(|f| self.part.looks.features.get(f))
        {
            return (self.appearance(a), LookSource::Feature);
        }
        self.body_look(body)
    }

    /// The face each face override lands on in `body` (the face of that
    /// name nearest its hint): face id → appearance name.
    pub fn face_entries(&self, body: &BRep) -> HashMap<FaceId, &'a str> {
        let mut out = HashMap::new();
        if self.part.looks.faces.is_empty() {
            return out;
        }
        let mut by_name: HashMap<&str, Vec<FaceId>> = HashMap::new();
        for (id, f) in body.faces() {
            if let Some(n) = f.name.as_deref() {
                by_name.entry(n).or_default().push(id);
            }
        }
        for e in &self.part.looks.faces {
            let Some(cands) = by_name.get(e.face.as_str()) else {
                continue;
            };
            let pick = if cands.len() == 1 {
                Some(cands[0])
            } else {
                let hint = Point3::from(e.hint);
                cands
                    .iter()
                    .map(|&f| (f, face_distance(body, f, &hint)))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(f, _)| f)
            };
            if let Some(f) = pick {
                out.insert(f, e.appearance.as_str());
            }
        }
        out
    }

    /// Every face of the part's body with its appearance, grouped by solid.
    pub fn face_looks(&self) -> HashMap<FaceId, (Appearance, LookSource)> {
        let body = &self.part.body;
        let entries = self.face_entries(body);
        let mut out = HashMap::new();
        for (_, s) in body.solids() {
            let bname = s.name.as_deref();
            for sh in &s.shells {
                for &f in &sh.faces {
                    let name = body.face(f).and_then(|x| x.name.as_deref());
                    out.insert(f, self.face_look(bname, name, entries.get(&f).copied()));
                }
            }
        }
        out
    }
}

/// The face named `name` nearest `hint` (the only one when it is unique).
pub fn nearest_face(body: &BRep, name: &str, hint: &[Real; 3]) -> Option<FaceId> {
    let cands: Vec<FaceId> = body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some(name))
        .map(|(id, _)| id)
        .collect();
    if cands.len() <= 1 {
        return cands.first().copied();
    }
    let p = Point3::from(*hint);
    cands
        .into_iter()
        .map(|f| (f, face_distance(body, f, &p)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(f, _)| f)
}

/// Distance from `p` to the nearest display vertex of face `f`.
fn face_distance(body: &BRep, f: FaceId, p: &Point3) -> Real {
    let near = |pts: &[Point3]| {
        pts.iter()
            .map(|q| (q - p).norm())
            .min_by(|a, b| a.total_cmp(b))
            .unwrap_or(Real::INFINITY)
    };
    match body.face(f).and_then(|x| x.mesh.as_ref()) {
        Some(m) => near(&m.positions),
        None => body
            .face_mesh(f, &Default::default())
            .map_or(Real::INFINITY, |m| near(&m.positions)),
    }
}

/// A point on face `f` (for a face override's hint): its display vertex
/// nearest the face's mean.
pub fn face_hint(body: &BRep, f: FaceId) -> Option<[Real; 3]> {
    let pts: Vec<Point3> = match body.face(f)?.mesh.as_ref() {
        Some(m) => m.positions.clone(),
        None => body.face_mesh(f, &Default::default()).ok()?.positions,
    };
    let n = pts.len().max(1) as Real;
    let mean = pts
        .iter()
        .fold(Point3::origin(), |acc, p| acc + p.coords / n);
    let p = pts
        .iter()
        .min_by(|a, b| (*a - mean).norm().total_cmp(&(*b - mean).norm()))?;
    Some([p.x, p.y, p.z])
}

/// The persistent name of the solid holding face `f`.
pub fn solid_of_face(body: &BRep, f: FaceId) -> Option<(SolidId, Option<String>)> {
    body.solids()
        .find(|(_, s)| s.shells.iter().any(|sh| sh.faces.contains(&f)))
        .map(|(id, s)| (id, s.name.clone()))
}

impl PartDocument {
    /// Copies appearance `name` into the document (from `lib`) unless it is
    /// there already.
    fn adopt_appearance(&mut self, name: &str, lib: &Library) -> Result<(), String> {
        if self.looks.appearances.iter().any(|a| a.name == name) {
            return Ok(());
        }
        let a = lib
            .appearance(name)
            .or_else(|| materials::builtin().appearance(name))
            .ok_or_else(|| format!("unknown appearance {name}"))?;
        self.looks.appearances.push(a.clone());
        Ok(())
    }

    /// Copies material `name` (and its appearance) into the document.
    fn adopt_material(&mut self, name: &str, lib: &Library) -> Result<(), String> {
        if !self.looks.materials.iter().any(|m| m.name == name) {
            let m = lib
                .material(name)
                .or_else(|| materials::builtin().material(name))
                .ok_or_else(|| format!("unknown material {name}"))?
                .clone();
            self.looks.materials.push(m);
        }
        let look = self
            .looks
            .materials
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.appearance.clone())
            .unwrap_or_default();
        // A material whose appearance is missing still works (fallback look).
        let _ = self.adopt_appearance(&look, lib);
        Ok(())
    }

    /// Sets the part's material (`None`: Generic). Unknown names fail and
    /// leave the part unchanged.
    pub fn set_material(&mut self, name: Option<&str>, lib: &Library) -> Result<(), String> {
        if let Some(n) = name {
            self.adopt_material(n, lib)?;
        }
        self.material = name.filter(|n| *n != GENERIC).map(str::to_owned);
        Ok(())
    }

    /// Sets a body's material (`None`: the part's).
    pub fn set_body_material(
        &mut self,
        body: &str,
        name: Option<&str>,
        lib: &Library,
    ) -> Result<(), String> {
        if let Some(n) = name {
            self.adopt_material(n, lib)?;
        }
        let e = self.looks.bodies.entry(body.to_owned()).or_default();
        e.material = name.map(str::to_owned);
        if e.is_empty() {
            self.looks.bodies.remove(body);
        }
        Ok(())
    }

    /// Gives `target` appearance `name` (`None` removes that override: the
    /// part goes back to As Material).
    pub fn set_appearance(
        &mut self,
        target: &LookTarget,
        name: Option<&str>,
        lib: &Library,
    ) -> Result<(), String> {
        if let Some(n) = name {
            self.adopt_appearance(n, lib)?;
        }
        let own = name.map(str::to_owned);
        match target {
            LookTarget::Part => self.appearance = own,
            LookTarget::Body(b) => {
                let e = self.looks.bodies.entry(b.clone()).or_default();
                e.appearance = own;
                if e.is_empty() {
                    self.looks.bodies.remove(b);
                }
            }
            LookTarget::Feature(f) => match own {
                Some(a) => {
                    self.looks.features.insert(f.clone(), a);
                }
                None => {
                    self.looks.features.remove(f);
                }
            },
            LookTarget::Face { face, hint } => {
                // One entry per face piece: replace one landing on it.
                let body = &self.body;
                let target = nearest_face(body, face, hint);
                self.looks.faces.retain(|e| {
                    e.face != *face
                        || match target {
                            Some(t) => nearest_face(body, &e.face, &e.hint) != Some(t),
                            None => (Point3::from(e.hint) - Point3::from(*hint)).norm() > 1e-6,
                        }
                });
                if let Some(a) = own {
                    self.looks.faces.push(FaceLook {
                        face: face.clone(),
                        hint: *hint,
                        appearance: a,
                    });
                }
            }
        }
        Ok(())
    }

    /// Adds a custom appearance (Adjust's colour editor) to the document and
    /// returns its name (`Custom N`).
    pub fn add_custom_appearance(&mut self, color: [u8; 3], opacity: Real) -> String {
        let mut k = 1;
        let name = loop {
            let n = format!("Custom {k}");
            if !self.looks.appearances.iter().any(|a| a.name == n) {
                break n;
            }
            k += 1;
        };
        self.looks
            .appearances
            .push(Appearance::custom(name.clone(), color, opacity));
        name
    }

    /// Clear: removes overrides. With `only`, just those targets'
    /// (faces of a selected feature/body are not touched); otherwise every
    /// override of the chosen levels. Returns how many were removed.
    pub fn clear_appearances(&mut self, what: ClearWhat, only: Option<&[LookTarget]>) -> usize {
        let before = self.override_count();
        let picked = |t: &LookTarget| only.is_none_or(|o| o.contains(t));
        if what.faces {
            match only {
                None => self.looks.faces.clear(),
                Some(o) => {
                    let body = &self.body;
                    let sel: Vec<FaceId> = o
                        .iter()
                        .filter_map(|t| match t {
                            LookTarget::Face { face, hint } => nearest_face(body, face, hint),
                            _ => None,
                        })
                        .collect();
                    self.looks.faces.retain(|e| {
                        !nearest_face(body, &e.face, &e.hint).is_some_and(|f| sel.contains(&f))
                    });
                }
            }
        }
        if what.features {
            self.looks
                .features
                .retain(|f, _| !picked(&LookTarget::Feature(f.clone())));
        }
        if what.bodies {
            for (b, s) in self.looks.bodies.iter_mut() {
                if picked(&LookTarget::Body(b.clone())) {
                    s.appearance = None;
                }
            }
            self.looks.bodies.retain(|_, s| !s.is_empty());
        }
        if what.part && picked(&LookTarget::Part) {
            self.appearance = None;
        }
        before - self.override_count()
    }

    /// Number of appearance overrides (faces, features, bodies, part).
    pub fn override_count(&self) -> usize {
        self.looks.faces.len()
            + self.looks.features.len()
            + self
                .looks
                .bodies
                .values()
                .filter(|s| s.appearance.is_some())
                .count()
            + usize::from(self.appearance.is_some())
    }

    /// Names of the materials and appearances in use.
    pub fn styles_in_use(&self) -> (Vec<String>, Vec<String>) {
        let mut mats: Vec<String> = self.material.iter().cloned().collect();
        mats.extend(
            self.looks
                .bodies
                .values()
                .filter_map(|s| s.material.clone()),
        );
        let r = Resolver::new(self);
        let mut looks: Vec<String> = self.appearance.iter().cloned().collect();
        looks.extend(
            self.looks
                .bodies
                .values()
                .filter_map(|s| s.appearance.clone()),
        );
        looks.extend(self.looks.features.values().cloned());
        looks.extend(self.looks.faces.iter().map(|f| f.appearance.clone()));
        looks.extend(
            mats.iter()
                .filter_map(|m| r.find_material(m).map(|m| m.appearance.clone())),
        );
        mats.sort();
        mats.dedup();
        looks.sort();
        looks.dedup();
        (mats, looks)
    }

    /// Purge: drops document copies nobody uses. Returns how many went.
    pub fn purge_styles(&mut self) -> usize {
        let (mats, looks) = self.styles_in_use();
        let n = self.looks.materials.len() + self.looks.appearances.len();
        self.looks.materials.retain(|m| mats.contains(&m.name));
        self.looks.appearances.retain(|a| looks.contains(&a.name));
        n - self.looks.materials.len() - self.looks.appearances.len()
    }

    /// Update: replaces document copies by the library's entries of the
    /// same name. Returns how many changed.
    pub fn update_styles(&mut self, lib: &Library) -> usize {
        let mut n = 0;
        for m in &mut self.looks.materials {
            if let Some(l) = lib.material(&m.name)
                && l != m
            {
                *m = l.clone();
                n += 1;
            }
        }
        for a in &mut self.looks.appearances {
            if let Some(l) = lib.appearance(&a.name)
                && l != a
            {
                *a = l.clone();
                n += 1;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{ExtrudeFeature, Operation, ProfileRef, SketchOptions};
    use crate::part::{PlaneRef, SketchPlacement};
    use crate::sketch::Point2;

    /// Box1 80×60×20 with a 10 mm boss (Extrusion1) joined on its top.
    fn boss() -> PartDocument {
        let mut part = PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap();
        let opts = SketchOptions {
            auto_reference_edges: false,
            auto_project_origin: false,
            ..Default::default()
        };
        let i = part
            .create_sketch(
                SketchPlacement {
                    plane: PlaneRef::Face("Box1:Top".into()),
                    ..Default::default()
                },
                &opts,
            )
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        sk.add_circle(Point2::new(10.0, 10.0), 5.0).unwrap();
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        let mut e = ExtrudeFeature::new(vec![pr], 10.0);
        e.operation = Operation::Join;
        part.add_extrude(i, e).unwrap();
        part
    }

    fn look_of(part: &PartDocument, face: &str) -> (String, LookSource) {
        let f = part.body.find_face(face).unwrap();
        let (a, s) = Resolver::new(part).face_looks()[&f].clone();
        (a.name, s)
    }

    /// The five levels, each winning over the ones below it.
    #[test]
    fn precedence_face_feature_body_part_material() {
        let lib = materials::builtin();
        let mut part = boss();
        let front = "Box1:Front";
        let side = "Extrusion1:End";
        assert!(part.body.find_face(side).is_some(), "{:?}", names(&part));
        // 1. Nothing set: Generic's Default.
        assert_eq!(
            look_of(&part, front),
            ("Default".into(), LookSource::Material)
        );
        // 1b. A material: its appearance.
        part.set_material(Some("Steel S235"), lib).unwrap();
        assert_eq!(
            look_of(&part, front),
            ("Steel - Satin".into(), LookSource::Material)
        );
        // 2. Part appearance beats the material.
        part.set_appearance(&LookTarget::Part, Some("ABS - Red"), lib)
            .unwrap();
        assert_eq!(
            look_of(&part, front),
            ("ABS - Red".into(), LookSource::Part)
        );
        // 3. Body beats part.
        part.set_appearance(&LookTarget::Body("Box1".into()), Some("ABS - Blue"), lib)
            .unwrap();
        assert_eq!(
            look_of(&part, front),
            ("ABS - Blue".into(), LookSource::Body)
        );
        // 4. Feature beats body (only the feature's faces).
        part.set_appearance(
            &LookTarget::Feature("Extrusion1".into()),
            Some("ABS - Green"),
            lib,
        )
        .unwrap();
        assert_eq!(
            look_of(&part, side),
            ("ABS - Green".into(), LookSource::Feature)
        );
        assert_eq!(look_of(&part, front).1, LookSource::Body);
        // 5. Face beats feature.
        let f = part.body.find_face(side).unwrap();
        let hint = face_hint(&part.body, f).unwrap();
        part.set_appearance(
            &LookTarget::Face {
                face: side.into(),
                hint,
            },
            Some("ABS - Yellow"),
            lib,
        )
        .unwrap();
        assert_eq!(
            look_of(&part, side),
            ("ABS - Yellow".into(), LookSource::Face)
        );
        // Clear everything: back to the material.
        let removed = part.clear_appearances(ClearWhat::ALL, None);
        assert_eq!(removed, 4);
        assert_eq!(
            look_of(&part, side),
            ("Steel - Satin".into(), LookSource::Material)
        );
        // Purge keeps the material and its look only.
        part.purge_styles();
        let names: Vec<_> = part
            .looks
            .appearances
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, ["Steel - Satin"]);
    }

    fn names(part: &PartDocument) -> Vec<String> {
        part.body
            .faces()
            .filter_map(|(_, f)| f.name.clone())
            .collect()
    }

    /// Overrides follow persistent names through a regeneration and an
    /// edit of an earlier feature (the box gets taller).
    #[test]
    fn overrides_survive_regeneration_and_edits() {
        let lib = materials::builtin();
        let mut part = boss();
        let f = part.body.find_face("Box1:Front").unwrap();
        let hint = face_hint(&part.body, f).unwrap();
        part.set_appearance(
            &LookTarget::Face {
                face: "Box1:Front".into(),
                hint,
            },
            Some("ABS - Red"),
            lib,
        )
        .unwrap();
        part.set_appearance(
            &LookTarget::Feature("Extrusion1".into()),
            Some("ABS - Green"),
            lib,
        )
        .unwrap();
        part.regenerate().unwrap();
        // Edit the box (an earlier feature) as the GUI does.
        part.box_feature_mut().unwrap().size[2] = 35.0;
        part.regenerate().unwrap();
        assert_eq!(look_of(&part, "Box1:Front").0, "ABS - Red");
        assert_eq!(look_of(&part, "Extrusion1:End").0, "ABS - Green");
        assert_eq!(look_of(&part, "Box1:Back").1, LookSource::Material);
        // The boss sits on the new top.
        let end = part.body.find_face("Extrusion1:End").unwrap();
        assert!(face_hint(&part.body, end).unwrap()[2] > 44.0);
    }

    /// Two pieces of one name (the boss's cylinder is two half faces): the
    /// hint picks the piece; Clear of that piece leaves the other.
    #[test]
    fn face_pieces_are_told_apart_by_the_hint() {
        let lib = materials::builtin();
        let mut part = boss();
        let pieces: Vec<FaceId> = part
            .body
            .faces()
            .filter(|(_, f)| f.name.as_deref() == Some("Extrusion1:Side:1v1:0"))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(pieces.len(), 2);
        let hints: Vec<_> = pieces
            .iter()
            .map(|&f| face_hint(&part.body, f).unwrap())
            .collect();
        let at = |i: usize| LookTarget::Face {
            face: "Extrusion1:Side:1v1:0".into(),
            hint: hints[i],
        };
        part.set_appearance(&at(0), Some("ABS - Red"), lib).unwrap();
        part.set_appearance(&at(1), Some("ABS - Blue"), lib)
            .unwrap();
        // Re-assigning a piece replaces its entry.
        part.set_appearance(&at(1), Some("ABS - Green"), lib)
            .unwrap();
        assert_eq!(part.looks.faces.len(), 2);
        let looks = Resolver::new(&part).face_looks();
        assert_eq!(looks[&pieces[0]].0.name, "ABS - Red");
        assert_eq!(looks[&pieces[1]].0.name, "ABS - Green");
        let only = [at(0)];
        assert_eq!(part.clear_appearances(ClearWhat::ALL, Some(&only)), 1);
        let looks = Resolver::new(&part).face_looks();
        assert_eq!(looks[&pieces[0]].1, LookSource::Material);
        assert_eq!(looks[&pieces[1]].0.name, "ABS - Green");
    }

    #[test]
    fn unknown_names_fail_and_change_nothing() {
        let lib = materials::builtin();
        let mut part = boss();
        assert!(part.set_material(Some("Kryptonite"), lib).is_err());
        assert!(
            part.set_appearance(&LookTarget::Part, Some("Plaid"), lib)
                .is_err()
        );
        assert!(part.looks.is_empty() && part.material.is_none());
        // Generic is stored as "no material".
        part.set_material(Some(GENERIC), lib).unwrap();
        assert!(part.material.is_none());
    }

    /// A document copy wins over the library: the file looks the same on a
    /// machine whose library changed.
    #[test]
    fn document_copies_win_and_round_trip() {
        let mut lib = materials::builtin().clone();
        let mut part = boss();
        part.set_material(Some("Aluminium 6061-T6"), &lib).unwrap();
        let mut odd = lib.appearance("ABS - Red").cloned().unwrap();
        odd.color = [1, 2, 3];
        lib.put_appearance(odd);
        part.set_appearance(&LookTarget::Body("Box1".into()), Some("ABS - Red"), &lib)
            .unwrap();
        let custom = part.add_custom_appearance([9, 8, 7], 0.5);
        part.set_appearance(
            &LookTarget::Feature("Extrusion1".into()),
            Some(&custom),
            &lib,
        )
        .unwrap();
        assert_eq!(look_of(&part, "Box1:Front").0, "ABS - Red");
        let f = part.body.find_face("Box1:Front").unwrap();
        assert_eq!(Resolver::new(&part).face_looks()[&f].0.color, [1, 2, 3]);
        let json = serde_json::to_string(&part).unwrap();
        let mut back: PartDocument = serde_json::from_str(&json).unwrap();
        back.regenerate().unwrap();
        assert_eq!(back.looks, part.looks);
        assert_eq!(back.material, part.material);
        let side = back.body.find_face("Extrusion1:End").unwrap();
        let (a, _) = Resolver::new(&back).face_looks()[&side].clone();
        assert_eq!((a.color, a.opacity), ([9, 8, 7], 0.5));
        // Update pulls the library's version.
        assert_eq!(part.update_styles(materials::builtin()), 1);
    }

    /// Old files (no fields) and parts without styles serialise without
    /// any new key.
    #[test]
    fn no_styles_no_new_keys() {
        let part = boss();
        let json = serde_json::to_string(&part).unwrap();
        for key in ["\"looks\"", "\"material\"", "\"appearance\""] {
            assert!(!json.contains(key), "{key}");
        }
    }
}
