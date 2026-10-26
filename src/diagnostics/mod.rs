//! Diagnostics and recovery: what happened in a session, and getting the
//! user's work back after a crash (`docs/spec/0.14-diagnostics.md`).
//!
//! Everything lives under the app-data root ([`root`]:
//! `%APPDATA%\3dcadcraft` on Windows, `~/.config/3dcadcraft` elsewhere,
//! `CADCRAFT3D_DIAG_DIR` overrides it):
//!
//! ```text
//! logs/session-YYYYMMDD-HHMMSS.log            log (UTC timestamps, thread names)
//! logs/session-YYYYMMDD-HHMMSS-journal.jsonl  one JSON line per user action
//! logs/session-YYYYMMDD-HHMMSS.json           session sidecar (clean exit?)
//! logs/crash-YYYYMMDD-HHMMSS.txt              panic report
//! recovery/<project>-<hash>-<stamp>.3dcadcraft (+ .json sidecar)  autosaves
//! ```
//!
//! * [`log`] — the logger (file + stderr + a ring buffer for crash reports);
//! * [`journal`] — the action journal and its part snapshots (JSON deltas);
//! * [`replay`] — rebuilds a part from a journal step by step;
//! * [`recovery`] — autosave files and the Restore decision;
//! * [`crash`] — the panic hook and crash reports;
//! * [`bundle`] — Help → Save Diagnostics (a zip of all of it).
//!
//! No function here panics; file problems are logged or returned.

pub mod bundle;
pub mod crash;
pub mod journal;
pub mod log;
pub mod recovery;
pub mod replay;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Sessions (log + journal + sidecar) kept in `logs/`.
pub const KEEP_SESSIONS: usize = 20;
/// Crash reports kept in `logs/`.
pub const KEEP_CRASHES: usize = 20;

/// The app-data root: `CADCRAFT3D_DIAG_DIR` if set, else
/// [`crate::presets::config_dir`].
pub fn root() -> Option<PathBuf> {
    std::env::var_os("CADCRAFT3D_DIAG_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(crate::presets::config_dir)
}

/// `<root>/logs`.
pub fn logs_dir(root: &Path) -> PathBuf {
    root.join("logs")
}

/// `<root>/recovery`.
pub fn recovery_dir(root: &Path) -> PathBuf {
    root.join("recovery")
}

// ------------------------------------------------------------- session ---

/// The files of one session.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionFiles {
    /// `session-YYYYMMDD-HHMMSS` (UTC; `-2`, `-3`… when taken).
    pub id: String,
    /// The log file.
    pub log: PathBuf,
    /// The action journal.
    pub journal: PathBuf,
    /// The sidecar (`clean_exit`).
    pub sidecar: PathBuf,
    /// The session before this one: its id and whether it exited cleanly.
    pub previous: Option<(String, bool)>,
}

/// The running session (set once by [`start`]).
#[derive(Debug)]
pub struct Session {
    /// The app-data root (`None`: nothing is written).
    pub root: Option<PathBuf>,
    /// The session's files (`None` without a root or when they could not be
    /// created).
    pub files: Option<SessionFiles>,
}

static SESSION: OnceLock<Session> = OnceLock::new();

/// The running session, if [`start`] was called.
pub fn session() -> Option<&'static Session> {
    SESSION.get()
}

/// The running session's id (empty without one).
pub fn session_id() -> String {
    session()
        .and_then(|s| s.files.as_ref())
        .map(|f| f.id.clone())
        .unwrap_or_default()
}

/// Starts a session: creates its files under `root` (old sessions beyond
/// [`KEEP_SESSIONS`] are deleted), installs the logger and the panic hook,
/// opens the journal and logs the program version and OS. Safe to call once;
/// later calls return the first session.
pub fn start(root: Option<PathBuf>) -> &'static Session {
    SESSION.get_or_init(|| {
        let files =
            root.as_deref()
                .and_then(|r| match open_session(&logs_dir(r), SystemTime::now()) {
                    Ok(f) => Some(f),
                    Err(e) => {
                        eprintln!("3dcadcraft: cannot create the session log: {e}");
                        None
                    }
                });
        log::install(files.as_ref().map(|f| f.log.as_path()));
        crash::install_hook();
        if let Some(f) = &files {
            journal::open_global(&f.journal);
        }
        ::log::info!(
            "3DCadCraft {} on {} {} (session {})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            files.as_ref().map_or("-", |f| f.id.as_str())
        );
        if let Some(r) = &root {
            ::log::info!("app data: {}", r.display());
            ::log::info!("settings: {}", r.join("settings.json").display());
        }
        if let Some((id, clean)) = files.as_ref().and_then(|f| f.previous.as_ref()) {
            ::log::info!(
                "previous session {id}: {}",
                if *clean {
                    "exited cleanly"
                } else {
                    "did NOT exit cleanly"
                }
            );
        }
        journal::global_entry(
            "session",
            "",
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
            }),
        );
        Session { root, files }
    })
}

/// Marks the running session as exited cleanly (its sidecar and journal).
pub fn finish() {
    journal::global_entry("exit", "", serde_json::json!({}));
    ::log::info!("clean exit");
    if let Some(f) = session().and_then(|s| s.files.as_ref())
        && let Err(e) = mark_clean(&f.sidecar)
    {
        ::log::warn!("session sidecar: {e}");
    }
}

/// Creates the files of a new session in `logs` at time `now`, rotates old
/// sessions and reads whether the previous one exited cleanly.
pub fn open_session(logs: &Path, now: SystemTime) -> Result<SessionFiles, String> {
    std::fs::create_dir_all(logs).map_err(|e| format!("{}: {e}", logs.display()))?;
    let base = format!("session-{}", stamp(now));
    let mut id = base.clone();
    let mut n = 2;
    while logs.join(format!("{id}.json")).exists() || logs.join(format!("{id}.log")).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    let previous = session_ids(logs).into_iter().next_back().map(|prev| {
        let v = std::fs::read_to_string(logs.join(format!("{prev}.json")))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        let clean = v
            .as_ref()
            .and_then(|v| v.get("clean_exit")?.as_bool())
            .unwrap_or(false);
        // What it was doing last (a process killed by an abort, such as a
        // stack overflow, leaves no crash report: this is the only trace).
        if !clean && let Some(a) = v.as_ref().and_then(|v| v.get("activity")?.as_str()) {
            ::log::warn!("previous session {prev} ended while: {a}");
        }
        (prev, clean)
    });
    let files = SessionFiles {
        log: logs.join(format!("{id}.log")),
        journal: logs.join(format!("{id}-journal.jsonl")),
        sidecar: logs.join(format!("{id}.json")),
        id,
        previous,
    };
    let sidecar = serde_json::json!({
        "id": files.id,
        "started": iso(now),
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "pid": std::process::id(),
        "clean_exit": false,
    });
    write_atomic(&files.sidecar, sidecar.to_string().as_bytes())?;
    rotate(logs, KEEP_SESSIONS, KEEP_CRASHES);
    Ok(files)
}

/// Records what the session is doing now in its sidecar (`activity`, with
/// the time): the work that starts a kernel computation — a checkpoint, a
/// preview — so that a session killed without a crash report (an abort, a
/// stack overflow) still says what it was doing. Not for every frame.
pub fn activity(what: &str) {
    let Some(f) = session().and_then(|s| s.files.as_ref()) else {
        return;
    };
    let mut v = std::fs::read_to_string(&f.sidecar)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    v["activity"] = serde_json::Value::String(format!("{} {what}", iso(SystemTime::now())));
    let _ = write_atomic(&f.sidecar, v.to_string().as_bytes());
}

/// Sets `clean_exit` in a session sidecar.
pub fn mark_clean(sidecar: &Path) -> Result<(), String> {
    let mut v = std::fs::read_to_string(sidecar)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    v["clean_exit"] = serde_json::Value::Bool(true);
    v["ended"] = serde_json::Value::String(iso(SystemTime::now()));
    write_atomic(sidecar, v.to_string().as_bytes())
}

/// The session id a file in `logs/` belongs to.
fn session_of(name: &str) -> Option<&str> {
    if !name.starts_with("session-") {
        return None;
    }
    name.strip_suffix("-journal.jsonl")
        .or_else(|| name.strip_suffix(".log"))
        .or_else(|| name.strip_suffix(".json"))
}

/// The session ids found in `logs`, oldest first.
pub fn session_ids(logs: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(logs)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| session_of(&e.file_name().to_string_lossy()).map(str::to_owned))
        .collect();
    ids.sort_by(|a, b| crate::presets::natural_cmp(a, b));
    ids.dedup();
    ids
}

/// Deletes the files of all but the newest `sessions` sessions and all but
/// the newest `crashes` crash reports.
pub fn rotate(logs: &Path, sessions: usize, crashes: usize) {
    let ids = session_ids(logs);
    let old: Vec<&String> = ids.iter().rev().skip(sessions).collect();
    let mut crash_files: Vec<String> = Vec::new();
    for e in std::fs::read_dir(logs).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if session_of(&name).is_some_and(|id| old.iter().any(|o| o.as_str() == id)) {
            if let Err(err) = std::fs::remove_file(e.path()) {
                ::log::warn!("cannot delete {}: {err}", e.path().display());
            }
        } else if name.starts_with("crash-") && name.ends_with(".txt") {
            crash_files.push(name);
        }
    }
    crash_files.sort();
    let n = crash_files.len().saturating_sub(crashes);
    for name in &crash_files[..n] {
        let _ = std::fs::remove_file(logs.join(name));
    }
}

// --------------------------------------------------------------- files ---

/// Writes `bytes` to `path` atomically: a hidden temporary sibling, flushed
/// to disk, then renamed over `path`. On any failure the temporary file is
/// removed and `path` keeps its old content.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let name = path
        .file_name()
        .map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", uuid::Uuid::new_v4().simple()));
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    let result = written
        .map_err(|e| format!("{}: {e}", tmp.display()))
        .and_then(|()| std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display())));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// FNV-1a, 64 bit: a hash that stays the same across builds (file names).
pub fn stable_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

// ---------------------------------------------------------------- time ---

/// UTC calendar fields of `t`: (year, month, day, hour, minute, second, ms).
pub fn utc(t: SystemTime) -> (i64, u32, u32, u32, u32, u32, u32) {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days → civil date (H. Hinnant's algorithm, public domain).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        month,
        day,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
        d.subsec_millis(),
    )
}

/// `YYYYMMDD-HHMMSS` (UTC), for file names.
pub fn stamp(t: SystemTime) -> String {
    let (y, mo, d, h, mi, s, _) = utc(t);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` (UTC).
pub fn iso(t: SystemTime) -> String {
    let (y, mo, d, h, mi, s, ms) = utc(t);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}Z")
}

/// Milliseconds since the Unix epoch.
pub fn unix_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Locks `m`, recovering the data of a poisoned lock (a panic elsewhere
/// must not silence diagnostics).
pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn utc_calendar() {
        let t = UNIX_EPOCH + Duration::from_millis(1_792_400_000_123);
        // 2026-10-19T08:53:20.123Z
        assert_eq!(iso(t), "2026-10-19T08:53:20.123Z");
        assert_eq!(stamp(t), "20261019-085320");
        assert_eq!(iso(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        // Leap day.
        let t = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(stamp(t), "20000229-000000");
    }

    #[test]
    fn rotation_keeps_the_newest_twenty_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path();
        for i in 0..25 {
            let t = UNIX_EPOCH + Duration::from_secs(1_800_000_000 + i * 60);
            let f = open_session(logs, t).unwrap();
            std::fs::write(&f.log, "x").unwrap();
            std::fs::write(&f.journal, "{}\n").unwrap();
        }
        for i in 0..25 {
            std::fs::write(logs.join(format!("crash-2027{i:04}-000000.txt")), "c").unwrap();
        }
        // The next session rotates: 20 sessions remain, the newest ones.
        let t = UNIX_EPOCH + Duration::from_secs(1_800_000_000 + 30 * 60);
        let f = open_session(logs, t).unwrap();
        let ids = session_ids(logs);
        assert_eq!(ids.len(), KEEP_SESSIONS);
        assert_eq!(ids.last(), Some(&f.id));
        let oldest_kept = stamp(UNIX_EPOCH + Duration::from_secs(1_800_000_000 + 6 * 60));
        assert_eq!(ids[0], format!("session-{oldest_kept}"));
        let files = std::fs::read_dir(logs).unwrap().count();
        // 19 old sessions with 3 files, the new sidecar, 20 crash reports.
        assert_eq!(files, 19 * 3 + 1 + KEEP_CRASHES);
        assert!(!logs.join("crash-20270000-000000.txt").exists());
        assert!(logs.join("crash-20270024-000000.txt").exists());
    }

    #[test]
    fn previous_session_and_clean_exit() {
        let dir = tempfile::tempdir().unwrap();
        let t = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let a = open_session(dir.path(), t).unwrap();
        assert_eq!(a.previous, None);
        // Same second: a new id.
        let b = open_session(dir.path(), t).unwrap();
        assert_eq!(b.id, format!("{}-2", a.id));
        assert_eq!(b.previous, Some((a.id.clone(), false)));
        mark_clean(&b.sidecar).unwrap();
        let c = open_session(dir.path(), t + Duration::from_secs(5)).unwrap();
        assert_eq!(c.previous, Some((b.id.clone(), true)));
    }

    #[test]
    fn atomic_writes_never_leave_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub").join("a.json");
        write_atomic(&p, b"first").unwrap();
        write_atomic(&p, b"second").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"second");
        // The target is a folder: the rename fails, nothing is left behind.
        let d = dir.path().join("taken");
        std::fs::create_dir(&d).unwrap();
        std::fs::write(d.join("inside"), "keep").unwrap();
        assert!(write_atomic(&d, b"data").is_err());
        assert!(d.is_dir());
        assert_eq!(std::fs::read(d.join("inside")).unwrap(), b"keep");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .chain(std::fs::read_dir(dir.path().join("sub")).unwrap())
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn stable_hash_is_fnv1a() {
        assert_eq!(stable_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(stable_hash(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
