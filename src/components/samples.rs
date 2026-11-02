//! The built-in components: `data/components` (families, types, tables) read
//! into [`Sample`]s, each built as an ordinary part (sketch + Revolve / Extrude)
//! with its properties. The numbers are nominal values of the named standards,
//! not yet a checked data set (spec 6.3 §7, decision 1). Every part's axis is
//! Z; a fastener's seat lies on the XY plane, its head above it. Only families
//! whose type has a builder are listed; the others wait in the tables.
//!
//! Hexagons have no 30° corner chamfers yet: an Intersect with a cone blank
//! took the kernel 1.3–2 s a part, and a ring cut did not finish in 10 minutes
//! (spec 6.3 §6.4).

use crate::document::iprops::IProperties;
use crate::kernel::{Point3, Real, Surface};
use crate::part::frame::{Library, Loop, draw_loop, hidden};
use crate::part::hole::{ThreadFeature, ThreadSpec};
use crate::part::standards::{ANSI_UNIFIED, ISO_METRIC, Standards};
use crate::part::work::{WorkDef, WorkPlaneDef};
use crate::part::{
    AxisRef, ExtentDirection, ExtrudeFeature, Operation, OriginPlane, PartDocument, PartError,
    PlaneRef, ProfileRef, RevolveFeature, SketchOptions, SketchPlacement,
};
use crate::sketch::Point2;

/// What a sample is, with its numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Hex head bolt: thread diameter, length, across flats, head height.
    HexBolt(Real, Real, Real, Real),
    /// The same, threaded only near its end (2d + 6, 2d + 12 above 125 mm).
    PartThreadBolt(Real, Real, Real, Real),
    /// Socket head cap screw: thread diameter, length, head diameter, head
    /// height, socket across flats, socket depth.
    SocketScrew(Real, Real, Real, Real, Real, Real),
    /// Hex nut: thread diameter, across flats, height.
    HexNut(Real, Real, Real),
    /// Plain washer: inner and outer diameter, thickness.
    Washer(Real, Real, Real),
    /// Round head rivet: diameter, length, head diameter, head height.
    Rivet(Real, Real, Real, Real),
    /// Cylindrical pin: diameter, length.
    Pin(Real, Real),
    /// Parallel key with round ends: width, height, length.
    Key(Real, Real, Real),
    /// External retaining ring: shaft diameter, groove diameter, thickness.
    Circlip(Real, Real, Real),
    /// Ball bearing, simplified to one body: bore, outside diameter, width.
    Bearing(Real, Real, Real),
    /// Countersunk screw: thread diameter, length, head diameter, head height,
    /// socket across flats.
    Countersunk(Real, Real, Real, Real, Real),
    /// Button head screw: thread diameter, length, head diameter, head
    /// height, socket across flats.
    ButtonHead(Real, Real, Real, Real, Real),
    /// Set screw: thread diameter, length, socket across flats, socket depth.
    SetScrew(Real, Real, Real, Real),
    /// Threaded rod: thread diameter, length.
    Rod(Real, Real),
    /// Split ring, the slot a wedge of the given half angle (degrees): inner
    /// and outer diameter, height, half angle.
    SplitRing(Real, Real, Real, Real),
    /// Taper pin, 1:50: small end diameter, length.
    TaperPin(Real, Real),
    /// Flange bolt: thread diameter, length, across flats, head height, flange diameter.
    FlangeBolt(Real, Real, Real, Real, Real),
    /// Flange nut: thread diameter, across flats, height, flange diameter.
    FlangeNut(Real, Real, Real, Real),
    /// Domed cap nut: thread diameter, across flats, height.
    CapNut(Real, Real, Real),
    /// Woodruff key: width, height, diameter.
    Woodruff(Real, Real, Real),
    /// O-ring: inner diameter, section.
    ORing(Real, Real),
    /// Structural shape: family and size of the frame section table, length.
    Shape(&'static str, &'static str, Real),
}

/// How much of a component's shape its part holds (spec 6.3 §6.0a). All levels
/// are the same component: one designation, one Part Number, one BOM row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Detail {
    /// The envelope only, no thread and no recess: light assemblies.
    Simple,
    /// The usual shape with a cosmetic thread.
    #[default]
    Standard,
    /// The rich shape: chamfered hexagons, a bearing's rings and rolling
    /// elements. Takes longer to build.
    Full,
}

impl Detail {
    /// Every level, simplest first.
    pub const ALL: [Detail; 3] = [Detail::Simple, Detail::Standard, Detail::Full];

    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            Detail::Simple => "Simple",
            Detail::Standard => "Standard",
            Detail::Full => "Full",
        }
    }
}

/// One built-in component.
#[derive(Clone, Debug)]
pub struct Sample {
    /// Folders below the library.
    pub category: String,
    /// Family folder and the standard's name.
    pub family: String,
    /// Which standards body: `ISO`, `DIN`, `ANSI`, `EN`, `Other`.
    pub standard: String,
    /// Size in the designation (`M8 x 30`).
    pub size: String,
    /// Its shape.
    pub kind: Kind,
}

const FAMILIES: &str = include_str!("../../data/components/families.csv");
const TYPES: &str = include_str!("../../data/components/types.csv");
/// Bolt families threaded only near the end.
const PART_THREAD: [&str; 2] = ["ISO 4014", "DIN 931"];
/// The length of a structural shape placed from the library.
const SHAPE_LENGTH: Real = 1000.0;

/// The family tables built into the program, by their id in `families.csv`.
const TABLES: &[(&str, &str)] = &[
    (
        "ansi_b17_1",
        include_str!("../../data/components/families/ansi_b17_1.csv"),
    ),
    (
        "asme_b18_21_1",
        include_str!("../../data/components/families/asme_b18_21_1.csv"),
    ),
    (
        "asme_b18_22_1_sae",
        include_str!("../../data/components/families/asme_b18_22_1_sae.csv"),
    ),
    (
        "asme_b18_22_1_uss",
        include_str!("../../data/components/families/asme_b18_22_1_uss.csv"),
    ),
    (
        "asme_b18_2_1",
        include_str!("../../data/components/families/asme_b18_2_1.csv"),
    ),
    (
        "asme_b18_2_2",
        include_str!("../../data/components/families/asme_b18_2_2.csv"),
    ),
    (
        "asme_b18_2_2_jam",
        include_str!("../../data/components/families/asme_b18_2_2_jam.csv"),
    ),
    (
        "asme_b18_3",
        include_str!("../../data/components/families/asme_b18_3.csv"),
    ),
    (
        "asme_b18_3_button",
        include_str!("../../data/components/families/asme_b18_3_button.csv"),
    ),
    (
        "asme_b18_3_flat",
        include_str!("../../data/components/families/asme_b18_3_flat.csv"),
    ),
    (
        "asme_b18_3_set",
        include_str!("../../data/components/families/asme_b18_3_set.csv"),
    ),
    (
        "asme_b18_8_2",
        include_str!("../../data/components/families/asme_b18_8_2.csv"),
    ),
    (
        "astm_a193_rod",
        include_str!("../../data/components/families/astm_a193_rod.csv"),
    ),
    (
        "bearing_12",
        include_str!("../../data/components/families/bearing_12.csv"),
    ),
    (
        "bearing_222",
        include_str!("../../data/components/families/bearing_222.csv"),
    ),
    (
        "bearing_302",
        include_str!("../../data/components/families/bearing_302.csv"),
    ),
    (
        "bearing_511",
        include_str!("../../data/components/families/bearing_511.csv"),
    ),
    (
        "bearing_60",
        include_str!("../../data/components/families/bearing_60.csv"),
    ),
    (
        "bearing_62",
        include_str!("../../data/components/families/bearing_62.csv"),
    ),
    (
        "bearing_63",
        include_str!("../../data/components/families/bearing_63.csv"),
    ),
    (
        "bearing_68",
        include_str!("../../data/components/families/bearing_68.csv"),
    ),
    (
        "bearing_72",
        include_str!("../../data/components/families/bearing_72.csv"),
    ),
    (
        "bearing_hk",
        include_str!("../../data/components/families/bearing_hk.csv"),
    ),
    (
        "bearing_miniature",
        include_str!("../../data/components/families/bearing_miniature.csv"),
    ),
    (
        "bearing_nu2",
        include_str!("../../data/components/families/bearing_nu2.csv"),
    ),
    (
        "bearing_r",
        include_str!("../../data/components/families/bearing_r.csv"),
    ),
    (
        "din_125",
        include_str!("../../data/components/families/din_125.csv"),
    ),
    (
        "din_127",
        include_str!("../../data/components/families/din_127.csv"),
    ),
    (
        "din_1587",
        include_str!("../../data/components/families/din_1587.csv"),
    ),
    (
        "din_179",
        include_str!("../../data/components/families/din_179.csv"),
    ),
    (
        "din_3760",
        include_str!("../../data/components/families/din_3760.csv"),
    ),
    (
        "din_439",
        include_str!("../../data/components/families/din_439.csv"),
    ),
    (
        "din_471",
        include_str!("../../data/components/families/din_471.csv"),
    ),
    (
        "din_471_light",
        include_str!("../../data/components/families/din_471_light.csv"),
    ),
    (
        "din_472",
        include_str!("../../data/components/families/din_472.csv"),
    ),
    (
        "din_5419",
        include_str!("../../data/components/families/din_5419.csv"),
    ),
    (
        "din_6334",
        include_str!("../../data/components/families/din_6334.csv"),
    ),
    (
        "din_660",
        include_str!("../../data/components/families/din_660.csv"),
    ),
    (
        "din_6885_a",
        include_str!("../../data/components/families/din_6885_a.csv"),
    ),
    (
        "din_6888",
        include_str!("../../data/components/families/din_6888.csv"),
    ),
    (
        "din_6927",
        include_str!("../../data/components/families/din_6927.csv"),
    ),
    (
        "din_7",
        include_str!("../../data/components/families/din_7.csv"),
    ),
    (
        "din_705",
        include_str!("../../data/components/families/din_705.csv"),
    ),
    (
        "din_7349",
        include_str!("../../data/components/families/din_7349.csv"),
    ),
    (
        "din_7991",
        include_str!("../../data/components/families/din_7991.csv"),
    ),
    (
        "din_9021",
        include_str!("../../data/components/families/din_9021.csv"),
    ),
    (
        "din_912",
        include_str!("../../data/components/families/din_912.csv"),
    ),
    (
        "din_913",
        include_str!("../../data/components/families/din_913.csv"),
    ),
    (
        "din_916",
        include_str!("../../data/components/families/din_916.csv"),
    ),
    (
        "din_931",
        include_str!("../../data/components/families/din_931.csv"),
    ),
    (
        "din_933",
        include_str!("../../data/components/families/din_933.csv"),
    ),
    (
        "din_934",
        include_str!("../../data/components/families/din_934.csv"),
    ),
    (
        "din_938",
        include_str!("../../data/components/families/din_938.csv"),
    ),
    (
        "din_976",
        include_str!("../../data/components/families/din_976.csv"),
    ),
    (
        "din_985",
        include_str!("../../data/components/families/din_985.csv"),
    ),
    (
        "din_988",
        include_str!("../../data/components/families/din_988.csv"),
    ),
    (
        "iso_10642",
        include_str!("../../data/components/families/iso_10642.csv"),
    ),
    (
        "iso_2338",
        include_str!("../../data/components/families/iso_2338.csv"),
    ),
    (
        "iso_2339",
        include_str!("../../data/components/families/iso_2339.csv"),
    ),
    (
        "iso_3547",
        include_str!("../../data/components/families/iso_3547.csv"),
    ),
    (
        "iso_3601",
        include_str!("../../data/components/families/iso_3601.csv"),
    ),
    (
        "iso_4014",
        include_str!("../../data/components/families/iso_4014.csv"),
    ),
    (
        "iso_4017",
        include_str!("../../data/components/families/iso_4017.csv"),
    ),
    (
        "iso_4026",
        include_str!("../../data/components/families/iso_4026.csv"),
    ),
    (
        "iso_4027",
        include_str!("../../data/components/families/iso_4027.csv"),
    ),
    (
        "iso_4028",
        include_str!("../../data/components/families/iso_4028.csv"),
    ),
    (
        "iso_4029",
        include_str!("../../data/components/families/iso_4029.csv"),
    ),
    (
        "iso_4032",
        include_str!("../../data/components/families/iso_4032.csv"),
    ),
    (
        "iso_4035",
        include_str!("../../data/components/families/iso_4035.csv"),
    ),
    (
        "iso_4161",
        include_str!("../../data/components/families/iso_4161.csv"),
    ),
    (
        "iso_4162",
        include_str!("../../data/components/families/iso_4162.csv"),
    ),
    (
        "iso_4762",
        include_str!("../../data/components/families/iso_4762.csv"),
    ),
    (
        "iso_7089",
        include_str!("../../data/components/families/iso_7089.csv"),
    ),
    (
        "iso_7093",
        include_str!("../../data/components/families/iso_7093.csv"),
    ),
    (
        "iso_7380_1",
        include_str!("../../data/components/families/iso_7380_1.csv"),
    ),
    (
        "iso_8734",
        include_str!("../../data/components/families/iso_8734.csv"),
    ),
    (
        "iso_8752",
        include_str!("../../data/components/families/iso_8752.csv"),
    ),
    (
        "lm_uu",
        include_str!("../../data/components/families/lm_uu.csv"),
    ),
    (
        "magnet_disc",
        include_str!("../../data/components/families/magnet_disc.csv"),
    ),
];

/// A table's lines without comments, the first one the heading.
fn records(text: &str) -> Vec<Vec<&str>> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split(',').map(str::trim).collect())
        .collect()
}

/// The built-in tables by name, for tests.
pub fn table_ids() -> Vec<&'static str> {
    TABLES.iter().map(|t| t.0).collect()
}

/// The template of a type (`hex_bolt`).
fn template_of(category: &str, type_: &str) -> Option<String> {
    records(TYPES)
        .iter()
        .skip(1)
        .find(|r| r.first() == Some(&category) && r.get(1) == Some(&type_))
        .and_then(|r| r.get(2))
        .map(|t| (*t).to_owned())
}

/// A nominal length as its standard writes it: mm, or inches in 16ths (`1-1/2`).
fn length_text(l: Real, inches: bool) -> String {
    if !inches {
        return l.to_string();
    }
    let sixteenths = (l / 25.4 * 16.0).round() as u32;
    let (whole, mut n, mut d) = (sixteenths / 16, sixteenths % 16, 16);
    while n > 0 && n % 2 == 0 {
        n /= 2;
        d /= 2;
    }
    match (whole, n) {
        (w, 0) => w.to_string(),
        (0, n) => format!("{n}/{d}"),
        (w, n) => format!("{w}-{n}/{d}"),
    }
}

/// Every member of a row: one, or one per nominal length.
fn row_members(
    template: &str,
    get: &dyn Fn(&str) -> Option<Real>,
    size: &str,
    lengths: &[Real],
    inches: bool,
) -> Vec<(String, Kind)> {
    let per_length = |make: &dyn Fn(Real) -> Option<Kind>| -> Vec<(String, Kind)> {
        lengths
            .iter()
            .filter_map(|&l| make(l).map(|k| (format!("{size} x {}", length_text(l, inches)), k)))
            .collect()
    };
    let one = |k: Option<Kind>| -> Vec<(String, Kind)> {
        k.map(|k| (size.to_owned(), k)).into_iter().collect()
    };
    match template {
        "hex_bolt" => per_length(&|l| Some(Kind::HexBolt(get("d")?, l, get("s")?, get("k")?))),
        "socket_screw" => per_length(&|l| {
            Some(Kind::SocketScrew(
                get("d")?,
                l,
                get("dk")?,
                get("k")?,
                get("s")?,
                get("t")?,
            ))
        }),
        "hex_nut" | "lock_nut" | "coupling_nut" => {
            one((|| Some(Kind::HexNut(get("d")?, get("s")?, get("m")?)))())
        }
        "countersunk_screw" => per_length(&|l| {
            Some(Kind::Countersunk(
                get("d")?,
                l,
                get("dk")?,
                get("k")?,
                get("s")?,
            ))
        }),
        "button_screw" => per_length(&|l| {
            Some(Kind::ButtonHead(
                get("d")?,
                l,
                get("dk")?,
                get("k")?,
                get("s")?,
            ))
        }),
        "set_screw" => per_length(&|l| Some(Kind::SetScrew(get("d")?, l, get("s")?, get("t")?))),
        "threaded_rod" => per_length(&|l| Some(Kind::Rod(get("d")?, l))),
        "spring_washer" => one((|| {
            let (d1, d2, h) = (get("d1")?, get("d2")?, get("s")?);
            Some(Kind::SplitRing(d1, d2, h, 2.0))
        })()),
        "taper_pin" => per_length(&|l| Some(Kind::TaperPin(get("d")?, l))),
        "spring_pin" => per_length(&|l| {
            let (d, wall) = (get("d")?, get("s")?);
            Some(Kind::SplitRing(d - 2.0 * wall, d, l, 8.0))
        }),
        "hex_flange_bolt" => per_length(&|l| {
            Some(Kind::FlangeBolt(
                get("d")?,
                l,
                get("s")?,
                get("k")?,
                get("dc")?,
            ))
        }),
        "hex_flange_nut" => one((|| {
            Some(Kind::FlangeNut(get("d")?, get("s")?, get("m")?, get("dc")?))
        })()),
        "cap_nut" => one((|| Some(Kind::CapNut(get("d")?, get("s")?, get("m")?)))()),
        "circlip_internal" => one((|| {
            // The groove is outside the bore: the ring lies inside the groove's circle.
            let (d, groove, t) = (get("d1")?, get("d2")?, get("s")?);
            Some(Kind::SplitRing(groove - 0.24 * d, groove, t, 15.0))
        })()),
        "woodruff_key" => one((|| Some(Kind::Woodruff(get("b")?, get("h")?, get("D")?)))()),
        "o_ring" => one((|| Some(Kind::ORing(get("d1")?, get("d2")?)))()),
        "collar" | "shaft_seal" | "felt_ring" => {
            one((|| Some(Kind::Washer(get("d")?, get("D")?, get("b")?)))())
        }
        "bush" => per_length(&|l| Some(Kind::Washer(get("d")?, get("D")?, l))),
        "washer" => one((|| Some(Kind::Washer(get("d1")?, get("d2")?, get("h")?)))()),
        "rivet" => per_length(&|l| Some(Kind::Rivet(get("d")?, l, get("dk")?, get("k")?))),
        "pin" => per_length(&|l| Some(Kind::Pin(get("d")?, l))),
        "key" => per_length(&|l| Some(Kind::Key(get("b")?, get("h")?, l))),
        "circlip" => one((|| Some(Kind::Circlip(get("d1")?, get("d2")?, get("s")?)))()),
        "ball_bearing" | "roller_bearing" => {
            one((|| Some(Kind::Bearing(get("d")?, get("D")?, get("B")?)))())
        }
        "taper_bearing" | "thrust_bearing" => {
            one((|| Some(Kind::Bearing(get("d")?, get("D")?, get("T")?)))())
        }
        _ => Vec::new(),
    }
}

/// Every built-in component whose type has a builder.
pub fn all() -> Vec<Sample> {
    let mut out = Vec::new();
    for f in records(FAMILIES).iter().skip(1) {
        let part_thread = f.get(3).is_some_and(|n| PART_THREAD.contains(n));
        // A title may hold a comma: the standard is the last cell.
        let [id, category, type_, family, .., standard] = f.as_slice() else {
            continue;
        };
        let folder = format!("{category}/{type_}");
        if template_of(category, type_).as_deref() == Some("section") {
            let lib = Library::builtin();
            let Some(fam) = lib.families_named(family) else {
                continue;
            };
            for size in lib.sizes(fam) {
                out.push(Sample {
                    category: folder.clone(),
                    family: (*family).to_owned(),
                    standard: (*standard).to_owned(),
                    size: size.to_owned(),
                    kind: Kind::Shape(fam, size, SHAPE_LENGTH),
                });
            }
            continue;
        }
        let (Some(template), Some((_, text))) = (
            template_of(category, type_),
            TABLES.iter().find(|t| t.0 == *id),
        ) else {
            continue;
        };
        let rows = records(text);
        let Some(head) = rows.first() else { continue };
        let at = |n: &str| head.iter().position(|h| *h == n);
        for row in rows.iter().skip(1) {
            let get = |n: &str| -> Option<Real> { row.get(at(n)?)?.parse().ok() };
            let size = at("size").and_then(|i| row.get(i)).copied().unwrap_or("");
            let lengths: Vec<Real> = at("lengths")
                .and_then(|i| row.get(i))
                .map(|v| v.split(';').filter_map(|x| x.parse().ok()).collect())
                .unwrap_or_default();
            for (size, kind) in row_members(&template, &get, size, &lengths, *standard == "ANSI") {
                let kind = match kind {
                    Kind::HexBolt(d, l, s, k) if part_thread => Kind::PartThreadBolt(d, l, s, k),
                    k => k,
                };
                out.push(Sample {
                    category: folder.clone(),
                    family: (*family).to_owned(),
                    standard: (*standard).to_owned(),
                    size,
                    kind,
                });
            }
        }
    }
    out
}

impl Sample {
    /// `ISO 4017 M8 x 30`, `6204`.
    pub fn designation(&self) -> String {
        // A bearing's size is its whole designation (`6204`).
        if matches!(self.kind, Kind::Bearing(..)) {
            return self.size.clone();
        }
        format!("{} {}", self.family, self.size)
    }

    /// The file's name (an inch size holds a `/`).
    pub fn file_name(&self) -> String {
        format!("{}.ccpart", self.designation().replace('/', "_"))
    }

    /// The file's path below the library folder.
    pub fn relative_path(&self) -> std::path::PathBuf {
        self.relative_path_at(Detail::Standard)
    }

    /// The file of one level of detail.
    pub fn relative_path_at(&self, detail: Detail) -> std::path::PathBuf {
        let folder = std::path::Path::new(&self.category).join(&self.family);
        match self.level_of(detail) {
            Detail::Standard => folder.join(self.file_name()),
            d => folder.join(format!(
                "{} ({}).ccpart",
                self.designation().replace('/', "_"),
                d.label()
            )),
        }
    }

    /// The level this component really has: Full only where it differs from
    /// Standard, Simple only where it differs too.
    pub fn level_of(&self, detail: Detail) -> Detail {
        match detail {
            Detail::Full if !self.has_full() => Detail::Standard,
            Detail::Simple if !self.has_simple() => Detail::Standard,
            d => d,
        }
    }

    /// A richer shape than Standard exists.
    pub fn has_full(&self) -> bool {
        matches!(
            self.kind,
            Kind::HexBolt(..) | Kind::PartThreadBolt(..) | Kind::HexNut(..) | Kind::Bearing(..)
        )
    }

    /// A simpler shape than Standard exists.
    pub fn has_simple(&self) -> bool {
        !matches!(
            self.kind,
            Kind::Washer(..)
                | Kind::Rivet(..)
                | Kind::Pin(..)
                | Kind::Key(..)
                | Kind::TaperPin(..)
                | Kind::Woodruff(..)
                | Kind::ORing(..)
                | Kind::Shape(..)
        )
    }

    /// Part number = designation; the BOM and drawings read these.
    pub fn properties(&self) -> IProperties {
        let mut p = match self.kind {
            Kind::Shape(family, size, length) => Library::builtin()
                .find(family, size)
                .map(|s| s.member_properties(length))
                .unwrap_or_default(),
            _ => IProperties::default(),
        };
        p.set("part_number", &self.designation());
        p.set("title", &self.designation());
        if p.get("description").is_empty() {
            p.set("description", &format!("{} {}", self.noun(), self.size));
        }
        p.set("comments", "Sample library component: check the numbers");
        p
    }

    fn noun(&self) -> &'static str {
        match self.kind {
            Kind::HexBolt(..) | Kind::PartThreadBolt(..) => "Hex head bolt",
            Kind::SocketScrew(..) => "Socket head cap screw",
            Kind::HexNut(..) => "Hex nut",
            Kind::Washer(..) => "Plain washer",
            Kind::Rivet(..) => "Round head rivet",
            Kind::Pin(..) => "Cylindrical pin",
            Kind::Key(..) => "Parallel key",
            Kind::Circlip(..) => "Retaining ring",
            Kind::Bearing(..) => "Ball bearing",
            Kind::Countersunk(..) => "Countersunk screw",
            Kind::ButtonHead(..) => "Button head screw",
            Kind::SetScrew(..) => "Set screw",
            Kind::Rod(..) => "Threaded rod",
            Kind::SplitRing(..) => "Split ring",
            Kind::TaperPin(..) => "Taper pin",
            Kind::FlangeBolt(..) => "Hexagon flange bolt",
            Kind::FlangeNut(..) => "Hexagon flange nut",
            Kind::CapNut(..) => "Domed cap nut",
            Kind::Woodruff(..) => "Woodruff key",
            Kind::ORing(..) => "O-ring",
            Kind::Shape(..) => "Structural shape",
        }
    }

    /// Builds the part at the usual level.
    pub fn part(&self) -> Result<PartDocument, PartError> {
        self.part_at(Detail::Standard)
    }

    /// Builds the part at `detail`.
    pub fn part_at(&self, detail: Detail) -> Result<PartDocument, PartError> {
        let mut part = match (self.level_of(detail), self.kind) {
            (Detail::Simple, kind) => simple_part(kind),
            (Detail::Full, Kind::HexBolt(d, l, s, k)) => full_hex_bolt(d, l, s, k, None),
            (Detail::Full, Kind::PartThreadBolt(d, l, s, k)) => {
                full_hex_bolt(d, l, s, k, Some(part_thread(d, l)))
            }
            (Detail::Full, Kind::HexNut(d, s, m)) => full_hex_nut(d, s, m),
            (Detail::Full, Kind::Bearing(d, od, b)) => full_bearing(d, od, b),
            (_, kind) => standard_part(kind),
        }?;
        part.set_material(Some(self.material()), crate::materials::with_user())
            .map_err(|e| bad(&e))?;
        Ok(part)
    }

    fn material(&self) -> &'static str {
        match self.kind {
            Kind::Shape(..) => "Steel S235",
            _ => "Steel AISI 4140",
        }
    }
}

/// The thread length of a partly threaded bolt.
fn part_thread(d: Real, l: Real) -> Real {
    2.0 * d + if l > 125.0 { 12.0 } else { 6.0 }
}

/// The usual shape of each kind.
fn standard_part(kind: Kind) -> Result<PartDocument, PartError> {
    {
        match kind {
            Kind::HexBolt(d, l, s, k) => hex_bolt(d, l, s, k, None),
            Kind::PartThreadBolt(d, l, s, k) => hex_bolt(d, l, s, k, Some(part_thread(d, l))),
            Kind::SocketScrew(d, l, dk, k, s, t) => socket_screw(d, l, dk, k, s, t),
            Kind::HexNut(d, s, m) => hex_nut(d, s, m),
            Kind::Washer(d1, d2, h) => ring(&[
                (d1 / 2.0, 0.0),
                (d2 / 2.0, 0.0),
                (d2 / 2.0, h),
                (d1 / 2.0, h),
            ]),
            Kind::Rivet(d, l, dk, k) => rivet(d, l, dk, k),
            Kind::Pin(d, l) => {
                let (r, c) = (d / 2.0, d / 8.0);
                ring(&[
                    (0.0, 0.0),
                    (r - c, 0.0),
                    (r, c),
                    (r, l - c),
                    (r - c, l),
                    (0.0, l),
                ])
            }
            Kind::Key(b, h, l) => key(b, h, l),
            Kind::Countersunk(d, l, dk, k, s) => countersunk(d, l, dk, k, s),
            Kind::ButtonHead(d, l, dk, k, s) => button_head(d, l, dk, k, s),
            Kind::SetScrew(d, l, s, t) => set_screw(d, l, s, t),
            Kind::Rod(d, l) => rod(d, l),
            Kind::SplitRing(d1, d2, h, half) => split_ring(d1, d2, h, half),
            Kind::TaperPin(d, l) => ring(&[
                (0.0, 0.0),
                (d / 2.0, 0.0),
                (d / 2.0 + l / 100.0, l),
                (0.0, l),
            ]),
            Kind::FlangeBolt(d, l, s, k, dc) => flange_bolt(d, l, s, k, dc),
            Kind::FlangeNut(d, s, m, dc) => flange_nut(d, s, m, dc),
            Kind::CapNut(d, s, m) => cap_nut(d, s, m),
            Kind::Woodruff(b, h, dia) => woodruff(b, h, dia),
            Kind::ORing(d1, d2) => o_ring(d1, d2),
            Kind::Circlip(d, groove, t) => circlip(d, groove, t),
            Kind::Bearing(d, od, b) => {
                let (r0, r1, c) = (d / 2.0, od / 2.0, (od - d) / 40.0);
                ring(&[
                    (r0 + c, 0.0),
                    (r1 - c, 0.0),
                    (r1, c),
                    (r1, b - c),
                    (r1 - c, b),
                    (r0 + c, b),
                    (r0, b - c),
                    (r0, c),
                ])
            }
            Kind::Shape(family, size, length) => Library::builtin()
                .find(family, size)
                .ok_or_else(|| bad("the section is not in the table"))?
                .member_part(length, None),
        }
    }
}

/// The envelope: everything is one revolve, in milliseconds.
fn simple_part(kind: Kind) -> Result<PartDocument, PartError> {
    let head = |d: Real, l: Real, rk: Real, k: Real| {
        ring(&[
            (0.0, -l),
            (d / 2.0, -l),
            (d / 2.0, 0.0),
            (rk, 0.0),
            (rk, k),
            (0.0, k),
        ])
    };
    let corner = |s: Real| s / 3f64.sqrt();
    let cylinder = |d: Real, h: Real| ring(&[(0.0, 0.0), (d / 2.0, 0.0), (d / 2.0, h), (0.0, h)]);
    let tube = |d1: Real, d2: Real, h: Real| {
        ring(&[
            (d1 / 2.0, 0.0),
            (d2 / 2.0, 0.0),
            (d2 / 2.0, h),
            (d1 / 2.0, h),
        ])
    };
    match kind {
        Kind::HexBolt(d, l, s, k) | Kind::PartThreadBolt(d, l, s, k) => head(d, l, corner(s), k),
        Kind::FlangeBolt(d, l, _, k, dc) => head(d, l, dc / 2.0, k),
        Kind::SocketScrew(d, l, dk, k, _, _) | Kind::ButtonHead(d, l, dk, k, _) => {
            head(d, l, dk / 2.0, k)
        }
        Kind::Countersunk(d, l, dk, k, _) => ring(&[
            (0.0, -l),
            (d / 2.0, -l),
            (d / 2.0, -k),
            (dk / 2.0, 0.0),
            (0.0, 0.0),
        ]),
        Kind::SetScrew(d, l, _, _) | Kind::Rod(d, l) => cylinder(d, l),
        Kind::HexNut(d, s, m) | Kind::CapNut(d, s, m) => tube(d, 2.0 * corner(s), m),
        Kind::FlangeNut(d, _, m, dc) => tube(d, dc, m),
        Kind::SplitRing(d1, d2, h, _) | Kind::Circlip(d1, d2, h) => {
            // A retaining ring's columns are its shaft and groove.
            let (a, b) = if d1 < d2 {
                (d1, d2)
            } else {
                (d2, d1 + 0.12 * d1)
            };
            tube(a, b, h)
        }
        Kind::Bearing(d, od, b) => tube(d, od, b),
        other => standard_part(other),
    }
}

/// The hexagon's corners cut at 30° from a circle of 0.95 s by intersecting
/// with a turned blank (1.3–2 s in the kernel, so only at this level).
fn chamfer_blank(s: Real, h: Real, both: bool) -> Vec<(Real, Real)> {
    let (r0, r1) = (0.475 * s, s / 3f64.sqrt() + 0.2);
    let rise = (r1 - r0) * (30f64).to_radians().tan();
    let mut v = vec![(0.0, h), (r0, h), (r1, h - rise)];
    if both {
        v.extend([(r1, rise), (r0, 0.0), (0.0, 0.0)]);
    } else {
        v.extend([(r1, 0.0), (0.0, 0.0)]);
    }
    v
}

fn full_hex_bolt(
    d: Real,
    l: Real,
    s: Real,
    k: Real,
    thread: Option<Real>,
) -> Result<PartDocument, PartError> {
    let mut spec = thread_spec(d, false)?;
    let d = spec.major;
    if let Some(b) = thread.filter(|b| *b < l) {
        spec.full_depth = false;
        spec.depth = b;
    }
    let mut part = blank();
    extrude(
        &mut part,
        vec![hexagon(s)],
        Point2::new(0.0, 0.0),
        0.0,
        k,
        false,
        Operation::NewSolid,
    )?;
    revolve(&mut part, &chamfer_blank(s, k, false), Operation::Intersect)?;
    revolve(&mut part, &shank(d, l, k / 2.0), Operation::Join)?;
    add_thread(
        &mut part,
        spec,
        d / 2.0,
        Point3::new(d / 2.0, 0.0, -l + d / 5.0),
    )?;
    Ok(part)
}

fn full_hex_nut(d: Real, s: Real, m: Real) -> Result<PartDocument, PartError> {
    let spec = thread_spec(d, true)?;
    let bore = spec.minor;
    let mut part = blank();
    extrude(
        &mut part,
        vec![
            hexagon(s),
            Loop::Circle(Point2::new(0.0, 0.0), bore / 2.0, true),
        ],
        Point2::new(0.45 * s, 0.0),
        0.0,
        m,
        false,
        Operation::NewSolid,
    )?;
    revolve(&mut part, &chamfer_blank(s, m, true), Operation::Intersect)?;
    add_thread(
        &mut part,
        spec,
        bore / 2.0,
        Point3::new(bore / 2.0, 0.0, 0.0),
    )?;
    Ok(part)
}

/// Two rings with a gap and the row of rolling elements between them, as three
/// bodies; the row is one ring (separate balls add nothing seen from outside).
fn full_bearing(d: Real, od: Real, b: Real) -> Result<PartDocument, PartError> {
    let (r0, r1) = (d / 2.0, od / 2.0);
    let wall = (r1 - r0) * 0.28;
    let c = (r1 - r0) / 40.0;
    let mut part = blank();
    let section = |a: Real, e: Real| -> Vec<(Real, Real)> {
        vec![
            (a + c, 0.0),
            (e - c, 0.0),
            (e, c),
            (e, b - c),
            (e - c, b),
            (a + c, b),
            (a, b - c),
            (a, c),
        ]
    };
    revolve(&mut part, &section(r0, r0 + wall), Operation::NewSolid)?;
    revolve(&mut part, &section(r1 - wall, r1), Operation::NewSolid)?;
    let mid = (r0 + r1) / 2.0;
    let radius = ((r1 - r0) / 2.0 - wall) * 0.95;
    revolve_loop(
        &mut part,
        Loop::Circle(Point2::new(mid, b / 2.0), radius.min(b * 0.45), false),
        Operation::NewSolid,
    )?;
    Ok(part)
}

fn bad(msg: &str) -> PartError {
    PartError::Feature(msg.to_owned())
}

fn options() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

fn blank() -> PartDocument {
    PartDocument {
        exact_only: true,
        ..Default::default()
    }
}

/// A closed outline of lines, (radius, z), revolved about Z; a new solid or
/// joined to / intersected with the body.
fn revolve(
    part: &mut PartDocument,
    outline: &[(Real, Real)],
    op: Operation,
) -> Result<(), PartError> {
    let v = outline
        .iter()
        .map(|&(r, z)| (Point2::new(r, z), None))
        .collect();
    revolve_loop(part, Loop::Path(v), op)
}

fn revolve_loop(part: &mut PartDocument, outline: Loop, op: Operation) -> Result<(), PartError> {
    let i = part.create_sketch(
        SketchPlacement::on_origin_plane(OriginPlane::XZ),
        &options(),
    )?;
    draw_loop(&mut part.sketches[i].sketch, outline)?;
    let regions = part.sketches[i].sketch.regions();
    let r = regions
        .first()
        .ok_or_else(|| bad("the outline is not closed"))?;
    let mut f = RevolveFeature::new(
        vec![ProfileRef::of(r, r.interior_point())],
        AxisRef::Origin(2),
    );
    f.operation = op;
    part.add_revolve(i, f)?;
    Ok(())
}

/// A part that is one outline revolved about Z.
fn ring(outline: &[(Real, Real)]) -> Result<PartDocument, PartError> {
    let mut part = blank();
    revolve(&mut part, outline, Operation::NewSolid)?;
    Ok(part)
}

/// The hexagon's corners, across flats `s`, counter-clockwise.
fn hexagon(s: Real) -> Loop {
    let r = s / 3f64.sqrt();
    Loop::Path(
        (0..6)
            .map(|k| {
                let a = (k as Real) * std::f64::consts::FRAC_PI_3;
                (Point2::new(r * a.cos(), r * a.sin()), None)
            })
            .collect(),
    )
}

/// `loops` on the XY plane offset to `z`, extruded by `length` towards +Z
/// (or −Z when `down`); `seed` picks the region.
fn extrude(
    part: &mut PartDocument,
    loops: Vec<Loop>,
    seed: Point2,
    z: Real,
    length: Real,
    down: bool,
    op: Operation,
) -> Result<(), PartError> {
    let placement = if z.abs() < 1e-9 {
        SketchPlacement::on_origin_plane(OriginPlane::XY)
    } else {
        let w = part.add_work(hidden(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: z,
        })))?;
        SketchPlacement {
            plane: PlaneRef::Work(w),
            ..Default::default()
        }
    };
    let i = part.create_sketch(placement, &options())?;
    for l in loops {
        draw_loop(&mut part.sketches[i].sketch, l)?;
    }
    let sk = &part.sketches[i].sketch;
    let (flip, q) = (sk.plane.normal.z < 0.0, seed);
    let regions = sk.regions();
    let r = regions
        .iter()
        .find(|r| r.contains(q))
        .ok_or_else(|| bad("the section has no closed profile"))?;
    let mut e = ExtrudeFeature::new(vec![ProfileRef::of(r, q)], length);
    e.operation = op;
    if flip != down {
        e.direction = ExtentDirection::Flipped;
    }
    part.add_extrude(i, e)?;
    Ok(())
}

/// The inch threads of the American tables, by major diameter in mm.
const UNC: [(Real, &str); 9] = [
    (4.826, "#10-24 UNC"),
    (6.35, "1/4-20 UNC"),
    (7.938, "5/16-18 UNC"),
    (9.525, "3/8-16 UNC"),
    (11.112, "7/16-14 UNC"),
    (12.7, "1/2-13 UNC"),
    (15.875, "5/8-11 UNC"),
    (19.05, "3/4-10 UNC"),
    (25.4, "1-8 UNC"),
];

fn thread_spec(d: Real, internal: bool) -> Result<ThreadSpec, PartError> {
    if let Some((_, name)) = UNC.iter().find(|(m, _)| (m - d).abs() < 1e-3) {
        return ThreadSpec::from_table(Standards::global(), ANSI_UNIFIED, name, internal)
            .ok_or_else(|| bad("the thread is not in the table"));
    }
    ThreadSpec::from_table(Standards::global(), ISO_METRIC, &format!("M{d}"), internal)
        .ok_or_else(|| bad("the thread is not in the table"))
}

/// A cosmetic thread along the cylinder of `radius` nearest `hint`.
fn add_thread(
    part: &mut PartDocument,
    spec: ThreadSpec,
    radius: Real,
    hint: Point3,
) -> Result<(), PartError> {
    let on_it = part.body.faces().find(|(_, f)| {
        matches!(
            part.body.surface(f.surface),
            Some(Surface::Cylinder(c)) if (c.radius - radius).abs() < 1e-6
        )
    });
    let (fid, _) = on_it.ok_or_else(|| bad("the threaded cylinder was not found"))?;
    let face = crate::part::shell::face_ref(&part.body, fid, Some(hint))
        .ok_or_else(|| bad("the threaded face has no name"))?;
    part.add_thread(ThreadFeature::new(face, spec))?;
    Ok(())
}

/// Shank below the seat, its end chamfered; it reaches a little into the
/// head, so the Join has no coplanar faces.
fn shank(d: Real, l: Real, into: Real) -> Vec<(Real, Real)> {
    let (r, c) = (d / 2.0, d / 10.0);
    vec![(0.0, into), (r, into), (r, -l + c), (r - c, -l), (0.0, -l)]
}

fn hex_bolt(
    d: Real,
    l: Real,
    s: Real,
    k: Real,
    thread: Option<Real>,
) -> Result<PartDocument, PartError> {
    let mut spec = thread_spec(d, false)?;
    let d = spec.major;
    // Shorter than the shank: from the end; else the whole shank.
    if let Some(b) = thread.filter(|b| *b < l) {
        spec.full_depth = false;
        spec.depth = b;
    }
    let mut part = blank();
    extrude(
        &mut part,
        vec![hexagon(s)],
        Point2::new(0.0, 0.0),
        0.0,
        k,
        false,
        Operation::NewSolid,
    )?;
    revolve(&mut part, &shank(d, l, k / 2.0), Operation::Join)?;
    add_thread(
        &mut part,
        spec,
        d / 2.0,
        Point3::new(d / 2.0, 0.0, -l + d / 5.0),
    )?;
    Ok(part)
}

fn socket_screw(
    d: Real,
    l: Real,
    dk: Real,
    k: Real,
    s: Real,
    t: Real,
) -> Result<PartDocument, PartError> {
    let spec = thread_spec(d, false)?;
    let d = spec.major;
    let (r, rk, c, e) = (d / 2.0, dk / 2.0, d / 10.0, dk / 25.0);
    let mut part = blank();
    revolve(
        &mut part,
        &[
            (0.0, -l),
            (r - c, -l),
            (r, -l + c),
            (r, 0.0),
            (rk, 0.0),
            (rk, k - e),
            (rk - e, k),
            (0.0, k),
        ],
        Operation::NewSolid,
    )?;
    extrude(
        &mut part,
        vec![hexagon(s)],
        Point2::new(0.0, 0.0),
        k,
        t,
        true,
        Operation::Cut,
    )?;
    add_thread(&mut part, spec, r, Point3::new(r, 0.0, -l + 2.0 * c))?;
    Ok(part)
}

fn hex_nut(d: Real, s: Real, m: Real) -> Result<PartDocument, PartError> {
    let spec = thread_spec(d, true)?;
    let bore = spec.minor;
    let mut part = blank();
    extrude(
        &mut part,
        vec![
            hexagon(s),
            Loop::Circle(Point2::new(0.0, 0.0), bore / 2.0, true),
        ],
        Point2::new(0.45 * s, 0.0),
        0.0,
        m,
        false,
        Operation::NewSolid,
    )?;
    add_thread(
        &mut part,
        spec,
        bore / 2.0,
        Point3::new(bore / 2.0, 0.0, 0.0),
    )?;
    Ok(part)
}

/// Round head on the seat, plain shank below it.
fn rivet(d: Real, l: Real, dk: Real, k: Real) -> Result<PartDocument, PartError> {
    let (r, rk) = (d / 2.0, dk / 2.0);
    // The sphere through the head's rim and top; its centre lies on the axis.
    let rs = (rk * rk + k * k) / (2.0 * k);
    let centre = Point2::new(0.0, k - rs);
    let mut part = blank();
    revolve_loop(
        &mut part,
        Loop::Path(vec![
            (Point2::new(0.0, -l), None),
            (Point2::new(r, -l), None),
            (Point2::new(r, 0.0), None),
            (Point2::new(rk, 0.0), Some((centre, true))),
            (Point2::new(0.0, k), None),
        ]),
        Operation::NewSolid,
    )?;
    Ok(part)
}

/// An outline on the XY plane extruded along Z; `seed` lies in its material.
fn plate(outline: Loop, seed: Point2, length: Real) -> Result<PartDocument, PartError> {
    let mut part = blank();
    extrude(
        &mut part,
        vec![outline],
        seed,
        0.0,
        length,
        false,
        Operation::NewSolid,
    )?;
    Ok(part)
}

/// The section is the key seen from above; `h` is its height along Z.
fn key(b: Real, h: Real, l: Real) -> Result<PartDocument, PartError> {
    let (x, r) = ((l - b) / 2.0, b / 2.0);
    plate(
        Loop::Path(vec![
            (Point2::new(-x, -r), None),
            (Point2::new(x, -r), Some((Point2::new(x, 0.0), true))),
            (Point2::new(x, r), None),
            (Point2::new(-x, r), Some((Point2::new(-x, 0.0), true))),
        ]),
        Point2::new(0.0, 0.0),
        h,
    )
}

/// An open ring on the groove with a 30° gap, of constant width (the real
/// ring's lugs and taper are left out).
fn circlip(d: Real, groove: Real, t: Real) -> Result<PartDocument, PartError> {
    let (r0, r1) = (groove / 2.0, groove / 2.0 + 0.12 * d);
    let half = (15f64).to_radians();
    let at = |r: Real, a: Real| Point2::new(r * a.cos(), r * a.sin());
    let o = Point2::new(0.0, 0.0);
    plate(
        Loop::Path(vec![
            (at(r0, half), None),
            (at(r1, half), Some((o, true))),
            (at(r1, -half), None),
            (at(r0, -half), Some((o, false))),
        ]),
        at((r0 + r1) / 2.0, std::f64::consts::PI),
        t,
    )
}

/// Shank, then a cone up to the head's diameter; the seat is the head's top.
fn countersunk(d: Real, l: Real, dk: Real, k: Real, s: Real) -> Result<PartDocument, PartError> {
    let spec = thread_spec(d, false)?;
    let d = spec.major;
    let (r, c) = (d / 2.0, d / 10.0);
    let mut part = blank();
    revolve(
        &mut part,
        &[
            (0.0, -l),
            (r - c, -l),
            (r, -l + c),
            (r, -k),
            (dk / 2.0, 0.0),
            (0.0, 0.0),
        ],
        Operation::NewSolid,
    )?;
    extrude(
        &mut part,
        vec![hexagon(s)],
        Point2::new(0.0, 0.0),
        0.0,
        0.6 * k,
        true,
        Operation::Cut,
    )?;
    add_thread(&mut part, spec, r, Point3::new(r, 0.0, -l + 2.0 * c))?;
    Ok(part)
}

/// A shank under a low spherical head with a flat where the socket opens, as
/// real ones have: the kernel found no result cutting a hexagon through a dome.
fn button_head(d: Real, l: Real, dk: Real, k: Real, s: Real) -> Result<PartDocument, PartError> {
    let spec = thread_spec(d, false)?;
    let d = spec.major;
    let (r, rk, flat) = (d / 2.0, dk / 2.0, 0.65 * s);
    // The sphere through the rim and the flat's edge.
    let y0 = (k * k + flat * flat - rk * rk) / (2.0 * k);
    let centre = Point2::new(0.0, y0);
    let mut part = blank();
    revolve_loop(
        &mut part,
        Loop::Path(vec![
            (Point2::new(0.0, -l), None),
            (Point2::new(r, -l), None),
            (Point2::new(r, 0.0), None),
            (Point2::new(rk, 0.0), Some((centre, true))),
            (Point2::new(flat, k), None),
            (Point2::new(0.0, k), None),
        ]),
        Operation::NewSolid,
    )?;
    extrude(
        &mut part,
        vec![hexagon(s)],
        Point2::new(0.0, 0.0),
        k + 1.0,
        0.6 * k + 1.0,
        true,
        Operation::Cut,
    )?;
    add_thread(&mut part, spec, r, Point3::new(r, 0.0, -l / 2.0))?;
    Ok(part)
}

/// A threaded cylinder with a socket in its top (the point's shape is left out).
fn set_screw(d: Real, l: Real, s: Real, t: Real) -> Result<PartDocument, PartError> {
    let spec = thread_spec(d, false)?;
    let d = spec.major;
    let (r, c) = (d / 2.0, d / 10.0);
    let mut part = blank();
    revolve(
        &mut part,
        &[
            (0.0, 0.0),
            (r - c, 0.0),
            (r, c),
            (r, l - c),
            (r - c, l),
            (0.0, l),
        ],
        Operation::NewSolid,
    )?;
    extrude(
        &mut part,
        vec![hexagon(s)],
        Point2::new(0.0, 0.0),
        l,
        t.min(0.8 * l),
        true,
        Operation::Cut,
    )?;
    add_thread(&mut part, spec, r, Point3::new(r, 0.0, l / 2.0))?;
    Ok(part)
}

fn rod(d: Real, l: Real) -> Result<PartDocument, PartError> {
    let spec = thread_spec(d, false)?;
    let d = spec.major;
    let (r, c) = (d / 2.0, d / 10.0);
    let mut part = ring(&[
        (0.0, 0.0),
        (r - c, 0.0),
        (r, c),
        (r, l - c),
        (r - c, l),
        (0.0, l),
    ])?;
    add_thread(&mut part, spec, r, Point3::new(r, 0.0, l / 2.0))?;
    Ok(part)
}

/// An annulus along Z with a wedge-shaped slot about +X.
fn split_ring(d1: Real, d2: Real, h: Real, half: Real) -> Result<PartDocument, PartError> {
    let (r0, r1, half) = (d1 / 2.0, d2 / 2.0, half.to_radians());
    let at = |r: Real, a: Real| Point2::new(r * a.cos(), r * a.sin());
    let o = Point2::new(0.0, 0.0);
    plate(
        Loop::Path(vec![
            (at(r0, half), None),
            (at(r1, half), Some((o, true))),
            (at(r1, -half), None),
            (at(r0, -half), Some((o, false))),
        ]),
        at((r0 + r1) / 2.0, std::f64::consts::PI),
        h,
    )
}

/// Flange and shank in one revolve, the hexagon starting inside the flange
/// (joined the other way round the kernel took 3 s).
fn flange_bolt(d: Real, l: Real, s: Real, k: Real, dc: Real) -> Result<PartDocument, PartError> {
    let spec = thread_spec(d, false)?;
    let d = spec.major;
    let (r, c, tf) = (d / 2.0, d / 10.0, 0.25 * k);
    let mut part = blank();
    revolve(
        &mut part,
        &[
            (0.0, tf),
            (dc / 2.0, tf),
            (dc / 2.0, 0.0),
            (r, 0.0),
            (r, -l + c),
            (r - c, -l),
            (0.0, -l),
        ],
        Operation::NewSolid,
    )?;
    extrude(
        &mut part,
        vec![hexagon(s)],
        Point2::new(0.0, 0.0),
        tf / 2.0,
        k - tf / 2.0,
        false,
        Operation::Join,
    )?;
    add_thread(&mut part, spec, r, Point3::new(r, 0.0, -l + 2.0 * c))?;
    Ok(part)
}

/// A solid hexagon and flange, bored afterwards so bore and body stay one wall.
fn flange_nut(d: Real, s: Real, m: Real, dc: Real) -> Result<PartDocument, PartError> {
    let spec = thread_spec(d, true)?;
    let (bore, tf, inner) = (spec.minor, 0.25 * m, 0.4 * s);
    let mut part = blank();
    extrude(
        &mut part,
        vec![hexagon(s)],
        Point2::new(0.0, 0.0),
        0.0,
        m,
        false,
        Operation::NewSolid,
    )?;
    revolve(
        &mut part,
        &[
            (0.0, 0.0),
            (dc / 2.0, 0.0),
            (dc / 2.0, tf),
            (inner, tf),
            (inner, m / 2.0),
            (0.0, m / 2.0),
        ],
        Operation::Join,
    )?;
    let r = bore / 2.0;
    revolve(
        &mut part,
        &[(0.0, -1.0), (r, -1.0), (r, m + 1.0), (0.0, m + 1.0)],
        Operation::Cut,
    )?;
    add_thread(&mut part, spec, r, Point3::new(r, 0.0, 0.0))?;
    Ok(part)
}

/// A hexagon under a dome, with a blind threaded bore from the seat. The
/// dome's rim stands clear of the hexagon's top: on that plane its Join failed.
fn cap_nut(d: Real, s: Real, m: Real) -> Result<PartDocument, PartError> {
    let spec = thread_spec(d, true)?;
    let hex = 0.5 * m;
    let rim = hex + 0.08 * m;
    let rd = 0.45 * s;
    let top = m - rim;
    let rs = (rd * rd + top * top) / (2.0 * top);
    let centre = Point2::new(0.0, m - rs);
    let mut part = blank();
    revolve_loop(
        &mut part,
        Loop::Path(vec![
            (Point2::new(0.0, hex / 2.0), None),
            (Point2::new(rd, hex / 2.0), None),
            (Point2::new(rd, rim), Some((centre, true))),
            (Point2::new(0.0, m), None),
        ]),
        Operation::NewSolid,
    )?;
    extrude(
        &mut part,
        vec![hexagon(s)],
        Point2::new(0.0, 0.0),
        0.0,
        hex,
        false,
        Operation::Join,
    )?;
    let r = spec.minor / 2.0;
    revolve(
        &mut part,
        &[(0.0, -1.0), (r, -1.0), (r, 0.7 * m), (0.0, 0.7 * m)],
        Operation::Cut,
    )?;
    add_thread(&mut part, spec, r, Point3::new(r, 0.0, 0.0))?;
    Ok(part)
}

/// A circular segment `h` high on a circle of `dia`, extruded by its width.
fn woodruff(b: Real, h: Real, dia: Real) -> Result<PartDocument, PartError> {
    let r = dia / 2.0;
    let a = (r * r - (r - h) * (r - h)).max(0.0).sqrt();
    let centre = Point2::new(0.0, r - h);
    plate(
        Loop::Path(vec![
            (Point2::new(-a, 0.0), None),
            (Point2::new(a, 0.0), Some((centre, false))),
        ]),
        Point2::new(0.0, -h / 2.0),
        b,
    )
}

/// A circle revolved about Z.
fn o_ring(d1: Real, d2: Real) -> Result<PartDocument, PartError> {
    let mut part = blank();
    revolve_loop(
        &mut part,
        Loop::Circle(Point2::new((d1 + d2) / 2.0, d2 / 2.0), d2 / 2.0, false),
        Operation::NewSolid,
    )?;
    Ok(part)
}
