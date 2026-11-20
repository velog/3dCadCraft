//! The running app as an MCP server (spec 0.7 §3.9): a local endpoint for the current user, the
//! `--attach` bridge an MCP client starts, and the connections handing tool calls to the app's
//! main thread.
//!
//! - The endpoint is a named pipe (Windows) or a socket file in the settings folder (Unix),
//!   through `interprocess` local sockets. Its name and a random token are written to the
//!   *ticket* `<settings>/mcp-connector.json`, readable by the user only; a client must send the
//!   token first, so another user's process cannot drive the app.
//! - [`attach`] is the bridge: stdin / stdout of the MCP client ↔ the endpoint, line by line.
//! - [`Host`] is the app's side: an accept thread and one thread per connection running the MCP
//!   protocol ([`serve_connection`]); every tool call becomes a [`HostCall`] the main thread
//!   takes with [`Host::try_recv`] and answers with [`HostCall::answer`]. Nothing here touches the
//!   project: the GUI runs the tools (on a background job) and decides what is allowed when.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, Name, Stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::mcp::{error, initialize, reply, send, tool_result, tools_list};
use super::session::{ApiError, Session};
use super::tools::{self, Tool};

/// What a client needs to reach the app: written by the app while it accepts connections.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    /// The endpoint: a pipe name (Windows) or a socket path (Unix).
    pub endpoint: String,
    /// The secret a client sends first.
    pub token: String,
    /// The app's process id (for messages).
    pub pid: u32,
}

/// `<settings>/mcp-connector.json`.
pub fn ticket_path() -> Option<PathBuf> {
    crate::presets::config_dir().map(|d| d.join("mcp-connector.json"))
}

/// The endpoint name of this process for the current user.
pub fn endpoint_for_user() -> String {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    let user: String = user
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let leaf = format!("3dcadcraft-mcp-{user}-{}", std::process::id());
    if cfg!(windows) {
        leaf
    } else {
        crate::presets::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("{leaf}.sock"))
            .display()
            .to_string()
    }
}

fn name_of(endpoint: &str) -> io::Result<Name<'static>> {
    if cfg!(windows) {
        endpoint
            .to_owned()
            .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
    } else {
        PathBuf::from(endpoint).to_fs_name::<interprocess::local_socket::GenericFilePath>()
    }
}

fn write_ticket(path: &Path, t: &Ticket) -> io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(serde_json::to_string(t).unwrap_or_default().as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Reads the ticket at `path`.
pub fn read_ticket(path: &Path) -> Result<Ticket, String> {
    let text = std::fs::read_to_string(path).map_err(|_| {
        "the app does not accept AI connections (Tools > AI Connector > Allow AI connections)"
            .to_owned()
    })?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

// ---------------------------------------------------------------- bridge ---

/// One side of a connection: `&Stream` reads and writes, so both sides share the stream.
pub struct Half(Arc<Stream>);

impl io::Read for Half {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Write for Half {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        (&*self.0).flush()
    }
}

fn halves(stream: Stream) -> (Half, Half) {
    let s = Arc::new(stream);
    (Half(s.clone()), Half(s))
}

/// Connects to the app named by the ticket at `ticket` and sends the hello; the stream is ready
/// for MCP lines.
pub fn connect(ticket: &Path, read_only: bool) -> Result<(BufReader<Half>, Half), String> {
    let t = read_ticket(ticket)?;
    let stream = Stream::connect(name_of(&t.endpoint).map_err(|e| e.to_string())?)
        .map_err(|e| format!("the app (process {}) does not answer: {e}", t.pid))?;
    let (rh, mut sh) = halves(stream);
    let mut rd = BufReader::new(rh);
    let hello = json!({"hello": t.token, "read_only": read_only});
    writeln!(sh, "{hello}")
        .and_then(|()| sh.flush())
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    rd.read_line(&mut line).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(line.trim()).map_err(|_| "the app did not answer".to_owned())?;
    if let Some(r) = v.get("refused").and_then(Value::as_str) {
        return Err(format!("the app refused the connection: {r}"));
    }
    Ok((rd, sh))
}

/// The `--attach` bridge: this process's stdin / stdout ↔ the running app, until stdin closes
/// (then the process ends) or the app goes away. `Err` before anything was forwarded: the caller
/// serves headless instead.
pub fn attach(read_only: bool) -> Result<(), String> {
    let path = ticket_path().ok_or("no settings folder")?;
    let (mut from_app, mut to_app) = connect(&path, read_only)?;
    ::log::info!("--attach: connected to the app (read only: {read_only})");
    std::thread::Builder::new()
        .name("attach-stdin".into())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if writeln!(to_app, "{line}").and_then(|()| to_app.flush()).is_err() {
                    break;
                }
            }
            // The client closed stdin (MCP shutdown): the bridge ends.
            crate::diagnostics::finish();
            std::process::exit(0);
        })
        .map_err(|e| e.to_string())?;
    let mut out = io::stdout().lock();
    let mut line = String::new();
    loop {
        line.clear();
        match from_app.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if out.write_all(line.as_bytes()).and_then(|()| out.flush()).is_err() {
                    break;
                }
            }
        }
    }
    ::log::info!("--attach: the app closed the connection");
    Ok(())
}

// ------------------------------------------------------------------ host ---

/// A tool call for the app's main thread.
pub struct HostCall {
    /// The tool.
    pub tool: &'static Tool,
    /// Its arguments (an object).
    pub args: Value,
    /// The connection is read-only.
    pub read_only: bool,
    /// Set when the client cancels the call (the app stops its job).
    pub cancel: Arc<AtomicBool>,
    seq: u64,
    reply: Sender<Event>,
}

impl HostCall {
    /// Sends the call's result back to its connection.
    pub fn answer(self, result: Result<Value, ApiError>) {
        let _ = self.reply.send(Event::Done {
            seq: self.seq,
            result,
        });
    }

    /// A session for this call on `project` (the app's), with the connection's mode.
    pub fn session(&self, project: crate::document::Project) -> Session {
        let mut s = Session::new(project);
        s.read_only = self.read_only;
        s.hosted = true;
        s
    }
}

/// What a connection's loop waits for.
enum Event {
    In(Value),
    Bad(String),
    Eof,
    Done {
        seq: u64,
        result: Result<Value, ApiError>,
    },
}

/// The app's endpoint while connections are allowed; dropping it stops accepting (connections
/// that are open end at their next call) and removes the ticket.
pub struct Host {
    rx: Receiver<HostCall>,
    stop: Arc<AtomicBool>,
    clients: Arc<AtomicUsize>,
    busy: Arc<Mutex<Option<String>>>,
    ticket: PathBuf,
    token: String,
}

impl Host {
    /// Starts accepting at the current user's endpoint, writing the ticket to [`ticket_path`].
    pub fn start() -> io::Result<Self> {
        let path = ticket_path().ok_or_else(|| io::Error::other("no settings folder"))?;
        Self::start_at(&endpoint_for_user(), &path)
    }

    /// Starts accepting at `endpoint`, the ticket at `ticket` (tests use their own).
    pub fn start_at(endpoint: &str, ticket: &Path) -> io::Result<Self> {
        let listener = ListenerOptions::new()
            .name(name_of(endpoint)?)
            .nonblocking(ListenerNonblockingMode::Accept)
            .try_overwrite(true)
            .create_sync()?;
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        write_ticket(
            ticket,
            &Ticket {
                endpoint: endpoint.to_owned(),
                token: token.clone(),
                pid: std::process::id(),
            },
        )?;
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let clients = Arc::new(AtomicUsize::new(0));
        let busy = Arc::new(Mutex::new(None));
        let (s, c, b, tk) = (stop.clone(), clients.clone(), busy.clone(), token.clone());
        std::thread::Builder::new()
            .name("ai-accept".into())
            .spawn(move || accept_loop(listener, &s, &c, &b, &tk, &tx))?;
        ::log::info!("AI Connector: accepting connections at {endpoint}");
        Ok(Self {
            rx,
            stop,
            clients,
            busy,
            ticket: ticket.to_path_buf(),
            token,
        })
    }

    /// The next call waiting for the main thread.
    pub fn try_recv(&self) -> Option<HostCall> {
        self.rx.try_recv().ok()
    }

    /// Connected clients.
    pub fn clients(&self) -> usize {
        self.clients.load(Ordering::Relaxed)
    }

    /// The command of the call the app is running for a client, if any (the status indicator).
    pub fn busy(&self) -> Option<String> {
        self.busy.lock().ok().and_then(|b| b.clone())
    }

    /// Sets what [`Host::busy`] says.
    pub fn set_busy(&self, what: Option<String>) {
        if let Ok(mut b) = self.busy.lock() {
            *b = what;
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Only our own ticket goes (another app may have written its own since).
        if read_ticket(&self.ticket).is_ok_and(|t| t.token == self.token) {
            let _ = std::fs::remove_file(&self.ticket);
        }
        ::log::info!("AI Connector: no longer accepting connections");
    }
}

fn accept_loop(
    listener: interprocess::local_socket::Listener,
    stop: &AtomicBool,
    clients: &Arc<AtomicUsize>,
    busy: &Arc<Mutex<Option<String>>>,
    token: &str,
    calls: &Sender<HostCall>,
) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok(stream) => {
                let (c, b, t, tx) = (clients.clone(), busy.clone(), token.to_owned(), calls.clone());
                let spawned = std::thread::Builder::new()
                    .name("ai-connection".into())
                    .spawn(move || {
                        c.fetch_add(1, Ordering::Relaxed);
                        if let Err(e) = connection(stream, &t, &tx, &b) {
                            ::log::info!("AI Connector: connection ended ({e})");
                        }
                        c.fetch_sub(1, Ordering::Relaxed);
                    });
                if let Err(e) = spawned {
                    ::log::warn!("AI Connector: no thread for a connection: {e}");
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                ::log::warn!("AI Connector: accept failed: {e}");
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

fn connection(
    stream: Stream,
    token: &str,
    calls: &Sender<HostCall>,
    busy: &Arc<Mutex<Option<String>>>,
) -> io::Result<()> {
    let (rh, mut sh) = halves(stream);
    let mut rd = BufReader::new(rh);
    let mut line = String::new();
    rd.read_line(&mut line)?;
    let hello: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
    if hello.get("hello").and_then(Value::as_str) != Some(token) {
        send(&mut sh, &json!({"refused": "wrong token"}))?;
        return Err(io::Error::other("a client sent a wrong token"));
    }
    let read_only = hello.get("read_only").and_then(Value::as_bool) == Some(true);
    send(&mut sh, &json!({"welcome": "3dcadcraft", "version": env!("CARGO_PKG_VERSION")}))?;
    ::log::info!("AI Connector: a client connected (read only: {read_only})");
    serve_connection(rd, sh, read_only, calls, busy)
}

/// The call a connection waits for.
struct Waiting {
    seq: u64,
    id: Option<Value>,
    tool: &'static Tool,
    cancel: Arc<AtomicBool>,
    token: Option<Value>,
    sent: u64,
    last: String,
}

/// The MCP protocol of one connection: initialize, tools/list and ping here, every tool call
/// (and resources) through the main thread, one at a time; `status` and `cancel` answer while a
/// call runs, other calls wait for it.
pub fn serve_connection<R, W>(
    input: R,
    mut out: W,
    read_only: bool,
    calls: &Sender<HostCall>,
    busy: &Arc<Mutex<Option<String>>>,
) -> io::Result<()>
where
    R: BufRead + Send + 'static,
    W: Write,
{
    let (tx, rx) = mpsc::channel::<Event>();
    let reader = tx.clone();
    std::thread::Builder::new()
        .name("ai-reader".into())
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
    let mode = Session {
        read_only,
        hosted: true,
        ..Session::default()
    };
    let mut seq = 0u64;
    let mut waiting: Option<Waiting> = None;
    let mut queue: VecDeque<Value> = VecDeque::new();
    // Calls made for a `resources/*` request: seq, request id, "list" or the uri read.
    let mut internal: Vec<(u64, Value, String)> = Vec::new();
    loop {
        if waiting.is_none()
            && let Some(msg) = queue.pop_front()
        {
            waiting = handle(&mode, &mut out, msg, calls, &tx, &mut seq, &mut internal)?;
            continue;
        }
        let ev = match rx.recv_timeout(Duration::from_millis(1000)) {
            Ok(ev) => ev,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(w) = &mut waiting {
                    progress(&mut out, w)?;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match ev {
            Event::Eof => {
                if let Some(w) = &waiting {
                    w.cancel.store(true, Ordering::Relaxed);
                }
                break;
            }
            Event::Bad(e) => send(&mut out, &error(&Value::Null, -32700, &format!("parse error: {e}")))?,
            Event::Done { seq: s, result } => {
                if let Some(k) = internal.iter().position(|(q, _, _)| *q == s) {
                    let (_, id, what) = internal.remove(k);
                    send(&mut out, &resource_answer(&id, &what, result))?;
                    continue;
                }
                let Some(w) = waiting.take_if(|w| w.seq == s) else {
                    continue;
                };
                if let Ok(mut b) = busy.lock() {
                    *b = None;
                }
                if let Some(id) = &w.id {
                    send(&mut out, &reply(id, tool_result(result)))?;
                }
            }
            Event::In(msg) => {
                if waiting.is_none() {
                    waiting = handle(&mode, &mut out, msg, calls, &tx, &mut seq, &mut internal)?;
                    continue;
                }
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
                let name = msg.pointer("/params/name").and_then(Value::as_str);
                let Some(id) = msg.get("id").cloned() else {
                    if method == "notifications/cancelled"
                        && let Some(w) = &mut waiting
                        && w.id.as_ref() == msg.pointer("/params/requestId")
                    {
                        // The client gave up on it: no answer (MCP), the app stops the job.
                        w.cancel.store(true, Ordering::Relaxed);
                        w.id = None;
                    }
                    continue;
                };
                match (method, name) {
                    ("ping", _) => send(&mut out, &reply(&id, json!({})))?,
                    ("tools/call", Some("cancel")) => {
                        if let Some(w) = &waiting {
                            w.cancel.store(true, Ordering::Relaxed);
                            let v = json!({"cancelled": w.tool.name});
                            send(&mut out, &reply(&id, tool_result(Ok(v))))?;
                        }
                    }
                    ("tools/call", Some("status")) => {
                        // The main thread answers a read at once, also while a job runs.
                        let busy_tool = waiting.as_ref().map(|w| w.tool.name);
                        let r = call_now(calls, "status", json!({}), read_only).map(|mut v| {
                            v["busy"] = json!(busy_tool);
                            v
                        });
                        send(&mut out, &reply(&id, tool_result(r)))?;
                    }
                    _ => queue.push_back(msg),
                }
            }
        }
    }
    if let Ok(mut b) = busy.lock() {
        *b = None;
    }
    Ok(())
}

/// Runs `tool` on the main thread and waits for its answer (reads only: the app answers them
/// within a frame).
fn call_now(
    calls: &Sender<HostCall>,
    tool: &str,
    args: Value,
    read_only: bool,
) -> Result<Value, ApiError> {
    let t = tools::find(tool).ok_or_else(|| ApiError::new(tool, "no such tool"))?;
    let (tx, rx) = mpsc::channel();
    calls
        .send(HostCall {
            tool: t,
            args,
            read_only,
            cancel: Arc::new(AtomicBool::new(false)),
            seq: 0,
            reply: tx,
        })
        .map_err(|_| stopped(t))?;
    match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(Event::Done { result, .. }) => result,
        _ => Err(ApiError::new(t.command, "the app did not answer")),
    }
}

fn stopped(t: &Tool) -> ApiError {
    ApiError::new(t.command, "the app no longer accepts AI connections")
}

/// The answer to a `resources/*` request from the status / features call made for it.
fn resource_answer(id: &Value, what: &str, result: Result<Value, ApiError>) -> Value {
    match (what, result) {
        ("list", Ok(v)) => {
            let mut list = vec![json!({"uri": "cadcraft://project", "name": "project", "title": "Project and documents", "mimeType": "application/json"})];
            for d in v["documents"].as_array().into_iter().flatten() {
                if d["kind"] == "part"
                    && let Some(n) = d["name"].as_str()
                {
                    list.push(json!({"uri": format!("cadcraft://part/{n}/features"), "name": format!("{n} features"),
                        "title": format!("Feature tree of {n}"), "mimeType": "application/json"}));
                }
            }
            reply(id, json!({"resources": list}))
        }
        (uri, Ok(v)) => reply(
            id,
            json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": v.to_string()}]}),
        ),
        (_, Err(e)) => error(id, -32002, &e.to_string()),
    }
}

/// Handles one message while nothing runs; returns the call sent to the main thread.
fn handle<W: Write>(
    mode: &Session,
    out: &mut W,
    msg: Value,
    calls: &Sender<HostCall>,
    tx: &Sender<Event>,
    seq: &mut u64,
    internal: &mut Vec<(u64, Value, String)>,
) -> io::Result<Option<Waiting>> {
    let (Some(method), Some(id)) = (msg.get("method").and_then(Value::as_str), msg.get("id").cloned())
    else {
        return Ok(None); // notifications and answers to nothing we asked
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let mut forward = |tool: &'static Tool, args: Value| -> Result<(u64, Arc<AtomicBool>), ApiError> {
        *seq += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        calls
            .send(HostCall {
                tool,
                args,
                read_only: mode.read_only,
                cancel: cancel.clone(),
                seq: *seq,
                reply: tx.clone(),
            })
            .map_err(|_| stopped(tool))?;
        Ok((*seq, cancel))
    };
    match method {
        "initialize" => send(out, &reply(&id, initialize(&params)))?,
        "ping" => send(out, &reply(&id, json!({})))?,
        "tools/list" => send(out, &reply(&id, tools_list(mode)))?,
        "resources/list" | "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
            let (tool, args, what) = if method == "resources/list" {
                ("status", json!({}), "list".to_owned())
            } else if uri == "cadcraft://project" {
                ("status", json!({}), uri.to_owned())
            } else if let Some(n) = uri
                .strip_prefix("cadcraft://part/")
                .and_then(|r| r.strip_suffix("/features"))
            {
                ("features", json!({"part": n}), uri.to_owned())
            } else {
                send(out, &error(&id, -32002, &format!("resource not found: {uri}")))?;
                return Ok(None);
            };
            let Some(t) = tools::find(tool) else {
                return Ok(None);
            };
            match forward(t, args) {
                Ok((s, _)) => internal.push((s, id, what)),
                Err(e) => send(out, &error(&id, -32603, &e.to_string()))?,
            }
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let Some(tool) = tools::find(name) else {
                send(out, &error(&id, -32602, &format!("unknown tool: {name}")))?;
                return Ok(None);
            };
            if tool.name == "cancel" {
                let v = json!({"cancelled": null, "note": "nothing was running"});
                send(out, &reply(&id, tool_result(Ok(v))))?;
                return Ok(None);
            }
            if let Err(e) = tools::allowed(mode, tool) {
                let e = Err(ApiError::new(tool.command, e));
                super::log::record(super::log::Via::App, tool, &params["arguments"], &e, 0);
                send(out, &reply(&id, tool_result(e)))?;
                return Ok(None);
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match forward(tool, args) {
                Ok((s, cancel)) => {
                    return Ok(Some(Waiting {
                        seq: s,
                        id: Some(id),
                        tool,
                        cancel,
                        token: params.pointer("/_meta/progressToken").cloned(),
                        sent: 0,
                        last: String::new(),
                    }));
                }
                Err(e) => send(out, &reply(&id, tool_result(Err(e))))?,
            }
        }
        _ => send(out, &error(&id, -32601, &format!("method not found: {method}")))?,
    }
    Ok(None)
}

/// The progress job name of an AI call in the app (`AI: Extrude`).
pub fn job_label(t: &Tool) -> String {
    format!("AI: {}", t.command)
}

fn progress<W: Write>(out: &mut W, w: &mut Waiting) -> io::Result<()> {
    let Some(token) = w.token.clone() else {
        return Ok(());
    };
    let label = job_label(w.tool);
    let Some(job) = crate::part::progress::running()
        .into_iter()
        .find(|j| j.job == label)
    else {
        return Ok(());
    };
    let text = format!("{} ({:.0} s)", job.text(), job.elapsed.as_secs_f64());
    if text == w.last {
        return Ok(());
    }
    w.sent += 1;
    w.last = text.clone();
    let params = json!({"progressToken": token, "progress": w.sent, "message": text});
    send(out, &json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": params}))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A main thread that runs every call on its own session at once.
    fn main_thread(rx: Receiver<HostCall>, mut s: Session) -> std::thread::JoinHandle<Session> {
        std::thread::spawn(move || {
            while let Ok(c) = rx.recv() {
                s.read_only = c.read_only;
                s.hosted = true;
                let r = tools::call(&mut s, c.tool.name, &c.args);
                c.answer(r);
            }
            s
        })
    }

    fn line<R: BufRead>(r: &mut R) -> Value {
        let mut l = String::new();
        let _ = r.read_line(&mut l);
        serde_json::from_str(l.trim()).unwrap_or(Value::Null)
    }

    #[test]
    fn an_endpoint_with_a_ticket_serves_tools_through_the_main_thread() {
        let dir = std::env::temp_dir().join(format!("cadcraft-host-{}", std::process::id()));
        let ticket = dir.join("ticket.json");
        let endpoint = if cfg!(windows) {
            format!("3dcadcraft-mcp-test-{}", std::process::id())
        } else {
            dir.join("t.sock").display().to_string()
        };
        let _ = std::fs::create_dir_all(&dir);
        let host = Host::start_at(&endpoint, &ticket).unwrap_or_else(|e| panic!("start: {e}"));
        // A wrong token is refused.
        let t = read_ticket(&ticket).unwrap_or_else(|e| panic!("{e}"));
        let bad = Ticket {
            token: "nope".into(),
            ..t.clone()
        };
        let other = dir.join("bad.json");
        write_ticket(&other, &bad).unwrap_or_else(|e| panic!("{e}"));
        let e = connect(&other, false).err().unwrap_or_default();
        assert!(e.contains("refused"), "{e}");
        // The real one works; calls reach the main thread.
        let (mut rd, mut wr) = connect(&ticket, false).unwrap_or_else(|e| panic!("{e}"));
        let (calls_tx, calls_rx) = mpsc::channel();
        // Forward the host's calls to a test main thread.
        let main = main_thread(calls_rx, Session::default());
        let done = Arc::new(AtomicBool::new(false));
        let stop = done.clone();
        let fwd = std::thread::spawn(move || {
            let until = std::time::Instant::now() + Duration::from_secs(30);
            while !stop.load(Ordering::Relaxed) && std::time::Instant::now() < until {
                match host.try_recv() {
                    Some(c) => {
                        if calls_tx.send(c).is_err() {
                            break;
                        }
                    }
                    None => std::thread::sleep(Duration::from_millis(5)),
                }
            }
            assert_eq!(host.clients(), 1);
            drop(host);
        });
        let msg = |id: u64, method: &str, params: Value| {
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        };
        let mut ask = |v: Value| -> Value {
            let _ = writeln!(wr, "{v}").and_then(|()| wr.flush());
            line(&mut rd)
        };
        let init = ask(msg(1, "initialize", json!({"protocolVersion": "2025-06-18"})));
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
        let list = ask(msg(2, "tools/list", json!({})));
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(names.contains(&"extrude") && !names.contains(&"project_open"), "{names:?}");
        let r = ask(msg(3, "tools/call", json!({"name": "document_new", "arguments": {"kind": "part", "name": "P"}})));
        assert_eq!(r["result"]["isError"], false, "{r}");
        let r = ask(msg(4, "tools/call", json!({"name": "project_open", "arguments": {"path": "x.3dcadcraft"}})));
        assert_eq!(r["result"]["isError"], true, "{r}");
        let r = ask(msg(5, "resources/list", json!({})));
        assert_eq!(r["result"]["resources"][1]["uri"], "cadcraft://part/P/features", "{r}");
        let r = ask(msg(6, "tools/call", json!({"name": "status", "arguments": {}})));
        assert_eq!(r["result"]["structuredContent"]["active"], "P", "{r}");
        done.store(true, Ordering::Relaxed);
        let _ = fwd.join();
        drop(wr);
        drop(rd);
        let s = main.join().unwrap_or_default();
        assert_eq!(s.project.documents().len(), 1);
        assert!(!ticket.exists(), "the ticket goes with the host");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
