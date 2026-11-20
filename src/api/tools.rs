//! The tool table: every command an agent can call, its input schema and the function that runs
//! it on a [`Session`] (spec 0.7 §3).
//!
//! A tool that changes the project runs on a copy of the session; only a success is taken back,
//! as one Undo step (rule 5: failure leaves the state unchanged).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::args::Args;
use super::session::{ApiError, Session};
use super::{asm, more, part};
use crate::document::{AssemblyDocument, DocumentData, Project};
use crate::part::PartDocument;

/// What a tool does to the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Only reads.
    Read,
    /// Changes the project: one Undo step.
    Change,
    /// Replaces the project (New, Open): clears Undo.
    Replace,
    /// Runs on the session itself, at once, never an Undo step of its own (Save, Undo, Redo).
    InPlace,
}

/// One tool.
pub struct Tool {
    /// The MCP tool name (`extrude`).
    pub name: &'static str,
    /// The command's name in errors and Undo (`Extrude`, Inventor's name where it has one).
    pub command: &'static str,
    /// What it does, for the agent.
    pub description: &'static str,
    /// What it does to the session.
    pub effect: Effect,
    /// The JSON schema of its arguments.
    pub schema: fn() -> Value,
    /// The work.
    pub run: fn(&mut Session, &Args) -> Result<Value, ApiError>,
}

/// An object schema with `props` and the `required` keys.
pub fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

fn none() -> Value {
    obj(json!({}), &[])
}

const PART_ARG: &str = "Part document to work on (default: the active document).";

/// The `part` argument every part tool takes.
pub fn part_prop() -> Value {
    json!({ "type": "string", "description": PART_ARG })
}

/// Every tool, in the order `tools/list` shows them.
pub fn tools() -> &'static [Tool] {
    TOOLS
}

/// The tool named `name`.
pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

static TOOLS: &[Tool] = &[
    // ---- project ---------------------------------------------------------
    Tool {
        name: "status",
        command: "Status",
        description: "The project: name, file, active document, documents, Undo / Redo steps and the work running now (with progress). Answers even while another call runs.",
        effect: Effect::Read,
        schema: none,
        run: status,
    },
    Tool {
        name: "cancel",
        command: "Cancel",
        description: "Cancels the call that is running: the project stays as it was before it. Answers at once.",
        effect: Effect::Read,
        schema: none,
        run: cancel_idle,
    },
    Tool {
        name: "project_new",
        command: "New",
        description: "Starts a new, empty project (forgets the current one and its Undo history). Then create a part with document_new.",
        effect: Effect::Replace,
        schema: || {
            obj(
                json!({"name": {"type": "string", "description": "Project name"}}),
                &[],
            )
        },
        run: project_new,
    },
    Tool {
        name: "project_open",
        command: "Open",
        description: "Opens a .3dcadcraft project file (or project folder). Every part is rebuilt from its features.",
        effect: Effect::Replace,
        schema: || {
            obj(
                json!({"path": {"type": "string", "description": "File path"}}),
                &["path"],
            )
        },
        run: project_open,
    },
    Tool {
        name: "project_save",
        command: "Save",
        description: "Saves the project (a .3dcadcraft file). Without 'path' it saves where it was opened or last saved.",
        effect: Effect::InPlace,
        schema: || {
            obj(
                json!({"path": {"type": "string", "description": "File path (.3dcadcraft)"}}),
                &[],
            )
        },
        run: project_save,
    },
    Tool {
        name: "document_new",
        command: "New Document",
        description: "Adds a part or an assembly to the project and makes it the active document.",
        effect: Effect::Change,
        schema: || {
            obj(
                json!({
                    "kind": {"type": "string", "enum": ["part", "assembly"]},
                    "name": {"type": "string", "description": "Unique document name, e.g. Plate"}
                }),
                &["kind", "name"],
            )
        },
        run: document_new,
    },
    Tool {
        name: "document_activate",
        command: "Activate",
        description: "Makes another document of the project the active one.",
        effect: Effect::Change,
        schema: || obj(json!({"name": {"type": "string"}}), &["name"]),
        run: document_activate,
    },
    Tool {
        name: "undo",
        command: "Undo",
        description: "Undoes the last changing call (a batch is one step).",
        effect: Effect::InPlace,
        schema: none,
        run: undo,
    },
    Tool {
        name: "redo",
        command: "Redo",
        description: "Redoes the last undone call.",
        effect: Effect::InPlace,
        schema: none,
        run: redo,
    },
    Tool {
        name: "batch",
        command: "Batch",
        description: "Runs several calls as one: all succeed or nothing changes, and the error names the failing entry. One Undo step.",
        effect: Effect::Change,
        schema: || {
            obj(
                json!({"commands": {"type": "array", "items": {"type": "object", "properties": {
            "tool": {"type": "string"}, "arguments": {"type": "object"}}, "required": ["tool"]}}}),
                &["commands"],
            )
        },
        run: batch,
    },
    // ---- sketch ----------------------------------------------------------
    Tool {
        name: "sketch_create",
        command: "Start 2D Sketch",
        description: "Starts a sketch on an origin plane (XY, XZ, YZ) or on a planar face by persistent name (e.g. Extrusion1:End). The origin is projected as a fixed point. Returns the sketch id.",
        effect: Effect::Change,
        schema: part::sketch_create_schema,
        run: part::sketch_create,
    },
    Tool {
        name: "sketch_line",
        command: "Line",
        description: "Adds a line. Each end is [x, y] (sketch mm) or a point handle (p3) to connect to.",
        effect: Effect::Change,
        schema: part::sketch_line_schema,
        run: part::sketch_line,
    },
    Tool {
        name: "sketch_rectangle",
        command: "Two Point Rectangle",
        description: "Adds an axis-aligned rectangle from two opposite corners (4 lines, horizontal / vertical constraints). With center=true the first point is the centre.",
        effect: Effect::Change,
        schema: part::sketch_rectangle_schema,
        run: part::sketch_rectangle,
    },
    Tool {
        name: "sketch_circle",
        command: "Center Point Circle",
        description: "Adds a circle: centre ([x, y] or a point handle) and radius (mm).",
        effect: Effect::Change,
        schema: part::sketch_circle_schema,
        run: part::sketch_circle,
    },
    Tool {
        name: "sketch_arc",
        command: "Center Point Arc",
        description: "Adds a counter-clockwise arc from its centre, start and end (each [x, y] or a point handle); the end is moved onto the circle through the start.",
        effect: Effect::Change,
        schema: part::sketch_arc_schema,
        run: part::sketch_arc,
    },
    Tool {
        name: "sketch_point",
        command: "Point",
        description: "Adds a sketch point (center=true: a Center Point, which Hole uses).",
        effect: Effect::Change,
        schema: part::sketch_point_schema,
        run: part::sketch_point,
    },
    Tool {
        name: "sketch_constrain",
        command: "Constraint",
        description: "Adds a constraint or dimension and solves. 'constraint' is an object with 'type' and handles, e.g. {type: Distance, a: p1, b: p2, value: 30}. Types: Coincident{a,b}, PointOnLine{point,line}, PointOnCircle{point,circle}, Midpoint{point,line}, Horizontal{line}, Vertical{line}, HorizontalPoints{a,b}, VerticalPoints{a,b}, Parallel{a,b}, Perpendicular{a,b}, Collinear{a,b}, Concentric{a,b}, Tangent{a,b}, Equal{a,b}, Symmetric{a,b,line}, Fix{point}, Distance{a,b,value}, HorizontalDistance{a,b,value}, VerticalDistance{a,b,value}, PointLineDistance{point,line,value}, Radius{circle,value}, Diameter{circle,value}, Angle{a,b,value in degrees}. Refused (nothing changes) when it over-constrains the sketch or does not solve. A dimension gets a parameter name (d0, d1...).",
        effect: Effect::Change,
        schema: part::sketch_constrain_schema,
        run: part::sketch_constrain,
    },
    Tool {
        name: "sketch_info",
        command: "Sketch",
        description: "A sketch's points, curves, constraints, closed regions (profiles for extrude) and its degrees of freedom. detail=false (default) lists handles and counts only.",
        effect: Effect::Read,
        schema: part::sketch_info_schema,
        run: part::sketch_info,
    },
    // ---- features --------------------------------------------------------
    Tool {
        name: "extrude",
        command: "Extrude",
        description: "Extrudes closed regions of a sketch. Operation: new_solid (default for the first body), join (default after), cut, intersect. Extent: distance (default) or through_all. Direction: default (along the sketch normal), flipped, symmetric.",
        effect: Effect::Change,
        schema: part::extrude_schema,
        run: part::extrude,
    },
    Tool {
        name: "revolve",
        command: "Revolve",
        description: "Revolves closed regions of a sketch about an axis: X, Y, Z (origin axes) or a sketch line handle (e3). Angle in degrees (default 360).",
        effect: Effect::Change,
        schema: part::revolve_schema,
        run: part::revolve,
    },
    Tool {
        name: "hole",
        command: "Hole",
        description: "Holes on a planar face (persistent name) at model points [x, y, z] (projected onto the face). type: simple (diameter, depth or through_all), clearance (size like M8, fit close/normal/loose, through all) or tapped (size like M8, depth or through_all). Depths in mm.",
        effect: Effect::Change,
        schema: part::hole_schema,
        run: part::hole,
    },
    Tool {
        name: "fillet",
        command: "Fillet",
        description: "Rounds edges. Each edge is 'FaceA|FaceB' (the two faces it separates, as listed by edges) or an edge name.",
        effect: Effect::Change,
        schema: part::fillet_schema,
        run: part::fillet,
    },
    Tool {
        name: "chamfer",
        command: "Chamfer",
        description: "Chamfers edges by an equal distance. Edges as for fillet.",
        effect: Effect::Change,
        schema: part::chamfer_schema,
        run: part::chamfer,
    },
    Tool {
        name: "shell",
        command: "Shell",
        description: "Hollows the solid to a wall thickness, opening the listed faces. direction: inside (default), outside, both.",
        effect: Effect::Change,
        schema: part::shell_schema,
        run: part::shell,
    },
    Tool {
        name: "pattern",
        command: "Pattern",
        description: "Rectangular or circular pattern of features. Rectangular: direction X/Y/Z (or a straight edge name), count, spacing, optional second direction. Circular: axis X/Y/Z, count, angle in degrees (default 360).",
        effect: Effect::Change,
        schema: part::pattern_schema,
        run: part::pattern,
    },
    Tool {
        name: "mirror",
        command: "Mirror",
        description: "Mirrors features about a plane: XY, XZ, YZ, a work plane or a planar face (name or alias).",
        effect: Effect::Change,
        schema: more::mirror_schema,
        run: more::mirror,
    },
    Tool {
        name: "work_plane",
        command: "Work Plane",
        description: "Adds a work plane: offset from a plane (distance mm), midplane of two planes, or at an angle (degrees) to a plane about a line. Returns its id (WorkPlane1) for sketch_create {plane}, mirror, other work features.",
        effect: Effect::Change,
        schema: more::work_plane_schema,
        run: more::work_plane,
    },
    Tool {
        name: "work_axis",
        command: "Work Axis",
        description: "Adds a work axis: where two planes meet, through a circular edge's centre, or square to a plane through a point (origin or a work point). Returns its id (WorkAxis1).",
        effect: Effect::Change,
        schema: more::work_axis_schema,
        run: more::work_axis,
    },
    Tool {
        name: "work_point",
        command: "Work Point",
        description: "Adds a work point: fixed at [x, y, z], where three planes meet, or where a line pierces a plane. Returns its id (WorkPoint1).",
        effect: Effect::Change,
        schema: more::work_point_schema,
        run: more::work_point,
    },
    Tool {
        name: "sweep",
        command: "Sweep",
        description: "Sweeps closed regions of a profile sketch along a path: curves of another sketch (path_sketch + path_curves handles) or model edges (path_edges). Operation as extrude.",
        effect: Effect::Change,
        schema: more::sweep_schema,
        run: more::sweep,
    },
    Tool {
        name: "loft",
        command: "Loft",
        description: "Lofts through two or more sections in order: {sketch, region} or {sketch, point} (a point only first or last). Sketch the sections on parallel work planes. Operation as extrude; closed=true joins the last to the first.",
        effect: Effect::Change,
        schema: more::loft_schema,
        run: more::loft,
    },
    Tool {
        name: "parameters",
        command: "Parameters",
        description: "The part's parameters: sketch dimensions, feature values and user parameters, with value, unit and equation.",
        effect: Effect::Read,
        schema: || obj(json!({"part": part_prop()}), &[]),
        run: part::parameters,
    },
    Tool {
        name: "parameter_set",
        command: "Parameters",
        description: "Sets a parameter's value or equation (e.g. '120 mm', 'd0 / 2') and rebuilds the part. Refused, with nothing changed, when it does not evaluate or a feature fails.",
        effect: Effect::Change,
        schema: || {
            obj(
                json!({"part": part_prop(), "name": {"type": "string"}, "expression": {"type": "string"}}),
                &["name", "expression"],
            )
        },
        run: part::parameter_set,
    },
    // ---- queries ---------------------------------------------------------
    Tool {
        name: "features",
        command: "Model",
        description: "The part's feature tree: id, name, type, health (an error text when it failed), suppressed; and the sketches with their DOF.",
        effect: Effect::Read,
        schema: || obj(json!({"part": part_prop()}), &[]),
        run: part::features,
    },
    Tool {
        name: "faces",
        command: "Faces",
        description: "Faces as [name, surface, alias, dir]: the persistent name (Extrusion1:End), surface type, a short alias when one exists (Top, Extrusion1:+X; any face argument accepts it) and a plane's outward normal (+X ... -Z, or a vector). detail=true adds area, centre, normal, radius. 'feature' keeps the faces of one feature. Answers always use persistent names.",
        effect: Effect::Read,
        schema: || {
            obj(
                json!({"part": part_prop(), "feature": {"type": "string"}, "detail": {"type": "boolean"}}),
                &[],
            )
        },
        run: part::faces,
    },
    Tool {
        name: "edges",
        command: "Edges",
        description: "Edges as [edge, kind, length, alias]: 'FaceA|FaceB' (the faces they separate), line / circle / curve, length, and 'AliasA|AliasB' when both faces have an alias (edge arguments accept aliases on either side, e.g. Top|+X). detail=true adds the middle point and a line's direction. 'face' keeps the edges of one face.",
        effect: Effect::Read,
        schema: || {
            obj(
                json!({"part": part_prop(), "face": {"type": "string"}, "feature": {"type": "string"}, "detail": {"type": "boolean"}}),
                &[],
            )
        },
        run: part::edges,
    },
    Tool {
        name: "face_at",
        command: "Select Face",
        description: "The face at a model point: with 'direction', the first face the ray from 'point' meets (like a click); without, the face nearest the point. Answers its persistent name, aliases, surface, centre, normal, the point on it and the distance.",
        effect: Effect::Read,
        schema: part::face_at_schema,
        run: part::face_at,
    },
    Tool {
        name: "measure",
        command: "Measure",
        description: "Measures one item (length, area, radius, position) or two (minimum distance, centre distance, angle). Items: 'face:<name>', 'edge:<FaceA|FaceB>', 'vertex:<name>', 'body:<name>' or a point [x, y, z].",
        effect: Effect::Read,
        schema: || obj(json!({"part": part_prop(), "a": {}, "b": {}}), &["a"]),
        run: part::measure,
    },
    Tool {
        name: "physical",
        command: "Physical Properties",
        description: "Volume (mm3), area (mm2), mass (g), centre of gravity (mm) and bounding box of the part's solids. 'material' sets the part's material first (a name of the material library).",
        effect: Effect::Read,
        schema: || {
            obj(
                json!({"part": part_prop(), "material": {"type": "string"}}),
                &[],
            )
        },
        run: part::physical,
    },
    Tool {
        name: "export",
        command: "Export",
        description: "Writes the part to STEP (.step/.stp), STL (.stl) or glTF (.glb), by the file's extension or 'format'.",
        effect: Effect::Read,
        schema: || {
            obj(
                json!({"part": part_prop(), "path": {"type": "string"}, "format": {"type": "string", "enum": ["step", "stl", "glb"]}}),
                &["path"],
            )
        },
        run: part::export,
    },
    // ---- assemblies ------------------------------------------------------
    Tool {
        name: "place",
        command: "Place",
        description: "Places a part of the project in the active assembly at [x, y, z] (mm). The first component is grounded unless grounded=false. Returns the occurrence name (Plate:1).",
        effect: Effect::Change,
        schema: asm::place_schema,
        run: asm::place,
    },
    Tool {
        name: "constrain",
        command: "Constrain",
        description: "Adds an assembly relationship and solves: mate, flush (faces; offset in mm), insert (two circular edges 'FaceA|FaceB'), angle (degrees). Each side is {occurrence, face} or {occurrence, edge}. Refused when the assembly does not solve.",
        effect: Effect::Change,
        schema: asm::constrain_schema,
        run: asm::constrain,
    },
    Tool {
        name: "assembly_solve",
        command: "Assembly Solve",
        description: "Solves the active assembly: converged, residual, the degrees of freedom left per component and relationships that cannot be met.",
        effect: Effect::Change,
        schema: || obj(json!({"assembly": {"type": "string"}}), &[]),
        run: asm::solve,
    },
    Tool {
        name: "library",
        command: "Component Library",
        description: "Component Library families as [category, family, standard, sizes]; 'search' filters by words, 'family' lists that family's sizes.",
        effect: Effect::Read,
        schema: more::library_schema,
        run: more::library,
    },
    Tool {
        name: "library_place",
        command: "Place from Library",
        description: "Places a Component Library member (family + size) in the active assembly at [x, y, z]: the member becomes a part document of the project (reused when placed again). Returns the occurrence.",
        effect: Effect::Change,
        schema: more::library_place_schema,
        run: more::library_place,
    },
    Tool {
        name: "components",
        command: "Components",
        description: "The active assembly's components: occurrence name, part, position, grounded; and its relationships.",
        effect: Effect::Read,
        schema: || obj(json!({"assembly": {"type": "string"}}), &[]),
        run: asm::components,
    },
];

/// Whether `t` may run on `s`: a read-only server runs only reading tools (and, headless,
/// `project_open`); the app's session refuses the tools that replace its project.
pub fn allowed(s: &Session, t: &Tool) -> Result<(), String> {
    if s.hosted && t.effect == Effect::Replace {
        return Err("refused while attached to the app: open or start the project there".into());
    }
    if s.read_only && t.effect != Effect::Read && !(t.name == "project_open" && !s.hosted) {
        return Err("refused, the server is read-only".into());
    }
    Ok(())
}

/// The tools `s` may run, in `tools/list` order.
pub fn listed(s: &Session) -> Vec<&'static Tool> {
    TOOLS.iter().filter(|t| allowed(s, t).is_ok()).collect()
}

/// Runs tool `t` on `work` (a copy of the session for a changing tool). On success a changing
/// tool marks the copy dirty.
pub fn run_on(work: &mut Session, t: &Tool, args: &Value) -> Result<Value, ApiError> {
    allowed(work, t).map_err(|e| ApiError::new(t.command, e))?;
    let empty = json!({});
    let args = if args.is_null() { &empty } else { args };
    if !args.is_object() {
        return Err(ApiError::new(t.command, "the arguments must be an object"));
    }
    let v = (t.run)(work, &Args::new(t.command, args))?;
    if t.effect == Effect::Change {
        work.dirty = true;
    }
    Ok(v)
}

/// Takes the copy that ran `t` back into `session`.
pub fn finish(session: &mut Session, t: &Tool, work: Session) {
    match t.effect {
        Effect::Read | Effect::InPlace => {}
        Effect::Change => session.commit(t.command, work),
        Effect::Replace => {
            session.commit(t.command, work);
            session.history.clear();
        }
    }
}

/// Calls tool `name` with `args` on `session`: the command layer in one step (the MCP server
/// runs [`run_on`] on a worker thread and [`finish`] when it is done).
pub fn call(session: &mut Session, name: &str, args: &Value) -> Result<Value, ApiError> {
    let t = find(name).ok_or_else(|| ApiError::new(name, "no such tool"))?;
    let started = std::time::Instant::now();
    let r = if matches!(t.effect, Effect::Read | Effect::InPlace) {
        run_on(session, t, args)
    } else {
        let mut work = session.clone();
        let r = run_on(&mut work, t, args);
        if r.is_ok() {
            finish(session, t, work);
        }
        r
    };
    let via = if session.hosted {
        super::log::Via::App
    } else {
        super::log::Via::Headless
    };
    super::log::record(via, t, args, &r, started.elapsed().as_millis());
    r
}

// ---- project tools -------------------------------------------------------

fn summary(s: &Session) -> Value {
    let active = s
        .active
        .and_then(|id| s.project.document(id))
        .map(|d| d.name.clone());
    json!({
        "project": s.project.name,
        "path": s.path.as_ref().map(|p| p.display().to_string()),
        "active": active,
        "documents": s.project.documents().iter().map(|d| json!({
            "name": d.name,
            "kind": match d.data { DocumentData::Part(_) => "part", DocumentData::Assembly(_) => "assembly", DocumentData::Drawing(_) => "drawing" },
        })).collect::<Vec<_>>(),
    })
}

fn status(s: &mut Session, _: &Args) -> Result<Value, ApiError> {
    let mut v = summary(s);
    v["unsaved"] = json!(s.dirty);
    v["undo"] = json!(s.history.undo_label());
    v["redo"] = json!(s.history.redo_label());
    v["running"] = json!(running_jobs());
    Ok(v)
}

/// The work running now, one line each (`Extrude: building Extrusion2 (2 of 5)`, seconds).
pub fn running_jobs() -> Vec<Value> {
    crate::part::progress::running()
        .iter()
        .map(|j| json!({"job": j.text(), "seconds": (j.elapsed.as_secs_f64() * 10.0).round() / 10.0}))
        .collect()
}

fn cancel_idle(_: &mut Session, _: &Args) -> Result<Value, ApiError> {
    Ok(json!({"cancelled": null, "note": "nothing was running"}))
}

/// `path` resolved, and refused outside `CADCRAFT3D_MCP_ROOT` when that is set (spec 0.7 §3.6).
pub fn checked_path(a: &Args, path: &str) -> Result<PathBuf, ApiError> {
    let p = PathBuf::from(path);
    let p = if p.is_absolute() {
        p
    } else {
        std::env::current_dir()
            .map_err(|e| a.err(e.to_string()))?
            .join(p)
    };
    if let Some(root) = std::env::var_os("CADCRAFT3D_MCP_ROOT") {
        let root =
            std::fs::canonicalize(&root).map_err(|e| a.err(format!("CADCRAFT3D_MCP_ROOT: {e}")))?;
        let dir = p.parent().unwrap_or(Path::new("."));
        let dir =
            std::fs::canonicalize(dir).map_err(|e| a.err(format!("{}: {e}", dir.display())))?;
        if !dir.starts_with(&root) {
            return Err(a.err(format!(
                "{} is outside the allowed folder {}",
                p.display(),
                root.display()
            )));
        }
    }
    Ok(p)
}

fn project_new(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let name = a.opt_str("name")?.unwrap_or("Untitled");
    *s = Session::new(Project::new(name));
    Ok(summary(s))
}

fn project_open(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let path = checked_path(a, a.str("path")?)?;
    let mut project = Project::load(&path).map_err(|e| a.err(e.to_string()))?;
    let mut sick = Vec::new();
    for d in project.documents().iter().map(|d| d.id).collect::<Vec<_>>() {
        if let Some(doc) = project.document_mut(d)
            && let DocumentData::Part(p) = &mut doc.data
        {
            if let Err(e) = p.regenerate() {
                sick.push(format!("{}: {e}", doc.name));
            }
            for (f, msg) in &p.feature_health {
                sick.push(format!("{}/{f}: {msg}", doc.name));
            }
        }
    }
    *s = Session::new(project);
    s.path = Some(crate::document::library::without_verbatim(path.clone()));
    let mut v = summary(s);
    v["health"] = json!(sick);
    Ok(v)
}

fn project_save(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let path = match a.opt_str("path")? {
        Some(p) => checked_path(a, p)?,
        None => s
            .path
            .clone()
            .ok_or_else(|| a.err("the project has no file yet: give 'path'"))?,
    };
    s.project.save(&path).map_err(|e| a.err(e.to_string()))?;
    s.path = Some(path.clone());
    s.dirty = false;
    Ok(json!({"path": path.display().to_string()}))
}

fn document_new(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let name = a.str("name")?.trim();
    if name.is_empty() {
        return Err(a.err("the name is empty"));
    }
    // A repeated name is an error, never a silent rename (ROADMAP E).
    if s.project.name_taken(name, None) {
        return Err(a.err(format!("a document named '{name}' exists already")));
    }
    let data = match a.choice("kind", &["part", "assembly"], "part")? {
        "assembly" => DocumentData::Assembly(AssemblyDocument::default()),
        _ => DocumentData::Part(PartDocument::default()),
    };
    let id = s.project.add_document(name, data);
    s.active = Some(id);
    Ok(json!({"document": name, "active": name}))
}

fn document_activate(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let id = s.doc_id(a.cmd, Some(a.str("name")?))?;
    s.active = Some(id);
    Ok(summary(s))
}

fn undo(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let label = s.history.undo_label().map(str::to_owned);
    let snap = s.snapshot();
    let back = s
        .history
        .undo(snap)
        .ok_or_else(|| a.err("nothing to undo"))?;
    s.restore(back);
    s.dirty = true;
    Ok(json!({"undone": label}))
}

fn redo(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let label = s.history.redo_label().map(str::to_owned);
    let snap = s.snapshot();
    let back = s
        .history
        .redo(snap)
        .ok_or_else(|| a.err("nothing to redo"))?;
    s.restore(back);
    s.dirty = true;
    Ok(json!({"redone": label}))
}

fn batch(s: &mut Session, a: &Args) -> Result<Value, ApiError> {
    let list = a
        .get("commands")
        .and_then(Value::as_array)
        .ok_or_else(|| a.err("'commands' must be a list of {tool, arguments}"))?;
    let mut out = Vec::new();
    for (i, c) in list.iter().enumerate() {
        let name = c.get("tool").and_then(Value::as_str).unwrap_or("");
        let fail = |reason: String| {
            ApiError::new(
                "Batch",
                format!("entry {i} ({name}) failed, nothing was changed: {reason}"),
            )
        };
        let t = find(name).ok_or_else(|| fail("no such tool".into()))?;
        if !matches!(t.effect, Effect::Read | Effect::Change) || t.name == "batch" {
            return Err(fail(
                "only part, sketch, assembly and query tools can be batched".into(),
            ));
        }
        let args = c.get("arguments").cloned().unwrap_or_else(|| json!({}));
        out.push(run_on(s, t, &args).map_err(|e| fail(e.to_string()))?);
    }
    Ok(json!({"results": out}))
}
