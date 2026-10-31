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
    /// What the last read of the text found per body (names and colours from the file; not
    /// saved: every regeneration reads it again).
    #[serde(skip)]
    pub found: FileBodies,
    /// Derive (spec 3.10 §3.5): the text was generated from another part's bodies, which
    /// Update reads again. Old builds see a plain import of the same text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derive: Option<super::derive::DeriveLink>,
}

/// A body as the file describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct FileBody {
    /// Its persistent name (`Import1`, `Import1#2` …).
    pub name: String,
    /// The file's label of the solid, else its product's name (may be empty).
    pub label: String,
    /// Its colour in the file (sRGB 0–1).
    pub color: Option<[f32; 3]>,
}

/// [`FileBody`]s of an import's last read, shared by the clones of the part (a regeneration of a
/// copy fills the original's too). Always equal: it is derived from the text.
#[derive(Clone, Debug, Default)]
pub struct FileBodies(Arc<std::sync::OnceLock<Vec<FileBody>>>);

impl PartialEq for FileBodies {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl FileBodies {
    /// The bodies, once the text was read.
    pub fn get(&self) -> Option<&[FileBody]> {
        self.0.get().map(Vec::as_slice)
    }
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
    /// Their names (`Import1:F123`).
    pub gone: Vec<String>,
}

/// The first line of the OBJ text a mesh import embeds.
const MESH_HEADER: &str = "# 3DCadCraft mesh import";

/// Whether an import's text is a mesh (OBJ) rather than STEP.
fn is_mesh_text(text: &str) -> bool {
    text.trim_start().starts_with(MESH_HEADER)
}

/// One object of a mesh file.
struct MeshObject {
    name: String,
    positions: Vec<crate::kernel::Point3>,
    triangles: Vec<[u32; 3]>,
}

/// The most triangles a mesh file may bring (untrusted input).
const MESH_LIMIT: usize = 5_000_000;

/// An STL (binary or ASCII) or OBJ file as the OBJ text an import embeds (vertices and triangles
/// in mm, one `o` per solid / object; normals and textures dropped). `scale`: millimetres per
/// file unit. `file` names the file in errors and decides OBJ by its extension.
pub fn mesh_text(bytes: &[u8], file: &str, scale: Real) -> Result<String, String> {
    let obj = file.to_ascii_lowercase().ends_with(".obj");
    let objects = if obj {
        parse_obj(&String::from_utf8_lossy(bytes))?
    } else {
        parse_stl(bytes)?
    };
    let mut out = String::from(MESH_HEADER);
    out.push('\n');
    let mut base = 1usize;
    for o in &objects {
        out += &format!("o {}\n", o.name.replace(['\n', '\r'], " "));
        for p in &o.positions {
            out += &format!("v {} {} {}\n", p.x * scale, p.y * scale, p.z * scale);
        }
        for t in &o.triangles {
            out += &format!(
                "f {} {} {}\n",
                t[0] as usize + base,
                t[1] as usize + base,
                t[2] as usize + base
            );
        }
        base += o.positions.len();
    }
    Ok(out)
}

/// The objects of OBJ text (`o` / `g` start one; faces of more corners are fanned; negative
/// indices count back from the last vertex).
fn parse_obj(text: &str) -> Result<Vec<MeshObject>, String> {
    let mut verts: Vec<crate::kernel::Point3> = Vec::new();
    let mut objects: Vec<(String, Vec<[usize; 3]>)> = vec![(String::new(), Vec::new())];
    let mut count = 0usize;
    for line in text.lines() {
        let mut w = line.split_whitespace();
        match w.next() {
            Some("v") => {
                let c: Vec<Real> = w.take(3).filter_map(|x| x.parse().ok()).collect();
                let [x, y, z] = c[..] else {
                    return Err(format!("OBJ: a vertex without three numbers: {line}"));
                };
                verts.push(crate::kernel::Point3::new(x, y, z));
            }
            Some("o") | Some("g") => {
                let name = w.collect::<Vec<_>>().join(" ");
                match objects.last_mut() {
                    Some(last) if last.1.is_empty() => last.0 = name,
                    _ => objects.push((name, Vec::new())),
                }
            }
            Some("f") => {
                let idx: Vec<usize> = w
                    .filter_map(|c| c.split('/').next()?.parse::<i64>().ok())
                    .filter_map(|i| {
                        if i > 0 {
                            Some(i as usize - 1)
                        } else if i < 0 {
                            verts.len().checked_sub(i.unsigned_abs() as usize)
                        } else {
                            None
                        }
                    })
                    .collect();
                if idx.len() < 3 || idx.iter().any(|&i| i >= verts.len()) {
                    return Err(format!("OBJ: a face with a missing vertex: {line}"));
                }
                count += idx.len() - 2;
                if count > MESH_LIMIT {
                    return Err(format!("OBJ: more than {MESH_LIMIT} triangles"));
                }
                if let Some(last) = objects.last_mut() {
                    for k in 1..idx.len() - 1 {
                        last.1.push([idx[0], idx[k], idx[k + 1]]);
                    }
                }
            }
            _ => {}
        }
    }
    // Each object keeps only the vertices it uses.
    let mut out = Vec::new();
    for (k, (name, tris)) in objects.into_iter().enumerate() {
        if tris.is_empty() {
            continue;
        }
        let mut map: BTreeMap<usize, u32> = BTreeMap::new();
        let mut positions = Vec::new();
        let triangles = tris
            .iter()
            .map(|t| {
                t.map(|i| {
                    *map.entry(i).or_insert_with(|| {
                        positions.push(verts[i]);
                        (positions.len() - 1) as u32
                    })
                })
            })
            .collect();
        let name = if name.is_empty() {
            format!("Mesh{}", k + 1)
        } else {
            name
        };
        out.push(MeshObject {
            name,
            positions,
            triangles,
        });
    }
    if out.is_empty() {
        return Err("OBJ: no faces".into());
    }
    Ok(out)
}

/// The solids of an STL file: binary (80-byte header, count, 50 bytes a triangle) or ASCII
/// (`solid name` … `endsolid`).
fn parse_stl(bytes: &[u8]) -> Result<Vec<MeshObject>, String> {
    let binary = bytes.len() >= 84 && {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        84 + n * 50 == bytes.len()
    };
    let mut objects = Vec::new();
    if binary {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if n > MESH_LIMIT {
            return Err(format!("STL: more than {MESH_LIMIT} triangles"));
        }
        let f = |o: usize| {
            bytes
                .get(o..o + 4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as Real)
                .unwrap_or(0.0)
        };
        let mut positions = Vec::with_capacity(n * 3);
        let mut triangles = Vec::with_capacity(n);
        for t in 0..n {
            let at = 84 + t * 50 + 12;
            for c in 0..3 {
                let o = at + c * 12;
                positions.push(crate::kernel::Point3::new(f(o), f(o + 4), f(o + 8)));
            }
            let k = (t * 3) as u32;
            triangles.push([k, k + 1, k + 2]);
        }
        let header = String::from_utf8_lossy(&bytes[..80]);
        let name = header
            .trim_start_matches("solid")
            .trim_matches(|c: char| c.is_whitespace() || c == '\0')
            .to_owned();
        objects.push(MeshObject {
            name: if name.is_empty() {
                "Mesh1".into()
            } else {
                name
            },
            positions,
            triangles,
        });
    } else {
        let text = String::from_utf8_lossy(bytes);
        let mut cur: Option<MeshObject> = None;
        for line in text.lines() {
            let mut w = line.split_whitespace();
            match w.next() {
                Some("solid") => {
                    cur = Some(MeshObject {
                        name: w.collect::<Vec<_>>().join(" "),
                        positions: Vec::new(),
                        triangles: Vec::new(),
                    });
                }
                Some("vertex") => {
                    let c: Vec<Real> = w.take(3).filter_map(|x| x.parse().ok()).collect();
                    let [x, y, z] = c[..] else {
                        return Err(format!("STL: a vertex without three numbers: {line}"));
                    };
                    let o = cur.get_or_insert_with(|| MeshObject {
                        name: String::new(),
                        positions: Vec::new(),
                        triangles: Vec::new(),
                    });
                    o.positions.push(crate::kernel::Point3::new(x, y, z));
                    if o.positions.len().is_multiple_of(3) {
                        let k = (o.positions.len() - 3) as u32;
                        o.triangles.push([k, k + 1, k + 2]);
                        if o.triangles.len() > MESH_LIMIT {
                            return Err(format!("STL: more than {MESH_LIMIT} triangles"));
                        }
                    }
                }
                Some("endsolid") => {
                    if let Some(o) = cur.take()
                        && !o.triangles.is_empty()
                    {
                        objects.push(o);
                    }
                }
                _ => {}
            }
        }
        if let Some(o) = cur.take()
            && !o.triangles.is_empty()
        {
            objects.push(o);
        }
        for (k, o) in objects.iter_mut().enumerate() {
            if o.name.is_empty() {
                o.name = format!("Mesh{}", k + 1);
            }
        }
    }
    if objects.is_empty() {
        return Err("STL: no triangles".into());
    }
    Ok(objects)
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
            found: FileBodies::default(),
            derive: None,
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
            return Err("choose a STEP, STL or OBJ file (Browse...)".into());
        }
        if !self.solids && !self.surfaces {
            return Err("turn on Solids or Surfaces".into());
        }
        Ok(())
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        match &self.derive {
            Some(d) => format!("Derived from {}", d.name),
            None => format!("Import of {}", self.file_name()),
        }
    }

    /// The bodies the filters keep (and the reader's warnings).
    pub fn read(&self, id: &str) -> Result<(Vec<ExactSolid>, Vec<String>), String> {
        if self.is_mesh() {
            return self.read_mesh(id);
        }
        let r = exact::step_bodies(&self.text, id)?;
        let total = r.bodies.len();
        let warnings = r.warnings;
        let mut kept: Vec<ExactSolid> = Vec::new();
        let mut found = Vec::new();
        for b in r.bodies {
            if !(if b.closed { self.solids } else { self.surfaces }) {
                continue;
            }
            found.push(FileBody {
                name: b.solid.name.clone().unwrap_or_default(),
                label: b.label.trim().to_owned(),
                color: b.color,
            });
            kept.push(self.renamed(b.solid));
        }
        // Names and colours for the browser and the import's appearances (the first read).
        let _ = self.found.0.set(found);
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
    /// The embedded text is a mesh (our OBJ text from an STL or OBJ file, spec 7.7a 3.14), not
    /// STEP.
    pub fn is_mesh(&self) -> bool {
        is_mesh_text(&self.text)
    }

    /// The mesh bodies of the embedded OBJ text: one per object, named `id`, `id#2` …
    fn read_mesh(&self, id: &str) -> Result<(Vec<ExactSolid>, Vec<String>), String> {
        let objects = parse_obj(&self.text)?;
        let total = objects.len();
        let mut kept = Vec::new();
        let mut found = Vec::new();
        for (k, o) in objects.into_iter().enumerate() {
            let name = if k == 0 {
                id.to_owned()
            } else {
                format!("{id}#{}", k + 1)
            };
            let s = exact::mesh_solid(&o.positions, &o.triangles, &name)?;
            if !(if s.is_open() {
                self.surfaces
            } else {
                self.solids
            }) {
                continue;
            }
            found.push(FileBody {
                name,
                label: o.name,
                color: None,
            });
            kept.push(s);
        }
        let _ = self.found.0.set(found);
        if kept.is_empty() {
            return Err(match (total, self.solids) {
                (0, _) => "the file has no triangles".into(),
                (_, true) => "the mesh is not closed (turn on Surfaces?)".into(),
                (_, false) => "the mesh is closed (turn on Solids?)".into(),
            });
        }
        Ok((kept, Vec::new()))
    }

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
        if self.is_mesh() || is_mesh_text(&text) {
            // A mesh has no faces to match: the new triangles replace the old ones.
            let mut next = ImportOp {
                text,
                keep: BTreeMap::new(),
                found: FileBodies::default(),
                ..self.clone()
            };
            let (new, _) = next.read(id)?;
            if let Some(l) = &mut next.link {
                *l = SourceLink::now(&l.path);
            }
            let report = UpdateReport {
                new: new.len(),
                ..Default::default()
            };
            return Ok((next, report));
        }
        let (old, _) = self.read(id)?;
        let mut next = ImportOp {
            text,
            keep: BTreeMap::new(),
            found: FileBodies::default(),
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
        report.gone = used
            .iter()
            .zip(&of)
            .filter(|(u, _)| !**u)
            .map(|(_, f)| {
                let body = old
                    .get(f.body)
                    .and_then(|b| b.name.clone())
                    .unwrap_or_default();
                format!("{body}:F{}", f.entity)
            })
            .collect();
        report.lost = report.gone.len();
        if let Some(l) = &mut next.link {
            *l = SourceLink::now(&l.path);
        }
        Ok((next, report))
    }
}

/// Import after the solids `cur` and the surface bodies `sheets` built so far: every body after
/// it.
impl super::PartDocument {
    /// The file's name for body `name` of an import (spec 7.7a 3.9), when it has one.
    pub fn file_label(&self, name: &str) -> Option<&str> {
        self.features.iter().find_map(|f| match f {
            super::Feature::BodyOp(b) => match &b.op {
                super::BodyOp::Import(op) => op
                    .found
                    .get()?
                    .iter()
                    .find(|fb| fb.name == name && !fb.label.is_empty())
                    .map(|fb| fb.label.as_str()),
                _ => None,
            },
            _ => None,
        })
    }

    /// The colours the file gives the bodies of import `id` as body appearances (document copies
    /// named `Color RRGGBB`), where the body has none of its own yet; returns how many it set.
    /// Called when the import is added: after that they are the user's to change.
    pub fn adopt_file_colors(&mut self, id: &str) -> usize {
        let found: Vec<FileBody> = self
            .features
            .iter()
            .find(|f| f.id() == id)
            .and_then(|f| match f {
                super::Feature::BodyOp(b) => match &b.op {
                    super::BodyOp::Import(op) => op.found.get().map(<[FileBody]>::to_vec),
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or_default();
        let mut n = 0;
        for fb in found {
            let Some(c) = fb.color else { continue };
            if self
                .looks
                .bodies
                .get(&fb.name)
                .is_some_and(|s| s.appearance.is_some())
            {
                continue;
            }
            let rgb = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            let name = format!("Color {:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
            if !self.looks.appearances.iter().any(|a| a.name == name) {
                self.looks
                    .appearances
                    .push(crate::materials::Appearance::custom(name.clone(), rgb, 1.0));
            }
            self.looks.bodies.entry(fb.name).or_default().appearance = Some(name);
            n += 1;
        }
        n
    }
}

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

    /// A 10 × 20 × 30 box as 12 triangles wound outward: (corners, triangles).
    fn box_triangles() -> (Vec<[f32; 3]>, Vec<[usize; 3]>) {
        let c = |i: usize| {
            [
                if i & 1 != 0 { 10.0 } else { 0.0 },
                if i & 2 != 0 { 20.0 } else { 0.0 },
                if i & 4 != 0 { 30.0 } else { 0.0 },
            ]
        };
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        let mut tris = Vec::new();
        for q in quads {
            tris.push([q[0], q[1], q[2]]);
            tris.push([q[0], q[2], q[3]]);
        }
        ((0..8).map(c).collect(), tris)
    }

    fn binary_stl() -> Vec<u8> {
        let (pts, tris) = box_triangles();
        let mut out = vec![0u8; 80];
        out[..10].copy_from_slice(b"solid Brkt");
        out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
        for t in tris {
            out.extend_from_slice(&[0u8; 12]);
            for k in t {
                for v in pts[k] {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            out.extend_from_slice(&[0u8; 2]);
        }
        out
    }

    fn ascii_stl() -> String {
        let (pts, tris) = box_triangles();
        let mut s = String::from("solid Block\n");
        for t in tris {
            s += "facet normal 0 0 0\nouter loop\n";
            for k in t {
                s += &format!("vertex {} {} {}\n", pts[k][0], pts[k][1], pts[k][2]);
            }
            s += "endloop\nendfacet\n";
        }
        s + "endsolid Block\n"
    }

    fn mesh_part(bytes: &[u8], file: &str) -> PartDocument {
        let text = mesh_text(bytes, file, 1.0).expect("mesh");
        let mut p = PartDocument::default();
        p.add_body_op(BodyOpFeature::new(BodyOp::Import(ImportOp::new(
            file, text,
        ))))
        .expect("import");
        p
    }

    /// Spec 7.7a 3.14: a binary and an ASCII STL and an OBJ of quads come in as mesh bodies
    /// with the box's volume and the file's names; an open mesh is a surface body.
    #[test]
    fn meshes_come_in_as_mesh_bodies() {
        for (bytes, file, label) in [
            (binary_stl(), "a.stl", "Brkt"),
            (ascii_stl().into_bytes(), "b.stl", "Block"),
        ] {
            let p = mesh_part(&bytes, file);
            assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
            assert_eq!(p.exact_bodies.0.len(), 1);
            let s = &p.exact_bodies.0[0];
            assert!(!s.is_open(), "{file}: a closed mesh is a solid");
            let v = s.volume_at(1e-3).unwrap_or(0.0);
            assert!((v - 6000.0).abs() < 1e-6, "{file}: volume {v}");
            // Shown: the display B-rep has its one face with the triangles.
            let faces: Vec<usize> = p
                .body
                .faces()
                .map(|(_, f)| f.mesh.as_ref().map_or(0, |m| m.triangles.len()))
                .collect();
            assert_eq!(faces, vec![12], "{file}: {faces:?} {:?}", p.feature_health);
            assert_eq!(
                p.bodies_before(usize::MAX),
                vec![("Import1".to_owned(), label.to_owned())]
            );
        }
        // OBJ: two objects, quads, the second open (its top left out).
        let mut obj = String::from("# test\no Lid\n");
        let (pts, _) = box_triangles();
        for q in &pts {
            obj += &format!("v {} {} {}\n", q[0], q[1], q[2]);
        }
        obj += "f 1 3 4 2\nf 5 6 8 7\nf 1 2 6 5\nf 3 7 8 4\nf 1 5 7 3\nf 2 4 8 6\n";
        obj += "o Tray\nf 1 3 4 2\nf 1 2 6 5\nf 3 7 8 4\nf 1 5 7 3\nf 2 4 8 6\n";
        let p = mesh_part(obj.as_bytes(), "c.obj");
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        let open: Vec<bool> = p.exact_bodies.0.iter().map(|s| s.is_open()).collect();
        assert_eq!(open, vec![false, true]);
        assert_eq!(p.file_label("Import1#2"), Some("Tray"));
        // Bad files are refused with the reason.
        assert!(mesh_text(b"solid x\nendsolid x\n", "x.stl", 1.0).is_err());
        assert!(mesh_text(b"v 0 0 0\nf 1 2 3\n", "x.obj", 1.0).is_err());
    }

    /// Spec 7.7a 3.9: bodies are shown by the file's names for them and take its colours as body
    /// appearances; their persistent names stay `Import1`, `Import1#2`; after a save and a load
    /// the names come back from the embedded text.
    #[test]
    fn bodies_take_the_files_names_and_colours() {
        let mut a = exact::box_named(
            crate::kernel::Point3::origin(),
            crate::kernel::Point3::new(10.0, 10.0, 10.0),
            "A",
        )
        .expect("box");
        a.name = Some("Bracket".into());
        let mut b = exact::box_named(
            crate::kernel::Point3::new(20.0, 0.0, 0.0),
            crate::kernel::Point3::new(25.0, 5.0, 5.0),
            "B",
        )
        .expect("box");
        b.name = Some("Pin".into());
        let red = exact::StepPaint {
            color: Some([1.0, 0.0, 0.0]),
            faces: Vec::new(),
        };
        let text = exact::step_text_painted(&[a, b], "Asm", &[red]).expect("export");
        let mut p = PartDocument::default();
        p.add_body_op(BodyOpFeature::new(BodyOp::Import(ImportOp::new(
            "two.step",
            text.as_str(),
        ))))
        .expect("import");
        let shown = p.bodies_before(usize::MAX);
        assert_eq!(
            shown,
            vec![
                ("Import1".to_owned(), "Bracket".to_owned()),
                ("Import1#2".to_owned(), "Pin".to_owned())
            ]
        );
        let look = p
            .looks
            .bodies
            .get("Import1")
            .and_then(|s| s.appearance.clone());
        assert_eq!(look.as_deref(), Some("Color FF0000"));
        assert!(p.looks.appearances.iter().any(|a| a.name == "Color FF0000"));
        let pin = p
            .looks
            .bodies
            .get("Import1#2")
            .and_then(|s| s.appearance.clone());
        assert!(
            pin.is_none() || pin.as_deref() != Some("Color FF0000"),
            "{pin:?}"
        );
        // Saved and loaded: the names come back (the colour is document data).
        let json = serde_json::to_string(&p.features).expect("save");
        let mut q = PartDocument {
            features: serde_json::from_str(&json).expect("load"),
            looks: p.looks.clone(),
            ..Default::default()
        };
        q.regenerate().expect("regenerate");
        assert_eq!(q.bodies_before(usize::MAX), shown);
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
        // The gone face is named as the part knew it (the Update window lists it).
        assert_eq!(r.gone.len(), 1);
        assert!(r.gone[0].starts_with("Import1:F"), "{:?}", r.gone);
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

    /// A 40 × 30 × 10 plate with a Ø10 through hole as other CAD systems write it: each rim of
    /// the hole one closed circle edge with one vertex, the hole's wall one face bounded by both.
    const PLATE_STEP: &str = include_str!("../../tests/data/plate_hole.stp");

    /// The plate read, written by our exporter and read again is still one valid solid of the
    /// same volume (ROADMAP K8: the hole's wall, one face round the cylinder with a seam, came out
    /// as three loops and failed Euler's formula).
    #[test]
    fn a_reexported_hole_reads_back_as_a_solid() {
        let valid = |text: &str| -> (f64, bool) {
            let r = exact::step_bodies(text, "C").expect("read");
            let s = &r.bodies[0].solid;
            let b = exact::to_brep_with(std::slice::from_ref(s), true).expect("brep");
            let ok = b
                .solids()
                .all(|(id, so)| !so.open && b.validate_solid(id).is_ok());
            (s.volume_at(1e-3).unwrap_or(0.0), ok)
        };
        let (v0, ok0) = valid(PLATE_STEP);
        let want = 40.0 * 30.0 * 10.0 - std::f64::consts::PI * 25.0 * 10.0;
        assert!(ok0 && (v0 - want).abs() < 1e-3 * want, "{v0} {ok0}");
        let r = exact::step_bodies(PLATE_STEP, "C").expect("read");
        let again = exact::step_text(&[r.bodies[0].solid.clone()], "Plate").expect("export");
        let (v1, ok1) = valid(&again);
        assert!(ok1, "the re-export is no valid solid");
        assert!((v1 - v0).abs() < 1e-3 * v0, "{v1} vs {v0}");
    }
}
