//! Pack and Go (spec `docs/spec/0.6d-pack-and-go.md`): a file and every
//! file it links, copied into one destination folder with links that work
//! there, plus a new design project (`.ccproj`) for the copy.
//!
//! Three steps, GUI-free: [`collect`] walks the links from the source files
//! (only the manifests are read), [`plan`] decides where each file goes,
//! [`run`] copies (byte for byte, or with only the link paths rewritten)
//! and writes the project. The originals are never touched.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::design_project::{DesignProject, NamedPath, relative_inside};
use super::format::{copy_relinked, read_preview};
use super::library::{locate, relative_path, without_verbatim};
use super::{LibraryId, LibraryLink};

/// Where the copies go (spec 0.6d §3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// The project's folders rebuilt under the destination (the default).
    KeepHierarchy,
    /// Every file into the destination folder itself.
    SinglePath,
}

/// Pack and Go's options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackOptions {
    /// Where the copies go.
    pub layout: Layout,
    /// Library files stay where they are; links point at them absolutely.
    pub skip_libraries: bool,
    /// The templates folder is not copied.
    pub skip_templates: bool,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            layout: Layout::KeepHierarchy,
            skip_libraries: false,
            skip_templates: true,
        }
    }
}

/// The design project the files belong to: the project and its `.ccproj`
/// folder (`None` for the built-in Default project, whose paths are
/// absolute).
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    /// The project's settings.
    pub project: &'a DesignProject,
    /// The folder of its `.ccproj`.
    pub base: Option<&'a Path>,
}

impl Scope<'_> {
    fn locate_in(&self) -> Option<(&DesignProject, &Path)> {
        self.base.map(|b| (self.project, b))
    }

    fn resolve(&self, p: &Path) -> PathBuf {
        match self.base {
            Some(b) => DesignProject::resolve(b, p),
            None => p.to_path_buf(),
        }
    }

    /// The libraries with their absolute folders.
    fn libraries(&self) -> Vec<(&NamedPath, PathBuf)> {
        self.project
            .libraries
            .iter()
            .map(|l| (l, self.resolve(&l.path)))
            .collect()
    }
}

/// One link of a found file.
#[derive(Clone, Debug)]
pub struct FoundLink {
    /// The link as stored.
    pub link: LibraryLink,
    /// The file it reaches (`None`: not found).
    pub target: Option<PathBuf>,
}

/// A file reached from the source.
#[derive(Clone, Debug)]
pub struct Found {
    /// Its place (canonical, no `\\?\`).
    pub path: PathBuf,
    /// Its size in bytes.
    pub size: u64,
    /// Its links.
    pub links: Vec<FoundLink>,
    /// Why it cannot be packed (unreadable, an unpacked folder…).
    pub problem: Option<String>,
}

/// `p` canonical without the verbatim prefix (as it is when that fails).
fn key(p: &Path) -> PathBuf {
    without_verbatim(p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
}

/// Every file `roots` reach through their links, each once, roots first.
pub fn collect(roots: &[PathBuf], scope: Scope) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut queue: Vec<PathBuf> = roots.iter().map(|r| key(r)).collect();
    queue.reverse();
    while let Some(path) = queue.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let meta = fs::metadata(&path);
        let mut found = Found {
            path: path.clone(),
            size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            links: Vec::new(),
            problem: None,
        };
        match meta {
            Ok(m) if m.is_dir() => {
                found.problem = Some("an unpacked folder: save it as a file to pack it".into())
            }
            Ok(_) => match read_preview(&path) {
                Ok(f) => {
                    let mut next = Vec::new();
                    for link in f.libraries {
                        let target = locate(&path, &link, scope.locate_in()).map(|t| key(&t));
                        if let Some(t) = &target {
                            next.push(t.clone());
                        }
                        found.links.push(FoundLink { link, target });
                    }
                    // Depth first in link order keeps the list readable.
                    queue.extend(next.into_iter().rev());
                }
                Err(e) => found.problem = Some(e.to_string()),
            },
            Err(e) => found.problem = Some(e.to_string()),
        }
        out.push(found);
    }
    out
}

/// Model files (by extension) that may link others.
fn is_design_file(p: &Path) -> bool {
    super::filetype::FileType::from_path(p).is_some_and(|t| t.is_model())
}

/// Files under `dirs` (subfolders when `deep`) that link a file of `found`
/// and are not in it yet (Search for referencing files, spec 0.6d §3.1).
pub fn referencing(found: &[Found], dirs: &[PathBuf], deep: bool, scope: Scope) -> Vec<PathBuf> {
    let have: HashSet<&PathBuf> = found.iter().map(|f| &f.path).collect();
    let mut files = Vec::new();
    for d in dirs {
        walk(d, deep, 0, &mut files);
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for f in files {
        let k = key(&f);
        if have.contains(&k) || out.contains(&k) {
            continue;
        }
        let Ok(p) = read_preview(&k) else { continue };
        let hits = p
            .libraries
            .iter()
            .any(|l| locate(&k, l, scope.locate_in()).is_some_and(|t| have.contains(&key(&t))));
        if hits {
            out.push(k);
        }
    }
    out.sort();
    out
}

fn walk(dir: &Path, deep: bool, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => {
                let n = e.file_name().to_string_lossy().into_owned();
                if deep && depth < 16 && !n.starts_with('.') && n != "OldVersions" {
                    walk(&p, deep, depth + 1, out);
                }
            }
            Ok(t) if t.is_file() && is_design_file(&p) => out.push(p),
            _ => {}
        }
    }
}

/// A file's place in the package.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// The original.
    pub source: PathBuf,
    /// Relative to the destination; `None` = not copied (a skipped library
    /// file: links point at `source`).
    pub dest: Option<PathBuf>,
    /// Its size in bytes.
    pub size: u64,
}

/// What Start will do.
#[derive(Clone, Debug)]
pub struct Plan {
    /// Every file with its place.
    pub files: Vec<Placed>,
    /// The new project: its file name and content.
    pub project: (PathBuf, DesignProject),
    /// The templates folder to copy and where (relative).
    pub templates: Option<(PathBuf, PathBuf)>,
    /// Files left out and links not found, as messages.
    pub problems: Vec<String>,
}

impl Plan {
    /// Files copied and their bytes.
    pub fn totals(&self) -> (usize, u64) {
        self.files
            .iter()
            .filter(|f| f.dest.is_some())
            .fold((0, 0), |(n, s), f| (n + 1, s + f.size))
    }
}

/// `name` with ` (2)`, ` (3)`… before the extension until `taken` lacks it
/// (any case on Windows).
fn unique(rel: PathBuf, taken: &mut HashSet<String>) -> PathBuf {
    let fold = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    let mut cand = rel.clone();
    let mut k = 2;
    while taken.contains(&fold(&cand)) {
        let stem = rel.file_stem().unwrap_or_default().to_string_lossy();
        let name = match rel.extension() {
            Some(e) => format!("{stem} ({k}).{}", e.to_string_lossy()),
            None => format!("{stem} ({k})"),
        };
        cand = rel.with_file_name(name);
        k += 1;
    }
    taken.insert(fold(&cand));
    cand
}

/// The project's name for the copy.
fn project_name(scope: Scope, found: &[Found]) -> String {
    if scope.base.is_some() && !scope.project.name.trim().is_empty() {
        return scope.project.name.clone();
    }
    found
        .first()
        .and_then(|f| f.path.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Package".into())
}

/// Where each file goes (spec 0.6d §3.1).
pub fn plan(found: &[Found], scope: Scope, opts: &PackOptions) -> Plan {
    let base = scope.base.map(Path::to_path_buf).or_else(|| {
        found
            .first()
            .and_then(|f| f.path.parent())
            .map(Path::to_path_buf)
    });
    let workspace = scope.resolve(&scope.project.workspace);
    let libraries = scope.libraries();
    let mut taken: HashSet<String> = HashSet::new();
    let mut used_libs: HashSet<String> = HashSet::new();
    let mut files = Vec::new();
    let mut problems = Vec::new();
    for f in found {
        if let Some(p) = &f.problem {
            problems.push(format!("{}: {p}", f.path.display()));
            continue;
        }
        for l in &f.links {
            if l.target.is_none() {
                problems.push(format!(
                    "{}: link to {} not found",
                    f.path.display(),
                    l.link.path
                ));
            }
        }
        let name = PathBuf::from(f.path.file_name().unwrap_or_default());
        let lib = libraries
            .iter()
            .find_map(|(l, dir)| relative_inside(&f.path, dir).map(|r| (*l, r)));
        let lib_name = lib.as_ref().map(|(l, _)| l.name.clone());
        let dest = match (opts.layout, lib) {
            (_, Some(_)) if opts.skip_libraries => None,
            (Layout::SinglePath, _) => Some(name),
            (Layout::KeepHierarchy, Some((l, r))) => {
                Some(Path::new("Libraries").join(&l.name).join(r))
            }
            (Layout::KeepHierarchy, None) => Some(
                base.as_deref()
                    .and_then(|b| relative_inside(&f.path, b))
                    .or_else(|| relative_inside(&f.path, &workspace))
                    .unwrap_or(name),
            ),
        };
        if let Some(n) = lib_name.filter(|_| !opts.skip_libraries) {
            used_libs.insert(n);
        }
        files.push(Placed {
            source: f.path.clone(),
            dest: dest.map(|d| unique(d, &mut taken)),
            size: f.size,
        });
    }
    // The new project: every folder relative to the destination where the
    // copy holds it, absolute where it stays outside.
    let mut project = DesignProject::new(&project_name(scope, found));
    project.old_versions = scope.project.old_versions;
    project.unique_names = scope.project.unique_names;
    project.owner = scope.project.owner.clone();
    for (l, dir) in &libraries {
        let path = if used_libs.contains(&l.name) && opts.layout == Layout::KeepHierarchy {
            Path::new("Libraries").join(&l.name)
        } else if used_libs.contains(&l.name) {
            PathBuf::from(".")
        } else {
            dir.clone()
        };
        project.libraries.push(NamedPath {
            name: l.name.clone(),
            path,
        });
    }
    let templates = match (&scope.project.templates, opts.skip_templates) {
        (Some(t), false) => {
            let dir = scope.resolve(t);
            dir.is_dir().then(|| {
                project.templates = Some(PathBuf::from("Templates"));
                (dir, PathBuf::from("Templates"))
            })
        }
        _ => None,
    };
    let file = PathBuf::from(format!("{}.ccproj", project.name));
    let file = unique(file, &mut taken);
    Plan {
        files,
        project: (file, project),
        templates,
        problems,
    }
}

/// Files of `plan` that already exist under `dest` (Start refuses them).
pub fn conflicts(plan: &Plan, dest: &Path) -> Vec<PathBuf> {
    plan.files
        .iter()
        .filter_map(|f| f.dest.as_ref())
        .chain(std::iter::once(&plan.project.0))
        .map(|r| dest.join(r))
        .chain(plan.templates.iter().map(|(_, r)| dest.join(r)))
        .filter(|p| p.exists())
        .collect()
}

/// Refuses a destination inside a folder the package is read from.
pub fn check_destination(found: &[Found], scope: Scope, dest: &Path) -> Result<(), String> {
    let dest = key(dest);
    let mut sources: Vec<PathBuf> = scope.base.map(Path::to_path_buf).into_iter().collect();
    sources.extend(
        found
            .iter()
            .filter_map(|f| f.path.parent().map(Path::to_path_buf)),
    );
    if sources.iter().any(|s| relative_inside(&dest, s).is_some()) {
        return Err("the destination is inside the project".into());
    }
    Ok(())
}

/// Copies the package into `dest` (created when missing). Returns the
/// number of files written.
pub fn run(plan: &Plan, found: &[Found], dest: &Path) -> Result<usize, String> {
    let clash = conflicts(plan, dest);
    if let Some(c) = clash.first() {
        return Err(format!("{} already exists", c.display()));
    }
    fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let dest_abs = key(dest);
    let place: HashMap<&PathBuf, &Placed> = plan.files.iter().map(|p| (&p.source, p)).collect();
    let mut written = 0;
    for f in found {
        let Some(placed) = place.get(&f.path) else {
            continue;
        };
        let Some(rel) = &placed.dest else { continue };
        let to = dest_abs.join(rel);
        let dir = to.parent().unwrap_or(&dest_abs).to_path_buf();
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut changes: Vec<(LibraryId, String)> = Vec::new();
        for l in &f.links {
            let Some(t) = &l.target else { continue };
            let new = match place.get(t).and_then(|p| p.dest.as_ref()) {
                Some(r) => relative_path(&dir, &dest_abs.join(r)),
                None => t.to_string_lossy().replace('\\', "/"),
            };
            if new != l.link.path {
                changes.push((l.link.id, new));
            }
        }
        let r = if changes.is_empty() {
            fs::copy(&f.path, &to)
                .map(|_| ())
                .map_err(|e| e.to_string())
        } else {
            copy_relinked(&f.path, &to, &changes).map_err(|e| e.to_string())
        };
        r.map_err(|e| format!("{}: {e}", to.display()))?;
        written += 1;
    }
    if let Some((src, rel)) = &plan.templates {
        copy_dir(src, &dest_abs.join(rel))?;
    }
    let (file, project) = &plan.project;
    let path = dest_abs.join(file);
    project
        .save(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(written)
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let entries = fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
    for e in entries.flatten() {
        let p = e.path();
        let q = to.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &q)?;
        } else if !q.exists() {
            fs::copy(&p, &q).map_err(|e| format!("{}: {e}", q.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::library::Resolver;
    use crate::document::tests::{assembly_of, part};
    use crate::document::{DocRef, Project};

    /// `Pump/` (project: workspace `.`, library `Fasteners` = `../Lib`) with
    /// `Parts/plate.ccpart`, `Asm/pump.ccasm` (plate + a library bolt),
    /// `Drw/pump.ccdrw` (links the assembly) and `Lib/bolt.ccpart`.
    fn design(root: &Path) -> (PathBuf, DesignProject) {
        for d in [
            "Pump/Parts",
            "Pump/Asm",
            "Pump/Drw",
            "Lib",
            "Pump/Templates",
        ] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let save_part = |path: &Path, name: &str| {
            let mut p = Project::new(name);
            let id = p.add_document(name, part());
            p.save(path).unwrap();
            id
        };
        let plate = root.join("Pump/Parts/plate.ccpart");
        let bolt = root.join("Lib/bolt.ccpart");
        let plate_id = save_part(&plate, "Plate");
        let bolt_id = save_part(&bolt, "Bolt");
        let asm = root.join("Pump/Asm/pump.ccasm");
        let mut r = Resolver::new();
        let mut a = Project::new("Pump");
        let refs: Vec<DocRef> = vec![
            r.link(&mut a, &asm, &plate, plate_id).unwrap(),
            r.link(&mut a, &asm, &bolt, bolt_id).unwrap(),
        ];
        let asm_id = a.add_document("Pump", assembly_of(&refs));
        a.save(&asm).unwrap();
        let drw = root.join("Pump/Drw/pump.ccdrw");
        let mut d = Project::new("Pump drawing");
        let link = r.link(&mut d, &drw, &asm, asm_id).unwrap();
        d.add_document("Pump drawing", assembly_of(&[link]));
        d.save(&drw).unwrap();
        fs::write(root.join("Pump/Templates/Standard.ccpart"), b"t").unwrap();
        let mut p = DesignProject::new("Pump");
        p.libraries.push(NamedPath {
            name: "Fasteners".into(),
            path: "../Lib".into(),
        });
        p.templates = Some("Templates".into());
        (root.join("Pump"), p)
    }

    #[test]
    fn keep_hierarchy_packs_everything_and_the_copy_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let root = key(dir.path());
        let (base, project) = design(&root);
        let scope = Scope {
            project: &project,
            base: Some(&base),
        };
        let asm = base.join("Asm/pump.ccasm");
        let found = collect(std::slice::from_ref(&asm), scope);
        let names: Vec<_> = found
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["pump.ccasm", "plate.ccpart", "bolt.ccpart"]);
        // The drawing links the assembly: Search for referencing files.
        let more = referencing(&found, std::slice::from_ref(&base), true, scope);
        assert_eq!(more, [base.join("Drw/pump.ccdrw")]);
        let mut roots = vec![asm.clone()];
        roots.extend(more);
        let found = collect(&roots, scope);
        assert_eq!(found.len(), 4);

        let opts = PackOptions {
            skip_templates: false,
            ..Default::default()
        };
        let plan = plan(&found, scope, &opts);
        assert!(plan.problems.is_empty(), "{:?}", plan.problems);
        let dests: Vec<_> = plan.files.iter().map(|f| f.dest.clone().unwrap()).collect();
        assert_eq!(
            dests,
            [
                Path::new("Asm").join("pump.ccasm"),
                Path::new("Parts").join("plate.ccpart"),
                Path::new("Libraries").join("Fasteners").join("bolt.ccpart"),
                Path::new("Drw").join("pump.ccdrw"),
            ]
        );
        let dest = root.join("Out");
        assert!(check_destination(&found, scope, &base.join("Out")).is_err());
        check_destination(&found, scope, &dest).unwrap();
        assert_eq!(run(&plan, &found, &dest).unwrap(), 4);
        // Moved elsewhere, the package is whole: every link resolves to
        // the copy, never to the original.
        let moved = root.join("Moved");
        fs::rename(&dest, &moved).unwrap();
        fs::remove_dir_all(&base).unwrap();
        fs::remove_dir_all(root.join("Lib")).unwrap();
        let p = DesignProject::load(&moved.join("Pump.ccproj")).unwrap();
        assert_eq!(
            p.libraries[0].path,
            Path::new("Libraries").join("Fasteners")
        );
        assert_eq!(p.templates, Some(PathBuf::from("Templates")));
        assert!(moved.join("Templates/Standard.ccpart").is_file());
        let scope2 = Scope {
            project: &p,
            base: Some(&moved),
        };
        let again = collect(&[moved.join("Drw/pump.ccdrw")], scope2);
        assert_eq!(again.len(), 4);
        assert!(again.iter().all(|f| f.problem.is_none()));
        assert!(
            again
                .iter()
                .flat_map(|f| &f.links)
                .all(|l| l.target.as_ref().is_some_and(|t| t.starts_with(&moved)))
        );
        // A file whose links were rewritten still loads whole.
        let a = Project::load(&moved.join("Asm/pump.ccasm")).unwrap();
        assert_eq!(a.libraries().len(), 2);
        assert_eq!(a.libraries()[1].path, "../Libraries/Fasteners/bolt.ccpart");
        // Packing again over it is refused.
        assert!(run(&plan, &found, &moved).is_err());
    }

    #[test]
    fn single_path_skip_libraries_and_name_clashes() {
        let dir = tempfile::tempdir().unwrap();
        let root = key(dir.path());
        let (base, project) = design(&root);
        // A second plate.ccpart in another folder, placed in the assembly.
        fs::create_dir_all(base.join("Old")).unwrap();
        let mut q = Project::new("Old plate");
        let qid = q.add_document("Old plate", part());
        q.save(&base.join("Old/plate.ccpart")).unwrap();
        let top = base.join("top.ccasm");
        let mut r = Resolver::new();
        let mut t = Project::new("Top");
        let old = r
            .link(&mut t, &top, &base.join("Old/plate.ccpart"), qid)
            .unwrap();
        t.add_document("Top", assembly_of(&[old]));
        t.save(&top).unwrap();
        let scope = Scope {
            project: &project,
            base: Some(&base),
        };
        let found = collect(&[top.clone(), base.join("Asm/pump.ccasm")], scope);
        let opts = PackOptions {
            layout: Layout::SinglePath,
            skip_libraries: true,
            skip_templates: true,
        };
        let plan = plan(&found, scope, &opts);
        let dests: Vec<_> = plan.files.iter().map(|f| f.dest.clone()).collect();
        assert!(dests.contains(&Some(PathBuf::from("plate.ccpart"))));
        assert!(dests.contains(&Some(PathBuf::from("plate (2).ccpart"))));
        assert!(dests.contains(&None), "the library bolt stays");
        let dest = root.join("Single");
        run(&plan, &found, &dest).unwrap();
        let a = Project::load(&dest.join("pump.ccasm")).unwrap();
        let bolt = a.libraries()[1].path.clone();
        assert!(Path::new(&bolt).is_absolute(), "{bolt}");
        let p = DesignProject::load(&dest.join("Pump.ccproj")).unwrap();
        assert!(p.libraries[0].path.is_absolute());
        assert!(p.templates.is_none());
    }

    #[test]
    fn a_moved_link_is_found_in_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let root = key(dir.path());
        let (base, mut project) = design(&root);
        // The plate moves to another subfolder: the stored path is gone.
        fs::create_dir_all(base.join("Moved/Deep")).unwrap();
        fs::rename(
            base.join("Parts/plate.ccpart"),
            base.join("Moved/Deep/plate.ccpart"),
        )
        .unwrap();
        // A different file with the same name, earlier in the search: not
        // the same design file, so it is passed over.
        fs::create_dir_all(base.join("A")).unwrap();
        Project::new("Impostor")
            .save(&base.join("A/plate.ccpart"))
            .unwrap();
        let asm = base.join("Asm/pump.ccasm");
        let link = read_preview(&asm).unwrap().libraries[0].clone();
        assert_eq!(locate(&asm, &link, Some((&project, &base))), None);
        project.unique_names = true;
        assert_eq!(
            locate(&asm, &link, Some((&project, &base))).map(|p| key(&p)),
            Some(base.join("Moved/Deep/plate.ccpart"))
        );
        // Search order without unique names: the stored folders under the
        // workspace.
        project.unique_names = false;
        fs::create_dir_all(root.join("W/Parts")).unwrap();
        fs::rename(
            base.join("Moved/Deep/plate.ccpart"),
            root.join("W/Parts/plate.ccpart"),
        )
        .unwrap();
        project.workspace = "../W".into();
        assert_eq!(
            locate(&asm, &link, Some((&project, &base))).map(|p| key(&p)),
            Some(root.join("W/Parts/plate.ccpart"))
        );
    }
}
