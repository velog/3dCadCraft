//! The Calculation pages of Spur Gears, Parallel Key, Bearing and Compression
//! Spring (spec 6.2-design-calculators §3.3–3.5): the planned panel's values
//! through `design::gear`, `design::key`, `design::bearing` and
//! `design::spring`, shown live under Calculate; OK of Spur Gears and Key
//! places their parts (`design::gear_model`).

use eframe::egui::{self, RichText};

use super::asm_planned::AsmPlanned;
use super::panels::plabel;
use super::part_planned::{PartDialog, Planned};
use crate::assembly::elements;
use crate::assembly::relation::ASSEMBLY;
use crate::design::gear::{
    self, BendingFactors, GEAR_WEIBULL, GearPair, bending_stress, life_at, tooth_life,
};
use crate::design::gear_model as gear_parts;
use crate::design::key::{self, Ends};
use crate::design::{bearing, spring};
use crate::document::{AssemblyDocument, DocumentData, DocumentId};
use crate::kernel::{Point3, Transform3, Vector3};

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

/// Result rows `label: value`, each with its explanation on hover (formula,
/// and `nums`' entry for it: the formula with the numbers).
fn rows(ui: &mut egui::Ui, generator: &str, rows: &[(String, String)], nums: &[(&str, String)]) {
    ui.vertical(|ui| {
        for (a, b) in rows {
            result_row(
                ui,
                generator,
                a,
                egui::RichText::new(format!("{a}: {b}")),
                nums,
            );
        }
    });
}

/// One result line `text` whose label is `label`, explained on hover.
fn result_row(
    ui: &mut egui::Ui,
    generator: &str,
    label: &str,
    text: RichText,
    nums: &[(&str, String)],
) {
    let key = result_key(generator, label);
    let n = nums
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.as_str());
    let r = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
    super::explain::result_hover(r, generator, key, n);
}

/// The explanation's key of a result label: as it is, else without its
/// "(eq. …)" and without "Gear 1 " / "Gear 2 ".
fn result_key<'a>(generator: &str, label: &'a str) -> &'a str {
    if crate::design::explain::result(generator, label).is_some() {
        return label;
    }
    let a = label.split(" (").next().unwrap_or(label);
    let a = a
        .strip_prefix("Gear 1 ")
        .or_else(|| a.strip_prefix("Gear 2 "))
        .unwrap_or(a);
    a
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
        let mut nums: Vec<(&str, String)> = vec![(
            "Pitch Diameter",
            format!(
                "d = m·z / cos β = {}·{} / cos {:.1}° = {:.3} mm",
                pair.module,
                pair.teeth[0],
                pair.helix.to_degrees(),
                g.pitch[0]
            ),
        )];
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
            nums.push((
                "Torque (gear 1)",
                format!("T = 9549·P / n = 9549·{:.3} / {rpm:.0} = {:.2} N·m", power / 1000.0, t / 1000.0),
            ));
            nums.push((
                "Tangential Force",
                format!("Wt = 2·T / d1 = 2·{:.0} / {:.3} = {wt:.1} N", t, g.pitch[0]),
            ));
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
                if k == 0 {
                    nums.push((
                        "Bending Stress",
                        format!(
                            "σb = Wt·Ka·Ks·Km / (Kv·b·m·J) = {wt:.1}·{}·{:.3}·{} / ({kv:.3}·{}·{}·{j:.3}) = {sb:.1} MPa (gear 1)",
                            f.ka,
                            gear::size_factor(pair.module),
                            f.km,
                            pair.face[k],
                            pair.module
                        ),
                    ));
                }
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
                        "Tooth Life L10".into(),
                        format!("{t10:.4e} million cycles, {:.0} h", hours(t10)),
                    ));
                    r.push((
                        "Gear 1 Life (eq. 67, 70)".into(),
                        format!(
                            "{:.0} h at {:.2} %",
                            hours(life_at(g10, rel, GEAR_WEIBULL)),
                            rel * 100.0
                        ),
                    ));
                }
                Err(e) => r.push(("Surface Life".into(), e)),
            }
        }
        rows(ui, crate::design::explain::GEARS, &r, &nums);
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
                let g = crate::design::explain::KEY;
                let lt = c.bearing_length;
                let nums = [
                    (
                        "Force at the shaft",
                        format!("F = 2·T / d = 2·{t:.0} / {dia} = {:.0} N", c.force),
                    ),
                    (
                        "Shear",
                        format!(
                            "τ = F / (b·lt) = {:.0} / ({}·{lt:.1}) = {:.1} MPa (per key share)",
                            c.force, k.b, c.shear
                        ),
                    ),
                    (
                        "Pressure",
                        format!(
                            "p = F / ((h − t1)·lt) = {:.0} / (({} − {})·{lt:.1}) = {:.1} MPa (per key share)",
                            c.force, k.h, k.t1, c.pressure
                        ),
                    ),
                ];
                rows(ui, g, &r, &nums);
                for (what, v, s) in [
                    ("Shear (eq. 1)", c.shear, c.shear_safety),
                    ("Pressure (eq. 3)", c.pressure, c.pressure_safety),
                ] {
                    let col = if s >= 1.0 { tok.ok } else { tok.error };
                    let text = RichText::new(format!("{what}: {v:.1} MPa, safety {s:.2}")).color(col);
                    result_row(ui, g, what, text, &nums);
                }
                if let Some(min) = key::shortest_length(&k, dia, ends, keys, t, tp) {
                    result_row(
                        ui,
                        g,
                        "Shortest standard length",
                        RichText::new(format!("Shortest standard length for the pressure: {min} mm")),
                        &nums,
                    );
                }
            }
            Err(e) => {
                rows(ui, crate::design::explain::KEY, &r, &[]);
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
                let m = kind.exponent();
                let nums = [
                    (
                        "Equivalent Load Fe",
                        format!(
                            "Fe = X·Fr + Y·Fa = {}·{} + {}·{} = {:.1} N",
                            n("Radial Factor X", 1.0),
                            n("Radial Load (N)", 2000.0),
                            n("Axial Factor Y", 0.0),
                            n("Axial Load (N)", 0.0),
                            b.fe
                        ),
                    ),
                    (
                        "Basic Life L10",
                        format!(
                            "L10 = (C / Fe)^m = ({} / {:.1})^{m:.3} = {:.2} million revolutions",
                            n("Basic Dynamic Load Rating C (N)", 14000.0),
                            b.fe,
                            b.l10
                        ),
                    ),
                ];
                rows(
                    ui,
                    crate::design::explain::BEARING,
                    &[
                        ("Equivalent Load Fe".into(), format!("{:.1} N", b.fe)),
                        (
                            "Basic Life L10".into(),
                            format!("{:.2} million revolutions, {:.0} h", b.l10, b.l10_hours),
                        ),
                        ("Required C".into(), format!("{:.0} N", b.required_c)),
                    ],
                    &nums,
                );
                let col = if b.adjusted_hours >= need {
                    tok.ok
                } else {
                    tok.error
                };
                result_row(
                    ui,
                    crate::design::explain::BEARING,
                    "Adjusted life",
                    RichText::new(format!(
                        "Adjusted life at the reliability: {:.0} h (required {need:.0} h)",
                        b.adjusted_hours
                    ))
                    .color(col),
                    &nums,
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
    let n1 = |l: &str, def: f64| d.num(1, l).unwrap_or(def);
    let s = spring_of(d);
    let wire = s.wire;
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
        let (dd, dm) = (s.wire, s.mean_diameter);
        let nums = [
            (
                "Rate",
                format!(
                    "k = d⁴·G / (8·D³·n) = {dd}⁴·{} / (8·{dm}³·{}) = {:.3} N/mm",
                    s.g,
                    s.active,
                    s.rate()
                ),
            ),
            (
                "Deflection",
                format!("δ = P / k = {p} / {:.3} = {defl:.3} mm", s.rate()),
            ),
            (
                "Stress",
                format!("p = 8·P·D / (π·d³) = 8·{p}·{dm} / (π·{dd}³) = {stress:.1} MPa"),
            ),
            (
                "Solid Length",
                format!(
                    "Ls = d·(n + inactive) = {dd}·({} + {}) = {:.3} mm",
                    s.active,
                    s.ends.inactive(),
                    s.solid_length()
                ),
            ),
        ];
        rows(
            ui,
            crate::design::explain::SPRING,
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
            &nums,
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
        if !d.element_edit.is_empty() {
            return self.gear_rebuild(d);
        }
        let pair = pair_of(d);
        let g = pair.geometry()?;
        let asm_id = self.part_id;
        let mut parts = Vec::new();
        for k in 0..2 {
            if d.index(0, if k == 0 { "Gear 1" } else { "Gear 2" }) != 0 {
                continue;
            }
            let keys = GEAR_KEYS[k];
            let picked = d.refs.get(keys.0).is_some_and(|v| !v.is_empty());
            let (mut place, radius) = self.axis_placement(d, keys.0, keys.1, false)?;
            let bore = radius.map_or(0.0, |r| 2.0 * r);
            let mut part = gear_parts::gear_part(&pair, &g, k, bore)?;
            part.regenerate().map_err(|e| e.to_string())?;
            if let Some((_, msg)) = part.feature_health.iter().next() {
                return Err(msg.clone());
            }
            let mut element = crate::design::Element {
                command: GEAR_ID.into(),
                index: k,
                values: d.values(),
                ..Default::default()
            };
            element.extra.insert("bore".into(), bore);
            part.element = Some(element);
            if k == 1 && !picked {
                // Beside gear 1 at the centre distance.
                let (base, _) = self.axis_placement(d, "cyl1", "start1", false)?;
                place = base
                    * Transform3::new(Vector3::new(g.center_distance, 0.0, 0.0), Vector3::zeros());
            }
            parts.push((part, place, k));
        }
        if parts.is_empty() {
            return Err("both gears are set to No Model or Feature".into());
        }
        let mut warn = None;
        if parts.len() == 2 {
            // Gear 2 turned so a space faces gear 1's tooth.
            parts[1].1 = elements::mesh_turn(&parts[0].1, &parts[1].1, pair.teeth);
            let (c1, c2) = (parts[0].1 * Point3::origin(), parts[1].1 * Point3::origin());
            let n = parts[0].1 * Vector3::z();
            let v = c2 - c1;
            let apart = (v - n * n.dot(&v)).norm();
            if (apart - g.center_distance).abs() > 1e-3 {
                warn = Some(format!(
                    "the axes stand {apart:.3} mm apart, the gears need {:.3} mm",
                    g.center_distance
                ));
            }
        }
        let mut trial = self.project.clone();
        let mut occs = Vec::new();
        let mut mates = Vec::new();
        for (part, place, k) in parts {
            let occ =
                self.place_generated(&mut trial, part, &format!("Spur Gear {}", k + 1), place)?;
            let keys = GEAR_KEYS[k];
            let seat = seat_on(&mut trial, asm_id, d, occ, keys.0, keys.1, &place)?;
            occs.push(occ);
            mates.push(seat.axis);
        }
        if let [m1, m2] = mates[..] {
            let mut a = asm_copy(&trial, asm_id)?;
            elements::gear_link(&trial, &mut a, m1, m2, pair.teeth)?;
            put_asm(&mut trial, asm_id, a);
        }
        self.history.break_coalescing();
        self.checkpoint("Spur Gears");
        self.project = trial;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = occs.clone();
        self.asm.sub.clear();
        self.scene_dirty = true;
        let mut text = format!(
            "Spur Gears: {} placed, centre distance {:.3} mm",
            occs.len(),
            g.center_distance
        );
        if occs.len() == 2 {
            text.push_str(&format!(", ratio {} : {}", pair.teeth[0], pair.teeth[1]));
        }
        if let Some(w) = warn {
            text.push_str(&format!(" (Warning: {w})"));
        }
        Ok(text)
    }

    /// Edit Spur Gears' OK: each edited gear's part rebuilt from the panel
    /// (its document, name and occurrences stay); a gear seated on a
    /// coordinate system of the assembly moves to the new centre distance,
    /// turned to mesh, and the pair's motion link takes the new ratio. One
    /// Undo step.
    fn gear_rebuild(&mut self, d: &PartDialog) -> Result<String, String> {
        let pair = pair_of(d);
        let g = pair.geometry()?;
        let asm_id = self.part_id;
        let mut trial = self.project.clone();
        let mut by_index: [Option<u32>; 2] = [None, None];
        for &(occ, doc) in &d.element_edit {
            let old = element_in(&trial, doc).ok_or("the gear's part no longer exists")?;
            let k = old.index.min(1);
            let bore = old.extra.get("bore").copied().unwrap_or(0.0);
            let hub = match (old.extra.get("hub_b"), old.extra.get("hub_t2")) {
                (Some(&b), Some(&t)) => Some((b, t)),
                _ => None,
            };
            let mut part = gear_parts::gear_part_keyed(&pair, &g, k, bore, hub)?;
            part.regenerate().map_err(|e| e.to_string())?;
            if let Some((_, msg)) = part.feature_health.iter().next() {
                return Err(msg.clone());
            }
            part.element = Some(crate::design::Element {
                values: d.values(),
                ..old
            });
            if let Some(x) = trial.document_mut(doc) {
                x.data = DocumentData::Part(part);
            }
            by_index[k] = Some(occ);
        }
        if let [Some(o1), Some(o2)] = by_index {
            let a = asm_copy(&trial, asm_id)?;
            let t1 = a.find(o1).ok_or("gear 1 is gone")?.transform;
            let t2 = a.find(o2).ok_or("gear 2 is gone")?.transform;
            let m2 = elements::axis_mate(&a, o2);
            if let Some(ucs) = m2.and_then(|m| elements::ucs_of(&a, m)) {
                // Along the old centre line, at the new distance.
                let (c1, c2) = (t1 * Point3::origin(), t2 * Point3::origin());
                let dir = (c2 - c1)
                    .try_normalize(1e-12)
                    .unwrap_or_else(|| t1 * Vector3::x());
                let mut t = t2;
                t.translation.vector = (c1 + dir * g.center_distance).coords;
                let t = elements::mesh_turn(&t1, &t, pair.teeth);
                elements::move_ucs(&mut trial, asm_id, &ucs, &t)?;
                let mut a = asm_copy(&trial, asm_id)?;
                if let Some(o) = a.find_mut(o2) {
                    o.transform = t;
                }
                put_asm(&mut trial, asm_id, a);
            }
            // The link takes the new ratio, meshing where the gears stand.
            let mut a = asm_copy(&trial, asm_id)?;
            let (m1, m2) = (elements::axis_mate(&a, o1), elements::axis_mate(&a, o2));
            if let (Some(m1), Some(m2)) = (m1, m2) {
                a.motion
                    .links
                    .retain(|l| !(l.driver == m1 && l.driven == m2));
                for c in [m1, m2] {
                    if let Some(c) = a.constraint_mut(c) {
                        c.locked = false;
                    }
                }
                elements::gear_link(&trial, &mut a, m1, m2, pair.teeth)?;
            }
            put_asm(&mut trial, asm_id, a);
        }
        self.history.break_coalescing();
        self.checkpoint("Edit Spur Gears");
        self.project = trial;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = d.element_edit.iter().map(|e| e.0).collect();
        self.asm.sub.clear();
        self.scene_dirty = true;
        Ok(format!(
            "Spur Gears: rebuilt, centre distance {:.3} mm",
            g.center_distance
        ))
    }

    /// The generated element occurrence `occ` of the active assembly places
    /// (a part of this file made by a planned-panel generator): its document
    /// and stored input.
    pub(super) fn element_of(&self, occ: u32) -> Option<(DocumentId, crate::design::Element)> {
        let a = self.assembly()?;
        let o = a.find(occ)?;
        if o.component.library.is_some() {
            return None;
        }
        let doc = o.component.base().document;
        Some((doc, element_in(&self.project, doc)?))
    }

    /// The right-click menu's label to edit a generated element.
    pub(super) fn element_menu_label(&self, occ: u32) -> Option<&'static str> {
        let (_, e) = self.element_of(occ)?;
        Some(match e.command.as_str() {
            GEAR_ID => "Edit Spur Gears...",
            KEY_ID => "Edit Key...",
            BEARING_ID => "Edit Bearing...",
            SPRING_ID => "Edit Compression Spring...",
            _ => return None,
        })
    }

    /// Edit using the generator (Inventor: Edit using Design Accelerator): the
    /// generator's panel opens on the element's stored values; a gear brings
    /// its partner (the other gear of its motion link) along.
    pub(super) fn element_edit(&mut self, occ: u32) {
        let Some((doc, e)) = self.element_of(occ) else {
            return;
        };
        let kind = match e.command.as_str() {
            GEAR_ID => AsmPlanned::SpurGear,
            KEY_ID => AsmPlanned::Key,
            BEARING_ID => AsmPlanned::Bearing,
            SPRING_ID => AsmPlanned::CompressionSpring,
            _ => return,
        };
        let mut d = PartDialog::new(kind);
        d.set_values(&e.values);
        d.element_edit.push((occ, doc));
        if kind == AsmPlanned::SpurGear
            && let Some(a) = self.assembly()
            && let Some(m) = elements::axis_mate(a, occ)
        {
            let other = a.motion.links.iter().find_map(|l| {
                if l.driver == m {
                    Some(l.driven)
                } else if l.driven == m {
                    Some(l.driver)
                } else {
                    None
                }
            });
            let partner = other
                .and_then(|c| a.constraint(c))
                .map(|c| c.a.occurrence())
                .filter(|&o| o != occ);
            if let Some(o) = partner
                && let Some((pdoc, pe)) = self.element_of(o)
                && pe.command == GEAR_ID
            {
                d.element_edit.push((o, pdoc));
            }
        }
        d.active = None;
        self.part_planned = Some(d);
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
        if !d.element_edit.is_empty() {
            return self.key_rebuild(d);
        }
        let asm_id = self.part_id;
        let (mut place, radius) = self.axis_placement(d, "shaft", "start", false)?;
        let dia = radius.map_or_else(|| d.num(1, "Shaft Diameter").unwrap_or(40.0), |r| 2.0 * r);
        let k = key::size_for(dia)?;
        let l = d.num(1, "Key Length").unwrap_or(50.0);
        let ends = key_ends(d);
        let mut part = crate::design::gear_model::key_part(&k, l, ends)?;
        part.regenerate().map_err(|e| e.to_string())?;
        if let Some((_, msg)) = part.feature_health.iter().next() {
            return Err(msg.clone());
        }
        let mut element = crate::design::Element {
            command: KEY_ID.into(),
            values: d.values(),
            ..Default::default()
        };
        element.extra.insert("dia".into(), dia);
        part.element = Some(element);
        let keys = d
            .num(0, "Number of Keys")
            .unwrap_or(1.0)
            .round()
            .clamp(1.0, 2.0) as u32;
        let shaft_ref = d.refs.get("shaft").and_then(|v| v.first()).cloned();
        let start_ref = d.refs.get("start").and_then(|v| v.first()).cloned();
        let shaft_occ = shaft_ref
            .as_ref()
            .map(|r| r.occurrence())
            .filter(|&o| o != ASSEMBLY);
        // The key's X along the shaft part's X: a generated shaft's keyway
        // lies on its +X side.
        if let Some(o) = shaft_occ.and_then(|o| self.assembly()?.find(o)) {
            let n = place * Vector3::z();
            let sx = o.transform * Vector3::x();
            if let Some(x) = (sx - n * n.dot(&sx)).try_normalize(1e-9) {
                place = frame_zx(place * Point3::origin(), n, x);
            }
        }
        let mut trial = self.project.clone();
        let mut occs = Vec::new();
        for n in 0..keys {
            // A second key opposite the first.
            let turn = Transform3::new(
                Vector3::zeros(),
                Vector3::z() * (std::f64::consts::PI * f64::from(n)),
            );
            let floor = Transform3::new(Vector3::new(dia / 2.0 - k.t1, 0.0, 0.0), Vector3::zeros());
            occs.push(self.place_generated(
                &mut trial,
                part.clone(),
                "Key",
                place * turn * floor,
            )?);
        }
        // Held in the shaft's frame, its start on the start plane.
        let mut notes = Vec::new();
        if let Some(shaft) = shaft_occ {
            let start = match start_ref {
                Some(s) => s,
                None => elements::ucs_seat(&mut trial, asm_id, &place)?.1,
            };
            let mut a = asm_copy(&trial, asm_id)?;
            for &o in &occs {
                if elements::hold_key(&trial, &mut a, o, shaft, start.clone())?.is_none() {
                    notes.push("the shaft's axis is not its own Z: the key is placed without relationships");
                    break;
                }
            }
            put_asm(&mut trial, asm_id, a);
        }
        // The hub's groove: a gear made by Spur Gears gets it in its bore and
        // turns with the shaft.
        let hub_ref = d.refs.get("hub").and_then(|v| v.first()).cloned();
        if let Some(h) = hub_ref {
            let hub_radius = self.axis_placement(d, "hub", "", false)?.1;
            match self.groove_hub(&mut trial, h.occurrence(), shaft_occ, &k, place, hub_radius)? {
                Some(note) => notes.push(note),
                None => {
                    if let Some(DocumentData::Part(p)) = trial
                        .document_mut(part_doc_of(&trial, asm_id, occs[0])?)
                        .map(|x| &mut x.data)
                        && let Some(e) = &mut p.element
                    {
                        e.extra.insert("hub".into(), f64::from(h.occurrence()));
                    }
                }
            }
        }
        self.history.break_coalescing();
        self.checkpoint("Parallel Key");
        self.project = trial;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = occs;
        self.asm.sub.clear();
        self.scene_dirty = true;
        let mut text = format!(
            "Key: DIN 6885 A {} × {} × {l} placed for a Ø{dia:.1} shaft",
            k.b, k.h
        );
        for n in notes {
            text.push_str(&format!(" (Note: {n})"));
        }
        Ok(text)
    }

    /// Cuts the hub groove of key `k` into the hub occurrence `hub` when its
    /// part was made by Spur Gears (rebuilt with the groove in its bore,
    /// turned so the groove sits on the key at `key_place`, its turn on the
    /// shaft held, a motion link on its turn moved to the shaft's own axis
    /// Mate). A note when the hub is another part (left as it is).
    fn groove_hub(
        &self,
        trial: &mut crate::document::Project,
        hub: u32,
        shaft: Option<u32>,
        k: &key::KeySize,
        key_place: Transform3,
        hub_radius: Option<f64>,
    ) -> Result<Option<&'static str>, String> {
        let asm_id = self.part_id;
        let doc = part_doc_of(trial, asm_id, hub)?;
        let Some(old) = element_in(trial, doc).filter(|e| e.command == GEAR_ID) else {
            // Any other hub: the groove cut into its part through all, on a
            // UCS at the key's place in the hub's coordinates.
            let a = asm_copy(trial, asm_id)?;
            let t = a.find(hub).ok_or("the hub is gone")?.transform;
            let r = hub_radius.ok_or("pick the hub's bore (a cylindrical face)")?;
            let at = t.inverse() * key_place;
            let Some(DocumentData::Part(p)) = trial.document_mut(doc).map(|x| &mut x.data) else {
                return Err("the hub is not a part".into());
            };
            gear_parts::cut_hub_groove(p, &at, r, k.b, k.t2)?;
            return Ok(None);
        };
        let bore = old.extra.get("bore").copied().unwrap_or(0.0);
        if bore <= 0.0 {
            return Err(
                "the gear has no bore for the key (place it on the shaft's cylinder)".into(),
            );
        }
        let probe = PartDialog::from_values(AsmPlanned::SpurGear, &old.values);
        let pair = pair_of(&probe);
        let g = pair.geometry()?;
        let mut part =
            gear_parts::gear_part_keyed(&pair, &g, old.index.min(1), bore, Some((k.b, k.t2)))?;
        part.regenerate().map_err(|e| e.to_string())?;
        if let Some((_, msg)) = part.feature_health.iter().next() {
            return Err(msg.clone());
        }
        let mut e = old;
        e.extra.insert("hub_b".into(), k.b);
        e.extra.insert("hub_t2".into(), k.t2);
        part.element = Some(e);
        if let Some(x) = trial.document_mut(doc) {
            x.data = DocumentData::Part(part);
        }
        let mut a = asm_copy(trial, asm_id)?;
        // The groove (+X of the gear) onto the key's side.
        if let Some(o) = a.find_mut(hub) {
            let t = o.transform;
            let n = t * Vector3::z();
            let (gx, kx) = (t * Vector3::x(), key_place * Vector3::x());
            let turn = n.cross(&gx).dot(&kx).atan2(gx.dot(&kx));
            o.transform = t * Transform3::new(Vector3::zeros(), Vector3::z() * turn);
        }
        if let Some(m) = elements::axis_mate(&a, hub) {
            let v = a.constraint(m).and_then(|c| elements::value(trial, &a, c));
            if let (Some(v), Some(c)) = (v, a.constraint_mut(m)) {
                c.locked = true;
                c.value = v;
            }
            if let Some(s) = shaft.and_then(|s| elements::axis_mate(&a, s)) {
                elements::move_links(trial, &mut a, m, s)?;
            }
        }
        put_asm(trial, asm_id, a);
        Ok(None)
    }

    /// Edit Key's OK: the key's part rebuilt for the new size and length (its
    /// occurrences stay, their floor moved to the new groove depth); a gear
    /// it grooved gets the new groove.
    fn key_rebuild(&mut self, d: &PartDialog) -> Result<String, String> {
        let &(_, doc) = d.element_edit.first().ok_or("nothing to edit")?;
        let old = element_in(&self.project, doc).ok_or("the key's part no longer exists")?;
        let dia = old.extra.get("dia").copied().unwrap_or(40.0);
        let k = key::size_for(dia)?;
        let l = d.num(1, "Key Length").unwrap_or(50.0);
        let mut part = crate::design::gear_model::key_part(&k, l, key_ends(d))?;
        part.regenerate().map_err(|e| e.to_string())?;
        if let Some((_, msg)) = part.feature_health.iter().next() {
            return Err(msg.clone());
        }
        part.element = Some(crate::design::Element {
            values: d.values(),
            ..old.clone()
        });
        let mut trial = self.project.clone();
        if let Some(x) = trial.document_mut(doc) {
            x.data = DocumentData::Part(part);
        }
        self.history.break_coalescing();
        self.checkpoint("Edit Key");
        self.project = trial;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = d.element_edit.iter().map(|e| e.0).collect();
        self.asm.sub.clear();
        self.scene_dirty = true;
        Ok(format!("Key: rebuilt, {} × {} × {l}", k.b, k.h))
    }
}

/// The groove ends chosen on the panel.
fn key_ends(d: &PartDialog) -> Ends {
    if d.index(0, "Groove") == 1 {
        Ends::Square
    } else {
        Ends::Rounded
    }
}

/// The command ids of the planned-panel generators (`design::Element::command`).
pub(super) const GEAR_ID: &str = "design.spur_gear";
pub(super) const KEY_ID: &str = "design.key";
pub(super) const BEARING_ID: &str = "design.bearing";
pub(super) const SPRING_ID: &str = "design.compression_spring";

/// The Spur Gears panel's selectors of each gear: axis, start plane.
const GEAR_KEYS: [(&str, &str); 2] = [("cyl1", "start1"), ("cyl2", "start2")];

/// The stored input of part document `doc`, when a generator made it.
fn element_in(p: &crate::document::Project, doc: DocumentId) -> Option<crate::design::Element> {
    match &p.document(doc)?.data {
        DocumentData::Part(part) => part.element.clone(),
        _ => None,
    }
}

/// A copy of assembly `id` of `p`.
fn asm_copy(p: &crate::document::Project, id: DocumentId) -> Result<AssemblyDocument, String> {
    match p.document(id).map(|x| &x.data) {
        Some(DocumentData::Assembly(a)) => Ok(a.clone()),
        _ => Err("no assembly is open".into()),
    }
}

/// Puts assembly `a` back as document `id` of `p`.
fn put_asm(p: &mut crate::document::Project, id: DocumentId, a: AssemblyDocument) {
    if let Some(x) = p.document_mut(id) {
        x.data = DocumentData::Assembly(a);
    }
}

/// The part document occurrence `occ` of assembly `asm` places (this file's).
fn part_doc_of(
    p: &crate::document::Project,
    asm: DocumentId,
    occ: u32,
) -> Result<DocumentId, String> {
    let a = match p.document(asm).map(|x| &x.data) {
        Some(DocumentData::Assembly(a)) => a,
        _ => return Err("no assembly is open".into()),
    };
    let o = a.find(occ).ok_or("the component no longer exists")?;
    if o.component.library.is_some() {
        return Err("the component is a library part".into());
    }
    Ok(o.component.base().document)
}

/// A frame at `p` whose Z is `z` and X is `x` (square to each other).
fn frame_zx(p: Point3, z: Vector3, x: Vector3) -> Transform3 {
    let z = z.normalize();
    let x = (x - z * z.dot(&x)).normalize();
    let y = z.cross(&x);
    let r = nalgebra::Rotation3::from_basis_unchecked(&[x, y, z]);
    Transform3::from_parts(
        p.coords.into(),
        nalgebra::UnitQuaternion::from_rotation_matrix(&r),
    )
}

/// Seats occurrence `occ` of assembly `asm` on the picks of selectors
/// `axis_key` / `start_key` (relationships held as it stands at `place`); a
/// selector left empty takes a coordinate system of the assembly at `place`.
pub(super) fn seat_on(
    trial: &mut crate::document::Project,
    asm: DocumentId,
    d: &PartDialog,
    occ: u32,
    axis_key: &str,
    start_key: &str,
    place: &Transform3,
) -> Result<elements::Seated, String> {
    let axis = d.refs.get(axis_key).and_then(|v| v.first()).cloned();
    let start = d.refs.get(start_key).and_then(|v| v.first()).cloned();
    let (ax, st) = match (axis, start) {
        (Some(a), Some(s)) => (a, s),
        (a, s) => {
            let (ua, us) = elements::ucs_seat(trial, asm, place)?;
            (a.unwrap_or(ua), s.unwrap_or(us))
        }
    };
    let mut a = asm_copy(trial, asm)?;
    let s = elements::seat(trial, &mut a, occ, ax, st)?;
    put_asm(trial, asm, a);
    Ok(s)
}

/// The Bearing panel's list of the library's bearings that fit the shaft
/// (`Catalog::bearings_for`), found again when the diameter or the category
/// changes.
#[derive(Clone, Debug, Default)]
pub struct BearingList {
    /// What the rows were found for: (diameter in µm, category).
    key: Option<(i64, usize)>,
    /// The library they come from.
    catalog: Option<std::sync::Arc<crate::components::catalog::Catalog>>,
    /// (family, member, d, D, B, label, category).
    rows: Vec<BearingRow>,
    /// The row chosen.
    pub chosen: usize,
}

/// One bearing of [`BearingList`].
#[derive(Clone, Debug)]
struct BearingRow {
    family: usize,
    member: usize,
    label: String,
    category: String,
}

/// The fitting bearings with their icon (rule 23) under the Note row; a click
/// chooses one.
pub(super) fn bearing_list_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    plabel(ui, "Bearing");
    ui.vertical(|ui| {
        fit(ui);
        if d.bearing.rows.is_empty() {
            ui.label(
                RichText::new(format!(
                    "No bearing of the library has a Ø{} bore in this category",
                    d.num(0, "Diameter").unwrap_or(0.0)
                ))
                .weak()
                .italics(),
            );
            return;
        }
        let side = 18.0;
        for (i, r) in d.bearing.rows.iter().enumerate() {
            let on = i == d.bearing.chosen;
            let resp = ui
                .horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                    super::icons::paint_category(ui.painter(), rect, &r.category);
                    ui.selectable_label(on, &r.label)
                })
                .inner;
            if resp.clicked() {
                d.bearing.chosen = i;
            }
        }
    });
    ui.end_row();
}

impl super::CadCraftApp {
    /// Once a frame while a generator's panel is open: a picked cylinder gives
    /// the diameter its calculation and its list read (Bearing: Diameter and
    /// the library's fitting bearings; Key: Shaft Diameter).
    pub(super) fn design_follow(&mut self) {
        let Some(d) = self.part_planned.as_ref() else {
            return;
        };
        let (sel, row, tab) = match d.kind {
            Planned::Asm(AsmPlanned::Bearing) => ("cyl", "Diameter", 0),
            Planned::Asm(AsmPlanned::Key) if d.element_edit.is_empty() => {
                ("shaft", "Shaft Diameter", 1)
            }
            _ => return,
        };
        let radius = if d.refs.get(sel).is_some_and(|v| !v.is_empty()) {
            self.axis_placement(d, sel, "", false)
                .ok()
                .and_then(|(_, r)| r)
        } else {
            None
        };
        let Some(d) = self.part_planned.as_mut() else {
            return;
        };
        if let Some(r) = radius
            && d.num(tab, row).is_some_and(|v| (v - 2.0 * r).abs() > 1e-6)
        {
            d.set_num(tab, row, 2.0 * r);
        }
        if d.kind != Planned::Asm(AsmPlanned::Bearing) {
            return;
        }
        let dia = d.num(0, "Diameter").unwrap_or(0.0);
        let category = d.index(0, "Category");
        let key = ((dia * 1000.0).round() as i64, category);
        if d.bearing.key == Some(key) {
            return;
        }
        let catalog = match &d.bearing.catalog {
            Some(c) => c.clone(),
            None => {
                let Some(root) = crate::components::root_dir() else {
                    return;
                };
                std::sync::Arc::new(crate::components::catalog::Catalog::load(&root))
            }
        };
        let rows = catalog
            .bearings_for(dia, category)
            .into_iter()
            .map(|(f, m, b, od, w)| {
                let fam = &catalog.families[f];
                BearingRow {
                    family: f,
                    member: m,
                    label: format!(
                        "{} ({}): {} × {} × {}",
                        fam.members[m].size,
                        fam.name,
                        num(b),
                        num(od),
                        num(w)
                    ),
                    category: fam.category.clone(),
                }
            })
            .collect();
        d.bearing = BearingList {
            key: Some(key),
            catalog: Some(catalog),
            rows,
            chosen: 0,
        };
    }

    /// Bearing OK: the bearing chosen from the library (linked, so the
    /// assembly must be saved) placed on the shaft's cylinder from the start
    /// plane, held there by an axis Mate and its start plane. One Undo step.
    pub(super) fn bearing_ok(&mut self, d: &PartDialog) -> bool {
        match self.bearing_make(d) {
            Ok(text) => {
                self.file_status = Some(Ok(text));
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Bearing: {e}")));
                false
            }
        }
    }

    fn bearing_make(&mut self, d: &PartDialog) -> Result<String, String> {
        if self.file_path.trim().is_empty() {
            return Err(
                "save the assembly first (the bearing is linked from the library by a path relative to it)"
                    .into(),
            );
        }
        let row = d.bearing.rows.get(d.bearing.chosen).ok_or(
            "no bearing of the library fits; pick the shaft's cylinder or change the Diameter",
        )?;
        let catalog = d
            .bearing
            .catalog
            .as_ref()
            .ok_or("the library is not loaded")?;
        let path = catalog.member_file_at(
            row.family,
            row.member,
            crate::components::samples::Detail::Standard,
        )?;
        let file = self.part_file_pub(&path).map_err(|e| e.to_string())?;
        let doc = file
            .documents()
            .iter()
            .find(|x| matches!(x.data, DocumentData::Part(_)))
            .map(|x| x.id)
            .ok_or("the bearing's file holds no part")?;
        let (place, _) = self.axis_placement(d, "cyl", "start", false)?;
        let asm_id = self.part_id;
        let owner = std::path::PathBuf::from(self.file_path.trim());
        let mut trial = self.project.clone();
        let mut resolver = crate::document::Resolver::new();
        let r = resolver
            .link(&mut trial, &owner, &path, doc)
            .map_err(|e| e.to_string())?;
        let name = catalog.families[row.family].members[row.member]
            .size
            .clone();
        let mut a = asm_copy(&trial, asm_id)?;
        let first = a.occurrences.is_empty();
        let picked = ["cyl", "start"]
            .iter()
            .any(|k| d.refs.get(k).is_some_and(|v| !v.is_empty()));
        let occ = a.place(r, &name, place, first && !picked);
        put_asm(&mut trial, asm_id, a);
        if picked {
            // The linked part's origin is read through the app's cache.
            seat_on(&mut trial, asm_id, d, occ, "cyl", "start", &place)?;
        }
        self.history.break_coalescing();
        self.checkpoint("Bearing");
        self.project = trial;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = vec![occ];
        self.asm.sub.clear();
        self.scene_dirty = true;
        Ok(format!("Bearing: {} placed", row.label))
    }

    /// Compression Spring OK: the spring's part (one Coil, `spring_part`) on
    /// its axis from the start plane (Edit: rebuilt in place). One Undo step.
    pub(super) fn spring_ok(&mut self, d: &PartDialog) -> bool {
        match self.spring_make(d) {
            Ok(text) => {
                self.file_status = Some(Ok(text));
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Compression Spring: {e}")));
                false
            }
        }
    }

    fn spring_make(&mut self, d: &PartDialog) -> Result<String, String> {
        let s = spring_of(d);
        let mut part = gear_parts::spring_part(&s)?;
        part.regenerate().map_err(|e| e.to_string())?;
        if let Some((_, msg)) = part.feature_health.iter().next() {
            return Err(msg.clone());
        }
        part.element = Some(crate::design::Element {
            command: SPRING_ID.into(),
            values: d.values(),
            ..Default::default()
        });
        let asm_id = self.part_id;
        let mut trial = self.project.clone();
        let occs: Vec<u32> = if d.element_edit.is_empty() {
            let (place, _) = self.axis_placement(d, "axis", "start", false)?;
            let occ = self.place_generated(&mut trial, part, "Compression Spring", place)?;
            if ["axis", "start"]
                .iter()
                .any(|k| d.refs.get(k).is_some_and(|v| !v.is_empty()))
            {
                seat_on(&mut trial, asm_id, d, occ, "axis", "start", &place)?;
            }
            vec![occ]
        } else {
            for &(_, doc) in &d.element_edit {
                if let Some(x) = trial.document_mut(doc) {
                    x.data = DocumentData::Part(part.clone());
                }
            }
            d.element_edit.iter().map(|e| e.0).collect()
        };
        self.history.break_coalescing();
        self.checkpoint("Compression Spring");
        self.project = trial;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = occs;
        self.asm.sub.clear();
        self.scene_dirty = true;
        Ok(format!(
            "Compression Spring: Ø{} wire, {} active coils, {} mm free length",
            num(s.wire),
            num(s.active),
            num(s.free_length)
        ))
    }
}

/// The spring of the panel's Design and Calculation values.
fn spring_of(d: &PartDialog) -> spring::Spring {
    let n0 = |l: &str, def: f64| d.num(0, l).unwrap_or(def);
    let wire = n0("Wire Diameter", 2.0);
    spring::Spring {
        wire,
        mean_diameter: n0("Outer Diameter", 20.0) - wire,
        active: n0("Active Coils", 6.0),
        free_length: n0("Free Length", 50.0),
        ends: match d.index(0, "Ends") {
            1 => spring::Ends::Closed,
            2 => spring::Ends::Open,
            _ => spring::Ends::ClosedGround,
        },
        g: d.num(1, "Shear Modulus G (MPa)")
            .unwrap_or(spring::G_SPRING_STEEL),
    }
}

/// A number without trailing zeros.
fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}
