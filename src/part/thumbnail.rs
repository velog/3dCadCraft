//! Preview picture (thumbnail) of a document, stored in the project file as
//! `thumbnail.png` and shown by the Open dialog (`docs/spec/0.6-files.md`
//! §3.0a). A small software renderer draws the display triangles (smooth
//! shading in each face's appearance) and the edges, 2× supersampled, onto a
//! transparent background: it runs the same in the GUI, the API and headless
//! saves, and never waits for the GPU.

use crate::kernel::{Point3, TessellationOptions, Transform3, Vector3};
use crate::materials::Appearance;
use crate::part::PartDocument;
use crate::part::appearance::{LookSource, Resolver};

/// Side of the stored picture in pixels.
pub const SIZE: u32 = 256;

/// Surface bodies without an appearance of their own (as in the view).
const SURFACE_AMBER: [f32; 3] = [0.86, 0.62, 0.32];
/// Edge colour (linear).
const EDGE: [f32; 3] = [0.02, 0.02, 0.04];

/// One triangle: corners, their normals, linear colour and opacity.
struct Tri {
    p: [Point3; 3],
    n: [Vector3; 3],
    rgb: [f32; 3],
    alpha: f32,
}

/// What a picture shows: triangles and edge polylines in one frame.
#[derive(Default)]
pub struct Scene {
    tris: Vec<Tri>,
    edges: Vec<Vec<Point3>>,
}

impl Scene {
    /// Nothing to draw.
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Adds every visible body of `part`, moved by `place` (an assembly
    /// occurrence), each face in its resolved appearance.
    pub fn add_part(&mut self, part: &PartDocument, place: Option<&Transform3>) {
        let body = &part.body;
        let looks = Resolver::new(part).face_looks();
        let fallback = (Appearance::fallback(), LookSource::Material);
        let opts = TessellationOptions::default();
        for (id, solid) in body.solids() {
            if solid.name.as_deref().is_some_and(|n| !part.body_visible(n)) {
                continue;
            }
            let Ok(mesh) = body.tessellate_solid(id, &opts) else {
                continue;
            };
            let f = &mesh.faces;
            let at = |i: u32| -> (Point3, Vector3) {
                let p = f.positions.get(i as usize).copied().unwrap_or_default();
                let n = f.normals.get(i as usize).copied().unwrap_or_default();
                match place {
                    Some(t) => (t * p, t * n),
                    None => (p, n),
                }
            };
            for (fid, range) in &mesh.face_triangles {
                let (look, src) = looks.get(fid).unwrap_or(&fallback);
                let rgb = if solid.open && *src == LookSource::Material {
                    SURFACE_AMBER
                } else {
                    look.shader_rgb()
                };
                let alpha = look.opacity.clamp(0.0, 1.0) as f32;
                for t in f.triangles.get(range.clone()).unwrap_or_default() {
                    let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
                    self.tris.push(Tri {
                        p: [a.0, b.0, c.0],
                        n: [a.1, b.1, c.1],
                        rgb,
                        alpha,
                    });
                }
            }
            for e in mesh.edges {
                self.edges.push(match place {
                    Some(t) => e.into_iter().map(|p| t * p).collect(),
                    None => e,
                });
            }
        }
    }
}

/// The view of the default picture: from the front-right-top corner
/// (front = −Y, right = +X, Z up), as the isometric home view.
pub fn iso_view() -> Vector3 {
    Vector3::new(1.0, -1.0, 1.0).normalize()
}

/// A picture of `part` as PNG bytes (`None`: nothing to draw).
pub fn part_png(part: &PartDocument, view: Vector3) -> Option<Vec<u8>> {
    let mut scene = Scene::default();
    scene.add_part(part, None);
    png(&scene, view, SIZE)
}

/// `scene` seen along `-view` (`view` points at the eye), fitted, as PNG.
pub fn png(scene: &Scene, view: Vector3, size: u32) -> Option<Vec<u8>> {
    let rgba = render(scene, view, size)?;
    encode_png(&rgba, size, size)
}

/// `scene` as `size`² RGBA pixels (sRGB, straight alpha); `None` when empty.
pub fn render(scene: &Scene, view: Vector3, size: u32) -> Option<Vec<u8>> {
    if scene.is_empty() || size == 0 {
        return None;
    }
    let back = view.try_normalize(1e-12).unwrap_or_else(iso_view);
    // Screen axes: right square to Z (or to Y looking straight down / up).
    let up_world = if back.z.abs() > 0.999 {
        Vector3::y()
    } else {
        Vector3::z()
    };
    let right = up_world.cross(&back).normalize();
    let up = back.cross(&right);
    let eye = |p: &Point3| {
        let v = p.coords;
        [v.dot(&right), v.dot(&up), v.dot(&back)]
    };
    // Fit the triangles (edges lie on them) with a margin.
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for t in &scene.tris {
        for p in &t.p {
            let e = eye(p);
            for k in 0..2 {
                lo[k] = lo[k].min(e[k]);
                hi[k] = hi[k].max(e[k]);
            }
        }
    }
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-9);
    let ss = (size * 2) as usize; // supersampled side
    let scale = ss as f64 * 0.88 / span;
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let half = ss as f64 / 2.0;
    let to_px = |p: &Point3| {
        let e = eye(p);
        [
            half + (e[0] - mid[0]) * scale,
            half - (e[1] - mid[1]) * scale,
            e[2],
        ]
    };
    // Light: mostly from the eye, a little from the upper left.
    let light = (back + up * 0.45 - right * 0.25).normalize();
    let shade = |n: &Vector3| {
        let d = n.dot(&light).abs() as f32;
        0.28 + 0.72 * d
    };
    let mut depth = vec![f64::MIN; ss * ss];
    let mut color = vec![[0f32; 4]; ss * ss];
    let draw = |t: &Tri, blend: bool, depth: &mut [f64], color: &mut [[f32; 4]]| {
        let q = [to_px(&t.p[0]), to_px(&t.p[1]), to_px(&t.p[2])];
        let area =
            (q[1][0] - q[0][0]) * (q[2][1] - q[0][1]) - (q[2][0] - q[0][0]) * (q[1][1] - q[0][1]);
        if area.abs() < 1e-12 {
            return;
        }
        let s = [shade(&t.n[0]), shade(&t.n[1]), shade(&t.n[2])];
        let x0 = q
            .iter()
            .map(|v| v[0])
            .fold(f64::MAX, f64::min)
            .floor()
            .max(0.0) as usize;
        let x1 = (q.iter().map(|v| v[0]).fold(f64::MIN, f64::max).ceil() as usize).min(ss - 1);
        let y0 = q
            .iter()
            .map(|v| v[1])
            .fold(f64::MAX, f64::min)
            .floor()
            .max(0.0) as usize;
        let y1 = (q.iter().map(|v| v[1]).fold(f64::MIN, f64::max).ceil() as usize).min(ss - 1);
        for y in y0..=y1 {
            let py = y as f64 + 0.5;
            for x in x0..=x1 {
                let px = x as f64 + 0.5;
                let w0 = ((q[1][0] - px) * (q[2][1] - py) - (q[2][0] - px) * (q[1][1] - py)) / area;
                let w1 = ((q[2][0] - px) * (q[0][1] - py) - (q[0][0] - px) * (q[2][1] - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                    continue;
                }
                let z = w0 * q[0][2] + w1 * q[1][2] + w2 * q[2][2];
                let k = y * ss + x;
                if z <= depth[k] {
                    continue;
                }
                let l = w0 as f32 * s[0] + w1 as f32 * s[1] + w2 as f32 * s[2];
                let c = [t.rgb[0] * l, t.rgb[1] * l, t.rgb[2] * l];
                let px = &mut color[k];
                if blend {
                    let a = t.alpha;
                    for i in 0..3 {
                        px[i] = px[i] * (1.0 - a) + c[i] * a;
                    }
                    px[3] = px[3] + a * (1.0 - px[3]);
                } else {
                    depth[k] = z;
                    *px = [c[0], c[1], c[2], 1.0];
                }
            }
        }
    };
    for t in scene.tris.iter().filter(|t| t.alpha >= 0.999) {
        draw(t, false, &mut depth, &mut color);
    }
    // Edges over the opaque faces they lie on (a small depth allowance).
    let bias = 2e-3 * span;
    for e in &scene.edges {
        for w in e.windows(2) {
            let (a, b) = (to_px(&w[0]), to_px(&w[1]));
            let steps = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil() as usize).max(1);
            for i in 0..=steps {
                let f = i as f64 / steps as f64;
                let (x, y, z) = (
                    a[0] + (b[0] - a[0]) * f,
                    a[1] + (b[1] - a[1]) * f,
                    a[2] + (b[2] - a[2]) * f,
                );
                // A 2 × 2 pen at the supersampled size (one picture pixel).
                for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                    let (px, py) = (x - 0.5 + dx, y - 0.5 + dy);
                    if px < 0.0 || py < 0.0 || px >= ss as f64 || py >= ss as f64 {
                        continue;
                    }
                    let k = py as usize * ss + px as usize;
                    if z + bias >= depth[k] {
                        color[k] = [EDGE[0], EDGE[1], EDGE[2], 1.0];
                    }
                }
            }
        }
    }
    for t in scene.tris.iter().filter(|t| t.alpha < 0.999) {
        draw(t, true, &mut depth, &mut color);
    }
    // 2 × 2 box filter, premultiplied, then sRGB (the view's 1/2.2).
    let n = size as usize;
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let mut acc = [0f32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let c = color[(2 * y + dy) * ss + 2 * x + dx];
                for i in 0..3 {
                    acc[i] += c[i] * c[3];
                }
                acc[3] += c[3];
            }
            let a = acc[3] / 4.0;
            let o = &mut out[(y * n + x) * 4..(y * n + x) * 4 + 4];
            if a > 0.0 {
                for i in 0..3 {
                    let lin = (acc[i] / 4.0 / a).clamp(0.0, 1.0);
                    o[i] = (lin.powf(1.0 / 2.2) * 255.0).round() as u8;
                }
            }
            o[3] = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    Some(out)
}

/// RGBA pixels as PNG bytes.
pub fn encode_png(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut bytes, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
        writer.finish().ok()?;
    }
    Some(bytes)
}

/// PNG bytes as (width, height, RGBA pixels).
pub fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[g, a]| [g, g, g, a])
            .collect(),
        _ => return None,
    };
    Some((info.width, info.height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed() -> PartDocument {
        PartDocument::with_box("Box1", [40.0, 20.0, 10.0]).unwrap_or_default()
    }

    #[test]
    fn a_box_fills_the_middle_and_leaves_the_corners_clear() {
        let part = boxed();
        let mut scene = Scene::default();
        scene.add_part(&part, None);
        assert!(!scene.is_empty());
        let px = render(&scene, iso_view(), 64).unwrap_or_default();
        assert_eq!(px.len(), 64 * 64 * 4);
        let at = |x: usize, y: usize| &px[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(at(32, 32)[3], 255, "the middle is the part");
        assert_eq!(at(0, 0)[3], 0, "the corner is transparent");
        // Faces facing different ways are shaded differently.
        let opaque: std::collections::BTreeSet<[u8; 3]> = px
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|c| c[3] == 255)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        assert!(opaque.len() > 3, "{} shades", opaque.len());
    }

    #[test]
    fn the_same_part_gives_the_same_bytes_and_round_trips() {
        let part = boxed();
        let a = part_png(&part, iso_view()).unwrap_or_default();
        let b = part_png(&part.clone(), iso_view()).unwrap_or_default();
        assert!(!a.is_empty());
        assert_eq!(a, b, "deterministic");
        let (w, h, rgba) = decode_png(&a).unwrap_or_default();
        assert_eq!((w, h, rgba.len()), (SIZE, SIZE, (SIZE * SIZE * 4) as usize));
    }

    #[test]
    fn empty_part_has_no_picture_and_views_straight_down_work() {
        assert!(part_png(&PartDocument::default(), iso_view()).is_none());
        let part = boxed();
        for v in [Vector3::z(), -Vector3::z(), Vector3::x(), Vector3::zeros()] {
            assert!(part_png(&part, v).is_some(), "{v:?}");
        }
    }
}
