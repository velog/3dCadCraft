//! The browser's Files tab (spec 0.6b §3.3): the documents of the open design
//! file, by kind, and the folder it is in, instead of a row of tabs above the
//! view that a file with many parts overflows.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};

use super::documents::{DocCmd, doc_menu, kind_icon};
use super::icons::Icon;
use super::panels::tree_icon;
use crate::document::{DocumentId, DocumentKind};

/// Extensions the Folder list shows.
const OPENED: &[&str] = &[
    "3dcadcraft",
    "ccpart",
    "ccasm",
    "ccdrw",
    "ccpres",
    "ccproj",
    "step",
    "stp",
    "stl",
    "obj",
];

/// How often a shown folder is read again.
const REFRESH: Duration = Duration::from_secs(3);

/// Which side of the browser panel shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrowserTab {
    /// The active document's tree.
    #[default]
    Model,
    /// This file's documents and the folder.
    Files,
}

/// One entry of a listed folder.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    path: PathBuf,
    name: String,
    folder: bool,
}

/// The Files tab's state.
#[derive(Debug, Default)]
pub struct FilesUi {
    /// Which tab shows.
    pub tab: BrowserTab,
    /// The folder shown (`None`: the open file's own).
    folder: Option<PathBuf>,
    /// What it holds, read at most every [`REFRESH`] (rule 20: never per frame).
    listed: Option<(PathBuf, Instant, Vec<Entry>)>,
}

/// The folder's subfolders and openable files, folders first, by name.
fn read_folder(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let folder = e.file_type().ok()?.is_dir();
            let known = path
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| OPENED.contains(&x.to_ascii_lowercase().as_str()));
            (folder || known).then_some(Entry { path, name, folder })
        })
        .collect();
    v.sort_by(|a, b| {
        b.folder
            .cmp(&a.folder)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    v
}

/// The icon of a file by its extension.
fn file_icon(path: &Path) -> Icon {
    match path
        .extension()
        .and_then(|x| x.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("ccasm") => Icon::Assembly,
        Some("ccdrw") => Icon::Drawing,
        Some("3dcadcraft" | "ccproj") => Icon::Project,
        _ => Icon::Part,
    }
}

impl super::CadCraftApp {
    /// The tab row at the top of the browser panel.
    pub(super) fn browser_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (tab, label, tip) in [
                (BrowserTab::Model, "Model", "The active document's tree"),
                (
                    BrowserTab::Files,
                    "Files",
                    "The documents of this file and the folder it is in",
                ),
            ] {
                let on = self.files.tab == tab;
                let text = if on {
                    RichText::new(label).strong()
                } else {
                    RichText::new(label)
                };
                if ui.selectable_label(on, text).on_hover_text(tip).clicked() {
                    self.files.tab = tab;
                }
            }
        });
    }

    /// The Files tab.
    pub(super) fn files_tab(&mut self, ui: &mut egui::Ui) {
        let mut open = None;
        let mut ask: Option<(DocCmd, DocumentId)> = None;
        let mut open_file: Option<PathBuf> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    tree_icon(ui, Icon::Project);
                    ui.label(RichText::new(self.project.name.clone()).strong());
                });
                let docs: Vec<(DocumentId, String, DocumentKind)> = self
                    .project
                    .documents()
                    .iter()
                    .map(|d| (d.id, d.name.clone(), d.data.kind()))
                    .collect();
                for (kind, title) in [
                    (DocumentKind::Assembly, "Assemblies"),
                    (DocumentKind::Part, "Parts"),
                    (DocumentKind::Drawing, "Drawings"),
                ] {
                    let of: Vec<_> = docs.iter().filter(|d| d.2 == kind).collect();
                    if of.is_empty() {
                        continue;
                    }
                    egui::CollapsingHeader::new(format!("{title} ({})", of.len()))
                        .id_salt(("files_kind", title))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (id, name, kind) in of {
                                ui.horizontal(|ui| {
                                    tree_icon(ui, kind_icon(*kind));
                                    let on = *id == self.part_id;
                                    let r = ui
                                        .selectable_label(on, name)
                                        .on_hover_text("Double-click to work on it");
                                    if r.double_clicked() && !on {
                                        open = Some(*id);
                                    }
                                    r.context_menu(|ui| doc_menu(ui, *id, &mut ask, &mut open));
                                });
                            }
                        });
                }
                ui.separator();
                self.folder_list(ui, &mut open_file);
            });
        if let Some(id) = open {
            self.asm.came_from = None;
            self.activate_document(id);
        }
        if let Some((c, id)) = ask {
            self.docs_apply_to(c, id);
        }
        if let Some(p) = open_file {
            self.open_path(&p);
        }
    }

    /// The folder's list: up, subfolders, files.
    fn folder_list(&mut self, ui: &mut egui::Ui, open_file: &mut Option<PathBuf>) {
        let own = Path::new(&self.file_path).parent().map(Path::to_path_buf);
        let Some(dir) = self
            .files
            .folder
            .clone()
            .or(own.filter(|p| !p.as_os_str().is_empty()))
            .or_else(super::files::home_dir)
        else {
            return;
        };
        let now = Instant::now();
        let stale = !matches!(&self.files.listed,
            Some((d, at, _)) if *d == dir && now.duration_since(*at) < REFRESH);
        if stale {
            self.files.listed = Some((dir.clone(), now, read_folder(&dir)));
        }
        let name = dir.file_name().map_or_else(
            || dir.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        ui.horizontal(|ui| {
            if ui
                .small_button("Up")
                .on_hover_text("The parent folder")
                .clicked()
                && let Some(p) = dir.parent()
            {
                self.files.folder = Some(p.to_path_buf());
            }
            ui.label(RichText::new(name).strong())
                .on_hover_text(dir.display().to_string());
        });
        let entries = self
            .files
            .listed
            .as_ref()
            .map(|(_, _, v)| v.clone())
            .unwrap_or_default();
        if entries.is_empty() {
            ui.label(RichText::new("No design files here").weak());
        }
        let current = Path::new(&self.file_path).to_path_buf();
        for e in entries {
            ui.horizontal(|ui| {
                tree_icon(
                    ui,
                    if e.folder {
                        Icon::Project
                    } else {
                        file_icon(&e.path)
                    },
                );
                let r = ui
                    .selectable_label(e.path == current, &e.name)
                    .on_hover_text(if e.folder {
                        "Double-click to open the folder"
                    } else {
                        "Double-click to open the file"
                    });
                if r.double_clicked() {
                    if e.folder {
                        self.files.folder = Some(e.path.clone());
                    } else {
                        *open_file = Some(e.path.clone());
                    }
                }
            });
        }
    }

    /// Where the document chips were: one button naming the active document,
    /// listing the file's documents.
    pub(super) fn active_document_menu(&mut self, ui: &mut egui::Ui) {
        if self.project.documents().len() < 2 {
            return;
        }
        let docs: Vec<(DocumentId, String, DocumentKind)> = self
            .project
            .documents()
            .iter()
            .map(|d| (d.id, d.name.clone(), d.data.kind()))
            .collect();
        let name = docs
            .iter()
            .find(|d| d.0 == self.part_id)
            .map_or_else(String::new, |d| d.1.clone());
        let mut open = None;
        ui.menu_button(format!("{name} ({})", docs.len()), |ui| {
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    for (id, n, kind) in &docs {
                        ui.horizontal(|ui| {
                            tree_icon(ui, kind_icon(*kind));
                            if ui.selectable_label(*id == self.part_id, n).clicked() {
                                open = Some(*id);
                                ui.close();
                            }
                        });
                    }
                });
        })
        .response
        .on_hover_text("The documents of this file (Files tab of the browser)");
        if let Some(id) = open
            && id != self.part_id
        {
            self.asm.came_from = None;
            self.activate_document(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_lists_folders_first_and_only_design_files() {
        let dir = std::env::temp_dir().join(format!("cc_files_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sub")).unwrap();
        for f in ["b.ccpart", "a.ccasm", "notes.txt", ".hidden.ccpart"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let names: Vec<String> = read_folder(&dir).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["Sub", "a.ccasm", "b.ccpart"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_lists_nothing() {
        assert!(read_folder(Path::new("Z:/no/such/folder/here")).is_empty());
    }
}
