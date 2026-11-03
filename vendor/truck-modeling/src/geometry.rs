use super::*;
use derive_more::*;
use serde::{Deserialize, Serialize};
#[doc(hidden)]
pub use truck_geometry::prelude::{algo, inv_or_zero};
pub use truck_geometry::{decorators::*, nurbs::*, specifieds::*};
pub use truck_polymesh::PolylineCurve;

/// Leading curve for intersection
#[derive(
    Clone,
    Debug,
    Serialize,
    Deserialize,
    From,
    TryInto,
    ParametricCurve,
    BoundedCurve,
    ParameterDivision1D,
    Cut,
    Invertible,
    SearchNearestParameterD1,
    SearchParameterD1,
)]
pub enum Leader {
    /// polyline curve
    Polyline(PolylineCurve<Point3>),
    /// bspline curve
    BSpline(BSplineCurve<Point3>),
}

/// 3-dimensional curve
#[derive(
    Clone,
    Debug,
    Serialize,
    Deserialize,
    From,
    TryInto,
    ParametricCurve,
    BoundedCurve,
    ParameterDivision1D,
    Cut,
    Invertible,
    SearchNearestParameterD1,
    SearchParameterD1,
)]
pub enum Curve {
    /// line
    Line(Line<Point3>),
    /// 3-dimensional B-spline curve
    BSplineCurve(BSplineCurve<Point3>),
    /// 3-dimensional NURBS curve
    NurbsCurve(NurbsCurve<Vector4>),
    /// intersection curve
    IntersectionCurve(IntersectionCurve<Leader, Surface>),
}

macro_rules! derive_curve_method {
    ($curve: expr, $method: expr, $($ver: ident),*) => {
        match $curve {
            Curve::Line(got) => $method(got, $($ver), *),
            Curve::BSplineCurve(got) => $method(got, $($ver), *),
            Curve::NurbsCurve(got) => $method(got, $($ver), *),
            Curve::IntersectionCurve(got) => $method(got, $($ver), *),
        }
    };
}

macro_rules! derive_curve_self_method {
    ($curve: expr, $method: expr, $($ver: ident),*) => {
        match $curve {
            Curve::Line(got) => Curve::Line($method(got, $($ver), *)),
            Curve::BSplineCurve(got) => Curve::BSplineCurve($method(got, $($ver), *)),
            Curve::NurbsCurve(got) => Curve::NurbsCurve($method(got, $($ver), *)),
            Curve::IntersectionCurve(got) => Curve::IntersectionCurve($method(got, $($ver), *)),
        }
    };
}

impl Transformed<Matrix4> for Leader {
    fn transform_by(&mut self, trans: Matrix4) {
        match self {
            Leader::Polyline(x) => Transformed::transform_by(x, trans),
            Leader::BSpline(x) => Transformed::transform_by(x, trans),
        }
    }
    fn transformed(&self, trans: Matrix4) -> Leader {
        match self {
            Leader::Polyline(x) => Leader::Polyline(Transformed::transformed(x, trans)),
            Leader::BSpline(x) => Leader::BSpline(Transformed::transformed(x, trans)),
        }
    }
}

impl Transformed<Matrix4> for Curve {
    fn transform_by(&mut self, trans: Matrix4) {
        derive_curve_method!(self, Transformed::transform_by, trans);
    }
    fn transformed(&self, trans: Matrix4) -> Self {
        derive_curve_self_method!(self, Transformed::transformed, trans)
    }
}

impl From<IntersectionCurve<PolylineCurve<Point3>, Surface>> for Curve {
    fn from(x: IntersectionCurve<PolylineCurve<Point3>, Surface>) -> Curve {
        Curve::IntersectionCurve(x.change_leader(Leader::Polyline))
    }
}

impl Curve {
    /// Into non-ratinalized 4-dimensinal B-spline curve
    pub fn lift_up(self) -> BSplineCurve<Vector4> {
        match self {
            Curve::Line(curve) => Curve::BSplineCurve(curve.to_bspline()).lift_up(),
            Curve::BSplineCurve(curve) => BSplineCurve::new(
                curve.knot_vec().clone(),
                curve
                    .control_points()
                    .iter()
                    .map(|pt| pt.to_vec().extend(1.0))
                    .collect(),
            ),
            Curve::NurbsCurve(curve) => curve.into_non_rationalized(),
            Curve::IntersectionCurve(_) => {
                unimplemented!("intersection curve cannot connect by homotopy")
            }
        }
    }
    /// Make the leaders of `IntersectionCurve`s B-spline curves.
    pub fn to_bspline_leader(&mut self, p_tol: f64, d_tol: f64, trials: usize) -> bool {
        if let Curve::IntersectionCurve(ref mut curve) = self {
            if matches!(curve.leader(), Leader::Polyline(_)) {
                if let Some(bspcurve) = BSplineCurve::cubic_approximation(
                    curve,
                    curve.range_tuple(),
                    p_tol,
                    d_tol,
                    trials,
                ) {
                    let editor = curve.editor();
                    *editor.leader = Leader::BSpline(bspcurve);
                    *editor.tol = p_tol;
                    return true;
                }
            }
        }
        false
    }
}

/// 3-dimensional surfaces
#[derive(
    Clone,
    Debug,
    Serialize,
    Deserialize,
    From,
    TryInto,
    ParametricSurface,
    ParameterDivision2D,
    Invertible,
    SearchParameterD2,
)]
pub enum Surface {
    /// Plane
    Plane(Plane),
    /// 3-dimensional B-spline surface
    BSplineSurface(BSplineSurface<Point3>),
    /// 3-dimensional NURBS Surface
    NurbsSurface(NurbsSurface<Vector4>),
    /// revoluted curve
    RevolutedCurve(Processor<RevolutedCurve<Curve>, Matrix4>),
}

macro_rules! derive_surface_method {
    ($surface: expr, $method: expr, $($ver: ident),*) => {
        match $surface {
            Self::Plane(got) => $method(got, $($ver), *),
            Self::BSplineSurface(got) => $method(got, $($ver), *),
            Self::NurbsSurface(got) => $method(got, $($ver), *),
            Self::RevolutedCurve(got) => $method(got, $($ver), *),
        }
    };
}

macro_rules! derive_surface_self_method {
    ($surface: expr, $method: expr, $($ver: ident),*) => {
        match $surface {
            Self::Plane(got) => Self::Plane($method(got, $($ver), *)),
            Self::BSplineSurface(got) => Self::BSplineSurface($method(got, $($ver), *)),
            Self::NurbsSurface(got) => Self::NurbsSurface($method(got, $($ver), *)),
            Self::RevolutedCurve(got) => Self::RevolutedCurve($method(got, $($ver), *)),
        }
    };
}

impl ParametricSurface3D for Surface {
    #[inline(always)]
    fn normal(&self, u: f64, v: f64) -> Vector3 {
        derive_surface_method!(self, ParametricSurface3D::normal, u, v)
    }
}

impl Transformed<Matrix4> for Surface {
    fn transform_by(&mut self, trans: Matrix4) {
        derive_surface_method!(self, Transformed::transform_by, trans);
    }
    fn transformed(&self, trans: Matrix4) -> Self {
        derive_surface_self_method!(self, Transformed::transformed, trans)
    }
}

impl IncludeCurve<Curve> for Surface {
    #[inline(always)]
    fn include(&self, curve: &Curve) -> bool {
        match self {
            Surface::BSplineSurface(surface) => match curve {
                Curve::Line(curve) => surface.include(&curve.to_bspline()),
                Curve::BSplineCurve(curve) => surface.include(curve),
                Curve::NurbsCurve(curve) => surface.include(curve),
                Curve::IntersectionCurve(_) => unimplemented!(),
            },
            Surface::NurbsSurface(surface) => match curve {
                Curve::Line(curve) => surface.include(&curve.to_bspline()),
                Curve::BSplineCurve(curve) => surface.include(curve),
                Curve::NurbsCurve(curve) => surface.include(curve),
                Curve::IntersectionCurve(_) => unimplemented!(),
            },
            Surface::Plane(surface) => match curve {
                Curve::Line(curve) => surface.include(&curve.to_bspline()),
                Curve::BSplineCurve(curve) => surface.include(curve),
                Curve::NurbsCurve(curve) => surface.include(curve),
                Curve::IntersectionCurve(_) => unimplemented!(),
            },
            Surface::RevolutedCurve(surface) => match surface.entity_curve() {
                Curve::Line(curve) => self.include(&Curve::BSplineCurve(curve.to_bspline())),
                Curve::BSplineCurve(entity_curve) => {
                    let surface = RevolutedCurve::by_revolution(
                        entity_curve,
                        surface.origin(),
                        surface.axis(),
                    );
                    match curve {
                        Curve::Line(curve) => surface.include(&curve.to_bspline()),
                        Curve::BSplineCurve(curve) => surface.include(curve),
                        Curve::NurbsCurve(curve) => surface.include(curve),
                        Curve::IntersectionCurve(_) => unimplemented!(),
                    }
                }
                Curve::NurbsCurve(entity_curve) => {
                    let surface = RevolutedCurve::by_revolution(
                        entity_curve,
                        surface.origin(),
                        surface.axis(),
                    );
                    match curve {
                        Curve::Line(curve) => surface.include(&curve.to_bspline()),
                        Curve::BSplineCurve(curve) => surface.include(curve),
                        Curve::NurbsCurve(curve) => surface.include(curve),
                        Curve::IntersectionCurve(_) => unimplemented!(),
                    }
                }
                Curve::IntersectionCurve(_) => unimplemented!(),
            },
        }
    }
}

impl SearchNearestParameter<D2> for Surface {
    type Point = Point3;
    fn search_nearest_parameter<H: Into<SPHint2D>>(
        &self,
        point: Point3,
        hint: H,
        trials: usize,
    ) -> Option<(f64, f64)> {
        match self {
            Surface::Plane(plane) => plane.search_nearest_parameter(point, hint, trials),
            Surface::BSplineSurface(bspsurface) => {
                bspsurface.search_nearest_parameter(point, hint, trials)
            }
            Surface::NurbsSurface(surface) => surface.search_nearest_parameter(point, hint, trials),
            Surface::RevolutedCurve(rotted) => {
                let hint = match hint.into() {
                    SPHint2D::Parameter(hint0, hint1) => (hint0, hint1),
                    SPHint2D::Range(x, y) => revolved_presearch(rotted, point, (x, y)),
                    SPHint2D::None => revolved_presearch(rotted, point, rotted.range_tuple()),
                };
                algo::surface::search_nearest_parameter(rotted, point, hint, trials)
            }
        }
    }
}

/// 3DCadCraft: a revolved straight line (a cylinder, a cone) keeps its grid of the search without
/// a hint (8 of 10201 points a thread: 2 MB), read at 0.5 ms a point otherwise.
fn revolved_presearch(
    surface: &Processor<RevolutedCurve<Curve>, Matrix4>,
    point: Point3,
    range: ((f64, f64), (f64, f64)),
) -> (f64, f64) {
    use std::cell::RefCell;
    use truck_geometry::presearch_grid::PresearchGrids;
    type Key = (Point3, Point3, Point3, Vector3, Matrix4, bool);
    const DIVISION: usize = 100;
    thread_local! {
        static GRIDS: RefCell<PresearchGrids<Key>> =
            const { RefCell::new(PresearchGrids::new(8, DIVISION)) };
    }
    let Curve::Line(line) = surface.entity().entity_curve() else {
        return algo::surface::presearch(surface, point, range, DIVISION);
    };
    let key = (
        line.0,
        line.1,
        surface.entity().origin(),
        surface.entity().axis(),
        *surface.transform(),
        surface.orientation(),
    );
    GRIDS.with_borrow_mut(|g| g.presearch(&key, |u, v| surface.subs(u, v), point, range))
}

#[cfg(test)]
mod presearch_tests {
    use super::*;

    /// A cone, asked at many points and on two ranges: the answers of the plain search.
    #[test]
    fn the_same_answer_as_the_plain_search() {
        let line = Curve::Line(Line(Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 3.0)));
        let cone = Processor::new(RevolutedCurve::by_revolution(
            line,
            Point3::origin(),
            Vector3::unit_z(),
        ));
        let ranges = [cone.range_tuple(), ((0.1, 0.9), (0.2, 3.0))];
        for k in 0..60 {
            let x = k as f64 * 0.17;
            let point = Point3::new(x.cos() * 1.3, x.sin() * 1.7, x * 0.4);
            let range = ranges[k % 2];
            let got = revolved_presearch(&cone, point, range);
            let plain = algo::surface::presearch(&cone, point, range, 100);
            assert_eq!(got.0.to_bits(), plain.0.to_bits());
            assert_eq!(got.1.to_bits(), plain.1.to_bits());
        }
    }
}
