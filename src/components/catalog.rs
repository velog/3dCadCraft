//! What the Component Library holds: the built-in samples and the user's
//! published families, as one list of families with their members. A member's
//! file is written the first time it is needed and reused after that.

use std::path::{Path, PathBuf};

use super::samples::Detail;
use super::{samples, write_part};
use crate::document::iprops::IProperties;
use crate::document::{DocumentData, Project};
use crate::part::PartDocument;

/// Raised when a builder's shapes change without their table's numbers changing: the members written
/// before are written again.
const BUILDERS: u32 = 1;

/// Where a member comes from.
#[derive(Clone, Debug)]
pub enum Source {
    /// A built-in sample.
    Sample(Box<samples::Sample>),
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
    /// What its table was compared with (`data/components/SOURCES.md`); empty: not yet.
    pub checked: String,
    /// A published family's table.
    pub table: Option<crate::part::family::FamilyTable>,
    /// A published family's connection on an edge: what makes it droppable.
    pub connection: Option<crate::part::connection::Connection>,
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

/// Side of a family's picture, in pixels.
pub const THUMBNAIL: u32 = 56;
/// Folder of the pictures, a cache: deleting it only costs the time to draw them again.
const THUMBNAILS: &str = ".thumbnails";

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
                source: Source::Sample(Box::new(s)),
            };
            match c
                .families
                .iter_mut()
                .find(|f| f.file.is_none() && f.category == category && f.name == name)
            {
                Some(f) => f.members.push(entry),
                None => c.families.push(Family {
                    category,
                    checked: samples::checked_of(&name),
                    name,
                    standard,
                    file: None,
                    table: None,
                    connection: None,
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
                    Ok((table, connection)) => {
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
                            checked: String::new(),
                            table: Some(table),
                            connection,
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
        let phrase = words.join(" ");
        let (mut exact, mut out) = (Vec::new(), Vec::new());
        for (i, f) in self.families.iter().enumerate() {
            for (k, m) in f.members.iter().enumerate() {
                let hay = format!("{} {} {}", f.category, f.name, m.size).to_lowercase();
                if hay.contains(&phrase) {
                    exact.push((i, k));
                } else if words.iter().all(|w| hay.contains(w)) {
                    out.push((i, k));
                }
            }
        }
        // What was typed as one phrase comes first: `De 20 d 2` is a spring, not every part
        // whose name holds those letters.
        exact.append(&mut out);
        exact
    }

    /// The washers and nut of a bolt of family `family` with diameter `d`
    /// (Bolted Connection): the standard's plain washer and hex nut.
    pub fn bolted_stack(&self, family: usize, d: crate::kernel::Real) -> Option<super::fit::Stack> {
        let inch = self.families.get(family)?.standard == "ANSI";
        let (washers, nuts) = if inch {
            ("ASME B18.22.1 SAE", "ASME B18.2.2")
        } else {
            ("ISO 7089", "ISO 4032")
        };
        let t = super::fit::Target {
            hole: false,
            diameter: d,
            thread: None,
            depth: 0.0,
            blind: false,
        };
        let member = |name: &str| {
            let f = self.families.iter().position(|x| x.name == name)?;
            let k = super::fit::choose(&self.fits(f), &t)?;
            let Source::Sample(s) = &self.families[f].members.get(k)?.source else {
                return None;
            };
            Some((f, k, s.kind))
        };
        let (wf, wk, washer) = member(washers)?;
        let (nf, nk, nut) = member(nuts)?;
        let (samples::Kind::Washer(_, _, washer_h), samples::Kind::HexNut(_, _, nut_m)) =
            (washer, nut)
        else {
            return None;
        };
        Some(super::fit::Stack {
            washer: (wf, wk),
            washer_h,
            nut: (nf, nk),
            nut_m,
        })
    }

    /// The fit of each member, in order (a published family goes by its
    /// connection; without one it is placed by hand).
    pub fn fits(&self, family: usize) -> Vec<super::fit::Fit> {
        self.families
            .get(family)
            .map(|f| {
                f.members
                    .iter()
                    .map(|m| match &m.source {
                        Source::Sample(s) => super::fit::Fit::of(&s.kind),
                        Source::Row(r) => row_fit(f, *r),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The family and member a built-in member's file belongs to, at any
    /// level of detail, and that level (Change Size).
    pub fn locate_level(&self, path: &Path) -> Option<(usize, usize, Detail)> {
        let norm = |x: &Path| {
            crate::document::library::without_verbatim(
                x.canonicalize().unwrap_or_else(|_| x.to_path_buf()),
            )
        };
        let below = norm(path)
            .strip_prefix(norm(&self.root.join("Samples")))
            .ok()
            .map(Path::to_path_buf);
        for (i, f) in self.families.iter().enumerate() {
            for (k, m) in f.members.iter().enumerate() {
                if let Source::Sample(s) = &m.source {
                    if let Some(below) = &below {
                        for d in Detail::ALL {
                            if s.relative_path_at(d) == *below {
                                return Some((i, k, s.level_of(d)));
                            }
                        }
                    }
                } else if let Source::Row(r) = &m.source
                    && let Some(table) = &f.table
                    && let Ok(name) = table.member_name(*r)
                    && norm(&row_path(&self.root, table, &f.name, &name)) == norm(path)
                {
                    return Some((i, k, Detail::Standard));
                }
            }
        }
        None
    }

    /// The family and member a built-in member's file belongs to.
    pub fn locate(&self, path: &Path) -> Option<(usize, usize)> {
        self.locate_level(path).map(|(f, m, _)| (f, m))
    }

    /// A picture of a family's first member, `THUMBNAIL`² RGBA. Takes a build
    /// the first time (call it off the interface's thread), then reads the
    /// cache; a changed table or published file names another picture.
    pub fn thumbnail(&self, family: usize) -> Result<Vec<u8>, String> {
        let f = self.families.get(family).ok_or("no such family")?;
        let m = f.members.first().ok_or("the family has no members")?;
        let identity = match (&m.source, &f.file) {
            // `section` in the name: the pictures saved before bearings drew their section.
            (Source::Sample(s), _) => format!("{}|{:?}|section2", s.designation(), s.kind),
            (Source::Row(_), Some(file)) => {
                let time = std::fs::metadata(file)
                    .and_then(|d| d.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|t| t.as_secs())
                    .unwrap_or_default();
                format!("{}|{time}", file.display())
            }
            (Source::Row(_), None) => return Err("the family has no file".into()),
        };
        let path = self
            .root
            .join(THUMBNAILS)
            .join(format!("{}.png", super::derived_id("thumbnail", &identity)));
        if let Ok(bytes) = std::fs::read(&path)
            && let Some((w, h, rgba)) = crate::part::thumbnail::decode_png(&bytes)
            && (w, h) == (THUMBNAIL, THUMBNAIL)
        {
            return Ok(rgba);
        }
        if let Source::Sample(s) = &m.source
            && let (Some(style), samples::Kind::Bearing(d, od, b)) = (s.bearing_style(), s.kind)
        {
            let rgba = super::icon::bearing_section(style, d, od, b, THUMBNAIL);
            if let Some(png) = crate::part::thumbnail::encode_png(&rgba, THUMBNAIL, THUMBNAIL) {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&path, png);
            }
            return Ok(rgba);
        }
        let part = match (&m.source, &f.file) {
            (Source::Sample(s), _) => s
                .for_picture()
                .part_at(Detail::Standard)
                .map_err(|e| e.to_string())?,
            (Source::Row(r), Some(file)) => {
                let project = Project::load(file).map_err(|e| e.to_string())?;
                let (template, props) = first_part(&project).ok_or("the file holds no part")?;
                let table = template
                    .family
                    .as_ref()
                    .ok_or("the file holds no family table")?;
                table
                    .member(template, props, *r)
                    .map_err(|e| e.to_string())?
                    .part
            }
            (Source::Row(_), None) => return Err("the family has no file".into()),
        };
        let mut scene = crate::part::thumbnail::Scene::default();
        scene.add_part(&part, None);
        let rgba =
            crate::part::thumbnail::render(&scene, crate::part::thumbnail::iso_view(), THUMBNAIL)
                .ok_or("the part has nothing to draw")?;
        // The picture is only a cache: a folder that cannot be written costs time, nothing else.
        if let Some(png) = crate::part::thumbnail::encode_png(&rgba, THUMBNAIL, THUMBNAIL) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, png);
        }
        Ok(rgba)
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
                // A corrected table or a better shape reaches the file; its ids stay, so links still resolve.
                let stamp = path.with_extension("stamp");
                let made_from = format!("{BUILDERS}|{:?}|{:?}", s.kind, s.finish);
                let current = std::fs::read_to_string(&stamp).is_ok_and(|had| had == made_from);
                if !path.exists() || !current {
                    let part = s.part_at(level).map_err(|e| e.to_string())?;
                    // Standard keeps the identity files already have.
                    let identity = match level {
                        Detail::Standard => s.designation(),
                        d => format!("{}|{}", d.label(), s.designation()),
                    };
                    write_part(&path, &identity, part, s.properties())?;
                    std::fs::write(&stamp, made_from).map_err(|e| e.to_string())?;
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
                let row = table.member_name(*r)?;
                let name = format!("{} {row}", f.name);
                let path = row_path(&self.root, table, &f.name, &row);
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

/// The file of a published family's member.
fn row_path(
    root: &Path,
    table: &crate::part::family::FamilyTable,
    family: &str,
    row: &str,
) -> PathBuf {
    user_dir(root)
        .join(MEMBERS)
        .join(safe_name(&table.id))
        .join(format!("{}.ccpart", safe_name(&format!("{family} {row}"))))
}

/// How row `r` of a published family lands: by its connection, at the diameter
/// its column gives.
fn row_fit(f: &Family, r: usize) -> super::fit::Fit {
    use crate::part::connection::ConnectionRole;
    use crate::part::expr::Value;
    let Some(c) = &f.connection else {
        return super::fit::Fit::Manual;
    };
    let d = c
        .diameter_column
        .as_ref()
        .and_then(|col| match f.table.as_ref()?.values(r).ok()?.get(col)? {
            Value::Num(v, _) => Some(*v),
            _ => None,
        })
        .unwrap_or(c.diameter);
    match c.role {
        ConnectionRole::Into => super::fit::Fit::Into {
            d,
            length: None,
            sink: 0.0,
        },
        ConnectionRole::Over => super::fit::Fit::Over { bore: d },
    }
}

/// A published file's table, and its connection on an edge.
fn read_family(
    path: &Path,
) -> Result<
    (
        crate::part::family::FamilyTable,
        Option<crate::part::connection::Connection>,
    ),
    String,
> {
    let project = Project::load(path).map_err(|e| e.to_string())?;
    let (part, _) = first_part(&project).ok_or("the file holds no part")?;
    let table = part
        .family
        .clone()
        .ok_or_else(|| "the file holds no family table".to_owned())?;
    let connection = part.connections.iter().find(|c| c.edge.is_some()).cloned();
    Ok((table, connection))
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
