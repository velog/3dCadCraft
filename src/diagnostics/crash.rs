//! The panic hook: a crash report in `logs/crash-YYYYMMDD-HHMMSS.txt` (the
//! panic message and place, a backtrace, the last log and journal lines)
//! and an emergency autosave of the last consistent project.
//!
//! Panics the program expects and catches (the geometry kernel's guard,
//! [`guard`]) only log one warning line: they are reported as feature
//! errors already.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::document::Project;

thread_local! {
    /// Depth of [`guard`] calls on this thread.
    static GUARDED: Cell<u32> = const { Cell::new(0) };
}

/// `std::panic::catch_unwind` for panics that are expected and handled
/// (the kernel guard): the hook writes no crash report for them.
pub fn guard<R>(f: impl FnOnce() -> R) -> std::thread::Result<R> {
    GUARDED.with(|g| g.set(g.get() + 1));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    GUARDED.with(|g| g.set(g.get().saturating_sub(1)));
    r
}

/// Whether a panic on this thread is caught by [`guard`].
fn guarded() -> bool {
    GUARDED.with(|g| g.get() > 0)
}

/// The facts of a panic.
#[derive(Clone, Debug, Default)]
pub struct PanicReport {
    /// The panic message.
    pub message: String,
    /// `file:line:column`.
    pub location: String,
    /// The panicking thread's name.
    pub thread: String,
    /// The backtrace.
    pub backtrace: String,
}

impl PanicReport {
    /// From a panic hook's info (the backtrace is captured here).
    pub fn from_hook(info: &std::panic::PanicHookInfo<'_>) -> Self {
        let p = info.payload();
        let message = p
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        Self {
            message,
            location: info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_default(),
            thread: std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_owned(),
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        }
    }
}

/// The text of a crash report.
pub fn report_text(
    r: &PanicReport,
    session: &str,
    time: SystemTime,
    log_tail: &[String],
    journal_tail: &[String],
    emergency: Option<&Result<PathBuf, String>>,
) -> String {
    let mut s = String::new();
    s += "3DCadCraft crash report\n";
    s += &format!("version: {}\n", env!("CARGO_PKG_VERSION"));
    s += &format!("os: {} {}\n", std::env::consts::OS, std::env::consts::ARCH);
    s += &format!("session: {session}\n");
    s += &format!("time: {}\n", super::iso(time));
    s += &format!("thread: {}\n", r.thread);
    s += &format!("panic: {}\n", r.message);
    s += &format!("at: {}\n", r.location);
    match emergency {
        Some(Ok(p)) => s += &format!("emergency autosave: {}\n", p.display()),
        Some(Err(e)) => s += &format!("emergency autosave failed: {e}\n"),
        None => s += "emergency autosave: none (no project registered)\n",
    }
    s += "\n--- backtrace ---\n";
    s += &r.backtrace;
    s += &format!("\n--- last {} log lines ---\n", log_tail.len());
    for l in log_tail {
        s += l;
        s += "\n";
    }
    s += &format!("\n--- last {} journal lines ---\n", journal_tail.len());
    for l in journal_tail {
        s += l;
        s += "\n";
    }
    s
}

/// Writes a crash report into `logs` (`crash-<stamp>.txt`).
pub fn write_report(logs: &Path, text: &str, time: SystemTime) -> Result<PathBuf, String> {
    let base = format!("crash-{}", super::stamp(time));
    let mut path = logs.join(format!("{base}.txt"));
    let mut n = 2;
    while path.exists() {
        path = logs.join(format!("{base}-{n}.txt"));
        n += 1;
    }
    super::write_atomic(&path, text.as_bytes())?;
    Ok(path)
}

/// The last consistent project, for an emergency autosave.
struct Emergency {
    project: Project,
    original: String,
}

static EMERGENCY: Mutex<Option<Emergency>> = Mutex::new(None);

/// Registers the project as it is now (a consistent state, e.g. after a
/// commit) for an emergency autosave. `None` clears it.
pub fn set_emergency(project: Option<(Project, String)>) {
    *super::lock(&EMERGENCY) = project.map(|(project, original)| Emergency { project, original });
}

/// Writes an emergency autosave of the registered project into `dir`.
/// Never blocks: a busy lock means no autosave.
pub fn emergency_save(dir: &Path, session: &str) -> Option<Result<PathBuf, String>> {
    let guard = EMERGENCY.try_lock().ok()?;
    let e = guard.as_ref()?;
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::recovery::write(
            dir,
            &e.project,
            &e.original,
            session,
            "emergency",
            SystemTime::now(),
        )
    }));
    Some(r.unwrap_or_else(|_| Err("panicked while saving".into())))
}

/// Handles a panic: logs it, and unless it is [`guard`]ed writes a crash
/// report and an emergency autosave. Returns the report's path.
pub fn handle(r: &PanicReport, root: Option<&Path>, session: &str) -> Option<PathBuf> {
    log::error!(
        "panic in thread {}: {} at {}",
        r.thread,
        r.message,
        r.location
    );
    let root = root?;
    let emergency = emergency_save(&super::recovery_dir(root), session);
    let now = SystemTime::now();
    let text = report_text(
        r,
        session,
        now,
        &super::log::tail(),
        &super::journal::global_tail(),
        emergency.as_ref(),
    );
    match write_report(&super::logs_dir(root), &text, now) {
        Ok(p) => {
            eprintln!("3dcadcraft: crash report written to {}", p.display());
            super::journal::global_entry(
                "crash",
                "",
                serde_json::json!({ "text": r.message, "at": r.location, "report": p.display().to_string() }),
            );
            Some(p)
        }
        Err(e) => {
            eprintln!("3dcadcraft: cannot write the crash report: {e}");
            None
        }
    }
}

/// Installs the panic hook (keeps the previous one, which prints the panic).
pub fn install_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if guarded() {
            let at = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_default();
            log::warn!("caught panic in the geometry kernel at {at}");
            return;
        }
        let report = PanicReport::from_hook(info);
        let root = super::session().and_then(|s| s.root.clone());
        handle(&report, root.as_deref(), &super::session_id());
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_report_content() {
        let dir = tempfile::tempdir().unwrap();
        let r = PanicReport {
            message: "index out of bounds: the len is 3 but the index is 7".into(),
            location: "src/part/blend.rs:42:9".into(),
            thread: "main".into(),
            backtrace: "   0: cadcraft3d::part::blend::build\n".into(),
        };
        let logs = vec!["2026-10-19T08:00:00.000Z INFO  [main] cadcraft3d: hello".to_owned()];
        let journal = vec![r#"{"kind":"checkpoint","label":"Fillet"}"#.to_owned()];
        let saved: Result<PathBuf, String> = Ok(PathBuf::from("r/x.3dcadcraft"));
        let t = SystemTime::now();
        let text = report_text(&r, "session-1", t, &logs, &journal, Some(&saved));
        let p = write_report(dir.path(), &text, t).unwrap();
        let back = std::fs::read_to_string(&p).unwrap();
        for want in [
            "panic: index out of bounds",
            "at: src/part/blend.rs:42:9",
            "thread: main",
            "session: session-1",
            "--- backtrace ---",
            "cadcraft3d::part::blend::build",
            "cadcraft3d: hello",
            r#""label":"Fillet""#,
            "emergency autosave: r",
        ] {
            assert!(back.contains(want), "missing {want:?} in\n{back}");
        }
        assert!(
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("crash-")
        );
        // A second report in the same second gets its own file.
        let q = write_report(dir.path(), &text, t).unwrap();
        assert_ne!(p, q);
    }

    #[test]
    fn handle_writes_report_and_emergency_autosave() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::new("Crashy");
        project.add_document(
            "Part1",
            crate::document::DocumentData::Part(crate::part::PartDocument::default()),
        );
        set_emergency(Some((project, String::new())));
        let r = PanicReport {
            message: "boom".into(),
            location: "x.rs:1:1".into(),
            thread: "main".into(),
            backtrace: String::new(),
        };
        let report = handle(&r, Some(dir.path()), "session-t").unwrap();
        set_emergency(None);
        let text = std::fs::read_to_string(report).unwrap();
        assert!(text.contains("panic: boom"));
        let saves = super::super::recovery::list(&super::super::recovery_dir(dir.path()));
        assert_eq!(saves.len(), 1);
        assert_eq!(saves[0].meta.kind, "emergency");
        assert_eq!(saves[0].meta.session, "session-t");
        assert!(text.contains("emergency autosave: "));
    }

    #[test]
    fn guarded_panics_are_caught() {
        let r = guard(|| -> u32 { std::panic::panic_any("expected") });
        assert!(r.is_err());
        assert!(!guarded());
        assert_eq!(guard(|| 5).ok(), Some(5));
    }
}
