//! Design projects (`.ccproj`, `docs/spec/0.6a-file-types-projects.md` §3.2):
//! Inventor's project file in our own form — a small JSON file naming where a
//! design's files live (workspace, libraries, shortcuts, template / style /
//! Content Center folders) and how saving treats old versions.
//!
//! A project is user setup, not model data: switching projects never changes
//! a document. The built-in *Default* project has no file. Which project is
//! active and which ones are known are user settings (`settings.json`
//! `projects`, owned by the GUI).

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::filetype::FileType;
use super::{ProjectError, ProjectResult};

/// The `format` value of a `.ccproj` file.
const PROJECT_MAGIC: &str = "3dcadcraft-project";

/// Newest `.ccproj` version this build reads.
pub const PROJECT_VERSION: u32 = 1;

/// A named folder (library or frequently used subfolder).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedPath {
    /// Shown name.
    pub name: String,
    /// Folder, relative to the project file's folder when inside it.
    pub path: PathBuf,
}

/// A design project. Paths are stored as written (relative ones are relative
/// to the project file's folder); [`DesignProject::resolve`] makes them
/// absolute.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DesignProject {
    /// Always `"3dcadcraft-project"`.
    pub format: String,
    /// `.ccproj` version.
    pub version: u32,
    /// Display name (the file's stem when empty).
    #[serde(default)]
    pub name: String,
    /// Where the user edits and saves files.
    #[serde(default = "dot")]
    pub workspace: PathBuf,
    /// Read-only folders of standard parts, searched after the workspace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<NamedPath>,
    /// Frequently used subfolders (shortcuts in the file dialogs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shortcuts: Vec<NamedPath>,
    /// Templates folder (`None` = Application Options default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub templates: Option<PathBuf>,
    /// Styles (design data) folder (`None` = default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub styles: Option<PathBuf>,
    /// Content Center files folder (`None` = default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_center: Option<PathBuf>,
    /// Old versions kept on save (Inventor's default 1; −1 = all).
    #[serde(default = "one")]
    pub old_versions: i32,
    /// Look a missing reference up by its name in the whole project.
    #[serde(default)]
    pub unique_names: bool,
    /// Free text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
}

fn dot() -> PathBuf {
    PathBuf::from(".")
}

fn one() -> i32 {
    1
}

impl DesignProject {
    /// A new project named `name` whose workspace is its own folder.
    pub fn new(name: &str) -> Self {
        Self {
            format: PROJECT_MAGIC.into(),
            version: PROJECT_VERSION,
            name: name.into(),
            workspace: dot(),
            libraries: Vec::new(),
            shortcuts: Vec::new(),
            templates: None,
            styles: None,
            content_center: None,
            old_versions: 1,
            unique_names: false,
            owner: String::new(),
        }
    }

    /// The built-in Default project: workspace `<documents>/3DCadCraft`.
    pub fn default_project(documents: &Path) -> Self {
        Self {
            workspace: documents.join("3DCadCraft"),
            ..Self::new("Default")
        }
    }

    /// Reads a `.ccproj` file.
    pub fn load(path: &Path) -> ProjectResult<Self> {
        let text = fs::read_to_string(path).map_err(|source| ProjectError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let corrupt = |reason: String| ProjectError::Corrupt {
            path: path.to_path_buf(),
            reason,
        };
        let mut p: DesignProject =
            serde_json::from_str(&text).map_err(|e| corrupt(format!("not a project ({e})")))?;
        if p.format != PROJECT_MAGIC {
            return Err(corrupt(format!("format \"{}\"", p.format)));
        }
        if p.version > PROJECT_VERSION {
            return Err(ProjectError::UnsupportedVersion {
                path: path.to_path_buf(),
                found: p.version,
                supported: PROJECT_VERSION,
            });
        }
        if p.name.trim().is_empty() {
            p.name = stem(path);
        }
        Ok(p)
    }

    /// Writes the project to `path` (atomic: a temporary file, then rename).
    pub fn save(&self, path: &Path) -> ProjectResult<()> {
        let io = |source| ProjectError::Io {
            path: path.to_path_buf(),
            source,
        };
        let mut text = serde_json::to_string_pretty(self).map_err(|e| ProjectError::Corrupt {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        text.push('\n');
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        fs::write(&tmp, text).map_err(io)?;
        fs::rename(&tmp, path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            io(e)
        })
    }

    /// `p` made absolute against the project file's folder `base`.
    pub fn resolve(base: &Path, p: &Path) -> PathBuf {
        if p.is_absolute() {
            return p.to_path_buf();
        }
        let mut out = base.to_path_buf();
        for c in p.components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other),
            }
        }
        out
    }

    /// The workspace folder (absolute) of a project stored in `base`.
    pub fn workspace_dir(&self, base: &Path) -> PathBuf {
        Self::resolve(base, &self.workspace)
    }

    /// The folders searched for a file, in order: workspace, then libraries.
    pub fn search_dirs(&self, base: &Path) -> Vec<PathBuf> {
        std::iter::once(self.workspace_dir(base))
            .chain(self.libraries.iter().map(|l| Self::resolve(base, &l.path)))
            .collect()
    }

    /// Finds `name` (a file name or a relative path) in the search folders.
    pub fn search(&self, base: &Path, name: &Path) -> Option<PathBuf> {
        self.search_dirs(base)
            .into_iter()
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    }

    /// Is `file` inside the workspace or a library of this project?
    pub fn contains(&self, base: &Path, file: &Path) -> bool {
        self.search_dirs(base).iter().any(|d| file.starts_with(d))
    }

    /// Is `file` inside one of the libraries (opened read-only)?
    pub fn in_library(&self, base: &Path, file: &Path) -> bool {
        self.libraries
            .iter()
            .any(|l| file.starts_with(Self::resolve(base, &l.path)))
    }
}

/// `path`'s stem as a name.
fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `<folder>/<name>.ccproj`.
pub fn project_file(folder: &Path, name: &str) -> PathBuf {
    folder.join(format!("{name}.{}", FileType::Project.ext()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadcraft-proj-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_load_round_trip_and_defaults() {
        let d = tmp("rt");
        let mut p = DesignProject::new("Pump");
        p.libraries.push(NamedPath {
            name: "Fasteners".into(),
            path: "../lib".into(),
        });
        p.old_versions = -1;
        let f = project_file(&d, "Pump");
        assert_eq!(f.extension().unwrap(), "ccproj");
        p.save(&f).unwrap();
        assert_eq!(DesignProject::load(&f).unwrap(), p);
        // A minimal file gets Inventor's defaults and the file's stem.
        fs::write(&f, r#"{"format":"3dcadcraft-project","version":1}"#).unwrap();
        let q = DesignProject::load(&f).unwrap();
        assert_eq!(q.name, "Pump");
        assert_eq!(q.workspace, PathBuf::from("."));
        assert_eq!(q.old_versions, 1);
        assert!(!q.unique_names);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn refuses_other_files_and_newer_versions() {
        let d = tmp("bad");
        let f = d.join("x.ccproj");
        fs::write(&f, r#"{"format":"other","version":1}"#).unwrap();
        assert!(matches!(
            DesignProject::load(&f),
            Err(ProjectError::Corrupt { .. })
        ));
        fs::write(&f, r#"{"format":"3dcadcraft-project","version":99}"#).unwrap();
        assert!(matches!(
            DesignProject::load(&f),
            Err(ProjectError::UnsupportedVersion { found: 99, .. })
        ));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn search_order_is_workspace_then_libraries() {
        let d = tmp("search");
        let lib = d.join("lib");
        fs::create_dir_all(d.join("ws")).unwrap();
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("bolt.ccpart"), b"x").unwrap();
        let mut p = DesignProject::new("P");
        p.workspace = "ws".into();
        p.libraries.push(NamedPath {
            name: "Lib".into(),
            path: "lib".into(),
        });
        let name = Path::new("bolt.ccpart");
        assert_eq!(p.search(&d, name), Some(lib.join("bolt.ccpart")));
        fs::write(d.join("ws").join("bolt.ccpart"), b"y").unwrap();
        assert_eq!(p.search(&d, name), Some(d.join("ws").join("bolt.ccpart")));
        assert!(p.in_library(&d, &lib.join("bolt.ccpart")));
        assert!(!p.in_library(&d, &d.join("ws").join("bolt.ccpart")));
        assert!(p.contains(&d, &d.join("ws").join("a.ccasm")));
        assert!(!p.contains(&d, &d.join("other").join("a.ccasm")));
        assert_eq!(
            DesignProject::resolve(&d, Path::new("./a/../b")),
            d.join("b")
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn default_project_lives_in_documents() {
        let p = DesignProject::default_project(Path::new("/home/u/Documents"));
        assert_eq!(p.name, "Default");
        assert_eq!(p.workspace, PathBuf::from("/home/u/Documents/3DCadCraft"));
    }
}
