//! The Calculation pages of Spur Gears, Parallel Key, Bearing and Compression
//! Spring (spec 6.2-design-calculators §3.3–3.5): the planned panel's values
//! through `design::gear`, `design::key`, `design::bearing` and
//! `design::spring`, shown live under Calculate; OK of Spur Gears and Key
//! places their parts (`design::gear_model`).

use eframe::egui::{self, RichText};

use super::asm_planned::AsmPlanned;
use super::panels::plabel;
use super::part_planned::{PartDialog, Planned};
use crate::design::gear::{
    self, BendingFactors, GEAR_WEIBULL, GearPair, bending_stress, life_at, tooth_life,
};
use crate::design::gear_model as gear_parts;
use crate::design::key::{self, Ends};
use crate::design::{bearing, spring};

/// Whether `d` is the Spur Gears panel.
pub(super) fn is_spur(d: &PartDialog) -> bool {
    d.kind == Planned::Asm(AsmPlanned::SpurGear)
}

/// Whether `d` is the Key panel.
pub(super) fn is_key(d: &PartDialog) -> bool {
    d.kind == Planned::Asm(AsmPlanned::Key)
}

/// Whether `d` is the Bearing panel.
pub(super) fn is_bearing(d: &PartDialog) -> bool {
    d.kind == Planned::Asm(AsmPlanned::Bearing)
}

/// Whether `d` is one of the spring panels.
pub(super) fn is_spring(d: &PartDialog) -> bool {
    matches!(
        d.kind,
        Planned::Asm(
            AsmPlanned::CompressionSpring
                | AsmPlanned::ExtensionSpring
                | AsmPlanned::TorsionSpring
                | AsmPlanned::BellevilleSpring
        )
    )
}

fn rows(ui: &mut egui::Ui, id: &str, rows: &[(String, String)]) {
    ui.vertical(|ui| {
        let _ = id;
        for (a, b) in rows {
            ui.label(format!("{a}: {b}"));
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
        fit(ui);
        wrap(ui);
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
        let power = n("Power (kW)", 10.0) * 1000.0;
        let rpm = n("Speed (rpm)", 1500.0);
        if power > 0.0 && rpm > 0.0 {
            let t = gear::torque_from_power(power, rpm);
            let (wt, wn) = gear::tooth_loads(&g, t);
            r.push(("Torque (gear 1)".into(), format!("{:.2} N·m", t / 1000.0)));
            r.push(("Tangential Force".into(), format!("{wt:.1} N")));
            // Kv from the pitch-line speed and the accuracy level (Fig. 25).
            let level = n("Accuracy Level (AGMA)", 8.0).round().clamp(3.0, 11.0) as u32;
            let v = gear::pitch_line_velocity(g.pitch[0], rpm);
            let kv = gear::dynamic_factor(level, v);
            r.push((
                "Pitch-Line Velocity".into(),
                format!("{v:.2} m/s, Kv {kv:.3} (level {level}, Fig. 25)"),
            ));
            let tip = d.check(1, "Load at the Tip");
            let allowed = n("Allowable Bending Stress (MPa)", 250.0);
            for k in 0..2 {
                // J laid out from the tooth (spur), typed for helical gears.
                let j = match gear::geometry_factor(&pair, &g, k, tip) {
                    Ok(f) => f.j,
                    Err(_) => n("Geometry Factor J (helical)", 0.35),
                };
                let f = BendingFactors {
                    j,
                    ka: n("Application Factor Ka", 1.0),
                    km: n("Load Distribution Factor Km", 1.3),
                    kv,
                };
                let sb = bending_stress(wt, pair.face[k], pair.module, &f);
                r.push((
                    format!("Gear {} Bending Stress (eq. 53)", k + 1),
                    format!("J {j:.3}, {sb:.1} MPa, safety {:.2}", allowed / sb),
                ));
            }
            let helical = pair.helix.abs() > 1e-9 && g.overlap_ratio > 2.0;
            let face = pair.face[0].min(pair.face[1]);
            match tooth_life(&g, &pair, wn, face, helical) {
                Ok(life) => {
                    let factor = n("Life Factor (material)", 1.0);
                    let rel = (n("Reliability (%)", 90.0) / 100.0).clamp(0.01, 0.99999);
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
            RichText::new(
                "J from the tooth laid out by its rack (Lewis parabola, Dolan and Broghamer), \
                 Kv of RP-1152 Fig. 25",
            )
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
        fit(ui);
        wrap(ui);
        let k = match key::size_for(dia) {
            Ok(k) => k,
            Err(e) => {
                ui.colored_label(super::theme::tokens().error, format!("Key: {e}"));
                return;
            }
        };
        let t = n("Torque (N·m)", 100.0) * 1000.0;
        let l = n("Key Length", 50.0);
        let (ts, tp) = (
            n("Allowed Shear (MPa)", 80.0),
            n("Allowed Pressure (MPa)", 150.0),
        );
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

/// The Bearing panel's Calculate row: equivalent load, L10 and the adjusted life.
pub(super) fn bearing_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    plabel(ui, "Results");
    let n = |l: &str, def: f64| d.num(1, l).unwrap_or(def);
    let kind = if d.index(1, "Bearing Type") == 1 {
        bearing::Kind::Roller
    } else {
        bearing::Kind::Ball
    };
    ui.vertical(|ui| {
        fit(ui);
        wrap(ui);
        let tok = super::theme::tokens();
        let need = n("Required Life (h)", 10000.0);
        match bearing::check(
            n("Radial Load (N)", 2000.0),
            n("Axial Load (N)", 0.0),
            n("Radial Factor X", 1.0),
            n("Axial Factor Y", 0.0),
            n("Basic Dynamic Load Rating C (N)", 14000.0),
            n("Speed (rpm)", 1500.0),
            kind,
            n("Life Adjustment Factors (D·E·F·G·H)", 1.0),
            n("Required Reliability (%)", 90.0) / 100.0,
            need,
        ) {
            Ok(b) => {
                rows(
                    ui,
                    "bearing_results",
                    &[
                        ("Equivalent Load Fe".into(), format!("{:.1} N", b.fe)),
                        (
                            "Basic Life L10 = (C/Fe)^m".into(),
                            format!("{:.2} million revolutions, {:.0} h", b.l10, b.l10_hours),
                        ),
                        ("Required C".into(), format!("{:.0} N", b.required_c)),
                    ],
                );
                let col = if b.adjusted_hours >= need {
                    tok.ok
                } else {
                    tok.error
                };
                ui.colored_label(
                    col,
                    format!(
                        "Adjusted life at the reliability: {:.0} h (required {need:.0} h)",
                        b.adjusted_hours
                    ),
                );
            }
            Err(e) => {
                ui.colored_label(tok.error, format!("Bearing: {e}"));
            }
        }
        ui.label(
            RichText::new("Hamrock and Anderson, Rolling-Element Bearings, NASA RP-1105, §12")
                .weak(),
        );
    });
    ui.end_row();
}

/// A spring panel's Calculate row: rate, deflection, stress and lengths.
pub(super) fn spring_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    plabel(ui, "Results");
    let compression = d.kind == Planned::Asm(AsmPlanned::CompressionSpring);
    let n0 = |l: &str, def: f64| d.num(0, l).unwrap_or(def);
    let n1 = |l: &str, def: f64| d.num(1, l).unwrap_or(def);
    let wire = n0("Wire Diameter", 2.0);
    let s = spring::Spring {
        wire,
        mean_diameter: n0("Outer Diameter", 20.0) - wire,
        active: n0("Active Coils", 6.0),
        free_length: n0("Free Length", 50.0),
        ends: match d.index(0, "Ends") {
            1 => spring::Ends::Closed,
            2 => spring::Ends::Open,
            _ => spring::Ends::ClosedGround,
        },
        g: n1("Shear Modulus G (MPa)", spring::G_SPRING_STEEL),
    };
    ui.vertical(|ui| {
        fit(ui);
        wrap(ui);
        let tok = super::theme::tokens();
        if !compression {
            ui.label(RichText::new("The calculation covers compression springs yet").weak());
            return;
        }
        if let Err(e) = s.validate() {
            ui.colored_label(tok.error, format!("Compression Spring: {e}"));
            return;
        }
        let p = n1("Working Load (N)", 100.0);
        let typed = n1("Allowed Stress (MPa)", 0.0);
        let allowed = if typed > 0.0 {
            typed
        } else {
            spring::safe_stress(wire)
        };
        let defl = s.deflection(p);
        let stress = s.stress(p);
        rows(
            ui,
            "spring_results",
            &[
                ("Rate".into(), format!("{:.3} N/mm", s.rate())),
                ("Deflection (eq. 2)".into(), format!("{defl:.3} mm")),
                (
                    "Working Length".into(),
                    format!("{:.3} mm", s.free_length - defl),
                ),
                ("Solid Length".into(), format!("{:.3} mm", s.solid_length())),
                ("Stress (eq. 4)".into(), format!("{stress:.1} MPa")),
                (
                    "Deflection at the Allowed Stress (eq. 7)".into(),
                    format!("{:.3} mm", s.deflection_at_stress(allowed)),
                ),
            ],
        );
        let ok = stress <= allowed && defl <= s.travel();
        let col = if ok { tok.ok } else { tok.error };
        let why = if defl > s.travel() {
            "the spring goes solid before the load".to_owned()
        } else {
            format!("stress {stress:.0} of {allowed:.0} MPa allowed")
        };
        ui.colored_label(col, why);
        ui.label(
            RichText::new(
                "Kimball and Barr, Elements of Machine Design, §43 (torsion of the wire, no \
                 curvature correction); allowed stress by eq. 12 unless typed",
            )
            .weak(),
        );
    });
    ui.end_row();
}

/// Keeps a cell inside the Properties panel.
fn fit(ui: &mut egui::Ui) {
    super::bolted::fit_cell(ui);
}

/// Wraps the text of a cell of results.
fn wrap(ui: &mut egui::Ui) {
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
}

impl super::CadCraftApp {
    /// Spur Gears OK: each gear whose model is Component becomes a part (its
    /// teeth extruded, a bore the size of its picked cylinder) placed on its
    /// axis; without picks gear 1 stands at the origin and gear 2 beside it
    /// at the centre distance, turned half a pitch so the teeth mesh. One
    /// Undo step; nothing is made on an error (rule 5).
    pub(super) fn gear_ok(&mut self, d: &PartDialog) -> bool {
        match self.gear_make(d) {
            Ok(text) => {
                self.file_status = Some(Ok(text));
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Spur Gears: {e}")));
                false
            }
        }
    }

    fn gear_make(&mut self, d: &PartDialog) -> Result<String, String> {
        let pair = pair_of(d);
        let g = pair.geometry()?;
        let mut parts = Vec::new();
        for k in 0..2 {
            if d.index(0, if k == 0 { "Gear 1" } else { "Gear 2" }) != 0 {
                continue;
            }
            let keys = [("cyl1", "start1"), ("cyl2", "start2")][k];
            let picked = d.refs.get(keys.0).is_some_and(|v| !v.is_empty());
            let (mut place, radius) = self.axis_placement(d, keys.0, keys.1, false)?;
            let bore = radius.map_or(0.0, |r| 2.0 * r);
            let mut part = gear_parts::gear_part(&pair, &g, k, bore)?;
            part.regenerate().map_err(|e| e.to_string())?;
            if let Some((_, msg)) = part.feature_health.iter().next() {
                return Err(msg.clone());
            }
            if k == 1 && !picked {
                // Beside gear 1, a space facing its tooth.
                let (base, _) = self.axis_placement(d, "cyl1", "start1", false)?;
                let turn = std::f64::consts::PI + std::f64::consts::PI / f64::from(pair.teeth[1]);
                let local = crate::kernel::Transform3::new(
                    crate::kernel::Vector3::new(g.center_distance, 0.0, 0.0),
                    crate::kernel::Vector3::z() * turn,
                );
                place = base * local;
            }
            parts.push((part, place, k));
        }
        if parts.is_empty() {
            return Err("both gears are set to No Model or Feature".into());
        }
        let mut trial = self.project.clone();
        let mut occs = Vec::new();
        for (part, place, k) in parts {
            occs.push(self.place_generated(
                &mut trial,
                part,
                &format!("Spur Gear {}", k + 1),
                place,
            )?);
        }
        self.history.break_coalescing();
        self.checkpoint("Spur Gears");
        self.project = trial;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = occs.clone();
        self.asm.sub.clear();
        self.scene_dirty = true;
        Ok(format!(
            "Spur Gears: {} placed, centre distance {:.3} mm",
            occs.len(),
            g.center_distance
        ))
    }

    /// Key OK: the DIN 6885 key for the shaft (the picked cylinder, else the
    /// Shaft Diameter) placed in its groove: on the axis, its bottom at the
    /// groove's floor, from the start plane. One Undo step.
    pub(super) fn key_ok(&mut self, d: &PartDialog) -> bool {
        match self.key_make(d) {
            Ok(text) => {
                self.file_status = Some(Ok(text));
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Key: {e}")));
                false
            }
        }
    }

    fn key_make(&mut self, d: &PartDialog) -> Result<String, String> {
        let (place, radius) = self.axis_placement(d, "shaft", "start", false)?;
        let dia = radius.map_or_else(|| d.num(1, "Shaft Diameter").unwrap_or(40.0), |r| 2.0 * r);
        let k = key::size_for(dia)?;
        let l = d.num(1, "Key Length").unwrap_or(50.0);
        let ends = if d.index(0, "Groove") == 1 {
            Ends::Square
        } else {
            Ends::Rounded
        };
        let mut part = crate::design::gear_model::key_part(&k, l, ends)?;
        part.regenerate().map_err(|e| e.to_string())?;
        if let Some((_, msg)) = part.feature_health.iter().next() {
            return Err(msg.clone());
        }
        let keys = d
            .num(0, "Number of Keys")
            .unwrap_or(1.0)
            .round()
            .clamp(1.0, 2.0) as u32;
        let mut trial = self.project.clone();
        let mut occs = Vec::new();
        for n in 0..keys {
            // A second key opposite the first.
            let turn = crate::kernel::Transform3::new(
                crate::kernel::Vector3::zeros(),
                crate::kernel::Vector3::z() * (std::f64::consts::PI * f64::from(n)),
            );
            let floor = crate::kernel::Transform3::new(
                crate::kernel::Vector3::new(dia / 2.0 - k.t1, 0.0, 0.0),
                crate::kernel::Vector3::zeros(),
            );
            occs.push(self.place_generated(
                &mut trial,
                part.clone(),
                "Key",
                place * turn * floor,
            )?);
        }
        self.history.break_coalescing();
        self.checkpoint("Parallel Key");
        self.project = trial;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = occs;
        self.asm.sub.clear();
        self.scene_dirty = true;
        Ok(format!(
            "Key: DIN 6885 A {} × {} × {l} placed for a Ø{dia:.1} shaft",
            k.b, k.h
        ))
    }
}
