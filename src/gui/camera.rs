//! Orbit camera with CAD conventions (Z up, orbit around a target point).

use eframe::egui::{Pos2, Rect};
use nalgebra::{Matrix4, Rotation3, Vector4};

use crate::kernel::{Plane, Point3, Real, Vector3};

/// How the view is projected (Inventor: View → Appearance, ViewCube menu).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Projection {
    /// Parallel projection.
    Orthographic,
    /// Perspective.
    #[default]
    Perspective,
    /// Perspective, but orthographic while looking straight at a ViewCube
    /// face (Front, Top, …).
    PerspectiveOrthoFaces,
}

impl Projection {
    /// Inventor's name.
    pub fn label(self) -> &'static str {
        match self {
            Projection::Orthographic => "Orthographic",
            Projection::Perspective => "Perspective",
            Projection::PerspectiveOrthoFaces => "Perspective with Ortho Faces",
        }
    }
}

/// Inventor's Application Options › Display › Default Orbit Type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OrbitType {
    /// Relative to the screen (a trackball): a horizontal drag turns about
    /// the screen's vertical axis, a vertical one about its horizontal axis,
    /// whatever the view — a top view tilts instead of spinning about Z.
    #[default]
    Free,
    /// Relative to the model: a horizontal drag turns about the model's Z.
    Constrained,
}

impl OrbitType {
    /// Inventor's name.
    pub fn label(self) -> &'static str {
        match self {
            OrbitType::Free => "Free",
            OrbitType::Constrained => "Constrained",
        }
    }
}

/// A camera orbiting a target point.
#[derive(Clone, Debug)]
pub struct OrbitCamera {
    /// The point the camera looks at and orbits around.
    pub target: Point3,
    /// Distance from eye to target (mm).
    pub distance: Real,
    /// Rotation around world Z (radians).
    pub yaw: Real,
    /// Elevation above the XY plane (radians). Any value is allowed: past
    /// ±90° the camera has orbited over a pole and is upside down. Kept in
    /// (−π, π] by [`OrbitCamera::orbit`].
    pub pitch: Real,
    /// Vertical field of view (radians); in orthographic mode it sets the
    /// visible height at the target (`2·distance·tan(fov/2)`), so switching
    /// modes keeps the size of the model on screen.
    pub fov_y: Real,
    /// Projection mode.
    pub projection: Projection,
    /// Rotation of the image around the view axis (radians; ViewCube roll
    /// arrows). Picking a view resets it.
    pub roll: Real,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: Point3::origin(),
            distance: 150.0,
            yaw: (-60.0_f64).to_radians(),
            pitch: 30.0_f64.to_radians(),
            fov_y: 40.0_f64.to_radians(),
            projection: Projection::Perspective,
            roll: 0.0,
        }
    }
}

impl OrbitCamera {
    /// Eye position in world space.
    pub fn eye(&self) -> Point3 {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        self.target + self.distance * Vector3::new(cp * cy, cp * sy, sp)
    }

    /// The view's up vector before roll: d(eye)/d(pitch), unit length and
    /// perpendicular to the view direction. Well defined at the poles and
    /// continuous over them (it points down when the camera is upside
    /// down).
    pub fn up(&self) -> Vector3 {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        Vector3::new(-sp * cy, -sp * sy, cp)
    }

    /// Is the camera upside down (orbited over a pole)?
    pub fn upside_down(&self) -> bool {
        self.pitch.cos() < 0.0
    }

    /// World → camera transform.
    pub fn view(&self) -> Matrix4<Real> {
        let v = Matrix4::look_at_rh(&self.eye(), &self.target, &self.up());
        if self.roll == 0.0 {
            return v;
        }
        let r = nalgebra::Rotation3::from_axis_angle(&Vector3::z_axis(), self.roll);
        r.to_homogeneous() * v
    }

    /// Looks from direction `d` (target → eye); straight up/down keeps the
    /// front at the bottom of the screen, as Inventor's Top view.
    pub fn look_from(&mut self, d: Vector3) {
        let d = d.normalize();
        self.pitch = d.z.clamp(-1.0, 1.0).asin();
        self.yaw = if d.x.hypot(d.y) < 1e-9 {
            -std::f64::consts::FRAC_PI_2
        } else {
            d.y.atan2(d.x)
        };
        self.roll = 0.0;
    }

    /// The camera frame: (direction target → eye, screen-up vector), both
    /// unit, roll included.
    pub fn frame(&self) -> (Vector3, Vector3) {
        let v = self.view();
        let up = Vector3::new(v[(1, 0)], v[(1, 1)], v[(1, 2)]);
        let back = Vector3::new(v[(2, 0)], v[(2, 1)], v[(2, 2)]);
        (back, up)
    }

    /// This view in coordinates moved by the rigid motion `m`: a point `q`
    /// shows where `m · q` shows in the result (Edit in place keeps the
    /// assembly's picture in the part's coordinates).
    pub fn moved(&self, m: &crate::kernel::Transform3) -> Self {
        let (back, up) = self.frame();
        let mut out = self.clone();
        out.target = m * self.target;
        out.set_frame(m * back, m * up);
        out
    }

    /// Sets yaw, pitch and roll so the camera looks from direction `d`
    /// (target → eye) with `up` pointing up on screen. Of the two (yaw,
    /// pitch) pairs giving the same eye it takes the one whose own up is
    /// closest to `up`, so roll stays small; roll covers the rest.
    pub fn set_frame(&mut self, d: Vector3, up: Vector3) {
        let Some(d) = d.try_normalize(1e-12) else {
            return;
        };
        let Some(up) = (up - d * up.dot(&d)).try_normalize(1e-9) else {
            self.look_from(d);
            return;
        };
        if d.x.hypot(d.y) < 1e-9 {
            // At a pole the yaw alone turns the image: up() = ∓(cos y, sin y, 0).
            self.pitch = std::f64::consts::FRAC_PI_2.copysign(d.z);
            self.yaw = if d.z > 0.0 {
                (-up.y).atan2(-up.x)
            } else {
                up.y.atan2(up.x)
            };
        } else {
            self.pitch = d.z.clamp(-1.0, 1.0).asin();
            self.yaw = d.y.atan2(d.x);
            if self.up().dot(&up) < 0.0 {
                // The same eye, upside down: (π − p, y + π).
                self.pitch = std::f64::consts::PI - self.pitch;
                self.yaw += std::f64::consts::PI;
            }
        }
        self.pitch = wrap_angle(self.pitch);
        self.yaw = wrap_angle(self.yaw);
        // Roll: the angle that turns up() onto `up` around the view axis.
        self.roll = 0.0;
        let v = self.view();
        let cx = v[(0, 0)] * up.x + v[(0, 1)] * up.y + v[(0, 2)] * up.z;
        let cy = v[(1, 0)] * up.x + v[(1, 1)] * up.y + v[(1, 2)] * up.z;
        let roll = wrap_angle(std::f64::consts::FRAC_PI_2 - cy.atan2(cx));
        self.roll = if roll.abs() < 1e-9 { 0.0 } else { roll };
    }

    /// Camera → clip transform with wgpu's `[0, 1]` depth range.
    pub fn projection(&self, aspect: Real) -> Matrix4<Real> {
        let far = self.distance * 100.0;
        let gl = if self.is_ortho() {
            let h = self.distance * (self.fov_y / 2.0).tan();
            let w = h * aspect;
            // Geometry behind the eye stays visible in a parallel view.
            Matrix4::new_orthographic(-w, w, -h, h, -far, far)
        } else {
            Matrix4::new_perspective(aspect, self.fov_y, self.distance * 0.01, far)
        };
        // nalgebra produces OpenGL clip space (z ∈ [−1, 1]); remap to [0, 1].
        #[rustfmt::skip]
        let gl_to_wgpu = Matrix4::new(
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 0.5, 0.5,
            0.0, 0.0, 0.0, 1.0,
        );
        gl_to_wgpu * gl
    }

    /// Is the view parallel right now? (Perspective with Ortho Faces: when
    /// looking straight along a world axis.)
    pub fn is_ortho(&self) -> bool {
        match self.projection {
            Projection::Orthographic => true,
            Projection::Perspective => false,
            Projection::PerspectiveOrthoFaces => {
                // Along a world axis (also upside down / over a pole).
                let d = (self.eye() - self.target) / self.distance.max(1e-12);
                d.iter().any(|c| c.abs() > 1.0 - 1e-6)
            }
        }
    }

    /// World → clip transform for a viewport of the given aspect ratio.
    pub fn view_projection(&self, aspect: Real) -> Matrix4<Real> {
        self.projection(aspect) * self.view()
    }

    /// Rotates around the target by a mouse drag (in points). Vertical
    /// drags pass over the poles freely (Inventor's orbit); while upside
    /// down the horizontal drag is mirrored so the model still follows the
    /// mouse, and on a rolled image the drag is turned back first.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        let (dx, dy) = (dx as Real, dy as Real);
        // Screen drag (y down) → the unrolled camera frame.
        let (s, c) = self.roll.sin_cos();
        let (dx, dy) = (c * dx - s * dy, s * dx + c * dy);
        let flip = if self.upside_down() { -1.0 } else { 1.0 };
        self.yaw = wrap_angle(self.yaw - flip * dx * 0.008);
        self.pitch = wrap_angle(self.pitch + dy * 0.008);
    }

    /// Orbits by a mouse drag (in points) of the given type about `pivot`
    /// (the model point under the cursor when the drag began; `None`: the
    /// target, the centre of the screen). The camera turns rigidly about
    /// the pivot, so the pivot stays where it is on screen.
    pub fn orbit_about(&mut self, pivot: Option<Point3>, dx: f32, dy: f32, kind: OrbitType) {
        let (b0, u0) = self.frame();
        match kind {
            OrbitType::Constrained => self.orbit(dx, dy),
            OrbitType::Free => {
                // The same speed as the constrained orbit: at a level view
                // both give the same turn.
                let r0 = u0.cross(&b0);
                let turn = Rotation3::from_axis_angle(
                    &nalgebra::Unit::new_normalize(r0),
                    -(dy as Real) * 0.008,
                ) * Rotation3::from_axis_angle(
                    &nalgebra::Unit::new_normalize(u0),
                    -(dx as Real) * 0.008,
                );
                self.set_frame(turn * b0, turn * u0);
            }
        }
        let Some(pivot) = pivot else {
            return;
        };
        // The turn the frame made, applied to the target about the pivot.
        let (b1, u1) = self.frame();
        let f0 = nalgebra::Matrix3::from_columns(&[u0.cross(&b0), u0, b0]);
        let f1 = nalgebra::Matrix3::from_columns(&[u1.cross(&b1), u1, b1]);
        let r = f1 * f0.transpose();
        self.target = pivot + r * (self.target - pivot);
    }

    /// Turns the view about world Z by `angle` (radians; positive moves
    /// the eye counter-clockwise seen from above). ViewCube compass ring.
    pub fn turn_about_z(&mut self, angle: Real) {
        self.yaw = wrap_angle(self.yaw + angle);
    }

    /// Moves the target in the view plane so the scene follows the mouse.
    pub fn pan(&mut self, dx: f32, dy: f32, viewport_height: f32) {
        let view = self.view();
        let right = Vector3::new(view[(0, 0)], view[(0, 1)], view[(0, 2)]);
        let up = Vector3::new(view[(1, 0)], view[(1, 1)], view[(1, 2)]);
        let world_per_point =
            2.0 * self.distance * (self.fov_y / 2.0).tan() / viewport_height.max(1.0) as Real;
        self.target += (-right * dx as Real + up * dy as Real) * world_per_point;
    }

    /// Zooms in (positive `scroll`) or out, exponentially.
    pub fn zoom(&mut self, scroll: f32) {
        self.distance = (self.distance * (-scroll as Real * 0.002).exp()).clamp(0.01, 1e6);
    }

    /// Zooms like [`Self::zoom`] but keeps the point under `pos` (screen,
    /// in `rect`) where it is: Inventor's zoom to the cursor.
    pub fn zoom_at(&mut self, scroll: f32, pos: Pos2, rect: Rect) {
        let before = self.distance;
        self.zoom(scroll);
        let f = self.distance / before;
        let (back, up) = self.frame();
        let right = up.cross(&back);
        let per_point = 2.0 * before * (self.fov_y / 2.0).tan() / rect.height().max(1.0) as Real;
        let c = rect.center();
        let offset = (right * (pos.x - c.x) as Real - up * (pos.y - c.y) as Real) * per_point;
        self.target += offset * (1.0 - f);
    }

    /// Does the box `[min, max]` lie inside `rect` (shrunk by `margin`
    /// points)? A corner behind the eye counts as outside.
    pub fn box_in_view(&self, min: Point3, max: Point3, rect: Rect, margin: f32) -> bool {
        let inner = rect.shrink(margin);
        (0..8).all(|k| {
            let p = Point3::new(
                if k & 1 == 0 { min.x } else { max.x },
                if k & 2 == 0 { min.y } else { max.y },
                if k & 4 == 0 { min.z } else { max.z },
            );
            self.project(&p, rect).is_some_and(|s| inner.contains(s))
        })
    }

    /// Frames the axis-aligned box `[min, max]`.
    pub fn fit(&mut self, min: Point3, max: Point3) {
        self.target = nalgebra::center(&min, &max);
        let radius = (nalgebra::distance(&min, &max) / 2.0).max(1.0);
        self.distance = radius / (self.fov_y / 2.0).sin() * 1.1;
    }

    /// Projects a world point into the screen rectangle `rect`; `None` if it
    /// is behind the camera.
    pub fn project(&self, p: &Point3, rect: Rect) -> Option<Pos2> {
        let clip = self.view_projection(aspect(rect)) * p.to_homogeneous();
        if clip.w <= 1e-9 {
            return None;
        }
        let (x, y) = (clip.x / clip.w, clip.y / clip.w);
        Some(Pos2::new(
            rect.left() + ((x + 1.0) / 2.0) as f32 * rect.width(),
            rect.top() + ((1.0 - y) / 2.0) as f32 * rect.height(),
        ))
    }

    /// Intersects the eye ray through screen point `pos` with `plane`.
    pub fn ray_plane(&self, pos: Pos2, rect: Rect, plane: &Plane) -> Option<Point3> {
        let (origin, dir) = self.ray(pos, rect)?;
        let denom = dir.dot(&plane.normal);
        if denom.abs() < 1e-12 {
            return None;
        }
        let t = (plane.origin - origin).dot(&plane.normal) / denom;
        Some(origin + dir * t)
    }

    /// The eye ray through screen point `pos`: (point on the near plane,
    /// direction towards the far plane). Points along it are `origin + t·dir`
    /// with `t ≥ 0` in front of the camera.
    pub fn ray(&self, pos: Pos2, rect: Rect) -> Option<(Point3, Vector3)> {
        let inv = self.view_projection(aspect(rect)).try_inverse()?;
        let x = ((pos.x - rect.left()) / rect.width() * 2.0 - 1.0) as Real;
        let y = (1.0 - (pos.y - rect.top()) / rect.height() * 2.0) as Real;
        let unproject = |z: Real| {
            let h = inv * Vector4::new(x, y, z, 1.0);
            Point3::from(h.xyz() / h.w)
        };
        let (near, far) = (unproject(0.0), unproject(1.0));
        Some((near, far - near))
    }

    /// **Look At**: views a plane exactly head-on from its normal side, with
    /// the plane's y axis pointing up on screen, framing a disc of `radius`
    /// around `center`.
    pub fn look_at(&mut self, plane: &Plane, center: Point3, radius: Real) {
        let n = plane.normal.into_inner();
        let y = plane.y_axis().into_inner();
        self.set_frame(n, y);
        self.target = center;
        self.distance = radius.max(1.0) / (self.fov_y / 2.0).sin() * 1.15;
    }
}

/// `a` wrapped into (−π, π].
pub fn wrap_angle(a: Real) -> Real {
    use std::f64::consts::{PI, TAU};
    let w = a.rem_euclid(TAU);
    if w > PI { w - TAU } else { w }
}

/// Duration of a view transition (seconds).
pub const VIEW_ANIM_SECONDS: f64 = 0.3;

/// A smooth, time-based camera transition (ViewCube clicks, Home, standard
/// views; Inventor's "animated transitions"). The view direction moves
/// along the great circle between start and end (a slerp), the screen-up
/// vector is carried along with it and twisted about the view axis by the
/// remaining angle, the distance changes geometrically and the target
/// linearly. Projection follows the end camera at once.
#[derive(Clone, Debug)]
pub struct ViewAnim {
    from: OrbitCamera,
    to: OrbitCamera,
    /// Start direction (target → eye) and screen-up.
    b0: Vector3,
    u0: Vector3,
    /// Rotation axis and angle taking `b0` to the end direction.
    axis: nalgebra::Unit<Vector3>,
    angle: Real,
    /// The twist about the view axis still needed at the end.
    twist: Real,
    start: f64,
    duration: f64,
}

impl ViewAnim {
    /// A transition from `from` to `to` starting at time `start` (seconds).
    pub fn new(from: OrbitCamera, to: OrbitCamera, start: f64) -> Self {
        let (b0, u0) = from.frame();
        let (b1, u1) = to.frame();
        let cross = b0.cross(&b1);
        let angle = b0.dot(&b1).clamp(-1.0, 1.0).acos();
        // Opposite views (Front → Back): turn about the screen-up axis.
        let axis = nalgebra::Unit::try_new(cross, 1e-9).unwrap_or(nalgebra::Unit::new_normalize(
            if u0.norm() > 0.5 { u0 } else { Vector3::z() },
        ));
        let carried = Rotation3::from_axis_angle(&axis, angle) * u0;
        let twist = carried.cross(&u1).dot(&b1).atan2(carried.dot(&u1));
        ViewAnim {
            from,
            to,
            b0,
            u0,
            axis,
            angle,
            twist,
            start,
            duration: VIEW_ANIM_SECONDS,
        }
    }

    /// The camera at time `now` and whether the transition has finished
    /// (then it is exactly the end camera).
    pub fn at(&self, now: Real) -> (OrbitCamera, bool) {
        let t = ((now - self.start) / self.duration).clamp(0.0, 1.0);
        if t >= 1.0 - 1e-9 {
            return (self.to.clone(), true);
        }
        let s = t * t * (3.0 - 2.0 * t); // ease in-out
        let r = Rotation3::from_axis_angle(&self.axis, s * self.angle);
        let back = r * self.b0;
        let up = r * self.u0;
        let up = match nalgebra::Unit::try_new(back, 1e-12) {
            Some(b) => Rotation3::from_axis_angle(&b, s * self.twist) * up,
            None => up,
        };
        let mut cam = self.to.clone();
        cam.set_frame(back, up);
        let ratio = self.to.distance / self.from.distance.max(1e-12);
        cam.distance = self.from.distance * ratio.powf(s);
        cam.target = self.from.target + (self.to.target - self.from.target) * s;
        (cam, false)
    }
}

fn aspect(rect: Rect) -> Real {
    (rect.width() / rect.height().max(1.0)) as Real
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_moved_view_shows_moved_points_in_the_same_place() {
        use crate::kernel::Transform3;
        let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), eframe::egui::vec2(800.0, 600.0));
        let mut cam = OrbitCamera {
            target: Point3::new(40.0, -10.0, 5.0),
            distance: 300.0,
            yaw: 0.7,
            pitch: 0.4,
            roll: 0.2,
            ..Default::default()
        };
        for projection in [cam.projection, Projection::Orthographic] {
            cam.projection = projection;
            let m = Transform3::new(
                Vector3::new(120.0, 30.0, -8.0),
                Vector3::new(0.3, -0.5, 1.1),
            );
            let moved = cam.moved(&m.inverse());
            for q in [
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(50.0, 20.0, 10.0),
                Point3::new(-30.0, 60.0, 25.0),
            ] {
                let a = cam.project(&q, rect).unwrap();
                let b = moved.project(&(m.inverse() * q), rect).unwrap();
                assert!(a.distance(b) < 1e-3, "{a:?} vs {b:?}");
            }
        }
    }

    use super::*;
    use crate::kernel::BRep;

    #[test]
    fn orthographic_rays_are_parallel_and_still_pick() {
        let mut cam = OrbitCamera {
            projection: Projection::Orthographic,
            ..Default::default()
        };
        cam.fit(Point3::origin(), Point3::new(80.0, 60.0, 20.0));
        let rect = Rect::from_min_size(Pos2::new(250.0, 110.0), eframe::egui::vec2(850.0, 765.0));
        let p = Point3::new(40.0, 30.0, 20.0);
        let pos = cam.project(&p, rect).unwrap();
        let (o, d) = cam.ray(pos, rect).unwrap();
        // The ray passes through the point.
        let t = (p - o).dot(&d) / d.norm_squared();
        let miss = (o + d * t - p).norm();
        assert!(miss < 1e-3, "miss {miss}");
        let (_, d2) = cam
            .ray(pos + eframe::egui::vec2(100.0, 50.0), rect)
            .unwrap();
        assert!(
            d.normalize().cross(&d2.normalize()).norm() < 1e-9,
            "parallel"
        );
        // Ortho Faces: perspective in the iso view, parallel from the front.
        cam.projection = Projection::PerspectiveOrthoFaces;
        assert!(!cam.is_ortho());
        cam.yaw = -std::f64::consts::FRAC_PI_2;
        cam.pitch = 0.0;
        assert!(cam.is_ortho());
    }

    #[test]
    fn screen_ray_picks_the_face_under_the_cursor() {
        let mut body = BRep::new();
        body.make_box_named(Point3::origin(), Point3::new(80.0, 60.0, 20.0), "Box1")
            .unwrap();
        let mut cam = OrbitCamera::default();
        cam.fit(Point3::origin(), Point3::new(80.0, 60.0, 20.0));
        let rect = Rect::from_min_size(Pos2::new(250.0, 110.0), eframe::egui::vec2(850.0, 765.0));
        let top_center = Point3::new(40.0, 30.0, 20.0);
        let pos = cam.project(&top_center, rect).unwrap();
        let (o, d) = cam.ray(pos, rect).unwrap();
        let (face, _) = body
            .pick_face(&o, &d)
            .expect("ray through the top face centre must hit it");
        assert_eq!(body.face(face).unwrap().name.as_deref(), Some("Box1:Top"));
    }

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(250.0, 110.0), eframe::egui::vec2(850.0, 765.0))
    }

    /// The up vector is d(eye)/d(pitch): unit, perpendicular to the view
    /// direction, and equal to the screen-up row of the view matrix.
    #[test]
    fn up_is_the_pitch_derivative_and_the_screen_up() {
        for (yaw, pitch) in [(-60.0, 30.0), (10.0, 90.0), (10.0, -90.0), (200.0, 135.0)] {
            let cam = OrbitCamera {
                yaw: f64::to_radians(yaw),
                pitch: f64::to_radians(pitch),
                ..Default::default()
            };
            let up = cam.up();
            let h = 1e-6;
            let mut c2 = cam.clone();
            c2.pitch += h;
            let num = (c2.eye() - cam.eye()) / (h * cam.distance);
            assert!((num - up).norm() < 1e-5, "{yaw} {pitch}: {num} vs {up}");
            let (back, screen_up) = cam.frame();
            assert!(back.dot(&up).abs() < 1e-12);
            assert!((screen_up - up).norm() < 1e-9);
        }
    }

    /// Orbiting vertically goes over the top: the eye and the screen-up
    /// vector change continuously, and the camera ends upside down.
    #[test]
    fn orbit_passes_over_the_pole_continuously() {
        let mut cam = OrbitCamera {
            yaw: f64::to_radians(-90.0),
            pitch: f64::to_radians(80.0),
            ..Default::default()
        };
        let mut eye = cam.eye();
        let (_, mut up) = cam.frame();
        for _ in 0..60 {
            cam.orbit(0.0, 1.0); // 0.008 rad per step
            let (_, u) = cam.frame();
            assert!((cam.eye() - eye).norm() < cam.distance * 0.01, "eye jumped");
            assert!((u - up).norm() < 0.01, "up jumped: {up} -> {u}");
            eye = cam.eye();
            up = u;
        }
        assert!(cam.pitch > std::f64::consts::FRAC_PI_2, "went over the top");
        assert!(cam.upside_down());
        // Over the top from the front we look from the back, upside down.
        assert!(cam.eye().y > 0.0);
        assert!(up.z < 0.0);
        // Pitch wraps, so orbiting round and round stays bounded.
        for _ in 0..2000 {
            cam.orbit(0.0, 3.0);
        }
        assert!(cam.pitch.abs() <= std::f64::consts::PI + 1e-12);
    }

    /// Upside down, a drag to the right still turns the model to the right
    /// on screen (the yaw sign flips).
    #[test]
    fn horizontal_drag_follows_the_mouse_upside_down() {
        for pitch in [30.0, 150.0, -150.0] {
            let mut cam = OrbitCamera {
                yaw: f64::to_radians(-90.0),
                pitch: f64::to_radians(pitch),
                ..Default::default()
            };
            // The point of the model nearest the viewer moves with the mouse.
            let e = cam.eye();
            let near = Point3::from(Vector3::new(e.x, e.y, 0.0).normalize() * 30.0);
            let before = cam.project(&near, rect()).unwrap();
            cam.orbit(10.0, 0.0);
            let after = cam.project(&near, rect()).unwrap();
            assert!(after.x > before.x, "pitch {pitch}: {before:?} -> {after:?}");
        }
    }

    /// Top view: exactly from +Z, front (−Y) at the bottom of the screen;
    /// it counts as a face view for Ortho Faces.
    #[test]
    fn top_view_has_the_front_at_the_bottom() {
        let mut cam = OrbitCamera {
            projection: Projection::PerspectiveOrthoFaces,
            ..Default::default()
        };
        cam.look_from(Vector3::z());
        assert!((cam.pitch - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        let (back, _) = cam.frame();
        assert!((back - Vector3::z()).norm() < 1e-12, "straight down");
        let front = cam.project(&Point3::new(0.0, -50.0, 0.0), rect()).unwrap();
        let back_pt = cam.project(&Point3::new(0.0, 50.0, 0.0), rect()).unwrap();
        let right = cam.project(&Point3::new(50.0, 0.0, 0.0), rect()).unwrap();
        assert!(front.y > back_pt.y, "front below back");
        assert!(right.x > rect().center().x, "X to the right");
        assert!(cam.is_ortho());
        // The bottom view, too; and upside-down side views.
        cam.look_from(-Vector3::z());
        assert!(cam.is_ortho());
        cam.pitch = std::f64::consts::PI;
        assert!(cam.is_ortho());
    }

    /// Look At puts the plane's normal towards the eye and its y axis up on
    /// screen, for planes facing up, down and sideways.
    #[test]
    fn look_at_faces_the_plane_with_its_y_up() {
        let planes = [
            (Vector3::z(), Vector3::x()),
            (-Vector3::z(), Vector3::x()),
            (Vector3::z(), Vector3::y()),
            (Vector3::new(0.3, 0.1, 0.95), Vector3::x()),
            (Vector3::new(0.0, -1.0, 0.0), Vector3::x()),
            (Vector3::new(0.0, 0.0, -1.0), Vector3::new(1.0, 1.0, 0.0)),
        ];
        for (n, x) in planes {
            let n = n.normalize();
            let x = (x - n * x.dot(&n)).normalize();
            let plane = Plane::new(Point3::new(5.0, 6.0, 7.0), n, x).unwrap();
            let mut cam = OrbitCamera::default();
            cam.look_at(&plane, plane.origin, 40.0);
            let (back, up) = cam.frame();
            assert!((back - n).norm() < 1e-9, "head-on for {n}");
            let y = plane.y_axis().into_inner();
            assert!((up - y).norm() < 1e-9, "{n}: up {up} vs y {y}");
            // On screen: +y of the plane is above the centre.
            let c = cam.project(&plane.origin, rect()).unwrap();
            let above = cam.project(&(plane.origin + y * 10.0), rect()).unwrap();
            assert!(above.y < c.y - 1.0);
        }
    }

    /// Rays still hit the face under the cursor when the camera is upside
    /// down (looking at the box from below and behind).
    #[test]
    fn upside_down_rays_pick_the_face_under_the_cursor() {
        let mut body = BRep::new();
        body.make_box_named(Point3::origin(), Point3::new(80.0, 60.0, 20.0), "Box1")
            .unwrap();
        for (pitch, p, name) in [
            (150.0, Point3::new(40.0, 30.0, 20.0), "Box1:Top"),
            (-150.0, Point3::new(40.0, 30.0, 0.0), "Box1:Bottom"),
        ] {
            let mut cam = OrbitCamera {
                yaw: f64::to_radians(-60.0),
                pitch: f64::to_radians(pitch),
                ..Default::default()
            };
            cam.fit(Point3::origin(), Point3::new(80.0, 60.0, 20.0));
            assert!(cam.upside_down());
            let pos = cam.project(&p, rect()).unwrap();
            let (o, d) = cam.ray(pos, rect()).unwrap();
            let (face, _) = body.pick_face(&o, &d).expect("hit");
            assert_eq!(body.face(face).unwrap().name.as_deref(), Some(name));
        }
    }

    /// `set_frame` reproduces any camera frame (with roll when needed).
    #[test]
    fn set_frame_round_trips() {
        for (yaw, pitch, roll) in [(-60.0, 30.0, 0.0), (20.0, 120.0, 0.0), (5.0, -40.0, 30.0)] {
            let cam = OrbitCamera {
                yaw: f64::to_radians(yaw),
                pitch: f64::to_radians(pitch),
                roll: f64::to_radians(roll),
                ..Default::default()
            };
            let (b, u) = cam.frame();
            let mut c2 = OrbitCamera::default();
            c2.set_frame(b, u);
            let (b2, u2) = c2.frame();
            assert!((b - b2).norm() < 1e-9 && (u - u2).norm() < 1e-9);
            assert!((c2.eye() - cam.eye()).norm() < 1e-6);
        }
    }

    /// The animation starts at the start camera, ends exactly at the end
    /// camera and moves the view direction smoothly in between.
    #[test]
    fn view_animation_endpoints_and_midpoint() {
        let from = OrbitCamera::default();
        let mut to = from.clone();
        to.look_from(Vector3::z());
        to.distance = 600.0;
        to.target = Point3::new(10.0, 0.0, 0.0);
        let anim = ViewAnim::new(from.clone(), to.clone(), 5.0);
        let (c0, done0) = anim.at(5.0);
        assert!(!done0);
        assert!((c0.eye() - from.eye()).norm() < 1e-6);
        let (c1, done1) = anim.at(5.0 + VIEW_ANIM_SECONDS);
        assert!(done1);
        assert_eq!(
            (c1.yaw, c1.pitch, c1.distance),
            (to.yaw, to.pitch, to.distance)
        );
        let (cm, _) = anim.at(5.0 + VIEW_ANIM_SECONDS / 2.0);
        let (b0, _) = from.frame();
        let (b1, _) = to.frame();
        let (bm, _) = cm.frame();
        // Halfway along the great circle between both directions.
        let half = (b0 + b1).normalize();
        assert!((bm - half).norm() < 1e-6, "{bm} vs {half}");
        assert!(cm.distance > from.distance && cm.distance < to.distance);
        // Before the start it holds the start camera.
        assert!((anim.at(0.0).0.eye() - from.eye()).norm() < 1e-6);
    }

    /// Just before the end the frame is already (nearly) the end frame, so
    /// the last step does not jump — also for opposite views, upside-down
    /// ends and rolled starts.
    #[test]
    fn view_animation_has_no_jump_at_the_end() {
        let mut cases = Vec::new();
        let mut a = OrbitCamera::default();
        a.look_from(-Vector3::y());
        let mut b = a.clone();
        b.look_from(Vector3::y());
        cases.push((a.clone(), b)); // Front → Back
        let mut c = a.clone();
        c.pitch = 2.5; // upside down
        cases.push((OrbitCamera::default(), c));
        let d = OrbitCamera {
            roll: 1.2,
            ..Default::default()
        };
        let mut e = OrbitCamera::default();
        e.look_from(Vector3::z());
        cases.push((d, e));
        for (from, to) in cases {
            let anim = ViewAnim::new(from.clone(), to.clone(), 0.0);
            let (near_end, done) = anim.at(VIEW_ANIM_SECONDS * 0.999);
            assert!(!done);
            let (b1, u1) = to.frame();
            let (bn, un) = near_end.frame();
            assert!((bn - b1).norm() < 1e-3 && (un - u1).norm() < 1e-3);
            let mut prev = from.frame();
            for k in 1..=30 {
                let (c, _) = anim.at(VIEW_ANIM_SECONDS * k as f64 / 30.0);
                let f = c.frame();
                assert!((f.0 - prev.0).norm() < 0.3 && (f.1 - prev.1).norm() < 0.3);
                prev = f;
            }
        }
    }

    #[test]
    fn free_orbit_matches_the_constrained_one_at_a_level_view() {
        for (dx, dy) in [(40.0, 0.0), (-25.0, 0.0), (0.0, 30.0), (0.0, -50.0)] {
            let mut a = OrbitCamera {
                pitch: 0.0,
                ..OrbitCamera::default()
            };
            let mut b = a.clone();
            a.orbit_about(None, dx, dy, OrbitType::Free);
            b.orbit_about(None, dx, dy, OrbitType::Constrained);
            let (ea, eb) = (a.eye(), b.eye());
            assert!((ea - eb).norm() < 1e-9, "{dx},{dy}: {ea} vs {eb}");
            let ((_, ua), (_, ub)) = (a.frame(), b.frame());
            assert!((ua - ub).norm() < 1e-9);
        }
    }

    #[test]
    fn free_orbit_tilts_a_top_view_instead_of_spinning_it() {
        let mut top = OrbitCamera::default();
        top.look_from(Vector3::z());
        let mut free = top.clone();
        free.orbit_about(None, 60.0, 0.0, OrbitType::Free);
        let (back, _) = free.frame();
        // The eye left the Z axis (turned about the screen's vertical) ...
        assert!(back.z < 0.95, "{back}");
        // ... towards screen right, which stays the same.
        let ((b0, u0), (_, u1)) = (top.frame(), free.frame());
        assert!((u0 - u1).norm() < 1e-9);
        assert!(back.dot(&u0.cross(&b0)) < 0.0, "model follows the mouse");
        let mut constrained = top.clone();
        constrained.orbit_about(None, 60.0, 0.0, OrbitType::Constrained);
        assert!(constrained.frame().0.z > 1.0 - 1e-9, "spins about Z");
    }

    #[test]
    fn orbit_about_a_pivot_keeps_it_on_screen() {
        let r = rect();
        for kind in [OrbitType::Free, OrbitType::Constrained] {
            let mut cam = OrbitCamera::default();
            let pivot = Point3::new(30.0, -10.0, 5.0);
            let before = cam.project(&pivot, r).expect("in front");
            for _ in 0..10 {
                cam.orbit_about(Some(pivot), 13.0, -7.0, kind);
            }
            let after = cam.project(&pivot, r).expect("in front");
            assert!(
                (after - before).length() < 1e-3,
                "{kind:?}: {before:?} {after:?}"
            );
            assert!(
                ((cam.eye() - pivot).norm() - (OrbitCamera::default().eye() - pivot).norm()).abs()
                    < 1e-6
            );
        }
    }

    #[test]
    fn zoom_at_keeps_the_point_under_the_cursor() {
        let r = rect();
        let pos = Pos2::new(r.left() + 120.0, r.top() + 90.0);
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut cam = OrbitCamera {
                projection,
                ..OrbitCamera::default()
            };
            // The point under the cursor in the plane through the target.
            let (back, _) = cam.frame();
            let (_, up) = cam.frame();
            let plane = Plane::new(cam.target, back, up.cross(&back)).expect("plane");
            let p = cam.ray_plane(pos, r, &plane).expect("hit");
            cam.zoom_at(300.0, pos, r);
            let s = cam.project(&p, r).expect("in front");
            assert!(
                (s - pos).length() < 1e-3,
                "{projection:?}: {s:?} vs {pos:?}"
            );
            assert!(cam.distance < OrbitCamera::default().distance);
        }
    }

    #[test]
    fn box_in_view_sees_a_box_leaving_the_screen() {
        let r = rect();
        let mut cam = OrbitCamera::default();
        cam.fit(
            Point3::new(-10.0, -10.0, -10.0),
            Point3::new(10.0, 10.0, 10.0),
        );
        let (lo, hi) = (
            Point3::new(-10.0, -10.0, -10.0),
            Point3::new(10.0, 10.0, 10.0),
        );
        assert!(cam.box_in_view(lo, hi, r, 4.0));
        assert!(!cam.box_in_view(lo, Point3::new(1000.0, 10.0, 10.0), r, 4.0));
    }
}
