//! Export of a part for other programs (`docs/spec/7.7-export.md`):
//! **STL** (binary, for 3D printing), **STEP** (exact B-rep, for other CAD
//! systems, through the exact kernel) and **GLB** (binary glTF 2.0, for
//! viewers and the web). Mesh formats use the body's display triangles;
//! STL keeps millimetres, glTF is in metres (its unit) with the model's
//! Z-up turned to glTF's Y-up.

use std::path::Path;

use super::PartDocument;
use super::appearance::{LookSource, Resolver};
use crate::kernel::{Point3, Real, TessellationOptions, Vector3};
use crate::materials::Appearance;

/// Export formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    /// Binary STL (triangles, mm).
    Stl,
    /// STEP AP214/AP242 B-rep.
    Step,
    /// Binary glTF 2.0.
    Glb,
}

impl ExportFormat {
    /// Every format, for menus.
    pub const ALL: [ExportFormat; 3] = [ExportFormat::Stl, ExportFormat::Step, ExportFormat::Glb];

    /// The file extension (without the dot).
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Stl => "stl",
            ExportFormat::Step => "step",
            ExportFormat::Glb => "glb",
        }
    }

    /// The format a path's extension names.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "stl" => Some(ExportFormat::Stl),
            "step" | "stp" => Some(ExportFormat::Step),
            "glb" => Some(ExportFormat::Glb),
            _ => None,
        }
    }
}

/// The triangles of every body of the part (outward, counter-clockwise).
#[derive(Clone, Debug, Default)]
pub struct ExportMesh {
    /// Vertex positions (mm).
    pub positions: Vec<Point3>,
    /// Unit normals per vertex.
    pub normals: Vec<Vector3>,
    /// Triangles, sorted by appearance (see `groups`).
    pub triangles: Vec<[u32; 3]>,
    /// Runs of `triangles` sharing one appearance (empty: one default
    /// material).
    pub groups: Vec<ExportGroup>,
}

/// Triangles of one appearance.
#[derive(Clone, Debug)]
pub struct ExportGroup {
    /// The resolved appearance (face > feature > body > part > material).
    pub appearance: Appearance,
    /// Range of `ExportMesh::triangles`.
    pub triangles: std::ops::Range<usize>,
}

impl ExportMesh {
    /// Collects the part's bodies, each face with its appearance.
    pub fn of(part: &PartDocument) -> Result<Self, String> {
        let opts = TessellationOptions::default();
        let looks = Resolver::new(part).face_looks();
        let mut m = ExportMesh::default();
        // Appearance → triangles, in order of first use.
        let mut by_look: Vec<(Appearance, Vec<[u32; 3]>)> = Vec::new();
        for (sid, _) in part.body.solids() {
            let s = part
                .body
                .tessellate_solid(sid, &opts)
                .map_err(|e| e.to_string())?;
            let base = m.positions.len() as u32;
            m.positions.extend(s.faces.positions.iter().copied());
            m.normals.extend(s.faces.normals.iter().copied());
            for (fid, range) in &s.face_triangles {
                let look = looks
                    .get(fid)
                    .map_or_else(Appearance::fallback, |(a, _)| a.clone());
                let k = match by_look.iter().position(|(a, _)| *a == look) {
                    Some(k) => k,
                    None => {
                        by_look.push((look, Vec::new()));
                        by_look.len() - 1
                    }
                };
                by_look[k].1.extend(
                    s.faces.triangles[range.clone()]
                        .iter()
                        .map(|t| t.map(|i| i + base)),
                );
            }
        }
        for (appearance, tris) in by_look {
            let start = m.triangles.len();
            m.triangles.extend(tris);
            m.groups.push(ExportGroup {
                appearance,
                triangles: start..m.triangles.len(),
            });
        }
        if m.triangles.is_empty() {
            return Err("the part has no body".into());
        }
        Ok(m)
    }

    /// Enclosed volume (mm³; divergence theorem).
    pub fn volume(&self) -> Real {
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.positions[i as usize].coords);
                a.dot(&b.cross(&c)) / 6.0
            })
            .sum()
    }
}

/// Binary STL: 80-byte header, triangle count, then per triangle its
/// facet normal and three vertices (f32, mm) and a zero attribute.
pub fn stl_bytes(m: &ExportMesh, name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(84 + m.triangles.len() * 50);
    let mut header = format!("3DCadCraft {name} (mm)").into_bytes();
    header.resize(80, b' ');
    out.extend_from_slice(&header);
    out.extend_from_slice(&(m.triangles.len() as u32).to_le_bytes());
    let f = |out: &mut Vec<u8>, v: Real| out.extend_from_slice(&(v as f32).to_le_bytes());
    for t in &m.triangles {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        let n = (b - a).cross(&(c - a));
        let n = n.try_normalize(1e-300).unwrap_or_else(Vector3::zeros);
        for v in [n.x, n.y, n.z] {
            f(&mut out, v);
        }
        for p in [a, b, c] {
            for v in [p.x, p.y, p.z] {
                f(&mut out, v);
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

/// Binary glTF 2.0 (GLB): one mesh with positions, normals and indices
/// in one buffer, metres, Y up, a grey-blue metallic-roughness material.
pub fn glb_bytes(m: &ExportMesh, name: &str) -> Vec<u8> {
    // Z-up mm → Y-up m: (x, y, z) → (x, z, −y) / 1000.
    let to_gltf = |x: Real, y: Real, z: Real| [x, z, -y];
    let pos: Vec<[f32; 3]> = m
        .positions
        .iter()
        .map(|p| to_gltf(p.x / 1000.0, p.y / 1000.0, p.z / 1000.0).map(|v| v as f32))
        .collect();
    let nor: Vec<[f32; 3]> = m
        .normals
        .iter()
        .map(|n| to_gltf(n.x, n.y, n.z).map(|v| v as f32))
        .collect();
    let mut bin: Vec<u8> = Vec::new();
    for p in &pos {
        for v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let nor_offset = bin.len();
    for n in &nor {
        for v in n {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let idx_offset = bin.len();
    for t in &m.triangles {
        for i in t {
            bin.extend_from_slice(&i.to_le_bytes());
        }
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &pos {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let n_vert = pos.len();
    let groups: Vec<ExportGroup> = if m.groups.is_empty() {
        vec![ExportGroup {
            appearance: Appearance::fallback(),
            triangles: 0..m.triangles.len(),
        }]
    } else {
        m.groups.clone()
    };
    let mut primitives = Vec::new();
    let mut materials = Vec::new();
    let mut views = vec![
        serde_json::json!({ "buffer": 0, "byteOffset": 0, "byteLength": nor_offset, "target": 34962 }),
        serde_json::json!({ "buffer": 0, "byteOffset": nor_offset,
            "byteLength": idx_offset - nor_offset, "target": 34962 }),
    ];
    let mut accessors = vec![
        serde_json::json!({ "bufferView": 0, "componentType": 5126, "count": n_vert,
            "type": "VEC3", "min": lo, "max": hi }),
        serde_json::json!({ "bufferView": 1, "componentType": 5126, "count": n_vert, "type": "VEC3" }),
    ];
    // One primitive (own index range) and material per appearance.
    for (k, g) in groups.iter().enumerate() {
        let a = &g.appearance;
        let [r, gr, b] = a.linear();
        let mut mat = serde_json::json!({
            "name": a.name,
            "pbrMetallicRoughness": {
                "baseColorFactor": [r, gr, b, a.opacity as f32],
                "metallicFactor": a.metalness.unwrap_or(0.1),
                "roughnessFactor": a.roughness.unwrap_or(0.6)
            }
        });
        if a.is_transparent() {
            mat["alphaMode"] = serde_json::json!("BLEND");
            mat["doubleSided"] = serde_json::json!(true);
        }
        materials.push(mat);
        views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": idx_offset + g.triangles.start * 12,
            "byteLength": g.triangles.len() * 12,
            "target": 34963
        }));
        accessors.push(serde_json::json!({
            "bufferView": 2 + k, "componentType": 5125,
            "count": g.triangles.len() * 3, "type": "SCALAR"
        }));
        primitives.push(serde_json::json!({
            "attributes": { "POSITION": 0, "NORMAL": 1 },
            "indices": 2 + k,
            "material": k
        }));
    }
    let json = serde_json::json!({
        "asset": { "version": "2.0", "generator": "3DCadCraft" },
        "scene": 0,
        "scenes": [ { "nodes": [0] } ],
        "nodes": [ { "mesh": 0, "name": name } ],
        "meshes": [ { "name": name, "primitives": primitives } ],
        "materials": materials,
        "buffers": [ { "byteLength": bin.len() } ],
        "bufferViews": views,
        "accessors": accessors
    });
    let mut js = json.to_string().into_bytes();
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// STEP text of the part's exact bodies.
pub fn step_text(part: &PartDocument, name: &str) -> Result<String, String> {
    if part.exact_bodies.0.is_empty() {
        return Err(
            "STEP needs the exact kernel (this part was built with the faceted one)".into(),
        );
    }
    let r = Resolver::new(part);
    let srgb = |a: &Appearance| a.color.map(|c| c as f32 / 255.0);
    // Solids only: surface bodies (Split tools) are construction geometry.
    let solids: Vec<crate::kernel::exact::ExactSolid> = part
        .exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .cloned()
        .collect();
    let paint: Vec<crate::kernel::exact::StepPaint> = solids
        .iter()
        .map(|s| {
            let (body_look, _) = r.body_look(s.name.as_deref());
            // Face overrides land on the piece of that name nearest their
            // hint (face points are computed only for split names).
            let names = s.names();
            let mut own: std::collections::HashMap<usize, &str> = Default::default();
            for e in &part.looks.faces {
                let cands: Vec<usize> = (0..names.len())
                    .filter(|&i| names[i].as_deref() == Some(e.face.as_str()))
                    .collect();
                let pick = if cands.len() <= 1 {
                    cands.first().copied()
                } else {
                    let hint = Point3::from(e.hint);
                    cands
                        .into_iter()
                        .map(|i| {
                            let d = crate::kernel::exact::face_point(s, i)
                                .map_or(Real::INFINITY, |p| (p - hint).norm());
                            (i, d)
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(i, _)| i)
                };
                if let Some(i) = pick {
                    own.insert(i, e.appearance.as_str());
                }
            }
            let faces = names
                .iter()
                .enumerate()
                .filter_map(|(i, n)| {
                    let n = n.as_deref()?;
                    let (a, src) = r.face_look(s.name.as_deref(), Some(n), own.get(&i).copied());
                    matches!(src, LookSource::Face | LookSource::Feature)
                        .then(|| (i, srgb(&a), a.opacity as f32))
                })
                .collect();
            crate::kernel::exact::StepPaint {
                color: Some(srgb(&body_look)),
                faces,
            }
        })
        .collect();
    crate::kernel::exact::step_text_painted(&solids, name, &paint)
}

/// The file content for `format`.
pub fn export_bytes(
    part: &PartDocument,
    name: &str,
    format: ExportFormat,
) -> Result<Vec<u8>, String> {
    Ok(match format {
        ExportFormat::Stl => stl_bytes(&ExportMesh::of(part)?, name),
        ExportFormat::Glb => glb_bytes(&ExportMesh::of(part)?, name),
        ExportFormat::Step => step_text(part, name)?.into_bytes(),
    })
}

/// Writes the part to `path` in the format its extension names
/// (atomically: a temporary file renamed over the target).
pub fn export(part: &PartDocument, name: &str, path: &Path) -> Result<ExportFormat, String> {
    let started = std::time::Instant::now();
    let r = export_file(part, name, path);
    match &r {
        Ok(f) => log::info!(
            "export {} ({}): ok ({} ms)",
            path.display(),
            f.extension(),
            started.elapsed().as_millis()
        ),
        Err(e) => log::warn!("export {}: {e}", path.display()),
    }
    r
}

fn export_file(part: &PartDocument, name: &str, path: &Path) -> Result<ExportFormat, String> {
    let format = ExportFormat::from_path(path)
        .ok_or("the file name must end in .stl, .step / .stp or .glb")?;
    let bytes = export_bytes(part, name, format)?;
    let tmp = path.with_extension(format!("{}.tmp", format.extension()));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{BlendFeature, PartDocument};

    fn rounded_box() -> PartDocument {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let top = crate::part::select::edges_of_face(
            &part.body,
            part.body.find_face("Box1:Top").unwrap(),
        );
        let refs: Vec<_> = top
            .into_iter()
            .filter_map(|e| crate::part::blend::edge_ref(&part.body, e))
            .collect();
        part.add_blend(BlendFeature::fillet(refs, 3.0)).unwrap();
        part
    }

    /// Binary STL: size = 84 + 50 per triangle, the volume read back from
    /// the file's triangles is the body's.
    #[test]
    fn stl_round_trip_volume() {
        let part = rounded_box();
        let m = ExportMesh::of(&part).unwrap();
        let bytes = stl_bytes(&m, "Part1");
        let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        assert_eq!(n, m.triangles.len());
        assert_eq!(bytes.len(), 84 + 50 * n);
        let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as f64;
        let mut v = 0.0;
        for k in 0..n {
            let o = 84 + 50 * k + 12;
            let p = |j: usize| {
                nalgebra::Vector3::new(f(o + 12 * j), f(o + 12 * j + 4), f(o + 12 * j + 8))
            };
            v += p(0).dot(&p(1).cross(&p(2))) / 6.0;
        }
        // The f32 file holds the same solid as the mesh.
        assert!(
            (v - m.volume()).abs() < 1e-5 * m.volume(),
            "{v} vs {}",
            m.volume()
        );
        assert!(v > 23_000.0 && v < 24_000.0, "{v}");
        // Watertight for slicers: every edge (by rounded end points) is
        // shared by exactly two triangles.
        let mut edges: std::collections::HashMap<([i64; 3], [i64; 3]), u32> = Default::default();
        let key = |p: &Point3| [p.x, p.y, p.z].map(|x| (x * 1e4).round() as i64);
        for t in &m.triangles {
            let k = t.map(|i| key(&m.positions[i as usize]));
            for j in 0..3 {
                let (a, b) = (k[j], k[(j + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        assert!(
            edges.values().all(|&c| c == 2),
            "open or non-manifold edges"
        );
    }

    /// GLB: header, JSON chunk naming the accessors, binary chunk sized as
    /// declared, positions in metres with Y up.
    #[test]
    fn glb_structure() {
        let part = rounded_box();
        let m = ExportMesh::of(&part).unwrap();
        let b = glb_bytes(&m, "Part1");
        assert_eq!(&b[0..4], b"glTF");
        let total = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
        assert_eq!(total, b.len());
        let jlen = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
        assert_eq!(&b[16..20], b"JSON");
        let json: serde_json::Value = serde_json::from_slice(&b[20..20 + jlen]).unwrap();
        let blen = u32::from_le_bytes(b[20 + jlen..24 + jlen].try_into().unwrap()) as usize;
        assert_eq!(&b[24 + jlen..28 + jlen], b"BIN\0");
        assert_eq!(
            json["buffers"][0]["byteLength"].as_u64().unwrap() as usize,
            blen
        );
        assert_eq!(
            json["accessors"][2]["count"].as_u64().unwrap() as usize,
            m.triangles.len() * 3
        );
        // The 20 mm height is glTF's Y: 0.02 m.
        let hi = json["accessors"][0]["max"][1].as_f64().unwrap();
        assert!((hi - 0.02).abs() < 1e-6, "{hi}");
    }

    /// GLB with appearances: one material per look, base colour linear,
    /// the overridden face in its own primitive, transparent looks blended;
    /// the index ranges cover every triangle once.
    #[test]
    fn glb_carries_colours() {
        use crate::part::appearance::{LookTarget, face_hint};
        let lib = crate::materials::builtin();
        let mut part = rounded_box();
        part.set_material(Some("Steel S235"), lib).unwrap();
        let top = part.body.find_face("Box1:Top").unwrap();
        let hint = face_hint(&part.body, top).unwrap();
        part.set_appearance(
            &LookTarget::Face {
                face: "Box1:Top".into(),
                hint,
            },
            Some("Glass - Clear"),
            lib,
        )
        .unwrap();
        let m = ExportMesh::of(&part).unwrap();
        assert_eq!(m.groups.len(), 2);
        let covered: usize = m.groups.iter().map(|g| g.triangles.len()).sum();
        assert_eq!(covered, m.triangles.len());
        let b = glb_bytes(&m, "Part1");
        let jlen = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value = serde_json::from_slice(&b[20..20 + jlen]).unwrap();
        let mats = json["materials"].as_array().unwrap();
        assert_eq!(mats.len(), 2);
        let steel = mats.iter().find(|x| x["name"] == "Steel - Satin").unwrap();
        let c = &steel["pbrMetallicRoughness"]["baseColorFactor"];
        // sRGB 158 → linear 0.341.
        assert!((c[0].as_f64().unwrap() - 0.341).abs() < 0.002, "{c}");
        let glass = mats.iter().find(|x| x["name"] == "Glass - Clear").unwrap();
        assert_eq!(glass["alphaMode"], "BLEND");
        let alpha = glass["pbrMetallicRoughness"]["baseColorFactor"][3]
            .as_f64()
            .unwrap();
        assert!((alpha - 0.25).abs() < 1e-6);
        let prims = json["meshes"][0]["primitives"].as_array().unwrap();
        assert_eq!(prims.len(), 2);
        let total: u64 = prims
            .iter()
            .map(|p| {
                json["accessors"][p["indices"].as_u64().unwrap() as usize]["count"]
                    .as_u64()
                    .unwrap()
            })
            .sum();
        assert_eq!(total as usize, m.triangles.len() * 3);
    }

    /// STEP carries the body colour and the overridden face's colour.
    #[test]
    fn step_carries_colours() {
        use crate::part::appearance::LookTarget;
        let lib = crate::materials::builtin();
        let mut part = rounded_box();
        let custom = part.add_custom_appearance([255, 0, 0], 1.0);
        part.set_appearance(&LookTarget::Feature("Fillet1".into()), Some(&custom), lib)
            .unwrap();
        part.set_appearance(&LookTarget::Part, Some("ABS - Blue"), lib)
            .unwrap();
        let text = step_text(&part, "Part1").unwrap();
        // ABS - Blue: 40, 90, 186 → 0.157, 0.353, 0.729.
        assert!(text.contains("COLOUR_RGB"), "no colour");
        assert!(
            text.contains("COLOUR_RGB('',1.,0.,0.)") || text.contains("COLOUR_RGB('',1.0,0.0,0.0)"),
            "{}",
            text.lines()
                .filter(|l| l.contains("COLOUR_RGB"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let vols = crate::kernel::exact::step_volumes(&text).unwrap();
        assert_eq!(vols.len(), 1);
    }

    /// STEP: written from the exact bodies and read back by the kernel's
    /// importer with the same volume.
    #[test]
    fn step_round_trip_volume() {
        let part = rounded_box();
        assert!(part.exact);
        let text = step_text(&part, "Part1").unwrap();
        assert!(text.starts_with("ISO-10303-21;"), "{}", &text[..40]);
        let vols = crate::kernel::exact::step_volumes(&text).unwrap();
        assert_eq!(vols.len(), 1);
        let want = ExportMesh::of(&part).unwrap().volume();
        assert!(
            (vols[0] - want).abs() < 2e-3 * want,
            "{} vs {want}",
            vols[0]
        );
    }

    #[test]
    fn format_from_extension() {
        assert_eq!(
            ExportFormat::from_path(Path::new("a/b.STL")),
            Some(ExportFormat::Stl)
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("b.stp")),
            Some(ExportFormat::Step)
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("b.glb")),
            Some(ExportFormat::Glb)
        );
        assert_eq!(ExportFormat::from_path(Path::new("b.obj")), None);
    }
}
