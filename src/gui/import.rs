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
use super::part_planned::PartDialog;
use crate::part::import::ImportOp;
use crate::part::{BodyOp, BodyOpFeature};

/// The STEP text of `path`.
fn read_step(path: &Path) -> Result<Arc<str>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // STEP is ASCII (ISO 10303-21); stray bytes in comments or names must not stop it.
    Ok(String::from_utf8_lossy(&bytes).into())
}

/// What the worker found.
enum Read {
    /// Bodies for one part: how many the filters keep, and the reader's warnings.
    Part(usize, Vec<String>),
    /// File → Open of an assembly file: the design file made from it.
    Assembly(
        Box<crate::document::Project>,
        crate::assembly::import::AssemblyImport,
    ),
}

type ReadResult = Result<Read, String>;

/// A STEP file being read on a worker thread.
pub(super) struct ImportJob {
    path: PathBuf,
    text: Arc<str>,
    /// File → Open (a new part) or the Import panel's Browse.
    open: bool,
    started: std::time::Instant,
    rx: Receiver<ReadResult>,
}

/// The worker's job: an assembly file opened becomes a design file of documents; anything else
/// is read as the bodies of one part.
fn read_in_worker(text: &Arc<str>, open: bool, file: &str, stem: &str) -> ReadResult {
    if open {
        let r = crate::kernel::exact::step_bodies(text, "Import1")?;
        if crate::assembly::import::is_assembly(&r) {
            let mut p = crate::document::Project::new(stem);
            let out = crate::assembly::import::step_project(&r, text, file, &mut p)?;
            return Ok(Read::Assembly(Box::new(p), out));
        }
    }
    ImportOp::new("", text.clone())
        .read("Import1")
        .map(|(b, w)| Read::Part(b.len(), w))
}

impl ImportJob {
    fn start(path: &Path, open: bool) -> Result<Self, String> {
        let text = read_step(path)?;
        let (tx, rx) = std::sync::mpsc::channel();
        let t = text.clone();
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imported".into());
        std::thread::Builder::new()
            .name("step-import".into())
            .spawn(move || {
                let _ = tx.send(read_in_worker(&t, open, &file, &stem));
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
            "read {} in {:.2} s: {}",
            job.path.display(),
            job.started.elapsed().as_secs_f64(),
            match &res {
                Ok(Read::Part(n, w)) => format!("{n} bodies, {} warnings", w.len()),
                Ok(Read::Assembly(_, o)) => format!("{o:?}"),
                Err(e) => e.clone(),
            }
        );
        let (bodies, warnings) = match res {
            Ok(Read::Part(n, w)) => (n, w),
            Ok(Read::Assembly(p, out)) => {
                self.open_imported_assembly(*p, out, &name);
                return;
            }
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
        let Some(d) = self.part_planned.as_mut().filter(|d| d.kind.is_import()) else {
            return;
        };
        d.set_text(0, "File", &job.path.display().to_string());
        d.file_text = Some(job.text);
        d.note = Some(line.clone());
        self.file_status = Some(Ok(format!("Import: {line}; OK brings it in")));
    }

    /// File → Open of an assembly file: its design file in a new tab, the top assembly active.
    fn open_imported_assembly(
        &mut self,
        project: crate::document::Project,
        out: crate::assembly::import::AssemblyImport,
        name: &str,
    ) {
        let Some(top) = out.top else {
            return;
        };
        self.add_blank_tab();
        self.new_project();
        self.project = project;
        self.part_id = top;
        self.reset_asm_ui();
        self.history.clear();
        self.after_restore();
        self.scene_dirty = true;
        self.fit_view();
        self.mark_saved();
        let mut line = format!(
            "Imported {name}: {} part(s), {} assembly(ies), {} occurrence(s)",
            out.parts, out.assemblies, out.occurrences
        );
        if let Some(w) = out.warnings.first() {
            line += &format!("; {} warning(s), first: {w}", out.warnings.len());
        }
        self.file_status = Some(Ok(line));
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
        // File → Open follows the file (Reference Model): Update brings its changes.
        let path = job.path.display().to_string();
        let op = ImportOp {
            link: Some(crate::part::import::SourceLink::now(&path)),
            ..ImportOp::new(path, job.text.clone())
        };
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

    /// Place Imported CAD Files' OK: the file's assembly (a subassembly of new documents) or one
    /// multi-body part, placed at the active assembly's origin as one Undo step. Returns whether
    /// the panel closes.
    pub(super) fn place_imported_ok(&mut self, d: &PartDialog) -> bool {
        const TITLE: &str = "Place Imported CAD Files";
        match self.place_imported(d) {
            Ok(line) => {
                self.file_status = Some(Ok(format!("{TITLE}: {line}")));
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("{TITLE}: {e}")));
                false
            }
        }
    }

    fn place_imported(&mut self, d: &PartDialog) -> Result<String, String> {
        if !self.is_assembly() {
            return Err("no assembly is active".into());
        }
        if self.import_job.is_some() {
            return Err("the file is still being read".into());
        }
        let file = d.text(0, "File").trim().to_owned();
        let text = match &d.file_text {
            Some(t) => t.clone(),
            None if file.is_empty() => return Err("choose a STEP file (Browse...)".into()),
            None => read_step(Path::new(&file))?,
        };
        let path = Path::new(&file);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imported".into());
        let stem = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imported".into());
        let r = crate::kernel::exact::step_bodies(&text, "Import1")?;
        let multi = d.index(0, "Import as") == 1;
        // On a copy: nothing changes unless all of it works (rule 5).
        let mut p = self.project.clone();
        let (comp, line) = if !multi && crate::assembly::import::is_assembly(&r) {
            let out = crate::assembly::import::step_project(&r, &text, &name, &mut p)?;
            let top = out.top.ok_or("no component of the file could be read")?;
            let mut line = format!(
                "{name} placed: {} part(s), {} assembly(ies)",
                out.parts, out.assemblies
            );
            if let Some(w) = out.warnings.first() {
                line += &format!("; {} warning(s), first: {w}", out.warnings.len());
            }
            (top, line)
        } else {
            let op = ImportOp {
                solids: d.check(0, "Solids"),
                surfaces: d.check(0, "Surfaces"),
                ..ImportOp::new(file.clone(), text.clone())
            };
            let mut part = crate::part::PartDocument::default();
            part.add_body_op(BodyOpFeature::new(BodyOp::Import(op)))
                .map_err(|e| e.to_string())?;
            let n = part.exact_bodies.0.len();
            let taken = |s: &str| p.documents().iter().any(|d| d.name == s);
            let mut doc = stem.clone();
            let mut k = 1;
            while taken(&doc) {
                k += 1;
                doc = format!("{stem}_{k}");
            }
            let id = p.add_document(doc, crate::document::DocumentData::Part(part));
            (id, format!("{name} placed as one part ({n} body(ies))"))
        };
        let Some(crate::document::DocumentData::Assembly(a)) =
            p.document_mut(self.part_id).map(|x| &mut x.data)
        else {
            return Err("no assembly is active".into());
        };
        let first = a.occurrences.is_empty();
        let occ = a.place(
            crate::document::DocRef::local(comp),
            &stem,
            crate::kernel::Transform3::identity(),
            first,
        );
        self.history.break_coalescing();
        self.checkpoint("Place Imported CAD Files");
        self.project = p;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = vec![occ];
        self.asm.sub.clear();
        self.scene_dirty = true;
        Ok(line)
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
        // Reference Model follows the file; an update keeps its matched names (`keep`).
        let link =
            (d.index(0, "Import Type") == 0).then(|| crate::part::import::SourceLink::now(&file));
        let keep = prev
            .filter(|o| Arc::ptr_eq(&o.text, &text))
            .map(|o| o.keep.clone())
            .unwrap_or_default();
        let op = ImportOp {
            solids: d.check(0, "Solids"),
            surfaces: d.check(0, "Surfaces"),
            link,
            keep,
            ..ImportOp::new(file, text)
        };
        op.validate()?;
        Ok(BodyOp::Import(op))
    }
}

/// The Reference Model imports of the active part whose file changed, checked every 2 s.
#[derive(Default)]
pub(super) struct Stale {
    /// When the files were last looked at (egui time).
    checked: f64,
    /// Feature indices of the changed imports.
    pub features: Vec<usize>,
}

impl CadCraftApp {
    /// The Quick Access toolbar's Update button: shown while a referenced file changed (Inventor's
    /// Update icon). Click: every changed import of the part reads its file again.
    pub(super) fn import_update_button(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        if now - self.import_stale.checked > 2.0 || now < self.import_stale.checked {
            self.import_stale.checked = now;
            self.import_stale.features = if self.is_assembly() || self.is_drawing() {
                Vec::new()
            } else {
                self.part()
                    .features
                    .iter()
                    .enumerate()
                    .filter_map(|(k, f)| match f {
                        crate::part::Feature::BodyOp(b) => match &b.op {
                            BodyOp::Import(o) if o.link.as_ref().is_some_and(|l| l.changed()) => {
                                Some(k)
                            }
                            _ => None,
                        },
                        _ => None,
                    })
                    .collect()
            };
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(2));
        }
        if self.import_stale.features.is_empty() {
            return;
        }
        let tip = "A referenced file changed: read it again (faces that stay keep their names)";
        if ui
            .button(
                egui::RichText::new("Update")
                    .color(ui.visuals().warn_fg_color)
                    .strong(),
            )
            .on_hover_text(tip)
            .clicked()
        {
            self.update_imports();
        }
    }

    /// Update: each changed import reads its file again, as one Undo step.
    fn update_imports(&mut self) {
        let ks = std::mem::take(&mut self.import_stale.features);
        let mut lines = Vec::new();
        let mut failed = Vec::new();
        self.history.break_coalescing();
        self.checkpoint("Update");
        let mut changed = false;
        for k in ks {
            let Some(crate::part::Feature::BodyOp(b)) = self.part().features.get(k).cloned() else {
                continue;
            };
            let BodyOp::Import(op) = &b.op else { continue };
            let Some(link) = &op.link else { continue };
            let r = read_step(Path::new(&link.path))
                .and_then(|t| op.updated(&b.id, t))
                .and_then(|(next, report)| {
                    let mut nb = b.clone();
                    nb.op = BodyOp::Import(next);
                    self.part_mut()
                        .replace_feature(k, crate::part::Feature::BodyOp(nb))
                        .map(|_| report)
                        .map_err(|e| e.to_string())
                });
            match r {
                Ok(rep) => {
                    changed = true;
                    lines.push(format!(
                        "{}: {} face(s) kept, {} new, {} gone",
                        b.name, rep.matched, rep.new, rep.lost
                    ));
                }
                Err(e) => failed.push(format!("{}: {e}", b.name)),
            }
        }
        if changed {
            self.history.break_coalescing();
            self.scene_dirty = true;
        } else {
            self.history.cancel();
        }
        let sick = self.part().feature_health.len();
        self.file_status = Some(if failed.is_empty() {
            let mut s = format!("Update: {}", lines.join("; "));
            if sick > 0 {
                s += &format!(" ({sick} feature(s) lost a reference: see the browser)");
            }
            Ok(s)
        } else {
            Err(format!("Update: {}", failed.join("; ")))
        });
    }
}
