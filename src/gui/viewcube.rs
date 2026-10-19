//! ViewCube: an orientation cube in the viewport corner. It turns with the
//! model. Like Inventor's, it has 26 pickable directions — 6 faces, 12 edges
//! and 8 corners — and, while looking straight at a face, four arrows to
//! the neighbouring faces and two arrows that roll the view by 90°.
//!
//! Conventions (Z up): FRONT looks along +Y (camera at −Y), RIGHT along −X
//! (camera at +X), TOP down −Z (camera at +Z).

use eframe::egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use nalgebra::Vector3;

use super::camera::OrbitCamera;

/// Standard camera orientations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandardView {
    /// Isometric-ish default view, fitted to the model.
    Home,
    /// From −Y.
    Front,
    /// From +X.
    Right,
    /// From +Z.
    Top,
}

impl StandardView {
    /// (yaw, pitch) in degrees for [`OrbitCamera`].
    pub fn angles(self) -> (f64, f64) {
        match self {
            StandardView::Home => (-60.0, 30.0),
            StandardView::Front => (-90.0, 0.0),
            StandardView::Right => (0.0, 0.0),
            StandardView::Top => (-90.0, 89.0),
        }
    }
}

/// What the pointer is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CubeHit {
    /// The Home button.
    Home,
    /// A face, edge or corner: the camera goes to this side (components in
    /// −1, 0, 1; one non-zero = face, two = edge, three = corner).
    Dir([i8; 3]),
    /// Arrow to the neighbouring face: screen direction (right, up).
    Arrow(i8, i8),
    /// Roll the view by ±90° (positive = counter-clockwise).
    Roll(i8),
}

/// Faces: outward normal, label.
const FACES: [([f64; 3], &str); 6] = [
    ([0.0, -1.0, 0.0], "FRONT"),
    ([0.0, 1.0, 0.0], "BACK"),
    ([-1.0, 0.0, 0.0], "LEFT"),
    ([1.0, 0.0, 0.0], "RIGHT"),
    ([0.0, 0.0, 1.0], "TOP"),
    ([0.0, 0.0, -1.0], "BOTTOM"),
];

/// Width of the edge/corner bands on a face (in face units, the face is
/// [-1, 1]²).
const BAND: f64 = 0.4;

/// Screen rectangle of the cube inside the viewport.
pub fn area(viewport: Rect) -> Rect {
    Rect::from_center_size(
        viewport.right_top() + Vec2::new(-88.0, 84.0),
        Vec2::splat(110.0),
    )
}

/// Rectangle of the small "Home" button at the cube's top-left corner
/// (where Inventor has its house icon).
pub fn home_button(viewport: Rect) -> Rect {
    let a = area(viewport);
    Rect::from_center_size(
        Pos2::new(a.left() + 6.0, a.top() - 6.0),
        Vec2::new(44.0, 16.0),
    )
}

/// A visible face: its corners on screen, how much it faces the viewer,
/// its label, normal and in-face axes.
struct Face {
    facing: f64,
    label: &'static str,
    n: Vector3<f64>,
    u: Vector3<f64>,
    w: Vector3<f64>,
}

/// Screen mapping of cube space.
struct Proj {
    rot: nalgebra::Matrix3<f64>,
    center: Pos2,
    half: f32,
}

impl Proj {
    fn new(cam: &OrbitCamera, viewport: Rect) -> Self {
        let view = cam.view();
        let a = area(viewport);
        Proj {
            rot: view.fixed_view::<3, 3>(0, 0).into_owned(),
            center: a.center(),
            half: a.width() * 0.32,
        }
    }

    fn to_screen(&self, v: Vector3<f64>) -> Pos2 {
        let r = self.rot * v;
        Pos2::new(
            self.center.x + r.x as f32 * self.half,
            self.center.y - r.y as f32 * self.half,
        )
    }
}

fn visible_faces(p: &Proj) -> Vec<Face> {
    let mut faces = Vec::new();
    for (n, label) in FACES {
        let n = Vector3::from(n);
        let facing = (p.rot * n).z;
        if facing <= 1e-3 {
            continue;
        }
        let u = if n.z.abs() > 0.5 {
            Vector3::x()
        } else {
            Vector3::z()
        };
        let w = n.cross(&u);
        faces.push(Face {
            facing,
            label,
            n,
            u,
            w,
        });
    }
    faces
}

/// The quad of a face region `s ∈ [s0, s1], t ∈ [t0, t1]`.
fn cell(p: &Proj, f: &Face, s0: f64, s1: f64, t0: f64, t1: f64) -> Vec<Pos2> {
    [(s0, t0), (s1, t0), (s1, t1), (s0, t1)]
        .map(|(s, t)| p.to_screen(f.n + f.u * s + f.w * t))
        .to_vec()
}

/// The 9 regions of a face: (quad, direction).
fn regions(p: &Proj, f: &Face) -> Vec<(Vec<Pos2>, [i8; 3])> {
    let bands = [
        (-1.0, -1.0 + BAND, -1),
        (-1.0 + BAND, 1.0 - BAND, 0),
        (1.0 - BAND, 1.0, 1),
    ];
    let mut out = Vec::with_capacity(9);
    for &(s0, s1, cs) in &bands {
        for &(t0, t1, ct) in &bands {
            let d = f.n + f.u * cs as f64 + f.w * ct as f64;
            let dir = [d.x.round() as i8, d.y.round() as i8, d.z.round() as i8];
            out.push((cell(p, f, s0, s1, t0, t1), dir));
        }
    }
    out
}

/// The axis-aligned direction the camera looks from, if it looks straight
/// at a face.
fn face_on(cam: &OrbitCamera) -> Option<Vector3<f64>> {
    let d = (cam.eye() - cam.target).normalize();
    let k = d.iamax();
    (d[k].abs() > 0.999).then(|| {
        let mut v = Vector3::zeros();
        v[k] = d[k].signum();
        v
    })
}

/// Arrow triangles around the cube (screen right/up, polygon).
fn arrows(viewport: Rect) -> Vec<(i8, i8, Vec<Pos2>)> {
    let a = area(viewport);
    let c = a.center();
    let r = a.width() * 0.48;
    let s = 7.0;
    [(1i8, 0i8), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .map(|(x, y)| {
            let dir = Vec2::new(x as f32, -y as f32);
            let tip = c + dir * (r + s);
            let side = Vec2::new(-dir.y, dir.x) * s;
            (x, y, vec![tip, tip - dir * s + side, tip - dir * s - side])
        })
        .collect()
}

/// Roll buttons (sign, centre).
fn roll_buttons(viewport: Rect) -> [(i8, Pos2); 2] {
    let a = area(viewport);
    let c = a.center();
    let r = a.width() * 0.48;
    [
        (1, c + Vec2::new(r - 6.0, -r - 6.0)),
        (-1, c + Vec2::new(r + 10.0, -r + 10.0)),
    ]
}

/// Draws the cube; the part under `hover` is highlighted.
pub fn draw(painter: &Painter, cam: &OrbitCamera, viewport: Rect, hover: Option<Pos2>) {
    let hit = hover.and_then(|p| pick(cam, viewport, p));
    let proj = Proj::new(cam, viewport);
    let hot = Color32::from_rgb(120, 170, 235);
    for f in visible_faces(&proj) {
        let shade = (150.0 + 90.0 * f.facing) as u8;
        let fill = Color32::from_rgb(shade, shade, shade.saturating_add(8));
        let quad = cell(&proj, &f, -1.0, 1.0, -1.0, 1.0);
        painter.add(Shape::convex_polygon(
            quad.clone(),
            fill,
            Stroke::new(1.0, Color32::from_gray(70)),
        ));
        if let Some(CubeHit::Dir(d)) = hit {
            for (q, dir) in regions(&proj, &f) {
                if dir == d {
                    painter.add(Shape::convex_polygon(q, hot, Stroke::NONE));
                }
            }
        }
        if f.facing > 0.35 {
            let c = quad.iter().fold(Vec2::ZERO, |acc, p| acc + p.to_vec2()) / 4.0;
            painter.text(
                c.to_pos2(),
                Align2::CENTER_CENTER,
                f.label,
                FontId::proportional(11.0),
                Color32::from_gray(30),
            );
        }
    }
    if face_on(cam).is_some() {
        let ink = Color32::from_gray(150);
        for (x, y, tri) in arrows(viewport) {
            let c = if hit == Some(CubeHit::Arrow(x, y)) {
                hot
            } else {
                ink
            };
            painter.add(Shape::convex_polygon(tri, c, Stroke::NONE));
        }
        for (sign, c) in roll_buttons(viewport) {
            let col = if hit == Some(CubeHit::Roll(sign)) {
                hot
            } else {
                ink
            };
            let r = 6.0;
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = (-30.0 + 240.0 * i as f32 / 12.0).to_radians();
                    c + Vec2::new(t.cos(), -t.sin()) * r
                })
                .collect();
            let (end, prev) = if sign > 0 {
                (pts[12], pts[11])
            } else {
                (pts[0], pts[1])
            };
            painter.add(Shape::line(pts, Stroke::new(2.0, col)));
            let d = (end - prev).normalized();
            let n = Vec2::new(-d.y, d.x) * 3.5;
            painter.add(Shape::convex_polygon(
                vec![end + d * 4.0, end + n, end - n],
                col,
                Stroke::NONE,
            ));
        }
    }
    let home = home_button(viewport);
    let fill = if hit == Some(CubeHit::Home) {
        hot
    } else {
        Color32::from_gray(225)
    };
    painter.rect_filled(home, 3.0, fill);
    painter.text(
        home.center(),
        Align2::CENTER_CENTER,
        "Home",
        FontId::proportional(11.0),
        Color32::from_gray(30),
    );
}

/// What is under `pos`: Home, a face/edge/corner, an arrow or a roll button.
pub fn pick(cam: &OrbitCamera, viewport: Rect, pos: Pos2) -> Option<CubeHit> {
    if home_button(viewport).contains(pos) {
        return Some(CubeHit::Home);
    }
    if face_on(cam).is_some() {
        for (x, y, tri) in arrows(viewport) {
            let c = tri.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2()) / 3.0;
            if c.to_pos2().distance(pos) <= 9.0 {
                return Some(CubeHit::Arrow(x, y));
            }
        }
        for (sign, c) in roll_buttons(viewport) {
            if c.distance(pos) <= 8.0 {
                return Some(CubeHit::Roll(sign));
            }
        }
    }
    let proj = Proj::new(cam, viewport);
    let mut faces = visible_faces(&proj);
    faces.sort_by(|a, b| b.facing.total_cmp(&a.facing));
    for f in &faces {
        for (q, dir) in regions(&proj, f) {
            if inside_convex(&q, pos) {
                return Some(CubeHit::Dir(dir));
            }
        }
    }
    None
}

/// The world direction (from the target to the eye) for a hit on a face,
/// edge or corner.
pub fn direction(d: [i8; 3]) -> Vector3<f64> {
    Vector3::new(d[0] as f64, d[1] as f64, d[2] as f64)
}

/// For an arrow: the new view direction (the neighbouring face in that
/// screen direction).
pub fn arrow_direction(cam: &OrbitCamera, x: i8, y: i8) -> Option<Vector3<f64>> {
    face_on(cam)?;
    let view = cam.view();
    let right = Vector3::new(view[(0, 0)], view[(0, 1)], view[(0, 2)]);
    let up = Vector3::new(view[(1, 0)], view[(1, 1)], view[(1, 2)]);
    let v = right * x as f64 + up * y as f64;
    let k = v.iamax();
    let mut out = Vector3::zeros();
    out[k] = v[k].signum();
    Some(out)
}

fn inside_convex(poly: &[Pos2], p: Pos2) -> bool {
    let sign = |a: Pos2, b: Pos2| (b - a).x * (p - a).y - (b - a).y * (p - a).x;
    let s: Vec<f32> = (0..poly.len())
        .map(|i| sign(poly[i], poly[(i + 1) % poly.len()]))
        .collect();
    s.iter().all(|&v| v >= 0.0) || s.iter().all(|&v| v <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Rect {
        Rect::from_min_size(Pos2::new(250.0, 110.0), Vec2::new(850.0, 765.0))
    }

    #[test]
    fn corners_edges_and_faces_are_pickable() {
        let cam = OrbitCamera::default(); // home: front-right-top visible
        let vp = viewport();
        let proj = Proj::new(&cam, vp);
        // The screen point of the top-front-right corner picks that corner.
        let corner = proj.to_screen(Vector3::new(0.95, -0.95, 1.0));
        assert_eq!(pick(&cam, vp, corner), Some(CubeHit::Dir([1, -1, 1])));
        let edge = proj.to_screen(Vector3::new(0.0, -0.95, 1.0));
        assert_eq!(pick(&cam, vp, edge), Some(CubeHit::Dir([0, -1, 1])));
        let face = proj.to_screen(Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(pick(&cam, vp, face), Some(CubeHit::Dir([0, 0, 1])));
        // No arrows in an oblique view.
        assert!(arrow_direction(&cam, 1, 0).is_none());
    }

    #[test]
    fn arrows_go_to_the_neighbouring_faces() {
        let mut cam = OrbitCamera::default();
        let (yaw, pitch) = StandardView::Front.angles();
        cam.yaw = yaw.to_radians();
        cam.pitch = pitch.to_radians();
        // From the front: right → the right face (+X), up → the top (+Z).
        assert_eq!(arrow_direction(&cam, 1, 0), Some(Vector3::x()));
        assert_eq!(arrow_direction(&cam, 0, 1), Some(Vector3::z()));
        assert_eq!(arrow_direction(&cam, -1, 0), Some(-Vector3::x()));
    }
}
