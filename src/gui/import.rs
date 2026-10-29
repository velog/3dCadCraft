//! Import of STEP files (`docs/spec/7.7a-import.md`): the Import panel's Browse... and OK (through
//! `surface_cmd`'s planned-panel flow), and File → Open of a `.step` / `.stp` (a new part with one
//! Import feature, Inventor's Open → Import CAD Formats).
//!
//! A file is read once on a worker thread (`ImportJob`) before anything changes: a large or
//! difficult file keeps the window responsive and can be cancelled. The reader keeps its recent
//! results, so the feature built afterwards (on the main thread) does not read it again.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui;

use super::CadCraftApp;
use super::part_planned::{PartDialog, PartPlanned, Planned};
use crate::part::import::ImportOp;
use crate::part::{BodyOp, BodyOpFeature};

/// The STEP text of `path`.
fn read_step(path: &Path) -> Result<Arc<str>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // STEP is ASCII (ISO 10303-21); stray bytes in comments or names must not stop it.
    Ok(String::from_utf8_lossy(&bytes).into())
}

/// What the worker found: the bodies kept and the reader's warnings.
type ReadResult = Result<(usize, Vec<String>), String>;

/// A STEP file being read on a worker thread.
pub(super) struct ImportJob {
    path: PathBuf,
    text: Arc<str>,
    /// File → Open (a new part) or the Import panel's Browse.
    open: bool,
    started: std::time::Instant,
    rx: Receiver<ReadResult>,
}

impl ImportJob {
    fn start(path: &Path, open: bool) -> Result<Self, String> {
        let text = read_step(path)?;
        let (tx, rx) = std::sync::mpsc::channel();
        let t = text.clone();
        std::thread::Builder::new()
            .name("step-import".into())
            .spawn(move || {
                let r = ImportOp::new("", t)
                    .read("Import1")
                    .map(|(b, w)| (b.len(), w));
                let _ = tx.send(r);
            })
            .map_err(|e| format!("cannot start the reader ({e})"))?;
        Ok(Self {
            path: path.to_path_buf(),
            text,
            open,
            started: std::time::Instant::now(),
            rx,
        })
    }

    fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// The status line of a read file.
fn summary(name: &str, bodies: usize, warnings: &[String]) -> String {
    let mut s = format!("{name}: {bodies} body(ies)");
    if let Some(w) = warnings.first() {
        s += &format!("; {} warning(s), first: {w}", warnings.len());
    }
    s
}

impl CadCraftApp {
    fn start_import_job(&mut self, path: &Path, open: bool) {
        match ImportJob::start(path, open) {
            Ok(job) => {
                self.file_status = Some(Ok(format!("Import: reading {}...", job.file_name())));
                self.import_job = Some(job);
            }
            Err(e) => self.file_status = Some(Err(format!("Import: {e}"))),
        }
    }

    /// Browse... of the Import panel chose `path`: it is read, then the panel takes it.
    pub(super) fn import_file_picked(&mut self, path: &Path) {
        if let Some(d) = self.part_planned.as_mut() {
            d.note = Some("Reading the file...".into());
        }
        self.start_import_job(path, false);
    }

    /// File → Open of a STEP file: read it, then a new part (its own tab) holding one Import
    /// feature.
    pub(super) fn open_step(&mut self, path: &Path) {
        self.start_import_job(path, true);
    }

    /// The running read: its window (Cancel) and, once done, its result. Called once per frame.
    pub(super) fn import_ui(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.import_job else {
            return;
        };
        let res = match job.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err("the reader stopped (see the session log)".into()))
            }
        };
        let Some(res) = res else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            let name = job.file_name();
            let secs = job.started.elapsed().as_secs();
            let mut cancel = false;
            egui::Window::new("Import")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("Reading {name}... {secs} s"));
                    });
                    cancel = ui.button("Cancel").clicked();
                });
            if cancel {
                // The worker cannot be stopped; its result is dropped.
                log::warn!("import of {name} cancelled after {secs} s");
                self.import_job = None;
                self.file_status = Some(Err(format!("Import: {name} cancelled")));
                if let Some(d) = self.part_planned.as_mut() {
                    d.note = None;
                }
            }
            return;
        };
        let Some(job) = self.import_job.take() else {
            return;
        };
        let name = job.file_name();
        log::info!(
            "read {} in {:.2} s: {:?}",
            job.path.display(),
            job.started.elapsed().as_secs_f64(),
            res.as_ref().map(|(n, w)| (n, w.len()))
        );
        let (bodies, warnings) = match res {
            Ok(r) => r,
            Err(e) => {
                self.file_status = Some(Err(format!("Import: {name}: {e}")));
                if let Some(d) = self.part_planned.as_mut() {
                    d.note = Some(format!("Cannot be read: {e}"));
                }
                return;
            }
        };
        let line = summary(&name, bodies, &warnings);
        if job.open {
            self.import_into_new_part(&job, &name, &line);
            return;
        }
        let Some(d) = self
            .part_planned
            .as_mut()
            .filter(|d| d.kind == Planned::Part(PartPlanned::Import))
        else {
            return;
        };
        d.set_text(0, "File", &job.path.display().to_string());
        d.file_text = Some(job.text);
        d.note = Some(line.clone());
        self.file_status = Some(Ok(format!("Import: {line}; OK brings it in")));
    }

    fn import_into_new_part(&mut self, job: &ImportJob, name: &str, line: &str) {
        let stem = job
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imported".into());
        self.add_blank_tab();
        self.new_project();
        self.project.name = stem.clone();
        if let Some(doc) = self.project.document_mut(self.part_id) {
            doc.name = stem;
        }
        let op = ImportOp::new(job.path.display().to_string(), job.text.clone());
        match self
            .part_mut()
            .add_body_op(BodyOpFeature::new(BodyOp::Import(op)))
        {
            Ok(_) => {
                self.scene_dirty = true;
                self.fit_view();
                self.mark_saved();
                self.file_status = Some(Ok(format!(
                    "Imported {line} (Save As keeps it as a part file)"
                )));
            }
            Err(e) => self.file_status = Some(Err(format!("Import: {name}: {e}"))),
        }
    }

    /// The Import panel's definition: the file Browse... read, else the embedded one of the
    /// feature being edited (same path), else the typed path read now.
    pub(super) fn import_op_from(
        &self,
        d: &PartDialog,
        old: Option<&BodyOp>,
    ) -> Result<BodyOp, String> {
        if self.import_job.is_some() {
            return Err("the file is still being read".into());
        }
        let file = d.text(0, "File").trim().to_owned();
        let prev = match old {
            Some(BodyOp::Import(o)) => Some(o),
            _ => None,
        };
        let text = match (&d.file_text, prev) {
            (Some(t), _) => t.clone(),
            (None, Some(o)) if o.file == file || file.is_empty() => o.text.clone(),
            (None, _) if file.is_empty() => return Err("choose a STEP file (Browse...)".into()),
            (None, _) => read_step(Path::new(&file))?,
        };
        let file = if file.is_empty() {
            prev.map(|o| o.file.clone()).unwrap_or_default()
        } else {
            file
        };
        let op = ImportOp {
            solids: d.check(0, "Solids"),
            surfaces: d.check(0, "Surfaces"),
            ..ImportOp::new(file, text)
        };
        op.validate()?;
        Ok(BodyOp::Import(op))
    }
}
