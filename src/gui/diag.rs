//! Diagnostics in the GUI (`docs/spec/0.14-diagnostics.md`): the action
//! journal hooks, autosave, the Restore prompt and the Help menu items
//! (Save Diagnostics…, Open Logs Folder). The logic is in
//! [`crate::diagnostics`]; this file only observes the app once a frame.

use std::collections::BTreeMap;
use std::time::Instant;

use eframe::egui;
use serde_json::json;

use super::CadCraftApp;
use super::files::DialogPurpose;
use crate::diagnostics::{self, crash, journal, recovery};

/// A snapshot is taken once the last checkpoint is this old (a gesture
/// records many checkpoints with one label).
const SETTLE_SECS: f64 = 0.3;
/// Checkpoints with the same label this close together are one gesture
/// (as `History` coalesces them).
const COALESCE_SECS: f64 = 0.8;

/// A feature dialog that is open.
struct OpenCommand {
    name: String,
    started: Instant,
    /// The features (JSON) when it opened.
    features: serde_json::Value,
}

/// What the diagnostics hooks remember between frames.
pub(super) struct DiagUi {
    /// A mutation happened: write the part's state once it settles.
    snap_due: bool,
    /// The last checkpoint's label and time.
    last_checkpoint: Option<(String, Instant)>,
    command: Option<OpenCommand>,
    /// The status-bar text last journaled.
    status: Option<Result<String, String>>,
    /// The sick features last journaled.
    health: BTreeMap<String, String>,
    /// A feature was committed since the last autosave check.
    committed: bool,
    autosave: recovery::Autosaver,
    autosave_job: Option<std::thread::JoinHandle<()>>,
    /// Autosaves to offer (Restore prompt).
    offers: Vec<recovery::Offer>,
}

impl DiagUi {
    /// Reads what the previous session left (Restore offers).
    pub(super) fn new() -> Self {
        let previous = diagnostics::session()
            .and_then(|s| s.files.as_ref())
            .and_then(|f| f.previous.clone());
        let offers = diagnostics::session()
            .and_then(|s| s.root.as_deref())
            .map(|root| {
                let list = recovery::list(&diagnostics::recovery_dir(root));
                recovery::offers(&list, previous.as_ref(), recovery::file_mtime_ms)
            })
            .unwrap_or_default();
        for o in &offers {
            log::info!(
                "offering to restore {} (autosave of '{}')",
                o.entry.file.display(),
                o.entry.meta.original
            );
        }
        Self {
            snap_due: false,
            last_checkpoint: None,
            command: None,
            status: None,
            health: BTreeMap::new(),
            committed: false,
            autosave: recovery::Autosaver::new(Instant::now()),
            autosave_job: None,
            offers,
        }
    }

    /// `CadCraftApp::checkpoint`: one journal line per gesture.
    pub(super) fn checkpoint(&mut self, label: &str) {
        let now = Instant::now();
        let same_gesture = self.last_checkpoint.as_ref().is_some_and(|(l, t)| {
            l == label && now.duration_since(*t).as_secs_f64() < COALESCE_SECS
        });
        if !same_gesture {
            journal::global_entry("checkpoint", label, json!({}));
        }
        self.last_checkpoint = Some((label.to_owned(), now));
        self.snap_due = true;
    }

    /// Undo / Redo (`kind`) of the step `label`.
    pub(super) fn undo_redo(&mut self, kind: &str, label: Option<String>) {
        let label = label.unwrap_or_default();
        journal::global_entry(kind, &label, json!({}));
        self.last_checkpoint = Some((format!("{kind} {label}"), Instant::now()));
        self.snap_due = true;
    }
}

impl CadCraftApp {
    /// The feature dialog that is open, by Inventor's command name.
    fn open_command(&self) -> Option<String> {
        if let Some(d) = &self.extrude {
            return Some(d.def.command().into());
        }
        if let Some(d) = &self.blend_draft {
            return Some(d.def.command().into());
        }
        if let Some(d) = &self.pattern_draft {
            return Some(d.def.command().into());
        }
        [
            (self.shell_draft.is_some(), "Shell"),
            (self.draft_draft.is_some(), "Face Draft"),
            (self.hole_draft.is_some(), "Hole"),
            (self.thread_draft.is_some(), "Thread"),
            (self.work_draft.is_some(), "Work Feature"),
            (self.sweep_draft.is_some(), "Sweep"),
            (self.loft_draft.is_some(), "Loft"),
            (self.coil_draft.is_some(), "Coil"),
            (self.rib_draft.is_some(), "Rib"),
            (self.combine_draft.is_some(), "Combine"),
            (self.split_draft.is_some(), "Split"),
        ]
        .into_iter()
        .find(|(open, _)| *open)
        .map(|(_, n)| n.to_owned())
    }

    /// The definition in the open feature dialog as a feature (JSON, with
    /// its profile sketch's id where the dialog holds one): an `error`
    /// while a dialog is open carries it as `attempt`, so a failed OK can be
    /// replayed ([`crate::diagnostics::replay::retry`]).
    fn open_attempt(&self) -> Option<serde_json::Value> {
        use crate::part::Feature;
        let sketch_id = |k: Option<usize>| {
            k.and_then(|k| self.part().sketches.get(k))
                .map(|s| s.id.clone())
        };
        let (f, sketch) = if let Some(d) = &self.extrude {
            let f = match &d.def {
                super::DraftDef::Extrude(e) => Feature::Extrude(e.clone()),
                super::DraftDef::Revolve(r) => Feature::Revolve(r.clone()),
            };
            (f, sketch_id(Some(d.sketch)))
        } else if let Some(d) = &self.sweep_draft {
            (Feature::Sweep(d.def.clone()), sketch_id(d.sketch))
        } else if let Some(d) = &self.coil_draft {
            (Feature::Coil(d.def.clone()), sketch_id(d.sketch))
        } else if let Some(d) = &self.rib_draft {
            (Feature::Rib(d.def.clone()), sketch_id(d.sketch))
        } else if let Some(d) = &self.loft_draft {
            (Feature::Loft(d.def.clone()), None)
        } else if let Some(d) = &self.blend_draft {
            (Feature::Blend(d.def.clone()), None)
        } else if let Some(d) = &self.shell_draft {
            (Feature::Shell(d.def.clone()), None)
        } else if let Some(d) = &self.draft_draft {
            (Feature::Draft(d.def.clone()), None)
        } else if let Some(d) = &self.pattern_draft {
            (Feature::Pattern(d.def.clone()), None)
        } else if let Some(d) = &self.hole_draft {
            (Feature::Hole(d.def.clone()), None)
        } else if let Some(d) = &self.combine_draft {
            (Feature::Combine(d.def.clone()), None)
        } else if let Some(d) = &self.split_draft {
            (Feature::Split(d.def.clone()), None)
        } else {
            return None;
        };
        journal::attempt_json(&f, sketch.as_deref())
    }

    /// A command preview or a drag is in progress: the state is not one
    /// to save or snapshot.
    fn diag_busy(&self) -> bool {
        self.open_command().is_some()
            || self.dragging.is_some()
            || self.dragging_entity.is_some()
            || self.dragging_dim.is_some()
            || self.manip_drag.is_some()
            || self.pattern_drag.is_some()
            || self.handle_drag.is_some()
            || self.arc_drag.is_some()
            || self.part().rollback.is_some()
    }

    /// Once a frame, after everything else: journal command starts and
    /// ends, status-bar results, feature health and part states; autosave;
    /// the Restore prompt.
    pub(super) fn diag_frame(&mut self, ctx: &egui::Context) {
        self.diag_commands();
        self.diag_status(ctx);
        let busy = self.diag_busy();
        if !busy {
            self.diag_health();
        }
        if self.diag.snap_due {
            let settled = self
                .diag
                .last_checkpoint
                .as_ref()
                .is_none_or(|(_, t)| t.elapsed().as_secs_f64() >= SETTLE_SECS);
            if settled && !busy {
                self.diag_snapshot(ctx);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(SETTLE_SECS));
            }
        }
        if !busy {
            self.diag_autosave(ctx);
        }
        self.restore_window(ctx);
    }

    fn diag_commands(&mut self) {
        let now = self.open_command();
        let was = self.diag.command.as_ref().map(|c| c.name.clone());
        if now == was {
            return;
        }
        if let Some(c) = self.diag.command.take() {
            let after = self.features_json();
            let dur = c.started.elapsed().as_millis() as u64;
            if after != c.features {
                // The feature that is new or changed.
                let changed = after
                    .as_array()
                    .and_then(|a| {
                        a.iter()
                            .enumerate()
                            .find(|(i, f)| c.features.get(*i) != Some(*f))
                            .map(|(_, f)| f.clone())
                    })
                    .unwrap_or_default();
                let result = match self
                    .part()
                    .features
                    .iter()
                    .find(|f| serde_json::to_value(f).is_ok_and(|v| v == changed))
                {
                    Some(f) => match self.part().feature_health.get(f.id()) {
                        Some(msg) => json!({ "sick": msg }),
                        None => json!("ok"),
                    },
                    None => json!("ok"),
                };
                journal::global_entry(
                    "command_ok",
                    &c.name,
                    json!({ "dur_ms": dur, "feature": changed, "result": result }),
                );
                log::info!("{}: OK ({dur} ms)", c.name);
                self.diag.committed = true;
                self.diag.snap_due = true;
            } else {
                journal::global_entry("command_cancel", &c.name, json!({ "dur_ms": dur }));
            }
        }
        if let Some(name) = now {
            journal::global_entry("command_start", &name, json!({}));
            self.diag.command = Some(OpenCommand {
                name,
                started: Instant::now(),
                features: self.features_json(),
            });
        }
    }

    fn features_json(&self) -> serde_json::Value {
        serde_json::to_value(&self.part().features).unwrap_or_default()
    }

    fn diag_status(&mut self, ctx: &egui::Context) {
        if self.file_status == self.diag.status {
            return;
        }
        self.diag.status = self.file_status.clone();
        match &self.file_status {
            Some(Ok(text)) => {
                journal::global_entry("status", "", json!({ "text": text }));
                log::info!("status: {text}");
                // Opened, saved, a new project: the state may be new.
                self.diag.snap_due = true;
                let t = ctx.input(|i| i.time);
                if !self.is_dirty(t, true) {
                    crash::set_emergency(None);
                    self.diag.autosave.reset_sig();
                }
            }
            Some(Err(text)) => {
                // A feature dialog still open: the error is its OK's (or a
                // field's), and the definition it tried goes with it.
                let mut extra = json!({ "text": text });
                if let (Some(name), Some(attempt)) = (self.open_command(), self.open_attempt()) {
                    extra["command"] = json!(name);
                    extra["attempt"] = attempt;
                }
                journal::global_entry("error", "", extra);
                log::warn!("status: {text}");
            }
            None => {}
        }
    }

    fn diag_health(&mut self) {
        let now: BTreeMap<String, String> = self
            .part()
            .feature_health
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if now == self.diag.health {
            return;
        }
        for (id, msg) in &now {
            if self.diag.health.get(id) != Some(msg) {
                journal::global_entry("health", id, json!({ "feature": id, "text": msg }));
                log::warn!("feature {id} is sick: {msg}");
            }
        }
        for id in self.diag.health.keys() {
            if !now.contains_key(id) {
                journal::global_entry("health", id, json!({ "feature": id, "text": "ok" }));
                log::info!("feature {id} is healthy again");
            }
        }
        self.diag.health = now;
    }

    /// Journals the part's state and registers the project for an
    /// emergency autosave.
    fn diag_snapshot(&mut self, ctx: &egui::Context) {
        self.diag.snap_due = false;
        let label = self
            .diag
            .last_checkpoint
            .as_ref()
            .map(|(l, _)| l.clone())
            .unwrap_or_default();
        let in_sketch = self.editing.is_some();
        let part = self.part_mut();
        journal::with_global(|j| j.state_in(&label, part, in_sketch));
        let t = ctx.input(|i| i.time);
        crash::set_emergency(
            self.is_dirty(t, true)
                .then(|| (self.project.clone(), self.file_path.clone())),
        );
    }

    fn diag_autosave(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if !self.diag.autosave.due(now, self.diag.committed) {
            return;
        }
        self.diag.committed = false;
        if self
            .diag
            .autosave_job
            .as_ref()
            .is_some_and(|j| !j.is_finished())
        {
            return;
        }
        let Some(dir) = diagnostics::session()
            .and_then(|s| s.root.as_deref())
            .map(diagnostics::recovery_dir)
        else {
            self.diag.autosave.done(now, None);
            return;
        };
        let t = ctx.input(|i| i.time);
        let dirty = self.is_dirty(t, true);
        let sig = diagnostics::stable_hash(&serde_json::to_vec(&self.project).unwrap_or_default());
        if !self.diag.autosave.needs(dirty, sig) {
            self.diag.autosave.done(now, None);
            return;
        }
        let project = self.project.clone();
        let original = self.file_path.clone();
        let session = diagnostics::session_id();
        let job =
            std::thread::Builder::new().name("autosave".into()).spawn(
                move || match recovery::write(
                    &dir,
                    &project,
                    &original,
                    &session,
                    "autosave",
                    std::time::SystemTime::now(),
                ) {
                    Ok(p) => log::info!("autosave: {}", p.display()),
                    Err(e) => log::warn!("autosave failed: {e}"),
                },
            );
        match job {
            Ok(h) => {
                self.diag.autosave_job = Some(h);
                self.diag.autosave.done(now, Some(sig));
            }
            Err(e) => {
                log::warn!("autosave: cannot start: {e}");
                self.diag.autosave.done(now, None);
            }
        }
    }

    /// "Restore the autosaved version of …?" after an unclean exit.
    fn restore_window(&mut self, ctx: &egui::Context) {
        let Some(offer) = self.diag.offers.first().cloned() else {
            return;
        };
        let meta = &offer.entry.meta;
        let name = std::path::Path::new(&meta.original)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{} (never saved)", meta.project));
        let when = diagnostics::iso(
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(meta.time_ms),
        );
        let has_original =
            !meta.original.is_empty() && std::path::Path::new(&meta.original).exists();
        let mut answer = None;
        egui::Window::new("Restore")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("3DCadCraft did not close properly last time.");
                ui.label(format!("Restore the autosaved version of {name}?"));
                ui.weak(format!("Autosaved {} (UTC)", when.replace('T', " ")));
                if !meta.original.is_empty() {
                    ui.weak(format!("Original: {}", meta.original));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("Restore")
                        .on_hover_text("Open the autosave; your file is not changed until you save")
                        .clicked()
                    {
                        answer = Some(0);
                    }
                    if ui
                        .add_enabled(has_original, egui::Button::new("Open Original"))
                        .on_hover_text("Open the file as it was last saved (the autosave is kept)")
                        .clicked()
                    {
                        answer = Some(1);
                    }
                    if ui
                        .button("Discard")
                        .on_hover_text("Delete the autosaves of this project")
                        .clicked()
                    {
                        answer = Some(2);
                    }
                });
            });
        let Some(a) = answer else {
            return;
        };
        self.diag.offers.remove(0);
        match a {
            0 => self.restore_autosave(&offer),
            1 => self.open_path(std::path::Path::new(&meta.original)),
            _ => {
                recovery::discard(&offer);
                journal::global_entry("restore_discard", &name, json!({}));
            }
        }
    }

    /// Opens an autosave as the project; it saves to the original path
    /// (never automatically) and counts as unsaved.
    fn restore_autosave(&mut self, offer: &recovery::Offer) {
        let e = &offer.entry;
        let before = std::mem::replace(&mut self.file_path, e.file.display().to_string());
        self.open_project();
        if matches!(self.file_status, Some(Ok(_))) {
            self.file_path = e.meta.original.clone();
            // Unsaved changes: the title shows "*", closing asks to save.
            self.saved_sig = 0;
            self.dirty_cache = (f64::NEG_INFINITY, true);
            self.file_status = Some(Ok(format!(
                "Restored the autosave of {} - save to keep it",
                if e.meta.original.is_empty() {
                    e.meta.project.as_str()
                } else {
                    e.meta.original.as_str()
                }
            )));
            journal::global_entry(
                "restore",
                &e.meta.project,
                json!({ "file": e.file.display().to_string(), "original": e.meta.original }),
            );
        } else {
            self.file_path = before;
        }
    }

    /// The Help menu.
    pub(super) fn help_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("Help                F1").clicked() {
            self.open_help();
            ui.close();
        }
        ui.separator();
        if ui
            .button("Save Diagnostics...")
            .on_hover_text(
                "Zip the session log, the action journal, crash reports, \
                 the open project and settings.json (to send with a problem report)",
            )
            .clicked()
        {
            self.open_dialog(DialogPurpose::Diagnostics);
            ui.close();
        }
        if ui
            .button("Open Logs Folder")
            .on_hover_text("The folder with the session logs and crash reports")
            .clicked()
        {
            self.open_logs_folder();
            ui.close();
        }
    }

    /// Help → Save Diagnostics… to `path`.
    pub(super) fn save_diagnostics(&mut self, path: &std::path::Path) {
        let root = diagnostics::session().and_then(|s| s.root.clone());
        let mut inputs = diagnostics::bundle::Inputs::for_session(root.as_deref());
        inputs.project = Some(&self.project);
        self.file_status = Some(
            diagnostics::bundle::write(path, &inputs)
                .map(|names| {
                    format!(
                        "Saved diagnostics ({} files) to {}",
                        names.len(),
                        path.display()
                    )
                })
                .map_err(|e| format!("Save Diagnostics: {e}")),
        );
    }

    /// Help → Open Logs Folder (Explorer / Finder / xdg-open).
    pub(super) fn open_logs_folder(&mut self) {
        let Some(dir) = diagnostics::session()
            .and_then(|s| s.root.as_deref())
            .map(diagnostics::logs_dir)
        else {
            self.file_status = Some(Err("Open Logs Folder: no app-data folder".into()));
            return;
        };
        let _ = std::fs::create_dir_all(&dir);
        // `dir` is built with `join` from APPDATA: native separators, which
        // Explorer needs.
        let opener = if cfg!(target_os = "windows") {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        if let Err(e) = std::process::Command::new(opener).arg(&dir).spawn() {
            self.file_status = Some(Err(format!(
                "Open Logs Folder: cannot open {}: {e}",
                dir.display()
            )));
        }
    }
}
