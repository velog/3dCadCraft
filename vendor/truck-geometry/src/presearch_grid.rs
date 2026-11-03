//! 3DCadCraft: the grid of a search without a hint, kept between searches.
//!
//! A nearest-point search without a hint looks at a whole grid of surface points first, and the
//! same surfaces are asked again and again (a Boolean projects each point of its intersection
//! curves onto both). Evaluating the grid was most of a general Boolean's time. The answer is
//! the same as `algo::surface::presearch`: the same points in the same order, the first nearest.

use truck_base::cgmath64::*;

/// One remembered grid.
#[derive(Debug)]
struct Grid<K> {
    key: K,
    range: [u64; 4],
    points: Vec<Point3>,
}

/// The grids of the surfaces searched last, on one thread: the longest unused goes first.
#[derive(Debug)]
pub struct PresearchGrids<K> {
    grids: Vec<Grid<K>>,
    capacity: usize,
    division: usize,
}

impl<K: PartialEq + Clone> PresearchGrids<K> {
    /// At most `capacity` grids of `(division + 1)²` points.
    pub const fn new(capacity: usize, division: usize) -> Self {
        PresearchGrids { grids: Vec::new(), capacity, division }
    }

    /// The parameters of the grid point nearest to `point`. `key` says everything `surface`
    /// depends on.
    pub fn presearch(
        &mut self,
        key: &K,
        surface: impl Fn(f64, f64) -> Point3,
        point: Point3,
        ((u0, u1), (v0, v1)): ((f64, f64), (f64, f64)),
    ) -> (f64, f64) {
        let n = self.division;
        let at = |k: usize, a: f64, b: f64| {
            let p = k as f64 / n as f64;
            a * (1.0 - p) + b * p
        };
        let range = [u0.to_bits(), u1.to_bits(), v0.to_bits(), v1.to_bits()];
        match self.grids.iter().position(|g| g.range == range && g.key == *key) {
            Some(i) => {
                let grid = self.grids.remove(i);
                self.grids.push(grid);
            }
            None => {
                let mut points = Vec::with_capacity((n + 1) * (n + 1));
                for i in 0..=n {
                    for j in 0..=n {
                        points.push(surface(at(i, u0, u1), at(j, v0, v1)));
                    }
                }
                if self.grids.len() >= self.capacity {
                    self.grids.remove(0);
                }
                self.grids.push(Grid { key: key.clone(), range, points });
            }
        }
        let mut res = (0.0, 0.0);
        let mut min = f64::INFINITY;
        if let Some(grid) = self.grids.last() {
            for (k, q) in grid.points.iter().enumerate() {
                let dist = q.distance2(point);
                if dist < min {
                    min = dist;
                    res = (at(k / (n + 1), u0, u1), at(k % (n + 1), v0, v1));
                }
            }
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use truck_geotrait::{algo, ParametricSurface};

    /// A surface with no symmetry to hide a wrong grid order.
    #[derive(Clone)]
    struct Wavy(f64);
    impl ParametricSurface for Wavy {
        type Point = Point3;
        type Vector = Vector3;
        fn subs(&self, u: f64, v: f64) -> Point3 {
            Point3::new(u * 3.0, v * 2.0 + u * u, self.0 * (u * 7.0).sin() * (v * 5.0).cos())
        }
        fn uder(&self, _: f64, _: f64) -> Vector3 { Vector3::zero() }
        fn vder(&self, _: f64, _: f64) -> Vector3 { Vector3::zero() }
        fn uuder(&self, _: f64, _: f64) -> Vector3 { Vector3::zero() }
        fn uvder(&self, _: f64, _: f64) -> Vector3 { Vector3::zero() }
        fn vvder(&self, _: f64, _: f64) -> Vector3 { Vector3::zero() }
    }

    fn same(a: (f64, f64), b: (f64, f64)) -> bool {
        a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits()
    }

    #[test]
    fn the_same_answer_as_the_plain_search() {
        let surfaces = [Wavy(0.5), Wavy(1.5), Wavy(-0.7)];
        let ranges = [((0.0, 1.0), (0.0, 1.0)), ((-0.3, 0.8), (0.2, 2.1))];
        // Two grids only: going round three surfaces and two ranges misses, and asking again hits.
        let mut grids = PresearchGrids::<u64>::new(2, 10);
        for round in 0..40 {
            let (i, r) = (round % 3, round % 2);
            let s = &surfaces[i];
            let x = round as f64 * 0.11;
            let point = Point3::new(x, 1.0 - x * 0.5, 0.2 * x);
            for _ in 0..2 {
                let got = grids.presearch(&s.0.to_bits(), |u, v| s.subs(u, v), point, ranges[r]);
                let plain = algo::surface::presearch(s, point, ranges[r], 10);
                assert!(same(got, plain), "{got:?} {plain:?}");
            }
        }
        assert!(grids.grids.len() <= 2);
    }

    #[test]
    fn a_point_with_no_distance_gives_what_the_plain_search_gives() {
        let s = Wavy(0.5);
        let nan = Point3::new(f64::NAN, 0.0, 0.0);
        let range = ((0.2, 1.0), (0.1, 1.0));
        let got = PresearchGrids::<u64>::new(2, 10).presearch(&1, |u, v| s.subs(u, v), nan, range);
        assert!(same(got, algo::surface::presearch(&s, nan, range, 10)));
    }
}
