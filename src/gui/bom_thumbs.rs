//! Pictures of the components in the Bill of Materials (`docs/spec/3.6-bom.md`,
//! Thumbnail column): each component drawn once on a worker thread by the CPU
//! renderer of the saved previews (`part::thumbnail`), kept by its document
//! and drawn again only when its body (or a subassembly's parts and
//! placements) changed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use super::CadCraftApp;
use crate::document::DocRef;
use crate::part::thumbnail;

/// Pixels of a picture (drawn smaller in the row, sharp when the table zooms).
const SIZE: u32 = 96;
/// How often a component's signature is compared (it walks its vertices).
const RECHECK: Duration = Duration::from_millis(1000);

/// One component's picture.
struct Thumb {
    /// What it was drawn from.
    sig: String,
    checked: Instant,
    /// The worker's PNG (`Some(None)`: nothing to draw).
    slot: Arc<Mutex<Option<Option<Vec<u8>>>>>,
    texture: Option<egui::TextureHandle>,
}

/// The pictures of the open BOM window.
#[derive(Default)]
pub(super) struct BomThumbs {
    by_doc: HashMap<DocRef, Thumb>,
}

impl CadCraftApp {
    /// What the picture of `r` is drawn from: its body, or for an assembly each
    /// part's body and placement.
    fn thumb_signature(&self, r: DocRef) -> String {
        if let Some(s) = self.assembly_signature(r, Default::default()) {
            return s;
        }
        self.with_part(r, |p| p.body_signature())
            .unwrap_or_default()
    }

    /// The scene of `r`: a part, or every shown part of an assembly at its place.
    fn thumb_scene(&self, r: DocRef) -> thumbnail::Scene {
        let mut scene = thumbnail::Scene::default();
        if let Some(leaves) = self.drawing_leaves(r, Default::default()) {
            for (_, _, component, world) in leaves {
                self.with_part(component, |p| scene.add_part(p, Some(&world)));
            }
        } else {
            self.with_part(r, |p| scene.add_part(p, None));
        }
        scene
    }

    /// The picture of component `r` in a BOM row, `side` points square.
    pub(super) fn bom_thumb(
        &self,
        ui: &mut egui::Ui,
        thumbs: &mut BomThumbs,
        r: Option<DocRef>,
        side: f32,
    ) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
        let Some(r) = r else {
            return;
        };
        let now = Instant::now();
        // The signature walks the body's vertices: not every frame.
        let due = thumbs
            .by_doc
            .get(&r)
            .is_none_or(|t| now.duration_since(t.checked) >= RECHECK);
        let sig = due.then(|| self.thumb_signature(r));
        let stale = match (&sig, thumbs.by_doc.get_mut(&r)) {
            (Some(_), None) => true,
            (Some(s), Some(t)) => {
                t.checked = now;
                *s != t.sig
            }
            (None, _) => false,
        };
        if let (true, Some(sig)) = (stale, sig) {
            let scene = self.thumb_scene(r);
            let slot: Arc<Mutex<Option<Option<Vec<u8>>>>> = Arc::default();
            let out = slot.clone();
            let wake = ui.ctx().clone();
            let spawned = std::thread::Builder::new()
                .name("bom-thumb".into())
                .spawn(move || {
                    let png = thumbnail::png(&scene, thumbnail::iso_view(), SIZE);
                    *crate::diagnostics::lock(&out) = Some(png);
                    wake.request_repaint();
                });
            if let Err(e) = spawned {
                log::warn!("BOM picture: {e}");
            }
            // The old picture stays until the new one is there.
            let texture = thumbs.by_doc.remove(&r).and_then(|t| t.texture);
            thumbs.by_doc.insert(
                r,
                Thumb {
                    sig,
                    checked: now,
                    slot,
                    texture,
                },
            );
        }
        let Some(t) = thumbs.by_doc.get_mut(&r) else {
            return;
        };
        if let Some(png) = crate::diagnostics::lock(&t.slot).take() {
            t.texture = png
                .as_deref()
                .and_then(thumbnail::decode_png)
                .map(|(w, h, rgba)| {
                    let img =
                        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                    ui.ctx().load_texture(
                        format!("bom-thumb:{r:?}"),
                        img,
                        egui::TextureOptions::LINEAR,
                    )
                });
        }
        if let Some(tex) = &t.texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(tex.id(), rect.shrink(1.0), uv, egui::Color32::WHITE);
            resp.on_hover_ui(|ui| {
                ui.image((tex.id(), egui::vec2(SIZE as f32 * 2.0, SIZE as f32 * 2.0)));
            });
        }
    }
}
