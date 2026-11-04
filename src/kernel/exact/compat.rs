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

    pub fn prism_between(_: &[[(Plane, Loop2); 2]]) -> Result<Body> { not_ported("prism between planes") }
    pub fn loft_matched(_: &[(Plane, Loop2)]) -> Result<Body> { not_ported("matched loft") }
    pub fn loft_matched_ring(_: &[(Plane, Loop2)]) -> Result<Body> { not_ported("closed matched loft") }
    pub fn bridge_edges_tangent(_: &Body, _: &[Vec3], _: &Body, _: &[Vec3], _: [Option<f64>; 2]) -> Result<Body> { not_ported("bridge between edges") }
    pub fn patch_edges_tangent(_: &Body, _: &[Vec3], _: Option<f64>) -> Result<Body> { not_ported("tangent patch") }
    pub fn bspline_sheet(_: &[Vec<Vec3>]) -> Result<Body> { not_ported("B-spline sheet") }
    pub fn bspline_tube(_: &[Vec<Vec3>]) -> Result<Body> { not_ported("B-spline tube") }
    pub fn delete_shell(_: &Body, _: usize) -> Result<Body> { not_ported("delete shell") }
    pub fn thicken_with_tolerance(b: &Body, t: f64, sym: bool, _tolerance: f64) -> Result<Body> { thicken(b, t, sym) }
    pub fn chamfer_loops(_: &Body, _: &[Vec3], _: f64) -> Result<Body> { not_ported("chamfer of loops") }

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

    pub fn axial_stack(r: &[AxialRun], c: Option<usize>, closed: bool) -> Result<Body> { axial_stack_drilled(r, c, closed, &[]) }
    pub fn axial_stack_drilled(_: &[AxialRun], _: Option<usize>, _: bool, _: &[AxialDrill]) -> Result<Body> { not_ported("axial stack") }
    pub fn pipe_bend(_: f64, _: f64, _: f64, _: f64, _: [usize; 4]) -> Result<Body> { not_ported("pipe bend") }
    pub fn pipe_tee(_: f64, _: f64, _: f64, _: TeeNumbers) -> Result<Body> { not_ported("pipe tee") }
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
