//! The **breadcrumb** at the top of a feature's property panel (Inventor:
//! `Sketch1 > Extrusion1`). Reusable by every feature command (Extrude,
//! Revolve, Hole, Shell, ...):
//!
//! - a click on another crumb (the sketch) asks to go there (the command
//!   keeps its pending inputs and comes back when the sketch is finished);
//! - a click on the current crumb (the feature) asks to come back to it;
//!   a second click on it, or a double click, starts renaming it;
//! - Enter applies the new name, Esc or an empty text keeps the old one.
//!
//! The logic is [`BreadcrumbState`] (no egui, unit-tested); [`show`] draws
//! it and feeds the clicks in.
//!
//! Usage: keep a `BreadcrumbState` in the command's draft, build the
//! crumbs each frame, and handle the returned [`CrumbEvent`].

use eframe::egui;

/// One part of the breadcrumb.
#[derive(Clone, Debug, PartialEq)]
pub struct Crumb {
    /// Text shown.
    pub label: String,
    /// The name can be changed here (the feature itself).
    pub renamable: bool,
}

impl Crumb {
    /// A crumb that only navigates (a sketch).
    pub fn link(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            renamable: false,
        }
    }

    /// The feature's own crumb (renamable).
    pub fn feature(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            renamable: true,
        }
    }
}

/// What the user asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum CrumbEvent {
    /// Go to crumb `k` (another one than the current: e.g. edit the sketch).
    Open(usize),
    /// Back to the current crumb (the feature).
    Back(usize),
    /// Rename crumb `k`.
    Rename(usize, String),
}

/// Breadcrumb state: which crumb is current and an ongoing rename.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BreadcrumbState {
    /// Index of the current crumb (the feature, usually the last).
    pub current: usize,
    /// Crumb being renamed and the text typed so far.
    pub renaming: Option<(usize, String)>,
    /// Keyboard focus was given to the rename field.
    pub focused: bool,
}

impl BreadcrumbState {
    /// A breadcrumb whose current crumb is `current`.
    pub fn at(current: usize) -> Self {
        Self {
            current,
            ..Default::default()
        }
    }

    /// A single click on crumb `k`.
    pub fn click(&mut self, crumbs: &[Crumb], k: usize) -> Option<CrumbEvent> {
        let c = crumbs.get(k)?;
        if self.renaming.is_some() {
            // A click elsewhere ends the rename without changing it.
            self.cancel();
            return None;
        }
        if k == self.current {
            if c.renamable {
                self.start_rename(crumbs, k);
            }
            return Some(CrumbEvent::Back(k));
        }
        Some(CrumbEvent::Open(k))
    }

    /// A double click on crumb `k`: rename it (if it can be).
    pub fn double_click(&mut self, crumbs: &[Crumb], k: usize) -> Option<CrumbEvent> {
        if crumbs.get(k).is_some_and(|c| c.renamable) && self.renaming.is_none() {
            self.start_rename(crumbs, k);
        }
        None
    }

    fn start_rename(&mut self, crumbs: &[Crumb], k: usize) {
        if let Some(c) = crumbs.get(k) {
            self.renaming = Some((k, c.label.clone()));
            self.focused = false;
        }
    }

    /// Enter in the rename field: the new name (if changed and not empty).
    pub fn commit(&mut self, crumbs: &[Crumb]) -> Option<CrumbEvent> {
        let (k, text) = self.renaming.take()?;
        self.focused = false;
        let text = text.trim().to_owned();
        let old = crumbs.get(k)?;
        (!text.is_empty() && text != old.label).then_some(CrumbEvent::Rename(k, text))
    }

    /// Esc: keep the old name.
    pub fn cancel(&mut self) {
        self.renaming = None;
        self.focused = false;
    }
}

/// Draws the breadcrumb (`A > B`) and returns what was asked for.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut BreadcrumbState,
    crumbs: &[Crumb],
) -> Option<CrumbEvent> {
    let mut event = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (k, c) in crumbs.iter().enumerate() {
            if k > 0 {
                ui.label(egui::RichText::new(">").weak());
            }
            if let Some((rk, text)) = state.renaming.as_mut()
                && *rk == k
            {
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(120.0));
                if !state.focused {
                    super::valuefield::focus_all(ui.ctx(), &r, text);
                    state.focused = true;
                }
                if r.lost_focus() {
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        state.cancel();
                    } else {
                        event = state.commit(crumbs);
                    }
                }
                continue;
            }
            let current = k == state.current;
            let text = if current {
                egui::RichText::new(&c.label).strong()
            } else {
                egui::RichText::new(&c.label).underline()
            };
            let tip = match (current, c.renamable) {
                (true, true) => "The feature: click again (or double-click) to rename it",
                (true, false) => "Back to the command",
                (false, _) => "Edit it; Finish Sketch returns to this command",
            };
            let r = ui
                .add(egui::Label::new(text).sense(egui::Sense::click()))
                .on_hover_text(tip);
            if r.double_clicked() {
                if let Some(e) = state.double_click(crumbs, k) {
                    event = Some(e);
                }
            } else if r.clicked() {
                event = state.click(crumbs, k);
            }
        }
    });
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crumbs() -> Vec<Crumb> {
        vec![Crumb::link("Sketch1"), Crumb::feature("Extrusion1")]
    }

    #[test]
    fn sketch_click_opens_the_sketch() {
        let mut s = BreadcrumbState::at(1);
        assert_eq!(s.click(&crumbs(), 0), Some(CrumbEvent::Open(0)));
        assert!(s.renaming.is_none());
    }

    #[test]
    fn feature_click_goes_back_then_renames() {
        let c = crumbs();
        let mut s = BreadcrumbState::at(1);
        // First click: back to the feature, and the name becomes editable.
        assert_eq!(s.click(&c, 1), Some(CrumbEvent::Back(1)));
        assert_eq!(s.renaming, Some((1, "Extrusion1".into())));
        if let Some((_, t)) = s.renaming.as_mut() {
            *t = "  Boss  ".into();
        }
        assert_eq!(s.commit(&c), Some(CrumbEvent::Rename(1, "Boss".into())));
        assert!(s.renaming.is_none());
    }

    #[test]
    fn rename_keeps_the_name_when_empty_unchanged_or_cancelled() {
        let c = crumbs();
        let mut s = BreadcrumbState::at(1);
        s.double_click(&c, 1);
        assert_eq!(s.commit(&c), None, "unchanged");
        s.double_click(&c, 1);
        if let Some((_, t)) = s.renaming.as_mut() {
            t.clear();
        }
        assert_eq!(s.commit(&c), None, "empty");
        s.double_click(&c, 1);
        s.cancel();
        assert_eq!(s.commit(&c), None, "cancelled");
        // A sketch crumb cannot be renamed.
        s.double_click(&c, 0);
        assert!(s.renaming.is_none());
    }

    #[test]
    fn a_click_elsewhere_ends_a_rename() {
        let c = crumbs();
        let mut s = BreadcrumbState::at(1);
        s.double_click(&c, 1);
        assert_eq!(s.click(&c, 0), None);
        assert!(s.renaming.is_none());
        assert_eq!(s.click(&c, 0), Some(CrumbEvent::Open(0)));
    }
}
