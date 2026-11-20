//! **3MF** (3D Manufacturing Format, Core specification 1.3): a ZIP package whose model part
//! (`3D/3dmodel.model`, XML) lists objects as triangle meshes and a build of placed items.
//! Read for Import as mesh bodies (spec 7.7a 3.14, beside STL and OBJ) and written by File ›
//! Export (spec 7.7), one object per body colour group, shared vertices, millimetres.
//!
//! After SolidCraft's `crates/io/src/threemf.rs` (MIT OR Apache-2.0: one mesh per body, shared
//! vertices, the build's transforms); the reader is a small XML scanner of our own (the model
//! part needs only elements and attributes).

use std::io::{Read, Write};

use crate::kernel::{Point3, Real};

/// The most triangles a 3MF file may bring (untrusted input).
const LIMIT: usize = 5_000_000;
/// The largest model part read (bytes, unpacked).
const MAX_MODEL: u64 = 1 << 30;

/// One placed object: a name, its triangles in mm.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Object3mf {
    /// Its name.
    pub name: String,
    /// Vertex positions (mm).
    pub positions: Vec<Point3>,
    /// Triangles as vertex indices, counter-clockwise seen from outside.
    pub triangles: Vec<[u32; 3]>,
}

/// A start or empty tag: its name (namespace prefix dropped) and attributes.
struct Tag<'a> {
    name: &'a str,
    attrs: Vec<(&'a str, String)>,
    /// `</name>`.
    end: bool,
}

impl Tag<'_> {
    fn get(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.as_str())
    }
    fn num(&self, k: &str) -> Option<Real> {
        self.get(k)?
            .trim()
            .parse()
            .ok()
            .filter(|v: &Real| v.is_finite())
    }
}

fn unescape(v: &str) -> String {
    v.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Every tag of `xml`, in order (comments, declarations and text skipped).
fn tags(xml: &str) -> Vec<Tag<'_>> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let Some(j) = rest.find('>') else { break };
        let body = &rest[..j];
        rest = &rest[j + 1..];
        if body.starts_with('?') || body.starts_with('!') {
            continue;
        }
        let (end, body) = match body.strip_prefix('/') {
            Some(b) => (true, b),
            None => (false, body.trim_end_matches('/')),
        };
        let name_end = body.find(|c: char| c.is_whitespace()).unwrap_or(body.len());
        let full = &body[..name_end];
        let name = full.rsplit(':').next().unwrap_or(full);
        let mut attrs = Vec::new();
        let mut a = &body[name_end..];
        while let Some(eq) = a.find('=') {
            let key = a[..eq].trim();
            let after = a[eq + 1..].trim_start();
            let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
                break;
            };
            let Some(close) = after[1..].find(q) else {
                break;
            };
            let key = key.rsplit(':').next().unwrap_or(key);
            attrs.push((key, unescape(&after[1..1 + close])));
            a = &after[close + 2..];
        }
        out.push(Tag { name, attrs, end });
    }
    out
}

/// A 3MF transform `m00 m01 m02 m10 m11 m12 m20 m21 m22 m30 m31 m32` (row vectors: p' = p M).
type Affine = [Real; 12];
const IDENTITY: Affine = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

fn affine(t: Option<&str>) -> Affine {
    let v: Vec<Real> = t
        .unwrap_or("")
        .split_whitespace()
        .filter_map(|x| x.parse().ok())
        .collect();
    v.try_into().unwrap_or(IDENTITY)
}

fn apply(m: &Affine, p: Point3) -> Point3 {
    Point3::new(
        p.x * m[0] + p.y * m[3] + p.z * m[6] + m[9],
        p.x * m[1] + p.y * m[4] + p.z * m[7] + m[10],
        p.x * m[2] + p.y * m[5] + p.z * m[8] + m[11],
    )
}

/// `a` then `b`.
fn compose(a: &Affine, b: &Affine) -> Affine {
    let mut r = [0.0; 12];
    for row in 0..4 {
        for col in 0..3 {
            let mut v = if row == 3 { b[9 + col] } else { 0.0 };
            for k in 0..3 {
                v += a[row * 3 + k] * b[k * 3 + col];
            }
            r[row * 3 + col] = v;
        }
    }
    r
}

/// An object of the model part: its own mesh, or components (object id, transform).
#[derive(Default)]
struct Def {
    name: String,
    positions: Vec<Point3>,
    triangles: Vec<[u32; 3]>,
    components: Vec<(String, Affine)>,
}

/// The size of the model's unit in mm.
fn unit_mm(u: Option<&str>) -> Real {
    match u.unwrap_or("millimeter") {
        "micron" => 0.001,
        "centimeter" => 10.0,
        "meter" => 1000.0,
        "inch" => 25.4,
        "foot" => 304.8,
        _ => 1.0,
    }
}

/// Reads a 3MF package's build: every item's object (components expanded, transforms applied),
/// in mm.
pub fn read(bytes: &[u8]) -> Result<Vec<Object3mf>, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("3MF: not a 3MF package ({e})"))?;
    // The model part: named in the root relationships, else the usual place.
    let mut target = "3D/3dmodel.model".to_string();
    if let Ok(mut rels) = zip.by_name("_rels/.rels") {
        let mut s = String::new();
        let _ = rels.read_to_string(&mut s);
        if let Some(t) = tags(&s)
            .iter()
            .find(|t| {
                t.name == "Relationship" && t.get("Type").is_some_and(|x| x.ends_with("/3dmodel"))
            })
            .and_then(|t| t.get("Target"))
        {
            target = t.trim_start_matches('/').to_string();
        }
    }
    let mut model = String::new();
    {
        let f = zip
            .by_name(&target)
            .map_err(|_| format!("3MF: the package has no model part ({target})"))?;
        f.take(MAX_MODEL)
            .read_to_string(&mut model)
            .map_err(|e| format!("3MF: {e}"))?;
    }
    let all = tags(&model);
    let scale = unit_mm(
        all.iter()
            .find(|t| t.name == "model")
            .and_then(|t| t.get("unit")),
    );
    let mut defs: std::collections::HashMap<String, Def> = Default::default();
    let mut build: Vec<(String, Affine)> = Vec::new();
    let mut cur: Option<(String, Def)> = None;
    let mut count = 0usize;
    for t in &all {
        match (t.name, t.end) {
            ("object", false) => {
                let id = t.get("id").unwrap_or("").to_string();
                let def = Def {
                    name: t.get("name").unwrap_or("").to_string(),
                    ..Default::default()
                };
                cur = Some((id, def));
            }
            ("object", true) => {
                if let Some((id, d)) = cur.take() {
                    defs.insert(id, d);
                }
            }
            ("vertex", false) => {
                if let (Some((_, d)), Some(x), Some(y), Some(z)) =
                    (cur.as_mut(), t.num("x"), t.num("y"), t.num("z"))
                {
                    d.positions
                        .push(Point3::new(x * scale, y * scale, z * scale));
                }
            }
            ("triangle", false) => {
                let v = |k: &str| t.get(k).and_then(|x| x.trim().parse::<u32>().ok());
                if let (Some((_, d)), Some(a), Some(b), Some(c)) =
                    (cur.as_mut(), v("v1"), v("v2"), v("v3"))
                {
                    count += 1;
                    if count > LIMIT {
                        return Err(format!("3MF: more than {LIMIT} triangles"));
                    }
                    let n = d.positions.len() as u32;
                    if a >= n || b >= n || c >= n {
                        return Err("3MF: a triangle names a vertex that is not there".into());
                    }
                    d.triangles.push([a, b, c]);
                }
            }
            ("component", false) => {
                if let (Some((_, d)), Some(id)) = (cur.as_mut(), t.get("objectid")) {
                    let mut m = affine(t.get("transform"));
                    m[9..].iter_mut().for_each(|x| *x *= scale);
                    d.components.push((id.to_string(), m));
                }
            }
            ("item", false) => {
                if let Some(id) = t.get("objectid") {
                    let mut m = affine(t.get("transform"));
                    m[9..].iter_mut().for_each(|x| *x *= scale);
                    build.push((id.to_string(), m));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    // Walks an object (components up to 8 deep), collecting its triangles moved by `m`.
    fn walk(
        defs: &std::collections::HashMap<String, Def>,
        id: &str,
        m: &Affine,
        depth: usize,
        o: &mut Object3mf,
    ) {
        let Some(d) = defs.get(id) else { return };
        if o.name.is_empty() {
            o.name = d.name.clone();
        }
        let base = o.positions.len() as u32;
        o.positions.extend(d.positions.iter().map(|p| apply(m, *p)));
        // A mirroring transform turns the triangles inside out: keep them facing out.
        let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
            + m[2] * (m[3] * m[7] - m[4] * m[6]);
        o.triangles.extend(d.triangles.iter().map(|t| {
            if det < 0.0 {
                [t[0] + base, t[2] + base, t[1] + base]
            } else {
                t.map(|i| i + base)
            }
        }));
        if depth < 8 {
            for (cid, cm) in &d.components {
                walk(defs, cid, &compose(cm, m), depth + 1, o);
            }
        }
    }
    for (k, (id, m)) in build.iter().enumerate() {
        let mut o = Object3mf::default();
        walk(&defs, id, m, 0, &mut o);
        if o.name.is_empty() {
            o.name = format!("Object{}", k + 1);
        }
        if !o.triangles.is_empty() {
            out.push(o);
        }
    }
    if out.is_empty() {
        return Err("3MF: the build has no mesh".into());
    }
    Ok(out)
}

fn esc(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A 3MF package of `objects` (mm; each one object of the model and one build item; `colors`,
/// when given per object, as its base material colour, sRGB 0–255).
pub fn write(objects: &[Object3mf], colors: &[Option<[u8; 3]>]) -> Result<Vec<u8>, String> {
    use std::fmt::Write as _;
    let mut model = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<model unit=\"millimeter\" xml:lang=\"en-US\" ",
        "xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n",
        " <metadata name=\"Application\">3DCadCraft</metadata>\n",
        " <resources>\n"
    ));
    let painted = colors.iter().any(Option::is_some);
    if painted {
        model += "  <basematerials id=\"1\">\n";
        for (k, c) in colors.iter().enumerate() {
            let c = c.unwrap_or([180, 180, 180]);
            let _ = writeln!(
                model,
                "   <base name=\"Color{k}\" displaycolor=\"#{:02X}{:02X}{:02X}\"/>",
                c[0], c[1], c[2]
            );
        }
        model += "  </basematerials>\n";
    }
    for (k, o) in objects.iter().enumerate() {
        let id = k + 2;
        let paint = if painted {
            format!(" pid=\"1\" pindex=\"{k}\"")
        } else {
            String::new()
        };
        let _ = writeln!(
            model,
            "  <object id=\"{id}\" type=\"model\" name=\"{}\"{paint}>\n   <mesh>\n    <vertices>",
            esc(&o.name)
        );
        for p in &o.positions {
            let _ = writeln!(
                model,
                "     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>",
                p.x, p.y, p.z
            );
        }
        model += "    </vertices>\n    <triangles>\n";
        for t in &o.triangles {
            let _ = writeln!(
                model,
                "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>",
                t[0], t[1], t[2]
            );
        }
        model += "    </triangles>\n   </mesh>\n  </object>\n";
    }
    model += " </resources>\n <build>\n";
    for k in 0..objects.len() {
        let _ = writeln!(model, "  <item objectid=\"{}\"/>", k + 2);
    }
    model += " </build>\n</model>\n";
    let types = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n",
        " <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n",
        " <Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>\n",
        "</Types>\n"
    );
    let rels = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n",
        " <Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" ",
        "Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/>\n",
        "</Relationships>\n"
    );
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        for (name, text) in [
            ("[Content_Types].xml", types),
            ("_rels/.rels", rels),
            ("3D/3dmodel.model", model.as_str()),
        ] {
            zip.start_file(name, opts).map_err(|e| e.to_string())?;
            zip.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetra() -> Object3mf {
        Object3mf {
            name: "Tet & Co".into(),
            positions: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(10.0, 0.0, 0.0),
                Point3::new(0.0, 10.0, 0.0),
                Point3::new(0.0, 0.0, 10.0),
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]],
        }
    }

    fn volume(o: &Object3mf) -> Real {
        o.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| o.positions[i as usize].coords);
                a.dot(&b.cross(&c)) / 6.0
            })
            .sum()
    }

    #[test]
    fn written_and_read_back() {
        let bytes = write(&[tetra()], &[Some([255, 0, 0])]).unwrap();
        let back = read(&bytes).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "Tet & Co");
        assert_eq!(back[0].triangles.len(), 4);
        assert!((volume(&back[0]) - 1000.0 / 6.0).abs() < 1e-9);
    }

    /// Components, a build transform that mirrors, and inches.
    #[test]
    fn components_transforms_and_units() {
        let model = r#"<?xml version="1.0"?>
<model unit="inch" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
 <resources>
  <object id="1" type="model"><mesh><vertices>
   <vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/><vertex x="0" y="0" z="1"/>
  </vertices><triangles>
   <triangle v1="0" v2="2" v3="1"/><triangle v1="0" v2="1" v3="3"/><triangle v1="1" v2="2" v3="3"/><triangle v1="0" v2="3" v3="2"/>
  </triangles></mesh></object>
  <object id="2" type="model" name="Pair"><components>
   <component objectid="1"/>
   <component objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/>
  </components></object>
 </resources>
 <build><item objectid="2" transform="-1 0 0 0 1 0 0 0 1 0 0 0"/></build>
</model>"#;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("3D/3dmodel.model", o).unwrap();
            z.write_all(model.as_bytes()).unwrap();
            z.finish().unwrap();
        }
        let back = read(buf.get_ref()).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "Pair");
        assert_eq!(back[0].triangles.len(), 8);
        // Two tetrahedra of 1 in, mirrored: still facing out (positive volume).
        let v = 25.4f64.powi(3) / 6.0;
        assert!(
            (volume(&back[0]) - 2.0 * v).abs() < 1e-6 * v,
            "{}",
            volume(&back[0])
        );
        // The second sits 5 in along −X after the mirror.
        assert!(
            back[0]
                .positions
                .iter()
                .any(|p| (p.x + 5.0 * 25.4).abs() < 1e-9)
        );
    }

    #[test]
    fn not_3mf_is_an_error() {
        assert!(read(b"hello").is_err());
    }
}
