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
use crate::assembly::bolted::{self, BoltFastener, BoltKind, BoltedConnection, HoleUse, Placement};
use crate::assembly::relation::{Geom, GeomRef};
use crate::components::catalog::{Catalog, Source};
use crate::components::fit::{self, Fit, Target};
use crate::components::samples::{Detail, Kind};
use crate::document::{DocumentData, DocumentId};
use crate::kernel::Real;

/// Plates closer than this to a plane are on it (mm).
const ON: Real = 0.05;

/// A connection in the browser: id, title, its fasteners (occurrence, name).
type BoltRow = (u32, String, Vec<(u32, String)>);

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
    axis: bolted::Axis,
    kind: BoltKind,
    holes: Vec<(crate::part::bolt_hole::BoltHoleOp, HoleUse)>,
    /// Occurrence paths cut by the clearance hole and by the tapped one.
    clearance: Vec<Vec<u32>>,
    tapped: Vec<Vec<u32>>,
    pieces: Vec<Piece>,
    record: BoltedConnection,
    bolt_length: Real,
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
        let res = self.bolted_plan(d).and_then(|plan| {
            self.checkpoint("Bolted Connection");
            let before = self.project.clone();
            match self.bolted_apply(plan, d.bolt.editing) {
                Ok(msg) => Ok(msg),
                Err(e) => {
                    self.project = before;
                    self.history.cancel();
                    Err(e)
                }
            }
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

    /// The plan of the panel's connection, or why there is none (nothing changes).
    fn bolted_plan(&mut self, d: &PartDialog) -> Result<Plan, String> {
        let axis = match &d.bolt.axis {
            Some(Ok(a)) => *a,
            Some(Err(e)) => return Err(e.clone()),
            None => return Err("pick the start plane, the reference and the termination".into()),
        };
        if let Some(p) = panel::stack_problems(d).into_iter().next() {
            return Err(p);
        }
        let kind = panel::kind(d);
        let catalog = d
            .bolt
            .catalog
            .clone()
            .ok_or("the Component Library could not be read")?;
        let (size, h) = panel::holes(d).ok_or("there is no clearance hole for this size")?;
        let thread = panel::choice(d, "Thread").unwrap_or_default();
        // The plates the clearance hole goes through, and a blind one's last.
        let clearance: Vec<Vec<u32>> = d
            .bolt
            .plates
            .iter()
            .filter(|p| p.from < axis.length - ON)
            .map(|p| p.path.clone())
            .collect();
        if clearance.is_empty() {
            return Err("no component between the start plane and the termination".into());
        }
        let last = match kind {
            BoltKind::ThroughAll => None,
            BoltKind::Blind => Some(
                d.bolt
                    .plates
                    .iter()
                    .find(|p| (p.from - axis.length).abs() <= ON)
                    .ok_or("a blind connection needs a component at the blind start plane")?,
            ),
        };
        let last_t = last.map_or(0.0, |p| p.thickness);
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
        let bolt_name = catalog
            .families
            .get(bolt.family)
            .map_or_else(String::new, |f| f.name.clone());
        let Some(k) = bolted::choose_length(&lengths, lay.min_length, lay.max_length) else {
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
        let holes = bolted::hole_ops(&axis, kind, &h, depth);
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
            end,
            end_auto: d.bolt.auto_end,
            thread,
            size,
            fasteners: pieces
                .iter()
                .map(|p| BoltFastener {
                    occurrence: 0,
                    nut_side: p.nut_side,
                    role: p.item.role,
                    family: family_of(p.item.family).map_or_else(String::new, |f| f.name.clone()),
                    standard: family_of(p.item.family)
                        .map_or_else(String::new, |f| f.standard.clone()),
                })
                .collect(),
            holes: Vec::new(),
        };
        Ok(Plan {
            axis,
            kind,
            holes,
            clearance,
            tapped: last.map(|p| vec![p.path.clone()]).unwrap_or_default(),
            pieces,
            record,
            bolt_length,
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
    fn bolted_apply(&mut self, plan: Plan, editing: Option<u32>) -> Result<String, String> {
        let asm = self.part_id;
        if let Some(id) = editing {
            self.bolt_remove(id)?;
        }
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
        let making = bolted::Making {
            axis: plan.axis,
            holes: plan.holes.clone(),
            clearance: plan.clearance.clone(),
            tapped: plan.tapped.clone(),
            start_plane: plane(self.bolt_world(&plan.record.start))?,
            end_plane: plane(self.bolt_world(&plan.record.end))?,
            fasteners,
            record: plan.record.clone(),
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
            "Bolted Connection {id}: {} {:.0} mm{}, grip {:.2} mm",
            plan.pieces.first().map_or("", |p| p.name.as_str()),
            plan.bolt_length,
            if plan.kind == BoltKind::Blind {
                " into a tapped hole"
            } else {
                " with nut"
            },
            plan.axis.length
        ))
    }

    /// Removes connection `id` from the active assembly: its fasteners
    /// (with their relationships), its holes and its record. No checkpoint.
    pub(super) fn bolt_remove(&mut self, id: u32) -> Result<(), String> {
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
            if b.placement == Placement::OnPoint {
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
        for f in &b.fasteners {
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
