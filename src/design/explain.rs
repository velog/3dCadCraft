//! Explanations of the calculating generators (spec 6.2-design-calculators
//! §3.6, rule 0-ui §4.3): for every input and result of Shaft, Spur Gears,
//! Key, Bearing and Compression Spring what it is and what it is for, its
//! symbol and unit, the usual values and how to choose them, what a value too
//! low or too high does; for every result its formula, the symbols, the
//! source (the free documents of spec §0) and the limits. Our own words; the
//! GUI shows them beside the fields (`gui/explain.rs`), Bulgarian through the
//! interface's translation.
//!
//! Keys: the generator's command id (`design.shaft`) and the field's label as
//! the panel shows it.

/// The generators that calculate (their command ids).
pub const SHAFT: &str = "design.shaft";
/// Spur Gears.
pub const GEARS: &str = "design.spur_gear";
/// Parallel Key.
pub const KEY: &str = "design.key";
/// Bearing.
pub const BEARING: &str = "design.bearing";
/// Compression Spring.
pub const SPRING: &str = "design.compression_spring";

/// What a field is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The generator's command id.
    pub generator: &'static str,
    /// The field's label.
    pub id: &'static str,
    /// Its symbol in the formulas (empty: none).
    pub symbol: &'static str,
    /// Its unit (empty: none).
    pub unit: &'static str,
    /// What it is and what it is for.
    pub what: &'static str,
    /// Usual values and how to choose them.
    pub typical: &'static str,
    /// What a value too low (or the other choice) does.
    pub low: &'static str,
    /// What a value too high does.
    pub high: &'static str,
}

/// How a result is computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Formula {
    /// The generator's command id.
    pub generator: &'static str,
    /// The result's label.
    pub id: &'static str,
    /// The formula, readable Unicode.
    pub formula: &'static str,
    /// The symbols.
    pub symbols: &'static str,
    /// Where it comes from.
    pub source: &'static str,
    /// Assumptions and limits.
    pub limits: &'static str,
}

/// The entry of input `id` of `generator`.
pub fn input(generator: &str, id: &str) -> Option<&'static Entry> {
    INPUTS
        .iter()
        .find(|e| e.generator == generator && e.id == id)
}

/// The entry of result `id` of `generator`.
pub fn result(generator: &str, id: &str) -> Option<&'static Entry> {
    RESULTS
        .iter()
        .find(|e| e.generator == generator && e.id == id)
}

/// The formula of result `id` of `generator`.
pub fn formula(generator: &str, id: &str) -> Option<&'static Formula> {
    FORMULAS
        .iter()
        .find(|f| f.generator == generator && f.id == id)
}

/// Every formula of `generator`, in the order of its results.
pub fn formulas(generator: &str) -> impl Iterator<Item = &'static Formula> + '_ {
    FORMULAS.iter().filter(move |f| f.generator == generator)
}

/// The calculation's assumptions and limits as a whole.
pub fn method(generator: &str) -> &'static str {
    match generator {
        SHAFT => {
            "Beam theory along the shaft (Euler–Bernoulli: plane sections, small deflections, shear deflection left out); \
             sections are treated as round (a polygon by its inscribed circle). Fatigue by NASA RP-1123: the distortion-energy \
             ellipse of reversed bending with steady torque (ASME B106.1M), the specimen's limit corrected by factors. DIN 743 \
             (Inventor's European method) uses other factors: its results differ by up to some tens of per cent."
        }
        GEARS => {
            "Involute spur and helical gears of a standard rack (NASA RP-1152 §2–3; profile shift by the involute function). \
             Bending: the Lewis / AGMA 218.01 form, eq. 53, with J laid out from the generated tooth. Surface: the NASA \
             Lundberg–Palmgren life model (eq. 65–75), a life rather than an ISO 6336 safety factor."
        }
        KEY => {
            "A parallel key carries the torque as a force at the shaft's surface; it is checked in shear across its width and \
             in crushing on the part standing in the hub (Kimball and Barr §69). Uneven bearing along the key and the hub's \
             own strength are not checked."
        }
        BEARING => {
            "The basic rating life of NASA RP-1105 §12 (the Lundberg–Palmgren theory ISO 281 rates with); the life at another \
             reliability by the Weibull slope; ISO 281's modified life (a_ISO, lubrication and contamination) is not in the \
             source and stays out."
        }
        SPRING => {
            "Round wire in torsion only (Kimball and Barr §43): the direct shear and the curvature of the wire are left out, as \
             the source does. For a spring index below about 5 the true stress is markedly higher (Wahl's later correction)."
        }
        _ => "",
    }
}

macro_rules! e {
    ($g:expr, $id:expr, $sym:expr, $unit:expr, $what:expr, $typ:expr, $low:expr, $high:expr) => {
        Entry {
            generator: $g,
            id: $id,
            symbol: $sym,
            unit: $unit,
            what: $what,
            typical: $typ,
            low: $low,
            high: $high,
        }
    };
}

macro_rules! f {
    ($g:expr, $id:expr, $f:expr, $sym:expr, $src:expr, $lim:expr) => {
        Formula {
            generator: $g,
            id: $id,
            formula: $f,
            symbols: $sym,
            source: $src,
            limits: $lim,
        }
    };
}

const RP1123: &str = "S. H. Loewenthal, Design of Power-Transmitting Shafts, NASA RP-1123 (1984)";
const RP1152: &str = "J. J. Coy, D. P. Townsend, E. V. Zaretsky, Gearing, NASA RP-1152 (1985)";
const RP1105: &str = "B. J. Hamrock, W. J. Anderson, Rolling-Element Bearings, NASA RP-1105 (1983)";
const KB: &str = "D. S. Kimball, J. H. Barr, Elements of Machine Design (1909)";

/// Every input.
pub const INPUTS: &[Entry] = &[
    // ------------------------------------------------------------- Shaft
    e!(
        SHAFT,
        "Ultimate Strength σu",
        "σu",
        "MPa",
        "The tensile strength of the shaft's material, from a tensile test or the material's sheet. The surface factor charts are read with it.",
        "Mild steel (S235, 1020) 360–470; medium-carbon steel (C45, 1045) 570–700; quenched and tempered alloy steel (42CrMo4, 4340) 900–1100.",
        "Too low: the shaft comes out heavier than it needs to be.",
        "Too high (more than the bar really has): the safety factor is overstated."
    ),
    e!(
        SHAFT,
        "Yield Strength σy",
        "σy",
        "MPa",
        "The stress where the material starts to yield. The steady torque term of the fatigue equation divides by it.",
        "Mild steel 235–350; C45 normalised 340–430; 42CrMo4 quenched and tempered 650–900.",
        "Too low: torque-dominated shafts come out too thick.",
        "Too high: the shaft may yield under the steady torque before fatigue counts."
    ),
    e!(
        SHAFT,
        "Fatigue Limit σ'f",
        "σ'f",
        "MPa",
        "The endurance limit of a small polished specimen in rotating bending; the factors ka … ki reduce it to the real shaft's limit σf.",
        "For steels about half the ultimate strength (0.5·σu) up to σu ≈ 1400 MPa, about 700 MPa above.",
        "Too low: conservative, a thicker shaft.",
        "Too high: the shaft may break by fatigue at a safety factor that looks sufficient."
    ),
    e!(
        SHAFT,
        "Modulus of Elasticity E",
        "E",
        "MPa",
        "The material's stiffness in tension and bending; only the deflections, slopes and the critical speed depend on it, not the stresses.",
        "Steel 200 000–210 000; cast iron 100 000–140 000; aluminium 70 000; titanium 110 000.",
        "Too low: deflections and critical speed look worse than they are.",
        "Too high: the shaft bends more and its critical speed is lower than shown."
    ),
    e!(
        SHAFT,
        "Modulus of Rigidity G",
        "G",
        "MPa",
        "The shear modulus; the angle of twist depends on it.",
        "Steel 79 000–81 000; aluminium 26 000; G ≈ E / (2 (1 + ν)) with ν ≈ 0.3.",
        "Too low: the twist is overstated.",
        "Too high: the shaft twists more than shown."
    ),
    e!(
        SHAFT,
        "Density ρ",
        "ρ",
        "kg/m³",
        "The material's density: the mass, the own weight as a load and the critical speed.",
        "Steel 7850; cast iron 7200; aluminium 2700; bronze 8800.",
        "Too low: mass and own-weight sag understated.",
        "Too high: mass overstated, critical speed understated."
    ),
    e!(
        SHAFT,
        "Hardened above 200 BHN (keyway factors)",
        "",
        "",
        "Whether the steel is hardened above 200 Brinell; the keyway's fatigue factor (RP-1123 Table V) is lower for hardened steels, whose notch sensitivity is higher.",
        "On for quenched and tempered or case-hardened shafts; off for annealed, normalised or plain carbon steel bars.",
        "Off on a hardened shaft: the keyway's effect is understated.",
        "On on a soft shaft: conservative."
    ),
    e!(
        SHAFT,
        "Use density (own weight)",
        "",
        "",
        "Adds the shaft's own weight as a load spread along it (gravity along the load angle 0).",
        "On for long or heavy shafts and always for the critical speed; off when the shaft stands upright.",
        "Off: the sag and the bending from the own weight are left out.",
        "On for an upright shaft: a load that does not exist."
    ),
    e!(
        SHAFT,
        "Number of shaft divisions",
        "n",
        "",
        "How many pieces the beam calculation cuts the shaft into; the curves and maxima are read at their ends.",
        "400 (the default) for ordinary shafts; 1000–2000 for long shafts with short grooves.",
        "Too few: a short step or groove can be missed, maxima are coarse.",
        "Too many: only slower."
    ),
    e!(
        SHAFT,
        "Reduced stress",
        "σred",
        "MPa",
        "How bending and torsional stress are combined into one; HMH (distortion energy, von Mises) or Tresca–Guest (maximum shear).",
        "HMH for ductile steels (recommended); Tresca is about 15 % more conservative.",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Surface factor ka (Fig. 3)",
        "ka",
        "",
        "Reduces the fatigue limit for the surface finish; rough surfaces start cracks earlier. Read it from RP-1123 Fig. 3 for the finish and σu.",
        "Ground 0.85–0.9; machined or cold drawn 0.7–0.8; hot rolled 0.5–0.7; as forged 0.35–0.55 (lower for stronger steels).",
        "Too low: conservative.",
        "Too high: fatigue strength overstated, typical for an unground high-strength shaft."
    ),
    e!(
        SHAFT,
        "Reliability kc (Table III)",
        "kc",
        "",
        "The share of shafts that must survive; the fatigue limit scatters about 8 % (one standard deviation), so higher reliability takes a lower limit.",
        "50 % 1.0; 90 % 0.897; 99 % 0.814; 99.9 % 0.753. 99 % or 99.9 % for machine design.",
        "Lower reliability: more shafts fail than you expect.",
        "Higher: a thicker shaft."
    ),
    e!(
        SHAFT,
        "Temperature factor kd (Table IV)",
        "kd",
        "",
        "Reduces the fatigue limit for a hot shaft.",
        "1.0 at ordinary temperatures; take the source's Table IV for shafts running hot.",
        "—",
        "1.0 on a hot shaft overstates its strength."
    ),
    e!(
        SHAFT,
        "Other factors kf·kg·kh·ki",
        "kf·kg·kh·ki",
        "",
        "The product of the remaining factors of eq. 19: press fit (fretting under a hub), residual stresses, corrosion and others.",
        "1.0 when none applies; a press-fitted hub or a corrosive medium lowers it considerably (see the source's discussion).",
        "—",
        "1.0 where a press fit or corrosion acts overstates the strength."
    ),
    e!(
        SHAFT,
        "Stress concentration Kt",
        "Kt",
        "",
        "The theoretical stress concentration of the worst notch (shoulder fillet, groove, cross hole); ke follows from it and q.",
        "Plain shaft 1.0; shoulder with r/d ≈ 0.1 about 1.4–1.7; sharp shoulder 2–2.7; retaining-ring groove 3–5.",
        "Too low: the notch is underrated, the shaft can break at a shoulder.",
        "Too high: conservative."
    ),
    e!(
        SHAFT,
        "Notch sensitivity q (Fig. 5)",
        "q",
        "",
        "How much of Kt the material feels; 0 none, 1 all of it. Read it from RP-1123 Fig. 5 for the notch radius and the steel.",
        "Steels with a notch radius of 1–3 mm 0.7–0.95, higher for stronger steels; 1.0 when unsure.",
        "Too low: the notch is underrated.",
        "Too high: conservative."
    ),
    e!(
        SHAFT,
        "Required safety factor",
        "FS",
        "",
        "The fatigue safety factor the shaft must reach at its weakest place.",
        "1.3–1.5 with well-known loads and material data; 1.5–2 in ordinary machine design; 2–3 with shocks or uncertain loads.",
        "Too low: little margin for what the calculation does not know.",
        "Too high: an oversized shaft."
    ),
    e!(
        SHAFT,
        "Support",
        "z",
        "mm",
        "A radial support (a bearing) at a place along the shaft. Fixed also takes the axial load, Free only radial load. Put it at the bearing's middle.",
        "Two supports for most shafts, one of them Fixed; a third makes the shaft statically indeterminate.",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Force",
        "F",
        "N",
        "A radial force at a place, its direction in the cross-section by the angle: a gear's tooth force, a belt's pull, a weight.",
        "From the elements on the shaft: a spur gear Wt = 2T/d and Wr = Wt·tan α; a belt 1.5–2.5 × the transmitted force.",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Axial Force",
        "Fa",
        "N",
        "A force along the axis (a helical gear's thrust, a preload), taken by the Fixed support.",
        "A helical gear Wa = Wt·tan β.",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Continuous Load",
        "q",
        "N/mm",
        "A load spread over a length of the shaft, its direction by the angle.",
        "A roller's or drum's weight per length; a belt wrapped over a long pulley.",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Torque",
        "T",
        "N·m",
        "A torque entering the shaft at one place and leaving at another (the pair sums to zero): the transmitted power.",
        "T = P / ω = 9549 · P[kW] / n[rpm] N·m.",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Bending Moment",
        "M",
        "N·m",
        "A moment at a place, bending the shaft in the plane of its angle: a helical gear's thrust acting at its pitch radius.",
        "Wa · d/2 of a helical gear.",
        "—",
        "—"
    ),
    // ------------------------------------------------------- Spur Gears
    e!(
        GEARS,
        "Design Guide",
        "",
        "",
        "Which value follows from the others; Center Distance from module and teeth, or the unit corrections from a given centre distance.",
        "Center Distance for a new pair; Total Unit Correction when the shafts' distance is fixed.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Pressure Angle",
        "α",
        "°",
        "The angle of the rack's flank (the line of action): it sets the tooth's shape for both gears.",
        "20° is the standard (ISO 53); 14.5° in old machines; 25° for stronger, noisier teeth.",
        "Smaller: thinner tooth roots, more undercut, smoother running.",
        "Larger: stronger teeth, higher bearing loads (Wr = Wt·tan α), lower contact ratio."
    ),
    e!(
        GEARS,
        "Helix Angle",
        "β",
        "°",
        "The teeth's angle to the axis; 0 for spur gears. The transverse module grows to mn / cos β.",
        "0° spur; 8–20° for helical gears (smoother and quieter, with an axial force Wt·tan β).",
        "—",
        "Larger: more axial thrust on the bearings."
    ),
    e!(
        GEARS,
        "Module",
        "m",
        "mm",
        "The tooth size; the pitch diameter is m·z, the tooth height about 2.25·m.",
        "Preferred series (ISO 54): 1, 1.25, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10, 12, 16, 20. Choose it from the bending stress: double the module, roughly a quarter the stress.",
        "Too small: the teeth break in bending.",
        "Too large: big, heavy gears and fewer teeth (less smooth)."
    ),
    e!(
        GEARS,
        "Desired Gear Ratio",
        "i",
        "",
        "The ratio z2 / z1 you aim at.",
        "1–6 for one spur pair; more needs two stages.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Center Distance",
        "a",
        "mm",
        "The distance between the shafts; with Design Guide Total Unit Correction the shifts follow from it.",
        "a = m (z1 + z2) / 2 for gears without shift.",
        "Smaller than the standard: negative shift, weaker teeth.",
        "Larger: positive shift, pointed tips at the extreme."
    ),
    e!(
        GEARS,
        "Gear 1",
        "",
        "",
        "What is made for the first gear; Component (a part placed in the assembly), Feature or No Model.",
        "Component.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Gear 2",
        "",
        "",
        "What is made for the second gear.",
        "Component.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Number of Teeth 1",
        "z1",
        "",
        "The pinion's teeth.",
        "At 20° at least 17 without undercut (14 with positive shift); 18–30 usual for a pinion.",
        "Too few: undercut, weak roots, low contact ratio.",
        "Too many at a given size: a small module, weak in bending."
    ),
    e!(
        GEARS,
        "Number of Teeth 2",
        "z2",
        "",
        "The wheel's teeth; z2 / z1 is the ratio.",
        "Keep z1 and z2 without a common factor (hunting teeth) for even wear.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Unit Correction 1",
        "x1",
        "",
        "Profile shift of gear 1 in modules; the rack is moved out (+) or in (−) while generating the teeth.",
        "0 for standard gears; +0.3 … +0.6 against undercut on small pinions; x1 + x2 sets the centre distance.",
        "Too negative: undercut, thin roots.",
        "Too positive: pointed tips (check the tip thickness)."
    ),
    e!(
        GEARS,
        "Unit Correction 2",
        "x2",
        "",
        "Profile shift of gear 2 in modules.",
        "Often −x1 (the centre distance stays standard).",
        "Too negative: undercut.",
        "Too positive: pointed tips."
    ),
    e!(
        GEARS,
        "Facewidth 1",
        "b1",
        "mm",
        "The width of gear 1's teeth.",
        "8–16 × the module; usually no wider than the pinion's pitch diameter. The pinion is often a little wider than the wheel.",
        "Too narrow: high bending and contact stress.",
        "Too wide: the load gathers at one end (Km grows) unless the shafts are stiff and true."
    ),
    e!(
        GEARS,
        "Facewidth 2",
        "b2",
        "mm",
        "The width of gear 2's teeth.",
        "As Facewidth 1.",
        "Too narrow: high stress.",
        "Too wide: uneven load."
    ),
    e!(
        GEARS,
        "Power (kW)",
        "P",
        "kW",
        "The power the pair transmits.",
        "The motor's rated power times the service factors you use.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Speed (rpm)",
        "n1",
        "rpm",
        "The pinion's (gear 1's) speed.",
        "Motor speeds 750–3000 rpm; the pitch-line velocity follows.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Accuracy Level (AGMA)",
        "Qv",
        "",
        "The gears' manufacturing quality; the dynamic factor Kv of RP-1152 Fig. 25 follows from it and the pitch-line velocity.",
        "5–7 commercial cut gears; 8–9 good shaved or ground gears; 10–11 precision ground.",
        "Lower: a larger dynamic load (Kv smaller).",
        "Higher than made: dynamic load understated."
    ),
    e!(
        GEARS,
        "Load at the Tip",
        "",
        "",
        "Takes the tooth load at the tip instead of at the highest point of single-tooth contact (where it really acts on accurate gears).",
        "Off for accurate gears; on for coarse gears where the load sharing between teeth is not sure.",
        "—",
        "On: conservative."
    ),
    e!(
        GEARS,
        "Geometry Factor J (helical)",
        "J",
        "",
        "The bending geometry factor of a helical gear; for spur gears J is laid out from the tooth and this value is not used.",
        "0.4–0.6 for helical gears of 20° normal pressure angle (AGMA charts).",
        "Too low: conservative.",
        "Too high: bending stress understated."
    ),
    e!(
        GEARS,
        "Application Factor Ka",
        "Ka",
        "",
        "Raises the nominal load for shocks from the driving and the driven machine (RP-1152 Table 8).",
        "1.0 uniform motor and load; 1.25 moderate shock; 1.5–1.75 heavy shock; up to 2.25 with a shock-loaded engine drive.",
        "Too low: shock loads break teeth.",
        "Too high: oversized gears."
    ),
    e!(
        GEARS,
        "Load Distribution Factor Km",
        "Km",
        "",
        "Raises the load for its uneven spread across the face (shaft deflection, misalignment), RP-1152 Table 9.",
        "1.3 for accurate mountings and faces below 50 mm; 1.6–2 for less accurate mountings or wide faces.",
        "Too low: one end of the tooth overloaded.",
        "Too high: conservative."
    ),
    e!(
        GEARS,
        "Allowable Bending Stress (MPa)",
        "σall",
        "MPa",
        "The bending stress the gear's material may take for the life required.",
        "Through-hardened steel 200–300; case-hardened steel 380–450; cast iron 50–70.",
        "Too low: conservative.",
        "Too high: teeth break in bending."
    ),
    e!(
        GEARS,
        "Life Factor (material)",
        "",
        "",
        "Scales the NASA surface life for the material; 1 for the vacuum-melted AISI 9310 steel of the NASA tests.",
        "1 for premium carburised steels; lower (0.2–0.5) for ordinary air-melted steels.",
        "—",
        "Too high: pitting earlier than predicted."
    ),
    e!(
        GEARS,
        "Reliability (%)",
        "S",
        "%",
        "The share of gears that must reach the life shown.",
        "90 % (the L10 life) as usual; 99 % for critical drives.",
        "—",
        "Higher: a shorter life is shown."
    ),
    // ------------------------------------------------------------ Key
    e!(
        KEY,
        "Key",
        "",
        "",
        "The standard of the key's sizes.",
        "DIN 6885 A (= ISO/R 773) parallel keys.",
        "—",
        "—"
    ),
    e!(
        KEY,
        "Groove",
        "",
        "",
        "The shaft groove's ends: rounded (form A, end-milled) or square (form B, side-milled). A rounded key bears only over l − b.",
        "Rounded for a groove inside the shaft; square at a shaft end.",
        "—",
        "—"
    ),
    e!(
        KEY,
        "Number of Keys",
        "i",
        "",
        "One key, or two opposite each other (each carries less than half: 0.75 of twice one key is counted).",
        "1; 2 when one key would be longer than about 1.5 d.",
        "—",
        "—"
    ),
    e!(
        KEY,
        "Torque (N·m)",
        "T",
        "N·m",
        "The torque the key carries.",
        "The shaft's highest torque including shocks.",
        "—",
        "—"
    ),
    e!(
        KEY,
        "Shaft Diameter",
        "d",
        "mm",
        "The shaft's diameter at the key; it decides the key's size b × h from the standard (taken from the picked cylinder).",
        "—",
        "—",
        "—"
    ),
    e!(
        KEY,
        "Key Length",
        "l",
        "mm",
        "The key's overall length.",
        "1–1.5 × d; no longer than the hub; from the standard series (shortest length for the pressure is shown).",
        "Too short: the hub crushes.",
        "Too long: longer than the hub, uneven bearing."
    ),
    e!(
        KEY,
        "Allowed Shear (MPa)",
        "τall",
        "MPa",
        "The shear stress the key's steel may take.",
        "60–90 for keys of C45 / 1045; lower with shocks.",
        "Too low: conservative.",
        "Too high: the key shears off."
    ),
    e!(
        KEY,
        "Allowed Pressure (MPa)",
        "pall",
        "MPa",
        "The pressure the weakest of key, shaft and hub may take on the key's side.",
        "Steel hub 100–150 (steady) / 60–90 (shocks); cast-iron hub 50–80.",
        "Too low: conservative.",
        "Too high: the groove's sides are crushed."
    ),
    // --------------------------------------------------------- Bearing
    e!(
        BEARING,
        "Diameter",
        "d",
        "mm",
        "The bearing's bore = the shaft's diameter there (taken from the picked cylinder).",
        "—",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Outside Diameter",
        "D",
        "mm",
        "The bearing's outside diameter (the housing's bore).",
        "From the series: wider series of one bore carry more.",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Width",
        "B",
        "mm",
        "The bearing's width.",
        "From the series.",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Category",
        "",
        "",
        "Which bearings the library lists; radial ball, radial roller or thrust bearings.",
        "Ball bearings for moderate loads and high speeds; roller bearings for heavy radial loads; thrust bearings for axial loads only.",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Bearing Type",
        "m",
        "",
        "Ball (life exponent 3) or roller (10/3) for the life calculation.",
        "As the bearing chosen.",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Radial Load (N)",
        "Fr",
        "N",
        "The radial load on the bearing; the shaft's support reaction there.",
        "Take it from the Shaft Generator's Support Reaction.",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Axial Load (N)",
        "Fa",
        "N",
        "The axial load on the bearing.",
        "A helical gear's thrust, a preload.",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Speed (rpm)",
        "n",
        "rpm",
        "The speed of the bearing's rotating ring.",
        "—",
        "—",
        "Above the bearing's limiting speed the life calculation no longer holds."
    ),
    e!(
        BEARING,
        "Basic Dynamic Load Rating C (N)",
        "C",
        "N",
        "The load at which 90 % of bearings reach one million revolutions: the bearing's capacity, from its maker's catalogue.",
        "Deep-groove ball 6204 about 13 000 N, 6304 about 16 000 N (typical catalogue values).",
        "Too low: conservative.",
        "Too high (another maker's value): the life is overstated."
    ),
    e!(
        BEARING,
        "Radial Factor X",
        "X",
        "",
        "Weights the radial load in the equivalent load Fe = X·Fr + Y·Fa.",
        "1 when Fa / Fr is small; 0.56 for deep-groove ball bearings above the limit e (catalogue tables).",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Axial Factor Y",
        "Y",
        "",
        "Weights the axial load in Fe.",
        "0 for small Fa / Fr; 1–2.3 for deep-groove ball bearings above e (catalogue tables).",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Life Adjustment Factors (D·E·F·G·H)",
        "D·E·F·G·H",
        "",
        "RP-1105 §12.5: material, processing, lubrication, speed and misalignment factors multiplying L10.",
        "1 for ordinary conditions; above 1 with premium steels and good lubrication, below 1 with poor lubrication or misalignment.",
        "—",
        "Too high: the life is overstated."
    ),
    e!(
        BEARING,
        "Required Reliability (%)",
        "S",
        "%",
        "The share of bearings that must reach the life.",
        "90 % (L10) as usual; 95–99 % for critical machines.",
        "—",
        "Higher: a shorter life shown."
    ),
    e!(
        BEARING,
        "Required Life (h)",
        "Lh",
        "h",
        "The hours the bearing must run.",
        "Household appliances 1 000–2 000; machines used 8 h a day 20 000–30 000; continuous duty 40 000–60 000; for 24 h service 100 000.",
        "Too low: the bearing wears out early.",
        "Too high: an oversized bearing."
    ),
    // ---------------------------------------------------------- Spring
    e!(
        SPRING,
        "Wire Diameter",
        "d",
        "mm",
        "The wire's diameter: rate and stress depend on it strongly (rate ∝ d⁴, stress ∝ 1 / d³).",
        "From the wire maker's series; choose it from the stress at the working load.",
        "Too thin: overstressed.",
        "Too thick: too stiff, solid too soon."
    ),
    e!(
        SPRING,
        "Outer Diameter",
        "Do",
        "mm",
        "The coil's outside diameter; the mean diameter D = Do − d.",
        "Spring index D / d between 4 and 12 (6–9 easiest to make); leave clearance to a bore or a guide pin.",
        "Index below 4: hard to coil, high real stress.",
        "Index above 12: buckles and tangles."
    ),
    e!(
        SPRING,
        "Free Length",
        "L0",
        "mm",
        "The spring's length without load.",
        "Up to about 4 × D without a guide (buckling).",
        "Too short: solid before the working load.",
        "Too long: buckles without a guide."
    ),
    e!(
        SPRING,
        "Active Coils",
        "n",
        "",
        "The coils that spring (the inactive end coils are added by the Ends).",
        "At least 3; usually 5–20.",
        "Few coils: stiff, sensitive to the end coils.",
        "Many coils: soft, long, may buckle."
    ),
    e!(
        SPRING,
        "Ends",
        "",
        "",
        "How the ends are made; closed and ground, closed, or open. Closed ends add one inactive coil each.",
        "Closed and ground for an even seat (most compression springs).",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Working Load (N)",
        "P",
        "N",
        "The load the spring carries in its working position.",
        "—",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Shear Modulus G (MPa)",
        "G",
        "MPa",
        "The wire's shear modulus; the rate is proportional to it.",
        "Spring steel 79 000–82 700 (the source's 12 000 000 psi = 82 737); stainless 69 000; phosphor bronze 41 000.",
        "Too low: the spring is stiffer than shown.",
        "Too high: softer than shown."
    ),
    e!(
        SPRING,
        "Allowed Stress (MPa)",
        "pall",
        "MPa",
        "The shear stress the wire may take; 0 takes the source's safe stress for steel wire, eq. 12 (275.8 + 2627 / d).",
        "0 (the book's value), or the wire maker's value for its grade.",
        "Too low: conservative.",
        "Too high: the spring sets (shortens) or breaks."
    ),
    // Options of Inventor's generator dialogs (spec §3.7)
    e!(
        GEARS,
        "Internal",
        "",
        "",
        "An internal gear pair (the wheel's teeth inside a ring).",
        "Off for external spur gears.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Size Type",
        "",
        "",
        "Whether the tooth size is given as a module (mm) or a diametral pitch (teeth per inch).",
        "Module for metric gears; P = 25.4 / m.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Helix Angle Flip",
        "",
        "",
        "Swaps the hand of the helix (left / right) of a helical pair.",
        "Choose the hand that sends the axial force to the bearing meant to take it.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Unit Correction Distribution",
        "",
        "",
        "How the total unit correction is shared between the gears.",
        "In proportion to the teeth (ours), equally, or typed by you.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Input Type",
        "",
        "",
        "Whether gear 2's teeth are typed or follow from the ratio.",
        "Number of Teeth for a known pair.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Flip 1",
        "",
        "",
        "Turns gear 1 round on its start plane (it grows to the other side).",
        "On when the gear would stand outside its shaft section.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Flip 2",
        "",
        "",
        "Turns gear 2 round on its start plane.",
        "As Flip 1.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Method of Strength Calculation",
        "",
        "",
        "The standard the strength check follows.",
        "Lewis / AGMA 218.01 per NASA RP-1152 (free source); ISO 6336, DIN 3990 and ANSI/AGMA 2001 are not free and stay out.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Type of Strength Calculation",
        "",
        "",
        "Check the gears as designed, or design the material or the geometry for the loads.",
        "Check Calculation.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Type of Load Calculation",
        "",
        "",
        "Which of power, torque and speed follows from the other two.",
        "Torque from power and speed.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Efficiency",
        "η",
        "",
        "The pair's efficiency (gear 2's torque over gear 1's times the ratio).",
        "0.97–0.99 for a good spur or helical pair.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Allowable Contact Stress (MPa)",
        "σH,all",
        "MPa",
        "The contact (Hertz) stress the flanks may take.",
        "Through-hardened steel 600–900; case-hardened 1200–1500.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Modulus of Elasticity (MPa)",
        "E",
        "MPa",
        "The gears' modulus for the contact stress.",
        "Steel 206 000.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Poisson's Ratio",
        "ν",
        "",
        "The gears' Poisson's ratio for the contact stress.",
        "Steel 0.3.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Required Life (h)",
        "Lh",
        "h",
        "The hours gear 1's flanks must last; the life is checked against it.",
        "Machines in one-shift use 10 000–20 000; continuous duty 40 000+.",
        "Too low: pitting in service.",
        "Too high: oversized gears."
    ),
    e!(
        GEARS,
        "Minimal Safety Factor (bending)",
        "SF,min",
        "",
        "The bending safety each gear must reach.",
        "1.2–1.5 for well-known loads; up to 2 with shocks.",
        "Too low: tooth breakage risk.",
        "Too high: oversized gears."
    ),
    e!(
        KEY,
        "Flip Key",
        "",
        "",
        "Puts the key on the other side of the shaft.",
        "Off.",
        "—",
        "—"
    ),
    e!(
        KEY,
        "Type of Strength Calculation",
        "",
        "",
        "Check the key as typed, or design its length for the torque.",
        "Check Calculation; the shortest standard length is shown anyway.",
        "—",
        "—"
    ),
    e!(
        KEY,
        "Allowed Pressure Hub (MPa)",
        "pH,all",
        "MPa",
        "The pressure the hub's groove may take when the hub is weaker than the key.",
        "Cast-iron hub 50–80; steel hub as the key.",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Method of Calculation",
        "",
        "",
        "The method the life follows.",
        "Basic life per NASA RP-1105 (free source); ISO 281's modified life is not in it.",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Factor of Additional Forces",
        "fd",
        "",
        "Raises the loads for shocks and forces the calculation does not see.",
        "1.0 smooth running; 1.2–1.5 gears and belts; 1.5–3 heavy shocks.",
        "Too low: the bearing wears out early.",
        "Too high: an oversized bearing."
    ),
    e!(
        BEARING,
        "Operating Temperature (°C)",
        "t",
        "°C",
        "The bearing's running temperature; above about 120 °C its capacity falls.",
        "Up to 100 °C for ordinary bearings.",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Type of Strength Calculation",
        "",
        "",
        "Check the spring as typed, compute its working forces, or design its fitted dimensions.",
        "Spring Check.",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Minimum Load (N)",
        "P1",
        "N",
        "The load in the spring's fitted (pre-loaded) position.",
        "Usually 20–50 % of the working load.",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Installed Length (mm)",
        "L1",
        "mm",
        "The spring's length where it is fitted.",
        "Between the solid length and the free length.",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Spring Start",
        "",
        "",
        "How the start (the end on the start plane) is made; closed coils lie on each other, a ground end is flat square to the axis.",
        "Closed and Ground for an even seat; Open ends are cheaper but stand badly.",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Closed Coils Start",
        "z1",
        "",
        "Closed (end) coils at the start, lying on each other at the wire's pitch; they do not spring.",
        "1 (0.75–1.5 usual).",
        "Fewer: the spring stands less squarely.",
        "More: a longer spring for the same rate."
    ),
    e!(
        SPRING,
        "Transition Coils Start",
        "z2",
        "",
        "Coils over which the pitch rises from the closed coils to the active pitch; half of them springs.",
        "0.5–1.",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Spring End",
        "",
        "",
        "How the far end is made.",
        "As the start.",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Closed Coils End",
        "z1",
        "",
        "Closed coils at the far end.",
        "As at the start.",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Transition Coils End",
        "z2",
        "",
        "Transition coils at the far end.",
        "As at the start.",
        "—",
        "—"
    ),
];

/// Every result.
pub const RESULTS: &[Entry] = &[
    // ------------------------------------------------------------- Shaft
    e!(
        SHAFT,
        "Length",
        "L",
        "mm",
        "The shaft's overall length.",
        "—",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Mass",
        "m",
        "kg",
        "The shaft's mass from its sections and the density.",
        "—",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Bending Stress σb",
        "σb",
        "MPa",
        "The largest bending stress at the surface along the shaft.",
        "Compare the fatigue safety, not this alone.",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Torsional Stress τt",
        "τt",
        "MPa",
        "The largest shear stress from torsion at the surface.",
        "—",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Reduced Stress σred",
        "σred",
        "MPa",
        "Bending and torsion combined into one stress (HMH or Tresca); compare it with the yield strength for a static check.",
        "Below σy / 1.5 for a static safety of 1.5.",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Bending Moment",
        "M",
        "N·m",
        "The largest bending moment along the shaft.",
        "—",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Shear Force",
        "V",
        "N",
        "The largest shear force along the shaft.",
        "—",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Deflection",
        "y",
        "µm",
        "The largest sag of the shaft's axis.",
        "General machinery below about 1/2000 of the span between supports; at gears a few hundredths of a millimetre.",
        "—",
        "Too much: gears and bearings run misaligned and wear."
    ),
    e!(
        SHAFT,
        "Angle of Twist",
        "θ",
        "°",
        "How far the shaft's ends turn against each other under the torque.",
        "Line shafts about 0.25° per metre; precision drives much less.",
        "—",
        "Too much: positioning errors, torsional vibration."
    ),
    e!(
        SHAFT,
        "Critical Speed",
        "nc",
        "rpm",
        "The first whirling speed; the shaft's own bending frequency as a speed.",
        "Run below about 0.7–0.8 × nc, or well above it after passing it quickly.",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Support Reaction",
        "R",
        "N",
        "The force each support takes (radial in two directions, axial at the Fixed one): the bearing's load.",
        "Use it as the Radial / Axial Load of the Bearing generator.",
        "—",
        "—"
    ),
    e!(
        SHAFT,
        "Fatigue Safety",
        "FS",
        "",
        "The smallest fatigue safety factor along the shaft (eq. 17) and where it is.",
        "At least the Required safety factor.",
        "Below it: the shaft is too weak there.",
        "—"
    ),
    // ------------------------------------------------------- Spur Gears
    e!(
        GEARS,
        "Module",
        "m",
        "mm",
        "The module and the teeth used, after the Design Guide (Module: from the centre distance; Number of Teeth: the teeth nearest the ratio).",
        "Round a computed module to the preferred series and use Total Unit Correction for the exact distance.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Center Distance",
        "a",
        "mm",
        "The distance of the axes in mesh.",
        "—",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Pitch Diameter",
        "d",
        "mm",
        "m·z (transverse module for helical gears): the circles that roll on each other without shift.",
        "—",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Outside Diameter",
        "da",
        "mm",
        "The tip circle's diameter (blank diameter).",
        "—",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Root Diameter",
        "df",
        "mm",
        "The root circle's diameter.",
        "—",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Base Circle Diameter",
        "db",
        "mm",
        "The circle the involute unwinds from; d·cos α.",
        "—",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Unit Corrections",
        "x1 / x2",
        "",
        "The profile shifts used (given, or from the centre distance).",
        "—",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Operating Pressure Angle",
        "αw",
        "°",
        "The pressure angle at the operating centre distance.",
        "—",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Contact Ratio",
        "εα",
        "",
        "How many tooth pairs are in mesh on average.",
        "At least 1.2; 1.4–1.8 usual for spur gears.",
        "Below 1: the teeth lose contact.",
        "—"
    ),
    e!(
        GEARS,
        "Overlap Ratio",
        "εβ",
        "",
        "Helical gears; how far the face width overlaps a pitch (b·tan β / pt).",
        "Above 1 for smooth helical running.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Torque (gear 1)",
        "T1",
        "N·m",
        "The pinion's torque from power and speed.",
        "—",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Tangential Force",
        "Wt",
        "N",
        "The tooth force along the pitch circle; also the shaft's load.",
        "—",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Pitch-Line Velocity",
        "v",
        "m/s",
        "The speed of the pitch circle, with the dynamic factor Kv it gives.",
        "Spur gears up to about 20 m/s; above, helical ground gears.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Bending Stress",
        "σb",
        "MPa",
        "Each gear's tooth-root bending stress with its geometry factor J and the safety against the allowable stress.",
        "Safety above 1.2–1.5.",
        "Below 1: the teeth break.",
        "—"
    ),
    e!(
        GEARS,
        "Tooth Life L10",
        "L10",
        "10⁶ cycles",
        "The surface (pitting) life of a single tooth at 90 % reliability.",
        "—",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Life",
        "L",
        "h",
        "Gear 1's surface life (all its teeth) at the reliability chosen.",
        "Compare with the machine's required hours.",
        "—",
        "—"
    ),
    e!(
        GEARS,
        "Surface Life",
        "",
        "",
        "Why the surface life cannot be computed.",
        "—",
        "—",
        "—"
    ),
    // ------------------------------------------------------------ Key
    e!(
        KEY,
        "Key (DIN 6885 A)",
        "b × h",
        "mm",
        "The key's width and height and the grooves' depths for the shaft diameter.",
        "—",
        "—",
        "—"
    ),
    e!(
        KEY,
        "Force at the shaft",
        "F",
        "N",
        "The torque as a force at the shaft's surface.",
        "—",
        "—",
        "—"
    ),
    e!(
        KEY,
        "Bearing length",
        "lt",
        "mm",
        "The length the key bears on (l − b with rounded ends).",
        "—",
        "—",
        "—"
    ),
    e!(
        KEY,
        "Shear",
        "τ",
        "MPa",
        "The key's shear stress across its width.",
        "Safety above 1.",
        "Below 1: the key shears.",
        "—"
    ),
    e!(
        KEY,
        "Pressure",
        "p",
        "MPa",
        "The pressure on the part of the key standing in the hub.",
        "Safety above 1.",
        "Below 1: the groove's sides crush.",
        "—"
    ),
    e!(
        KEY,
        "Shortest standard length",
        "lmin",
        "mm",
        "The shortest length of the standard series whose pressure is allowed.",
        "—",
        "—",
        "—"
    ),
    // --------------------------------------------------------- Bearing
    e!(
        BEARING,
        "Equivalent Load Fe",
        "Fe",
        "N",
        "The one radial load that gives the bearing the same life as its radial and axial loads together.",
        "—",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Basic Life L10",
        "L10",
        "10⁶ rev",
        "The life 90 % of bearings reach, in million revolutions and hours.",
        "—",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Required C",
        "Creq",
        "N",
        "The load rating needed for the required life and reliability.",
        "Choose a bearing whose C is above it.",
        "—",
        "—"
    ),
    e!(
        BEARING,
        "Adjusted life",
        "LA",
        "h",
        "The life at the required reliability with the adjustment factors.",
        "At least the required life.",
        "Below: choose a bigger bearing.",
        "—"
    ),
    // ---------------------------------------------------------- Spring
    e!(
        SPRING,
        "Rate",
        "k",
        "N/mm",
        "The force per millimetre of deflection.",
        "—",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Deflection",
        "δ",
        "mm",
        "How far the working load compresses the spring.",
        "Keep it within about 85 % of the travel to solid.",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Working Length",
        "L1",
        "mm",
        "The length under the working load.",
        "—",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Solid Length",
        "Ls",
        "mm",
        "The length with every coil touching.",
        "Leave 10–15 % of the travel as a reserve.",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Stress",
        "p",
        "MPa",
        "The wire's shear stress at the working load.",
        "Below the allowed stress.",
        "—",
        "Above: the spring sets or breaks."
    ),
    e!(
        SPRING,
        "Deflection at the Allowed Stress",
        "δall",
        "mm",
        "How far the spring may go before reaching the allowed stress.",
        "—",
        "—",
        "—"
    ),
    e!(
        SPRING,
        "Pitch",
        "t",
        "mm",
        "The active coils' pitch that gives the free length, and the coils in all (active, transition and closed).",
        "At least about 1.2 × the wire so the active coils do not touch before the working load.",
        "—",
        "—"
    ),
];

/// Every formula.
pub const FORMULAS: &[Formula] = &[
    f!(
        SHAFT,
        "Bending Stress σb",
        "σb = 32·M·B / (π·d³), B = 1 / (1 − (di/d)⁴)",
        "M bending moment, d diameter, di bore",
        RP1123,
        "Round sections; hollow by B (eq. 4)."
    ),
    f!(
        SHAFT,
        "Torsional Stress τt",
        "τt = 16·T·B / (π·d³)",
        "T torque",
        RP1123,
        "Round sections."
    ),
    f!(
        SHAFT,
        "Reduced Stress σred",
        "σred = √(σb² + 3·τt²) (HMH), √(σb² + 4·τt²) (Tresca)",
        "σb bending, τt torsional stress",
        RP1123,
        "Static combination; fatigue uses eq. 17."
    ),
    f!(
        SHAFT,
        "Deflection",
        "y'' = M / (E·I), I = π·(d⁴ − di⁴) / 64, y = 0 at the supports",
        "E modulus, I second moment",
        RP1123,
        "Small deflections, no shear deflection."
    ),
    f!(
        SHAFT,
        "Angle of Twist",
        "θ = T·L / (G·Ip) summed over the sections (eq. 32)",
        "Ip = π·d⁴ / 32",
        RP1123,
        "Each section's own diameter."
    ),
    f!(
        SHAFT,
        "Critical Speed",
        "ωc = √(g·ΣW·δ / ΣW·δ²) (eq. 33, Rayleigh)",
        "W weights of the pieces, δ their deflections under the own weight",
        RP1123,
        "First mode only; bearings rigid."
    ),
    f!(
        SHAFT,
        "Fatigue Safety",
        "FS = π·d³ / (32·√((M/σf)² + ¾·(T/σy)²)) (eq. 17), σf = ka·kb·kc·kd·ke·kf·kg·kh·ki·σ'f (eq. 19), ke = 1 / (1 + q·(Kt − 1)) (eq. 25)",
        "M reversed bending, T steady torque, σf corrected fatigue limit",
        RP1123,
        "Reversed bending with steady torque (rotating shaft); variable loads by eq. 28–31."
    ),
    f!(
        SHAFT,
        "Mass",
        "m = ρ·Σ A·L",
        "A section area, L section length",
        RP1123,
        "—"
    ),
    f!(
        GEARS,
        "Module",
        "m = 2·a·cos β / (z1 + z2) (guide Module); z1 + z2 = ⌊2·a·cos β / m⌋, z1 ≈ (z1 + z2) / (1 + i) (guide Number of Teeth)",
        "a centre distance, β helix angle, i ratio",
        RP1152,
        "eq. 1 and 6 solved for the unknown; no shift."
    ),
    f!(
        GEARS,
        "Center Distance",
        "a = m·(z1 + z2) / 2 · cos α / cos αw",
        "αw from inv αw = inv α + 2·(x1 + x2)·tan α / (z1 + z2)",
        RP1152,
        "Involute geometry."
    ),
    f!(
        GEARS,
        "Pitch Diameter",
        "d = m·z / cos β",
        "β helix angle",
        RP1152,
        "eq. 1–6."
    ),
    f!(
        GEARS,
        "Base Circle Diameter",
        "db = d·cos αt",
        "αt transverse pressure angle",
        RP1152,
        "—"
    ),
    f!(
        GEARS,
        "Contact Ratio",
        "εα = (√(ra1² − rb1²) + √(ra2² − rb2²) − a·sin αw) / (π·m·cos α)",
        "ra tip, rb base radii",
        RP1152,
        "eq. 8."
    ),
    f!(
        GEARS,
        "Tangential Force",
        "Wt = T / rp = 2·T / d1",
        "T pinion torque",
        RP1152,
        "§5.1."
    ),
    f!(
        GEARS,
        "Torque (gear 1)",
        "T = P / ω = 9549·P[kW] / n[rpm] N·m",
        "P power, n speed",
        RP1152,
        "—"
    ),
    f!(
        GEARS,
        "Pitch-Line Velocity",
        "v = π·d1·n1 / 60000, Kv from Fig. 25",
        "d1 in mm, n1 in rpm",
        RP1152,
        "Fig. 25 curves for levels 3–11."
    ),
    f!(
        GEARS,
        "Bending Stress",
        "σb = Wt·Ka·Ks·Km / (Kv·b·m·J) (eq. 53), Ks = (25.4 / m)^−0.25 (eq. 54)",
        "J geometry factor, b face width, Ks size factor",
        RP1152,
        "AGMA 218.01 form; J from the tooth (Lewis parabola, Dolan–Broghamer Kf)."
    ),
    f!(
        GEARS,
        "Tooth Life L10",
        "L10 ∝ (contact load)^−4.3 (eq. 67–70)",
        "NASA surface fatigue life model",
        RP1152,
        "Vacuum-melted AISI 9310 tests; scaled by the life factor."
    ),
    f!(
        GEARS,
        "Life",
        "Lgear = L10·z^(−1/e), L at S: L = L10·(ln S / ln 0.9)^(1/e) (eq. 70, 73)",
        "e Weibull slope 2.5",
        RP1152,
        "—"
    ),
    f!(
        KEY,
        "Force at the shaft",
        "F = 2·T / d",
        "T torque, d shaft diameter",
        KB,
        "§69."
    ),
    f!(
        KEY,
        "Shear",
        "τ = F / (b·lt) (eq. 1)",
        "b width, lt bearing length",
        KB,
        "Even bearing along the key."
    ),
    f!(
        KEY,
        "Pressure",
        "p = F / (k·lt), k = h − t1 (eq. 3)",
        "h height, t1 shaft groove depth",
        KB,
        "The part in the hub; the book takes h / 2."
    ),
    f!(
        BEARING,
        "Equivalent Load Fe",
        "Fe = X·Fr + Y·Fa",
        "X, Y from the catalogue",
        RP1105,
        "§12.4."
    ),
    f!(
        BEARING,
        "Basic Life L10",
        "L10 = (C / Fe)^m million revolutions, m = 3 ball, 10/3 roller; Lh = L10·10⁶ / (60·n)",
        "C dynamic rating",
        RP1105,
        "§12.2."
    ),
    f!(
        BEARING,
        "Adjusted life",
        "LA = D·E·F·G·H·L10, at S: L = L10·(ln S / ln 0.9)^(1/e), e = 10/9 ball, 3/2 roller",
        "Weibull slope e",
        RP1105,
        "§12.5; ISO 281's a_ISO not included."
    ),
    f!(
        BEARING,
        "Required C",
        "Creq = Fe·(L / (D·E·F·G·H))^(1/m)",
        "L required life in 10⁶ rev at the reliability",
        RP1105,
        "—"
    ),
    f!(
        SPRING,
        "Rate",
        "k = d⁴·G / (8·D³·n)",
        "d wire, D mean diameter, n active coils",
        KB,
        "§43 eq. (1)–(3) (64·r³ with r = D/2)."
    ),
    f!(
        SPRING,
        "Deflection",
        "δ = P / k = 8·P·D³·n / (d⁴·G)",
        "P load",
        KB,
        "eq. (2)."
    ),
    f!(
        SPRING,
        "Stress",
        "p = 8·P·D / (π·d³)",
        "torsion only",
        KB,
        "eq. (4) (16·P·r / (π·d³)); no curvature correction."
    ),
    f!(
        SPRING,
        "Deflection at the Allowed Stress",
        "δ = π·n·D²·p / (G·d)",
        "p allowed stress",
        KB,
        "eq. (7)."
    ),
    f!(
        SPRING,
        "Solid Length",
        "Ls = d·(n + inactive coils)",
        "—",
        KB,
        "The book's H = d·n."
    ),
    f!(
        SPRING,
        "Pitch",
        "L0 = z0 + Σ z1·d + Σ z2·(d + t)/2 + n·t + z1end; z0 = d/2 (plain end) or 0 (ground: the wire's centre on the plane)",
        "z1 closed, z2 transition coils, n active, t pitch",
        KB,
        "The pitch changes linearly over a transition; closed coils rise one wire a turn."
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// No entry appears twice, every text is filled, every formula names its source.
    #[test]
    fn entries_are_unique_and_filled() {
        for (list, what) in [(INPUTS, "input"), (RESULTS, "result")] {
            for (i, e) in list.iter().enumerate() {
                assert!(
                    !list[..i]
                        .iter()
                        .any(|o| o.generator == e.generator && o.id == e.id),
                    "{what} {}/{} twice",
                    e.generator,
                    e.id
                );
                assert!(!e.what.is_empty() && !e.typical.is_empty(), "{}", e.id);
            }
        }
        for f in FORMULAS {
            assert!(!f.source.is_empty() && !f.formula.is_empty(), "{}", f.id);
            assert!(
                result(f.generator, f.id).is_some(),
                "formula of no result: {}",
                f.id
            );
        }
        for g in [SHAFT, GEARS, KEY, BEARING, SPRING] {
            assert!(!method(g).is_empty());
        }
    }
}
