//! Arrange Dimensions (`docs/spec/4.7-drawing-commands.md`): the chosen
//! horizontal and vertical dimensions of each view side move into rows
//! parallel to the view — the first row the style's Part Offset outside the
//! view's box, each next one Spacing further out. Shorter dimensions go
//! first; dimensions whose extents do not overlap share a row (touching is
//! fine), as Inventor arranges them. The text keeps its place along the
//! dimension line.

use super::annot::{DimKind, Dimension, Placement, dim_points};
use super::hlr::ViewLines;

/// Which side of its view a dimension lies on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Above,
    Below,
    Left,
    Right,
}

/// One dimension to arrange: id, extent along its line (lo, hi), text
/// point on the sheet, its view's placement.
type Entry = (u32, f64, f64, [f64; 2], Placement);

/// New text positions (relative to the view's position, as
/// `Dimension::text`) of the dimensions `dims` (annotation id, dimension)
/// that can be arranged; the others are left out.
pub fn arrange<'a>(
    dims: &[(u32, &Dimension)],
    view: &dyn Fn(u32) -> Option<(Placement, &'a ViewLines)>,
    part_offset: f64,
    spacing: f64,
) -> Vec<(u32, [f64; 2])> {
    let mut groups: Vec<((u32, Side), Vec<Entry>)> = Vec::new();
    let mut edge_of: Vec<((u32, Side), f64)> = Vec::new();
    for &(id, d) in dims {
        let ax = match d.dim {
            DimKind::Horizontal => 0,
            DimKind::Vertical => 1,
            _ => continue,
        };
        let Some((pa, pb, pl, lines)) = dim_points(d, view) else {
            continue;
        };
        let (lo, hi) = (pl.to_sheet(lines.min), pl.to_sheet(lines.max));
        let (bmin, bmax) = (
            [lo[0].min(hi[0]), lo[1].min(hi[1])],
            [lo[0].max(hi[0]), lo[1].max(hi[1])],
        );
        let t = [pl.position[0] + d.text[0], pl.position[1] + d.text[1]];
        let nx = 1 - ax;
        let side = if t[nx] > bmax[nx] {
            if ax == 0 { Side::Above } else { Side::Right }
        } else if t[nx] < bmin[nx] {
            if ax == 0 { Side::Below } else { Side::Left }
        } else {
            continue;
        };
        let edge = match side {
            Side::Above | Side::Right => bmax[nx],
            Side::Below | Side::Left => bmin[nx],
        };
        let key = (d.a.view, side);
        if !edge_of.iter().any(|(k, _)| *k == key) {
            edge_of.push((key, edge));
        }
        let span = (pa[ax].min(pb[ax]), pa[ax].max(pb[ax]));
        let entry = (id, span.0, span.1, t, pl);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(entry),
            None => groups.push((key, vec![entry])),
        }
    }
    let mut out = Vec::new();
    for (key, mut g) in groups {
        let edge = edge_of.iter().find(|(k, _)| *k == key).map_or(0.0, |x| x.1);
        let (sign, nx) = match key.1 {
            Side::Above => (1.0, 1),
            Side::Below => (-1.0, 1),
            Side::Right => (1.0, 0),
            Side::Left => (-1.0, 0),
        };
        // Shortest first; equal lengths keep their order from the view.
        g.sort_by(|a, b| (a.2 - a.1).total_cmp(&(b.2 - b.1)));
        let mut rows: Vec<Vec<(f64, f64)>> = Vec::new();
        let eps = 1e-6;
        for (id, lo, hi, t, pl) in g {
            let row = rows
                .iter()
                .position(|r| r.iter().all(|&(a, b)| hi <= a + eps || lo >= b - eps))
                .unwrap_or_else(|| {
                    rows.push(Vec::new());
                    rows.len() - 1
                });
            rows[row].push((lo, hi));
            let mut nt = t;
            nt[nx] = edge + sign * (part_offset + spacing * row as f64);
            out.push((id, [nt[0] - pl.position[0], nt[1] - pl.position[1]]));
        }
    }
    out
}
