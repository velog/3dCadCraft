//! The MCP server and its tools (`docs/spec/0.7-api-mcp.md`): every tool through
//! `api::call`, the ROADMAP E plate over the real protocol (JSON-RPC lines through pipes),
//! batch atomicity and Undo.

use std::f64::consts::PI;
use std::io::{BufRead, BufReader, Write};

use cadcraft3d::api::{Session, call, mcp};
use cadcraft3d::document::{DocumentData, Project};
use serde_json::{Value, json};

fn ok(s: &mut Session, tool: &str, args: Value) -> Value {
    match call(s, tool, &args) {
        Ok(v) => v,
        Err(e) => panic!("{tool} {args}: {e}"),
    }
}

fn err(s: &mut Session, tool: &str, args: Value) -> String {
    match call(s, tool, &args) {
        Ok(v) => panic!("{tool} should fail, got {v}"),
        Err(e) => e.to_string(),
    }
}

fn h(v: &Value, key: &str, k: usize) -> String {
    v[key][k].as_str().unwrap().to_owned()
}

/// A fully dimensioned w × d rectangle on XY extruded `t`: the session and the corner handles.
fn plate(w: f64, d: f64, t: f64) -> Session {
    let mut s = Session::default();
    ok(
        &mut s,
        "document_new",
        json!({"kind": "part", "name": "Plate"}),
    );
    let sk = ok(&mut s, "sketch_create", json!({"plane": "XY"}));
    assert_eq!(sk["sketch"], "Sketch1");
    let origin = h(&sk, "projected_points", 0);
    let r = ok(
        &mut s,
        "sketch_rectangle",
        json!({"sketch": "Sketch1", "corner1": [1, 2], "corner2": [w - 3.0, d + 4.0]}),
    );
    let p: Vec<String> = (0..4).map(|k| h(&r, "points", k)).collect();
    ok(
        &mut s,
        "sketch_constrain",
        json!({"sketch": "Sketch1", "constraint": {"type": "Coincident", "a": p[0], "b": origin}}),
    );
    let c = ok(
        &mut s,
        "sketch_constrain",
        json!({"sketch": "Sketch1", "constraint": {"type": "Distance", "a": p[0], "b": p[1], "value": w}}),
    );
    assert_eq!(c["parameter"], "d0");
    let c = ok(
        &mut s,
        "sketch_constrain",
        json!({"sketch": "Sketch1", "constraint": {"type": "Distance", "a": p[1], "b": p[2], "value": d}}),
    );
    assert_eq!(c["dof"], 0, "{c}");
    let e = ok(
        &mut s,
        "extrude",
        json!({"sketch": "Sketch1", "distance": t}),
    );
    assert_eq!(e["feature"], "Extrusion1");
    s
}

fn volume(s: &mut Session) -> f64 {
    ok(s, "physical", json!({}))["volume"].as_f64().unwrap()
}

#[test]
fn sketch_extrude_and_queries() {
    let mut s = plate(50.0, 30.0, 10.0);
    assert!((volume(&mut s) - 15000.0).abs() < 1e-3);
    let f = ok(&mut s, "features", json!({}));
    assert_eq!(f["features"][0]["id"], "Extrusion1");
    assert_eq!(f["features"][0]["type"], "Extrude");
    assert_eq!(f["sketches"][0]["dof"], 0);
    let faces = ok(&mut s, "faces", json!({"detail": true}));
    let top = faces["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Extrusion1:End")
        .unwrap()
        .clone();
    assert_eq!(top["surface"], "plane");
    assert!((top["area"].as_f64().unwrap() - 1500.0).abs() < 1e-6);
    assert_eq!(top["normal"], json!([0.0, 0.0, 1.0]));
    let edges = ok(&mut s, "edges", json!({"face": "Extrusion1:End"}));
    assert_eq!(edges["edges"].as_array().unwrap().len(), 4, "{edges}");
    let m = ok(
        &mut s,
        "measure",
        json!({"a": "face:Extrusion1:End", "b": "face:Extrusion1:Start"}),
    );
    assert!((m["distance"].as_f64().unwrap() - 10.0).abs() < 1e-6, "{m}");
    let info = ok(
        &mut s,
        "sketch_info",
        json!({"sketch": "Sketch1", "detail": true}),
    );
    assert_eq!(info["regions"].as_array().unwrap().len(), 1);
    // A repeated dimension over-constrains: refused, nothing changes.
    let p0 = info["curves"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "line" && c["projected"].is_null())
        .unwrap()["points"]
        .clone();
    let n = info["constraints"].as_array().unwrap().len();
    let e = err(
        &mut s,
        "sketch_constrain",
        json!({"sketch": "Sketch1", "constraint": {"type": "Distance", "a": p0[0], "b": p0[1], "value": 50}}),
    );
    assert!(e.starts_with("Constraint: "), "{e}");
    let info = ok(
        &mut s,
        "sketch_info",
        json!({"sketch": "Sketch1", "detail": true}),
    );
    assert_eq!(info["constraints"].as_array().unwrap().len(), n);
}

#[test]
fn parameters_drive_the_part() {
    let mut s = plate(50.0, 30.0, 10.0);
    let p = ok(&mut s, "parameters", json!({}));
    assert!(
        p["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "d0" && r["value"] == 50.0),
        "{p}"
    );
    ok(
        &mut s,
        "parameter_set",
        json!({"name": "d0", "expression": "80 mm"}),
    );
    assert!((volume(&mut s) - 24000.0).abs() < 1e-3);
    let e = err(
        &mut s,
        "parameter_set",
        json!({"name": "d0", "expression": "nonsense +"}),
    );
    assert!(e.starts_with("Parameters: "), "{e}");
    assert!((volume(&mut s) - 24000.0).abs() < 1e-3);
    // Undo returns the 50 mm plate, Redo the 80 mm one.
    ok(&mut s, "undo", json!({}));
    assert!((volume(&mut s) - 15000.0).abs() < 1e-3);
    ok(&mut s, "redo", json!({}));
    assert!((volume(&mut s) - 24000.0).abs() < 1e-3);
}

#[test]
fn holes_pattern_fillet() {
    let mut s = plate(120.0, 80.0, 10.0);
    let h = ok(
        &mut s,
        "hole",
        json!({"face": "Extrusion1:End", "centers": [[20, 20, 10]], "type": "clearance", "size": "M8"}),
    );
    assert_eq!(h["feature"], "Hole1");
    let r = 4.5; // ISO 273 normal M8: 9 mm
    // Physical Properties integrate the display mesh (0.1 mm): a bore's inscribed polygon
    // leaves ~0.2 % of its volume in the part.
    let bore = PI * r * r * 10.0;
    let v0 = 96000.0 - bore;
    assert!(
        (volume(&mut s) - v0).abs() < 0.003 * bore,
        "{}",
        volume(&mut s)
    );
    let p = ok(
        &mut s,
        "pattern",
        json!({"features": ["Hole1"], "kind": "rectangular", "direction": "X", "count": 2, "spacing": 80,
               "direction2": "Y", "count2": 2, "spacing2": 40}),
    );
    assert_eq!(p["feature"], "RectangularPattern1", "{p}");
    let v1 = 96000.0 - 4.0 * PI * r * r * 10.0;
    assert!(
        (volume(&mut s) - v1).abs() < 0.003 * 4.0 * bore,
        "{}",
        volume(&mut s)
    );
    // A vertical corner edge: the two side faces meeting at the origin.
    let edges = ok(&mut s, "edges", json!({"feature": "Extrusion1"}));
    let corner = edges["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e[0].as_str().unwrap().to_owned())
        .find(|e| !e.contains("End") && !e.contains("Start"))
        .unwrap();
    let f = ok(&mut s, "fillet", json!({"edges": [corner], "radius": 5}));
    assert_eq!(f["feature"], "Fillet1");
    let v2 = v1 - (25.0 - PI * 25.0 / 4.0) * 10.0;
    assert!(
        (volume(&mut s) - v2).abs() < 0.003 * 4.0 * bore + 0.5,
        "{} vs {v2}",
        volume(&mut s)
    );
    let e = err(
        &mut s,
        "fillet",
        json!({"edges": ["Nope:A|Nope:B"], "radius": 1}),
    );
    assert!(e.starts_with("Fillet: no edge"), "{e}");
}

#[test]
fn chamfer_and_shell() {
    let mut s = plate(50.0, 30.0, 20.0);
    let top = ok(&mut s, "edges", json!({"face": "Extrusion1:End"}));
    let first = top["edges"][0].clone();
    assert_eq!(first[1], "line");
    let len = first[2].as_f64().unwrap();
    let c = ok(
        &mut s,
        "chamfer",
        json!({"edges": [first[0]], "distance": 2}),
    );
    assert_eq!(c["feature"], "Chamfer1");
    let v = volume(&mut s);
    assert!((30000.0 - v - 2.0 * len).abs() < 1e-3, "{v}");
    ok(&mut s, "undo", json!({}));
    let sh = ok(
        &mut s,
        "shell",
        json!({"remove_faces": ["Extrusion1:End"], "thickness": 2}),
    );
    assert_eq!(sh["feature"], "Shell1");
    let want = 30000.0 - 46.0 * 26.0 * 18.0;
    assert!((volume(&mut s) - want).abs() < 1e-2, "{}", volume(&mut s));
}

#[test]
fn revolve_and_circle() {
    let mut s = Session::default();
    ok(
        &mut s,
        "document_new",
        json!({"kind": "part", "name": "Disc"}),
    );
    ok(&mut s, "sketch_create", json!({"plane": "XZ"}));
    // A 10 × 5 rectangle 20 mm off the Z axis, revolved: a ring.
    ok(
        &mut s,
        "sketch_rectangle",
        json!({"sketch": "Sketch1", "corner1": [20, 0], "corner2": [30, 5]}),
    );
    ok(&mut s, "revolve", json!({"sketch": "Sketch1", "axis": "Z"}));
    let want = PI * (30.0f64.powi(2) - 20.0f64.powi(2)) * 5.0;
    let v = volume(&mut s);
    assert!((v - want).abs() < want * 2e-3, "{v} vs {want}");
    // A circle extruded on the ring's top face, cut through.
    let sk = ok(&mut s, "sketch_create", json!({"plane": "XY"}));
    let id = sk["sketch"].as_str().unwrap().to_owned();
    ok(
        &mut s,
        "sketch_circle",
        json!({"sketch": id, "center": [25, 0], "radius": 2}),
    );
    ok(
        &mut s,
        "extrude",
        json!({"sketch": id, "distance": 20, "operation": "cut", "direction": "symmetric"}),
    );
    let v2 = volume(&mut s);
    assert!((v - v2 - PI * 4.0 * 5.0).abs() < 0.5, "{v} {v2}");
}

#[test]
fn batch_is_atomic_and_names_the_entry() {
    let mut s = plate(50.0, 30.0, 10.0);
    let before = ok(&mut s, "features", json!({}));
    let e = err(
        &mut s,
        "batch",
        json!({"commands": [
            {"tool": "sketch_create", "arguments": {"face": "Extrusion1:End"}},
            {"tool": "sketch_circle", "arguments": {"sketch": "Sketch2", "center": [10, 10], "radius": 3}},
            {"tool": "extrude", "arguments": {"sketch": "Sketch9", "distance": 3}},
        ]}),
    );
    assert!(
        e.starts_with("Batch: entry 2 (extrude) failed, nothing was changed: Extrude: no sketch"),
        "{e}"
    );
    assert_eq!(ok(&mut s, "features", json!({})), before);
    // The same batch with the right sketch is one Undo step.
    let r = ok(
        &mut s,
        "batch",
        json!({"commands": [
            {"tool": "sketch_create", "arguments": {"face": "Extrusion1:End"}},
            {"tool": "sketch_circle", "arguments": {"sketch": "Sketch2", "center": [10, 10], "radius": 3}},
            {"tool": "extrude", "arguments": {"sketch": "Sketch2", "distance": 3}},
        ]}),
    );
    assert_eq!(r["results"][2]["feature"], "Extrusion2");
    assert_eq!(ok(&mut s, "status", json!({}))["undo"], "Batch");
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "features", json!({})), before);
}

#[test]
fn names_are_never_reused_silently() {
    let mut s = Session::default();
    ok(&mut s, "document_new", json!({"kind": "part", "name": "A"}));
    let e = err(&mut s, "document_new", json!({"kind": "part", "name": "a"}));
    assert_eq!(e, "New Document: a document named 'a' exists already");
    let e = err(
        &mut s,
        "extrude",
        json!({"sketch": "Sketch1", "distance": 1}),
    );
    assert_eq!(e, "Extrude: no sketch 'Sketch1'");
}

#[test]
fn assembly_place_and_mate() {
    let mut s = plate(50.0, 30.0, 10.0);
    ok(
        &mut s,
        "document_new",
        json!({"kind": "assembly", "name": "Stack"}),
    );
    let a = ok(&mut s, "place", json!({"component": "Plate"}));
    assert_eq!(a["occurrence"], "Plate:1");
    assert_eq!(a["grounded"], true);
    let b = ok(
        &mut s,
        "place",
        json!({"component": "Plate", "at": [5, 40, 30]}),
    );
    assert_eq!(b["occurrence"], "Plate:2");
    // The second plate's bottom on the first one's top.
    let r = ok(
        &mut s,
        "constrain",
        json!({"kind": "mate",
               "a": {"occurrence": "Plate:1", "face": "Extrusion1:End"},
               "b": {"occurrence": "Plate:2", "face": "Extrusion1:Start"}}),
    );
    assert_eq!(r["converged"], true, "{r}");
    let c = ok(&mut s, "components", json!({}));
    let z = c["components"][1]["at"][2].as_f64().unwrap();
    assert!((z - 10.0).abs() < 1e-6, "{c}");
    assert_eq!(c["relationships"][0]["kind"], "Mate");
}

#[test]
fn export_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = plate(20.0, 10.0, 5.0);
    for ext in ["stl", "step", "glb"] {
        let p = dir.path().join(format!("plate.{ext}"));
        let v = ok(&mut s, "export", json!({"path": p.display().to_string()}));
        assert!(v["bytes"].as_u64().unwrap() > 100, "{ext}: {v}");
    }
    let step = std::fs::read_to_string(dir.path().join("plate.step")).unwrap();
    assert!(step.starts_with("ISO-10303-21"));
}

// ---- over the protocol -------------------------------------------------------

struct Client {
    to: std::io::PipeWriter,
    from: BufReader<std::io::PipeReader>,
    id: u64,
}

impl Client {
    fn start() -> Self {
        let (in_r, in_w) = std::io::pipe().unwrap();
        let (out_r, out_w) = std::io::pipe().unwrap();
        std::thread::spawn(move || mcp::serve(BufReader::new(in_r), out_w, Session::default()));
        Self {
            to: in_w,
            from: BufReader::new(out_r),
            id: 0,
        }
    }

    fn send(&mut self, v: Value) {
        let mut line = v.to_string();
        line.push('\n');
        self.to.write_all(line.as_bytes()).unwrap();
    }

    /// Sends a request and reads lines until its answer (progress notifications skipped).
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(self.from.read_line(&mut line).unwrap() > 0, "server closed");
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        assert_eq!(r["result"]["isError"], false, "{name}: {r}");
        r["result"]["structuredContent"].clone()
    }
}

/// ROADMAP E: "a 120 × 80 × 10 plate with 4 M8 holes 20 mm from the edges", saved and read back.
#[test]
fn roadmap_plate_over_the_protocol() {
    let mut c = Client::start();
    let init = c.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "3dcadcraft");
    c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    let list = c.request("tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for t in [
        "sketch_create",
        "sketch_rectangle",
        "sketch_constrain",
        "extrude",
        "hole",
        "project_save",
    ] {
        assert!(names.contains(&t), "{t} missing in {names:?}");
    }
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));

    c.tool("document_new", json!({"kind": "part", "name": "Plate"}));
    let sk = c.tool("sketch_create", json!({"plane": "XY"}));
    let origin = sk["projected_points"][0].as_str().unwrap().to_owned();
    let r = c.tool(
        "sketch_rectangle",
        json!({"sketch": "Sketch1", "corner1": [0, 0], "corner2": [118, 83]}),
    );
    let p: Vec<String> = (0..3).map(|k| h(&r, "points", k)).collect();
    for con in [
        json!({"type": "Coincident", "a": p[0], "b": origin}),
        json!({"type": "Distance", "a": p[0], "b": p[1], "value": 120}),
        json!({"type": "Distance", "a": p[1], "b": p[2], "value": 80}),
    ] {
        c.tool(
            "sketch_constrain",
            json!({"sketch": "Sketch1", "constraint": con}),
        );
    }
    let info = c.tool("sketch_info", json!({"sketch": "Sketch1"}));
    assert_eq!(info["dof"], 0);
    c.tool("extrude", json!({"sketch": "Sketch1", "distance": 10}));
    let hole = c.tool(
        "hole",
        json!({"face": "Extrusion1:End", "type": "clearance", "size": "M8",
               "centers": [[20, 20, 10], [100, 20, 10], [20, 60, 10], [100, 60, 10]]}),
    );
    assert_eq!(hole["holes"], 4);
    // A failing call is a tool error, not a protocol error, and changes nothing.
    let bad = c.request("tools/call", json!({"name": "hole", "arguments": {"face": "Nope", "centers": [[0, 0, 0]], "type": "simple", "diameter": 3, "depth": 1}}));
    assert_eq!(bad["result"]["isError"], true);
    assert_eq!(
        bad["result"]["content"][0]["text"],
        "Hole: no face 'Nope' (see faces)"
    );
    let unknown = c.request("tools/call", json!({"name": "nope", "arguments": {}}));
    assert_eq!(unknown["error"]["code"], -32602);

    let res = c.request(
        "resources/read",
        json!({"uri": "cadcraft://part/Plate/features"}),
    );
    let text = res["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("Hole1"), "{text}");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plate.3dcadcraft");
    c.tool("project_save", json!({"path": path.display().to_string()}));
    let st = c.tool("status", json!({}));
    assert_eq!(st["unsaved"], false);

    // Read back with the file format alone.
    let mut project = Project::load(&path).unwrap();
    let doc = project.documents()[0].id;
    let DocumentData::Part(part) = &mut project.document_mut(doc).unwrap().data else {
        panic!("not a part");
    };
    part.regenerate().unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    assert_eq!(part.holes().len(), 4);
    let v = cadcraft3d::part::physical::physical_properties(part, Default::default())
        .unwrap()
        .total
        .volume;
    let bores = 4.0 * PI * 4.5 * 4.5 * 10.0;
    let want = 120.0 * 80.0 * 10.0 - bores;
    assert!((v - want).abs() < 0.003 * bores, "{v} vs {want}");

    // Opened by the server again.
    c.tool("project_new", json!({}));
    let o = c.tool("project_open", json!({"path": path.display().to_string()}));
    assert_eq!(o["documents"][0]["name"], "Plate");
    let f = c.tool("features", json!({}));
    assert_eq!(f["features"][1]["id"], "Hole1");
}

/// Spec 0.7 §3.4a: aliases and descriptions of faces, aliases accepted everywhere, answers in
/// persistent names, `face_at`.
#[test]
fn face_aliases_and_face_at() {
    let mut s = plate(50.0, 30.0, 10.0);
    let faces = ok(&mut s, "faces", json!({}));
    let row = |name: &str| -> Value {
        faces["faces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r[0] == name)
            .unwrap_or_else(|| panic!("no {name} in {faces}"))
            .clone()
    };
    assert_eq!(
        row("Extrusion1:End"),
        json!(["Extrusion1:End", "plane", "Top", "+Z"])
    );
    assert_eq!(row("Extrusion1:Start")[2], "Bottom");
    assert_eq!(row("Extrusion1:Start")[3], "-Z");
    let detail = ok(&mut s, "faces", json!({"detail": true}));
    let top = detail["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Extrusion1:End")
        .unwrap()
        .clone();
    let aliases: Vec<&str> = top["aliases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(aliases, ["Top", "+Z", "Extrusion1:Top", "Extrusion1:+Z"]);
    // The +X side: one face of the feature with that normal.
    let right = detail["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["dir"] == "+X")
        .unwrap()["name"]
        .as_str()
        .unwrap()
        .to_owned();
    // Aliases are accepted by face arguments; answers give persistent names.
    let m = ok(&mut s, "measure", json!({"a": "face:Top", "b": "Bottom"}));
    assert!((m["distance"].as_f64().unwrap() - 10.0).abs() < 1e-6, "{m}");
    let m = ok(
        &mut s,
        "measure",
        json!({"a": "Extrusion1:Left", "b": "+X"}),
    );
    assert!((m["distance"].as_f64().unwrap() - 50.0).abs() < 1e-6, "{m}");
    let edges = ok(&mut s, "edges", json!({"face": "Top"}));
    assert_eq!(edges["edges"].as_array().unwrap().len(), 4, "{edges}");
    let e = edges["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e[3] == "Top|Right" || e[3] == "Right|Top")
        .unwrap_or_else(|| panic!("no Top|Right alias in {edges}"))
        .clone();
    assert!(e[0].as_str().unwrap().contains(&right), "{e}");
    let sk = ok(&mut s, "sketch_create", json!({"face": "Top"}));
    assert_eq!(sk["origin"][2], 10.0, "{sk}");
    let f = ok(
        &mut s,
        "fillet",
        json!({"edges": ["Right|Top"], "radius": 2}),
    );
    assert_eq!(f["feature"], "Fillet1", "{f}");
    let e = err(&mut s, "measure", json!({"a": "Nowhere"}));
    assert!(e.contains("no face 'Nowhere'"), "{e}");
    // face_at: a ray down onto the plate, and the nearest face to a point beside it.
    let hit = ok(
        &mut s,
        "face_at",
        json!({"point": [25, 15, 50], "direction": [0, 0, -1]}),
    );
    assert_eq!(hit["name"], "Extrusion1:End", "{hit}");
    assert!(
        (hit["distance"].as_f64().unwrap() - 40.0).abs() < 1e-6,
        "{hit}"
    );
    assert_eq!(hit["point"], json!([25.0, 15.0, 10.0]));
    let near = ok(&mut s, "face_at", json!({"point": [25, -7, 5]}));
    assert_eq!(near["dir"], "-Y", "{near}");
    assert!(
        (near["distance"].as_f64().unwrap() - 7.0).abs() < 1e-6,
        "{near}"
    );
    let e = err(
        &mut s,
        "face_at",
        json!({"point": [25, 15, 50], "direction": [0, 0, 1]}),
    );
    assert!(e.contains("meets no face"), "{e}");
}

/// Spec 0.7 §3.3: work features, sketches on work planes, loft, sweep, mirror.
#[test]
fn work_features_loft_sweep_mirror() {
    let mut s = Session::default();
    ok(
        &mut s,
        "document_new",
        json!({"kind": "part", "name": "Cone"}),
    );
    let axis = ok(
        &mut s,
        "work_axis",
        json!({"method": "planes", "plane": "XZ", "plane2": "YZ"}),
    );
    assert_eq!(axis["feature"], "WorkAxis1", "{axis}");
    assert_eq!(
        axis["direction"].as_array().unwrap()[2]
            .as_f64()
            .unwrap()
            .abs(),
        1.0
    );
    let pt = ok(
        &mut s,
        "work_point",
        json!({"method": "planes", "planes": ["XY", "XZ", "YZ"]}),
    );
    assert_eq!(pt["point"], json!([0.0, 0.0, 0.0]), "{pt}");
    let wp = ok(&mut s, "work_plane", json!({"plane": "XY", "distance": 30}));
    assert_eq!(wp["feature"], "WorkPlane1", "{wp}");
    assert_eq!(wp["origin"][2], 30.0, "{wp}");
    ok(&mut s, "sketch_create", json!({"plane": "XY"}));
    ok(
        &mut s,
        "sketch_circle",
        json!({"sketch": "Sketch1", "center": [0, 0], "radius": 10}),
    );
    let top = ok(&mut s, "sketch_create", json!({"plane": "WorkPlane1"}));
    assert_eq!(top["origin"][2], 30.0, "{top}");
    ok(
        &mut s,
        "sketch_circle",
        json!({"sketch": "Sketch2", "center": [0, 0], "radius": 5}),
    );
    let l = ok(
        &mut s,
        "loft",
        json!({"sections": [{"sketch": "Sketch1"}, {"sketch": "Sketch2"}]}),
    );
    assert_eq!(l["feature"], "Loft1", "{l}");
    let frustum = PI * 30.0 / 3.0 * (100.0 + 50.0 + 25.0);
    let v = volume(&mut s);
    assert!(
        (v - frustum).abs() < 0.01 * frustum,
        "loft {v} vs {frustum}"
    );
    let e = err(&mut s, "loft", json!({"sections": [{"sketch": "Sketch1"}]}));
    assert!(e.starts_with("Loft: "), "{e}");

    // Sweep: a circle on XZ along a line on XY (a rod along +Y).
    let mut s = Session::default();
    ok(
        &mut s,
        "document_new",
        json!({"kind": "part", "name": "Rod"}),
    );
    ok(&mut s, "sketch_create", json!({"plane": "XZ"}));
    ok(
        &mut s,
        "sketch_circle",
        json!({"sketch": "Sketch1", "center": [0, 0], "radius": 2}),
    );
    ok(&mut s, "sketch_create", json!({"plane": "XY"}));
    let line = ok(
        &mut s,
        "sketch_line",
        json!({"sketch": "Sketch2", "from": [0, 0], "to": [0, 40]}),
    );
    let sw = ok(
        &mut s,
        "sweep",
        json!({"sketch": "Sketch1", "path_sketch": "Sketch2", "path_curves": [line["curves"][0]]}),
    );
    assert_eq!(sw["feature"], "Sweep1", "{sw}");
    let rod = PI * 4.0 * 40.0;
    let v = volume(&mut s);
    assert!((v - rod).abs() < 0.01 * rod, "sweep {v} vs {rod}");

    // Mirror a boss about a work plane through the middle of the plate.
    let mut s = plate(50.0, 30.0, 10.0);
    ok(&mut s, "sketch_create", json!({"face": "Top"}));
    ok(
        &mut s,
        "sketch_circle",
        json!({"sketch": "Sketch2", "center": [10, 15], "radius": 3}),
    );
    let boss = ok(
        &mut s,
        "extrude",
        json!({"sketch": "Sketch2", "distance": 5}),
    );
    let before = volume(&mut s);
    let mid = ok(&mut s, "work_plane", json!({"plane": "YZ", "distance": 25}));
    let m = ok(
        &mut s,
        "mirror",
        json!({"features": [boss["feature"]], "plane": mid["feature"]}),
    );
    assert!(m["feature"].as_str().unwrap().starts_with("Mirror"), "{m}");
    let after = volume(&mut s);
    let cyl = PI * 9.0 * 5.0;
    assert!(
        (after - before - cyl).abs() < 0.01 * cyl,
        "mirror adds {}",
        after - before
    );
    let e = err(
        &mut s,
        "mirror",
        json!({"features": ["Nope1"], "plane": "XY"}),
    );
    assert!(e.contains("no feature 'Nope1'"), "{e}");
}

/// Spec 0.7 §3.6: a read-only session lists and runs only reading tools.
#[test]
fn read_only_runs_only_queries() {
    let mut s = plate(20.0, 10.0, 5.0);
    s.read_only = true;
    let e = err(
        &mut s,
        "extrude",
        json!({"sketch": "Sketch1", "distance": 5}),
    );
    assert_eq!(e, "Extrude: refused, the server is read-only");
    let e = err(&mut s, "project_save", json!({"path": "x.3dcadcraft"}));
    assert!(e.ends_with("read-only"), "{e}");
    ok(&mut s, "faces", json!({}));
    ok(&mut s, "physical", json!({}));
    let names: Vec<&str> = cadcraft3d::api::tools::listed(&s)
        .iter()
        .map(|t| t.name)
        .collect();
    assert!(
        names.contains(&"faces") && names.contains(&"project_open"),
        "{names:?}"
    );
    assert!(
        !names.contains(&"extrude") && !names.contains(&"batch"),
        "{names:?}"
    );
    // The app's session refuses replacing its project, also when not read-only.
    s.read_only = false;
    s.hosted = true;
    let e = err(&mut s, "project_new", json!({}));
    assert!(e.contains("attached to the app"), "{e}");
}

/// Spec 0.7 §3.5: the Component Library's families are listed.
#[test]
fn library_lists_families() {
    let mut s = Session::default();
    let all = ok(&mut s, "library", json!({}));
    let n = all["families"].as_array().unwrap().len();
    assert!(n > 5, "{all}");
    let first = all["families"][0][1].as_str().unwrap().to_owned();
    let fam = ok(&mut s, "library", json!({"family": first}));
    assert!(!fam["sizes"].as_array().unwrap().is_empty(), "{fam}");
    let e = err(
        &mut s,
        "library_place",
        json!({"family": first, "size": "nope"}),
    );
    assert!(e.starts_with("Place from Library: "), "{e}");
}
