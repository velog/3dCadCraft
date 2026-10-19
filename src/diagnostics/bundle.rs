//! Help → Save Diagnostics: one zip with what is needed to find out what
//! happened — the current and the previous session's log, journal and
//! sidecar, the crash reports, the open project and `settings.json`.

use std::io::Write;
use std::path::{Path, PathBuf};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::document::{FILE_EXTENSION, Project};

/// What goes into the zip.
#[derive(Debug, Default)]
pub struct Inputs<'a> {
    /// `<root>/logs`.
    pub logs: Option<PathBuf>,
    /// Sessions whose files to include (current first).
    pub sessions: Vec<String>,
    /// The open project (saved to a temporary copy first).
    pub project: Option<&'a Project>,
    /// `settings.json`.
    pub settings: Option<PathBuf>,
    /// Crash reports included (newest).
    pub crashes: usize,
}

impl Inputs<'_> {
    /// The running session and the one before it, from `<root>/logs`.
    pub fn for_session(root: Option<&Path>) -> Self {
        let logs = root.map(super::logs_dir);
        let current = super::session_id();
        let mut sessions = Vec::new();
        if !current.is_empty() {
            sessions.push(current.clone());
        }
        if let Some(prev) = logs.as_deref().and_then(|l| {
            super::session_ids(l)
                .into_iter()
                .rev()
                .find(|id| *id != current)
        }) {
            sessions.push(prev);
        }
        Self {
            logs,
            sessions,
            project: None,
            settings: root.map(|r| r.join("settings.json")),
            crashes: 5,
        }
    }
}

/// Writes the zip to `target` (atomically) and returns its entry names.
pub fn write(target: &Path, inputs: &Inputs) -> Result<Vec<String>, String> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    if let Some(logs) = &inputs.logs {
        for id in &inputs.sessions {
            for suffix in [".log", "-journal.jsonl", ".json"] {
                let p = logs.join(format!("{id}{suffix}"));
                match std::fs::read(&p) {
                    Ok(b) => files.push((format!("logs/{id}{suffix}"), b)),
                    Err(e) => notes.push(format!("{}: {e}", p.display())),
                }
            }
        }
        let mut crashes: Vec<String> = std::fs::read_dir(logs)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("crash-") && n.ends_with(".txt"))
            .collect();
        crashes.sort();
        for name in crashes.iter().rev().take(inputs.crashes) {
            if let Ok(b) = std::fs::read(logs.join(name)) {
                files.push((format!("crash/{name}"), b));
            }
        }
    }
    if let Some(project) = inputs.project {
        let tmp = std::env::temp_dir().join(format!(
            "3dcadcraft-diagnostics-{}.{FILE_EXTENSION}",
            uuid::Uuid::new_v4().simple()
        ));
        let saved = project
            .save(&tmp)
            .map_err(|e| e.to_string())
            .and_then(|()| std::fs::read(&tmp).map_err(|e| e.to_string()));
        let _ = std::fs::remove_file(&tmp);
        match saved {
            Ok(b) => {
                let name: String = project
                    .name
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                    .collect();
                files.push((format!("project/{name}.{FILE_EXTENSION}"), b));
            }
            Err(e) => notes.push(format!("project: {e}")),
        }
    }
    if let Some(s) = &inputs.settings {
        match std::fs::read(s) {
            Ok(b) => files.push(("settings.json".into(), b)),
            Err(e) => notes.push(format!("{}: {e}", s.display())),
        }
    }
    let info = format!(
        "3DCadCraft {}\nos: {} {}\nwritten: {}\nsessions: {}\n{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        super::iso(std::time::SystemTime::now()),
        inputs.sessions.join(", "),
        notes
            .iter()
            .map(|n| format!("missing: {n}\n"))
            .collect::<String>()
    );
    files.insert(0, ("info.txt".into(), info.into_bytes()));

    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buf);
        let opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in &files {
            zip.start_file(name.as_str(), opts)
                .map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    super::write_atomic(target, buf.get_ref())?;
    Ok(files.into_iter().map(|(n, _)| n).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentData;
    use crate::part::PartDocument;

    #[test]
    fn diagnostics_zip_has_the_expected_entries() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        for id in ["session-a", "session-b"] {
            std::fs::write(logs.join(format!("{id}.log")), "log").unwrap();
            std::fs::write(logs.join(format!("{id}-journal.jsonl")), "{}\n").unwrap();
            std::fs::write(logs.join(format!("{id}.json")), "{}").unwrap();
        }
        std::fs::write(logs.join("crash-20261019-000000.txt"), "boom").unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, r#"{"recent_files":[]}"#).unwrap();
        let mut project = Project::new("My Part");
        project.add_document("Part1", DocumentData::Part(PartDocument::default()));
        let inputs = Inputs {
            logs: Some(logs),
            sessions: vec!["session-b".into(), "session-a".into()],
            project: Some(&project),
            settings: Some(settings),
            crashes: 5,
        };
        let target = dir.path().join("out").join("diag.zip");
        let names = write(&target, &inputs).unwrap();
        let file = std::fs::File::open(&target).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let mut inside: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned())
            .collect();
        inside.sort();
        let mut expected = vec![
            "info.txt",
            "logs/session-a.log",
            "logs/session-a-journal.jsonl",
            "logs/session-a.json",
            "logs/session-b.log",
            "logs/session-b-journal.jsonl",
            "logs/session-b.json",
            "crash/crash-20261019-000000.txt",
            "project/My_Part.3dcadcraft",
            "settings.json",
        ];
        expected.sort();
        assert_eq!(inside, expected);
        assert_eq!(names.len(), expected.len());
        // The project inside opens.
        let mut e = zip.by_name("project/My_Part.3dcadcraft").unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut e, &mut bytes).unwrap();
        let p = dir.path().join("back.3dcadcraft");
        std::fs::write(&p, bytes).unwrap();
        assert_eq!(Project::load(&p).unwrap().name, "My Part");
    }
}
