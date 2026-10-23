//! Physical quantities for parameters (Inventor's Unit Type dialog,
//! `docs/spec/0.5-parameters.md` §units): every value carries a
//! *dimension* — powers of length, mass, time, temperature, angle,
//! current, luminous intensity and amount of substance — and is stored in
//! the internal base units mm, kg, s, K, rad, A, cd, mol (mm and rad as
//! everywhere else in the program). A unit is a factor (and, for °C / °F,
//! an offset) to those base units with its dimension; unit specifications
//! like `N*m`, `kg/m^3`, `km/h`, `MPa`, `mm^2` combine named units with
//! SI prefixes, `*`, `/`, `^` and brackets.

use crate::kernel::Real;

/// Powers of the base quantities, in [`BASE`] order.
pub type Dim = [i8; 8];

/// The base quantities (and their internal units).
pub const BASE: [(&str, &str); 8] = [
    ("length", "mm"),
    ("mass", "kg"),
    ("time", "s"),
    ("temperature", "K"),
    ("angle", "rad"),
    ("current", "A"),
    ("luminous intensity", "cd"),
    ("amount of substance", "mol"),
];

const TH: usize = 3;
const A: usize = 4;
const I: usize = 5;
const J: usize = 6;
const N: usize = 7;

/// No dimension (a ratio, a count).
pub const NONE: Dim = [0; 8];

const fn d(l: i8, m: i8, t: i8) -> Dim {
    [l, m, t, 0, 0, 0, 0, 0]
}

const fn only(k: usize, p: i8) -> Dim {
    let mut x = [0; 8];
    x[k] = p;
    x
}

/// Dimension of a product.
pub fn mul(a: Dim, b: Dim) -> Dim {
    std::array::from_fn(|k| a[k] + b[k])
}

/// Dimension of a quotient.
pub fn div(a: Dim, b: Dim) -> Dim {
    std::array::from_fn(|k| a[k] - b[k])
}

/// Dimension of a power with an integer exponent.
pub fn pow(a: Dim, e: i8) -> Dim {
    std::array::from_fn(|k| a[k] * e)
}

/// Whether two dimensions can be added or assigned: equal, with angle
/// counted as a ratio (Inventor: rad is unitless).
pub fn compatible(a: Dim, b: Dim) -> bool {
    (0..8).all(|k| k == A || a[k] == b[k])
}

/// A unit: `internal = shown · factor + offset`, of dimension `dim`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitDef {
    /// Internal units per shown unit.
    pub factor: Real,
    /// Added after the factor (°C, °F: the zero of their scale in K).
    pub offset: Real,
    /// What kind of quantity it is.
    pub dim: Dim,
}

impl UnitDef {
    const fn new(factor: Real, dim: Dim) -> Self {
        Self {
            factor,
            offset: 0.0,
            dim,
        }
    }

    /// Internal value of a number shown in this unit.
    pub fn from_display(&self, v: Real) -> Real {
        v * self.factor + self.offset
    }

    /// Shown value of an internal value.
    pub fn to_display(&self, v: Real) -> Real {
        (v - self.offset) / self.factor
    }

    /// An angle unit.
    pub fn is_angle(&self) -> bool {
        self.dim == only(A, 1)
    }

    fn times(self, o: Self) -> Self {
        Self::new(self.factor * o.factor, mul(self.dim, o.dim))
    }

    fn over(self, o: Self) -> Self {
        Self::new(self.factor / o.factor, div(self.dim, o.dim))
    }

    fn powi(self, e: i8) -> Self {
        Self::new(self.factor.powi(e as i32), pow(self.dim, e))
    }
}

const PI: Real = std::f64::consts::PI;
const NEWTON: Real = 1000.0; // kg·m/s² in kg·mm/s²
const PASCAL: Real = 1e-3; // N/m² in kg/(mm·s²)
const JOULE: Real = 1e6; // N·m in kg·mm²/s²

/// Named units (before SI prefixes): name, factor, dimension.
const NAMED: &[(&str, Real, Dim)] = &[
    // Length
    ("mm", 1.0, d(1, 0, 0)),
    ("cm", 10.0, d(1, 0, 0)),
    ("m", 1000.0, d(1, 0, 0)),
    ("km", 1e6, d(1, 0, 0)),
    ("um", 1e-3, d(1, 0, 0)),
    ("µm", 1e-3, d(1, 0, 0)),
    ("nm", 1e-6, d(1, 0, 0)),
    ("in", 25.4, d(1, 0, 0)),
    ("inch", 25.4, d(1, 0, 0)),
    ("ft", 304.8, d(1, 0, 0)),
    ("foot", 304.8, d(1, 0, 0)),
    ("yd", 914.4, d(1, 0, 0)),
    ("mi", 1_609_344.0, d(1, 0, 0)),
    ("mile", 1_609_344.0, d(1, 0, 0)),
    ("mil", 0.0254, d(1, 0, 0)),
    ("nmi", 1_852_000.0, d(1, 0, 0)),
    // Mass
    ("kg", 1.0, d(0, 1, 0)),
    ("g", 1e-3, d(0, 1, 0)),
    ("t", 1000.0, d(0, 1, 0)),
    ("tonne", 1000.0, d(0, 1, 0)),
    ("lbmass", 0.453_592_37, d(0, 1, 0)),
    ("lb", 0.453_592_37, d(0, 1, 0)),
    ("oz", 0.028_349_523_125, d(0, 1, 0)),
    ("slug", 14.593_903, d(0, 1, 0)),
    // Time
    ("s", 1.0, d(0, 0, 1)),
    ("sec", 1.0, d(0, 0, 1)),
    ("min", 60.0, d(0, 0, 1)),
    ("h", 3600.0, d(0, 0, 1)),
    ("hr", 3600.0, d(0, 0, 1)),
    ("day", 86400.0, d(0, 0, 1)),
    ("Hz", 1.0, d(0, 0, -1)),
    ("rpm", 2.0 * PI / 60.0, [0, 0, -1, 0, 1, 0, 0, 0]),
    // Temperature (differences; °C / °F with offsets below)
    ("K", 1.0, only(TH, 1)),
    ("degR", 5.0 / 9.0, only(TH, 1)),
    // Angle
    ("rad", 1.0, only(A, 1)),
    ("deg", PI / 180.0, only(A, 1)),
    ("grad", PI / 200.0, only(A, 1)),
    ("rev", 2.0 * PI, only(A, 1)),
    ("turn", 2.0 * PI, only(A, 1)),
    ("arcmin", PI / 10_800.0, only(A, 1)),
    ("arcsec", PI / 648_000.0, only(A, 1)),
    ("sr", 1.0, only(A, 2)),
    // Volume
    ("l", 1e6, d(3, 0, 0)),
    ("L", 1e6, d(3, 0, 0)),
    ("gal", 3_785_411.784, d(3, 0, 0)),
    // Force
    ("N", NEWTON, d(1, 1, -2)),
    ("dyne", NEWTON * 1e-5, d(1, 1, -2)),
    ("lbforce", NEWTON * 4.448_221_615_260_5, d(1, 1, -2)),
    ("lbf", NEWTON * 4.448_221_615_260_5, d(1, 1, -2)),
    ("kgf", NEWTON * 9.806_65, d(1, 1, -2)),
    ("ozf", NEWTON * 0.278_013_850_953_8, d(1, 1, -2)),
    // Pressure
    ("Pa", PASCAL, d(-1, 1, -2)),
    ("bar", PASCAL * 1e5, d(-1, 1, -2)),
    ("atm", PASCAL * 101_325.0, d(-1, 1, -2)),
    ("psi", PASCAL * 6_894.757_293, d(-1, 1, -2)),
    ("ksi", PASCAL * 6_894_757.293, d(-1, 1, -2)),
    ("mmHg", PASCAL * 133.322_387, d(-1, 1, -2)),
    // Energy / work, power
    ("J", JOULE, d(2, 1, -2)),
    ("Wh", JOULE * 3600.0, d(2, 1, -2)),
    ("cal", JOULE * 4.184, d(2, 1, -2)),
    ("Btu", JOULE * 1055.056, d(2, 1, -2)),
    ("erg", JOULE * 1e-7, d(2, 1, -2)),
    ("W", JOULE, d(2, 1, -3)),
    ("hp", JOULE * 745.699_872, d(2, 1, -3)),
    // Electrical
    ("A", 1.0, only(I, 1)),
    ("C", 1.0, [0, 0, 1, 0, 0, 1, 0, 0]),
    ("V", JOULE, [2, 1, -3, 0, 0, -1, 0, 0]),
    ("ohm", JOULE, [2, 1, -3, 0, 0, -2, 0, 0]),
    ("F", 1.0 / JOULE, [-2, -1, 4, 0, 0, 2, 0, 0]),
    ("H", JOULE, [2, 1, -2, 0, 0, -2, 0, 0]),
    ("S", 1.0 / JOULE, [-2, -1, 3, 0, 0, 2, 0, 0]),
    ("Wb", JOULE, [2, 1, -2, 0, 0, -1, 0, 0]),
    ("T", 1.0, [0, 1, -2, 0, 0, -1, 0, 0]),
    // Luminosity
    ("cd", 1.0, only(J, 1)),
    ("lm", 1.0, [0, 0, 0, 0, 2, 0, 1, 0]),
    ("lx", 1e-6, [-2, 0, 0, 0, 2, 0, 1, 0]),
    // Substance
    ("mol", 1.0, only(N, 1)),
    // Unitless
    ("ul", 1.0, NONE),
    ("percent", 0.01, NONE),
];

/// SI prefixes (applied to the units in [`PREFIXABLE`]).
const PREFIXES: &[(&str, Real)] = &[
    ("G", 1e9),
    ("M", 1e6),
    ("k", 1e3),
    ("h", 1e2),
    ("da", 1e1),
    ("d", 1e-1),
    ("c", 1e-2),
    ("m", 1e-3),
    ("u", 1e-6),
    ("µ", 1e-6),
    ("n", 1e-9),
];

const PREFIXABLE: &[&str] = &[
    "m", "g", "s", "N", "Pa", "J", "W", "Wh", "A", "V", "ohm", "C", "F", "H", "Hz", "l", "L",
    "mol", "cd", "lm", "lx", "T", "Wb", "S", "bar", "cal",
];

/// One named unit, with an SI prefix if it takes one (`kN`, `MPa`, `mA`).
pub fn named(name: &str) -> Option<UnitDef> {
    if let Some(&(_, f, d)) = NAMED.iter().find(|(n, ..)| *n == name) {
        return Some(UnitDef::new(f, d));
    }
    match name {
        "degC" | "°C" | "C°" => {
            return Some(UnitDef {
                factor: 1.0,
                offset: 273.15,
                dim: only(TH, 1),
            });
        }
        "degF" | "°F" => {
            return Some(UnitDef {
                factor: 5.0 / 9.0,
                offset: 273.15 - 32.0 * 5.0 / 9.0,
                dim: only(TH, 1),
            });
        }
        _ => {}
    }
    PREFIXES.iter().find_map(|(p, f)| {
        let base = name.strip_prefix(p)?;
        PREFIXABLE.contains(&base).then(|| {
            let u = named(base)?;
            Some(UnitDef::new(u.factor * f, u.dim))
        })?
    })
}

/// Parses a unit specification: named units joined by `*` / `·` / `/`,
/// powers `^n` (also `²`, `³`), brackets; `1/s` and the empty or `ul`
/// specification (unitless). An offset unit (°C, °F) must stand alone.
pub fn parse_spec(spec: &str) -> Result<UnitDef, String> {
    let s = spec
        .trim()
        .replace('²', "^2")
        .replace('³', "^3")
        .replace('·', "*");
    if s.is_empty() {
        return Ok(UnitDef::new(1.0, NONE));
    }
    if let Some(u) = named(&s) {
        return Ok(u);
    }
    let cs: Vec<char> = s.chars().collect();
    let mut pos = 0;
    let u = spec_product(&cs, &mut pos)?;
    if pos != cs.len() {
        return Err(format!("'{spec}' is not a unit"));
    }
    Ok(u)
}

fn skip_ws(cs: &[char], pos: &mut usize) {
    while cs.get(*pos).is_some_and(|c| c.is_whitespace()) {
        *pos += 1;
    }
}

fn spec_product(cs: &[char], pos: &mut usize) -> Result<UnitDef, String> {
    let mut u = spec_power(cs, pos)?;
    loop {
        skip_ws(cs, pos);
        match cs.get(*pos) {
            Some('*') => {
                *pos += 1;
                u = u.times(spec_power(cs, pos)?);
            }
            Some('/') => {
                *pos += 1;
                u = u.over(spec_power(cs, pos)?);
            }
            _ => return Ok(u),
        }
    }
}

fn spec_power(cs: &[char], pos: &mut usize) -> Result<UnitDef, String> {
    let base = spec_atom(cs, pos)?;
    skip_ws(cs, pos);
    if cs.get(*pos) == Some(&'^') {
        *pos += 1;
        skip_ws(cs, pos);
        let start = *pos;
        if cs.get(*pos) == Some(&'-') {
            *pos += 1;
        }
        while cs.get(*pos).is_some_and(char::is_ascii_digit) {
            *pos += 1;
        }
        let e: i8 = cs[start..*pos]
            .iter()
            .collect::<String>()
            .parse()
            .map_err(|_| "a unit's power must be a whole number".to_string())?;
        return Ok(base.powi(e));
    }
    Ok(base)
}

fn spec_atom(cs: &[char], pos: &mut usize) -> Result<UnitDef, String> {
    skip_ws(cs, pos);
    match cs.get(*pos) {
        Some('(') => {
            *pos += 1;
            let u = spec_product(cs, pos)?;
            skip_ws(cs, pos);
            if cs.get(*pos) != Some(&')') {
                return Err("missing ')' in the unit".into());
            }
            *pos += 1;
            Ok(u)
        }
        Some('1') => {
            *pos += 1;
            Ok(UnitDef::new(1.0, NONE))
        }
        _ => {
            let start = *pos;
            while cs
                .get(*pos)
                .is_some_and(|c| c.is_alphabetic() || *c == '°' || *c == 'µ')
            {
                *pos += 1;
            }
            let name: String = cs[start..*pos].iter().collect();
            if name.is_empty() {
                return Err("a unit name is missing".into());
            }
            let u = named(&name).ok_or_else(|| format!("unknown unit '{name}'"))?;
            if u.offset != 0.0 {
                return Err(format!("'{name}' cannot be combined with other units"));
            }
            Ok(u)
        }
    }
}

/// The quantity a dimension is (Inventor's unit categories), or a
/// description from the base powers.
pub fn category(dim: Dim) -> String {
    let known: &[(&str, Dim)] = &[
        ("unitless", NONE),
        ("a length", d(1, 0, 0)),
        ("a mass", d(0, 1, 0)),
        ("a time", d(0, 0, 1)),
        ("a temperature", only(TH, 1)),
        ("an angle", only(A, 1)),
        ("an area", d(2, 0, 0)),
        ("a volume", d(3, 0, 0)),
        ("a velocity", d(1, 0, -1)),
        ("an acceleration", d(1, 0, -2)),
        ("a force", d(1, 1, -2)),
        ("a pressure", d(-1, 1, -2)),
        ("energy (work, torque)", d(2, 1, -2)),
        ("a power", d(2, 1, -3)),
        ("a density", d(-3, 1, 0)),
        ("a frequency", d(0, 0, -1)),
        ("a force per length (stiffness)", d(0, 1, -2)),
        ("a current", only(I, 1)),
        ("a voltage", [2, 1, -3, 0, 0, -1, 0, 0]),
        ("a luminous intensity", only(J, 1)),
        ("an amount of substance", only(N, 1)),
    ];
    if let Some((n, _)) = known.iter().find(|(_, k)| *k == dim) {
        return (*n).to_owned();
    }
    let parts: Vec<String> = (0..8)
        .filter(|&k| dim[k] != 0)
        .map(|k| match dim[k] {
            1 => BASE[k].1.to_owned(),
            p => format!("{}^{p}", BASE[k].1),
        })
        .collect();
    parts.join("·")
}

/// The unit categories of the Unit Type dialog with common units each.
pub const CATEGORIES: &[(&str, &[&str])] = &[
    (
        "Length",
        &["mm", "cm", "m", "km", "um", "in", "ft", "yd", "mi", "mil"],
    ),
    ("Mass", &["kg", "g", "t", "lbmass", "oz", "slug"]),
    ("Time", &["s", "ms", "min", "h", "day"]),
    ("Temperature", &["degC", "degF", "K", "degR"]),
    (
        "Angularity",
        &["deg", "rad", "grad", "rev", "arcmin", "arcsec"],
    ),
    ("Velocity", &["mm/s", "m/s", "km/h", "mi/h", "ft/s", "rpm"]),
    ("Acceleration", &["m/s^2", "mm/s^2", "ft/s^2"]),
    ("Area", &["mm^2", "cm^2", "m^2", "in^2", "ft^2"]),
    (
        "Volume",
        &["mm^3", "cm^3", "m^3", "l", "ml", "in^3", "ft^3", "gal"],
    ),
    ("Density", &["kg/m^3", "g/cm^3", "lbmass/in^3"]),
    ("Force", &["N", "kN", "lbforce", "kgf", "dyne"]),
    ("Force per Length", &["N/mm", "N/m", "kN/m", "lbforce/in"]),
    (
        "Pressure",
        &[
            "Pa", "kPa", "MPa", "GPa", "N/mm^2", "bar", "psi", "ksi", "atm",
        ],
    ),
    (
        "Torque / Work",
        &["N*m", "N*mm", "J", "kJ", "Wh", "kWh", "cal", "lbforce*in"],
    ),
    ("Power", &["W", "kW", "hp"]),
    ("Frequency", &["Hz", "kHz"]),
    ("Electrical", &["A", "mA", "V", "kV", "ohm", "C", "F", "H"]),
    ("Luminosity", &["cd", "lm", "lx"]),
    ("Substance", &["mol"]),
    ("Unitless", &["ul", "percent"]),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Real, b: Real) -> bool {
        (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn specs_and_conversions() {
        let n_per_m = parse_spec("N/m").unwrap();
        let n_per_mm = parse_spec("N/mm").unwrap();
        assert_eq!(n_per_m.dim, n_per_mm.dim);
        // 1 N/mm = 1000 N/m.
        assert!(close(
            n_per_m.to_display(n_per_mm.from_display(1.0)),
            1000.0
        ));
        let mpa = parse_spec("MPa").unwrap();
        let nmm2 = parse_spec("N/mm^2").unwrap();
        assert!(close(mpa.factor, nmm2.factor) && mpa.dim == nmm2.dim);
        let kmh = parse_spec("km/h").unwrap();
        let ms = parse_spec("m/s").unwrap();
        assert!(close(ms.to_display(kmh.from_display(36.0)), 10.0));
        let rho = parse_spec("kg/m^3").unwrap();
        let gcc = parse_spec("g/cm^3").unwrap();
        assert!(close(rho.to_display(gcc.from_display(7.85)), 7850.0));
        assert!(close(
            parse_spec("N*m").unwrap().factor,
            parse_spec("J").unwrap().factor
        ));
        let c = parse_spec("degC").unwrap();
        let f = parse_spec("degF").unwrap();
        assert!(close(f.to_display(c.from_display(100.0)), 212.0));
        assert!(parse_spec("degC/s").is_err());
        assert!(parse_spec("furlong").is_err());
        assert_eq!(parse_spec("").unwrap().dim, NONE);
        assert_eq!(category(parse_spec("kN").unwrap().dim), "a force");
        for (_, units) in CATEGORIES {
            for u in *units {
                assert!(parse_spec(u).is_ok(), "{u}");
            }
        }
    }
}
