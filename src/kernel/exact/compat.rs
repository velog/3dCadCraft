//! What the kernel's move to solidcore changed, in one place: where the plain types live now,
//! methods the old `Body` had, and our own operations that were written on truck and wait for a
//! solidcore version. Those report a refusal, which reaches the user as the feature's health
//! (rule 5); `docs/KERNEL_API.md` lists them.

/// `solidcraft_geom` plus the plain data that moved to `solidcore_record`.
pub mod sg {
    pub use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
    pub use solidcraft_geom::*;
}

/// `solidcraft_kernel` plus what is not (or not yet) in it.
pub mod sk {
    use super::sg::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
    pub use solidcraft_kernel::*;

    fn not_ported<T>(what: &str) -> Result<T> {
        Err(KernelError::Invalid(format!("{what}: not available on the solidcore kernel yet")))
    }

    /// All faces' surfaces.
    pub fn face_surfaces(b: &Body) -> Vec<Option<FaceSurface>> {
        (0..b.face_count()).map(|f| face_surface(b, f)).collect()
    }

    /// `extrude_tapered` with the kernel's default corners.
    pub fn extrude_tapered(p: &Plane, r: &Region2, len: f64, sign: f64, taper: f64) -> Result<Body> {
        solidcraft_kernel::extrude_tapered(p, r, len, sign, taper, DraftCorner::default())
    }

    /// `fillet_each` with mitred rounds.
    pub fn fillet_each(b: &Body, edges: &[Vec3], radii: &[f64]) -> Result<Body> {
        solidcraft_kernel::fillet_each(b, edges, radii, FilletTurn::default())
    }

    /// A B-rep of planar triangles sewn into a closed solid.
    pub fn polyhedron_body(positions: &[Vec3], triangles: &[[u32; 3]]) -> Result<Body> {
        let at = |i: u32| {
            positions
                .get(i as usize)
                .copied()
                .ok_or_else(|| KernelError::Invalid("polyhedron triangle index out of range".into()))
        };
        let mut faces = Vec::with_capacity(triangles.len());
        for t in triangles {
            let (p, q, r) = (at(t[0])?, at(t[1])?, at(t[2])?);
            let side = |a, b| EdgeSpec::Line { a, b };
            faces.push(FaceSpec {
                loops: vec![vec![side(p, q), side(q, r), side(r, p)]],
                surface: SurfSpec::Plane,
                probe: None,
            });
        }
        sew(&faces)
    }

    /// The patch of a closed loop, tangent to the faces round it with `tangent`'s weight (a fuller
    /// middle above 1, a flatter one below).
    pub fn patch_edges_tangent(b: &Body, at: &[Vec3], tangent: Option<f64>) -> Result<Body> {
        match tangent {
            None => patch_edges(b, at),
            Some(w) => patch_edges_with(
                b,
                at,
                &PatchSpec {
                    continuity: Continuity::G1,
                    crown: (w - 1.0).clamp(-0.9, 3.0),
                    ..PatchSpec::default()
                },
            ),
        }
    }

    /// A loft through matched sections (the kernel pairs them itself now).
    pub fn loft_matched(sections: &[(Plane, Loop2)]) -> Result<Body> {
        loft(sections)
    }

    /// A closed loop of sections: two lofts joined at their end sections.
    pub fn loft_matched_ring(sections: &[(Plane, Loop2)]) -> Result<Body> {
        let n = sections.len();
        if n < 4 {
            return Err(KernelError::Invalid("a closed loft needs four sections".into()));
        }
        let half = n / 2;
        let a = loft(&sections[..=half])?;
        let mut rest: Vec<(Plane, Loop2)> = sections[half..].to_vec();
        rest.extend(sections.first().cloned());
        let b = loft(&rest)?;
        boolean(&a, &b, BoolOp::Union)?.ok_or_else(|| KernelError::Failed("closed loft: nothing joined".into()))
    }

    /// Equal chamfers of whole loops: the kernel's chamfer takes them.
    pub fn chamfer_loops(b: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
        chamfer(b, edges, distance)
    }

    /// The shell of `b` holding face `face` taken out: a lump goes, a void fills with material.
    pub fn delete_shell(b: &Body, face: usize) -> Result<Body> {
        let lumps = b.lumps()?;
        if lumps.len() > 1 {
            let Some(Some(p)) = super::BodyCompat::face_corners(b).get(face).copied() else {
                return Err(KernelError::Invalid("Delete Face: no such face".into()));
            };
            let on = |l: &Body| matches!(l.classify_point(p), Ok(PointClass::OnBoundary));
            let mut rest: Vec<Body> = lumps.iter().filter(|l| !on(l)).cloned().collect();
            if rest.len() + 1 != lumps.len() {
                return Err(KernelError::Failed("Delete Face: the lump of the face was not found".into()));
            }
            if rest.len() == 1 {
                return rest.pop().ok_or_else(|| KernelError::Failed("Delete Face: nothing is left".into()));
            }
            return join_apart(&rest)?.ok_or_else(|| KernelError::Failed("Delete Face: nothing is left".into()));
        }
        // A void: its faces copied, stitched into the solid that fills it, joined to the body.
        let record = b.record();
        let index = face as u32;
        let faces: Vec<u32> = record
            .shells
            .iter()
            .find_map(|s| match s {
                solidcore_record::brep::Shell::Faces { faces, .. } if faces.iter().any(|f| f.face == index) => {
                    Some(faces.iter().map(|f| f.face).collect())
                }
                _ => None,
            })
            .ok_or_else(|| KernelError::Invalid("Delete Face: the face is in no shell".into()))?;
        let mesh = b.mesh(&MeshTolerance::chord(b.size() * 1e-2))?;
        let at: Vec<Vec3> = faces
            .iter()
            .filter_map(|f| {
                let t = mesh.tri_face.iter().position(|x| *x == *f)?;
                let [i, j, k] = *mesh.triangles.get(t)?;
                let at = |v: u32| mesh.positions.get(v as usize).copied();
                Some((at(i)? + at(j)? + at(k)?) * (1.0 / 3.0))
            })
            .collect();
        if at.len() != faces.len() {
            return Err(KernelError::Failed("Delete Face: a face of the void has no triangles".into()));
        }
        let sheet = copy_faces(b, &at)?;
        let fill = stitch(&[&sheet], 1e-3)?;
        boolean(b, &fill, BoolOp::Union)?.ok_or_else(|| KernelError::Failed("Delete Face: nothing is left".into()))
    }

    pub fn thicken_with_tolerance(b: &Body, t: f64, sym: bool, _tolerance: f64) -> Result<Body> {
        thicken(b, t, sym)
    }

    #[derive(Clone, Copy, Debug)]
    pub struct AxialCone { pub radius: f64, pub slope: f64, pub number: usize }
    #[derive(Clone, Debug)]
    pub enum AxialShape {
        Turned { segs: Vec<Seg2>, numbers: Vec<usize> },
        Prism { corners: Vec<Vec2>, z0: f64, z1: f64, numbers: Vec<usize>, start_cone: Option<AxialCone>, end_cone: Option<AxialCone> },
    }
    #[derive(Clone, Debug)]
    pub struct AxialRun { pub shape: AxialShape, pub after: Option<usize> }
    #[derive(Clone, Copy, Debug)]
    pub struct AxialDrill { pub center: Vec2, pub radius: f64, pub z0: f64, pub z1: f64, pub number: usize }
    #[derive(Clone, Copy, Debug)]
    pub struct TeeNumbers { pub run: usize, pub branch: usize, pub run_bore: usize, pub branch_bore: usize, pub low_end: usize, pub high_end: usize, pub branch_end: usize }

}

/// Traits the plain types' geometry moved to, and the old `Body`'s methods.
pub mod prelude {
    pub use super::BodyCompat as _;
    pub use solidcraft_geom::{Loop2Geom as _, PlaneGeom as _, Region2Geom as _, Seg2Geom as _};
    pub use solidcraft_geom::{Vec2Geom as _, Vec3Geom as _};
}

use sk::{Body, Result};

/// Methods the old `Body` had.
pub trait BodyCompat {
    fn tessellate(&self, tol: f64) -> Result<sg::Mesh>;
    fn display_mesh(&self, tol: f64) -> Result<sg::Mesh>;
    fn contains(&self, p: sg::Vec3) -> bool;
    fn face_corners(&self) -> Vec<Option<sg::Vec3>>;
}

impl BodyCompat for Body {
    fn tessellate(&self, tol: f64) -> Result<sg::Mesh> {
        self.mesh(&sk::MeshTolerance::chord(tol))
    }
    fn display_mesh(&self, tol: f64) -> Result<sg::Mesh> {
        self.mesh(&sk::MeshTolerance::display(tol))
    }
    fn contains(&self, p: sg::Vec3) -> bool {
        matches!(self.classify_point(p), Ok(sk::PointClass::Inside))
    }
    fn face_corners(&self) -> Vec<Option<sg::Vec3>> {
        let r = self.record();
        r.faces
            .iter()
            .map(|f| {
                let v = match f.loops.first()? {
                    solidcore_record::brep::Loop::Edges { edges } => {
                        let u = edges.first()?;
                        let e = r.edges.get(u.edge as usize)?;
                        r.vertices.get(e.vertices[usize::from(!u.forward)] as usize)?
                    }
                    solidcore_record::brep::Loop::Vertex { vertex } => r.vertices.get(*vertex as usize)?,
                };
                Some(sg::Vec3::new(v.point[0], v.point[1], v.point[2]))
            })
            .collect()
    }
}
