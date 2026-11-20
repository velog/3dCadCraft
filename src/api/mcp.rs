//! The MCP server: JSON-RPC 2.0 messages, one per line, over stdin / stdout (spec 0.7 §2).
//!
//! A reader thread hands every incoming message to the loop here. A `tools/call` runs on a
//! worker thread on a copy of the session while the loop keeps answering: `ping`, the `status`
//! and `cancel` tools, `notifications/cancelled` for the running call, and progress
//! notifications once a second when the client asked for them. A cancelled call's copy is
//! dropped, so the project stays as it was (rule 5); the next call waits for nothing.

use std::collections::VecDeque;
use std::io::{self, BufRead, Write};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use serde_json::{Value, json};

use super::part::tree;
use super::session::{ApiError, Session};
use super::tools::{self, Effect, Tool};
use crate::document::DocumentData;

/// Protocol versions this server speaks, newest first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// What the loop waits for.
enum Event {
    /// A message from the client.
    In(Value),
    /// A line that was no JSON.
    Bad(String),
    /// The client closed stdin.
    Eof,
    /// A worker finished job `seq`.
    Done {
        seq: u64,
        work: Box<Session>,
        result: Result<Value, ApiError>,
    },
}

/// The call running on a worker.
struct Running {
    seq: u64,
    id: Value,
    tool: &'static Tool,
    token: Option<Value>,
    sent: u64,
    last: String,
    args: Value,
    started: std::time::Instant,
}

impl Running {
    /// Writes the call's line into the action journal (spec 0.7 §3.6a).
    fn log(&self, session: &Session, result: &Result<Value, ApiError>) {
        let via = if session.hosted {
            super::log::Via::App
        } else {
            super::log::Via::Headless
        };
        super::log::record(via, self.tool, &self.args, result, self.started.elapsed().as_millis());
    }
}

const STACK: usize = 64 << 20;

/// Serves MCP on this process's stdin / stdout until stdin closes.
pub fn serve_stdio() -> io::Result<()> {
    serve(
        io::BufReader::new(io::stdin()),
        io::stdout(),
        Session::default(),
    )
}

/// The server's command line (after `--mcp`, or the arguments of `3dcadcraft-mcp`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// `--read-only`: only reading tools (spec 0.7 §3.6).
    pub read_only: bool,
    /// `--attach`: forward to the running app (spec 0.7 §3.9).
    pub attach: bool,
    /// A project to open first (headless).
    pub project: Option<String>,
}

impl Options {
    /// Reads `args` (without the program name); `--mcp` is skipped.
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut o = Self::default();
        for a in args {
            match a.as_str() {
                "--mcp" => {}
                "--read-only" => o.read_only = true,
                "--attach" => o.attach = true,
                s if s.starts_with("--") => return Err(format!("unknown option {s}")),
                _ if o.project.is_some() => return Err(format!("one project only ({a})")),
                _ => o.project = Some(a),
            }
        }
        Ok(o)
    }
}

/// Runs the server as a program: the diagnostics session (log + journal, nothing on stdout),
/// then the bridge to the app (`--attach`, falling back to headless when no app accepts) or the
/// headless server on stdin / stdout.
pub fn run(o: &Options) -> Result<(), String> {
    crate::diagnostics::start(crate::diagnostics::root());
    if o.attach {
        match super::host::attach(o.read_only) {
            Ok(()) => return Ok(()),
            Err(e) => {
                ::log::warn!("--attach: {e}; serving headless");
                eprintln!("3dcadcraft --mcp --attach: {e}; serving headless");
            }
        }
    }
    let mut session = Session {
        read_only: o.read_only,
        ..Session::default()
    };
    if let Some(path) = &o.project {
        tools::call(&mut session, "project_open", &json!({ "path": path }))
            .map_err(|e| e.to_string())?;
    }
    let r = serve(io::BufReader::new(io::stdin()), io::stdout(), session)
        .map_err(|e| e.to_string());
    crate::diagnostics::finish();
    r
}

/// Serves MCP: messages from `input`, answers to `output`, work on `session`.
pub fn serve<R, W>(input: R, mut output: W, mut session: Session) -> io::Result<()>
where
    R: BufRead + Send + 'static,
    W: Write,
{
    let (tx, rx) = mpsc::channel::<Event>();
    let reader = tx.clone();
    std::thread::Builder::new()
        .name("mcp-reader".into())
        .spawn(move || {
            for line in input.lines() {
                let ev = match line {
                    Ok(l) if l.trim().is_empty() => continue,
                    Ok(l) => match serde_json::from_str::<Value>(&l) {
                        Ok(v) => Event::In(v),
                        Err(e) => Event::Bad(e.to_string()),
                    },
                    Err(_) => break,
                };
                if reader.send(ev).is_err() {
                    return;
                }
            }
            let _ = reader.send(Event::Eof);
        })?;
    let mut queue: VecDeque<Value> = VecDeque::new();
    let mut running: Option<Running> = None;
    let mut seq = 0u64;
    loop {
        // Work queued while a call ran goes first, once nothing runs.
        if running.is_none()
            && let Some(msg) = queue.pop_front()
        {
            if let Some(r) = handle(&mut session, &mut output, msg, &tx, &mut seq)? {
                running = Some(r);
            }
            continue;
        }
        let ev = match rx.recv_timeout(Duration::from_millis(1000)) {
            Ok(ev) => ev,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(r) = &mut running {
                    progress(&mut output, r)?;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match ev {
            Event::Eof => break,
            Event::Bad(e) => send(
                &mut output,
                &error(&Value::Null, -32700, &format!("parse error: {e}")),
            )?,
            Event::Done {
                seq: s,
                work,
                result,
            } => {
                let Some(r) = running.take_if(|r| r.seq == s) else {
                    continue; // a cancelled call finished: its copy is dropped
                };
                if result.is_ok() {
                    tools::finish(&mut session, r.tool, *work);
                }
                r.log(&session, &result);
                send(&mut output, &reply(&r.id, tool_result(result)))?;
            }
            Event::In(msg) => {
                if running.is_none() {
                    if let Some(r) = handle(&mut session, &mut output, msg, &tx, &mut seq)? {
                        running = Some(r);
                    }
                    continue;
                }
                match while_running(&mut session, &mut output, &msg, &running)? {
                    Busy::Handled => {}
                    Busy::Queue => queue.push_back(msg),
                    Busy::Cancel { answer } => {
                        if let Some(r) = running.take() {
                            let e = Err(ApiError::new(
                                r.tool.command,
                                "cancelled; the project is as it was before the call",
                            ));
                            r.log(&session, &e);
                            if answer {
                                send(&mut output, &reply(&r.id, tool_result(e)))?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// What to do with a message that arrived while a call runs.
enum Busy {
    /// Answered.
    Handled,
    /// Waits for the running call.
    Queue,
    /// The running call is cancelled; `answer` = send it an error (our `cancel` tool), not
    /// for `notifications/cancelled` (the client gave up on it: MCP wants no answer).
    Cancel { answer: bool },
}

fn while_running<W: Write>(
    session: &mut Session,
    out: &mut W,
    msg: &Value,
    running: &Option<Running>,
) -> io::Result<Busy> {
    let Some(r) = running else {
        return Ok(Busy::Queue);
    };
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let Some(id) = msg.get("id").cloned() else {
        if method == "notifications/cancelled" && msg.pointer("/params/requestId") == Some(&r.id) {
            return Ok(Busy::Cancel { answer: false });
        }
        return Ok(if method.is_empty() {
            Busy::Handled
        } else {
            Busy::Queue
        });
    };
    match (method, msg.pointer("/params/name").and_then(Value::as_str)) {
        ("ping", _) => {
            send(out, &reply(&id, json!({})))?;
            Ok(Busy::Handled)
        }
        ("tools/call", Some("status")) => {
            let mut v = tools::call(session, "status", &json!({}));
            if let Ok(v) = &mut v {
                v["busy"] = json!(r.tool.name);
            }
            send(out, &reply(&id, tool_result(v)))?;
            Ok(Busy::Handled)
        }
        ("tools/call", Some("cancel")) => {
            send(
                out,
                &reply(&id, tool_result(Ok(json!({"cancelled": r.tool.name})))),
            )?;
            Ok(Busy::Cancel { answer: true })
        }
        _ => Ok(Busy::Queue),
    }
}

/// Handles one message while nothing runs; returns the call it started on a worker.
fn handle<W: Write>(
    session: &mut Session,
    out: &mut W,
    msg: Value,
    tx: &Sender<Event>,
    seq: &mut u64,
) -> io::Result<Option<Running>> {
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        return Ok(None); // an answer to nothing we asked: ignored
    };
    let Some(id) = msg.get("id").cloned() else {
        return Ok(None); // notifications: initialized, cancelled (nothing runs)
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let answer = match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list(session)),
        "resources/list" => Ok(json!({"resources": resources(session)})),
        "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
            read_resource(session, uri)
                .map(|text| {
                    json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": text}]})
                })
                .ok_or((-32002, format!("resource not found: {uri}")))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let Some(tool) = tools::find(name) else {
                send(out, &error(&id, -32602, &format!("unknown tool: {name}")))?;
                return Ok(None);
            };
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            if let Err(e) = tools::allowed(session, tool) {
                let e = Err(ApiError::new(tool.command, e));
                let via = if session.hosted {
                    super::log::Via::App
                } else {
                    super::log::Via::Headless
                };
                super::log::record(via, tool, &args, &e, 0);
                send(out, &reply(&id, tool_result(e)))?;
                return Ok(None);
            }
            if tool.effect == Effect::InPlace || tool.name == "status" || tool.name == "cancel" {
                let r = tools::call(session, name, &args);
                send(out, &reply(&id, tool_result(r)))?;
                return Ok(None);
            }
            *seq += 1;
            let job = *seq;
            let mut work = session.clone();
            let logged = args.clone();
            let tx = tx.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("mcp-{name}"))
                .stack_size(STACK)
                .spawn(move || {
                    let _report = crate::part::progress::Reporter::install(tool.command);
                    let result =
                        crate::diagnostics::crash::guard(|| tools::run_on(&mut work, tool, &args))
                            .unwrap_or_else(|_| {
                                Err(ApiError::new(
                                    tool.command,
                                    "internal error (the kernel stopped); nothing was changed",
                                ))
                            });
                    let _ = tx.send(Event::Done {
                        seq: job,
                        work: Box::new(work),
                        result,
                    });
                });
            if let Err(e) = spawned {
                send(out, &error(&id, -32603, &format!("no worker thread: {e}")))?;
                return Ok(None);
            }
            return Ok(Some(Running {
                seq: job,
                id,
                tool,
                token: params.pointer("/_meta/progressToken").cloned(),
                sent: 0,
                last: String::new(),
                args: logged,
                started: std::time::Instant::now(),
            }));
        }
        _ => Err((-32601, format!("method not found: {method}"))),
    };
    match answer {
        Ok(v) => send(out, &reply(&id, v))?,
        Err((code, m)) => send(out, &error(&id, code, &m))?,
    }
    Ok(None)
}

/// The answer to `initialize` (the client's protocol version when we speak it).
pub(crate) fn initialize(params: &Value) -> Value {
    let asked = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("");
    let version = PROTOCOL_VERSIONS
        .iter()
        .find(|v| **v == asked)
        .unwrap_or(&PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}, "resources": {"listChanged": false}},
        "serverInfo": {"name": "3dcadcraft", "title": "3DCadCraft", "version": env!("CARGO_PKG_VERSION")},
        "instructions": INSTRUCTIONS,
    })
}

/// The answer to `tools/list`: the tools `s` may run.
pub(crate) fn tools_list(s: &Session) -> Value {
    json!({"tools": tools::listed(s).iter().map(|t| json!({
        "name": t.name,
        "title": t.command,
        "description": t.description,
        "inputSchema": (t.schema)(),
        "annotations": {"readOnlyHint": t.effect == Effect::Read, "destructiveHint": false, "openWorldHint": false},
    })).collect::<Vec<_>>()})
}

const INSTRUCTIONS: &str = "3DCadCraft: parametric CAD (mm, degrees in arguments). Typical part: document_new {kind: part}, sketch_create {plane: XY}, sketch_rectangle, sketch_constrain (dimensions), extrude, then faces / edges to find persistent names (Extrusion1:End) for hole, fillet, shell and sketches on faces. Every changing call is one Undo step; batch makes several atomic. Errors read '<Command>: <reason>' and change nothing. Answers are compact; pass detail=true where offered. Save with project_save {path: x.3dcadcraft}.";

/// A tool's outcome as an MCP `CallToolResult`.
pub(crate) fn tool_result(r: Result<Value, ApiError>) -> Value {
    match r {
        Ok(v) => json!({
            "content": [{"type": "text", "text": v.to_string()}],
            "structuredContent": v,
            "isError": false,
        }),
        Err(e) => json!({
            "content": [{"type": "text", "text": e.to_string()}],
            "isError": true,
        }),
    }
}

/// Sends a progress notification when the running job's report changed.
fn progress<W: Write>(out: &mut W, r: &mut Running) -> io::Result<()> {
    let Some(token) = r.token.clone() else {
        return Ok(());
    };
    let Some(job) = crate::part::progress::running()
        .into_iter()
        .find(|j| j.job == r.tool.command)
    else {
        return Ok(());
    };
    let text = format!("{} ({:.0} s)", job.text(), job.elapsed.as_secs_f64());
    if text == r.last {
        return Ok(());
    }
    r.sent += 1;
    r.last = text.clone();
    let params = json!({"progressToken": token, "progress": r.sent, "message": text});
    send(
        out,
        &json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": params}),
    )
}

fn resources(s: &Session) -> Vec<Value> {
    let mut v = vec![
        json!({"uri": "cadcraft://project", "name": "project", "title": "Project and documents", "mimeType": "application/json"}),
    ];
    for d in s.project.documents() {
        if matches!(d.data, DocumentData::Part(_)) {
            v.push(json!({"uri": format!("cadcraft://part/{}/features", d.name), "name": format!("{} features", d.name),
                "title": format!("Feature tree of {}", d.name), "mimeType": "application/json"}));
        }
    }
    v
}

fn read_resource(s: &mut Session, uri: &str) -> Option<String> {
    if uri == "cadcraft://project" {
        return tools::call(s, "status", &json!({}))
            .ok()
            .map(|v| v.to_string());
    }
    let name = uri
        .strip_prefix("cadcraft://part/")?
        .strip_suffix("/features")?;
    let d = s.project.documents().iter().find(|d| d.name == name)?;
    match &d.data {
        DocumentData::Part(p) => Some(tree(p).to_string()),
        _ => None,
    }
}

pub(crate) fn reply(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub(crate) fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// One message, one line (serde_json writes no raw newline inside).
pub(crate) fn send<W: Write>(out: &mut W, v: &Value) -> io::Result<()> {
    let mut line = v.to_string();
    line.push('\n');
    out.write_all(line.as_bytes())?;
    out.flush()
}
