//! Bolted Connection's logic in the app (spec `docs/spec/6.2-bolted-connection.md`
//! §3–§4): OK drills the holes (bodies of the assembly's tools part, cutting
//! only the plates between the start plane and the termination, spec 3.10
//! §3.8), places the fasteners from the Component Library with relationships
//! (the hole's axis, each seat on its plane) and keeps one
//! `BoltedConnection` that the browser shows with Edit and Delete. Every step
//! works on the project after one Undo checkpoint and puts the old project
//! back when a later one fails (rule 5).

use std::path::PathBuf;

use eframe::egui;

use super::CadCraftApp;
use super::bolted::{self as panel, Role, StackItem};
use super::part_planned::PartDialog;
use crate::assembly::bolted::{self, BoltFastener, BoltKind, BoltedConnection, Spread};
use crate::assembly::relation::{Geom, GeomRef};
use crate::components::catalog::{Catalog, Source};
use crate::components::fit::{self, Fit, Target};
use crate::components::samples::{Detail, Kind};
use crate::document::{DocumentData, DocumentId};
use crate::kernel::Real;
use crate::part::standards::Standards;

/// Plates closer than this to a plane are on it (mm).
const ON: Real = 0.05;

/// A connection in the browser: id, title, its fasteners (occurrence, name).
type BoltRow = (u32, String, Vec<(u32, String)>);

/// A preview member: its file, on the nut side, its seat.
type GhostAt = (PathBuf, bool, Real);

/// Occurrence paths a hole cuts.
type Paths = Vec<Vec<u32>>;

/// One fastener as it will be placed.
struct Piece {
    item: StackItem,
    nut_side: bool,
    /// Its seat's distance from the plate along the axis (head side: above
    /// the start plane; nut side: below the termination).
    offset: Real,
    file: PathBuf,
    doc: DocumentId,
    name: String,
}

/// Everything OK needs, found before anything changes.
struct Plan {
    kind: BoltKind,
    /// Each bolt: its axis and its axis hole (cutting nothing).
    instances: Vec<bolted::Instance>,
    /// The Hole features for the plates' parts.
    drills: Vec<bolted::Drill>,
    pieces: Vec<Piece>,
    record: BoltedConnection,
    bolt_length: Real,
    /// The lengths of the bolt's family that work for this grip.
    lengths: Vec<Real>,
    /// Each head-side seat above the start plane, each nut-side seat below
    /// the termination, where the nut side ends.
    head_seats: Vec<Real>,
    nut_seats: Vec<Real>,
    nut_end: Real,
}

/// A washer's or a nut's thickness along its axis, from its table.
fn table_thickness(c: &Catalog, f: usize, m: usize) -> Option<Real> {
    let Source::Sample(s) = &c.families.get(f)?.members.get(m)?.source else {
        return None;
    };
    Some(match s.kind {
        Kind::Washer(_, _, h)
        | Kind::HexNut(_, _, h)
        | Kind::CapNut(_, _, h)
        | Kind::SquareNut(_, _, h)
        | Kind::FlangeNut(_, _, h, _)
        | Kind::SplitRing(_, _, h, _)
        | Kind::SquareWasher(_, _, _, h)
        | Kind::WingNut(_, _, _, h, _, _) => h,
        Kind::DiscSpring(_, _, t, h0) => t + h0,
        _ => return None,
    })
}

/// The member of a washer or nut family that goes over a `major` bolt.
fn over_member(c: &Catalog, f: usize, major: Real) -> Option<usize> {
    let t = Target {
        hole: false,
        diameter: major,
        thread: None,
        depth: 0.0,
        blind: false,
    };
    fit::choose(&c.fits(f), &t)
}

/// A bolt family's members of diameter `major`: (member, length, sink).
fn bolt_members(c: &Catalog, f: usize, major: Real) -> Vec<(usize, Real, Real)> {
    c.fits(f)
        .iter()
        .enumerate()
        .filter_map(|(k, fit)| match *fit {
            Fit::Into {
                d,
                length: Some(l),
                sink,
            } if (d - major).abs() < 0.01 => Some((k, l, sink)),
            _ => None,
        })
        .collect()
}

/// The label of a member in the browser (`ISO 4017 M8 x 30`).
fn member_name(c: &Catalog, f: usize, m: usize) -> String {
    c.families
        .get(f)
        .and_then(|fam| Some(format!("{} {}", fam.name, fam.members.get(m)?.size)))
        .unwrap_or_default()
}

impl CadCraftApp {
    /// OK of Bolted Connection: `true` when the connection was made.
    pub(super) fn bolted_ok(&mut self, d: &PartDialog) -> bool {
        // Check first: a plan that cannot be made changes nothing.
        let res = self.bolted_plan(d).and_then(|_| {
            self.checkpoint("Bolted Connection");
            let r = self.bolted_commit(d);
            if r.is_err() {
                self.history.cancel();
            }
            r
        });
        match res {
            Ok(msg) => {
                self.file_status = Some(Ok(msg));
                self.scene_dirty = true;
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Bolted Connection: {e}")));
                false
            }
        }
    }

    /// Makes the connection of panel `d` (Edit: the old one goes first and
    /// the plates are found again without its holes). No Undo step; on
    /// failure the project is put back.
    fn bolted_commit(&mut self, d: &PartDialog) -> Result<String, String> {
        let before = self.project.clone();
        let saved = self.part_planned.clone();
        let res = (|| {
            let old = d.bolt.editing.and_then(|id| {
                self.assembly()?
                    .bolts
                    .iter()
                    .find(|b| b.id == id)
                    .map(|b| (b.id, b.name.clone()))
            });
            let d = match d.bolt.editing {
                Some(id) => {
                    self.bolt_remove(id)?;
                    let mut fresh = d.clone();
                    fresh.bolt.key.clear();
                    fresh.bolt.view_key.clear();
                    fresh.bolt.editing = None;
                    self.part_planned = Some(fresh);
                    self.bolt_follow();
                    self.part_planned.clone().ok_or("the panel closed")?
                }
                None => d.clone(),
            };
            let mut plan = self.bolted_plan(&d)?;
            if let Some((id, name)) = old {
                plan.record.id = id;
                plan.record.name = name;
            }
            self.bolted_apply(plan)
        })();
        self.part_planned = saved;
        if res.is_err() {
            self.project = before;
        }
        res
    }

    /// Connection `id` made again from its own inputs (its holes, plates or
    /// references changed): the Edit panel's OK without the panel. No Undo
    /// step (it follows a change that has its own).
    pub(super) fn bolt_regenerate(&mut self, id: u32) -> Result<String, String> {
        let saved = self.part_planned.take();
        let status = self.file_status.clone();
        self.bolt_edit(id);
        let res = match self.part_planned.clone() {
            Some(mut d) => {
                d.bolt.key.clear();
                d.bolt.view_key.clear();
                self.part_planned = Some(d);
                self.bolt_follow();
                match self.part_planned.clone() {
                    Some(d) => self.bolted_commit(&d),
                    None => Err("the connection could not be opened".into()),
                }
            }
            None => Err("the connection no longer exists".into()),
        };
        self.part_planned = saved;
        self.file_status = status;
        res
    }

    /// A hash of what connection `b` is made from: its references'
    /// components (their geometry and places) and the parts it drilled.
    fn bolt_inputs_key(&self, b: &BoltedConnection) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let Some(a) = self.assembly() else {
            return 0;
        };
        let refs = std::iter::once(&b.start)
            .chain(&b.references)
            .chain(std::iter::once(&b.end));
        for g in refs {
            if let Some(o) = a.find(g.occurrence()) {
                format!("{:?}", o.transform).hash(&mut h);
                self.with_part(o.component, |p| p.body_signature())
                    .hash(&mut h);
            }
        }
        for (c, _) in &b.drilled {
            self.with_part(*c, |p| p.body_signature()).hash(&mut h);
        }
        h.finish()
    }

    /// Connections follow their holes and plates (spec 6.2 §4): one whose
    /// inputs changed since last seen (`all`: every one, Rebuild All) is made
    /// again. A connection seen for the first time is only remembered. The
    /// number made again.
    pub(super) fn bolts_follow(&mut self, all: bool) -> usize {
        let asm = self.part_id;
        let Some(list) = self.assembly().map(|a| a.bolts.clone()) else {
            return 0;
        };
        let mut made = 0;
        for b in &list {
            let key = self.bolt_inputs_key(b);
            let seen = self.bolt_keys.get(&(asm, b.id)).copied();
            if !all && seen.is_none_or(|k| k == key) {
                self.bolt_keys.insert((asm, b.id), key);
                continue;
            }
            match self.bolt_regenerate(b.id) {
                Ok(_) => {
                    made += 1;
                    // Its inputs as they are now (with its new holes).
                    if let Some(nb) = self
                        .assembly()
                        .and_then(|a| a.bolts.iter().find(|x| x.id == b.id).cloned())
                    {
                        let k = self.bolt_inputs_key(&nb);
                        self.bolt_keys.insert((asm, nb.id), k);
                    }
                }
                Err(e) => {
                    self.bolt_keys.insert((asm, b.id), key);
                    log::warn!("{}: {e}", b.name);
                    self.file_status =
                        Some(Err(format!("{}: cannot follow its holes ({e})", b.name)));
                }
            }
        }
        if made > 0 {
            self.scene_dirty = true;
        }
        made
    }

    /// What the preview shows of the plan: each member's file, side and
    /// seat, and the panel's numbers.
    pub(super) fn bolted_plan_pub(
        &self,
        d: &PartDialog,
    ) -> Result<(Vec<GhostAt>, panel::View), String> {
        let plan = self.bolted_plan(d)?;
        let ghosts = plan
            .pieces
            .iter()
            .map(|p| (p.file.clone(), p.nut_side, p.offset))
            .collect();
        Ok((
            ghosts,
            panel::View {
                ghosts: Vec::new(),
                length: plan.bolt_length,
                lengths: plan.lengths,
                bolt: plan
                    .pieces
                    .first()
                    .map_or_else(String::new, |p| p.name.clone()),
                head_seats: plan.head_seats,
                nut_seats: plan.nut_seats,
                nut_end: plan.nut_end,
            },
        ))
    }

    /// The plan of the panel's connection, or why there is none (nothing changes).
    fn bolted_plan(&self, d: &PartDialog) -> Result<Plan, String> {
        let axes = match &d.bolt.axes {
            Some(Ok(a)) if !a.is_empty() => a.clone(),
            Some(Err(e)) => return Err(e.clone()),
            _ => return Err("pick the start plane, the reference and the termination".into()),
        };
        let axis = axes[0];
        if let Some(p) = panel::stack_problems(d).into_iter().next() {
            return Err(p);
        }
        if d.bolt.head.is_empty() {
            return Err("Head side: add a bolt".into());
        }
        let kind = panel::kind(d);
        let catalog = d
            .bolt
            .catalog
            .clone()
            .ok_or("the Component Library could not be read")?;
        let (size, h) = panel::holes(d).ok_or("there is no clearance hole for this size")?;
        let thread = panel::choice(d, "Thread").unwrap_or_default();
        // Each bolt's plates: the clearance hole cuts those between the start
        // plane and the termination (not one already drilled there), a blind
        // one's tapped hole the plate at the termination.
        let mut cuts: Vec<(Paths, Paths)> = Vec::new();
        // The plates each bolt drills: (clearance rows, the tapped row).
        let mut rows: Vec<(Vec<panel::PlateRow>, Option<panel::PlateRow>)> = Vec::new();
        let mut last_t = Real::INFINITY;
        let grip_of = |plates: &[panel::PlateRow]| {
            plates
                .iter()
                .filter(|p| p.from < axis.length - ON)
                .map(|p| p.from + p.thickness)
                .fold(0.0, Real::max)
        };
        let first_grip = d.bolt.plates.first().map_or(0.0, |p| grip_of(p));
        for (i, plates) in d.bolt.plates.iter().enumerate() {
            let clearance: Vec<&panel::PlateRow> = plates
                .iter()
                .filter(|p| p.from < axis.length - ON)
                .collect();
            if clearance.is_empty() {
                return Err(if i == 0 {
                    "no component between the start plane and the termination".into()
                } else {
                    format!(
                        "bolt {}: no component between the start plane and the termination",
                        i + 1
                    )
                });
            }
            let grip = grip_of(plates);
            if (grip - axis.length).abs() > ON {
                return Err(format!(
                    "the bolts would go through different thicknesses ({:.2} mm and {grip:.2} mm): make two connections",
                    if i == 0 { axis.length } else { first_grip }
                ));
            }
            let last = match kind {
                BoltKind::ThroughAll => None,
                BoltKind::Blind => Some(
                    plates
                        .iter()
                        .find(|p| (p.from - axis.length).abs() <= ON)
                        .ok_or("a blind connection needs a component at the blind start plane")?,
                ),
            };
            if let Some(l) = last {
                last_t = last_t.min(l.thickness);
            }
            rows.push((
                clearance
                    .iter()
                    .filter(|p| p.solid)
                    .map(|p| (*p).clone())
                    .collect(),
                last.filter(|p| p.solid).cloned(),
            ));
            cuts.push((
                clearance
                    .iter()
                    .filter(|p| p.solid)
                    .map(|p| p.path.clone())
                    .collect(),
                last.filter(|p| p.solid)
                    .map(|p| vec![p.path.clone()])
                    .unwrap_or_default(),
            ));
        }
        if cuts.len() != axes.len() {
            return Err("the plates are still being found".into());
        }
        if !last_t.is_finite() {
            last_t = 0.0;
        }
        let head = &d.bolt.head;
        let nut: &[StackItem] = if kind == BoltKind::Blind {
            &[]
        } else {
            &d.bolt.nut
        };
        let Some(bolt) = head.first().filter(|x| x.role == Role::Bolt).cloned() else {
            return Err("Head side: add a bolt".into());
        };
        // Washers and nuts: their member and thickness.
        let mut over = Vec::new();
        for (nut_side, item) in head
            .iter()
            .skip(1)
            .map(|x| (false, x))
            .chain(nut.iter().map(|x| (true, x)))
        {
            let name = catalog
                .families
                .get(item.family)
                .map_or("", |f| f.name.as_str());
            let m = over_member(&catalog, item.family, h.major)
                .ok_or_else(|| format!("{name} has no {size}"))?;
            let file = catalog.member_file_at(item.family, m, Detail::Standard)?;
            let (doc, t) = self.bolt_member_doc(&file)?;
            let t = table_thickness(&catalog, item.family, m)
                .or(t)
                .ok_or_else(|| format!("{name}: its thickness is not known"))?;
            over.push((nut_side, item.clone(), m, t, file, doc));
        }
        let head_t: Vec<Real> = std::iter::once(0.0)
            .chain(over.iter().filter(|o| !o.0).map(|o| o.3))
            .collect();
        let nut_t: Vec<Real> = over.iter().filter(|o| o.0).map(|o| o.3).collect();
        let bolts = bolt_members(&catalog, bolt.family, h.major);
        let sink = bolts.first().map_or(0.0, |b| b.2);
        let lay = bolted::layout(kind, axis.length, last_t, h.major, &head_t, sink, &nut_t);
        let lengths: Vec<Real> = bolts.iter().map(|b| b.1).collect();
        let mut fitting: Vec<Real> = lengths
            .iter()
            .copied()
            .filter(|l| {
                *l >= lay.min_length - 1e-6 && lay.max_length.is_none_or(|m| *l <= m + 1e-6)
            })
            .collect();
        fitting.sort_by(Real::total_cmp);
        fitting.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        let bolt_name = catalog
            .families
            .get(bolt.family)
            .map_or_else(String::new, |f| f.name.clone());
        let chosen = d
            .bolt
            .length
            .and_then(|want| lengths.iter().position(|l| (l - want).abs() < 1e-6))
            .filter(|k| fitting.iter().any(|l| (l - lengths[*k]).abs() < 1e-6));
        let Some(k) =
            chosen.or_else(|| bolted::choose_length(&lengths, lay.min_length, lay.max_length))
        else {
            return Err(match lay.max_length {
                Some(max) => format!(
                    "no {bolt_name} {size} from {:.1} to {:.1} mm long",
                    lay.min_length, max
                ),
                None => format!(
                    "no {bolt_name} {size} of length {:.1} mm or more",
                    lay.min_length
                ),
            });
        };
        let (bolt_member, bolt_length, _) = bolts[k];
        let file = catalog.member_file_at(bolt.family, bolt_member, Detail::Standard)?;
        let (doc, _) = self.bolt_member_doc(&file)?;
        let mut pieces = vec![Piece {
            item: bolt.clone(),
            nut_side: false,
            offset: lay.head[0],
            file,
            doc,
            name: member_name(&catalog, bolt.family, bolt_member),
        }];
        let (mut hk, mut nk) = (1, 0);
        for (nut_side, item, m, _, file, doc) in over {
            let offset = if nut_side {
                nk += 1;
                lay.nut[nk - 1]
            } else {
                hk += 1;
                lay.head[hk - 1]
            };
            pieces.push(Piece {
                name: member_name(&catalog, item.family, m),
                item,
                nut_side,
                offset,
                file,
                doc,
            });
        }
        let depth = bolted::tapped_depth(bolt_length, &lay, axis.length, h.major, last_t);
        // Each bolt's axis body in the tools part cuts nothing: the holes go
        // into the plates' own parts (owner 2026-11-07), one Hole feature per
        // part and face with a centre per bolt.
        let instances: Vec<bolted::Instance> = axes
            .iter()
            .map(|a| {
                let mut ops = bolted::hole_ops(a, BoltKind::ThroughAll, &h, 0.0);
                ops.truncate(1);
                bolted::Instance::new(*a, ops, &[], &[])
            })
            .collect();
        let tapped = bolted::tapped_spec(Standards::global(), &thread, &size, depth);
        let mut drills: Vec<bolted::Drill> = Vec::new();
        let mut add = |r: &panel::PlateRow, tap: bool| -> Result<(), String> {
            let face = r
                .face
                .clone()
                .ok_or_else(|| format!("{}: the face the bolt enters by has no name", r.name))?;
            let spec = if tap {
                tapped
                    .clone()
                    .ok_or_else(|| format!("no tapped hole for {size}"))?
            } else {
                bolted::clearance_spec(h.clearance)
            };
            match drills
                .iter_mut()
                .find(|x| x.component == r.component && x.face == face && x.spec == spec)
            {
                Some(x) => {
                    if !x.centers.iter().any(|c| (c - r.local).norm() < 1e-6) {
                        x.centers.push(r.local);
                    }
                }
                None => drills.push(bolted::Drill {
                    component: r.component,
                    face,
                    centers: vec![r.local],
                    spec,
                }),
            }
            Ok(())
        };
        for (clearance, last) in &rows {
            for r in clearance {
                add(r, false)?;
            }
            if let Some(r) = last {
                add(r, true)?;
            }
        }
        let _ = &cuts;
        let placement = panel::placement(d);
        let references: Vec<GeomRef> = panel::reference_keys(placement)
            .iter()
            .filter_map(|k| d.refs.get(k))
            .flatten()
            .cloned()
            .collect();
        let start = d
            .refs
            .get("start")
            .and_then(|v| v.first())
            .cloned()
            .ok_or("pick the start plane")?;
        let end = d
            .refs
            .get(panel::end_key(d))
            .and_then(|v| v.first())
            .cloned()
            .ok_or("pick the termination")?;
        let family_of = |f: usize| catalog.families.get(f);
        let record = BoltedConnection {
            id: 0,
            name: String::new(),
            kind,
            placement,
            start,
            references,
            distances: [
                d.num(0, "Distance 1").unwrap_or(10.0),
                d.num(0, "Distance 2").unwrap_or(10.0),
            ],
            spread: panel::spread(d),
            fit: panel::fit(d),
            length: d.bolt.length,
            end,
            end_auto: d.bolt.auto_end,
            thread,
            size,
            fasteners: pieces
                .iter()
                .map(|p| BoltFastener {
                    occurrence: 0,
                    instance: 0,
                    nut_side: p.nut_side,
                    role: p.item.role,
                    family: family_of(p.item.family).map_or_else(String::new, |f| f.name.clone()),
                    standard: family_of(p.item.family)
                        .map_or_else(String::new, |f| f.standard.clone()),
                })
                .collect(),
            holes: Vec::new(),
            drilled: Vec::new(),
        };
        Ok(Plan {
            kind,
            instances,
            drills,
            pieces,
            record,
            bolt_length,
            lengths: fitting,
            nut_end: nut_t.iter().sum(),
            head_seats: lay.head,
            nut_seats: lay.nut,
        })
    }

    /// The part document of a member file and its height along Z (from its
    /// body, for members without a table).
    fn bolt_member_doc(
        &self,
        file: &std::path::Path,
    ) -> Result<(DocumentId, Option<Real>), String> {
        let p = self.part_file_pub(file)?;
        p.documents()
            .iter()
            .find_map(|d| match &d.data {
                DocumentData::Part(part) => {
                    Some((d.id, part.body.bounding_box().map(|(lo, hi)| hi.z - lo.z)))
                }
                _ => None,
            })
            .ok_or_else(|| "the member's file holds no part".to_owned())
    }

    /// Makes the connection of `plan` (replacing connection `editing`).
    fn bolted_apply(&mut self, plan: Plan) -> Result<String, String> {
        let asm = self.part_id;
        // The holes in linked part files: their edits (saved with this file).
        let (local, linked): (Vec<_>, Vec<_>) = plan
            .drills
            .iter()
            .cloned()
            .partition(|d| d.component.library.is_none());
        let mut drilled = Vec::new();
        for d in &linked {
            let (Some(lib), Some(mut part)) = (
                d.component.library,
                self.with_part(d.component, |p| p.clone()),
            ) else {
                return Err("a plate's part file cannot be read".into());
            };
            let id =
                bolted::drill(&mut part, d).map_err(|e| format!("the hole in the plate: {e}"))?;
            self.edit_linked_part(lib, d.component.document, part);
            drilled.push((d.component, id));
        }
        let mut record = plan.record.clone();
        record.drilled = drilled;
        // The member files, linked.
        let owner = PathBuf::from(self.file_path.trim());
        let mut resolver = crate::document::Resolver::new();
        let mut fasteners = Vec::new();
        for p in &plan.pieces {
            let component = resolver
                .link(&mut self.project, &owner, &p.file, p.doc)
                .map_err(|e| e.to_string())?;
            fasteners.push(bolted::Placing {
                component,
                name: p.name.clone(),
                nut_side: p.nut_side,
                offset: p.offset,
            });
        }
        let plane = |g: Option<Geom>| match g {
            Some(Geom::Plane { p, n }) => Ok((p, n)),
            _ => Err("the start plane or the termination is no plane".to_owned()),
        };
        let n = plan.instances.len();
        let grip = plan.instances.first().map_or(0.0, |i| i.axis.length);
        let making = bolted::Making {
            instances: plan.instances.clone(),
            drills: local,
            start_plane: plane(self.bolt_world(&plan.record.start))?,
            end_plane: plane(self.bolt_world(&plan.record.end))?,
            fasteners,
            record,
        };
        let id = bolted::make(&mut self.project, asm, making)?;
        let mut a = self.assembly().ok_or("no assembly")?.clone();
        let s = self.solve_copy(&a, None);
        if !s.converged {
            return Err(
                "the fasteners cannot be held on the axis and their seats (nothing was changed)"
                    .into(),
            );
        }
        crate::assembly::flexible::apply(&s, &mut a);
        if let Err(e) = self.place_patterns(&mut a) {
            log::warn!("{e}");
        }
        let placed: Vec<u32> = a
            .bolts
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.fasteners.iter().map(|f| f.occurrence).collect())
            .unwrap_or_default();
        if let Some(cur) = self.assembly_mut() {
            *cur = a;
        }
        self.cons.last = s;
        self.asm.sel = placed;
        self.context_follow();
        Ok(format!(
            "Bolted Connection {id}: {}{}{}, grip {:.2} mm",
            if n > 1 {
                format!("{n} x ")
            } else {
                String::new()
            },
            plan.pieces.first().map_or("", |p| p.name.as_str()),
            if plan.kind == BoltKind::Blind {
                " into a tapped hole"
            } else {
                " with nut"
            },
            grip
        ))
    }

    /// Removes connection `id` from the active assembly: its fasteners
    /// (with their relationships), its holes and its record. No checkpoint.
    pub(super) fn bolt_remove(&mut self, id: u32) -> Result<(), String> {
        // The holes in linked part files first (this file's edits of them).
        let linked: Vec<(crate::document::DocRef, String)> = self
            .assembly()
            .and_then(|a| a.bolts.iter().find(|b| b.id == id))
            .map(|b| {
                b.drilled
                    .iter()
                    .filter(|(c, _)| c.library.is_some())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        for (c, hole) in linked {
            if let (Some(lib), Some(mut part)) = (c.library, self.with_part(c, |p| p.clone())) {
                bolted::undrill(&mut part, &hole)?;
                self.edit_linked_part(lib, c.document, part);
            }
        }
        bolted::remove(&mut self.project, self.part_id, id)?;
        self.scene_dirty = true;
        Ok(())
    }

    /// Delete of a connection (browser): one Undo step.
    pub(super) fn bolt_delete(&mut self, id: u32) {
        self.checkpoint("Delete Bolted Connection");
        let before = self.project.clone();
        match self.bolt_remove(id) {
            Ok(()) => self.file_status = Some(Ok("Bolted Connection deleted".into())),
            Err(e) => {
                self.project = before;
                self.history.cancel();
                self.file_status = Some(Err(format!("Delete Bolted Connection: {e}")));
            }
        }
    }

    /// Edit of a connection: the panel opens on its stored inputs; OK replaces it.
    pub(super) fn bolt_edit(&mut self, id: u32) {
        let Some(b) = self
            .assembly()
            .and_then(|a| a.bolts.iter().find(|b| b.id == id).cloned())
        else {
            return;
        };
        self.start_asm_planned(super::asm_planned::AsmPlanned::BoltedConnection);
        let labels: Vec<(GeomRef, String)> = std::iter::once(&b.start)
            .chain(&b.references)
            .chain(std::iter::once(&b.end))
            .map(|g| (g.clone(), self.geom_label(g)))
            .collect();
        let Some(d) = self.part_planned.as_mut().filter(|d| panel::is_bolted(d)) else {
            return;
        };
        d.set_index(0, "Type", usize::from(b.kind == BoltKind::Blind));
        d.set_index(0, "Placement", b.placement.index());
        if let Some(i) =
            panel::combo(d, 0, "Thread").and_then(|v| v.iter().position(|s| *s == b.thread))
        {
            d.set_index(0, "Thread", i);
        }
        if let Some(i) =
            panel::combo(d, 0, "Diameter").and_then(|v| v.iter().position(|s| *s == b.size))
        {
            d.set_index(0, "Diameter", i);
        }
        d.set_num(0, "Distance 1", b.distances[0]);
        d.set_num(0, "Distance 2", b.distances[1]);
        match b.spread {
            Spread::One => {}
            Spread::Grid { count, spacing } => {
                d.set_num(0, "Count 1", f64::from(count[0]));
                d.set_num(0, "Count 2", f64::from(count[1]));
                d.set_num(0, "Spacing 1", spacing[0]);
                d.set_num(0, "Spacing 2", spacing[1]);
            }
            Spread::Circle {
                count,
                radius,
                angle,
                span,
            } => {
                d.set_num(0, "Count", f64::from(count));
                d.set_num(0, "Radius", radius);
                d.set_num(0, "Angle", angle.to_degrees());
                d.set_num(0, "Span", span.to_degrees());
            }
        }
        d.set_index(
            0,
            "Hole Fit",
            match b.fit {
                crate::part::hole::FastenerFit::Close => 0,
                crate::part::hole::FastenerFit::Normal => 1,
                crate::part::hole::FastenerFit::Loose => 2,
            },
        );
        d.bolt.length = b.length;
        let label = |g: &GeomRef| {
            labels
                .iter()
                .find(|(x, _)| x == g)
                .map_or_else(String::new, |(_, l)| l.clone())
        };
        d.set_ref("start", label(&b.start), b.start.clone());
        let keys = panel::reference_keys(b.placement);
        for (k, g) in b.references.iter().enumerate() {
            let key = keys.get(k).copied().unwrap_or(keys[keys.len() - 1]);
            if panel::multi(d, key) {
                d.add_ref(key, label(g), g.clone());
            } else {
                d.set_ref(key, label(g), g.clone());
            }
        }
        let end_key = panel::end_key(d);
        let end_label = if b.end_auto {
            format!("{} (auto)", label(&b.end))
        } else {
            label(&b.end)
        };
        d.set_ref(end_key, end_label, b.end.clone());
        d.bolt.auto_end = b.end_auto;
        let family = |name: &str| {
            d.bolt
                .catalog
                .as_ref()
                .and_then(|c| c.families.iter().position(|f| f.name == name))
        };
        let (mut head, mut nut) = (Vec::new(), Vec::new());
        for f in b.fasteners.iter().filter(|f| f.instance == 0) {
            if let Some(i) = family(&f.family) {
                let item = StackItem {
                    family: i,
                    role: f.role,
                };
                if f.nut_side {
                    nut.push(item);
                } else {
                    head.push(item);
                }
            }
        }
        d.bolt.head = head;
        d.bolt.nut = nut;
        d.bolt.editing = Some(id);
        d.active = None;
        self.file_status = Some(Ok(format!(
            "Edit {}: change the inputs, OK replaces it",
            b.name
        )));
    }

    /// The browser's Bolted Connections: each with its fasteners (Edit, Delete).
    pub(super) fn bolts_tree(&mut self, ui: &mut egui::Ui) {
        let Some(a) = self.assembly() else {
            return;
        };
        if a.bolts.is_empty() {
            return;
        }
        let rows: Vec<BoltRow> = a
            .bolts
            .iter()
            .map(|b| {
                (
                    b.id,
                    format!(
                        "{} ({} {})",
                        b.name,
                        b.size,
                        match b.kind {
                            BoltKind::ThroughAll => "through",
                            BoltKind::Blind => "blind",
                        }
                    ),
                    b.fasteners
                        .iter()
                        .filter_map(|f| Some((f.occurrence, a.find(f.occurrence)?.name.clone())))
                        .collect(),
                )
            })
            .collect();
        let (mut edit, mut delete, mut select) = (None, None, None);
        egui::CollapsingHeader::new("Bolted Connections")
            .id_salt("asm_bolts")
            .default_open(true)
            .show(ui, |ui| {
                for (id, title, fasteners) in &rows {
                    let r = egui::CollapsingHeader::new(title)
                        .id_salt(("asm_bolt", *id))
                        .show(ui, |ui| {
                            for (occ, name) in fasteners {
                                let sel = self.asm.sel.contains(occ);
                                let r = ui.selectable_label(sel, name);
                                if r.clicked() {
                                    select = Some(vec![*occ]);
                                }
                                r.context_menu(|ui| {
                                    if ui.button("Edit Bolted Connection").clicked() {
                                        edit = Some(*id);
                                        ui.close();
                                    }
                                    if ui.button("Delete Bolted Connection").clicked() {
                                        delete = Some(*id);
                                        ui.close();
                                    }
                                });
                            }
                        });
                    if r.header_response.clicked() {
                        select = Some(fasteners.iter().map(|f| f.0).collect());
                    }
                    r.header_response.context_menu(|ui| {
                        if ui.button("Edit Bolted Connection").clicked() {
                            edit = Some(*id);
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            delete = Some(*id);
                            ui.close();
                        }
                    });
                }
            });
        if let Some(s) = select {
            self.asm.sel = s;
            self.scene_dirty = true;
        }
        if let Some(id) = edit {
            self.bolt_edit(id);
        }
        if let Some(id) = delete {
            self.bolt_delete(id);
        }
    }

    /// The connection a fastener occurrence belongs to.
    pub(super) fn bolt_of(&self, occurrence: u32) -> Option<u32> {
        self.assembly()?
            .bolts
            .iter()
            .find(|b| b.fasteners.iter().any(|f| f.occurrence == occurrence))
            .map(|b| b.id)
    }
}
