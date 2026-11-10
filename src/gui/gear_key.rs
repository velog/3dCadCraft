//! The Calculation pages of Spur Gears and Parallel Key (spec
//! 6.2-design-calculators §3.3–3.4): the planned panel's values through
//! `design::gear` and `design::key`, shown live under Calculate. The models
//! (gear teeth, key grooves) come later, so OK stays off for both.

use eframe::egui::{self, RichText};

use super::asm_planned::AsmPlanned;
use super::panels::plabel;
use super::part_planned::{PartDialog, Planned};
use crate::design::gear::{
    self, BendingFactors, GEAR_WEIBULL, GearPair, bending_stress, life_at, tooth_life,
};
use crate::design::key::{self, Ends};

/// Whether `d` is the Spur Gears panel.
pub(super) fn is_spur(d: &PartDialog) -> bool {
    d.kind == Planned::Asm(AsmPlanned::SpurGear)
}

/// Whether `d` is the Key panel.
pub(super) fn is_key(d: &PartDialog) -> bool {
    d.kind == Planned::Asm(AsmPlanned::Key)
}

fn rows(ui: &mut egui::Ui, id: &str, rows: &[(String, String)]) {
    egui::Grid::new(id).show(ui, |ui| {
        for (a, b) in rows {
            ui.label(a.as_str());
            ui.label(b.as_str());
            ui.end_row();
        }
    });
}

/// The pair from the Design page.
pub(super) fn pair_of(d: &PartDialog) -> GearPair {
    let n = |l: &str, def: f64| d.num(0, l).unwrap_or(def);
    let teeth = |l: &str, def: f64| n(l, def).round().max(1.0) as u32;
    let mut p = GearPair {
        module: n("Module", 2.0),
        pressure_angle: n("Pressure Angle", 20.0).to_radians(),
        helix: n("Helix Angle", 0.0).to_radians(),
        teeth: [
            teeth("Number of Teeth 1", 20.0),
            teeth("Number of Teeth 2", 40.0),
        ],
        shift: [n("Unit Correction 1", 0.0), n("Unit Correction 2", 0.0)],
        face: [n("Facewidth 1", 20.0), n("Facewidth 2", 20.0)],
        ..GearPair::default()
    };
    // Design Guide "Total Unit Correction": the shifts follow from the centre
    // distance, shared in proportion to the teeth.
    if d.index(0, "Design Guide") == 3
        && let Ok(sum) = p.shift_sum_for(n("Center Distance", 60.0))
    {
        let [z1, z2] = p.teeth.map(f64::from);
        p.shift = [sum * z2 / (z1 + z2), sum * z1 / (z1 + z2)];
    }
    p
}

/// Spur Gears' Calculate row: geometry, loads, bending stress and life.
pub(super) fn gear_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    plabel(ui, "Results");
    let pair = pair_of(d);
    let n = |l: &str, def: f64| d.num(1, l).unwrap_or(def);
    ui.vertical(|ui| {
        let g = match pair.geometry() {
            Ok(g) => g,
            Err(e) => {
                ui.colored_label(super::theme::tokens().error, format!("Spur Gears: {e}"));
                return;
            }
        };
        let f2 = |a: [f64; 2]| format!("{:.3} / {:.3} mm", a[0], a[1]);
        let mut r = vec![
            (
                "Center Distance".to_owned(),
                format!("{:.3} mm", g.center_distance),
            ),
            ("Pitch Diameter".to_owned(), f2(g.pitch)),
            ("Outside Diameter".to_owned(), f2(g.tip)),
            ("Root Diameter".to_owned(), f2(g.root)),
            ("Base Circle Diameter".to_owned(), f2(g.base)),
            (
                "Unit Corrections".to_owned(),
                format!("{:.4} / {:.4}", pair.shift[0], pair.shift[1]),
            ),
            (
                "Operating Pressure Angle".to_owned(),
                format!("{:.3}°", g.working_pressure_angle.to_degrees()),
            ),
            (
                "Contact Ratio".to_owned(),
                format!("{:.3}", g.contact_ratio),
            ),
            (
                "Overlap Ratio".to_owned(),
                format!("{:.3}", g.overlap_ratio),
            ),
        ];
        let power = n("Power", 10.0) * 1000.0;
        let rpm = n("Speed", 1500.0);
        if power > 0.0 && rpm > 0.0 {
            let t = gear::torque_from_power(power, rpm);
            let (wt, wn) = gear::tooth_loads(&g, t);
            r.push(("Torque (gear 1)".into(), format!("{:.2} N·m", t / 1000.0)));
            r.push(("Tangential Force".into(), format!("{wt:.1} N")));
            let f = BendingFactors {
                j: n("Geometry Factor J", 0.35),
                ka: n("Application Factor Ka", 1.0),
                km: n("Load Distribution Factor Km", 1.3),
                kv: n("Dynamic Factor Kv", 0.8),
            };
            let face = pair.face[0].min(pair.face[1]);
            let sb = bending_stress(wt, face, pair.module, &f);
            let allowed = n("Allowable Bending Stress", 250.0);
            r.push((
                "Bending Stress (eq. 53)".into(),
                format!("{sb:.1} MPa, safety {:.2}", allowed / sb),
            ));
            let helical = pair.helix.abs() > 1e-9 && g.overlap_ratio > 2.0;
            match tooth_life(&g, &pair, wn, face, helical) {
                Ok(life) => {
                    let factor = n("Life Factor (material)", 1.0);
                    let rel = (n("Reliability", 90.0) / 100.0).clamp(0.01, 0.99999);
                    let t10 = life.t10 * factor;
                    let g10 = gear::gear_life(t10, pair.teeth[0]);
                    let hours = |cycles_m: f64| cycles_m * 1e6 / rpm / 60.0;
                    r.push((
                        "Tooth Life L10 (eq. 68)".into(),
                        format!("{t10:.4e} million cycles, {:.0} h", hours(t10)),
                    ));
                    r.push((
                        format!("Gear 1 Life at {:.2} % (eq. 67, 70)", rel * 100.0),
                        format!("{:.0} h", hours(life_at(g10, rel, GEAR_WEIBULL))),
                    ));
                }
                Err(e) => r.push(("Surface Life".into(), e)),
            }
        }
        rows(ui, "gear_results", &r);
        for w in &g.warnings {
            ui.colored_label(super::theme::tokens().warning, format!("Warning: {w}"));
        }
        ui.label(
            RichText::new("J and Kv are read off RP-1152 Fig. 27 / 25; the gear model comes later")
                .weak(),
        );
    });
    ui.end_row();
}

/// Key's Calculate row: the DIN 6885 size and the check.
pub(super) fn key_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    plabel(ui, "Results");
    let n = |l: &str, def: f64| d.num(1, l).unwrap_or(def);
    let dia = n("Shaft Diameter", 40.0);
    let ends = if d.index(0, "Groove") == 1 {
        Ends::Square
    } else {
        Ends::Rounded
    };
    let keys = d.num(0, "Number of Keys").unwrap_or(1.0).round().max(1.0) as u32;
    ui.vertical(|ui| {
        let k = match key::size_for(dia) {
            Ok(k) => k,
            Err(e) => {
                ui.colored_label(super::theme::tokens().error, format!("Key: {e}"));
                return;
            }
        };
        let t = n("Torque", 100.0) * 1000.0;
        let l = n("Key Length", 50.0);
        let (ts, tp) = (n("Allowed Shear", 80.0), n("Allowed Pressure", 150.0));
        let mut r = vec![(
            "Key (DIN 6885 A)".to_owned(),
            format!(
                "{} × {}, shaft groove {} deep, hub groove {} deep",
                k.b, k.h, k.t1, k.t2
            ),
        )];
        match key::check(&k, dia, l, ends, keys, t, ts, tp) {
            Ok(c) => {
                let tok = super::theme::tokens();
                r.push(("Force at the shaft".into(), format!("{:.0} N", c.force)));
                r.push((
                    "Bearing length".into(),
                    format!("{:.1} mm", c.bearing_length),
                ));
                rows(ui, "key_results", &r);
                for (what, v, s) in [
                    ("Shear (eq. 1)", c.shear, c.shear_safety),
                    ("Pressure (eq. 3)", c.pressure, c.pressure_safety),
                ] {
                    let col = if s >= 1.0 { tok.ok } else { tok.error };
                    ui.colored_label(col, format!("{what}: {v:.1} MPa, safety {s:.2}"));
                }
                if let Some(min) = key::shortest_length(&k, dia, ends, keys, t, tp) {
                    ui.label(format!(
                        "Shortest standard length for the pressure: {min} mm"
                    ));
                }
            }
            Err(e) => {
                rows(ui, "key_results", &r);
                ui.colored_label(super::theme::tokens().error, format!("Key: {e}"));
            }
        }
        ui.label(RichText::new("Kimball & Barr, Elements of Machine Design, §69").weak());
    });
    ui.end_row();
}
