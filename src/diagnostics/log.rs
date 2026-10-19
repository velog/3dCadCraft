//! The session logger: every `log::…!` record goes to the session file, to
//! stderr and into a ring buffer of the last [`TAIL`] lines (for crash
//! reports). A tiny own implementation: no extra dependency.
//!
//! Levels: our crate and the geometry kernel (`solidcraft*`, `truck*`) log
//! from `info`, other crates (wgpu, winit…) from `warn`. `CADCRAFT3D_LOG`
//! (`trace|debug|info|warn|error|off`) sets our level; `RUST_LOG` raises it
//! to the highest level it names, and a bare level there (`RUST_LOG=debug`)
//! also applies to the other crates.

use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use log::{LevelFilter, Log, Metadata, Record};

use super::lock;

/// Lines kept for crash reports.
pub const TAIL: usize = 200;

struct Logger {
    ours: LevelFilter,
    others: LevelFilter,
    file: Mutex<Option<std::fs::File>>,
    ring: Mutex<VecDeque<String>>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Whether `target` is our code (or the kernel's).
fn is_ours(target: &str) -> bool {
    ["cadcraft3d", "3dcadcraft", "solidcraft", "truck"]
        .iter()
        .any(|p| target.starts_with(p))
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        let max = if is_ours(m.target()) {
            self.ours
        } else {
            self.others
        };
        m.level() <= max
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_line(
            SystemTime::now(),
            std::thread::current().name().unwrap_or("?"),
            record.level(),
            record.target(),
            &record.args().to_string(),
        );
        eprintln!("{line}");
        // `try_lock`: a panic while logging must not deadlock the hook.
        if let Ok(mut f) = self.file.try_lock()
            && let Some(f) = f.as_mut()
        {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
        let mut ring = lock(&self.ring);
        if ring.len() == TAIL {
            ring.pop_front();
        }
        ring.push_back(line);
    }

    fn flush(&self) {
        if let Some(f) = lock(&self.file).as_mut() {
            let _ = f.flush();
        }
    }
}

/// One log line: `2026-10-19T08:53:20.123Z INFO  [main] target: message`.
pub fn format_line(
    t: SystemTime,
    thread: &str,
    level: log::Level,
    target: &str,
    msg: &str,
) -> String {
    format!("{} {level:<5} [{thread}] {target}: {msg}", super::iso(t))
}

/// Our level and the other crates' level from `CADCRAFT3D_LOG` and
/// `RUST_LOG` (see the module docs).
pub fn levels(cadcraft: Option<&str>, rust_log: Option<&str>) -> (LevelFilter, LevelFilter) {
    let mut ours = LevelFilter::Info;
    let mut others = LevelFilter::Warn;
    if let Some(r) = rust_log {
        for part in r.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part.split_once('=') {
                Some((_, lv)) => {
                    if let Ok(l) = lv.trim().parse::<LevelFilter>() {
                        ours = ours.max(l);
                    }
                }
                None => {
                    if let Ok(l) = part.parse::<LevelFilter>() {
                        ours = ours.max(l);
                        others = l;
                    }
                }
            }
        }
    }
    if let Some(l) = cadcraft.and_then(|c| c.trim().parse::<LevelFilter>().ok()) {
        ours = l;
    }
    (ours, others)
}

/// Installs the logger, writing to `file` too (appended). Does nothing when
/// a logger is already installed.
pub fn install(file: Option<&Path>) {
    let (ours, others) = levels(
        std::env::var("CADCRAFT3D_LOG").ok().as_deref(),
        std::env::var("RUST_LOG").ok().as_deref(),
    );
    let file = file.and_then(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .map_err(|e| eprintln!("3dcadcraft: cannot open {}: {e}", p.display()))
            .ok()
    });
    let logger = LOGGER.get_or_init(|| Logger {
        ours,
        others,
        file: Mutex::new(file),
        ring: Mutex::new(VecDeque::with_capacity(TAIL)),
    });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(ours.max(others));
    }
}

/// The last log lines (oldest first); empty when the logger is busy or not
/// installed.
pub fn tail() -> Vec<String> {
    LOGGER
        .get()
        .and_then(|l| l.ring.try_lock().ok().map(|r| r.iter().cloned().collect()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_format() {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_millis(1_500);
        assert_eq!(
            format_line(t, "main", log::Level::Warn, "cadcraft3d::part", "x"),
            "1970-01-01T00:00:01.500Z WARN  [main] cadcraft3d::part: x"
        );
    }

    #[test]
    fn level_overrides() {
        use LevelFilter::*;
        assert_eq!(levels(None, None), (Info, Warn));
        assert_eq!(levels(None, Some("debug")), (Debug, Debug));
        assert_eq!(
            levels(None, Some("wgpu=error,cadcraft3d=trace")),
            (Trace, Warn)
        );
        // Never lowered by RUST_LOG.
        assert_eq!(levels(None, Some("error")), (Info, Error));
        assert_eq!(levels(Some("debug"), None), (Debug, Warn));
        assert_eq!(levels(Some("bogus"), None), (Info, Warn));
        assert!(is_ours("cadcraft3d::kernel") && !is_ours("wgpu_core"));
    }
}
