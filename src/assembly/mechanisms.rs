//! Sample mechanisms for tests and demos (`docs/spec/3.1a-subassemblies.md`
//! S5, drag inside a flexible subassembly): chains of joints between blocks
//! whose joint origins are UCS work features of the parts.
//!
//! - [`arm`]: a turning arm, a slide running along it and a second arm
//!   turning on the slide (Rotational → Slider → Rotational).
//! - [`delta`]: a delta robot — three arms hinged on a grounded base, a rod
//!   on a Ball at each arm's tip, the three rods on Balls round one
//!   effector plate.
//!
//! [`solve_in`] solves an assembly of the project the way the app does
//! (references through the tree, flexible occurrences included).

use super::relation::{self, Constraint, ConstraintKind, Geom, GeomRef};
use super::solve::{Drag, Solved};
use super::{flexible, tree};
use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Occurrence, Project};
use crate::kernel::{Point3, Real, Transform3, Vector3};
use crate::part::PartDocument;
use crate::part::work::{UcsDef, WorkDef, WorkFeature};

/// A joint origin of a part: position and turns (as [`UcsDef::Offsets`]).
type At = ([Real; 3], [Real; 3]);

/// A block `size` (corner at the origin) with a UCS per joint origin; the
/// UCS ids in the order given.
fn block(size: [Real; 3], frames: &[At]) -> Result<(PartDocument, Vec<String>), String> {
    let mut part = PartDocument::with_box("Box1", size).map_err(|e| e.to_string())?;
    let mut ids = Vec::new();
    for ([x, y, z], [rx, ry, rz]) in frames {
        let def = WorkDef::Ucs(UcsDef::Offsets {
            x: *x,
            y: *y,
            z: *z,
            rx: *rx,
            ry: *ry,
            rz: *rz,
        });
        ids.push(
            part.add_work(WorkFeature::new(def))
                .map_err(|e| e.to_string())?,
        );
    }
    part.regenerate().map_err(|e| e.to_string())?;
    Ok((part, ids))
}

/// A UCS's XY plane on occurrence `o`: its frame for a joint.
fn frame(o: u32, ucs: &str) -> GeomRef {
    GeomRef::Work {
        occurrence: o,
        sub: Vec::new(),
        id: format!("{ucs}:XY"),
    }
}

fn joint(a: &mut AssemblyDocument, kind: ConstraintKind, x: GeomRef, y: GeomRef) -> u32 {
    let mut c = Constraint::new(kind, x, y);
    // The second origin keeps the first one's z (parts stand the same way up).
    c.aligned = true;
    a.add_constraint(c)
}

const HALF_PI: Real = std::f64::consts::FRAC_PI_2;

/// [`arm`]'s second arm: its far end's top middle (part coordinates).
pub const ARM_TIP: Point3 = Point3::new(60.0, 6.0, 6.0);

/// Ids of [`arm`]'s occurrences.
#[derive(Clone, Copy, Debug)]
pub struct ArmIds {
    /// The assembly document.
    pub doc: DocumentId,
    /// The grounded base, the first arm, the slide, the second arm.
    pub base: u32,
    /// The first arm (Rotational about the base's vertical).
    pub arm1: u32,
    /// The slide (Slider along the first arm).
    pub slide: u32,
    /// The second arm (Rotational about the slide's vertical).
    pub arm2: u32,
}

/// Adds "Arm" to `project`: a grounded 60 × 60 × 20 base (centred on the
/// origin), a 100 mm arm turning on its top centre, a 40 mm slide on top of
/// the arm sliding along it, and a 60 mm arm turning on the slide's far end.
pub fn arm(project: &mut Project) -> Result<ArmIds, String> {
    let (base, b) = block([60.0, 60.0, 20.0], &[([30.0, 30.0, 20.0], [0.0; 3])])?;
    let (arm1, a1) = block(
        [100.0, 20.0, 10.0],
        &[
            ([10.0, 10.0, 0.0], [0.0; 3]),
            // The rail: along the arm's X on its top.
            ([10.0, 10.0, 10.0], [0.0, HALF_PI, 0.0]),
        ],
    )?;
    let (slide, s) = block(
        [40.0, 16.0, 8.0],
        &[
            ([0.0, 8.0, 0.0], [0.0, HALF_PI, 0.0]),
            ([30.0, 8.0, 8.0], [0.0; 3]),
        ],
    )?;
    let (arm2, a2) = block([60.0, 12.0, 6.0], &[([6.0, 6.0, 0.0], [0.0; 3])])?;
    let doc = |p: &mut Project, name: &str, part| p.add_document(name, DocumentData::Part(part));
    let (base_d, arm1_d, slide_d, arm2_d) = (
        doc(project, "Base", base),
        doc(project, "Arm1", arm1),
        doc(project, "Slide", slide),
        doc(project, "Arm2", arm2),
    );
    let mut a = AssemblyDocument::default();
    let t = Transform3::translation;
    let base = a.place(DocRef::local(base_d), "Base", t(-30.0, -30.0, 0.0), true);
    let arm1 = a.place(DocRef::local(arm1_d), "Arm1", t(-10.0, -10.0, 20.0), false);
    let slide = a.place(DocRef::local(slide_d), "Slide", t(30.0, -8.0, 30.0), false);
    let arm2 = a.place(DocRef::local(arm2_d), "Arm2", t(54.0, -6.0, 38.0), false);
    joint(
        &mut a,
        ConstraintKind::Rotational,
        frame(base, &b[0]),
        frame(arm1, &a1[0]),
    );
    joint(
        &mut a,
        ConstraintKind::Slider,
        frame(arm1, &a1[1]),
        frame(slide, &s[0]),
    );
    joint(
        &mut a,
        ConstraintKind::Rotational,
        frame(slide, &s[1]),
        frame(arm2, &a2[0]),
    );
    let doc = project.add_document("Arm", DocumentData::Assembly(a));
    Ok(ArmIds {
        doc,
        base,
        arm1,
        slide,
        arm2,
    })
}

/// [`delta`]'s sizes: hinge radius on the base, arm length, rod length,
/// radius of the rods' Balls on the effector.
pub const DELTA: (Real, Real, Real, Real) = (60.0, 60.0, 120.0, 20.0);

/// Ids of [`delta`]'s occurrences.
#[derive(Clone, Debug)]
pub struct DeltaIds {
    /// The assembly document.
    pub doc: DocumentId,
    /// The grounded base plate.
    pub base: u32,
    /// The arms (hinged on the base at 0°, 120°, 240°).
    pub arms: [u32; 3],
    /// The rods.
    pub rods: [u32; 3],
    /// The effector plate.
    pub effector: u32,
}

/// The effector plate's top centre (part coordinates).
pub const EFFECTOR_CENTRE: Point3 = Point3::new(25.0, 25.0, 6.0);

/// Adds "Delta" to `project` (sizes [`DELTA`]): the base plate's underside
/// at z 0 centred on the origin, the arms level and pointing out, the
/// effector hanging below, every joint met.
pub fn delta(project: &mut Project) -> Result<DeltaIds, String> {
    let (r_base, l_arm, l_rod, r_eff) = DELTA;
    let angles = [0.0, 120.0, 240.0].map(|d: Real| d.to_radians());
    let hinges: Vec<At> = angles
        .iter()
        .map(|&f| {
            (
                [80.0 + r_base * f.cos(), 80.0 + r_base * f.sin(), 0.0],
                // z along the hinge: the circle's tangent.
                [-HALF_PI, 0.0, f],
            )
        })
        .collect();
    let (base, b) = block([160.0, 160.0, 10.0], &hinges)?;
    let (arm, a) = block(
        [l_arm, 10.0, 10.0],
        &[
            ([0.0, 5.0, 5.0], [-HALF_PI, 0.0, 0.0]),
            ([l_arm, 5.0, 5.0], [0.0; 3]),
        ],
    )?;
    let (rod, r) = block(
        [6.0, 6.0, l_rod],
        &[([3.0, 3.0, l_rod], [0.0; 3]), ([3.0, 3.0, 0.0], [0.0; 3])],
    )?;
    let balls: Vec<At> = angles
        .iter()
        .map(|&f| {
            (
                [25.0 + r_eff * f.cos(), 25.0 + r_eff * f.sin(), 6.0],
                [0.0; 3],
            )
        })
        .collect();
    let (eff, e) = block([50.0, 50.0, 6.0], &balls)?;
    let base_d = project.add_document("DeltaBase", DocumentData::Part(base));
    let arm_d = project.add_document("DeltaArm", DocumentData::Part(arm));
    let rod_d = project.add_document("DeltaRod", DocumentData::Part(rod));
    let eff_d = project.add_document("Effector", DocumentData::Part(eff));
    let mut asm = AssemblyDocument::default();
    let base = asm.place(
        DocRef::local(base_d),
        "DeltaBase",
        Transform3::translation(-80.0, -80.0, 0.0),
        true,
    );
    let reach = r_base + l_arm - r_eff;
    let drop = (l_rod * l_rod - reach * reach).sqrt();
    let effector = asm.place(
        DocRef::local(eff_d),
        "Effector",
        Transform3::translation(-25.0, -25.0, -drop - 6.0),
        false,
    );
    let mut arms = [0; 3];
    let mut rods = [0; 3];
    for (k, &f) in angles.iter().enumerate() {
        let (c, s) = (f.cos(), f.sin());
        // Arm: its X out along the radius, the hinge (0, 5, 5) on the base's.
        let turn = Transform3::new(Vector3::zeros(), Vector3::new(0.0, 0.0, f));
        let hinge = Point3::new(r_base * c, r_base * s, 0.0);
        let shift = hinge - turn * Point3::new(0.0, 5.0, 5.0);
        arms[k] = asm.place(
            DocRef::local(arm_d),
            "DeltaArm",
            Transform3::from_parts(shift.into(), turn.rotation),
            false,
        );
        // Rod: bottom on the effector's Ball, top on the arm's tip.
        let tip = Point3::new((r_base + l_arm) * c, (r_base + l_arm) * s, 0.0);
        let foot = Point3::new(r_eff * c, r_eff * s, -drop);
        let up = nalgebra::UnitQuaternion::rotation_between(&Vector3::z(), &(tip - foot))
            .unwrap_or_else(nalgebra::UnitQuaternion::identity);
        let shift = foot - up * Point3::new(3.0, 3.0, 0.0);
        rods[k] = asm.place(
            DocRef::local(rod_d),
            "DeltaRod",
            Transform3::from_parts(shift.into(), up),
            false,
        );
        joint(
            &mut asm,
            ConstraintKind::Rotational,
            frame(base, &b[k]),
            frame(arms[k], &a[0]),
        );
        joint(
            &mut asm,
            ConstraintKind::Ball,
            frame(arms[k], &a[1]),
            frame(rods[k], &r[0]),
        );
        joint(
            &mut asm,
            ConstraintKind::Ball,
            frame(rods[k], &r[1]),
            frame(effector, &e[k]),
        );
    }
    let doc = project.add_document("Delta", DocumentData::Assembly(asm));
    Ok(DeltaIds {
        doc,
        base,
        arms,
        rods,
        effector,
    })
}

/// Solves `asm` (an assembly of `project`, or a copy of one) as the app
/// does: references through the tree, flexible occurrences' components
/// moving, `drag` pulling one point.
pub fn solve_in(project: &Project, asm: &AssemblyDocument, drag: Option<&Drag>) -> Solved {
    let linked = tree::Linked::new();
    let local = |r: &GeomRef| -> Result<Geom, String> {
        let n = tree::nested(project, &linked, asm, r.occurrence(), r.sub())?;
        let part = tree::part_of(project, &linked, n.component).ok_or("not a part")?;
        let g = relation::resolve_in(part, r)?;
        Ok(if r.sub().is_empty() {
            g
        } else {
            g.moved(&n.at)
        })
    };
    let sub_of = |o: &Occurrence| {
        tree::assembly_of(project, &linked, o.component)
            .map(|s| tree::rebased(s, o.component.library))
    };
    flexible::solve(asm, &local, drag, &sub_of)
}

/// Where component `sub` of occurrence `top` stands (assembly coordinates).
pub fn placement(
    project: &Project,
    asm: &AssemblyDocument,
    top: u32,
    sub: &[u32],
) -> Option<Transform3> {
    let linked = tree::Linked::new();
    let n = tree::nested(project, &linked, asm, top, sub).ok()?;
    Some(asm.find(top)?.transform * n.at)
}
