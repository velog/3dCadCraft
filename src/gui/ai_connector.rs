//! Tools › AI Connector (spec 0.7 §3.9): the running app as the MCP server of an AI agent.
//!
//! While the user allows connections, [`crate::api::host::Host`] accepts `3dcadcraft --mcp
//! --attach` bridges on a local pipe and hands their tool calls to [`CadCraftApp::ai_tick`] once a
//! frame. Reads run at once on the project; changes are refused while the user has a command, a
//! sketch or a job open, else they run as the one background job (spec 0.19 §3.3) on a copy of the
//! project and come back as one Undo step `AI: <command>`. The status bar shows `AI` while
//! connections are allowed and `AI: <command>` while a call runs.

use std::time::Instant;

use eframe::egui::{self, RichText};
use serde_json::{Value, json};

use super::CadCraftApp;
use crate::api::host::{Host, HostCall};
use crate::api::tools::{Effect, Tool};
use crate::api::{ApiError, Session};

/// The connector's state in the app.
#[derive(Default)]
pub(super) struct AiUi {
    /// Accepting connections (the option is on and the endpoint could be opened).
    pub(super) host: Option<Host>,
    /// The option (Application Options, saved).
    pub(super) allowed: bool,
    /// The AI Connector window is open.
    pub(super) window: bool,
    /// Why the endpoint could not be opened.
    pub(super) error: Option<String>,
}

/// The command line an MCP client starts to reach this app.
fn bridge_command() -> String {
    std::env::current_exe()
        .map(|p| {
            crate::document::library::without_verbatim(p)
                .display()
                .to_string()
        })
        .unwrap_or_else(|_| "3dcadcraft".into())
}

impl CadCraftApp {
    /// Turns AI connections on or off (Tools › AI Connector).
    pub(super) fn ai_set_allowed(&mut self, on: bool) {
        self.ai.allowed = on;
        if !on {
            if self.ai.host.take().is_some() {
                self.file_status = Some(Ok("AI Connector: connections are off".into()));
            }
            return;
        }
        if self.ai.host.is_some() {
            return;
        }
        match Host::start() {
            Ok(h) => {
                self.ai.host = Some(h);
                self.ai.error = None;
                self.file_status = Some(Ok("AI Connector: accepting connections".into()));
            }
            Err(e) => {
                let e = format!("AI Connector: cannot accept connections ({e})");
                log::warn!("{e}");
                self.ai.error = Some(e.clone());
                self.file_status = Some(Err(e));
            }
        }
    }

    /// Once a frame: takes the calls the connections handed over and runs them.
    pub(super) fn ai_tick(&mut self, ctx: &egui::Context) {
        let Some(host) = &self.ai.host else {
            return;
        };
        // Calls arrive between frames: look again soon even when nothing moves.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let calls: Vec<HostCall> = std::iter::from_fn(|| host.try_recv()).collect();
        // The client cancelled the running AI job.
        if self
            .bg
            .as_ref()
            .and_then(|j| j.ai.as_ref())
            .is_some_and(|c| c.cancel.load(std::sync::atomic::Ordering::Relaxed))
        {
            self.bg_cancel();
        }
        for call in calls {
            self.ai_run(call);
        }
    }

    /// Why a changing call cannot run now, if it cannot.
    fn ai_refusal(&self) -> Option<String> {
        if let Some(j) = &self.bg {
            return Some(format!(
                "the app is busy ({}); try again when it is done",
                j.label
            ));
        }
        let st = self.open_state();
        let what = st
            .command
            .map(str::to_owned)
            .or_else(|| self.editing.map(|_| "a sketch".to_owned()))?;
        Some(format!(
            "the user is editing ({what}); try again when it is closed"
        ))
    }

    /// A session of `call` on `project` with this app's file, active document and state.
    fn ai_session(&self, call: &HostCall, project: crate::document::Project) -> Session {
        let mut s = call.session(project);
        s.path = Some(self.file_path.trim())
            .filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from);
        s.active = Some(self.part_id);
        s
    }

    fn ai_run(&mut self, call: HostCall) {
        let started = Instant::now();
        let t = call.tool;
        if let Err(e) =
            crate::api::tools::allowed(&call.session(crate::document::Project::new("")), t)
        {
            self.ai_answer(call, Err(ApiError::new(t.command, e)), started);
            return;
        }
        match t.effect {
            Effect::Read => {
                // On the project itself (no copy): reads change nothing.
                let project =
                    std::mem::replace(&mut self.project, crate::document::Project::new(""));
                let mut s = self.ai_session(&call, project);
                s.dirty = false;
                let mut r = crate::api::tools::run_on(&mut s, t, &call.args);
                self.project = s.project;
                if t.name == "status"
                    && let Ok(v) = &mut r
                {
                    v["undo"] = json!(self.history.undo_label());
                    v["redo"] = json!(self.history.redo_label());
                    v["unsaved"] = json!(self.project_signature_now() != self.saved_sig);
                    v["app"] = json!(true);
                }
                self.ai_answer(call, r, started);
            }
            _ if self.ai_refusal().is_some() => {
                let why = self.ai_refusal().unwrap_or_default();
                self.ai_answer(call, Err(ApiError::new(t.command, why)), started);
            }
            Effect::InPlace => {
                let r = self.ai_in_place(t, &call.args);
                self.ai_answer(call, r, started);
            }
            Effect::Replace => {
                let e = ApiError::new(t.command, "refused while attached to the app");
                self.ai_answer(call, Err(e), started);
            }
            Effect::Change => {
                let work = self.ai_session(&call, self.project.clone());
                if let Some(h) = &self.ai.host {
                    h.set_busy(Some(t.command.to_owned()));
                }
                self.ai_job(call, work);
            }
        }
    }

    /// Save, Undo, Redo: the app's own.
    fn ai_in_place(&mut self, t: &Tool, args: &Value) -> Result<Value, ApiError> {
        let err = |e: String| ApiError::new(t.command, e);
        match t.name {
            "project_save" => {
                if let Some(p) = args.get("path").and_then(Value::as_str) {
                    let a = crate::api::args::Args::new(t.command, args);
                    let path = crate::api::tools::checked_path(&a, p)?;
                    self.file_path = path.display().to_string();
                }
                if self.file_path.trim().is_empty() {
                    return Err(err("the project has no file yet: give 'path'".into()));
                }
                self.file_status = None;
                self.save_project(false);
                match &self.file_status {
                    Some(Err(e)) => Err(err(e.clone())),
                    _ => Ok(json!({"path": self.file_path})),
                }
            }
            "undo" | "redo" => {
                let undo = t.name == "undo";
                let label = if undo {
                    self.history.undo_label()
                } else {
                    self.history.redo_label()
                }
                .map(str::to_owned)
                .ok_or_else(|| err(format!("nothing to {}", t.name)))?;
                if undo {
                    self.undo();
                } else {
                    self.redo();
                }
                let key = if undo { "undone" } else { "redone" };
                let mut v = json!({});
                v[key] = json!(label);
                Ok(v)
            }
            _ => Err(err("not available in the app".into())),
        }
    }

    /// Takes a changing call's result: one Undo step `AI: <command>` (spec 0.7 §3.9).
    pub(super) fn ai_take(&mut self, tool: &Tool, work: Session) {
        self.checkpoint(&crate::api::host::job_label(tool));
        let empty = self.part().body.faces().next().is_none();
        let active = work.active;
        self.project = work.project;
        self.edge_cache.replace(None);
        self.after_restore();
        if let Some(id) = active
            && id != self.part_id
        {
            self.activate_document(id);
        }
        // The first body the agent makes comes into view (later ones keep the user's camera).
        if empty && self.part().body.faces().next().is_some() {
            self.fit_view();
        }
        self.scene_dirty = true;
        self.file_status = Some(Ok(crate::api::host::job_label(tool)));
    }

    /// Answers `call`, writes its journal line and clears the busy indicator.
    pub(super) fn ai_answer(
        &mut self,
        call: HostCall,
        result: Result<Value, ApiError>,
        started: Instant,
    ) {
        crate::api::log::record(
            crate::api::log::Via::App,
            call.tool,
            &call.args,
            &result,
            started.elapsed().as_millis(),
        );
        if let Err(e) = &result
            && call.tool.effect != Effect::Read
        {
            self.file_status = Some(Err(format!("AI: {e}")));
        }
        if let Some(h) = &self.ai.host {
            h.set_busy(None);
        }
        call.answer(result);
    }

    /// The status bar's `AI` / `AI: <command>` (right-to-left layout: drawn where it is called).
    pub(super) fn ai_indicator(&mut self, ui: &mut egui::Ui) {
        let Some(h) = &self.ai.host else {
            return;
        };
        let tokens = super::theme::tokens();
        let (text, color) = match h.busy() {
            Some(c) => (format!("AI: {c}"), tokens.warning),
            None if h.clients() > 0 => ("AI".to_owned(), tokens.ok),
            None => ("AI".to_owned(), ui.visuals().weak_text_color()),
        };
        let n = h.clients();
        let r = ui
            .add(
                egui::Label::new(RichText::new(text).color(color).strong())
                    .sense(egui::Sense::click()),
            )
            .on_hover_text(format!(
                "AI Connector: {n} client{} connected (Tools > AI Connector)",
                if n == 1 { "" } else { "s" }
            ));
        if r.clicked() {
            self.ai.window = true;
        }
        ui.separator();
    }

    /// The AI Connector window: the switch, the state and how to add the app to a client.
    pub(super) fn ai_window(&mut self, ctx: &egui::Context) {
        if !self.ai.window {
            return;
        }
        let mut open = true;
        let mut allowed = self.ai.allowed;
        let w = egui::Window::new("AI Connector")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0);
        let exe = bridge_command();
        let host = self.ai.host.as_ref().map(|h| (h.clients(), h.busy()));
        let error = self.ai.error.clone();
        self.layout
            .place("ai_connector", ctx.content_rect(), false, w)
            .show(ctx, |ui| {
                ui.checkbox(&mut allowed, "Allow AI connections")
                    .on_hover_text("An AI agent (Claude Code, Claude Desktop...) started with --mcp --attach works on the open project; every change is one Undo step");
                ui.add_space(4.0);
                let tokens = super::theme::tokens();
                match (&host, &error) {
                    (Some((n, busy)), _) => {
                        ui.label(RichText::new(format!("Accepting connections: {n} connected")).color(tokens.ok));
                        if let Some(b) = busy {
                            ui.label(format!("Running: {b}"));
                        }
                    }
                    (None, Some(e)) => {
                        ui.label(RichText::new(e).color(tokens.error));
                    }
                    (None, None) => {
                        ui.label(RichText::new("Off: no agent can reach the app").weak());
                    }
                }
                ui.separator();
                ui.label("Claude Code (in a terminal):");
                let code = format!("claude mcp add 3dcadcraft -- \"{exe}\" --mcp --attach");
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(&code).monospace()).wrap());
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(code.clone());
                    }
                });
                ui.label("Claude Desktop (claude_desktop_config.json, mcpServers):");
                let desktop = format!(
                    "\"3dcadcraft\": {{
  \"command\": {},
  \"args\": [\"--mcp\", \"--attach\"]
}}",
                    json!(exe)
                );
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(&desktop).monospace()).wrap());
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(desktop.clone());
                    }
                });
                ui.label(
                    RichText::new("Add --read-only for an agent that may only look. Without the app running, the same command works on its own copy (headless).")
                        .weak(),
                );
            });
        if allowed != self.ai.allowed {
            self.ai_set_allowed(allowed);
        }
        if !open {
            self.ai.window = false;
        }
    }
}
