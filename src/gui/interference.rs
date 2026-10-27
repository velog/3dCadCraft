//! Analyze Interference in the assembly (`docs/spec/3.0-assembly-commands.md`
//! §3.4): OK of the planned panel gathers the two sets, the overlap search
//! runs on a worker thread ([`crate::assembly::interference`]), and the
//! **Interference Detected** window lists every overlap (components, volume,
//! centroid) while the view shows them red. A row picked in the list is
//! drawn brighter and its two components are selected.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Instant;

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::part_planned::PartDialog;
use crate::assembly::interference::{self as itf, Body, Interference};

/// A finished analysis.
pub(super) struct Report {
    pub found: Vec<Interference>,
    /// Occurrence names by id (as when it ran).
    names: HashMap<u32, String>,
    /// Pairs checked.
    pairs: usize,
    secs: f64,
    /// The row picked in the list.
    pub picked: Option<usize>,
}

/// A running search: its answer, start, pair count and the names.
type Job = (
    mpsc::Receiver<Vec<Interference>>,
    Instant,
    usize,
    HashMap<u32, String>,
);

/// The command's state in the assembly UI.
#[derive(Default)]
pub(super) struct InterferenceUi {
    job: Option<Job>,
    /// The result window, while it is open.
    pub report: Option<Report>,
}

/// Red of the overlap volumes; the picked one is brighter.
const RED: [f32; 3] = [0.85, 0.08, 0.06];
const RED_PICKED: [f32; 3] = [1.0, 0.35, 0.25];

impl CadCraftApp {
    /// OK of Analyze Interference: starts the search over the sets of `d`;
    /// false (with the reason in the status bar) when set #1 is empty.
    pub(super) fn start_interference(&mut self, d: &PartDialog) -> bool {
        let Some(a) = self.assembly() else {
            return false;
        };
        let ids = |key: &str| -> Vec<u32> {
            d.picks_of(key)
                .iter()
                .filter_map(|w| {
                    let name = w.split(" · ").next().unwrap_or(w);
                    a.occurrences.iter().find(|o| o.name == name).map(|o| o.id)
                })
                .collect()
        };
        let (set1, set2) = (ids("set1"), ids("set2"));
        if set1.is_empty() {
            self.file_status = Some(Err(
                "Analyze Interference: select the components of set #1".into()
            ));
            return false;
        }
        let pairs = itf::pairs(&set1, &set2);
        // Each component's triangles once, then moved per occurrence.
        let mut local: HashMap<crate::document::DocRef, Vec<[crate::kernel::Point3; 3]>> =
            HashMap::new();
        let mut bodies = Vec::new();
        let mut names = HashMap::new();
        let used: Vec<u32> = pairs.iter().flat_map(|&(x, y)| [x, y]).collect();
        let occs: Vec<_> = a
            .occurrences
            .iter()
            .filter(|o| used.contains(&o.id) && !o.suppressed)
            .cloned()
            .collect();
        for o in &occs {
            names.insert(o.id, o.name.clone());
            if let std::collections::hash_map::Entry::Vacant(e) = local.entry(o.component) {
                e.insert(
                    self.with_part(o.component, itf::part_triangles)
                        .unwrap_or_default(),
                );
            }
            if let Some(b) = local.get(&o.component).and_then(|t| Body::of(o, t)) {
                bodies.push(b);
            }
        }
        let (tx, rx) = mpsc::channel();
        let n = pairs.len();
        std::thread::spawn(move || {
            let found = itf::analyze(&bodies, &pairs);
            let _ = tx.send(found);
        });
        self.asm.interference.job = Some((rx, Instant::now(), n, names));
        self.asm.interference.report = None;
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!("Analyze Interference: checking {n} pair(s)...")));
        true
    }

    /// Takes a finished search (called every frame in an assembly).
    pub(super) fn interference_poll(&mut self, ctx: &egui::Context) {
        let Some((rx, t0, pairs, _)) = &self.asm.interference.job else {
            return;
        };
        match rx.try_recv() {
            Ok(found) => {
                let secs = t0.elapsed().as_secs_f64();
                let pairs = *pairs;
                let names = self
                    .asm
                    .interference
                    .job
                    .take()
                    .map(|j| j.3)
                    .unwrap_or_default();
                log::info!(
                    "Analyze Interference: {} overlap(s) in {pairs} pair(s), {secs:.2} s",
                    found.len()
                );
                self.file_status = Some(Ok(if found.is_empty() {
                    "Analyze Interference: no interference detected".into()
                } else {
                    format!(
                        "Analyze Interference: {} interference(s) found",
                        found.len()
                    )
                }));
                self.asm.interference.report = Some(Report {
                    found,
                    names,
                    pairs,
                    secs,
                    picked: None,
                });
                self.scene_dirty = true;
            }
            Err(mpsc::TryRecvError::Empty) => ctx.request_repaint(),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.asm.interference.job = None;
                self.file_status = Some(Err(
                    "Analyze Interference: the search stopped unexpectedly".into(),
                ));
            }
        }
    }

    /// The overlap volumes in red (scene rebuild).
    pub(super) fn interference_scene(&self, g: &mut super::renderer::SceneGeometry) {
        let Some(r) = &self.asm.interference.report else {
            return;
        };
        for (k, i) in r.found.iter().enumerate() {
            let c = if r.picked == Some(k) { RED_PICKED } else { RED };
            let mut pos = Vec::with_capacity(i.tris.len() * 3);
            let mut nor = Vec::with_capacity(i.tris.len() * 3);
            let mut tri = Vec::with_capacity(i.tris.len());
            for t in &i.tris {
                let n = (t[1] - t[0])
                    .cross(&(t[2] - t[0]))
                    .try_normalize(1e-15)
                    .unwrap_or_else(crate::kernel::Vector3::z);
                let b = pos.len() as u32;
                for p in t {
                    pos.push(*p);
                    nor.push(n);
                }
                tri.push([b, b + 1, b + 2]);
            }
            let colors = vec![c; pos.len()];
            g.add_mesh_colored(&pos, &nor, &tri, &colors);
        }
    }

    /// The Interference Detected window (and "no interference").
    pub(super) fn interference_window(&mut self, ctx: &egui::Context) {
        let Some(mut r) = self.asm.interference.report.take() else {
            return;
        };
        let mut open = true;
        let mut pick: Option<usize> = None;
        let title = if r.found.is_empty() {
            "Analyze Interference"
        } else {
            "Interference Detected"
        };
        let win = egui::Window::new(title)
            .id(egui::Id::new("interference_window"))
            .open(&mut open)
            .resizable(true)
            .default_width(560.0)
            // Over the view's lower half on the first show (later: where
            // the user left it).
            .pivot(egui::Align2::LEFT_TOP)
            .default_pos(ctx.content_rect().center() + egui::vec2(-280.0, 120.0));
        let name = |id: u32| {
            r.names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| format!("#{id}"))
        };
        let mut copy = None;
        self.layout
            .place("interference", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                if r.found.is_empty() {
                    ui.label(RichText::new("No interference detected").strong());
                    ui.label(
                        RichText::new(format!(
                            "{} pair(s) of components checked in {:.2} s",
                            r.pairs, r.secs
                        ))
                        .weak(),
                    );
                    return;
                }
                let total: f64 = r.found.iter().map(|i| i.volume).sum();
                ui.label(format!(
                    "{} interference(s) found, total volume {:.3} mm³ ({} pair(s) checked, {:.2} s)",
                    r.found.len(),
                    total,
                    r.pairs,
                    r.secs
                ));
                ui.separator();
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    egui::Grid::new("interference_rows")
                        .striped(true)
                        .num_columns(5)
                        .show(ui, |ui| {
                            for h in ["#", "Component 1", "Component 2", "Volume", "Centroid"] {
                                ui.label(RichText::new(h).strong());
                            }
                            ui.end_row();
                            for (k, i) in r.found.iter().enumerate() {
                                let on = r.picked == Some(k);
                                if ui.selectable_label(on, format!("{}", k + 1)).clicked() {
                                    pick = Some(k);
                                }
                                ui.label(name(i.a));
                                ui.label(name(i.b));
                                ui.label(format!("{:.3} mm³", i.volume));
                                let c = i.centroid;
                                ui.label(format!("{:.3}, {:.3}, {:.3} mm", c.x, c.y, c.z));
                                ui.end_row();
                            }
                        });
                });
                ui.separator();
                if ui.button("Copy to Clipboard").clicked() {
                    let mut t = String::from("Component 1\tComponent 2\tVolume (mm^3)\tCentroid X\tCentroid Y\tCentroid Z\n");
                    for i in &r.found {
                        t += &format!(
                            "{}\t{}\t{:.6}\t{:.6}\t{:.6}\t{:.6}\n",
                            name(i.a),
                            name(i.b),
                            i.volume,
                            i.centroid.x,
                            i.centroid.y,
                            i.centroid.z
                        );
                    }
                    copy = Some(t);
                }
            });
        if let Some(t) = copy {
            ctx.copy_text(t);
            self.file_status = Some(Ok(
                "Analyze Interference: the list is on the clipboard".into()
            ));
        }
        if let Some(k) = pick {
            r.picked = if r.picked == Some(k) { None } else { Some(k) };
            if let Some(i) = r.picked.and_then(|k| r.found.get(k)) {
                self.asm.sel = vec![i.a, i.b];
            }
            self.scene_dirty = true;
        }
        if open {
            self.asm.interference.report = Some(r);
        } else {
            // Closing the window removes the red volumes.
            self.scene_dirty = true;
        }
    }
}
