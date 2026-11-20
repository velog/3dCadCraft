//! **Dynamic Simulation** (`docs/spec/7.2-dynamic-simulation.md`), GUI-free: an assembly's
//! components moved in time by gravity, springs, motors and loads, held together by its joints.
//!
//! The solver is SolidCraft's multibody dynamics crate (`vendor/solidcraft-dynamics`: rigid bodies
//! in maximal coordinates, Lagrange multipliers, Runge–Kutta 4 or Dormand–Prince 5(4), projected
//! back onto the joints after every step). Only our own types cross this module's boundary.
//!
//! The mapping (Inventor's Dynamic Simulation takes the assembly's joints the same way):
//! - every component that is neither grounded nor suppressed and that a joint reaches is a moving
//!   body; its mass, centre of gravity and inertia come from its part's Physical Properties (the
//!   material's density), its axes are the part's own at the start;
//! - grounded components (and components no joint reaches) stand still;
//! - our joints (Rigid, Rotational, Slider, Cylindrical, Planar, Ball) become the solver's joints of
//!   the same kind between the two joint origins (the second origin faces the first unless
//!   flipped, so its z is turned round for the solver, whose frames coincide); a locked joint
//!   becomes Rigid (a locked Cylindrical: Rotational);
//! - relationships that are no joints (Mate, Flush, Angle, Insert, Tangent, Symmetry) are not
//!   taken: they are listed in [`Run::skipped`] (Inventor converts them to joints first).
//!
//! Units are the assembly's: mm, s, radians; masses in kg, forces in mN (kg·mm/s²).

use std::collections::HashMap;

use solidcore_record::Vec3;
use solidcraft_dynamics as dy;

use super::joint::{Frame, frame_of};
use super::relation::{ConstraintKind, Geom, GeomRef};
use crate::document::AssemblyDocument;
use crate::kernel::{Point3, Real, Transform3, Vector3};
use crate::part::physical::MassProps;

/// Standard gravity (mm/s²).
pub const STANDARD_GRAVITY: Real = dy::STANDARD_GRAVITY;

/// A spring and damper on a joint's value (a torsion spring on a Rotational, a linear one on a
/// Slider): it pushes back by `stiffness·(value − rest) + damping·rate`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointSpring {
    /// The joint (relationship id).
    pub joint: u32,
    /// Which of its values (0 for one-value joints; a Cylindrical's angle 0, distance 1).
    pub value: usize,
    /// mN·mm per radian, or mN per mm (= N/m).
    pub stiffness: Real,
    /// mN·mm·s per radian, or mN·s per mm.
    pub damping: Real,
    /// The value it rests at (radians or mm).
    pub rest: Real,
}

/// A constant force (mN) at a point of a component (its part's coordinates) and a torque
/// (mN·mm), both in the assembly's axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Load {
    /// The component (occurrence id).
    pub occurrence: u32,
    /// The point it acts at (its part's coordinates).
    pub point: Point3,
    /// The force (mN).
    pub force: Vector3,
    /// The torque (mN·mm).
    pub torque: Vector3,
}

/// What to simulate and how.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Simulated time (s).
    pub duration: Real,
    /// Time between the samples returned (s).
    pub interval: Real,
    /// Gravity (mm/s²); Inventor's default points down the assembly's −Z here (our models stand
    /// on XY).
    pub gravity: Vector3,
    /// Joint springs.
    pub springs: Vec<JointSpring>,
    /// Loads.
    pub loads: Vec<Load>,
    /// Relative tolerance of the adaptive integrator.
    pub tolerance: Real,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            duration: 1.0,
            interval: 0.01,
            gravity: Vector3::new(0.0, 0.0, -STANDARD_GRAVITY),
            springs: Vec::new(),
            loads: Vec::new(),
            tolerance: 1e-8,
        }
    }
}

/// One output time.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    /// Time (s).
    pub time: Real,
    /// Where each moving component stands (occurrence id → placement).
    pub placements: HashMap<u32, Transform3>,
    /// Each joint's values (relationship id → values: radians and mm, the solver's order).
    pub joint_values: HashMap<u32, Vec<Real>>,
    /// Kinetic + potential energy minus the work put in plus what dampers took out (µJ): it
    /// stays constant up to the integration error.
    pub energy_balance: Real,
    /// The largest joint error (mm).
    pub constraint_error: Real,
}

/// A simulation's result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    /// The samples, at the start and every interval on.
    pub samples: Vec<Sample>,
    /// The moving components (occurrence ids).
    pub moving: Vec<u32>,
    /// Relationships that were not taken (no joints), by id.
    pub skipped: Vec<u32>,
}

fn v(p: Vector3) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

/// The rotation of `t` as the solver's quaternion.
fn quat(t: &Transform3) -> dy::Quat {
    let q = t.rotation.quaternion();
    dy::Quat {
        w: q.w,
        x: q.i,
        y: q.j,
        z: q.k,
    }
}

fn placement(s: &dy::BodyState, center: Point3) -> Transform3 {
    let q = nalgebra::UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(
        s.orientation.w,
        s.orientation.x,
        s.orientation.y,
        s.orientation.z,
    ));
    let p = Vector3::new(s.position.x, s.position.y, s.position.z);
    // The body's frame is the part's axes at its centre of gravity: x_world = p + R (x − c).
    Transform3::from_parts(nalgebra::Translation3::from(p - q * center.coords), q)
}

/// Runs a dynamic simulation of `asm`. `local` gives a reference's geometry in its part's
/// coordinates (as for [`super::solve::solve`]); `mass` gives a component's mass properties in its
/// part's coordinates (g, g·mm², as Physical Properties report them).
pub fn simulate(
    asm: &AssemblyDocument,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
    mass: &dyn Fn(u32) -> Option<MassProps>,
    settings: &Settings,
) -> Result<Run, String> {
    let fail = |e: String| format!("Dynamic Simulation: {e}");
    let on = |id: u32| asm.find(id).filter(|o| !o.suppressed);
    let moves = |id: u32| on(id).is_some_and(|o| !o.grounded) && !asm.is_element(id);
    let mut run = Run::default();
    let mut joints = Vec::new();
    for c in asm.constraints.iter().filter(|c| !c.suppressed) {
        let (a, b) = (c.a.occurrence(), c.b.occurrence());
        let ends_on = [a, b]
            .iter()
            .all(|&o| o == super::relation::ASSEMBLY || on(o).is_some());
        if !ends_on {
            continue;
        }
        if !c.kind.is_joint() {
            run.skipped.push(c.id);
            continue;
        }
        joints.push(c);
    }
    // The moving bodies: components a joint reaches.
    let mut index: HashMap<u32, usize> = HashMap::new();
    let mut bodies = Vec::new();
    let mut centers = Vec::new();
    for c in &joints {
        for id in [c.a.occurrence(), c.b.occurrence()] {
            if !moves(id) || index.contains_key(&id) {
                continue;
            }
            let o = asm
                .find(id)
                .ok_or_else(|| fail(format!("component {id} is missing")))?;
            let m = mass(id).ok_or_else(|| {
                fail(format!(
                    "{}: no mass properties (a part without a solid?)",
                    o.name
                ))
            })?;
            if m.mass.is_nan() || m.mass <= 0.0 {
                return Err(fail(format!(
                    "{}: its mass is 0 (give the part a material with a density)",
                    o.name
                )));
            }
            let i = m.inertia / 1000.0;
            let inertia = dy::Mat3([
                [i[(0, 0)], i[(0, 1)], i[(0, 2)]],
                [i[(1, 0)], i[(1, 1)], i[(1, 2)]],
                [i[(2, 0)], i[(2, 1)], i[(2, 2)]],
            ]);
            let world = o.transform * m.center;
            index.insert(id, bodies.len());
            run.moving.push(id);
            centers.push(m.center);
            bodies.push(dy::Body {
                mass: m.mass / 1000.0,
                inertia,
                position: v(world.coords),
                orientation: quat(&o.transform),
                velocity: Vec3::ZERO,
                angular_velocity: Vec3::ZERO,
            });
        }
    }
    if bodies.is_empty() {
        return Err(fail(
            "no component moves: nothing that is not grounded has a joint".into(),
        ));
    }
    let place = |id: u32| -> Transform3 {
        asm.find(id)
            .map(|o| o.transform)
            .unwrap_or_else(Transform3::identity)
    };
    // A joint origin as a marker: on a moving body in its own coordinates (axes the part's,
    // origin its centre of gravity), else fixed in the world.
    let marker = |id: u32, f: &Frame| -> dy::Marker {
        match index.get(&id) {
            Some(&k) => dy::Marker::new(Some(k), v(f.p - centers[k]), v(f.z), v(f.x)),
            None => {
                let t = place(id);
                dy::Marker::ground(v((t * f.p).coords), v(t * f.z), v(t * f.x))
            }
        }
    };
    let mut model = dy::Model {
        bodies,
        gravity: v(settings.gravity),
        ..Default::default()
    };
    let mut joint_index: HashMap<u32, usize> = HashMap::new();
    for c in &joints {
        let (ia, ib) = (c.a.occurrence(), c.b.occurrence());
        let fa = frame_of(
            &super::relation::for_kind(c.kind, &c.a, local(&c.a).map_err(fail)?).as_frame(),
        );
        let fb = frame_of(
            &super::relation::for_kind(c.kind, &c.b, local(&c.b).map_err(fail)?).as_frame(),
        );
        use ConstraintKind::*;
        let kind = match (c.kind, c.locked) {
            (Rigid, _) | (Rotational | Slider, true) => dy::JointKind::Rigid,
            (Rotational, false) => dy::JointKind::Revolute,
            (Slider, false) => dy::JointKind::Slider,
            (Cylindrical, true) => dy::JointKind::Revolute,
            (Cylindrical, false) => dy::JointKind::Cylindrical,
            (Planar, _) => dy::JointKind::Planar,
            (Ball, _) => dy::JointKind::Ball,
            _ => continue,
        };
        let a = marker(ia, &fa);
        // The second origin faces the first unless flipped; the solver's frames coincide. A
        // joint whose value is held takes the first origin itself, where it stands now, so the
        // start meets it exactly.
        let b = if kind == dy::JointKind::Rigid || (c.kind == Cylindrical && c.locked) {
            let (ta, tb) = (place(ia), place(ib));
            let w = Frame {
                p: ta * fa.p,
                z: ta * fa.z,
                x: ta * fa.x,
            };
            let inv = tb.inverse();
            marker(
                ib,
                &Frame {
                    p: inv * w.p,
                    z: inv * w.z,
                    x: inv * w.x,
                },
            )
        } else {
            let z = if c.aligned { fb.z } else { -fb.z };
            marker(
                ib,
                &Frame {
                    p: fb.p,
                    z,
                    x: fb.x,
                },
            )
        };
        joint_index.insert(c.id, model.joints.len());
        model.joints.push(dy::Joint { kind, a, b });
    }
    for s in &settings.springs {
        let &j = joint_index
            .get(&s.joint)
            .ok_or_else(|| fail(format!("the spring's joint {} is not simulated", s.joint)))?;
        model.forces.push(dy::Force::JointSpring {
            at: dy::Coord::new(j, s.value),
            stiffness: s.stiffness,
            damping: s.damping,
            rest: s.rest,
        });
    }
    for l in &settings.loads {
        let &k = index.get(&l.occurrence).ok_or_else(|| {
            fail(format!(
                "the load's component {} does not move",
                l.occurrence
            ))
        })?;
        model.forces.push(dy::Force::Load {
            body: k,
            point: v(l.point - centers[k]),
            force: v(l.force),
            torque: v(l.torque),
        });
    }
    let s = dy::Settings::adaptive(
        settings.duration,
        settings.interval,
        settings.tolerance,
        settings.tolerance,
    );
    let traj = crate::diagnostics::crash::guard(|| dy::simulate(&model, &s))
        .map_err(|_| fail("internal error in the solver".into()))?
        .map_err(|e| fail(e.to_string()))?;
    for smp in &traj.samples {
        let placements = run
            .moving
            .iter()
            .zip(&smp.bodies)
            .zip(&centers)
            .map(|((&id, b), &c)| (id, placement(b, c)))
            .collect();
        let joint_values = joint_index
            .iter()
            .filter_map(|(&id, &j)| Some((id, smp.joints.get(j)?.values.clone())))
            .collect();
        run.samples.push(Sample {
            time: smp.time,
            placements,
            joint_values,
            energy_balance: smp.energy.balance(),
            constraint_error: smp.constraint_error,
        });
    }
    Ok(run)
}

/// [`simulate`] of an assembly of `project` with references and masses as the app reads them
/// (parts through the tree, each part's Physical Properties at the Low accuracy).
pub fn simulate_in(
    project: &crate::document::Project,
    asm: &AssemblyDocument,
    settings: &Settings,
) -> Result<Run, String> {
    let linked = super::tree::Linked::new();
    let local = |r: &GeomRef| -> Result<Geom, String> {
        let n = super::tree::nested(project, &linked, asm, r.occurrence(), r.sub())?;
        let part = super::tree::part_of(project, &linked, n.component).ok_or("not a part")?;
        let g = super::relation::resolve_in(part, r)?;
        Ok(if r.sub().is_empty() {
            g
        } else {
            g.moved(&n.at)
        })
    };
    let mass = |id: u32| -> Option<MassProps> {
        let o = asm.find(id)?;
        let part = super::tree::part_of(project, &linked, o.component)?;
        crate::part::physical::physical_properties(part, Default::default())
            .ok()
            .map(|p| p.total)
    };
    simulate(asm, &local, &mass, settings)
}

/// [`pendulum`]'s hinge (assembly coordinates) and the bar's pin (its part's coordinates).
pub const PIVOT: Point3 = Point3::new(10.0, 20.0, 110.0);
/// The bar's pin (its part's coordinates).
pub const PIN: Point3 = Point3::new(5.0, 0.0, 95.0);

/// The sample of the specification: a grounded post and a bar hanging on a Rotational joint
/// (a compound pendulum), the bar's free end lifted by `lift` radians. Returns the assembly's
/// document, the bar's occurrence and the joint's id.
pub fn pendulum(
    project: &mut crate::document::Project,
    lift: Real,
) -> Result<(crate::document::DocumentId, u32, u32), String> {
    use super::relation::Constraint;
    use crate::document::{DocRef, DocumentData};
    use crate::part::PartDocument;
    use crate::part::work::{UcsDef, WorkDef, WorkFeature};
    let ucs = |part: &mut PartDocument, at: [Real; 3]| -> Result<String, String> {
        part.add_work(WorkFeature::new(WorkDef::Ucs(UcsDef::Offsets {
            x: at[0],
            y: at[1],
            z: at[2],
            rx: 0.0,
            ry: 0.0,
            rz: 0.0,
        })))
        .map_err(|e| e.to_string())
    };
    // The post: 20 × 20 × 120, its hinge 10 mm below the top, the axis along Y.
    let mut post =
        PartDocument::with_box("Box1", [20.0, 20.0, 120.0]).map_err(|e| e.to_string())?;
    let hinge = ucs(&mut post, [PIVOT.x, PIVOT.y, PIVOT.z])?;
    post.regenerate().map_err(|e| e.to_string())?;
    // The bar: 10 × 6 × 100 (Z long), hanging from its pin 5 mm below its top.
    let mut bar = PartDocument::with_box("Box1", [10.0, 6.0, 100.0]).map_err(|e| e.to_string())?;
    let pin = ucs(&mut bar, [PIN.x, PIN.y, PIN.z])?;
    bar.regenerate().map_err(|e| e.to_string())?;
    let post_doc = project.add_document("Post", DocumentData::Part(post));
    let bar_doc = project.add_document("Bar", DocumentData::Part(bar));
    let mut asm = AssemblyDocument::default();
    let p = asm.place(
        DocRef::local(post_doc),
        "Post",
        Transform3::identity(),
        true,
    );
    // The hinge axis is Y: the joint origins are the UCS's XZ planes (normal Y), so the bar
    // turns in the XZ plane.
    // The bar's pin on the post's hinge, the bar turned by `lift` about the hinge (Y).
    let turn = nalgebra::UnitQuaternion::from_axis_angle(&Vector3::y_axis(), lift);
    let t = nalgebra::Translation3::from(PIVOT.coords - turn * PIN.coords);
    let b = asm.place(
        DocRef::local(bar_doc),
        "Bar",
        Transform3::from_parts(t, turn),
        false,
    );
    let frame = |o: u32, id: &str| GeomRef::Work {
        occurrence: o,
        sub: Vec::new(),
        id: format!("{id}:XZ"),
    };
    let mut c = Constraint::new(ConstraintKind::Rotational, frame(p, &hinge), frame(b, &pin));
    c.aligned = true;
    let j = asm.add_constraint(c);
    let doc = project.add_document("Pendulum", DocumentData::Assembly(asm));
    Ok((doc, b, j))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{DocumentData, Project};

    fn asm(project: &Project, id: crate::document::DocumentId) -> AssemblyDocument {
        match &project.document(id).map(|d| &d.data) {
            Some(DocumentData::Assembly(a)) => a.clone(),
            _ => panic!("no assembly"),
        }
    }

    /// The bar swings about the hinge: the pin stays on the hinge, the energy balance holds, and
    /// the small-angle period is the compound pendulum's, T = 2π √(I_pivot / (m g d)).
    #[test]
    fn pendulum_swings_with_the_compound_period() {
        let mut project = Project::new("Pendulum");
        let lift = 0.05;
        let (doc, bar, joint) = pendulum(&mut project, lift).unwrap();
        let a = asm(&project, doc);
        let settings = Settings {
            duration: 1.0,
            interval: 0.002,
            ..Default::default()
        };
        let run = simulate_in(&project, &a, &settings).unwrap();
        assert_eq!(run.moving, vec![bar]);
        assert!(run.skipped.is_empty());
        // Bar 10 × 6 × 100, pin 5 mm from its end on the centre line: d = 45 mm,
        // I_cg = m (100² + 10²) / 12 about the hinge axis (Y), I_pivot = I_cg + m d².
        let (l, h, d) = (100.0_f64, 10.0_f64, 45.0_f64);
        let i_pivot = (l * l + h * h) / 12.0 + d * d;
        let period = std::f64::consts::TAU * (i_pivot / (STANDARD_GRAVITY * d)).sqrt();
        // The angle's sign changes: find the first two crossings of zero.
        let angle: Vec<(Real, Real)> = run
            .samples
            .iter()
            .map(|s| (s.time, s.joint_values[&joint][0]))
            .collect();
        let middle = mean(&angle);
        let mut crossings = Vec::new();
        for w in angle.windows(2) {
            let (x0, x1) = (w[0].1 - middle, w[1].1 - middle);
            if x0 * x1 < 0.0 {
                crossings.push(w[0].0 + (w[1].0 - w[0].0) * x0 / (x0 - x1));
            }
        }
        assert!(crossings.len() >= 3, "{crossings:?}");
        let measured = crossings[2] - crossings[0];
        // A 0.05 rad swing runs 0.016 % slower than the small-angle period.
        assert!(
            (measured - period).abs() < 2e-3 * period,
            "{measured} vs {period}"
        );
        let swing = angle.iter().map(|a| a.1).fold(Real::MIN, Real::max)
            - angle.iter().map(|a| a.1).fold(Real::MAX, Real::min);
        assert!((swing - 2.0 * lift).abs() < 1e-3, "{swing}");
        let e0 = run.samples[0].energy_balance;
        for s in &run.samples {
            assert!(
                (s.energy_balance - e0).abs() < 1e-6 * e0.abs().max(1.0),
                "{}",
                s.time
            );
            assert!(s.constraint_error < 1e-6);
            // The pin stays on the hinge.
            let pin = s.placements[&bar] * PIN;
            assert!((pin - PIVOT).norm() < 1e-5, "{pin}");
        }
    }

    fn mean(a: &[(Real, Real)]) -> Real {
        let (lo, hi) = a.iter().fold((Real::MAX, Real::MIN), |(lo, hi), x| {
            (lo.min(x.1), hi.max(x.1))
        });
        (lo + hi) / 2.0
    }

    /// With every component grounded nothing moves: an error, not an empty run.
    #[test]
    fn grounded_only_is_an_error() {
        let mut project = Project::new("Pendulum");
        let (doc, _, _) = pendulum(&mut project, 0.0).unwrap();
        let mut a = asm(&project, doc);
        for o in a.occurrences.iter_mut() {
            o.grounded = true;
        }
        let e = simulate_in(&project, &a, &Settings::default()).unwrap_err();
        assert!(
            e.starts_with("Dynamic Simulation: no component moves"),
            "{e}"
        );
    }
}
