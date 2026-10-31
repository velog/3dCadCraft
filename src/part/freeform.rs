//! Freeform (`docs/spec/5.5-freeform.md`): a control cage whose points are pulled one by one and
//! the cubic B-spline sheet it controls. The cage is kept as its rest grid (sizes and face counts
//! on a plane) plus the moved points, so the shape regenerates from the feature alone.

use serde::{Deserialize, Serialize};

use super::placement::PlaneRef;
use crate::kernel::{Plane, Point3, Real, Vector3};

/// The shape of a freeform primitive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreeformKind {
    /// A rectangular sheet on its plane.
    #[default]
    Plane,
    /// A tube round the plane's normal through the centre (`length` = diameter, `width` =
    /// height, `length_faces` around, `width_faces` up); its cage closes around.
    Cylinder,
}

/// A moved cage point: its index along the length (`i`) and the width (`j`) and its offset from
/// rest, in the plane's frame (x along the length, y along the width, z along the normal).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CageMove {
    /// Index along the length (0 ..= length faces).
    pub i: usize,
    /// Index along the width (0 ..= width faces).
    pub j: usize,
    /// Offset from the rest position (plane frame, mm).
    pub offset: [Real; 3],
}

/// **Freeform** (format revision 59): a primitive's control cage and its moved points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreeformOp {
    /// The primitive (`kind` is the body operation's tag).
    #[serde(default)]
    pub shape: FreeformKind,
    /// The base plane.
    pub plane: PlaneRef,
    /// The centre on the plane (plane coordinates; the plane's origin by default).
    #[serde(default)]
    pub center: [Real; 2],
    /// Size along the plane's x (mm).
    pub length: Real,
    /// Size along the plane's y (mm).
    pub width: Real,
    /// Cage faces along the length.
    pub length_faces: usize,
    /// Cage faces along the width.
    pub width_faces: usize,
    /// Symmetry across the centre lines (length, width): Edit Form moves the mirror images too.
    #[serde(default)]
    pub symmetry: [bool; 2],
    /// The moved points.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moved: Vec<CageMove>,
}

/// The most cage faces a side.
pub const MAX_FACES: usize = 64;

impl FreeformOp {
    /// A cylinder (tube) of `diameter` and `height` with 8 × 4 faces on `plane`.
    pub fn cylinder(plane: PlaneRef, diameter: Real, height: Real) -> Self {
        Self {
            shape: FreeformKind::Cylinder,
            length_faces: 8,
            ..Self::plane(plane, diameter, height)
        }
    }

    /// Whether the cage closes around (its first and last rows join).
    pub fn closed(&self) -> bool {
        self.shape == FreeformKind::Cylinder
    }

    /// Cage rows along the length / around (a closed cage does not repeat its first row).
    pub fn rows(&self) -> usize {
        if self.closed() {
            self.length_faces
        } else {
            self.length_faces + 1
        }
    }

    /// A plane of `length` × `width` with 4 × 4 faces on `plane`.
    pub fn plane(plane: PlaneRef, length: Real, width: Real) -> Self {
        Self {
            shape: FreeformKind::Plane,
            plane,
            center: [0.0, 0.0],
            length,
            width,
            length_faces: 4,
            width_faces: 4,
            symmetry: [false, false],
            moved: Vec::new(),
        }
    }

    /// Refuses a definition that cannot be built.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.length > 0.0 && self.width > 0.0) {
            return Err("the sizes must be above zero".into());
        }
        for f in [self.length_faces, self.width_faces] {
            if !(1..=MAX_FACES).contains(&f) {
                return Err(format!("faces each way: 1 to {MAX_FACES}"));
            }
        }
        if self.closed() && self.length_faces < 3 {
            return Err("a cylinder needs 3 faces around or more".into());
        }
        if self
            .moved
            .iter()
            .any(|m| m.i >= self.rows() || m.j > self.width_faces)
        {
            return Err("a moved point lies outside the cage".into());
        }
        Ok(())
    }

    /// A cage point's offset from rest (zero when it was not moved).
    pub fn offset(&self, i: usize, j: usize) -> [Real; 3] {
        self.moved
            .iter()
            .rev()
            .find(|m| m.i == i && m.j == j)
            .map_or([0.0; 3], |m| m.offset)
    }

    /// Sets a cage point's offset (dropping it when it is back at rest).
    pub fn set_offset(&mut self, i: usize, j: usize, offset: [Real; 3]) {
        self.moved.retain(|m| !(m.i == i && m.j == j));
        if offset.iter().any(|v| v.abs() > 1e-12) {
            self.moved.push(CageMove { i, j, offset });
            self.moved.sort_by_key(|m| (m.i, m.j));
        }
    }

    /// The point mirrored across the centre lines that symmetry asks for, with its offset's
    /// mirror (`(i, j)` itself included).
    pub fn mirrors(&self, i: usize, j: usize) -> Vec<(usize, usize, [bool; 2])> {
        let mut out = vec![(i, j, [false, false])];
        if self.closed() {
            return out;
        }
        if self.symmetry[0] {
            out.push((self.length_faces - i, j, [true, false]));
        }
        if self.symmetry[1] {
            out.push((i, self.width_faces - j, [false, true]));
        }
        if self.symmetry[0] && self.symmetry[1] {
            out.push((self.length_faces - i, self.width_faces - j, [true, true]));
        }
        out.sort_by_key(|(a, b, _)| (*a, *b));
        out.dedup_by_key(|(a, b, _)| (*a, *b));
        out
    }

    /// A cage point at rest in plane coordinates (x, y, along the normal).
    pub fn rest(&self, i: usize, j: usize) -> [Real; 3] {
        match self.shape {
            FreeformKind::Plane => {
                let x = self.center[0] - self.length / 2.0
                    + self.length * i as Real / self.length_faces as Real;
                let y = self.center[1] - self.width / 2.0
                    + self.width * j as Real / self.width_faces as Real;
                [x, y, 0.0]
            }
            FreeformKind::Cylinder => {
                let a = std::f64::consts::TAU * i as Real / self.length_faces as Real;
                let r = self.length / 2.0;
                [
                    self.center[0] + r * a.cos(),
                    self.center[1] + r * a.sin(),
                    self.width * j as Real / self.width_faces as Real,
                ]
            }
        }
    }

    /// The cage on `plane`: rows along the length (`net[i][j]`), each point at rest plus its offset.
    pub fn cage(&self, plane: &Plane) -> Vec<Vec<Point3>> {
        let n: Vector3 = plane.normal.into_inner();
        let (xa, ya) = (plane.x_axis.into_inner(), plane.y_axis().into_inner());
        (0..self.rows())
            .map(|i| {
                (0..=self.width_faces)
                    .map(|j| {
                        let [x, y, z] = self.rest(i, j);
                        let [dx, dy, dz] = self.offset(i, j);
                        plane.origin + xa * (x + dx) + ya * (y + dy) + n * (z + dz)
                    })
                    .collect()
            })
            .collect()
    }

    /// Browser tooltip.
    pub fn describe(&self) -> String {
        if self.closed() {
            return format!(
                "Freeform Cylinder Ø{} × {} mm, {} × {} faces, {} point(s) moved",
                self.length,
                self.width,
                self.length_faces,
                self.width_faces,
                self.moved.len()
            );
        }
        format!(
            "Freeform Plane {} × {} mm, {} × {} faces, {} point(s) moved",
            self.length,
            self.width,
            self.length_faces,
            self.width_faces,
            self.moved.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::OriginPlane;

    #[test]
    fn cage_rest_offsets_and_mirrors() {
        let mut op = FreeformOp::plane(PlaneRef::Origin(OriginPlane::XY), 40.0, 20.0);
        let pl = Plane::xy();
        let net = op.cage(&pl);
        assert_eq!((net.len(), net[0].len()), (5, 5));
        assert!((net[0][0] - Point3::new(-20.0, -10.0, 0.0)).norm() < 1e-12);
        assert!((net[4][4] - Point3::new(20.0, 10.0, 0.0)).norm() < 1e-12);
        op.set_offset(2, 2, [0.0, 0.0, 5.0]);
        assert!((op.cage(&pl)[2][2].z - 5.0).abs() < 1e-12);
        op.set_offset(2, 2, [0.0; 3]);
        assert!(op.moved.is_empty(), "back at rest: forgotten");
        op.symmetry = [true, true];
        assert_eq!(op.mirrors(1, 1).len(), 4);
        assert_eq!(op.mirrors(2, 2).len(), 1, "the centre is its own mirror");
        op.length_faces = 0;
        assert!(op.validate().is_err());
    }
}
