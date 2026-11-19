//! Work off the main thread (spec 0.19 §3.3): a command's OK, Rebuild All and the regeneration
//! after an edit build the part on a worker thread from a copy, while the view stays usable.
//!
//! One job at a time ([`BgJob`] in `CadCraftApp::bg`). [`CadCraftApp::part_job`] copies the active
//! part, runs the work on the copy on a worker (64 MB stack, progress reporter, Esc cancels) and
//! returns at once; [`CadCraftApp::bg_tick`] (once a frame) takes the finished copy:
//!
//! - success, and the part is still the one the job started from (same document, same
//!   [`CadCraftApp::part_signature`]): the Undo checkpoint is recorded *then*, with the state before
//!   the OK, and the copy replaces the part — one step, as before;
//! - failure: nothing changes (rule 5), the command's continuation reports the error;
//! - the part changed meanwhile: the result is dropped with "<label>: the part changed while it
//!   was built; press OK again".
//!
//! While a job runs, commands that change the document wait ([`allowed_while_building`]): the
//! view, the ViewCube, Zoom, visual styles, Measure and Help stay. The command's panel stays open
//! (its OK does nothing until the job ends). Without the option (Application Options › Part ›
//! *Build in the background*), and in tests, the same code runs at once on the main thread, so
//! both paths commit the same thing.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use super::CadCraftApp;
use super::ribbon::Action;
use crate::part::PartDocument;

/// What the main thread does once the job's part was taken (`Ok` with the work's answer, e.g. the
/// new feature's id) or the job failed (`Err` with the reason, the part unchanged).
pub(super) type Then = Box<dyn FnOnce(&mut CadCraftApp, Result<String, String>)>;

type Done = (PartDocument, Result<String, String>);

/// The running job.
pub(super) struct BgJob {
    /// The command, for the status bar and messages (`Extrude`, `Rebuild All`).
    pub(super) label: String,
    /// The Undo label recorded when the result is taken (`None`: no data changes, Rebuild All).
    checkpoint: Option<String>,
    doc: crate::document::DocumentId,
    sig: u64,
    rx: Receiver<Done>,
    cancel: Arc<AtomicBool>,
    then: Option<Then>,
    started: std::time::Instant,
}

impl BgJob {
    /// How long the job has run.
    pub(super) fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }
}

/// Whether `action` may run while a job builds the part: it only changes the view (or opens
/// Help / Measure, which read the old body), never the document.
pub(super) fn allowed_while_building(action: Action) -> bool {
    use super::commands::AppCmd;
    match action {
        Action::View(_)
        | Action::LookAt
        | Action::VisualStyle(_)
        | Action::Projection(_)
        | Action::ShowConstraints(_)
        | Action::Ui(_) => true,
        Action::App(c) => matches!(
            c,
            AppCmd::ZoomAll
                | AppCmd::ZoomSelected
                | AppCmd::PreviousView
                | AppCmd::NextView
                | AppCmd::ZoomWindow
                | AppCmd::Measure
                | AppCmd::Help
                | AppCmd::FindInBrowser
                | AppCmd::OpenLogsFolder
        ),
        _ => false,
    }
}

/// Runs `work` on `trial` here and now (a panic becomes the error).
fn build_now(
    mut trial: PartDocument,
    work: impl FnOnce(&mut PartDocument) -> Result<String, String>,
) -> Done {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&mut trial)))
        .unwrap_or_else(|_| Err("internal error while building the part".into()));
    (trial, r)
}

/// Runs `work` on `trial` on a worker thread (64 MB stack, a cancellable progress reporter named
/// `label`): the receiver gets the result, the flag cancels it.
fn build_on_worker(
    trial: PartDocument,
    label: &str,
    work: impl FnOnce(&mut PartDocument) -> Result<String, String> + Send + 'static,
) -> std::io::Result<(Receiver<Done>, Arc<AtomicBool>)> {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let name = label.to_owned();
    std::thread::Builder::new()
        .name("build".into())
        // The kernel recurses deeply on large bodies (the main thread has 64 MB too).
        .stack_size(64 << 20)
        .spawn(move || {
            let _progress = crate::part::progress::Reporter::install_cancellable(&name, flag);
            let _ = tx.send(build_now(trial, work));
        })?;
    Ok((rx, cancel))
}

/// `<Command>: <reason>` (0-ui), unless the reason already names the command (a result dropped
/// because the part changed).
pub(super) fn with_cmd(cmd: &str, e: String) -> String {
    if e.starts_with(&format!("{cmd}:")) {
        e
    } else {
        format!("{cmd}: {e}")
    }
}

/// The message of a result dropped because the part changed while it was built.
pub(super) fn changed_message(label: &str) -> String {
    format!("{label}: the part changed while it was built; press OK again")
}

impl CadCraftApp {
    /// Whether a job is building the part.
    pub(super) fn building(&self) -> bool {
        self.bg.is_some()
    }

    /// Refuses a document change while a job runs (status bar), true when refused.
    pub(super) fn refuse_while_building(&mut self, what: &str) -> bool {
        let Some(j) = &self.bg else {
            return false;
        };
        self.file_status = Some(Err(format!(
            "{what}: {} is still building; wait or press Esc to cancel it",
            j.label
        )));
        true
    }

    /// Runs `work` on a copy of the active part — on a worker when the option is on, else at once
    /// — and takes the copy when it succeeds (Undo `checkpoint` recorded at that moment), then
    /// calls `then` with the work's result. Refused (with a status message, `then` not called)
    /// while another job runs.
    pub(super) fn part_job(
        &mut self,
        label: &str,
        checkpoint: Option<&str>,
        work: impl FnOnce(&mut PartDocument) -> Result<String, String> + Send + 'static,
        then: impl FnOnce(&mut Self, Result<String, String>) + 'static,
    ) {
        if self.refuse_while_building(label) {
            return;
        }
        let trial = self.part().clone();
        let started = std::time::Instant::now();
        let (doc, sig) = (self.part_id, self.part_signature());
        let job = |rx, cancel| BgJob {
            label: label.to_owned(),
            checkpoint: checkpoint.map(str::to_owned),
            doc,
            sig,
            rx,
            cancel,
            then: Some(Box::new(then)),
            started,
        };
        if !self.background_ok {
            let (_, rx) = std::sync::mpsc::channel();
            let job = job(rx, Arc::new(AtomicBool::new(false)));
            let done = build_now(trial, work);
            self.bg_finish(job, done);
            return;
        }
        crate::diagnostics::activity(&format!("background {label}"));
        match build_on_worker(trial, label, work) {
            Ok((rx, cancel)) => self.bg = Some(job(rx, cancel)),
            Err(e) => self.file_status = Some(Err(format!("{label}: could not start ({e})"))),
        }
    }

    /// Once a frame: takes a finished job's result.
    pub(super) fn bg_tick(&mut self, ctx: &eframe::egui::Context) {
        let Some(job) = &self.bg else {
            return;
        };
        let done = match job.rx.try_recv() {
            Ok(d) => d,
            Err(TryRecvError::Empty) => {
                // the worker does not wake the frame
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
                return;
            }
            Err(TryRecvError::Disconnected) => (
                PartDocument::default(),
                Err("internal error while building the part".into()),
            ),
        };
        if let Some(job) = self.bg.take() {
            self.bg_finish(job, done);
        }
    }

    /// Esc while a job runs: the job stops where it can and its result is never taken.
    pub(super) fn bg_cancel(&mut self) -> bool {
        let Some(job) = self.bg.take() else {
            return false;
        };
        job.cancel.store(true, Ordering::Relaxed);
        log::info!(
            "{}: cancelled after {:.1} s",
            job.label,
            job.elapsed().as_secs_f64()
        );
        self.file_status = Some(Err(format!("{}: cancelled", job.label)));
        true
    }

    fn bg_finish(&mut self, mut job: BgJob, (part, result): Done) {
        let then = job.then.take();
        let result = match result {
            Ok(_) if job.doc != self.part_id || job.sig != self.part_signature() => {
                Err(changed_message(&job.label))
            }
            Ok(answer) => {
                if let Some(label) = &job.checkpoint {
                    self.checkpoint(label);
                }
                *self.part_mut() = part;
                self.edge_cache.replace(None);
                self.scene_dirty = true;
                Ok(answer)
            }
            Err(e) => Err(e),
        };
        if let Some(then) = then {
            then(self, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{BlendFeature, EdgeRef};

    fn fillet() -> BlendFeature {
        BlendFeature::fillet(
            vec![EdgeRef {
                faces: ["Box1:Top".into(), "Box1:Front".into()],
                hint: [20.0, 0.0, 20.0],
            }],
            3.0,
        )
    }

    fn names(p: &PartDocument) -> Vec<String> {
        let mut v: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
        v.sort();
        v
    }

    /// What a worker builds is what the main thread builds: the same features, volume and face
    /// names (spec 0.19 §3.3).
    #[test]
    fn a_worker_builds_what_the_main_thread_builds() {
        let part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let add = |p: &mut PartDocument| p.add_blend(fillet()).map_err(|e| e.to_string());
        let (now, r_now) = build_now(part.clone(), add);
        let (rx, _cancel) = build_on_worker(part.clone(), "test fillet", add).unwrap();
        let (bg, r_bg) = rx.recv().unwrap();
        assert_eq!(r_now, r_bg);
        assert!(r_now.is_ok());
        assert_eq!(now.features.len(), part.features.len() + 1);
        assert_eq!(
            serde_json::to_string(&now.features).unwrap(),
            serde_json::to_string(&bg.features).unwrap()
        );
        assert_eq!(names(&now), names(&bg));
        assert!(names(&now).iter().any(|n| n.starts_with("Fillet1:")));
    }

    /// A failing OK leaves the part as it was and says why; a panic in the work is an error too.
    #[test]
    fn a_failure_reports_and_the_original_is_untouched() {
        let part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let mut bad = fillet();
        bad.edges[0].faces = ["Nope:A".into(), "Nope:B".into()];
        let (rx, _c) = build_on_worker(part.clone(), "test bad", move |p| {
            p.add_blend(bad).map_err(|e| e.to_string())
        })
        .unwrap();
        let (_, r) = rx.recv().unwrap();
        assert!(r.is_err());
        let (_, r) = build_now(part.clone(), |_| -> Result<String, String> {
            std::panic::panic_any("boom")
        });
        assert_eq!(r, Err("internal error while building the part".into()));
        assert_eq!(part.features.len(), 1);
    }

    /// A cancelled job stops at the next feature: a rebuild of many features ends early.
    #[test]
    fn a_cancelled_rebuild_stops_between_features() {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        part.add_blend(fillet()).unwrap();
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let (rx, cancel) = build_on_worker(part, "test cancel", move |p| {
            let _ = wait.recv();
            p.build_cache = Default::default();
            p.regenerate()
                .map(|_| String::new())
                .map_err(|e| e.to_string())
        })
        .unwrap();
        cancel.store(true, Ordering::Relaxed);
        go.send(()).unwrap();
        let (p, _) = rx.recv().unwrap();
        // nothing was built: the loop stopped before the first feature
        assert_eq!(p.body.faces().count(), 0);
    }

    #[test]
    fn only_view_commands_run_while_building() {
        use super::super::commands::AppCmd;
        use super::super::view_style::VisualStyle;
        assert!(allowed_while_building(Action::App(AppCmd::ZoomAll)));
        assert!(allowed_while_building(Action::VisualStyle(
            VisualStyle::default()
        )));
        for a in [
            Action::Undo,
            Action::Redo,
            Action::Extrude,
            Action::RebuildAll,
            Action::App(AppCmd::Save),
            Action::App(AppCmd::Open),
        ] {
            assert!(!allowed_while_building(a), "{a:?}");
        }
    }
}
