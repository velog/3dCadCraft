//! What the Component Library holds: the built-in samples and the user's
//! published families, as one list of families with their members. A member's
//! file is written the first time it is needed and reused after that.

use std::path::{Path, PathBuf};

use super::samples::Detail;
use super::{samples, write_part};
use crate::document::iprops::IProperties;
use crate::document::{DocumentData, Project};
use crate::part::PartDocument;

/// Where a member comes from.
#[derive(Clone, Debug)]
pub enum Source {
    /// A built-in sample.
    Sample(samples::Sample),
    /// A row of a published family's table.
    Row(usize),
}

/// One size of a family.
#[derive(Clone, Debug)]
pub struct Entry {
    /// What the size list shows.
    pub size: String,
    /// How it is made.
    pub source: Source,
}

/// One family.
#[derive(Clone, Debug)]
pub struct Family {
    /// `/`-separated category (`Fasteners/Bolts/Hex Head`).
    pub category: String,
    /// Its name.
    pub name: String,
    /// Which standards body: `ISO`, `DIN`, `ANSI`, `EN`, `Other`; `Own` = published.
    pub standard: String,
    /// The user's published file; `None` = built in.
    pub file: Option<PathBuf>,
    /// The members.
    pub members: Vec<Entry>,
}

/// Every family of the library.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    /// Where its folders are.
    pub root: PathBuf,
    /// The families.
    pub families: Vec<Family>,
    /// Published files that could not be read: `<file>: <reason>`.
    pub warnings: Vec<String>,
}

/// The folder of the user's published families.
pub fn user_dir(root: &Path) -> PathBuf {
    root.join("User")
}

/// Folder of the generated members, skipped when families are listed.
const MEMBERS: &str = ".members";

impl Catalog {
    /// Lists the built-in samples and the families published under `root`.
    pub fn load(root: &Path) -> Self {
        let mut c = Catalog {
            root: root.to_path_buf(),
            ..Default::default()
        };
        for s in samples::all() {
            let category = s.category.clone();
            let name = s.family.clone();
            let standard = s.standard.clone();
            let entry = Entry {
                size: s.size.clone(),
                source: Source::Sample(s),
            };
            match c
                .families
                .iter_mut()
                .find(|f| f.file.is_none() && f.category == category && f.name == name)
            {
                Some(f) => f.members.push(entry),
                None => c.families.push(Family {
                    category,
                    name,
                    standard,
                    file: None,
                    members: vec![entry],
                }),
            }
        }
        let users = user_dir(root);
        c.scan(&users, &users);
        c
    }

    fn scan(&mut self, top: &Path, dir: &Path) {
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                if p.file_name().is_none_or(|n| n != MEMBERS) {
                    self.scan(top, &p);
                }
            } else if p.extension().is_some_and(|e| e == "ccpart") {
                match read_family(&p) {
                    Ok(table) => {
                        let category = dir
                            .strip_prefix(top)
                            .map(|r| r.to_string_lossy().replace('\\', "/"))
                            .unwrap_or_default();
                        let name = p
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let members = (0..table.rows.len())
                            .map(|r| Entry {
                                size: table
                                    .member_name(r)
                                    .unwrap_or_else(|_| format!("Member {}", r + 1)),
                                source: Source::Row(r),
                            })
                            .collect();
                        self.families.push(Family {
                            category,
                            name,
                            standard: "Own".to_owned(),
                            file: Some(p),
                            members,
                        });
                    }
                    Err(e) => self.warnings.push(format!("{}: {e}", p.display())),
                }
            }
        }
    }

    /// The standards present, in the order the window offers them.
    pub fn standards(&self) -> Vec<String> {
        let mut out = Vec::new();
        for name in ["ISO", "DIN", "ANSI", "EN", "Other", "Own"] {
            if self.families.iter().any(|f| f.standard == name) {
                out.push(name.to_owned());
            }
        }
        out
    }

    /// Does family `f` pass the filter (`None` = all standards)?
    pub fn in_standard(&self, f: usize, standard: Option<&str>) -> bool {
        standard.is_none_or(|s| self.families.get(f).is_some_and(|x| x.standard == s))
    }

    /// Every category with its parents, sorted (a tree in reading order).
    /// With `standard`, only those holding a family of it.
    pub fn categories_of(&self, standard: Option<&str>) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (i, f) in self.families.iter().enumerate() {
            if !self.in_standard(i, standard) {
                continue;
            }
            let mut at = String::new();
            for part in f.category.split('/').filter(|s| !s.is_empty()) {
                if !at.is_empty() {
                    at.push('/');
                }
                at.push_str(part);
                if !out.contains(&at) {
                    out.push(at.clone());
                }
            }
        }
        out.sort();
        out
    }

    /// Every category with its parents, sorted (a tree in reading order).
    pub fn categories(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for f in &self.families {
            let mut at = String::new();
            for part in f.category.split('/').filter(|s| !s.is_empty()) {
                if !at.is_empty() {
                    at.push('/');
                }
                at.push_str(part);
                if !out.contains(&at) {
                    out.push(at.clone());
                }
            }
        }
        out.sort();
        out
    }

    /// The families in `category` and the categories below it.
    pub fn in_category(&self, category: &str) -> Vec<usize> {
        let below = format!("{category}/");
        (0..self.families.len())
            .filter(|&i| {
                let c = &self.families[i].category;
                category.is_empty() || c == category || c.starts_with(&below)
            })
            .collect()
    }

    /// `(family, member)` pairs whose names hold every word of `text`, in
    /// any case. A family's name matching lists all its members.
    pub fn search(&self, text: &str) -> Vec<(usize, usize)> {
        let words: Vec<String> = text.split_whitespace().map(str::to_lowercase).collect();
        let mut out = Vec::new();
        for (i, f) in self.families.iter().enumerate() {
            for (k, m) in f.members.iter().enumerate() {
                let hay = format!("{} {} {}", f.category, f.name, m.size).to_lowercase();
                if words.iter().all(|w| hay.contains(w)) {
                    out.push((i, k));
                }
            }
        }
        out
    }

    /// The file of a member at the usual level, written now if it is missing.
    pub fn member_file(&self, family: usize, member: usize) -> Result<PathBuf, String> {
        self.member_file_at(family, member, Detail::Standard)
    }

    /// The levels a member offers: Standard, and what its shape adds. A
    /// published family has its one shape.
    pub fn levels(&self, family: usize, member: usize) -> Vec<Detail> {
        let Some(Source::Sample(s)) = self
            .families
            .get(family)
            .and_then(|f| f.members.get(member))
            .map(|m| &m.source)
        else {
            return vec![Detail::Standard];
        };
        Detail::ALL
            .into_iter()
            .filter(|d| s.level_of(*d) == *d)
            .collect()
    }

    /// The file of a member at `detail`, written now if it is missing.
    pub fn member_file_at(
        &self,
        family: usize,
        member: usize,
        detail: Detail,
    ) -> Result<PathBuf, String> {
        let f = self.families.get(family).ok_or("no such family")?;
        let m = f.members.get(member).ok_or("no such member")?;
        match (&m.source, &f.file) {
            (Source::Sample(s), _) => {
                let level = s.level_of(detail);
                let path = self.root.join("Samples").join(s.relative_path_at(level));
                if !path.exists() {
                    let part = s.part_at(level).map_err(|e| e.to_string())?;
                    // Standard keeps the identity files already have.
                    let identity = match level {
                        Detail::Standard => s.designation(),
                        d => format!("{}|{}", d.label(), s.designation()),
                    };
                    write_part(&path, &identity, part, s.properties())?;
                }
                Ok(path)
            }
            (Source::Row(r), Some(file)) => {
                let project = Project::load(file).map_err(|e| e.to_string())?;
                let (template, props) = first_part(&project).ok_or("the file holds no part")?;
                let table = template
                    .family
                    .as_ref()
                    .ok_or("the file holds no family table")?;
                // Named with its family: the file's name becomes the occurrence's.
                let name = format!("{} {}", f.name, table.member_name(*r)?);
                let path = user_dir(&self.root)
                    .join(MEMBERS)
                    .join(safe_name(&table.id))
                    .join(format!("{}.ccpart", safe_name(&name)));
                if !path.exists() {
                    let made = table
                        .member(template, props, *r)
                        .map_err(|e| e.to_string())?;
                    write_part(
                        &path,
                        &format!("{}|{name}", table.id),
                        made.part,
                        made.properties,
                    )?;
                }
                Ok(path)
            }
            (Source::Row(_), None) => Err("the family has no file".into()),
        }
    }
}

fn first_part(p: &Project) -> Option<(&PartDocument, &IProperties)> {
    p.documents().iter().find_map(|d| match &d.data {
        DocumentData::Part(part) => Some((part, &d.properties)),
        _ => None,
    })
}

fn read_family(path: &Path) -> Result<crate::part::family::FamilyTable, String> {
    let project = Project::load(path).map_err(|e| e.to_string())?;
    let (part, _) = first_part(&project).ok_or("the file holds no part")?;
    part.family
        .clone()
        .ok_or_else(|| "the file holds no family table".to_owned())
}

/// A name usable as a file name on every system.
fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim_end_matches(['.', ' '])
        .to_owned()
}

/// Publishes a part with a family table: its file in the user's library.
/// Publishing again replaces it. Refused while the table has problems; nothing
/// is written then.
pub fn publish(
    root: &Path,
    category: &str,
    name: &str,
    part: &PartDocument,
    properties: &IProperties,
) -> Result<PathBuf, String> {
    let table = part
        .family
        .as_ref()
        .ok_or("the part has no family table: use Create Part Family first")?;
    if let Some(i) = table.verify(part).first() {
        return Err(format!("the table: {}", i.message));
    }
    let name = safe_name(name.trim());
    if name.is_empty() {
        return Err("give the family a name".into());
    }
    let mut dir = user_dir(root);
    for c in category.split('/').map(str::trim).filter(|c| !c.is_empty()) {
        dir.push(safe_name(c));
    }
    let path = dir.join(format!("{name}.ccpart"));
    write_part(
        &path,
        &format!("family|{}", table.id),
        part.clone(),
        properties.clone(),
    )?;
    Ok(path)
}
