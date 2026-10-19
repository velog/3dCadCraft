//! Autosave and Restore.
//!
//! An autosave is a normal project file in `<root>/recovery/` named
//! `<project>-<hash>-<YYYYMMDD-HHMMSS>.3dcadcraft`, where `<hash>` is a
//! stable hash of the original path (or of the project id for a project
//! never saved), plus a sidecar `<file>.json` ([`Sidecar`]). The newest
//! [`KEEP`] per project are kept. The user's own file is never written.
//!
//! On start, [`offers`] decides what to offer: only when the previous
//! session did not exit cleanly, only its autosaves, and only when the
//! autosave is newer than the original file (or there is none).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::document::{FILE_EXTENSION, Project};

/// Autosaves kept per project.
pub const KEEP: usize = 3;
/// Autosave period.
pub const INTERVAL: Duration = Duration::from_secs(300);
/// After a feature commit, autosave when the last one is this old.
pub const AFTER_COMMIT: Duration = Duration::from_secs(30);

/// What an autosave belongs to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    /// The user's file (empty: never saved).
    pub original: String,
    /// When written (ms since the Unix epoch).
    pub time_ms: u64,
    /// The session that wrote it.
    pub session: String,
    /// The project's name.
    pub project: String,
    /// `autosave` or `emergency` (written by the crash hook).
    #[serde(default)]
    pub kind: String,
}

/// An autosave on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The project file.
    pub file: PathBuf,
    /// Its sidecar.
    pub meta: Sidecar,
}

impl Entry {
    /// `<project>-<hash>`: the autosaves of one project.
    pub fn stem(&self) -> String {
        stem_of_file(&self.file)
    }
}

/// `<file>.json`.
pub fn sidecar_path(file: &Path) -> PathBuf {
    let mut os = file.as_os_str().to_owned();
    os.push(".json");
    PathBuf::from(os)
}

/// `<project>-<hash>` for a project and its original path.
pub fn project_stem(project: &Project, original: &str) -> String {
    let name: String = Path::new(original.trim())
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&project.name)
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    let key = if original.trim().is_empty() {
        format!("{:?}", project.id)
    } else {
        original.trim().to_owned()
    };
    let name = if name.is_empty() {
        "project".into()
    } else {
        name
    };
    format!("{name}-{:016x}", super::stable_hash(key.as_bytes()))
}

/// The stem of an autosave file name (without `-<stamp>[-n].3dcadcraft`).
fn stem_of_file(file: &Path) -> String {
    let name = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    // <stem>-YYYYMMDD-HHMMSS[-n]: the hash is the last 16-hex-digit part.
    let parts: Vec<&str> = name.split('-').collect();
    match parts
        .iter()
        .rposition(|p| p.len() == 16 && p.chars().all(|c| c.is_ascii_hexdigit()))
    {
        Some(k) => parts[..=k].join("-"),
        None => name.to_owned(),
    }
}

/// Writes an autosave of `project` into `dir` and keeps the newest
/// [`KEEP`] of that project.
pub fn write(
    dir: &Path,
    project: &Project,
    original: &str,
    session: &str,
    kind: &str,
    now: SystemTime,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stem = project_stem(project, original);
    let base = format!("{stem}-{}", super::stamp(now));
    let mut file = dir.join(format!("{base}.{FILE_EXTENSION}"));
    let mut n = 2;
    while file.exists() {
        file = dir.join(format!("{base}-{n}.{FILE_EXTENSION}"));
        n += 1;
    }
    project.save(&file).map_err(|e| e.to_string())?;
    let meta = Sidecar {
        original: original.trim().to_owned(),
        time_ms: super::unix_ms(now),
        session: session.to_owned(),
        project: project.name.clone(),
        kind: kind.to_owned(),
    };
    let text = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    if let Err(e) = super::write_atomic(&sidecar_path(&file), text.as_bytes()) {
        let _ = std::fs::remove_file(&file);
        return Err(e);
    }
    prune(dir, &stem, KEEP);
    Ok(file)
}

/// The autosaves in `dir` (with a readable sidecar), oldest first.
pub fn list(dir: &Path) -> Vec<Entry> {
    let mut out: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|x| x.to_str()) == Some(FILE_EXTENSION)
                && !p
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
        .filter_map(|file| {
            let text = std::fs::read_to_string(sidecar_path(&file)).ok()?;
            let meta = serde_json::from_str(&text).ok()?;
            Some(Entry { file, meta })
        })
        .collect();
    out.sort_by(|a, b| {
        a.meta
            .time_ms
            .cmp(&b.meta.time_ms)
            .then_with(|| a.file.cmp(&b.file))
    });
    out
}

/// Deletes all but the newest `keep` autosaves of `stem`.
pub fn prune(dir: &Path, stem: &str, keep: usize) {
    let mine: Vec<Entry> = list(dir).into_iter().filter(|e| e.stem() == stem).collect();
    let n = mine.len().saturating_sub(keep);
    for e in &mine[..n] {
        remove(e);
    }
}

fn remove(e: &Entry) {
    for p in [e.file.clone(), sidecar_path(&e.file)] {
        if let Err(err) = std::fs::remove_file(&p)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("recovery: cannot delete {}: {err}", p.display());
        }
    }
}

/// An autosave to offer at start.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    /// The newest autosave of the project.
    pub entry: Entry,
    /// All its autosaves from that session (Discard deletes them).
    pub all: Vec<Entry>,
}

/// What to offer, given the previous session (`(id, exited cleanly)`) and
/// the modification time of a file (ms since the epoch; `None`: missing).
pub fn offers(
    entries: &[Entry],
    previous: Option<&(String, bool)>,
    mtime_ms: impl Fn(&Path) -> Option<u64>,
) -> Vec<Offer> {
    let Some((prev, false)) = previous else {
        return Vec::new();
    };
    let mut out: Vec<Offer> = Vec::new();
    for e in entries.iter().filter(|e| &e.meta.session == prev) {
        let stem = e.stem();
        match out.iter_mut().find(|o| o.entry.stem() == stem) {
            Some(o) => {
                o.all.push(e.clone());
                if e.meta.time_ms >= o.entry.meta.time_ms {
                    o.entry = e.clone();
                }
            }
            None => out.push(Offer {
                entry: e.clone(),
                all: vec![e.clone()],
            }),
        }
    }
    out.retain(|o| {
        let orig = o.entry.meta.original.as_str();
        orig.is_empty() || mtime_ms(Path::new(orig)).is_none_or(|t| o.entry.meta.time_ms > t)
    });
    out
}

/// The modification time of `path` in ms since the epoch.
pub fn file_mtime_ms(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(super::unix_ms)
}

/// Discard: deletes the offer's autosaves.
pub fn discard(offer: &Offer) {
    for e in &offer.all {
        remove(e);
    }
}

/// When to autosave: every [`INTERVAL`], and after a feature commit when
/// the last autosave is [`AFTER_COMMIT`] old; only when the project changed
/// since it was last saved and since the last autosave.
#[derive(Clone, Debug)]
pub struct Autosaver {
    /// The last autosave (or check); the session start at first.
    last: Instant,
    /// The project signature at the last autosave.
    last_sig: Option<u64>,
}

impl Autosaver {
    /// Starts the timer at `now`.
    pub fn new(now: Instant) -> Self {
        Self {
            last: now,
            last_sig: None,
        }
    }

    /// Whether to look at the project now (`committed`: a feature was just
    /// committed).
    pub fn due(&self, now: Instant, committed: bool) -> bool {
        let age = now.saturating_duration_since(self.last);
        age >= INTERVAL || (committed && age >= AFTER_COMMIT)
    }

    /// Whether a project with signature `sig` needs an autosave (`dirty`:
    /// changed since the user last saved).
    pub fn needs(&self, dirty: bool, sig: u64) -> bool {
        dirty && self.last_sig != Some(sig)
    }

    /// Records a check or a save at `now` (`sig`: of the saved project).
    pub fn done(&mut self, now: Instant, sig: Option<u64>) {
        self.last = now;
        if sig.is_some() {
            self.last_sig = sig;
        }
    }

    /// The project was saved by the user: autosaves start over.
    pub fn reset_sig(&mut self) {
        self.last_sig = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentData;
    use crate::part::PartDocument;

    fn project() -> Project {
        let mut p = Project::new("Bracket");
        let part = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap_or_default();
        p.add_document("Part1", DocumentData::Part(part));
        p
    }

    fn at(s: u64) -> SystemTime {
        std::time::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + s)
    }

    #[test]
    fn autosave_only_when_dirty_and_keeps_three() {
        let dir = tempfile::tempdir().unwrap();
        let p = project();
        let t0 = Instant::now();
        let mut a = Autosaver::new(t0);
        // Not due yet; due after a commit 30 s on, or after 5 min.
        assert!(!a.due(t0 + Duration::from_secs(10), true));
        assert!(a.due(t0 + Duration::from_secs(31), true));
        assert!(!a.due(t0 + Duration::from_secs(31), false));
        assert!(a.due(t0 + INTERVAL, false));
        // Clean project: nothing to write.
        assert!(!a.needs(false, 1));
        let mut written = Vec::new();
        for (k, sig) in [1u64, 1, 2, 3, 4, 5].into_iter().enumerate() {
            let now = t0 + INTERVAL * (k as u32 + 1);
            assert!(a.due(now, false));
            if a.needs(true, sig) {
                let f = write(
                    dir.path(),
                    &p,
                    "C:/w/bracket.3dcadcraft",
                    "s1",
                    "autosave",
                    at(k as u64),
                )
                .unwrap();
                written.push(f);
                a.done(now, Some(sig));
            } else {
                a.done(now, None);
            }
        }
        // The same signature twice: written once.
        assert_eq!(written.len(), 5);
        let left = list(dir.path());
        assert_eq!(left.len(), KEEP);
        assert_eq!(
            left.iter().map(|e| e.file.clone()).collect::<Vec<_>>(),
            written[2..].to_vec()
        );
        assert!(
            left.iter()
                .all(|e| e.meta.original == "C:/w/bracket.3dcadcraft")
        );
        assert!(left[0].stem().starts_with("bracket-"));
        // Another project's autosaves are separate.
        write(dir.path(), &p, "", "s1", "autosave", at(99)).unwrap();
        assert_eq!(list(dir.path()).len(), KEEP + 1);
        // An autosave is a project that opens.
        let back = Project::load(&left[2].file).unwrap();
        assert_eq!(back.name, "Bracket");
    }

    #[test]
    fn restore_is_offered_only_after_an_unclean_exit_for_newer_autosaves() {
        let entry = |orig: &str, session: &str, t: u64| Entry {
            file: PathBuf::from(format!("r/{}-00000000000000aa-x.3dcadcraft", orig.len())),
            meta: Sidecar {
                original: orig.into(),
                time_ms: t,
                session: session.into(),
                project: "P".into(),
                kind: "autosave".into(),
            },
        };
        let entries = vec![entry("C:/a.3dcadcraft", "s1", 2000)];
        let file_at = |t: u64| move |_: &Path| Some(t);
        // Unclean exit + autosave newer than the file: offer.
        let prev = ("s1".to_owned(), false);
        assert_eq!(offers(&entries, Some(&prev), file_at(1000)).len(), 1);
        // Clean exit: none.
        let clean = ("s1".to_owned(), true);
        assert!(offers(&entries, Some(&clean), file_at(1000)).is_empty());
        // No previous session: none.
        assert!(offers(&entries, None, file_at(1000)).is_empty());
        // The file was saved after the autosave: none.
        assert!(offers(&entries, Some(&prev), file_at(3000)).is_empty());
        // The original is gone: offer.
        assert_eq!(offers(&entries, Some(&prev), |_: &Path| None).len(), 1);
        // Never saved: offer.
        let unsaved = vec![entry("", "s1", 10)];
        assert_eq!(offers(&unsaved, Some(&prev), file_at(1000)).len(), 1);
        // Autosaves of an older session are not offered.
        let older = vec![entry("C:/a.3dcadcraft", "s0", 2000)];
        assert!(offers(&older, Some(&prev), file_at(1000)).is_empty());
        // Several autosaves of one project: the newest, all to discard.
        let two = vec![
            entry("C:/a.3dcadcraft", "s1", 2000),
            entry("C:/a.3dcadcraft", "s1", 2500),
        ];
        let o = offers(&two, Some(&prev), file_at(1000));
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].entry.meta.time_ms, 2500);
        assert_eq!(o[0].all.len(), 2);
    }

    #[test]
    fn discard_deletes_the_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = project();
        let f = write(dir.path(), &p, "", "s1", "autosave", at(0)).unwrap();
        let o = offers(
            &list(dir.path()),
            Some(&("s1".into(), false)),
            |_: &Path| None,
        );
        assert_eq!(o.len(), 1);
        discard(&o[0]);
        assert!(!f.exists() && !sidecar_path(&f).exists());
    }
}
