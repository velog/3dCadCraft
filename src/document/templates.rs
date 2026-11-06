//! Templates (`docs/spec/0.6c-templates.md`): every new document starts as a
//! copy of a template — an ordinary `.ccpart`, `.ccasm` or `.ccdrw` file of one
//! document in a templates folder (Inventor's model). `Standard.<ext>` in the
//! folder's root is the default of its kind; without it the program's own
//! empty document is used, so New never fails for want of a template.
//!
//! GUI-free: the folder tree, the built-in templates, a new document from a
//! template and Save Copy As Template.

use std::fs;
use std::path::{Path, PathBuf};

use super::filetype::FileType;
use super::iprops::IProperties;
use super::{
    AssemblyDocument, DesignProject, DocumentData, DocumentId, DocumentKind, PartDocument, Project,
    ProjectError, ProjectResult, SaveOptions,
};
use crate::drawing::{DrawingDocument, DrawingStandard, StandardKind};

/// Overrides the default templates folder (scripted runs, tests).
pub const DIR_ENV: &str = "CADCRAFT3D_TEMPLATES_DIR";

/// The file stem of the standard template of each kind.
pub const STANDARD: &str = "Standard";

/// The default templates folder: [`DIR_ENV`], else `<settings folder>/templates`.
pub fn default_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os(DIR_ENV).filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(d));
    }
    Some(crate::presets::config_dir()?.join("templates"))
}

/// The templates folder in force: the project's own (relative to the folder of
/// its `.ccproj`, `base`), else `app_default` (Application Options), else
/// [`default_dir`].
pub fn dir_for(
    project: &DesignProject,
    base: Option<&Path>,
    app_default: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(t) = project
        .templates
        .as_ref()
        .filter(|t| !t.as_os_str().is_empty())
    {
        return Some(match base {
            Some(b) => DesignProject::resolve(b, t),
            None => t.clone(),
        });
    }
    app_default.map(Path::to_path_buf).or_else(default_dir)
}

/// The program's own empty document of a kind (drawings: ISO).
pub fn builtin(kind: DocumentKind) -> DocumentData {
    match kind {
        DocumentKind::Part => DocumentData::Part(PartDocument::default()),
        DocumentKind::Assembly => DocumentData::Assembly(AssemblyDocument::default()),
        DocumentKind::Drawing => DocumentData::Drawing(DrawingDocument::default()),
    }
}

/// The templates written into an empty default folder: (path in the folder,
/// document).
pub fn builtin_files() -> Vec<(PathBuf, DocumentData)> {
    let drawing = |k| DocumentData::Drawing(DrawingDocument::new(DrawingStandard::of(k)));
    vec![
        (
            PathBuf::from("Standard.ccpart"),
            builtin(DocumentKind::Part),
        ),
        (
            PathBuf::from("Standard.ccasm"),
            builtin(DocumentKind::Assembly),
        ),
        (PathBuf::from("Standard.ccdrw"), drawing(StandardKind::Iso)),
        (
            Path::new("Metric").join("ISO.ccdrw"),
            drawing(StandardKind::Iso),
        ),
        (
            Path::new("Metric").join("DIN.ccdrw"),
            drawing(StandardKind::Din),
        ),
        (
            Path::new("English").join("ANSI.ccdrw"),
            drawing(StandardKind::Ansi),
        ),
    ]
}

/// The name a new document of `kind` takes in the file.
pub fn document_base_name(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Part => "Part",
        DocumentKind::Assembly => "Assembly",
        DocumentKind::Drawing => "Drawing",
    }
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> ProjectError + '_ {
    move |source| ProjectError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Writes one document as a single-document file (no `.bak`).
fn write_single(
    path: &Path,
    name: &str,
    data: DocumentData,
    props: IProperties,
) -> ProjectResult<()> {
    let mut p = Project::new(name);
    let id = p.add_document(name, data);
    if let Some(d) = p.document_mut(id) {
        d.properties = props;
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io(dir))?;
    }
    p.save_with(path, &SaveOptions { backup: false })
}

/// Fills `dir` with the built-in templates when it holds no template yet (a
/// missing folder is created). A folder the user filled is never touched.
/// Returns whether anything was written.
pub fn ensure_builtin(dir: &Path) -> ProjectResult<bool> {
    if dir.is_dir() && has_template_files(dir) {
        return Ok(false);
    }
    for (rel, data) in builtin_files() {
        let path = dir.join(&rel);
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        write_single(&path, &name, data, IProperties::default())?;
    }
    Ok(true)
}

/// Is `t` a type a template can have?
fn template_type(t: Option<FileType>) -> bool {
    matches!(
        t,
        Some(FileType::Part | FileType::Assembly | FileType::Drawing)
    )
}

fn has_template_files(dir: &Path) -> bool {
    let Ok(rd) = fs::read_dir(dir) else {
        return false;
    };
    rd.flatten().any(|e| {
        let p = e.path();
        if p.is_dir() {
            has_template_files(&p)
        } else {
            template_type(FileType::from_path(&p))
        }
    })
}

/// One template file.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    /// The file.
    pub path: PathBuf,
    /// Its stem (what Create New File shows).
    pub name: String,
    /// The kind of its document (read from the file, never the extension).
    pub kind: DocumentKind,
}

/// A folder of the templates tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Folder {
    /// The folder.
    pub path: PathBuf,
    /// Its name.
    pub name: String,
    /// Its templates, by name.
    pub templates: Vec<Template>,
    /// Files of a template type that cannot be used: (file, reason).
    pub unreadable: Vec<(PathBuf, String)>,
    /// Its subfolders, by name.
    pub folders: Vec<Folder>,
}

impl Folder {
    /// The folder at `path` in this tree.
    pub fn find(&self, path: &Path) -> Option<&Folder> {
        if self.path == path {
            return Some(self);
        }
        self.folders.iter().find_map(|f| f.find(path))
    }

    /// Every template of the tree.
    pub fn all(&self) -> Vec<&Template> {
        let mut v: Vec<&Template> = self.templates.iter().collect();
        for f in &self.folders {
            v.extend(f.all());
        }
        v
    }
}

/// The kind of a single-document file, or why it is no template.
pub fn template_kind(path: &Path) -> Result<DocumentKind, String> {
    let p = super::format::read_preview(path).map_err(|e| e.to_string())?;
    match p.documents.as_slice() {
        [(_, k)] => Ok(*k),
        [] => Err("the file holds no document".into()),
        _ => Err(format!(
            "the file holds {} documents; a template holds one",
            p.documents.len()
        )),
    }
}

/// The tree of templates under `dir` (any depth). A missing folder is an
/// error; unreadable files are listed in [`Folder::unreadable`].
pub fn list(dir: &Path) -> ProjectResult<Folder> {
    let rd = fs::read_dir(dir).map_err(io(dir))?;
    let mut f = Folder {
        path: dir.to_path_buf(),
        name: dir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.display().to_string()),
        ..Folder::default()
    };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if let Ok(sub) = list(&p) {
                f.folders.push(sub);
            }
        } else if template_type(FileType::from_path(&p)) {
            match template_kind(&p) {
                Ok(kind) => f.templates.push(Template {
                    name: p
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    path: p,
                    kind,
                }),
                Err(reason) => f.unreadable.push((p, reason)),
            }
        }
    }
    f.templates
        .sort_by_key(|t| (kind_order(t.kind), t.name.to_lowercase()));
    Ok(f)
}

fn kind_order(k: DocumentKind) -> u8 {
    match k {
        DocumentKind::Part => 0,
        DocumentKind::Assembly => 1,
        DocumentKind::Drawing => 2,
    }
}

/// `dir/Standard.<ext of kind>` when it is there.
pub fn standard_path(dir: &Path, kind: DocumentKind) -> Option<PathBuf> {
    let p = dir.join(format!("{STANDARD}.{}", FileType::for_kind(kind).ext()));
    p.is_file().then_some(p)
}

/// What a template gives a new document.
#[derive(Clone, Debug)]
pub struct NewDocument {
    /// The document's content.
    pub data: DocumentData,
    /// Its Properties: the template's, then the login name and today where
    /// the template left them empty.
    pub properties: IProperties,
}

/// The template's Properties completed for a new document: Creation Date
/// is today, Author / Designer the login name where the template has none.
fn new_properties(mut p: IProperties) -> IProperties {
    let fresh = IProperties::new_document();
    for (k, v) in &fresh.standard {
        if k == "creation_date" || p.get(k).is_empty() {
            p.set(k, v);
        }
    }
    p
}

/// A new document from the template file `path`.
pub fn instantiate(path: &Path) -> ProjectResult<NewDocument> {
    let p = Project::load(path)?;
    let corrupt = |reason: String| ProjectError::Corrupt {
        path: path.to_path_buf(),
        reason,
    };
    let d = match p.documents() {
        [d] => d,
        [] => return Err(corrupt("the template holds no document".into())),
        v => {
            return Err(corrupt(format!(
                "the template holds {} documents; a template holds one",
                v.len()
            )));
        }
    };
    if !d.data.references().is_empty() {
        return Err(corrupt(
            "the template places other documents; a template holds one document without references"
                .into(),
        ));
    }
    Ok(NewDocument {
        data: d.data.clone(),
        properties: new_properties(d.properties.clone()),
    })
}

/// A new document of `kind` from the standard template in `dir` (or the
/// built-in document when there is none).
pub fn standard_document(dir: Option<&Path>, kind: DocumentKind) -> ProjectResult<NewDocument> {
    match dir.and_then(|d| standard_path(d, kind)) {
        Some(p) => instantiate(&p),
        None => Ok(NewDocument {
            data: builtin(kind),
            properties: IProperties::new_document(),
        }),
    }
}

/// Save Copy As Template: document `id` of `project` written to `path` as a
/// single-document file. Refused for a document that places others (its
/// references would not resolve from the templates folder).
pub fn save_as_template(project: &Project, id: DocumentId, path: &Path) -> ProjectResult<()> {
    let d = project
        .document(id)
        .ok_or(ProjectError::UnknownDocument(id))?;
    if !d.data.references().is_empty() {
        return Err(ProjectError::Corrupt {
            path: path.to_path_buf(),
            reason: "this document places other documents; save them to separate files and \
                     use a template without components"
                .into(),
        });
    }
    let want = FileType::for_kind(d.data.kind());
    if FileType::from_path(path) != Some(want) {
        return Err(ProjectError::Corrupt {
            path: path.to_path_buf(),
            reason: format!("a {} template is a {} file", want.name(), want.filter()),
        });
    }
    write_single(path, &d.name, d.data.clone(), d.properties.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadcraft-tpl-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn builtin_templates_fill_an_empty_folder_once() {
        let d = tmp("builtin");
        assert!(ensure_builtin(&d).unwrap());
        let tree = list(&d).unwrap();
        let names: Vec<_> = tree
            .templates
            .iter()
            .map(|t| (t.name.as_str(), t.kind))
            .collect();
        assert_eq!(
            names,
            [
                ("Standard", DocumentKind::Part),
                ("Standard", DocumentKind::Assembly),
                ("Standard", DocumentKind::Drawing)
            ]
        );
        let folders: Vec<_> = tree.folders.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(folders, ["English", "Metric"]);
        assert_eq!(tree.all().len(), 6);
        // A filled folder is left alone, even with the standard ones gone.
        fs::remove_file(d.join("Standard.ccpart")).unwrap();
        assert!(!ensure_builtin(&d).unwrap());
        assert!(standard_path(&d, DocumentKind::Part).is_none());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_template_gives_its_content_and_properties() {
        let d = tmp("inst");
        ensure_builtin(&d).unwrap();
        let ansi = instantiate(&d.join("English").join("ANSI.ccdrw")).unwrap();
        match &ansi.data {
            DocumentData::Drawing(dr) => {
                assert_eq!(dr.standard.kind, StandardKind::Ansi);
                assert_eq!(dr.sheets.len(), 1);
            }
            other => panic!("not a drawing: {:?}", other.kind()),
        }
        assert!(!ansi.properties.get("creation_date").is_empty());
        // A company template: its own properties and parameters stay.
        let mut p = Project::new("T");
        let part = PartDocument {
            material: Some("Steel".into()),
            ..Default::default()
        };
        let id = p.add_document("Company", DocumentData::Part(part));
        let doc = p.document_mut(id).unwrap();
        doc.properties.set("company", "ACME");
        doc.properties.set("author", "Templater");
        doc.properties.set("creation_date", "2001-01-01");
        let file = d.join("Company.ccpart");
        save_as_template(&p, id, &file).unwrap();
        let n = instantiate(&file).unwrap();
        assert_eq!(n.properties.get("company"), "ACME");
        assert_eq!(n.properties.get("author"), "Templater");
        assert_ne!(n.properties.get("creation_date"), "2001-01-01");
        match n.data {
            DocumentData::Part(pd) => assert_eq!(pd.material.as_deref(), Some("Steel")),
            other => panic!("not a part: {:?}", other.kind()),
        }
        // The standard one is used for quick New, the built-in without it.
        let s = standard_document(Some(&d), DocumentKind::Drawing).unwrap();
        assert!(matches!(s.data, DocumentData::Drawing(_)));
        let none = standard_document(Some(&d.join("missing")), DocumentKind::Assembly).unwrap();
        assert!(matches!(none.data, DocumentData::Assembly(_)));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn templates_are_read_by_content_and_hold_one_document() {
        let d = tmp("kinds");
        fs::create_dir_all(&d).unwrap();
        // A part saved under a drawing's extension is listed as a part.
        let mut p = Project::new("X");
        p.add_document("X", builtin(DocumentKind::Part));
        p.save_with(&d.join("Odd.ccdrw"), &SaveOptions { backup: false })
            .unwrap();
        // Two documents: no template.
        p.add_document("Y", builtin(DocumentKind::Part));
        p.save_with(&d.join("Two.ccpart"), &SaveOptions { backup: false })
            .unwrap();
        fs::write(d.join("Broken.ccasm"), b"not a zip").unwrap();
        fs::write(d.join("notes.txt"), b"ignored").unwrap();
        let tree = list(&d).unwrap();
        assert_eq!(tree.templates.len(), 1);
        assert_eq!(tree.templates[0].kind, DocumentKind::Part);
        assert_eq!(tree.unreadable.len(), 2);
        assert!(instantiate(&d.join("Two.ccpart")).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn save_as_template_refuses_references_and_wrong_types() {
        let d = tmp("refuse");
        let mut p = Project::new("A");
        let part = p.add_document("Bolt", builtin(DocumentKind::Part));
        let mut asm = AssemblyDocument::default();
        asm.place(
            super::super::DocRef::local(part),
            "Bolt",
            crate::kernel::Transform3::identity(),
            true,
        );
        let a = p.add_document("Asm", DocumentData::Assembly(asm));
        assert!(save_as_template(&p, a, &d.join("Asm.ccasm")).is_err());
        assert!(save_as_template(&p, part, &d.join("Bolt.ccasm")).is_err());
        save_as_template(&p, part, &d.join("Bolt.ccpart")).unwrap();
        assert!(!d.join("Bolt.ccpart.bak").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_project_folder_wins_over_the_default() {
        let mut pr = DesignProject::new("P");
        let app = PathBuf::from("/app/templates");
        assert_eq!(
            dir_for(&pr, Some(Path::new("/p")), Some(&app)),
            Some(app.clone())
        );
        pr.templates = Some("Templates".into());
        assert_eq!(
            dir_for(&pr, Some(Path::new("/p")), Some(&app)),
            Some(PathBuf::from("/p").join("Templates"))
        );
    }
}
