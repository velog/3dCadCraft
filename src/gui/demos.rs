//! Built-in demo sketches for validating the constraint solver by hand.
//!
//! Each starts slightly "wrong" so the first solve visibly does work, and
//! leaves some freedom so dragging points shows the constraints holding.

#![allow(
    clippy::expect_used,
    reason = "demo data is fixed and built at start-up; a failure is a programming error caught by tests"
)]

use crate::kernel::Plane;
use crate::sketch::{Constraint, Point2, Sketch};

/// Available demo sketches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Demo {
    /// H/V rectangle with a fixed corner and two dimensions (fully defined).
    Rectangle,
    /// Slot: two arcs joined by two tangent lines (1 DOF: rotation).
    Slot,
    /// Line and circles with tangencies (under-defined, drag around).
    Tangents,
    /// Parallel / perpendicular lines.
    Lines,
    /// A shaft half section closed by its centerline: ready for Revolve.
    Revolve,
}

impl Demo {
    /// All demos, in menu order.
    pub const ALL: [Demo; 5] = [
        Demo::Rectangle,
        Demo::Slot,
        Demo::Tangents,
        Demo::Lines,
        Demo::Revolve,
    ];

    /// Parses a short name (`rectangle`, `slot`, `tangents`, `lines`), as used
    /// by the `CADCRAFT3D_DEMO` environment variable.
    pub fn from_name(name: &str) -> Option<Demo> {
        Demo::ALL
            .into_iter()
            .find(|d| format!("{d:?}").eq_ignore_ascii_case(name.trim()))
    }

    /// Short label for ribbon buttons.
    pub fn short_label(self) -> &'static str {
        match self {
            Demo::Rectangle => "Rectangle",
            Demo::Slot => "Slot",
            Demo::Tangents => "Tangents",
            Demo::Lines => "Lines",
            Demo::Revolve => "Revolve",
        }
    }

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            Demo::Rectangle => "Rectangle (fully defined)",
            Demo::Slot => "Slot (arcs + tangent lines)",
            Demo::Tangents => "Tangent circles",
            Demo::Lines => "Parallel / perpendicular",
            Demo::Revolve => "Shaft half section (centerline closes it)",
        }
    }

    /// Builds the (unsolved) demo sketch on `plane`.
    pub fn build(self, plane: Plane) -> Sketch {
        let mut s = Sketch::new(plane);
        let p = Point2::new;
        // Demo construction uses only valid IDs and values, so `expect` cannot fire.
        const OK: &str = "demo sketch is valid";

        match self {
            Demo::Rectangle => {
                let c = [
                    s.add_fixed_point(p(0.0, 0.0)),
                    s.add_point(p(37.0, 2.0)),
                    s.add_point(p(42.0, 27.0)),
                    s.add_point(p(-1.5, 22.0)),
                ];
                let l: Vec<_> = (0..4)
                    .map(|i| s.add_line_between(c[i], c[(i + 1) % 4]).expect(OK))
                    .collect();
                for (line, horizontal) in [(l[0], true), (l[1], false), (l[2], true), (l[3], false)]
                {
                    let con = if horizontal {
                        Constraint::Horizontal { line }
                    } else {
                        Constraint::Vertical { line }
                    };
                    s.add_constraint(con).expect(OK);
                }
                s.add_constraint(Constraint::Distance {
                    a: c[0],
                    b: c[1],
                    value: 40.0,
                })
                .expect(OK);
                s.add_constraint(Constraint::Distance {
                    a: c[1],
                    b: c[2],
                    value: 25.0,
                })
                .expect(OK);
            }
            Demo::Slot => {
                let c1 = s.add_fixed_point(p(0.0, 0.0));
                let c2 = s.add_point(p(31.0, 1.5));
                let a_top = s.add_point(p(0.5, 10.5));
                let a_bot = s.add_point(p(-0.5, -9.0));
                let b_bot = s.add_point(p(30.0, -8.5));
                let b_top = s.add_point(p(31.5, 11.0));
                let left = s.add_arc(c1, a_top, a_bot).expect(OK);
                let right = s.add_arc(c2, b_bot, b_top).expect(OK);
                let top = s.add_line_between(b_top, a_top).expect(OK);
                let bottom = s.add_line_between(a_bot, b_bot).expect(OK);
                for (line, arc) in [(top, left), (top, right), (bottom, left), (bottom, right)] {
                    s.add_tangent(line, arc).expect(OK);
                }
                s.add_constraint(Constraint::Radius {
                    circle: left,
                    value: 10.0,
                })
                .expect(OK);
                s.add_constraint(Constraint::Radius {
                    circle: right,
                    value: 10.0,
                })
                .expect(OK);
                s.add_constraint(Constraint::Distance {
                    a: c1,
                    b: c2,
                    value: 30.0,
                })
                .expect(OK);
            }
            Demo::Tangents => {
                let center = s.add_fixed_point(p(0.0, 0.0));
                let big = s.add_circle_at(center, 14.0).expect(OK);
                s.add_constraint(Constraint::Radius {
                    circle: big,
                    value: 15.0,
                })
                .expect(OK);
                let line = s.add_line(p(-25.0, 17.0), p(25.0, 18.5));
                s.add_constraint(Constraint::Horizontal { line }).expect(OK);
                s.add_tangent(line, big).expect(OK);
                let small = s.add_circle(p(22.0, 9.0), 5.0).expect(OK);
                s.add_tangent(big, small).expect(OK);
                s.add_tangent(line, small).expect(OK);
            }
            Demo::Lines => {
                let a = s.add_line(p(0.0, 0.0), p(30.0, 4.0));
                let b = s.add_line(p(0.0, 12.0), p(30.0, 18.0));
                let c = s.add_line(p(10.0, -5.0), p(13.0, 25.0));
                let (a0, _) = s.line_points(a).expect(OK);
                s.set_fixed(a0, true).expect(OK);
                s.add_constraint(Constraint::Parallel { a, b }).expect(OK);
                s.add_constraint(Constraint::Perpendicular { a, b: c })
                    .expect(OK);
            }
            Demo::Revolve => {
                // Half section of a stepped shaft, closed by its centerline.
                let c = [
                    p(0.0, 0.0),
                    p(15.0, 0.0),
                    p(15.0, 10.0),
                    p(8.0, 10.0),
                    p(8.0, 32.0),
                    p(0.0, 32.0),
                ]
                .map(|q| s.add_point(q));
                for i in 0..6 {
                    let line = s.add_line_between(c[i], c[(i + 1) % 6]).expect(OK);
                    let con = if i % 2 == 0 {
                        Constraint::Horizontal { line }
                    } else {
                        Constraint::Vertical { line }
                    };
                    s.add_constraint(con).expect(OK);
                    if i == 5 {
                        s.set_style(line, crate::sketch::Style::Centerline)
                            .expect(OK);
                    }
                }
            }
        }
        s
    }
}
