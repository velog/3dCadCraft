//! ViewCube: an orientation cube in the viewport corner. It turns with the
//! model. Like Inventor's, it has 26 pickable directions — 6 faces, 12 edges
//! and 8 corners — and, while looking straight at a face, four arrows to
//! the neighbouring faces and two arrows that roll the view by 90°. A
//! compass ring under the cube turns the view about Z, the cube itself can
//! be dragged to orbit, a house icon goes Home, and clicks animate the view
//! ([`ViewCube`] holds the transition and drag state). It is drawn at
//! reduced opacity while the mouse is away.
//!
//! The axis triad (X red, Y green, Z blue) in the viewport's bottom-right
//! corner is drawn here too ([`draw_triad`]).
//!
//! Conventions (Z up): FRONT looks along +Y (camera at −Y), RIGHT along −X
//! (camera at +X), TOP down −Z (camera at +Z). Compass: N = +Y (BACK),
//! E = +X, S = −Y (FRONT), W = −X.

use eframe::egui::{
    Align2, Color32, FontId, Id, Painter, Pos2, Rect, Response, Shape, Stroke, Vec2,
    epaint::TextShape,
};
use nalgebra::Vector3;

use super::camera::{OrbitCamera, ViewAnim, wrap_angle};
use super::icons;

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
            // Straight down; the up vector is well defined at the pole, so
            // the front stays at the bottom of the screen.
            StandardView::Top => (-90.0, 90.0),
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
    /// The compass ring under the cube; `Some((x, y))` = one of its N/E/S/W
    /// letters (world direction).
    Compass(Option<(i8, i8)>),
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

/// Radius of the compass ring (cube units; the cube is [-1, 1]³). It lies
/// in the plane of the cube's bottom face.
const RING: f64 = 1.5;

/// Compass letters: world direction, label.
const COMPASS: [((i8, i8), &str); 4] =
    [((0, 1), "N"), ((1, 0), "E"), ((0, -1), "S"), ((-1, 0), "W")];

/// Opacity of the cube while the mouse is away (Inventor: Inactive
/// Opacity, 50 % by default).
pub const INACTIVE_OPACITY: f32 = 0.5;

/// Screen rectangle of the cube inside the viewport.
pub fn area(viewport: Rect) -> Rect {
    Rect::from_center_size(
        viewport.right_top() + Vec2::new(-88.0, 84.0),
        Vec2::splat(110.0),
    )
}

/// The neighbourhood in which the cube counts as active (opaque, arrows
/// shown): the cube's area with its arrows and Home button.
pub fn active_area(viewport: Rect) -> Rect {
    area(viewport).expand(26.0)
}

/// Rectangle of the house-shaped Home button at the cube's top-left
/// corner (where Inventor has it).
pub fn home_button(viewport: Rect) -> Rect {
    let a = area(viewport);
    Rect::from_center_size(Pos2::new(a.left() + 4.0, a.top() - 4.0), Vec2::splat(20.0))
}

/// A visible face: how much it faces the viewer, its label, normal and
/// in-face axes.
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
            half: a.width() * 0.30,
        }
    }

    fn to_screen(&self, v: Vector3<f64>) -> Pos2 {
        let r = self.rot * v;
        Pos2::new(
            self.center.x + r.x as f32 * self.half,
            self.center.y - r.y as f32 * self.half,
        )
    }

    /// Does the bottom face (and so the compass plane) face the viewer?
    fn seen_from_below(&self) -> bool {
        (self.rot * Vector3::new(0.0, 0.0, -1.0)).z > 0.0
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

/// Reading direction of a face label: (text right, text up) on the face.
fn label_axes(n: Vector3<f64>) -> (Vector3<f64>, Vector3<f64>) {
    if n.z > 0.5 {
        (Vector3::x(), Vector3::y())
    } else if n.z < -0.5 {
        (Vector3::x(), -Vector3::y())
    } else {
        (Vector3::z().cross(&n), Vector3::z())
    }
}

/// The compass ring as a closed screen polyline.
fn ring_points(p: &Proj) -> Vec<Pos2> {
    (0..64)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / 64.0;
            p.to_screen(Vector3::new(RING * t.cos(), RING * t.sin(), -1.0))
        })
        .collect()
}

/// Screen position of a compass letter.
fn letter_pos(p: &Proj, (x, y): (i8, i8)) -> Pos2 {
    p.to_screen(Vector3::new(RING * x as f64, RING * y as f64, -1.0))
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
    let r = a.width() * 0.52;
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
        (1, c + Vec2::new(r - 4.0, -r - 6.0)),
        (-1, c + Vec2::new(r + 12.0, -r + 10.0)),
    ]
}

/// Colours of the cube, with the opacity applied.
struct Colors {
    alpha: f32,
}

impl Colors {
    fn c(&self, c: Color32) -> Color32 {
        c.gamma_multiply(self.alpha)
    }
}

/// Draws the cube; the part under `hover` is highlighted. Fully opaque
/// while the pointer is near (or `active`), [`INACTIVE_OPACITY`] otherwise,
/// fading in between.
pub fn draw(
    painter: &Painter,
    cam: &OrbitCamera,
    viewport: Rect,
    hover: Option<Pos2>,
    active: bool,
) {
    let near = active || hover.is_some_and(|p| active_area(viewport).contains(p));
    let t = painter
        .ctx()
        .animate_bool_with_time(Id::new("viewcube-active"), near, 0.2);
    let k = Colors {
        alpha: INACTIVE_OPACITY + (1.0 - INACTIVE_OPACITY) * t,
    };
    let hit = hover.and_then(|p| pick(cam, viewport, p));
    let proj = Proj::new(cam, viewport);
    let hot = Color32::from_rgb(120, 170, 235);
    let hot_fill = Color32::from_rgba_unmultiplied(110, 165, 240, 200);
    let edge = Stroke::new(1.0, k.c(Color32::from_gray(80)));

    // Compass ring: behind the cube unless seen from below.
    let draw_ring = |painter: &Painter| {
        let ring_hot = matches!(hit, Some(CubeHit::Compass(_)));
        let ring = ring_points(&proj);
        let col = if ring_hot {
            hot
        } else {
            Color32::from_gray(150)
        };
        painter.add(Shape::closed_line(ring, Stroke::new(4.0, k.c(col))));
        for (dir, label) in COMPASS {
            let at = letter_pos(&proj, dir);
            let on = hit == Some(CubeHit::Compass(Some(dir)));
            painter.circle_filled(at, 7.0, k.c(if on { hot } else { Color32::from_gray(205) }));
            painter.text(
                at,
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(10.0),
                k.c(Color32::from_gray(30)),
            );
        }
    };
    let below = proj.seen_from_below();
    if !below {
        draw_ring(painter);
    }

    for f in visible_faces(&proj) {
        // Shaded: brighter the more the face looks at the viewer.
        let l = 0.72 + 0.28 * f.facing as f32;
        let base = [222.0, 228.0, 238.0].map(|c: f32| (c * l) as u8);
        let fill = k.c(Color32::from_rgb(base[0], base[1], base[2]));
        let quad = cell(&proj, &f, -1.0, 1.0, -1.0, 1.0);
        painter.add(Shape::convex_polygon(quad.clone(), fill, edge));
        if let Some(CubeHit::Dir(d)) = hit {
            for (q, dir) in regions(&proj, &f) {
                if dir == d {
                    painter.add(Shape::convex_polygon(q, k.c(hot_fill), Stroke::NONE));
                }
            }
        }
        if f.facing > 0.3 {
            // The label lies on the face: turned with its reading direction.
            let (right, _) = label_axes(f.n);
            let c = proj.to_screen(f.n);
            let r = proj.to_screen(f.n + right) - c;
            let angle = r.y.atan2(r.x);
            let size = 9.0 + 2.5 * f.facing as f32;
            let galley = painter.layout_no_wrap(
                f.label.to_owned(),
                FontId::proportional(size),
                k.c(Color32::from_gray(35)),
            );
            let pos = c - galley.size() / 2.0;
            painter.add(
                TextShape::new(pos, galley, Color32::from_gray(35))
                    .with_angle_and_anchor(angle, Align2::CENTER_CENTER),
            );
        }
    }
    if below {
        draw_ring(painter);
    }

    if face_on(cam).is_some() && t > 0.01 {
        // Arrows and roll buttons fade out with the cube (Inventor hides
        // them while inactive).
        let ink = Color32::from_gray(150).gamma_multiply(t);
        let hot_a = hot.gamma_multiply(t);
        for (x, y, tri) in arrows(viewport) {
            let c = if hit == Some(CubeHit::Arrow(x, y)) {
                hot_a
            } else {
                ink
            };
            painter.add(Shape::convex_polygon(tri, c, Stroke::NONE));
        }
        for (sign, c) in roll_buttons(viewport) {
            let col = if hit == Some(CubeHit::Roll(sign)) {
                hot_a
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

    // Home: a house icon.
    let home = home_button(viewport);
    if hit == Some(CubeHit::Home) {
        painter.rect_filled(
            home,
            3.0,
            k.c(Color32::from_rgba_unmultiplied(110, 165, 240, 90)),
        );
    }
    let base = icons::Palette::ENABLED;
    let pal = icons::Palette {
        ink: k.c(base.ink),
        accent: k.c(if hit == Some(CubeHit::Home) {
            hot
        } else {
            base.accent
        }),
        hot: k.c(base.hot),
        warn: k.c(base.warn),
        ok: k.c(base.ok),
    };
    icons::paint(painter, home.shrink(1.0), icons::Icon::Home, &pal);
}

/// What is under `pos`: Home, a face/edge/corner, an arrow, a roll button
/// or the compass. The cube beats the compass ring.
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
    for (dir, _) in COMPASS {
        if letter_pos(&proj, dir).distance(pos) <= 7.5 {
            return Some(CubeHit::Compass(Some(dir)));
        }
    }
    let ring = ring_points(&proj);
    let near_ring = (0..ring.len()).any(|i| {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        segment_distance(a, b, pos) <= 5.0
    });
    near_ring.then_some(CubeHit::Compass(None))
}

/// The world direction (from the target to the eye) for a hit on a face,
/// edge or corner.
pub fn direction(d: [i8; 3]) -> Vector3<f64> {
    Vector3::new(d[0] as f64, d[1] as f64, d[2] as f64)
}

/// For a compass letter: look from that side, keeping the elevation.
pub fn compass_direction(cam: &OrbitCamera, (x, y): (i8, i8)) -> Vector3<f64> {
    let d = (cam.eye() - cam.target).normalize();
    let elev = d.z.clamp(-1.0, 1.0).asin();
    Vector3::new(x as f64 * elev.cos(), y as f64 * elev.cos(), elev.sin())
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

fn segment_distance(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// What a drag on the cube does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Dragging the cube: free orbit.
    Orbit,
    /// Dragging the compass ring: turn about Z; the last pointer angle
    /// around the ring centre (radians, counter-clockwise on screen).
    Compass(Option<f32>),
}

/// Interaction state of the ViewCube: the running view transition and a
/// drag that started on the cube or its compass.
#[derive(Clone, Debug, Default)]
pub struct ViewCube {
    anim: Option<ViewAnim>,
    /// The camera the animation set last; anyone else changing the camera
    /// (Look At, Fit, orbit) ends the animation.
    anim_cam: Option<OrbitCamera>,
    drag: Option<(Drag, CubeHit)>,
    /// Time of the current frame (seconds), from [`ViewCube::tick`].
    clock: f64,
}

fn same_camera(a: &OrbitCamera, b: &OrbitCamera) -> bool {
    a.yaw == b.yaw
        && a.pitch == b.pitch
        && a.roll == b.roll
        && a.distance == b.distance
        && a.target == b.target
}

impl ViewCube {
    /// Advances a running view transition to time `now`; returns true while
    /// it is still running (the caller repaints).
    pub fn tick(&mut self, cam: &mut OrbitCamera, now: f64) -> bool {
        self.clock = now;
        let Some(anim) = &self.anim else {
            return false;
        };
        if self.anim_cam.as_ref().is_some_and(|c| !same_camera(c, cam)) {
            // Someone else moved the camera: let them have it.
            self.anim = None;
            self.anim_cam = None;
            return false;
        }
        let (c, done) = anim.at(now);
        *cam = c.clone();
        if done {
            self.anim = None;
            self.anim_cam = None;
        } else {
            self.anim_cam = Some(c);
        }
        !done
    }

    /// Turns the jump from `from` to the current camera into a smooth
    /// transition: the camera goes back to `from` and moves to its current
    /// value over [`super::camera::VIEW_ANIM_SECONDS`].
    pub fn animate(&mut self, from: OrbitCamera, cam: &mut OrbitCamera) {
        if same_camera(&from, cam) {
            return;
        }
        let to = std::mem::replace(cam, from.clone());
        cam.projection = to.projection;
        self.anim = Some(ViewAnim::new(cam.clone(), to, self.clock));
        self.anim_cam = Some(cam.clone());
    }

    /// Is a transition running?
    #[cfg(test)]
    pub fn animating(&self) -> bool {
        self.anim.is_some()
    }

    /// Is a drag that started on the cube or compass in progress?
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The part of the cube a running drag started on.
    pub fn drag_hit(&self) -> Option<CubeHit> {
        self.drag.map(|(_, h)| h)
    }

    /// Primary-button drags on the cube (orbit) and on the compass ring
    /// (turn about Z). `press` is where the button went down. Returns true
    /// while the cube owns the drag. Any drag or scroll in the viewport ends
    /// a running transition.
    pub fn input(
        &mut self,
        cam: &mut OrbitCamera,
        viewport: Rect,
        response: &Response,
        press: Option<Pos2>,
        scrolled: bool,
    ) -> bool {
        use eframe::egui::PointerButton;
        if response.dragged() || scrolled {
            self.anim = None;
            self.anim_cam = None;
        }
        if response.drag_started_by(PointerButton::Primary)
            && let Some(p) = press
        {
            self.drag = match pick(cam, viewport, p) {
                Some(h @ CubeHit::Dir(_)) => Some((Drag::Orbit, h)),
                Some(h @ CubeHit::Compass(_)) => Some((Drag::Compass(None), h)),
                _ => None,
            };
        }
        let Some((mode, hit)) = self.drag else {
            return false;
        };
        if response.dragged_by(PointerButton::Primary) {
            let delta = response.drag_delta();
            match mode {
                Drag::Orbit => cam.orbit(delta.x, delta.y),
                Drag::Compass(last) => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let now = compass_drag(cam, viewport, pos, last, delta);
                        self.drag = Some((Drag::Compass(now), hit));
                    }
                }
            }
        }
        if response.drag_stopped() || !response.is_pointer_button_down_on() {
            self.drag = None;
        }
        true
    }
}

/// One step of a compass drag at `pos`: turns the camera about Z by the
/// pointer's angle around the ring centre (the ring follows the mouse), or
/// by the horizontal motion while the ring is seen edge-on. Returns the
/// pointer angle to remember.
fn compass_drag(
    cam: &mut OrbitCamera,
    viewport: Rect,
    pos: Pos2,
    last: Option<f32>,
    delta: Vec2,
) -> Option<f32> {
    let proj = Proj::new(cam, viewport);
    let d = (cam.eye() - cam.target).normalize();
    if d.z.abs() < 0.25 {
        cam.orbit(delta.x, 0.0);
        return None;
    }
    let c = proj.to_screen(Vector3::new(0.0, 0.0, -1.0));
    let v = pos - c;
    if v.length() < 3.0 {
        return last;
    }
    let angle = (-v.y).atan2(v.x);
    if let Some(prev) = last {
        let da = wrap_angle((angle - prev) as f64);
        // Seen from above, a counter-clockwise ring turns the model
        // counter-clockwise: the eye goes the other way.
        cam.turn_about_z(if d.z > 0.0 { -da } else { da });
    }
    Some(angle)
}

/// Draws the X/Y/Z axis triad in the viewport's bottom-right corner,
/// turning with the view (Inventor's 3D indicator; it sits bottom-left
/// there).
pub fn draw_triad(painter: &Painter, cam: &OrbitCamera, viewport: Rect) {
    let view = cam.view();
    let rot = view.fixed_view::<3, 3>(0, 0).into_owned();
    let origin = triad_origin(viewport);
    let len = 34.0;
    let mut axes = [
        (Vector3::x(), "X", Color32::from_rgb(220, 60, 55)),
        (Vector3::y(), "Y", Color32::from_rgb(60, 175, 70)),
        (Vector3::z(), "Z", Color32::from_rgb(60, 110, 230)),
    ]
    .map(|(a, l, c)| (rot * a, l, c));
    // Far axes first, so the nearer ones are drawn over them.
    axes.sort_by(|a, b| a.0.z.total_cmp(&b.0.z));
    for (r, label, col) in axes {
        let dir = Vec2::new(r.x as f32, -r.y as f32);
        let tip = origin + dir * len;
        painter.line_segment([origin, tip], Stroke::new(2.5, col));
        if dir.length() > 0.15 {
            let d = dir.normalized();
            let n = Vec2::new(-d.y, d.x) * 3.5;
            painter.add(Shape::convex_polygon(
                vec![tip + d * 5.0, tip + n, tip - n],
                col,
                Stroke::NONE,
            ));
        }
        let at = tip
            + if dir.length() > 0.15 {
                dir.normalized() * 12.0
            } else {
                Vec2::new(9.0, -9.0)
            };
        painter.text(
            at,
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(12.0),
            col,
        );
    }
    painter.circle_filled(origin, 2.5, Color32::from_gray(170));
}

/// Where the triad's origin sits: inside the bottom-right corner, with room
/// for the axes and labels in every orientation.
pub fn triad_origin(viewport: Rect) -> Pos2 {
    viewport.right_bottom() + Vec2::new(-60.0, -60.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Rect {
        Rect::from_min_size(Pos2::new(250.0, 110.0), Vec2::new(850.0, 765.0))
    }

    fn cam_at(yaw: f64, pitch: f64) -> OrbitCamera {
        OrbitCamera {
            yaw: yaw.to_radians(),
            pitch: pitch.to_radians(),
            ..Default::default()
        }
    }

    /// The screen point of `v` in cube space picks the direction of `v`'s
    /// region, for every visible face/edge/corner region.
    fn assert_regions_pick(cam: &OrbitCamera) {
        let vp = viewport();
        let proj = Proj::new(cam, vp);
        let mut tested = 0;
        for f in visible_faces(&proj) {
            if f.facing < 0.3 {
                continue; // too oblique: its bands are slivers
            }
            for (cs, ct) in [(-1, -1), (-1, 0), (0, 0), (1, 0), (1, 1), (0, -1)] {
                let s = cs as f64 * (1.0 - BAND / 2.0);
                let t = ct as f64 * (1.0 - BAND / 2.0);
                let d = f.n + f.u * cs as f64 + f.w * ct as f64;
                let dir = [d.x.round() as i8, d.y.round() as i8, d.z.round() as i8];
                let p = proj.to_screen(f.n + f.u * s + f.w * t);
                assert_eq!(
                    pick(cam, vp, p),
                    Some(CubeHit::Dir(dir)),
                    "{} region ({cs},{ct}) at yaw {} pitch {}",
                    f.label,
                    cam.yaw.to_degrees(),
                    cam.pitch.to_degrees()
                );
                tested += 1;
            }
        }
        assert!(tested >= 6);
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
    fn regions_pick_at_many_orientations_also_upside_down() {
        for (yaw, pitch) in [
            (-60.0, 30.0),
            (30.0, -40.0),
            (150.0, 60.0),
            (-60.0, 150.0), // over the top, upside down
            (20.0, -135.0), // under the bottom, upside down
            (-90.0, 90.0),  // top
            (-90.0, -90.0), // bottom
            (45.0, 180.0),  // horizontal, upside down
        ] {
            assert_regions_pick(&cam_at(yaw, pitch));
        }
        // With a rolled image too.
        let mut cam = cam_at(-60.0, 30.0);
        cam.roll = 0.7;
        assert_regions_pick(&cam);
    }

    #[test]
    fn upside_down_shows_the_bottom_face_upwards() {
        // Orbited over the top from the front: the top face is still
        // visible, the screen is upside down.
        let cam = cam_at(-90.0, 120.0);
        let vp = viewport();
        let proj = Proj::new(&cam, vp);
        let top = proj.to_screen(Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(pick(&cam, vp, top), Some(CubeHit::Dir([0, 0, 1])));
        // The BACK face is visible now and, upside down, appears *above*
        // the top face (world Z points down the screen).
        let back = proj.to_screen(Vector3::new(0.0, 1.0, 0.0));
        assert_eq!(pick(&cam, vp, back), Some(CubeHit::Dir([0, 1, 0])));
        assert!(back.y < top.y);
        let z = proj.to_screen(Vector3::new(0.0, 0.0, 1.0)) - proj.to_screen(Vector3::zeros());
        assert!(z.y > 0.0, "Z points down on screen");
    }

    #[test]
    fn compass_ring_and_letters_are_pickable() {
        let vp = viewport();
        for cam in [
            cam_at(-60.0, 30.0),
            cam_at(-90.0, 90.0),
            cam_at(-60.0, 150.0),
        ] {
            let proj = Proj::new(&cam, vp);
            // A ring point not covered by the cube.
            let mut found = false;
            for i in 0..32 {
                let t = std::f64::consts::TAU * i as f64 / 32.0 + 0.1;
                let p = proj.to_screen(Vector3::new(RING * t.cos(), RING * t.sin(), -1.0));
                match pick(&cam, vp, p) {
                    Some(CubeHit::Compass(_)) => found = true,
                    Some(CubeHit::Dir(_)) | Some(CubeHit::Home) | None => {}
                    other => panic!("unexpected {other:?}"),
                }
            }
            assert!(found, "ring pickable");
        }
        // From the top, all four letters are clear of the cube.
        let cam = cam_at(-90.0, 90.0);
        let proj = Proj::new(&cam, vp);
        for (dir, _) in COMPASS {
            let p = letter_pos(&proj, dir);
            assert_eq!(pick(&cam, vp, p), Some(CubeHit::Compass(Some(dir))));
        }
        // N is up on screen in the top view (+Y at the top, front below).
        let n = letter_pos(&proj, (0, 1));
        let s = letter_pos(&proj, (0, -1));
        assert!(n.y < s.y);
        // The point between the cube and nothing: far away misses.
        assert_eq!(
            pick(&cam, vp, area(vp).center() + Vec2::new(0.0, 200.0)),
            None
        );
    }

    #[test]
    fn compass_drag_turns_about_z_following_the_mouse() {
        let vp = viewport();
        let mut cam = cam_at(-90.0, 90.0); // from the top
        let proj = Proj::new(&cam, vp);
        let c = proj.to_screen(Vector3::new(0.0, 0.0, -1.0));
        let r = proj.half * RING as f32;
        // Start at E, move counter-clockwise on screen by 10°.
        let p0 = c + Vec2::new(r, 0.0);
        let a = 10f32.to_radians();
        let p1 = c + Vec2::new(r * a.cos(), -r * a.sin());
        let last = compass_drag(&mut cam, vp, p0, None, Vec2::ZERO);
        let yaw0 = cam.yaw;
        let east_before = Proj::new(&cam, vp).to_screen(Vector3::new(RING, 0.0, -1.0));
        compass_drag(&mut cam, vp, p1, last, Vec2::ZERO);
        assert!((cam.yaw - (yaw0 - a as f64)).abs() < 1e-4);
        // The E letter followed the mouse.
        let east_after = Proj::new(&cam, vp).to_screen(Vector3::new(RING, 0.0, -1.0));
        assert!(east_after.distance(p1) < 1.0, "{east_after:?} vs {p1:?}");
        assert!(east_before.distance(p0) < 1.0);
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
        // From the top: up → the back face (+Y), down → the front.
        let (yaw, pitch) = StandardView::Top.angles();
        cam.yaw = yaw.to_radians();
        cam.pitch = pitch.to_radians();
        assert_eq!(arrow_direction(&cam, 0, 1), Some(Vector3::y()));
        assert_eq!(arrow_direction(&cam, 0, -1), Some(-Vector3::y()));
    }

    #[test]
    fn home_button_is_inside_the_viewport_and_picked_first() {
        let vp = viewport();
        let h = home_button(vp);
        assert!(vp.contains_rect(h));
        let cam = OrbitCamera::default();
        assert_eq!(pick(&cam, vp, h.center()), Some(CubeHit::Home));
        // The whole active area stays within the viewport corner.
        assert!(vp.contains_rect(active_area(vp)));
    }

    #[test]
    fn animation_goes_to_the_clicked_view_and_yields_to_others() {
        let mut vc = ViewCube::default();
        let mut cam = OrbitCamera::default();
        vc.tick(&mut cam, 10.0);
        let before = cam.clone();
        cam.look_from(direction([0, 0, 1]));
        let target = cam.clone();
        vc.animate(before.clone(), &mut cam);
        assert!(vc.animating());
        // Back at the start right after the click.
        assert!((cam.eye() - before.eye()).norm() < 1e-9);
        assert!(vc.tick(&mut cam, 10.15));
        let mid = cam.eye();
        assert!((mid - before.eye()).norm() > 1.0 && (mid - target.eye()).norm() > 1.0);
        assert!(!vc.tick(&mut cam, 10.31));
        assert!(!vc.animating());
        assert_eq!((cam.yaw, cam.pitch), (target.yaw, target.pitch));
        // Someone else changing the camera mid-way ends the transition.
        let before = cam.clone();
        cam.look_from(direction([1, 0, 0]));
        vc.animate(before, &mut cam);
        vc.tick(&mut cam, 10.4);
        cam.yaw += 0.3; // e.g. Look At
        let mine = cam.clone();
        assert!(!vc.tick(&mut cam, 10.45));
        assert_eq!(cam.yaw, mine.yaw);
    }

    #[test]
    fn triad_stays_inside_the_viewport() {
        let vp = viewport();
        let o = triad_origin(vp);
        // Axis (34) + label offset (12) + text half (~6) in any direction.
        let r = Rect::from_center_size(o, Vec2::splat(2.0 * 52.0));
        assert!(vp.contains_rect(r));
        // And clear of the cube.
        assert!(!r.intersects(active_area(vp)));
    }
}
