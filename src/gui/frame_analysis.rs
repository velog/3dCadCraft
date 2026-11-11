//! The Frame Analysis environment (`docs/spec/7.1a-frame-analysis.md`):
//! its ribbon commands, browser, panels, picks on the beams and the results
//! in the view. The model and the runs are `assembly::frame_analysis`; here
//! is only what the user sees. A simulation lives in the frame's assembly
//! document; its model is converted again whenever it changes.

use super::CadCraftApp;
use super::icons::Icon;
use super::valuefield::ExprValue;
use crate::analysis::beam::{BeamSection, Station};
use crate::assembly::frame_analysis::{
    At, BeamOverride, Converted, FaConstraint, FaLink, FaLoad, FaLoadKind, FaRelease, Hold,
    ModalResult, SimKind, Simulation, StaticResult, convert, largest, modes_of, static_of,
};
use crate::command::flow::{Flow, PickKind, colors};
use crate::document::DocumentData;
use crate::kernel::{Point3, Real, Vector3};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};
use std::sync::mpsc;

/// A command of the environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaCmd {
    /// Environments › Frame Analysis.
    Begin,
    /// Create Simulation.
    Create,
    /// Simulate.
    Simulate,
    /// Beam Properties and Materials.
    Beam,
    /// A constraint.
    Fixed,
    /// A pin.
    Pinned,
    /// A floating pin.
    Floating,
    /// Chosen degrees of freedom.
    Custom,
    /// A force.
    Force,
    /// A uniform load.
    Continuous,
    /// A moment.
    Moment,
    /// A torque about the beam.
    Axial,
    /// A moment in a plane of the beam.
    Bending,
    /// Weight.
    Gravity,
    /// A custom node.
    Node,
    /// A release.
    Release,
    /// A rigid link.
    Link,
    /// Beam Detail.
    Detail,
    /// Animate.
    Animate,
    /// Finish Frame Analysis.
    Finish,
}

/// What a panel defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Hold(HoldKind),
    Force,
    Continuous,
    Moment,
    Axial,
    Bending,
    Gravity,
    Node,
    Release,
    Link,
    Beam,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoldKind {
    Fixed,
    Pinned,
    Floating,
    Custom,
}

/// An open panel.
#[derive(Clone, Debug)]
struct Tool {
    kind: Kind,
    at: Option<At>,
    /// Magnitude or value.
    value: Real,
    /// Angle of plane, degrees (or the bending plane).
    plane: Real,
    /// Angle in plane, degrees.
    in_plane: Real,
    /// By components.
    components: bool,
    /// X, Y, Z.
    v: [Real; 3],
    /// Continuous load: only a stretch.
    custom_size: bool,
    /// The stretch's offset.
    from: Real,
    /// The stretch's length.
    length: Real,
    /// An axis (0 X, 1 Y, 2 Z).
    axis: usize,
    /// Against the axis (Gravity).
    negative: bool,
    /// Custom constraint or start release.
    a: [bool; 6],
    /// End release.
    b: [bool; 6],
    /// Rigid link's children.
    children: Vec<At>,
    /// Beam numbers (E, G, ρ, A, Ix, Iy, J; 0 = as the frame).
    numbers: [Real; 7],
    /// Rigid Link: Back to the parent, the next click replaces it.
    repick: bool,
}

impl Tool {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            at: None,
            value: 1000.0,
            plane: 0.0,
            in_plane: 180.0,
            components: false,
            v: [0.0; 3],
            custom_size: false,
            from: 0.0,
            length: 0.0,
            axis: 2,
            negative: true,
            a: [false; 6],
            b: [false; 6],
            children: Vec::new(),
            numbers: [0.0; 7],
            repick: false,
        }
    }

    /// The panel's selectors for Next and Back (rule 21); its values follow
    /// them in the editor next to the cursor.
    fn flow(&self) -> Flow {
        const PLACE: &[PickKind] = &[PickKind::Edge, PickKind::Point];
        let held = usize::from(self.at.is_some());
        let (mut selectors, mut active) = (Vec::new(), None);
        match self.kind {
            Kind::Gravity => {}
            Kind::Link => {
                selectors.push(super::steps::input(
                    "parent",
                    "Parent",
                    PLACE,
                    true,
                    true,
                    held,
                    colors::DIRECTION,
                ));
                selectors.push(super::steps::input(
                    "children",
                    "Children",
                    PLACE,
                    false,
                    true,
                    self.children.len(),
                    colors::GUIDES,
                ));
                active = Some(if held == 1 && !self.repick {
                    "children"
                } else {
                    "parent"
                });
            }
            Kind::Beam | Kind::Release | Kind::Continuous => selectors.push(super::steps::input(
                "place",
                "Beam",
                PLACE,
                true,
                true,
                held,
                colors::EDGES,
            )),
            _ => selectors.push(super::steps::input(
                "place",
                "Location",
                PLACE,
                true,
                true,
                held,
                colors::EDGES,
            )),
        }
        Flow::new(selectors, active)
    }

    fn title(&self) -> &'static str {
        match self.kind {
            Kind::Hold(HoldKind::Fixed) => "Fixed Constraint",
            Kind::Hold(HoldKind::Pinned) => "Pinned Constraint",
            Kind::Hold(HoldKind::Floating) => "Floating Pinned Constraint",
            Kind::Hold(HoldKind::Custom) => "Custom Constraint",
            Kind::Force => "Force",
            Kind::Continuous => "Continuous Load",
            Kind::Moment => "Moment",
            Kind::Axial => "Axial Moment",
            Kind::Bending => "Bending Moment",
            Kind::Gravity => "Gravity",
            Kind::Node => "Custom Node",
            Kind::Release => "Release",
            Kind::Link => "Rigid Link",
            Kind::Beam => "Beam Properties",
        }
    }

    fn icon(&self) -> Icon {
        match self.kind {
            Kind::Hold(HoldKind::Fixed) => Icon::FaFixed,
            Kind::Hold(HoldKind::Pinned) => Icon::FaPinned,
            Kind::Hold(HoldKind::Floating) => Icon::FaFloating,
            Kind::Hold(HoldKind::Custom) => Icon::FaCustom,
            Kind::Force => Icon::FaForce,
            Kind::Continuous => Icon::FaContinuous,
            Kind::Moment => Icon::FaMoment,
            Kind::Axial => Icon::FaAxial,
            Kind::Bending => Icon::FaBending,
            Kind::Gravity => Icon::FaGravity,
            Kind::Node => Icon::FaNode,
            Kind::Release => Icon::FaRelease,
            Kind::Link => Icon::FaLink,
            Kind::Beam => Icon::FaBeam,
        }
    }

    /// The direction of Force from its two angles, or its components'.
    fn force(&self) -> [Real; 3] {
        if self.components {
            return self.v;
        }
        let (p, a) = (self.plane.to_radians(), self.in_plane.to_radians());
        [
            self.value * a.sin() * p.cos(),
            self.value * a.sin() * p.sin(),
            self.value * a.cos(),
        ]
    }

    /// What this panel still needs, for its prompt.
    fn prompt(&self) -> &'static str {
        match self.kind {
            Kind::Gravity => "Gravity: set the direction and press OK",
            Kind::Link if self.at.is_some() && !self.repick => {
                "Rigid Link: click the child points, then OK"
            }
            Kind::Link => "Rigid Link: click the parent point",
            Kind::Beam | Kind::Release | Kind::Continuous => "Click a beam",
            _ => "Click a node, or a beam at the place wanted",
        }
    }

    fn ready(&self) -> bool {
        match self.kind {
            Kind::Gravity => true,
            Kind::Link => self.at.is_some() && !self.children.is_empty(),
            _ => self.at.is_some(),
        }
    }
}

/// Results a run left.
#[derive(Clone, Debug, PartialEq)]
enum Run {
    Static(Box<StaticResult>),
    Modal(Box<ModalResult>),
}

/// What the view colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Show {
    Displacement,
    Fx,
    Fy,
    Fz,
    Mx,
    My,
    Mz,
    Smax,
    Smin,
    SmaxMx,
    SminMx,
    SmaxMy,
    SminMy,
    Saxial,
    Tx,
    Ty,
    T,
}

impl Show {
    const LOADS: [(Show, &'static str); 3] = [(Show::Fx, "Fx"), (Show::Fy, "Fy"), (Show::Fz, "Fz")];
    const MOMENTS: [(Show, &'static str); 3] =
        [(Show::Mx, "Mx"), (Show::My, "My"), (Show::Mz, "Mz")];
    const NORMAL: [(Show, &'static str); 7] = [
        (Show::Smax, "Smax"),
        (Show::Smin, "Smin"),
        (Show::SmaxMx, "Smax(Mx)"),
        (Show::SminMx, "Smin(Mx)"),
        (Show::SmaxMy, "Smax(My)"),
        (Show::SminMy, "Smin(My)"),
        (Show::Saxial, "Saxial"),
    ];
    const SHEAR: [(Show, &'static str); 2] = [(Show::Tx, "Tx"), (Show::Ty, "Ty")];

    fn label(self) -> &'static str {
        for (s, l) in Self::LOADS
            .iter()
            .chain(&Self::MOMENTS)
            .chain(&Self::NORMAL)
            .chain(&Self::SHEAR)
        {
            if *s == self {
                return l;
            }
        }
        match self {
            Show::T => "T",
            _ => "Displacement",
        }
    }

    /// The value of a station.
    fn of(self, s: &Station, c: &BeamSection) -> Real {
        match self {
            Show::Displacement => {
                let d = s.displacement;
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            }
            Show::Fx => s.fu,
            Show::Fy => s.fv,
            Show::Fz => s.n,
            Show::Mx => s.mu / 1000.0,
            Show::My => s.mv / 1000.0,
            Show::Mz => s.t / 1000.0,
            Show::Smax => s.s_max(c),
            Show::Smin => s.s_min(c),
            Show::SmaxMx => s.s_mu(c),
            Show::SminMx => -s.s_mu(c),
            Show::SmaxMy => s.s_mv(c),
            Show::SminMy => -s.s_mv(c),
            Show::Saxial => s.s_axial(c),
            Show::Tx => s.t_u(c),
            Show::Ty => s.t_v(c),
            Show::T => s.t_t(c),
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Show::Displacement => "mm",
            Show::Fx | Show::Fy | Show::Fz => "N",
            Show::Mx | Show::My | Show::Mz => "N m",
            _ => "MPa",
        }
    }
}

/// A run on its worker thread: dropped when the simulation changes, and the
/// thread's answer goes nowhere.
struct Job {
    of: Simulation,
    answer: mpsc::Receiver<Result<Run, String>>,
}

/// The environment's state (never saved; the simulations are).
pub(super) struct FaUi {
    /// The environment is open.
    pub active: bool,
    sim: usize,
    tool: Option<Tool>,
    /// The model of the active simulation, and the simulation it is of.
    model: Option<Converted>,
    model_of: Option<Simulation>,
    run: Option<Run>,
    /// The run being solved.
    job: Option<Job>,
    show: Show,
    mode: usize,
    /// Deformation scale.
    scale: Real,
    animate: bool,
    /// The member Beam Detail shows and Properties speaks about.
    selected: Option<u32>,
    detail: bool,
    /// Why the last conversion or run failed.
    status: Option<String>,
    /// Create Simulation's dialog.
    create: Option<(String, usize, usize)>,
}

impl Default for FaUi {
    fn default() -> Self {
        Self {
            active: false,
            sim: 0,
            tool: None,
            model: None,
            model_of: None,
            run: None,
            job: None,
            show: Show::Displacement,
            mode: 0,
            scale: 0.0,
            animate: false,
            selected: None,
            detail: false,
            status: None,
            create: None,
        }
    }
}

const BEAM: Color32 = Color32::from_rgb(60, 110, 200);
const NODE: Color32 = Color32::from_rgb(20, 30, 60);
const HOLD: Color32 = Color32::from_rgb(30, 150, 70);
const LOAD: Color32 = Color32::from_rgb(220, 60, 40);
const LINK: Color32 = Color32::from_rgb(170, 90, 200);
const HOVER: Color32 = Color32::from_rgb(255, 150, 0);

/// Blue → green → red.
fn colour(t: Real) -> Color32 {
    let t = t.clamp(0.0, 1.0) as f32;
    let (r, g, b) = if t < 0.25 {
        (0.0, t * 4.0, 1.0)
    } else if t < 0.5 {
        (0.0, 1.0, 1.0 - (t - 0.25) * 4.0)
    } else if t < 0.75 {
        ((t - 0.5) * 4.0, 1.0, 0.0)
    } else {
        (1.0, 1.0 - (t - 0.75) * 4.0, 0.0)
    };
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

impl CadCraftApp {
    /// The active simulation of the frame's document.
    fn fa_sim(&self) -> Option<&Simulation> {
        self.frame_asm()?.simulations.get(self.fa.sim)
    }

    /// Changes the active simulation as one Undo step.
    fn fa_edit(&mut self, label: &str, f: impl FnOnce(&mut Simulation)) {
        self.checkpoint(label);
        let doc = self.frame_doc();
        if let Some(DocumentData::Assembly(a)) = self.project.document_mut(doc).map(|d| &mut d.data)
            && let Some(s) = a.simulations.get_mut(self.fa.sim)
        {
            f(s);
        }
        self.fa_refresh();
    }

    /// Converts the model again when the simulation changed (after an Undo
    /// too); old results go with it.
    fn fa_refresh(&mut self) {
        let now = self.fa_sim().cloned();
        if now == self.fa.model_of && self.fa.model.is_some() {
            return;
        }
        self.fa.run = None;
        self.fa.model = None;
        self.fa.model_of = now.clone();
        let Some(sim) = now else {
            return;
        };
        let linked = self.linked();
        let Some(a) = self.frame_asm() else {
            return;
        };
        match convert(&self.project, &linked, a, &sim) {
            Ok(m) => {
                self.fa.status = None;
                self.fa.model = Some(m);
            }
            Err(e) => self.fa.status = Some(e),
        }
    }

    /// A ribbon command of the environment.
    pub(super) fn fa_command(&mut self, k: FaCmd) {
        match k {
            FaCmd::Begin => return self.fa_begin(),
            FaCmd::Finish => {
                self.fa = FaUi::default();
                self.scene_dirty = true;
                self.tab = super::ribbon::Tab::Environments;
                return;
            }
            FaCmd::Create => {
                let n = self.frame_asm().map_or(0, |a| a.simulations.len()) + 1;
                self.fa.create = Some((format!("Simulation:{n}"), 0, 8));
                return;
            }
            FaCmd::Simulate => return self.fa_simulate(),
            FaCmd::Detail => {
                self.fa.detail = true;
                return;
            }
            FaCmd::Animate => {
                self.fa.animate = !self.fa.animate;
                return;
            }
            _ => {}
        }
        let kind = match k {
            FaCmd::Fixed => Kind::Hold(HoldKind::Fixed),
            FaCmd::Pinned => Kind::Hold(HoldKind::Pinned),
            FaCmd::Floating => Kind::Hold(HoldKind::Floating),
            FaCmd::Custom => Kind::Hold(HoldKind::Custom),
            FaCmd::Force => Kind::Force,
            FaCmd::Continuous => Kind::Continuous,
            FaCmd::Moment => Kind::Moment,
            FaCmd::Axial => Kind::Axial,
            FaCmd::Bending => Kind::Bending,
            FaCmd::Gravity => Kind::Gravity,
            FaCmd::Node => Kind::Node,
            FaCmd::Release => Kind::Release,
            FaCmd::Link => Kind::Link,
            _ => Kind::Beam,
        };
        let mut t = Tool::new(kind);
        if kind == Kind::Moment || kind == Kind::Axial || kind == Kind::Bending {
            t.value = 100.0;
        }
        if kind == Kind::Gravity {
            t.value = 9806.65;
        }
        if kind == Kind::Continuous {
            t.v = [0.0, 0.0, -1.0];
        }
        self.fa.tool = Some(t);
    }

    /// Opens the environment: one simulation at least.
    fn fa_begin(&mut self) {
        if self.frame_ctx().is_none() {
            self.file_status = Some(Err("Frame Analysis: the assembly has no frame".into()));
            return;
        }
        let empty = self.frame_asm().is_some_and(|a| a.simulations.is_empty());
        if empty {
            let doc = self.frame_doc();
            self.checkpoint("Create Simulation");
            if let Some(DocumentData::Assembly(a)) =
                self.project.document_mut(doc).map(|d| &mut d.data)
            {
                a.simulations
                    .push(Simulation::new("Simulation:1", SimKind::Static));
            }
        }
        self.fa = FaUi {
            active: true,
            ..Default::default()
        };
        self.fa_refresh();
        self.tab = super::ribbon::Tab::FrameAnalysis;
        self.scene_dirty = true;
        if let Some(e) = self.fa.status.clone() {
            self.file_status = Some(Err(format!("Frame Analysis: {e}")));
        }
    }

    /// Simulate: refused without constraints, loads or when it can move;
    /// the model is never changed.
    fn fa_simulate(&mut self) {
        if self.fa.job.is_some() {
            return;
        }
        self.fa_refresh();
        let Some(sim) = self.fa.model_of.clone() else {
            return;
        };
        let Some(model) = self.fa.model.clone() else {
            let e = self.fa.status.clone().unwrap_or_default();
            self.file_status = Some(Err(format!("Simulate: {e}")));
            return;
        };
        // A large modal run takes seconds (rule 20): the frame goes on.
        let (send, answer) = mpsc::channel();
        let of = sim.clone();
        std::thread::spawn(move || {
            let r = match sim.kind {
                SimKind::Static => static_of(model, &sim).map(|r| Run::Static(Box::new(r))),
                SimKind::Modal { count } => {
                    modes_of(model, count.max(1) as usize).map(|r| Run::Modal(Box::new(r)))
                }
            };
            let _ = send.send(r);
        });
        self.fa.job = Some(Job { of, answer });
    }

    /// Takes the answer of the run being solved, if it has come.
    fn fa_poll(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.fa.job else {
            return;
        };
        if self.fa.model_of.as_ref() != Some(&job.of) {
            self.fa.job = None;
            return;
        }
        let r = match job.answer.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => Err("the solver stopped".to_owned()),
        };
        self.fa.job = None;
        match r {
            Ok(run) => {
                self.fa.scale = 0.0;
                self.fa.mode = 0;
                self.file_status = Some(Ok("Simulate: done".into()));
                self.fa.status = None;
                self.fa.run = Some(run);
            }
            Err(e) => {
                self.fa.run = None;
                self.fa.status = Some(e.clone());
                self.file_status = Some(Err(format!("Simulate: {e}")));
            }
        }
    }

    fn fa_project(&self, p: &Point3, rect: Rect) -> Option<Pos2> {
        self.camera
            .project(&self.frame_place().transform_point(p), rect)
    }

    /// The place under the cursor: a node within the snap distance, else
    /// the beam's nearest point.
    fn fa_pick(&self, p: Pos2, rect: Rect) -> Option<At> {
        let m = self.fa.model.as_ref()?;
        let mut best: Option<(f32, At)> = None;
        for l in &m.lines {
            let (Some(a), Some(b)) = (
                self.fa_project(&l.start, rect),
                self.fa_project(&l.end, rect),
            ) else {
                continue;
            };
            let d = b - a;
            let t = ((p - a).dot(d) / d.length_sq().max(1e-6)).clamp(0.0, 1.0);
            let dist = (a + d * t).distance(p);
            if dist > super::overlay::CURVE_SNAP_PX * 1.5 {
                continue;
            }
            if best.as_ref().is_none_or(|x| dist < x.0) {
                let mut s = t as Real * l.length();
                // Snap to a node of this beam.
                let mut near = NODE_SNAP_PX;
                for n in &m.model.nodes {
                    let q = Point3::from(*n);
                    let on = (q - l.start).dot(&(l.end - l.start).normalize());
                    let foot = l.start + (l.end - l.start).normalize() * on;
                    if (foot - q).norm() > 1e-3 || on < -1e-3 || on > l.length() + 1e-3 {
                        continue;
                    }
                    if let Some(w) = self.fa_project(&q, rect)
                        && w.distance(p) < near
                    {
                        near = w.distance(p);
                        s = on.clamp(0.0, l.length());
                    }
                }
                best = Some((
                    dist,
                    At {
                        member: l.member,
                        s,
                    },
                ));
            }
        }
        best.map(|b| b.1)
    }

    /// The view while the environment is open.
    pub(super) fn fa_viewport(
        &mut self,
        ui: &mut egui::Ui,
        frame: &eframe::Frame,
        rect: Rect,
        response: &egui::Response,
        pointer: Option<Pos2>,
        on_cube: bool,
    ) {
        self.fa_refresh();
        self.fa_poll(ui.ctx());
        let painter = ui.painter_at(rect);
        self.render_view(ui, frame, rect, &painter);
        let hover = pointer
            .filter(|_| !on_cube)
            .and_then(|p| self.fa_pick(p, rect));
        if response.clicked_by(egui::PointerButton::Primary) && !on_cube {
            self.fa_click(hover);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && self.fa.tool.is_some() {
            self.fa.tool = None;
        }
        if self.fa.tool.is_some() {
            response.context_menu(|ui| {
                self.step_menu(ui);
                ui.separator();
                self.ok_cancel_menu(ui);
            });
        }
        self.fa_draw(&painter, rect, hover);
        self.draw_cursor_value(ui.ctx(), &painter, rect, pointer);
        if self.fa.animate {
            ui.ctx().request_repaint();
        }
        self.fa_windows(ui.ctx());
    }

    /// The status bar's prompt.
    pub(super) fn fa_prompt(&self) -> String {
        if self.fa.job.is_some() && self.fa.tool.is_none() {
            return "Simulate: solving...".to_owned();
        }
        self.fa.tool.as_ref().map_or_else(
            || "Frame Analysis: add constraints and loads, then Simulate".to_owned(),
            |t| t.prompt().to_owned(),
        )
    }

    /// Is a panel of the environment open?
    pub(super) fn fa_tool_open(&self) -> bool {
        self.fa.tool.is_some()
    }

    /// The open panel's inputs, for Next and Back.
    pub(super) fn fa_flow(&self) -> Option<Flow> {
        self.fa.tool.as_ref().map(Tool::flow)
    }

    /// Makes an input of the open panel active; false when none is open.
    pub(super) fn fa_step_activate(&mut self, key: &str) -> bool {
        let Some(t) = self.fa.tool.as_mut() else {
            return false;
        };
        t.repick = key == "parent";
        true
    }

    fn fa_click(&mut self, hit: Option<At>) {
        let Some(t) = self.fa.tool.as_mut() else {
            self.fa.selected = hit.map(|a| a.member);
            return;
        };
        let Some(at) = hit else {
            return;
        };
        if t.kind == Kind::Link && t.at.is_some() && !t.repick {
            if t.at != Some(at) && !t.children.contains(&at) {
                t.children.push(at);
            }
            return;
        }
        if t.kind == Kind::Continuous && t.length == 0.0 {
            t.length = self
                .fa
                .model
                .as_ref()
                .and_then(|m| m.line_of(at.member).map(|l| m.lines[l].length()))
                .unwrap_or(0.0);
        }
        t.at = Some(at);
        t.repick = false;
    }

    fn fa_draw(&self, painter: &egui::Painter, rect: Rect, hover: Option<At>) {
        let Some(m) = self.fa.model.as_ref() else {
            return;
        };
        let at = |a: &At| -> Option<Point3> { m.line_of(a.member).map(|l| m.lines[l].at(a.s)) };
        if self.fa.run.is_none() {
            for l in &m.lines {
                if let (Some(a), Some(b)) = (
                    self.fa_project(&l.start, rect),
                    self.fa_project(&l.end, rect),
                ) {
                    let w = if self.fa.selected == Some(l.member) {
                        4.0
                    } else {
                        2.5
                    };
                    painter.line_segment([a, b], Stroke::new(w, BEAM));
                }
            }
        } else {
            self.fa_draw_results(painter, rect, m);
        }
        for n in &m.model.nodes {
            if let Some(q) = self.fa_project(&Point3::from(*n), rect) {
                painter.circle_filled(q, 2.5, NODE);
            }
        }
        for l in &m.model.links {
            let (a, b) = (m.model.nodes[l.parent], m.model.nodes[l.child]);
            if let (Some(a), Some(b)) = (
                self.fa_project(&Point3::from(a), rect),
                self.fa_project(&Point3::from(b), rect),
            ) {
                painter.line_segment([a, b], Stroke::new(2.0, LINK));
            }
        }
        let Some(sim) = self.fa_sim() else {
            return;
        };
        for c in sim.constraints.iter().filter(|c| !c.suppressed) {
            if let Some(q) = at(&c.at).and_then(|p| self.fa_project(&p, rect)) {
                self.fa_hold_glyph(painter, q, c.hold);
            }
        }
        for l in sim.loads.iter().filter(|l| !l.suppressed) {
            self.fa_load_glyph(painter, rect, m, &l.kind);
        }
        if let Some(a) = hover
            && let Some(q) = at(&a).and_then(|p| self.fa_project(&p, rect))
        {
            painter.circle_stroke(q, 6.0, Stroke::new(2.0, HOVER));
        }
        if let Some(t) = &self.fa.tool {
            for a in t.at.iter().chain(&t.children) {
                if let Some(q) = at(a).and_then(|p| self.fa_project(&p, rect)) {
                    painter.circle_filled(q, 5.0, HOVER);
                }
            }
        }
    }

    fn fa_hold_glyph(&self, painter: &egui::Painter, q: Pos2, h: Hold) {
        let s = 9.0;
        let stroke = Stroke::new(2.0, HOLD);
        match h {
            Hold::Fixed => {
                painter.rect_stroke(
                    Rect::from_center_size(q, egui::vec2(s * 1.6, s * 1.6)),
                    0.0,
                    stroke,
                    egui::StrokeKind::Middle,
                );
                painter.line_segment([q + egui::vec2(-s, s), q + egui::vec2(s, -s)], stroke);
            }
            Hold::Pinned => {
                painter.add(egui::Shape::closed_line(
                    vec![q, q + egui::vec2(-s, s * 1.6), q + egui::vec2(s, s * 1.6)],
                    stroke,
                ));
            }
            Hold::Floating { .. } => {
                painter.add(egui::Shape::closed_line(
                    vec![q, q + egui::vec2(-s, s * 1.3), q + egui::vec2(s, s * 1.3)],
                    stroke,
                ));
                painter.line_segment(
                    [q + egui::vec2(-s, s * 1.9), q + egui::vec2(s, s * 1.9)],
                    stroke,
                );
            }
            Hold::Custom { .. } => {
                painter.circle_stroke(q, s * 0.8, stroke);
            }
        }
    }

    fn fa_arrow(&self, painter: &egui::Painter, rect: Rect, tip: Point3, dir: Vector3, size: Real) {
        let Some(d) = dir.try_normalize(1e-12) else {
            return;
        };
        let tail = tip - d * size;
        if let (Some(a), Some(b)) = (self.fa_project(&tail, rect), self.fa_project(&tip, rect)) {
            let stroke = Stroke::new(2.0, LOAD);
            painter.line_segment([a, b], stroke);
            let n = (b - a).normalized();
            if (b - a).length() > 1.0 {
                let w = n.rot90() * 4.0;
                painter.line_segment([b, b - n * 9.0 + w], stroke);
                painter.line_segment([b, b - n * 9.0 - w], stroke);
            }
        }
    }

    fn fa_load_glyph(&self, painter: &egui::Painter, rect: Rect, m: &Converted, k: &FaLoadKind) {
        let size = m
            .lines
            .iter()
            .map(|l| l.length())
            .fold(0.0, Real::max)
            .max(10.0)
            * 0.1;
        let at = |a: &At| m.line_of(a.member).map(|l| m.lines[l].at(a.s));
        let v = |a: &[Real; 3]| Vector3::new(a[0], a[1], a[2]);
        match k {
            FaLoadKind::Force { at: a, f } => {
                if let Some(p) = at(a) {
                    self.fa_arrow(painter, rect, p, v(f), size);
                }
            }
            FaLoadKind::Moment { at: a, .. }
            | FaLoadKind::AxialMoment { at: a, .. }
            | FaLoadKind::BendingMoment { at: a, .. } => {
                if let Some(q) = at(a).and_then(|p| self.fa_project(&p, rect)) {
                    painter.add(egui::Shape::circle_stroke(q, 10.0, Stroke::new(2.0, LOAD)));
                    painter.circle_filled(q + egui::vec2(10.0, 0.0), 3.0, LOAD);
                }
            }
            FaLoadKind::Continuous {
                member,
                from,
                to,
                q,
            } => {
                let Some(l) = m.line_of(*member).map(|i| &m.lines[i]) else {
                    return;
                };
                let n = 6;
                for i in 0..=n {
                    let s = from + (to - from) * i as Real / n as Real;
                    self.fa_arrow(painter, rect, l.at(s), v(q), size * 0.7);
                }
            }
            FaLoadKind::Gravity { g } => {
                if let Some(l) = m.lines.first() {
                    self.fa_arrow(painter, rect, l.at(l.length() / 2.0), v(g), size * 1.5);
                }
            }
        }
    }

    fn fa_draw_results(&self, painter: &egui::Painter, rect: Rect, m: &Converted) {
        let wave = if self.fa.animate {
            (painter.ctx().input(|i| i.time) * 3.0).sin()
        } else {
            1.0
        };
        let size = m
            .lines
            .iter()
            .flat_map(|l| [l.start.coords.norm(), l.end.coords.norm()])
            .fold(0.0, Real::max)
            .max(1.0)
            .max(m.lines.iter().map(|l| l.length()).fold(0.0, Real::max));
        match self.fa.run.as_ref() {
            Some(Run::Static(r)) => {
                let big = largest(&r.members, |_, s| Show::Displacement.of(s, &BLANK));
                let scale = self.deform_scale(size, big) * wave;
                let c = self.show_range(r);
                for mem in &r.members {
                    let l = &m.lines[mem.line];
                    let pts: Vec<(Option<Pos2>, Real)> = mem
                        .stations
                        .iter()
                        .map(|s| {
                            let d = s.displacement;
                            let p = l.at(s.s) + Vector3::new(d[0], d[1], d[2]) * scale;
                            (self.fa_project(&p, rect), self.fa.show.of(s, &l.section))
                        })
                        .collect();
                    for w in pts.windows(2) {
                        if let (Some(a), Some(b)) = (w[0].0, w[1].0) {
                            let t = (w[0].1 + w[1].1) / 2.0;
                            let width = if self.fa.selected == Some(l.member) {
                                7.0
                            } else {
                                5.0
                            };
                            painter.line_segment(
                                [a, b],
                                Stroke::new(width, colour((t - c.0) / (c.1 - c.0).max(1e-12))),
                            );
                        }
                    }
                }
                self.fa_legend(painter, rect, c, self.fa.show.unit());
            }
            Some(Run::Modal(r)) => {
                let Some(mode) = r.modes.get(self.fa.mode) else {
                    return;
                };
                let scale = size * 0.1 * wave;
                for b in &m.model.beams {
                    let p = |i: usize| {
                        let u = mode.shape[i];
                        Point3::from(m.model.nodes[i]) + Vector3::new(u[0], u[1], u[2]) * scale
                    };
                    if let (Some(a), Some(c)) = (
                        self.fa_project(&p(b.start), rect),
                        self.fa_project(&p(b.end), rect),
                    ) {
                        painter.line_segment([a, c], Stroke::new(4.0, HOVER));
                    }
                }
            }
            None => {}
        }
    }

    /// The range of the shown value over all members.
    fn show_range(&self, r: &StaticResult) -> (Real, Real) {
        let mut lo = Real::INFINITY;
        let mut hi = Real::NEG_INFINITY;
        for m in &r.members {
            let c = r.model.lines[m.line].section;
            for s in &m.stations {
                let v = self.fa.show.of(s, &c);
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        (lo.min(hi), hi)
    }

    /// Screen scale of the deformation: the user's, else 5 % of the model.
    fn deform_scale(&self, size: Real, big: Real) -> Real {
        if self.fa.scale > 0.0 {
            self.fa.scale
        } else if big > 1e-12 {
            0.05 * size / big
        } else {
            1.0
        }
    }

    fn fa_legend(&self, painter: &egui::Painter, rect: Rect, c: (Real, Real), unit: &str) {
        let x = rect.left() + 18.0;
        let top = rect.top() + 40.0;
        let h = 200.0;
        let w = 16.0;
        let n = 40;
        for i in 0..n {
            let t = 1.0 - i as Real / n as Real;
            let y = top + h * i as f32 / n as f32;
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x, y), egui::vec2(w, h / n as f32 + 1.0)),
                0.0,
                colour(t),
            );
        }
        let font = egui::FontId::proportional(12.0);
        let ink = super::theme::canvas().dimension;
        for (k, v) in [(0.0, c.1), (0.5, (c.0 + c.1) / 2.0), (1.0, c.0)] {
            painter.text(
                Pos2::new(x + w + 6.0, top + h * k),
                egui::Align2::LEFT_CENTER,
                format!("{v:.3} {unit}"),
                font.clone(),
                ink,
            );
        }
        painter.text(
            Pos2::new(x, top - 14.0),
            egui::Align2::LEFT_CENTER,
            self.fa.show.label(),
            egui::FontId::proportional(13.0),
            ink,
        );
    }

    /// The Properties panel of the environment.
    pub(super) fn fa_properties(&mut self, ui: &mut egui::Ui) {
        if self.fa.tool.is_some() {
            self.fa_panel(ui);
            return;
        }
        ui.label(RichText::new("Frame Analysis").heading());
        ui.separator();
        if let Some(e) = &self.fa.status {
            ui.colored_label(super::theme::tokens().error, e);
        }
        if self.fa.job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Solving...");
            });
        }
        if let Some(m) = &self.fa.model {
            ui.label(format!(
                "{} beams, {} nodes, {} rigid links",
                m.lines.len(),
                m.model.nodes.len(),
                m.model.links.len()
            ));
            for s in &m.skipped {
                ui.colored_label(
                    super::theme::tokens().warning,
                    format!("Member {}: {}", s.member, s.why),
                );
            }
        }
        if self.fa.run.is_some() {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Display scale");
                ui.add(ExprValue::new(&mut self.fa.scale).range(0.0..=1.0e9))
                    .on_hover_text("0 = automatic (5 % of the model)");
            });
            ui.checkbox(&mut self.fa.animate, "Animate");
        }
        if let Some(Run::Static(r)) = &self.fa.run {
            let c = self.show_range(r);
            ui.label(format!(
                "{}: {:.3} … {:.3} {}",
                self.fa.show.label(),
                c.0,
                c.1,
                self.fa.show.unit()
            ));
            let stress = largest(&r.members, |m, s| {
                let c = r.model.lines[m.line].section;
                s.s_max(&c).abs().max(s.s_min(&c).abs())
            });
            ui.label(format!("Largest normal stress: {stress:.2} MPa"));
            if let Some(y) = r
                .model
                .lines
                .iter()
                .filter_map(|l| l.yield_strength)
                .reduce(Real::min)
                && stress > 0.0
            {
                ui.label(format!("Safety factor to yield: {:.2}", y / stress));
            }
            let Some(sim) = self.fa_sim() else {
                return;
            };
            if !r.solution.reactions.is_empty() {
                ui.separator();
                ui.label(RichText::new("Reactions").strong());
                for ((k, _), v) in r.model.held.iter().zip(&r.solution.reactions) {
                    let name = sim.constraints.get(*k).map_or("", |c| c.hold.label());
                    let text = format!(
                        "{name} {}: F {:.1} {:.1} {:.1} N, M {:.1} {:.1} {:.1} N m",
                        k + 1,
                        v[0],
                        v[1],
                        v[2],
                        v[3] / 1000.0,
                        v[4] / 1000.0,
                        v[5] / 1000.0
                    );
                    ui.add(egui::Label::new(text).wrap());
                }
            }
        }
    }

    fn fa_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut t) = self.fa.tool.take() else {
            return;
        };
        let title = t.title();
        self.panel_header(ui, t.icon(), title);
        let lengths = self.fa.model.as_ref().and_then(|m| {
            t.at.and_then(|a| m.line_of(a.member).map(|l| m.lines[l].length()))
        });
        egui::Grid::new("fa_panel")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .min_col_width(super::panels::label_width(ui.ctx()))
            .show(ui, |ui| {
                let row = |ui: &mut egui::Ui, label: &str, v: &mut Real, suffix: &str| {
                    super::panels::plabel(ui, label);
                    ui.add(ExprValue::new(v).suffix(suffix.to_owned()).cursor(label));
                    ui.end_row();
                };
                if t.kind != Kind::Gravity {
                    ui.label(
                        if matches!(t.kind, Kind::Beam | Kind::Release | Kind::Continuous) {
                            "Beam"
                        } else {
                            "Location"
                        },
                    );
                    ui.label(match (t.at, self.fa.model.as_ref()) {
                        (Some(a), Some(m)) => m
                            .line_of(a.member)
                            .map_or("?".to_owned(), |l| m.lines[l].name.clone()),
                        _ => "Select".to_owned(),
                    });
                    ui.end_row();
                }
                if let (Some(a), Some(_)) = (t.at.as_mut(), lengths)
                    && !matches!(t.kind, Kind::Beam | Kind::Release | Kind::Continuous)
                {
                    row(ui, "Offset", &mut a.s, " mm");
                }
                match t.kind {
                    Kind::Hold(HoldKind::Floating) => {
                        super::panels::plabel(ui, "Direction held");
                        egui::ComboBox::from_id_salt("fa_axis")
                            .selected_text(["X", "Y", "Z"][t.axis.min(2)])
                            .show_ui(ui, |ui| {
                                for (i, n) in ["X", "Y", "Z"].iter().enumerate() {
                                    ui.selectable_value(&mut t.axis, i, *n);
                                }
                            });
                        ui.end_row();
                    }
                    Kind::Hold(HoldKind::Custom) => {
                        for (i, n) in ["X", "Y", "Z", "Rx", "Ry", "Rz"].iter().enumerate() {
                            super::panels::plabel(ui, &format!("Hold {n}"));
                            ui.checkbox(&mut t.a[i], "");
                            ui.end_row();
                        }
                    }
                    Kind::Force => {
                        super::panels::plabel(ui, "Vector Components");
                        ui.checkbox(&mut t.components, "");
                        ui.end_row();
                        if t.components {
                            row(ui, "Fx (N)", &mut t.v[0], "");
                            row(ui, "Fy (N)", &mut t.v[1], "");
                            row(ui, "Fz (N)", &mut t.v[2], "");
                        } else {
                            row(ui, "Magnitude (N)", &mut t.value, "");
                            row(ui, "Angle of Plane", &mut t.plane, "°");
                            row(ui, "Angle in Plane", &mut t.in_plane, "°");
                        }
                    }
                    Kind::Continuous => {
                        row(ui, "Qx (N/mm)", &mut t.v[0], "");
                        row(ui, "Qy (N/mm)", &mut t.v[1], "");
                        row(ui, "Qz (N/mm)", &mut t.v[2], "");
                        super::panels::plabel(ui, "Custom size");
                        ui.checkbox(&mut t.custom_size, "");
                        ui.end_row();
                        if t.custom_size {
                            row(ui, "Offset", &mut t.from, " mm");
                            row(ui, "Length", &mut t.length, " mm");
                        }
                    }
                    Kind::Moment => {
                        row(ui, "Mx (N m)", &mut t.v[0], "");
                        row(ui, "My (N m)", &mut t.v[1], "");
                        row(ui, "Mz (N m)", &mut t.v[2], "");
                    }
                    Kind::Axial => row(ui, "Moment (N m)", &mut t.value, ""),
                    Kind::Bending => {
                        row(ui, "Moment (N m)", &mut t.value, "");
                        row(ui, "Plane angle", &mut t.plane, "°");
                    }
                    Kind::Gravity => {
                        super::panels::plabel(ui, "Direction");
                        egui::ComboBox::from_id_salt("fa_grav")
                            .selected_text(["X", "Y", "Z"][t.axis.min(2)])
                            .show_ui(ui, |ui| {
                                for (i, n) in ["X", "Y", "Z"].iter().enumerate() {
                                    ui.selectable_value(&mut t.axis, i, *n);
                                }
                            });
                        ui.end_row();
                        super::panels::plabel(ui, "Opposite");
                        ui.checkbox(&mut t.negative, "");
                        ui.end_row();
                        row(ui, "Acceleration (mm/s²)", &mut t.value, "");
                    }
                    Kind::Release => {
                        for (end, flags) in [("Start", &mut t.a), ("End", &mut t.b)] {
                            for (i, n) in ["Fz", "Fx", "Fy", "Mz", "Mx", "My"].iter().enumerate() {
                                super::panels::plabel(ui, &format!("{end} {n} released"));
                                ui.checkbox(&mut flags[i], "");
                                ui.end_row();
                            }
                        }
                    }
                    Kind::Link => {
                        super::panels::plabel(ui, "Children");
                        ui.label(format!("{}", t.children.len()));
                        ui.end_row();
                    }
                    Kind::Beam => {
                        for (i, n) in [
                            "E (MPa)",
                            "G (MPa)",
                            "Density (t/mm³)",
                            "A (mm²)",
                            "Ix (mm⁴)",
                            "Iy (mm⁴)",
                            "J (mm⁴)",
                        ]
                        .iter()
                        .enumerate()
                        {
                            row(ui, n, &mut t.numbers[i], "");
                        }
                    }
                    _ => {}
                }
            });
        if t.kind == Kind::Beam {
            ui.label("0 keeps the frame's own value");
        }
        let ready = t.ready();
        let mut ok = false;
        let mut cancel = false;
        super::theme::panel_footer(ui, |ui| {
            ok = ui
                .add_enabled(ready, super::theme::ok_button("OK"))
                .clicked();
            cancel = ui.button("Cancel").clicked();
        });
        ok |= super::keys::panel_ok_key(ui, ready);
        if cancel {
            return;
        }
        if ok {
            self.fa_apply(&t);
            return;
        }
        self.fa.tool = Some(t);
    }

    fn fa_apply(&mut self, t: &Tool) {
        let k = |v: Real| v * 1000.0;
        let at = t.at;
        let load = |kind| FaLoad {
            kind,
            suppressed: false,
        };
        let title = t.title();
        let t = t.clone();
        // A place on no member (Gravity) needs none below.
        self.fa_edit(title, |s| {
            let Some(a) = at.or(Some(At { member: 0, s: 0.0 })) else {
                return;
            };
            match t.kind {
                Kind::Hold(h) => {
                    let hold = match h {
                        HoldKind::Fixed => Hold::Fixed,
                        HoldKind::Pinned => Hold::Pinned,
                        HoldKind::Floating => Hold::Floating {
                            axis: t.axis.min(2) as u8,
                        },
                        HoldKind::Custom => Hold::Custom { dofs: t.a },
                    };
                    s.constraints.push(FaConstraint {
                        at: a,
                        hold,
                        suppressed: false,
                    });
                }
                Kind::Force => s.loads.push(load(FaLoadKind::Force {
                    at: a,
                    f: t.force(),
                })),
                Kind::Moment => s.loads.push(load(FaLoadKind::Moment {
                    at: a,
                    m: t.v.map(k),
                })),
                Kind::Axial => s.loads.push(load(FaLoadKind::AxialMoment {
                    at: a,
                    m: k(t.value),
                })),
                Kind::Bending => s.loads.push(load(FaLoadKind::BendingMoment {
                    at: a,
                    m: k(t.value),
                    angle: t.plane.to_radians(),
                })),
                Kind::Continuous => {
                    let (from, to) = if t.custom_size {
                        (t.from, t.from + t.length)
                    } else {
                        (0.0, Real::MAX)
                    };
                    s.loads.push(load(FaLoadKind::Continuous {
                        member: a.member,
                        from,
                        to,
                        q: t.v,
                    }));
                }
                Kind::Gravity => {
                    let mut g = [0.0; 3];
                    g[t.axis.min(2)] = if t.negative { -t.value } else { t.value };
                    s.loads.push(load(FaLoadKind::Gravity { g }));
                }
                Kind::Node => s.nodes.push(a),
                Kind::Release => {
                    s.releases.retain(|r| r.member != a.member);
                    s.releases.push(FaRelease {
                        member: a.member,
                        start: t.a,
                        end: t.b,
                    });
                }
                Kind::Link => s.links.push(FaLink {
                    parent: a,
                    children: t.children.clone(),
                }),
                Kind::Beam => {
                    let v = |i: usize| (t.numbers[i] > 0.0).then_some(t.numbers[i]);
                    s.overrides.retain(|o| o.member != a.member);
                    let o = BeamOverride {
                        member: a.member,
                        e: v(0),
                        g: v(1),
                        rho: v(2),
                        area: v(3),
                        ix: v(4),
                        iy: v(5),
                        j: v(6),
                    };
                    if t.numbers.iter().any(|x| *x > 0.0) {
                        s.overrides.push(o);
                    }
                }
            }
        });
    }

    /// The browser of the environment.
    pub(super) fn fa_tree(&mut self, ui: &mut egui::Ui) {
        let sims: Vec<String> = self
            .frame_asm()
            .map(|a| a.simulations.iter().map(|s| s.name.clone()).collect())
            .unwrap_or_default();
        let mut delete_load = None;
        let mut delete_hold = None;
        let mut toggle_load = None;
        let mut toggle_hold = None;
        for (i, name) in sims.iter().enumerate() {
            if ui
                .selectable_label(i == self.fa.sim, RichText::new(name).strong())
                .clicked()
                && i != self.fa.sim
            {
                self.fa.sim = i;
                self.fa_refresh();
            }
        }
        let Some(sim) = self.fa_sim().cloned() else {
            return;
        };
        ui.separator();
        let names = self.fa.model.as_ref().map(|m| {
            m.lines
                .iter()
                .map(|l| (l.member, l.name.clone()))
                .collect::<Vec<_>>()
        });
        egui::CollapsingHeader::new("Beams")
            .default_open(false)
            .show(ui, |ui| {
                for (m, n) in names.iter().flatten() {
                    if ui
                        .selectable_label(self.fa.selected == Some(*m), n)
                        .clicked()
                    {
                        self.fa.selected = Some(*m);
                    }
                }
            });
        egui::CollapsingHeader::new("Constraints")
            .default_open(true)
            .show(ui, |ui| {
                for (i, c) in sim.constraints.iter().enumerate() {
                    let text = format!("{}:{}", c.hold.label(), i + 1);
                    let text = if c.suppressed {
                        RichText::new(text).weak()
                    } else {
                        RichText::new(text)
                    };
                    ui.label(text).context_menu(|ui| {
                        if ui.button("Suppress").clicked() {
                            toggle_hold = Some(i);
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            delete_hold = Some(i);
                            ui.close();
                        }
                    });
                }
            });
        egui::CollapsingHeader::new("Loads")
            .default_open(true)
            .show(ui, |ui| {
                for (i, l) in sim.loads.iter().enumerate() {
                    let text = format!("{}:{}", l.kind.label(), i + 1);
                    let text = if l.suppressed {
                        RichText::new(text).weak()
                    } else {
                        RichText::new(text)
                    };
                    ui.label(text).context_menu(|ui| {
                        if ui.button("Suppress").clicked() {
                            toggle_load = Some(i);
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            delete_load = Some(i);
                            ui.close();
                        }
                    });
                }
            });
        if !sim.releases.is_empty() {
            ui.label(format!("Releases ({})", sim.releases.len()));
        }
        if !sim.links.is_empty() {
            ui.label(format!("Rigid Links ({})", sim.links.len()));
        }
        match &self.fa.run {
            Some(Run::Static(_)) => {
                egui::CollapsingHeader::new("Results")
                    .default_open(true)
                    .show(ui, |ui| {
                        let pick = |ui: &mut egui::Ui, s: Show, n: &str, cur: &mut Show| {
                            if ui.selectable_label(*cur == s, n).clicked() {
                                *cur = s;
                            }
                        };
                        let cur = &mut self.fa.show;
                        pick(ui, Show::Displacement, "Displacement", cur);
                        ui.collapsing("Loads", |ui| {
                            for (s, n) in Show::LOADS {
                                pick(ui, s, n, cur);
                            }
                        });
                        ui.collapsing("Moments", |ui| {
                            for (s, n) in Show::MOMENTS {
                                pick(ui, s, n, cur);
                            }
                        });
                        ui.collapsing("Normal Stresses", |ui| {
                            for (s, n) in Show::NORMAL {
                                pick(ui, s, n, cur);
                            }
                        });
                        ui.collapsing("Shear Stresses", |ui| {
                            for (s, n) in Show::SHEAR {
                                pick(ui, s, n, cur);
                            }
                        });
                        ui.collapsing("Torsional Stresses", |ui| {
                            pick(ui, Show::T, "T", cur);
                        });
                    });
            }
            Some(Run::Modal(r)) => {
                let freq: Vec<Real> = r.modes.iter().map(|m| m.hz).collect();
                egui::CollapsingHeader::new("Modal Frequency")
                    .default_open(true)
                    .show(ui, |ui| {
                        for (i, hz) in freq.iter().enumerate() {
                            if ui
                                .selectable_label(
                                    self.fa.mode == i,
                                    format!("F{} {hz:.2} Hz", i + 1),
                                )
                                .clicked()
                            {
                                self.fa.mode = i;
                            }
                        }
                    });
            }
            None => {}
        }
        if let Some(i) = delete_hold {
            self.fa_edit("Delete Constraint", |s| {
                s.constraints.remove(i);
            });
        }
        if let Some(i) = delete_load {
            self.fa_edit("Delete Load", |s| {
                s.loads.remove(i);
            });
        }
        if let Some(i) = toggle_hold {
            self.fa_edit("Suppress Constraint", |s| {
                s.constraints[i].suppressed ^= true;
            });
        }
        if let Some(i) = toggle_load {
            self.fa_edit("Suppress Load", |s| {
                s.loads[i].suppressed ^= true;
            });
        }
    }

    /// Create Simulation and Beam Detail windows.
    fn fa_windows(&mut self, ctx: &egui::Context) {
        if let Some((mut name, mut kind, mut count)) = self.fa.create.take() {
            let (mut ok, mut cancel) = (false, false);
            egui::Window::new("Create New Simulation")
                .collapsible(false)
                .resizable(false)
                .default_pos(ctx.content_rect().center() - egui::vec2(170.0, 80.0))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut name);
                    });
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut kind, 0, "Static Analysis");
                        ui.radio_value(&mut kind, 1, "Modal Analysis");
                    });
                    if kind == 1 {
                        ui.horizontal(|ui| {
                            ui.label("Number of Frequencies");
                            ui.add(ExprValue::new(&mut count).range(1..=64));
                        });
                    }
                    ui.horizontal(|ui| {
                        ok = ui.add(super::theme::ok_button("OK")).clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            if ok {
                let doc = self.frame_doc();
                self.checkpoint("Create Simulation");
                if let Some(DocumentData::Assembly(a)) =
                    self.project.document_mut(doc).map(|d| &mut d.data)
                {
                    let k = if kind == 0 {
                        SimKind::Static
                    } else {
                        SimKind::Modal {
                            count: count as u32,
                        }
                    };
                    a.simulations.push(Simulation::new(name.trim(), k));
                    self.fa.sim = a.simulations.len() - 1;
                }
                self.fa_refresh();
            } else if !cancel {
                self.fa.create = Some((name, kind, count));
            }
        }
        if self.fa.detail {
            self.fa_detail_window(ctx);
        }
    }

    /// Beam Detail: the diagrams of the selected member.
    fn fa_detail_window(&mut self, ctx: &egui::Context) {
        let mut open = self.fa.detail;
        let member = self.fa.selected;
        let win = egui::Window::new("Beam Detail")
            .open(&mut open)
            .default_size([520.0, 560.0]);
        let run = self.fa.run.clone();
        self.layout
            .place("fa_detail", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                let Some(Run::Static(r)) = &run else {
                    ui.label("Run a static simulation first");
                    return;
                };
                let Some(mem) = member
                    .and_then(|id| r.members.iter().find(|m| m.member == id))
                    .or(r.members.first())
                else {
                    ui.label("Click a beam in the view");
                    return;
                };
                let l = &r.model.lines[mem.line];
                ui.label(format!("{}   L = {:.1} mm", l.name, l.length()));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (name, unit, f) in DIAGRAMS {
                        diagram(ui, name, unit, l.length(), &mem.stations, &l.section, *f);
                    }
                });
            });
        self.fa.detail = open;
    }
}

/// Snap radius onto a node, pixels.
const NODE_SNAP_PX: f32 = 12.0;

/// A section of zero numbers, for values that do not read it.
const BLANK: BeamSection = BeamSection {
    area: 1.0,
    iu: 1.0,
    iv: 1.0,
    j: 1.0,
    wu: 1.0,
    wv: 1.0,
    wt: 1.0,
    au: 1.0,
    av: 1.0,
};

type Reader = fn(&Station, &BeamSection) -> Real;

const DIAGRAMS: &[(&str, &str, Reader)] = &[
    ("Displacement", "mm", |s, c| Show::Displacement.of(s, c)),
    ("Fz (axial)", "N", |s, c| Show::Fz.of(s, c)),
    ("Fx", "N", |s, c| Show::Fx.of(s, c)),
    ("Fy", "N", |s, c| Show::Fy.of(s, c)),
    ("Mx", "N m", |s, c| Show::Mx.of(s, c)),
    ("My", "N m", |s, c| Show::My.of(s, c)),
    ("Mz (torsion)", "N m", |s, c| Show::Mz.of(s, c)),
    ("Smax", "MPa", |s, c| Show::Smax.of(s, c)),
    ("Smin", "MPa", |s, c| Show::Smin.of(s, c)),
];

/// One diagram: value along the beam with its extremes.
fn diagram(
    ui: &mut egui::Ui,
    name: &str,
    unit: &str,
    length: Real,
    st: &[Station],
    c: &BeamSection,
    f: Reader,
) {
    let values: Vec<(Real, Real)> = st.iter().map(|s| (s.s, f(s, c))).collect();
    let lo = values.iter().map(|v| v.1).fold(0.0, Real::min);
    let hi = values.iter().map(|v| v.1).fold(0.0, Real::max);
    ui.label(format!("{name}   {lo:.3} … {hi:.3} {unit}"));
    let (r, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 70.0), egui::Sense::hover());
    let p = ui.painter_at(r);
    let tokens = super::theme::tokens();
    p.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0, tokens.line),
        egui::StrokeKind::Inside,
    );
    let span = (hi - lo).max(1e-9);
    let to = |(s, v): (Real, Real)| {
        Pos2::new(
            r.left() + (s / length.max(1e-9)) as f32 * r.width(),
            r.bottom() - ((v - lo) / span) as f32 * r.height(),
        )
    };
    let zero = to((0.0, 0.0)).y;
    p.line_segment(
        [Pos2::new(r.left(), zero), Pos2::new(r.right(), zero)],
        Stroke::new(1.0, tokens.line),
    );
    let pts: Vec<Pos2> = values.iter().copied().map(to).collect();
    p.add(egui::Shape::line(pts, Stroke::new(2.0, tokens.accent)));
}
