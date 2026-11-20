//! The 3D sketch's drawing plane (`docs/spec/1.10-3d-sketch.md`, Drawing
//! plane): where a click in empty space puts the next point. GUI-free.
//!
//! The plane is one of the part's XY / XZ / YZ planes through the last point
//! (or the origin when there is none), or a plane the user aligned to (a
//! planar face, a work or an origin plane). The cursor's ray meets it; the
//! point is then pulled onto a line along one of the plane's two axes through
//! the last point when the cursor is near it (inference, as in the 2D sketch).
//! None of this is saved: the plane is the open sketch's GUI state.

use crate::kernel::{Plane, Point3, Real, Vector3};

/// Below this cosine between the ray and the plane the plane is seen edge-on
/// and gives no point (it would jump far away for a pixel of motion).
const EDGE_ON: Real = 0.02;

/// The plane the 3D sketch's tools draw on.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum DrawPlane {
    /// The part's XY plane (normal Z) through the last point.
    #[default]
    Xy,
    /// The part's XZ plane (normal Y) through the last point.
    Xz,
    /// The part's YZ plane (normal X) through the last point.
    Yz,
    /// A plane the user picked (Align to Plane): it stays where it is.
    Aligned {
        /// The plane.
        plane: Plane,
        /// What was picked, for the status bar.
        name: String,
    },
}

impl DrawPlane {
    /// The next plane of the cycle (Shift+Space): XY, XZ, YZ, XY…; an
    /// aligned plane goes back to XY.
    pub fn next(&self) -> Self {
        match self {
            DrawPlane::Xy => DrawPlane::Xz,
            DrawPlane::Xz => DrawPlane::Yz,
            DrawPlane::Yz | DrawPlane::Aligned { .. } => DrawPlane::Xy,
        }
    }

    /// The name shown in the view and the status bar.
    pub fn label(&self) -> String {
        match self {
            DrawPlane::Xy => "XY".into(),
            DrawPlane::Xz => "XZ".into(),
            DrawPlane::Yz => "YZ".into(),
            DrawPlane::Aligned { name, .. } => name.clone(),
        }
    }

    /// The plane's unit normal.
    pub fn normal(&self) -> Vector3 {
        match self {
            DrawPlane::Xy => Vector3::z(),
            DrawPlane::Xz => Vector3::y(),
            DrawPlane::Yz => Vector3::x(),
            DrawPlane::Aligned { plane, .. } => plane.normal.into_inner(),
        }
    }

    /// The plane's two in-plane unit axes (the inference directions).
    pub fn axes(&self) -> [Vector3; 2] {
        match self {
            DrawPlane::Xy => [Vector3::x(), Vector3::y()],
            DrawPlane::Xz => [Vector3::x(), Vector3::z()],
            DrawPlane::Yz => [Vector3::y(), Vector3::z()],
            DrawPlane::Aligned { plane, .. } => {
                let x = plane.x_axis.into_inner();
                [x, plane.normal.cross(&x)]
            }
        }
    }

    /// The names of the two in-plane axes (Dynamic Input's deltas).
    pub fn axis_names(&self) -> [&'static str; 2] {
        match self {
            DrawPlane::Xy => ["dX", "dY"],
            DrawPlane::Xz => ["dX", "dZ"],
            DrawPlane::Yz => ["dY", "dZ"],
            DrawPlane::Aligned { .. } => ["dU", "dV"],
        }
    }

    /// A point of the plane: `through` (the last point) for the part's
    /// planes, the picked plane's own origin for an aligned one.
    pub fn origin(&self, through: Option<Point3>) -> Point3 {
        match self {
            DrawPlane::Aligned { plane, .. } => plane.origin,
            _ => through.unwrap_or_else(Point3::origin),
        }
    }

    /// Where the ray `o + t·d` (t > 0 not required: the plane may lie behind
    /// a parallel projection's eye) meets the plane through `through`; `None`
    /// when the plane is seen edge-on.
    pub fn ray_point(&self, through: Option<Point3>, o: Point3, d: Vector3) -> Option<Point3> {
        let d = d.try_normalize(1e-12)?;
        let n = self.normal();
        let c = d.dot(&n);
        if c.abs() < EDGE_ON {
            return None;
        }
        let t = (self.origin(through) - o).dot(&n) / c;
        t.is_finite().then(|| o + d * t)
    }

    /// The plane among XY / XZ / YZ that faces the view direction `d` most.
    pub fn facing(d: Vector3) -> Self {
        let a = d.map(Real::abs);
        if a.z >= a.x && a.z >= a.y {
            DrawPlane::Xy
        } else if a.y >= a.x {
            DrawPlane::Xz
        } else {
            DrawPlane::Yz
        }
    }

    /// Inference: `q` (on the plane) pulled onto the line through `last`
    /// along one of the plane's axes when `screen(a, b)` — the distance in
    /// pixels between two points as seen — is at most `tol` there. The
    /// nearer axis wins; with `force` (Ortho) the nearer axis always wins.
    /// Returns the point and the axis (0 or 1) it snapped to.
    pub fn infer(
        &self,
        last: Point3,
        q: Point3,
        tol: f32,
        force: bool,
        screen: impl Fn(Point3, Point3) -> Option<f32>,
    ) -> (Point3, Option<usize>) {
        let mut best: Option<(f32, usize, Point3)> = None;
        for (k, a) in self.axes().iter().enumerate() {
            let c = last + a * (q - last).dot(a);
            // Pixels between the cursor's point and its foot on the axis line;
            // in Ortho mode a missing projection still counts (3D distance).
            let px = screen(c, q).unwrap_or(if force {
                (c - q).norm() as f32
            } else {
                f32::INFINITY
            });
            if (force || px <= tol) && best.is_none_or(|b| px < b.0) {
                best = Some((px, k, c));
            }
        }
        match best {
            Some((_, k, c)) => (c, Some(k)),
            None => (q, None),
        }
    }

    /// `q - last` in the plane's axes (Dynamic Input's two deltas).
    pub fn deltas(&self, last: Point3, q: Point3) -> [Real; 2] {
        let [a, b] = self.axes();
        let v = q - last;
        [v.dot(&a), v.dot(&b)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_space_cycles_the_three_planes() {
        let p = DrawPlane::default();
        assert_eq!(p, DrawPlane::Xy);
        assert_eq!(p.next(), DrawPlane::Xz);
        assert_eq!(p.next().next(), DrawPlane::Yz);
        assert_eq!(p.next().next().next(), DrawPlane::Xy);
        let a = DrawPlane::Aligned {
            plane: Plane::xy(),
            name: "Box1:Top".into(),
        };
        assert_eq!(a.next(), DrawPlane::Xy);
        assert_eq!(a.label(), "Box1:Top");
    }

    #[test]
    fn the_ray_meets_the_plane_through_the_last_point() {
        let last = Point3::new(10.0, 20.0, 30.0);
        let o = Point3::new(5.0, 5.0, 100.0);
        let d = Vector3::new(0.1, 0.2, -1.0);
        let q = DrawPlane::Xy.ray_point(Some(last), o, d).unwrap();
        assert!((q.z - 30.0).abs() < 1e-9);
        // On the ray.
        let v = (q - o).normalize();
        assert!((v - d.normalize()).norm() < 1e-9);
        let q = DrawPlane::Xz.ray_point(Some(last), o, Vector3::new(0.0, 1.0, -0.5));
        assert!((q.unwrap().y - 20.0).abs() < 1e-9);
        let q = DrawPlane::Yz.ray_point(None, o, Vector3::new(-1.0, 0.0, 0.0));
        assert!(q.unwrap().x.abs() < 1e-9);
    }

    #[test]
    fn a_plane_seen_edge_on_gives_no_point() {
        let o = Point3::new(0.0, -100.0, 5.0);
        assert!(
            DrawPlane::Xy
                .ray_point(None, o, Vector3::new(0.0, 1.0, 0.001))
                .is_none()
        );
        assert_eq!(
            DrawPlane::facing(Vector3::new(0.0, 1.0, 0.001)),
            DrawPlane::Xz
        );
        assert_eq!(
            DrawPlane::facing(Vector3::new(-0.9, 0.3, 0.2)),
            DrawPlane::Yz
        );
    }

    #[test]
    fn an_aligned_plane_keeps_its_place_and_axes() {
        let plane = Plane::new(
            Point3::new(0.0, 0.0, 7.0),
            Vector3::new(0.0, -1.0, 1.0),
            Vector3::x(),
        )
        .unwrap();
        let p = DrawPlane::Aligned {
            plane,
            name: "Work Plane1".into(),
        };
        let q = p
            .ray_point(
                Some(Point3::new(50.0, 50.0, 50.0)),
                Point3::new(3.0, 0.0, 100.0),
                -Vector3::z(),
            )
            .unwrap();
        // z - y = 7 on that plane, whatever the last point.
        assert!((q.z - q.y - 7.0).abs() < 1e-9);
        let [a, b] = p.axes();
        assert!(a.dot(&b).abs() < 1e-12 && a.dot(&p.normal()).abs() < 1e-12);
        assert!(b.dot(&p.normal()).abs() < 1e-12);
    }

    #[test]
    fn inference_pulls_onto_the_nearer_axis_line() {
        let last = Point3::new(0.0, 0.0, 0.0);
        // Pixels = mm here.
        let screen = |a: Point3, b: Point3| Some((a - b).norm() as f32);
        let (q, k) = DrawPlane::Xy.infer(last, Point3::new(40.0, 3.0, 0.0), 8.0, false, screen);
        assert_eq!(k, Some(0));
        assert!((q - Point3::new(40.0, 0.0, 0.0)).norm() < 1e-12);
        let (q, k) = DrawPlane::Xz.infer(last, Point3::new(2.0, 0.0, -30.0), 8.0, false, screen);
        assert_eq!(k, Some(1));
        assert!((q - Point3::new(0.0, 0.0, -30.0)).norm() < 1e-12);
        // Far from both: unchanged.
        let free = Point3::new(20.0, 25.0, 0.0);
        assert_eq!(
            DrawPlane::Xy.infer(last, free, 8.0, false, screen),
            (free, None)
        );
        // Ortho: always on the nearer axis.
        let (q, k) = DrawPlane::Xy.infer(last, free, 8.0, true, screen);
        assert_eq!(k, Some(1));
        assert!((q - Point3::new(0.0, 25.0, 0.0)).norm() < 1e-12);
        assert_eq!(DrawPlane::Xy.deltas(last, q), [0.0, 25.0]);
    }
}
