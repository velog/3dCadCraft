//! Parameter expressions (Inventor's equations): `d0 / 2 + 3 mm`,
//! `2 * wall`, `atan(h / w)`, `45 deg`.
//!
//! Grammar (usual precedence, `^` right-associative):
//! `expr := term (('+'|'-') term)*`, `term := unary (('*'|'/') unary)*`,
//! `unary := '-' unary | power`, `power := atom ('^' unary)?`,
//! `atom := number unit? | name unit? | name '(' args ')' | '(' expr ')' unit?`.
//!
//! Values are in internal units: millimetres and radians. A unit after a
//! number converts it (`1 in` = 25.4); a bare number takes the unit of the
//! parameter being evaluated (mm for lengths, degrees for angles).

use std::collections::HashMap;

use crate::kernel::Real;

/// The kind of quantity a parameter holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Unit {
    /// Length (mm).
    #[default]
    Mm,
    /// Angle (shown and entered in degrees, stored in radians).
    Deg,
    /// Unitless.
    Ul,
    /// Length shown in centimetres (format revision 17).
    Cm,
    /// Length shown in metres (format revision 17).
    M,
    /// Length shown in inches (format revision 17).
    In,
    /// Length shown in feet (format revision 17).
    Ft,
    /// Angle shown in radians (format revision 17).
    Rad,
}

impl Unit {
    /// Every unit, in the order the Parameters table offers them.
    pub const ALL: [Unit; 8] = [
        Unit::Mm,
        Unit::Cm,
        Unit::M,
        Unit::In,
        Unit::Ft,
        Unit::Deg,
        Unit::Rad,
        Unit::Ul,
    ];

    /// The name typed after a value and shown in the table.
    pub fn label(self) -> &'static str {
        match self {
            Unit::Mm => "mm",
            Unit::Cm => "cm",
            Unit::M => "m",
            Unit::In => "in",
            Unit::Ft => "ft",
            Unit::Deg => "deg",
            Unit::Rad => "rad",
            Unit::Ul => "ul",
        }
    }

    /// An angle (stored in radians).
    pub fn is_angle(self) -> bool {
        matches!(self, Unit::Deg | Unit::Rad)
    }

    /// A length (stored in millimetres).
    pub fn is_length(self) -> bool {
        matches!(self, Unit::Mm | Unit::Cm | Unit::M | Unit::In | Unit::Ft)
    }

    /// Internal units (mm, rad) per displayed unit.
    fn factor(self) -> Real {
        match self {
            Unit::Mm | Unit::Ul | Unit::Rad => 1.0,
            Unit::Cm => 10.0,
            Unit::M => 1000.0,
            Unit::In => 25.4,
            Unit::Ft => 304.8,
            Unit::Deg => std::f64::consts::PI / 180.0,
        }
    }

    /// Internal value of a bare number typed in this unit.
    pub fn from_display(self, v: Real) -> Real {
        v * self.factor()
    }

    /// Display value of an internal value.
    pub fn to_display(self, v: Real) -> Real {
        v / self.factor()
    }
}

/// An expression error, for the status bar / a red table cell.
pub type ExprError = String;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(Real),
    Name(String),
    Op(char),
}

fn lex(s: &str) -> Result<Vec<Tok>, ExprError> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' || c == ',') && cs.get(i + 1).is_some_and(char::is_ascii_digit)
        {
            let start = i;
            // ',' is a decimal separator only between digits (`0,6`), else
            // it separates function arguments.
            while i < cs.len()
                && (cs[i].is_ascii_digit()
                    || cs[i] == '.'
                    || (cs[i] == ',' && cs.get(i + 1).is_some_and(char::is_ascii_digit)))
            {
                i += 1;
            }
            // Exponent: 1e-3
            if i < cs.len() && (cs[i] == 'e' || cs[i] == 'E') {
                let j = i + 1;
                let k = if cs.get(j).is_some_and(|c| *c == '-' || *c == '+') {
                    j + 1
                } else {
                    j
                };
                if cs.get(k).is_some_and(char::is_ascii_digit) {
                    i = k;
                    while i < cs.len() && cs[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = cs[start..i].iter().collect::<String>().replace(',', ".");
            out.push(Tok::Num(
                text.parse().map_err(|_| format!("bad number '{text}'"))?,
            ));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push(Tok::Name(cs[start..i].iter().collect()));
        } else if "+-*/^(),".contains(c) {
            out.push(Tok::Op(c));
            i += 1;
        } else if c == ';' {
            out.push(Tok::Op(',')); // argument separator with decimal commas
            i += 1;
        } else if c == '°' {
            out.push(Tok::Name("deg".into()));
            i += 1;
        } else {
            return Err(format!("unexpected '{c}'"));
        }
    }
    Ok(out)
}

/// Units recognised after a value: (name, factor to internal units, is angle).
fn unit_factor(name: &str) -> Option<(Real, bool)> {
    Some(match name {
        "mm" => (1.0, false),
        "cm" => (10.0, false),
        "m" => (1000.0, false),
        "in" => (25.4, false),
        "ft" => (304.8, false),
        "um" => (0.001, false),
        "deg" => (std::f64::consts::PI / 180.0, true),
        "rad" => (1.0, true),
        "ul" => (1.0, false),
        _ => return None,
    })
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    vars: &'a dyn Fn(&str) -> Option<Real>,
    /// Unit of a bare number at the top level.
    unit: Unit,
    /// Names the expression uses (for dependency order).
    used: Vec<String>,
}

/// A value and whether it carries a unit (a bare number does not).
#[derive(Clone, Copy)]
struct Q {
    v: Real,
    unit: bool,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Tok::Op(c)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    /// A bare number meeting a value with a unit in a sum takes the unit
    /// of the parameter (`d0 + 3` = d0 + 3 mm, `a + 15` = a + 15°).
    fn align(&self, x: Q, y: Q) -> (Real, Real, bool) {
        match (x.unit, y.unit) {
            (true, false) => (x.v, self.unit.from_display(y.v), true),
            (false, true) => (self.unit.from_display(x.v), y.v, true),
            (u, _) => (x.v, y.v, u),
        }
    }
    fn expr(&mut self) -> Result<Q, ExprError> {
        let mut q = self.term()?;
        loop {
            let plus = self.eat('+');
            if !plus && !self.eat('-') {
                return Ok(q);
            }
            let r = self.term()?;
            let (a, b, unit) = self.align(q, r);
            q = Q {
                v: if plus { a + b } else { a - b },
                unit,
            };
        }
    }
    fn term(&mut self) -> Result<Q, ExprError> {
        let mut q = self.unary()?;
        loop {
            if self.eat('*') {
                let r = self.unary()?;
                q = Q {
                    v: q.v * r.v,
                    unit: q.unit || r.unit,
                };
            } else if self.eat('/') {
                let d = self.unary()?;
                if d.v == 0.0 {
                    return Err("division by zero".into());
                }
                // length / length is a ratio; length / number a length
                q = Q {
                    v: q.v / d.v,
                    unit: q.unit && !d.unit,
                };
            } else {
                return Ok(q);
            }
        }
    }
    fn unary(&mut self) -> Result<Q, ExprError> {
        if self.eat('-') {
            let q = self.unary()?;
            return Ok(Q { v: -q.v, ..q });
        }
        if self.eat('+') {
            return self.unary();
        }
        let base = self.atom()?;
        if self.eat('^') {
            let e = self.unary()?;
            return Ok(Q {
                v: base.v.powf(e.v),
                unit: base.unit,
            });
        }
        Ok(base)
    }
    /// Applies a unit name following a value, if there is one.
    fn unit_suffix(&mut self, q: Q) -> Q {
        if let Some(Tok::Name(n)) = self.peek().cloned()
            && let Some((f, _)) = unit_factor(&n)
        {
            self.pos += 1;
            return Q {
                v: q.v * f,
                unit: n != "ul",
            };
        }
        q
    }
    fn atom(&mut self) -> Result<Q, ExprError> {
        match self.peek().cloned() {
            Some(Tok::Num(v)) => {
                self.pos += 1;
                Ok(self.unit_suffix(Q { v, unit: false }))
            }
            Some(Tok::Op('(')) => {
                self.pos += 1;
                let q = self.expr()?;
                if !self.eat(')') {
                    return Err("missing ')'".into());
                }
                Ok(self.unit_suffix(q))
            }
            Some(Tok::Name(n)) => {
                self.pos += 1;
                if self.eat('(') {
                    let mut args = vec![self.expr()?];
                    while self.eat(',') {
                        args.push(self.expr()?);
                    }
                    if !self.eat(')') {
                        return Err("missing ')'".into());
                    }
                    return call(&n, &args);
                }
                if n == "PI" || n == "pi" {
                    return Ok(Q {
                        v: std::f64::consts::PI,
                        unit: false,
                    });
                }
                if n == "E" {
                    return Ok(Q {
                        v: std::f64::consts::E,
                        unit: false,
                    });
                }
                if unit_factor(&n).is_some() {
                    return Err(format!("'{n}' is a unit, a value must come before it"));
                }
                self.used.push(n.clone());
                let v = (self.vars)(&n).ok_or_else(|| format!("unknown parameter '{n}'"))?;
                Ok(self.unit_suffix(Q { v, unit: true }))
            }
            Some(Tok::Op(c)) => Err(format!("unexpected '{c}'")),
            None => Err("incomplete expression".into()),
        }
    }
}

fn call(f: &str, a: &[Q]) -> Result<Q, ExprError> {
    let one = || {
        if a.len() == 1 {
            Ok(a[0])
        } else {
            Err(format!("{f}() takes one argument"))
        }
    };
    // Trigonometry takes radians; a bare number is read as degrees
    // (sin(30) = sin(30 deg)), as Inventor users write it.
    let angle = || one().map(|q| if q.unit { q.v } else { q.v.to_radians() });
    let ratio = |v: Real| Q { v, unit: false };
    let angle_out = |v: Real| Q { v, unit: true };
    Ok(match f {
        "sin" => ratio(angle()?.sin()),
        "cos" => ratio(angle()?.cos()),
        "tan" => ratio(angle()?.tan()),
        "asin" => angle_out(one()?.v.asin()),
        "acos" => angle_out(one()?.v.acos()),
        "atan" => angle_out(one()?.v.atan()),
        "sqrt" => {
            let x = one()?;
            if x.v < 0.0 {
                return Err("sqrt of a negative value".into());
            }
            Q { v: x.v.sqrt(), ..x }
        }
        "abs" => {
            let x = one()?;
            Q { v: x.v.abs(), ..x }
        }
        "floor" => {
            let x = one()?;
            Q {
                v: x.v.floor(),
                ..x
            }
        }
        "ceil" => {
            let x = one()?;
            Q { v: x.v.ceil(), ..x }
        }
        "round" => {
            let x = one()?;
            Q {
                v: x.v.round(),
                ..x
            }
        }
        "sinh" => ratio(one()?.v.sinh()),
        "cosh" => ratio(one()?.v.cosh()),
        "tanh" => ratio(one()?.v.tanh()),
        "asinh" => ratio(one()?.v.asinh()),
        "acosh" => ratio(one()?.v.acosh()),
        "atanh" => ratio(one()?.v.atanh()),
        "exp" => ratio(one()?.v.exp()),
        "ln" | "log" => {
            let x = one()?;
            if x.v <= 0.0 {
                return Err(format!("{f} of a value that is not positive"));
            }
            ratio(if f == "ln" { x.v.ln() } else { x.v.log10() })
        }
        "sign" => {
            let x = one()?;
            ratio(if x.v > 0.0 {
                1.0
            } else if x.v < 0.0 {
                -1.0
            } else {
                0.0
            })
        }
        "pow" => {
            let [x, y] = a else {
                return Err("pow() takes two arguments".into());
            };
            Q {
                v: x.v.powf(y.v),
                unit: x.unit,
            }
        }
        "min" if !a.is_empty() => Q {
            v: a.iter().map(|q| q.v).fold(Real::INFINITY, Real::min),
            unit: a.iter().any(|q| q.unit),
        },
        "max" if !a.is_empty() => Q {
            v: a.iter().map(|q| q.v).fold(Real::NEG_INFINITY, Real::max),
            unit: a.iter().any(|q| q.unit),
        },
        _ => return Err(format!("unknown function '{f}'")),
    })
}

/// Evaluates `text` for a parameter of `unit`, looking names up in `vars`.
/// Returns the value (internal units) and the names it used.
pub fn eval(
    text: &str,
    unit: Unit,
    vars: &dyn Fn(&str) -> Option<Real>,
) -> Result<(Real, Vec<String>), ExprError> {
    let toks = lex(text)?;
    if toks.is_empty() {
        return Err("empty expression".into());
    }
    let mut p = Parser {
        toks,
        pos: 0,
        vars,
        unit,
        used: Vec::new(),
    };
    let q = p.expr()?;
    if p.pos != p.toks.len() {
        return Err("unexpected text at the end".into());
    }
    // A bare result takes the parameter's unit ("45" for an angle = 45°).
    let v = if q.unit { q.v } else { unit.from_display(q.v) };
    if !v.is_finite() {
        return Err("the result is not a finite number".into());
    }
    Ok((v, p.used))
}

/// The parameter names an expression refers to (no evaluation errors for
/// unknown names).
pub fn names_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(toks) = lex(text) {
        for (i, t) in toks.iter().enumerate() {
            if let Tok::Name(n) = t
                && toks.get(i + 1) != Some(&Tok::Op('('))
                && unit_factor(n).is_none()
                && n != "PI"
                && n != "pi"
                && n != "E"
            {
                out.push(n.clone());
            }
        }
    }
    out
}

/// Is `text` a plain number (possibly with a unit), not an equation?
pub fn is_plain_number(text: &str) -> bool {
    matches!(
        lex(text).as_deref(),
        Ok([Tok::Num(_)]) | Ok([Tok::Num(_), Tok::Name(_)]) | Ok([Tok::Op('-'), Tok::Num(_)])
    )
}

/// Is `name` usable as a parameter name?
pub fn valid_name(name: &str) -> bool {
    let mut cs = name.chars();
    cs.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && cs.all(|c| c.is_alphanumeric() || c == '_')
        && unit_factor(name).is_none()
        && !matches!(
            name,
            "PI" | "pi"
                | "E"
                | "sinh"
                | "cosh"
                | "tanh"
                | "asinh"
                | "acosh"
                | "atanh"
                | "exp"
                | "ln"
                | "log"
                | "sign"
                | "pow"
                | "sin"
                | "cos"
                | "tan"
                | "asin"
                | "acos"
                | "atan"
                | "sqrt"
                | "abs"
                | "floor"
                | "ceil"
                | "round"
                | "min"
                | "max"
        )
}

/// Evaluates a set of named expressions in dependency order. Returns the
/// values, and per-name errors (cycles, unknown names, bad syntax). A name
/// whose expression fails keeps no value, and so do the names using it.
pub fn eval_all(
    exprs: &[(String, String, Unit)],
) -> (HashMap<String, Real>, HashMap<String, ExprError>) {
    let by_name: HashMap<&str, usize> = exprs
        .iter()
        .enumerate()
        .map(|(i, (n, _, _))| (n.as_str(), i))
        .collect();
    let mut values: HashMap<String, Real> = HashMap::new();
    let mut errors: HashMap<String, ExprError> = HashMap::new();
    // 0 = new, 1 = in progress, 2 = done
    let mut state = vec![0u8; exprs.len()];
    fn visit(
        i: usize,
        exprs: &[(String, String, Unit)],
        by_name: &HashMap<&str, usize>,
        state: &mut [u8],
        values: &mut HashMap<String, Real>,
        errors: &mut HashMap<String, ExprError>,
    ) {
        if state[i] == 2 {
            return;
        }
        let (name, text, unit) = &exprs[i];
        if state[i] == 1 {
            errors.insert(name.clone(), "circular reference".into());
            return;
        }
        state[i] = 1;
        for dep in names_in(text) {
            if let Some(&j) = by_name.get(dep.as_str()) {
                visit(j, exprs, by_name, state, values, errors);
                if errors.contains_key(&dep) && !errors.contains_key(name) {
                    let why = if state[j] == 1 {
                        "circular reference".to_owned()
                    } else {
                        format!("depends on '{dep}', which has an error")
                    };
                    errors.insert(name.clone(), why);
                }
            }
        }
        if !errors.contains_key(name) {
            match eval(text, *unit, &|n| values.get(n).copied()) {
                Ok((v, _)) => {
                    values.insert(name.clone(), v);
                }
                Err(e) => {
                    errors.insert(name.clone(), e);
                }
            }
        }
        state[i] = 2;
    }
    for i in 0..exprs.len() {
        visit(i, exprs, &by_name, &mut state, &mut values, &mut errors);
    }
    (values, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str, u: Unit) -> Real {
        let vars = |n: &str| match n {
            "d0" => Some(40.0),
            "a" => Some(30f64.to_radians()),
            _ => None,
        };
        eval(s, u, &vars).unwrap().0
    }

    #[test]
    fn arithmetic_units_functions() {
        assert_eq!(ev("2 + 3 * 4", Unit::Mm), 14.0);
        assert_eq!(ev("(2 + 3) * 4", Unit::Mm), 20.0);
        assert_eq!(ev("2 ^ 3 ^ 2", Unit::Ul), 512.0);
        assert_eq!(ev("-d0 / 2", Unit::Mm), -20.0);
        assert_eq!(ev("1 in + 0,6 mm", Unit::Mm), 26.0);
        assert_eq!(ev("d0 / 2 + 3 mm", Unit::Mm), 23.0);
        assert!(
            (ev("45", Unit::Deg) - 45f64.to_radians()).abs() < 1e-12,
            "bare → degrees"
        );
        assert!((ev("a * 2", Unit::Deg) - 60f64.to_radians()).abs() < 1e-12);
        assert!((ev("sin(a) * d0", Unit::Mm) - 20.0).abs() < 1e-9);
        assert!(
            (ev("sin(30) * d0", Unit::Mm) - 20.0).abs() < 1e-9,
            "bare trig arg = degrees"
        );
        assert!((ev("a + 15", Unit::Deg) - 45f64.to_radians()).abs() < 1e-12);
        assert!((ev("atan(1)", Unit::Deg) - 45f64.to_radians()).abs() < 1e-12);
        assert_eq!(ev("max(1, 5, 3)", Unit::Ul), 5.0);
        assert!((ev("2 * PI", Unit::Ul) - std::f64::consts::TAU).abs() < 1e-12);
        assert!((ev("90°", Unit::Deg) - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        for bad in ["", "2 +", "(1", "foo", "1 / 0", "2 3", "mm"] {
            assert!(eval(bad, Unit::Mm, &|_| None).is_err(), "{bad}");
        }
        assert!(is_plain_number("12.5") && is_plain_number("3 mm") && !is_plain_number("d0/2"));
        assert!(valid_name("wall_2") && !valid_name("2x") && !valid_name("mm"));
    }

    #[test]
    fn dependency_order_and_cycles() {
        let e = |n: &str, t: &str| (n.to_owned(), t.to_owned(), Unit::Mm);
        let (v, err) = eval_all(&[e("c", "b * 2"), e("b", "a + 1"), e("a", "10")]);
        assert_eq!((v["a"], v["b"], v["c"]), (10.0, 11.0, 22.0));
        assert!(err.is_empty());
        let (v, err) = eval_all(&[e("x", "y"), e("y", "x"), e("z", "5")]);
        assert!(err.contains_key("x") && err.contains_key("y"));
        assert_eq!(v["z"], 5.0);
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    /// A bare number takes the parameter's unit; every unit converts to
    /// mm / rad and back.
    #[test]
    fn parameter_units_convert() {
        let none = |_: &str| None;
        for (u, typed, internal) in [
            (Unit::Mm, "2", 2.0),
            (Unit::Cm, "2", 20.0),
            (Unit::M, "0.5", 500.0),
            (Unit::In, "2", 50.8),
            (Unit::Ft, "1", 304.8),
            (Unit::Rad, "1", 1.0),
            (Unit::Deg, "90", std::f64::consts::FRAC_PI_2),
            (Unit::Ul, "3", 3.0),
        ] {
            let (v, _) = eval(typed, u, &none).unwrap();
            assert!((v - internal).abs() < 1e-9, "{u:?}");
            assert!((u.to_display(v) - typed.parse::<Real>().unwrap()).abs() < 1e-9);
        }
        // A unit in the text wins over the parameter's.
        let (v, _) = eval("1 in + 1", Unit::Cm, &none).unwrap();
        assert!((v - 35.4).abs() < 1e-9, "1 in + 1 cm");
        assert_eq!(Unit::ALL.len(), 8);
        assert!(Unit::Rad.is_angle() && Unit::Ft.is_length() && !Unit::Ul.is_length());
        // More functions and E.
        for (t, v) in [
            ("pow(2, 10)", 1024.0),
            ("ln(E)", 1.0),
            ("log(1000)", 3.0),
            ("exp(0)", 1.0),
            ("sign(-3)", -1.0),
            ("tanh(0)", 0.0),
        ] {
            let (x, _) = eval(t, Unit::Ul, &none).unwrap();
            assert!((x - v).abs() < 1e-12, "{t} = {x}");
        }
        assert!(eval("ln(0)", Unit::Ul, &none).is_err());
        assert!(!valid_name("pow") && !valid_name("E"));
    }
}
