//! Orbit camera with CAD conventions (Z up, orbit around a target point).

use eframe::egui::{Pos2, Rect};
use nalgebra::{Matrix4, Vector4};

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

/// A camera orbiting a target point.
#[derive(Clone, Debug)]
pub struct OrbitCamera {
    /// The point the camera looks at and orbits around.
    pub target: Point3,
    /// Distance from eye to target (mm).
    pub distance: Real,
    /// Rotation around world Z (radians).
    pub yaw: Real,
    /// Elevation above the XY plane (radians, clamped to ±89°).
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

    /// World → camera transform.
    pub fn view(&self) -> Matrix4<Real> {
        let v = Matrix4::look_at_rh(&self.eye(), &self.target, &Vector3::z());
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
        let lim = 89f64.to_radians();
        self.pitch = d.z.asin().clamp(-lim, lim);
        self.yaw = if d.x.hypot(d.y) < 1e-9 {
            -std::f64::consts::FRAC_PI_2
        } else {
            d.y.atan2(d.x)
        };
        self.roll = 0.0;
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
                let eps = 1e-3;
                let quarter = std::f64::consts::FRAC_PI_2;
                let aligned = |a: Real| ((a / quarter).round() * quarter - a).abs() < eps;
                let top = (self.pitch.abs() - 89f64.to_radians()).abs() < eps;
                top || (self.pitch.abs() < eps && aligned(self.yaw))
            }
        }
    }

    /// World → clip transform for a viewport of the given aspect ratio.
    pub fn view_projection(&self, aspect: Real) -> Matrix4<Real> {
        self.projection(aspect) * self.view()
    }

    /// Rotates around the target by a mouse drag (in points).
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx as Real * 0.008;
        self.pitch =
            (self.pitch + dy as Real * 0.008).clamp(-89f64.to_radians(), 89f64.to_radians());
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

    /// **Look At**: views a plane head-on from its normal side, with the
    /// plane's y axis pointing up on screen as far as the Z-up orbit camera
    /// allows, framing a disc of `radius` around `center`.
    pub fn look_at(&mut self, plane: &Plane, center: Point3, radius: Real) {
        let n = plane.normal.into_inner();
        let y = plane.y_axis().into_inner();
        let max_pitch = 89f64.to_radians();
        if n.z.abs() > 0.9 {
            // Looking (almost) straight up or down: screen-up follows the
            // horizontal offset of the eye, so choose yaw to make it = y.
            self.pitch = max_pitch.copysign(n.z);
            self.yaw = if n.z > 0.0 {
                (-y.y).atan2(-y.x)
            } else {
                y.y.atan2(y.x)
            };
        } else {
            self.yaw = n.y.atan2(n.x);
            self.pitch = n.z.asin().clamp(-max_pitch, max_pitch);
        }
        self.target = center;
        self.distance = radius.max(1.0) / (self.fov_y / 2.0).sin() * 1.15;
    }
}

fn aspect(rect: Rect) -> Real {
    (rect.width() / rect.height().max(1.0)) as Real
}

#[cfg(test)]
mod tests {
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
}
