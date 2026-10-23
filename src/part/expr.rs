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

use super::units::{self, Dim, UnitDef};
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
    /// True / False (format revision 18).
    Bool,
    /// A text (format revision 18).
    Text,
}

impl Unit {
    /// Every unit, in the order the Parameters table offers them.
    pub const ALL: [Unit; 10] = [
        Unit::Mm,
        Unit::Cm,
        Unit::M,
        Unit::In,
        Unit::Ft,
        Unit::Deg,
        Unit::Rad,
        Unit::Ul,
        Unit::Bool,
        Unit::Text,
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
            Unit::Bool => "bool",
            Unit::Text => "text",
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
            Unit::Mm | Unit::Ul | Unit::Rad | Unit::Bool | Unit::Text => 1.0,
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

impl Unit {
    /// As a general unit (factor and dimension).
    pub fn def(self) -> UnitDef {
        let dim = if self.is_length() {
            units::parse_spec("mm")
                .map(|u| u.dim)
                .unwrap_or(units::NONE)
        } else if self.is_angle() {
            units::parse_spec("rad")
                .map(|u| u.dim)
                .unwrap_or(units::NONE)
        } else {
            units::NONE
        };
        UnitDef {
            factor: self.factor(),
            offset: 0.0,
            dim,
            logic: match self {
                Unit::Bool => units::Logic::Bool,
                Unit::Text => units::Logic::Text,
                _ => units::Logic::Num,
            },
        }
    }
}

impl From<Unit> for UnitDef {
    fn from(u: Unit) -> Self {
        u.def()
    }
}

/// An expression error, for the status bar / a red table cell.
pub type ExprError = String;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(Real),
    Name(String),
    Str(String),
    Op(char),
    /// `<`, `>`, `<=`, `>=`, `==`, `!=`.
    Cmp(&'static str),
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
        } else if c == '"' {
            // A text: "Steel" ("" inside is a quote).
            let mut t = String::new();
            i += 1;
            loop {
                match cs.get(i) {
                    None => return Err("a text is missing its closing \"".into()),
                    Some('"') if cs.get(i + 1) == Some(&'"') => {
                        t.push('"');
                        i += 2;
                    }
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some(&ch) => {
                        t.push(ch);
                        i += 1;
                    }
                }
            }
            out.push(Tok::Str(t));
        } else if "<>=!".contains(c) {
            let two = cs.get(i + 1) == Some(&'=');
            let op = match (c, two) {
                ('<', true) => "<=",
                ('>', true) => ">=",
                ('=', true) => "==",
                ('!', true) => "!=",
                ('<', false) => "<",
                ('>', false) => ">",
                ('=', false) => "==",
                _ => return Err("'!' must be followed by '=' (or write not)".into()),
            };
            out.push(Tok::Cmp(op));
            i += if two { 2 } else { 1 };
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

/// The units of the first build (`mm cm m in ft um deg rad ul`): a
/// parameter cannot be named after them.
fn reserved_unit(name: &str) -> bool {
    matches!(
        name,
        "mm" | "cm" | "m" | "in" | "ft" | "um" | "deg" | "rad" | "ul"
    )
}

/// Words of the language a parameter cannot be named after.
fn keyword(name: &str) -> bool {
    matches!(
        name,
        "PI" | "pi" | "E" | "if" | "and" | "or" | "not" | "true" | "false" | "True" | "False"
    )
}

/// A value of an equation: a quantity (internal units, dimension), true /
/// false, or a text.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A quantity: internal value and dimension.
    Num(Real, Dim),
    /// True / false.
    Bool(bool),
    /// A text.
    Text(String),
}

impl From<(Real, Dim)> for Value {
    fn from((v, d): (Real, Dim)) -> Self {
        Value::Num(v, d)
    }
}

impl Value {
    /// The number (1 / 0 for true / false, 0 for a text).
    pub fn number(&self) -> Real {
        match self {
            Value::Num(v, _) => *v,
            Value::Bool(b) => Real::from(u8::from(*b)),
            Value::Text(_) => 0.0,
        }
    }
}

/// What names stand for while parsing: a parameter's value.
pub type Vars<'a> = dyn Fn(&str) -> Option<Value> + 'a;

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    vars: &'a Vars<'a>,
    /// Unit of the parameter being evaluated (bare numbers take it).
    unit: UnitDef,
    /// Names the expression uses (for dependency order).
    used: Vec<String>,
    /// Inside the branch of `if` that is not taken: its arithmetic errors
    /// (`if(x == 0, 0, 1/x)`) do not count.
    skip: u32,
}

/// What a value is.
#[derive(Clone, Debug, PartialEq)]
enum K {
    Num,
    Bool,
    Text(String),
}

/// A value (internal units) and its dimension; a bare number has none.
#[derive(Clone, Debug)]
struct Q {
    v: Real,
    dim: Dim,
    k: K,
}

impl Q {
    fn bare(v: Real) -> Self {
        Self {
            v,
            dim: units::NONE,
            k: K::Num,
        }
    }
    fn num(v: Real, dim: Dim) -> Self {
        Self { v, dim, k: K::Num }
    }
    fn boolean(b: bool) -> Self {
        Self {
            v: Real::from(u8::from(b)),
            dim: units::NONE,
            k: K::Bool,
        }
    }
    fn text(t: String) -> Self {
        Self {
            v: 0.0,
            dim: units::NONE,
            k: K::Text(t),
        }
    }
    fn is_bare(&self) -> bool {
        self.k == K::Num && self.dim == units::NONE
    }
    fn what(&self) -> String {
        match &self.k {
            K::Num => units::category(self.dim),
            K::Bool => "true / false".into(),
            K::Text(_) => "a text".into(),
        }
    }
    fn number(&self, op: &str) -> Result<(), ExprError> {
        match self.k {
            K::Num => Ok(()),
            _ => Err(format!("{op} needs numbers, not {}", self.what())),
        }
    }
    fn truth(&self, op: &str) -> Result<bool, ExprError> {
        match self.k {
            K::Bool => Ok(self.v != 0.0),
            _ => Err(format!("{op} needs true / false, not {}", self.what())),
        }
    }
}

fn angle_dim() -> Dim {
    Unit::Rad.def().dim
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
    fn eat_word(&mut self, w: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Name(n)) if n == w) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    /// An error, unless inside the branch of `if` not taken.
    fn fail(&self, e: ExprError, dummy: Q) -> Result<Q, ExprError> {
        if self.skip > 0 { Ok(dummy) } else { Err(e) }
    }
    /// A bare number meeting a quantity takes the parameter's unit when that
    /// is the same kind (`d0 + 3` = d0 + 3 mm, `a + 15` = a + 15°), degrees
    /// for an angle, else the base unit.
    fn as_like(&self, bare: Real, other: Dim) -> Real {
        if units::compatible(self.unit.dim, other) && self.unit.dim != units::NONE {
            bare * self.unit.factor
        } else if other == angle_dim() {
            bare.to_radians()
        } else {
            bare
        }
    }
    /// Two numbers made comparable: (a, b, dimension).
    fn align(&self, x: &Q, y: &Q, op: &str) -> Result<(Real, Real, Dim), ExprError> {
        x.number(op)?;
        y.number(op)?;
        match (x.is_bare(), y.is_bare()) {
            (false, true) => Ok((x.v, self.as_like(y.v, x.dim), x.dim)),
            (true, false) => Ok((self.as_like(x.v, y.dim), y.v, y.dim)),
            _ if units::compatible(x.dim, y.dim) => Ok((x.v, y.v, x.dim)),
            _ => Err(format!(
                "cannot {op} {} and {}",
                units::category(x.dim),
                units::category(y.dim)
            )),
        }
    }
    // or := and ('or' and)*
    fn expr(&mut self) -> Result<Q, ExprError> {
        let mut q = self.and()?;
        while self.eat_word("or") {
            let r = self.and()?;
            let (a, b) = match (q.truth("or"), r.truth("or")) {
                (Ok(a), Ok(b)) => (a, b),
                (Err(e), _) | (_, Err(e)) => return self.fail(e, Q::boolean(false)),
            };
            q = Q::boolean(a || b);
        }
        Ok(q)
    }
    fn and(&mut self) -> Result<Q, ExprError> {
        let mut q = self.not()?;
        while self.eat_word("and") {
            let r = self.not()?;
            let (a, b) = match (q.truth("and"), r.truth("and")) {
                (Ok(a), Ok(b)) => (a, b),
                (Err(e), _) | (_, Err(e)) => return self.fail(e, Q::boolean(false)),
            };
            q = Q::boolean(a && b);
        }
        Ok(q)
    }
    fn not(&mut self) -> Result<Q, ExprError> {
        if self.eat_word("not") {
            let q = self.not()?;
            return match q.truth("not") {
                Ok(b) => Ok(Q::boolean(!b)),
                Err(e) => self.fail(e, Q::boolean(false)),
            };
        }
        self.cmp()
    }
    // cmp := sum (op sum)?
    fn cmp(&mut self) -> Result<Q, ExprError> {
        let a = self.sum()?;
        let Some(Tok::Cmp(op)) = self.peek().cloned() else {
            return Ok(a);
        };
        self.pos += 1;
        let b = self.sum()?;
        // Texts and true / false compare for equality.
        let eq_only = |o: &str| matches!(o, "==" | "!=");
        let r = match (&a.k, &b.k) {
            (K::Text(x), K::Text(y)) if eq_only(op) => Ok((x == y) == (op == "==")),
            (K::Bool, K::Bool) if eq_only(op) => Ok((a.v == b.v) == (op == "==")),
            (K::Num, K::Num) => self.align(&a, &b, "compare").map(|(x, y, _)| {
                let tol = 1e-9 * x.abs().max(y.abs()).max(1.0);
                match op {
                    "<" => x < y - tol,
                    ">" => x > y + tol,
                    "<=" => x <= y + tol,
                    ">=" => x >= y - tol,
                    "==" => (x - y).abs() <= tol,
                    _ => (x - y).abs() > tol,
                }
            }),
            _ => Err(format!(
                "cannot compare {} and {} with {op}",
                a.what(),
                b.what()
            )),
        };
        match r {
            Ok(v) => Ok(Q::boolean(v)),
            Err(e) => self.fail(e, Q::boolean(false)),
        }
    }
    fn sum(&mut self) -> Result<Q, ExprError> {
        let mut q = self.term()?;
        loop {
            let plus = self.eat('+');
            if !plus && !self.eat('-') {
                return Ok(q);
            }
            let r = self.term()?;
            // Texts join with `+`.
            if let (K::Text(x), K::Text(y), true) = (&q.k, &r.k, plus) {
                q = Q::text(format!("{x}{y}"));
                continue;
            }
            q = match self.align(&q, &r, "add") {
                Ok((a, b, dim)) => Q::num(if plus { a + b } else { a - b }, dim),
                Err(e) => self.fail(e, Q::bare(0.0))?,
            };
        }
    }
    fn term(&mut self) -> Result<Q, ExprError> {
        let mut q = self.unary()?;
        loop {
            let times = self.eat('*');
            if !times && !self.eat('/') {
                return Ok(q);
            }
            let r = self.unary()?;
            if let Err(e) = q.number("* and /").and(r.number("* and /")) {
                q = self.fail(e, Q::bare(0.0))?;
                continue;
            }
            q = if times {
                Q::num(q.v * r.v, units::mul(q.dim, r.dim))
            } else if r.v == 0.0 {
                self.fail("division by zero".into(), Q::bare(0.0))?
            } else {
                Q::num(q.v / r.v, units::div(q.dim, r.dim))
            };
        }
    }
    fn unary(&mut self) -> Result<Q, ExprError> {
        if self.eat('-') {
            let q = self.unary()?;
            if let Err(e) = q.number("-") {
                return self.fail(e, Q::bare(0.0));
            }
            return Ok(Q { v: -q.v, ..q });
        }
        if self.eat('+') {
            return self.unary();
        }
        let base = self.atom()?;
        if self.eat('^') {
            let e = self.unary()?;
            return match power(&base, &e) {
                Ok(q) => Ok(q),
                Err(err) => self.fail(err, Q::bare(0.0)),
            };
        }
        Ok(base)
    }
    /// A unit after a value (`10 mm`, `5 N`, `2 mm^2`), if there is one.
    fn unit_suffix(&mut self, q: Q) -> Result<Q, ExprError> {
        let Some(Tok::Name(n)) = self.peek().cloned() else {
            return Ok(q);
        };
        let Some(u) = units::named(&n) else {
            return Ok(q);
        };
        self.pos += 1;
        // `mm^2`: the power belongs to the unit.
        let mut u = u;
        if self.peek() == Some(&Tok::Op('^'))
            && let Some(Tok::Num(e)) = self.toks.get(self.pos + 1).cloned()
            && e.fract() == 0.0
            && e.abs() < 10.0
        {
            self.pos += 2;
            let e = e as i8;
            u = UnitDef {
                factor: u.factor.powi(e as i32),
                offset: 0.0,
                dim: units::pow(u.dim, e),
                logic: units::Logic::Num,
            };
        }
        if !q.is_bare() {
            return Err(format!("a unit ('{n}') follows a value that has one"));
        }
        Ok(Q::num(u.from_display(q.v), u.dim))
    }
    /// `if(condition, then, else)`: only the branch taken counts.
    fn if_call(&mut self) -> Result<Q, ExprError> {
        let c = self.expr()?;
        let take = match c.truth("if") {
            Ok(b) => b,
            Err(e) => self.fail(e, Q::boolean(true))?.v != 0.0,
        };
        if !self.eat(',') {
            return Err("if() takes a condition, a value and another value".into());
        }
        let branch = |p: &mut Self, taken: bool| -> Result<Q, ExprError> {
            if !taken {
                p.skip += 1;
            }
            let r = p.expr();
            if !taken {
                p.skip -= 1;
            }
            r
        };
        let a = branch(self, take)?;
        if !self.eat(',') {
            return Err("if() takes a condition, a value and another value".into());
        }
        let b = branch(self, !take)?;
        if !self.eat(')') {
            return Err("missing ')'".into());
        }
        // Both branches of one kind (a bare number fits a quantity).
        let same = match (&a.k, &b.k) {
            (K::Num, K::Num) => a.is_bare() || b.is_bare() || units::compatible(a.dim, b.dim),
            (K::Bool, K::Bool) | (K::Text(_), K::Text(_)) => true,
            _ => false,
        };
        if !same {
            return self.fail(
                format!(
                    "the two values of if() differ: {} and {}",
                    a.what(),
                    b.what()
                ),
                Q::bare(0.0),
            );
        }
        let r = if take { a } else { b };
        Ok(r)
    }
    fn atom(&mut self) -> Result<Q, ExprError> {
        match self.peek().cloned() {
            Some(Tok::Num(v)) => {
                self.pos += 1;
                self.unit_suffix(Q::bare(v))
            }
            Some(Tok::Str(t)) => {
                self.pos += 1;
                Ok(Q::text(t))
            }
            Some(Tok::Op('(')) => {
                self.pos += 1;
                let q = self.expr()?;
                if !self.eat(')') {
                    return Err("missing ')'".into());
                }
                if q.is_bare() {
                    self.unit_suffix(q)
                } else {
                    Ok(q)
                }
            }
            Some(Tok::Name(n)) => {
                self.pos += 1;
                if self.eat('(') {
                    if n == "if" {
                        return self.if_call();
                    }
                    let mut args = vec![self.expr()?];
                    while self.eat(',') {
                        args.push(self.expr()?);
                    }
                    if !self.eat(')') {
                        return Err("missing ')'".into());
                    }
                    return match call(&n, &args) {
                        Ok(q) => Ok(q),
                        Err(e) => self.fail(e, Q::bare(0.0)),
                    };
                }
                match n.as_str() {
                    "PI" | "pi" => return Ok(Q::bare(std::f64::consts::PI)),
                    "E" => return Ok(Q::bare(std::f64::consts::E)),
                    "true" | "True" => return Ok(Q::boolean(true)),
                    "false" | "False" => return Ok(Q::boolean(false)),
                    _ => {}
                }
                // A parameter first; else a unit on its own is one of it
                // (`9.81 m/s^2`: `s` is one second).
                if let Some(v) = (self.vars)(&n) {
                    self.used.push(n.clone());
                    return Ok(match v {
                        Value::Num(v, dim) => Q::num(v, dim),
                        Value::Bool(b) => Q::boolean(b),
                        Value::Text(t) => Q::text(t),
                    });
                }
                if let Some(u) = units::named(&n) {
                    if u.offset != 0.0 {
                        return Err(format!("'{n}' needs a value before it (20 {n})"));
                    }
                    return Ok(Q::num(u.factor, u.dim));
                }
                self.used.push(n.clone());
                Err(format!("unknown parameter '{n}'"))
            }
            Some(Tok::Op(c)) => Err(format!("unexpected '{c}'")),
            Some(Tok::Cmp(c)) => Err(format!("unexpected '{c}'")),
            None => Err("incomplete expression".into()),
        }
    }
}

/// `base ^ e`: the exponent is a ratio; a quantity only to whole powers.
fn power(base: &Q, e: &Q) -> Result<Q, ExprError> {
    base.number("^")?;
    e.number("^")?;
    if !e.is_bare() {
        return Err("an exponent cannot have a unit".into());
    }
    if base.is_bare() {
        return Ok(Q::bare(base.v.powf(e.v)));
    }
    let r = e.v.round();
    if (e.v - r).abs() > 1e-9 || r.abs() > 9.0 {
        return Err(format!(
            "{} can only be raised to a whole power",
            units::category(base.dim)
        ));
    }
    Ok(Q::num(base.v.powf(e.v), units::pow(base.dim, r as i8)))
}

fn call(f: &str, a: &[Q]) -> Result<Q, ExprError> {
    for q in a {
        q.number(&format!("{f}()"))?;
    }
    let one = || {
        if a.len() == 1 {
            Ok(a[0].clone())
        } else {
            Err(format!("{f}() takes one argument"))
        }
    };
    // Trigonometry takes radians; a bare number is read as degrees
    // (sin(30) = sin(30 deg)), as Inventor users write it.
    let angle = || {
        one().and_then(|q| {
            if q.is_bare() {
                Ok(q.v.to_radians())
            } else if units::compatible(q.dim, units::NONE) {
                Ok(q.v)
            } else {
                Err(format!(
                    "{f}() takes an angle, not {}",
                    units::category(q.dim)
                ))
            }
        })
    };
    // Functions of a ratio (exp, ln, asin…).
    let ratio_in = || {
        one().and_then(|q| {
            if units::compatible(q.dim, units::NONE) {
                Ok(q.v)
            } else {
                Err(format!(
                    "{f}() takes a ratio, not {}",
                    units::category(q.dim)
                ))
            }
        })
    };
    let ratio = Q::bare;
    let angle_out = |v: Real| Q::num(v, angle_dim());
    let same = |name: &str| -> Result<Dim, ExprError> {
        let d0 = a.first().map_or(units::NONE, |q| q.dim);
        if a.iter()
            .all(|q| q.is_bare() || units::compatible(q.dim, d0))
        {
            Ok(a.iter()
                .find(|q| !q.is_bare())
                .map_or(units::NONE, |q| q.dim))
        } else {
            Err(format!("{name}() takes values of one kind"))
        }
    };
    let keep = |x: Q, v: Real| Q::num(v, x.dim);
    Ok(match f {
        "sin" => ratio(angle()?.sin()),
        "cos" => ratio(angle()?.cos()),
        "tan" => ratio(angle()?.tan()),
        "asin" => angle_out(ratio_in()?.asin()),
        "acos" => angle_out(ratio_in()?.acos()),
        "atan" => angle_out(ratio_in()?.atan()),
        "sqrt" => {
            let x = one()?;
            if x.v < 0.0 {
                return Err("sqrt of a negative value".into());
            }
            if x.dim.iter().any(|p| p % 2 != 0) {
                return Err(format!("sqrt of {} has no unit", units::category(x.dim)));
            }
            Q::num(x.v.sqrt(), std::array::from_fn(|k| x.dim[k] / 2))
        }
        "abs" => {
            let x = one()?;
            let v = x.v.abs();
            keep(x, v)
        }
        "floor" => {
            let x = one()?;
            let v = x.v.floor();
            keep(x, v)
        }
        "ceil" => {
            let x = one()?;
            let v = x.v.ceil();
            keep(x, v)
        }
        "round" => {
            let x = one()?;
            let v = x.v.round();
            keep(x, v)
        }
        "sinh" => ratio(ratio_in()?.sinh()),
        "cosh" => ratio(ratio_in()?.cosh()),
        "tanh" => ratio(ratio_in()?.tanh()),
        "asinh" => ratio(ratio_in()?.asinh()),
        "acosh" => ratio(ratio_in()?.acosh()),
        "atanh" => ratio(ratio_in()?.atanh()),
        "exp" => ratio(ratio_in()?.exp()),
        "ln" | "log" => {
            let x = ratio_in()?;
            if x <= 0.0 {
                return Err(format!("{f} of a value that is not positive"));
            }
            ratio(if f == "ln" { x.ln() } else { x.log10() })
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
            power(x, y)?
        }
        "min" if !a.is_empty() => Q::num(
            a.iter().map(|q| q.v).fold(Real::INFINITY, Real::min),
            same("min")?,
        ),
        "max" if !a.is_empty() => Q::num(
            a.iter().map(|q| q.v).fold(Real::NEG_INFINITY, Real::max),
            same("max")?,
        ),
        _ => return Err(format!("unknown function '{f}'")),
    })
}

/// Evaluates `text` for a parameter of `unit` to its value, looking names
/// up in `vars`; also the names it used. The result must be of the
/// parameter's kind: `Force / Length` into N/m converts from N/mm, into a
/// length it is an error; a bare result takes the parameter's unit; a
/// unitless parameter takes any number; true / false and text parameters
/// take only those.
pub fn eval_value(
    text: &str,
    unit: impl Into<UnitDef>,
    vars: &Vars<'_>,
) -> Result<(Value, Vec<String>), ExprError> {
    let unit = unit.into();
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
        skip: 0,
    };
    let q = p.expr()?;
    if p.pos != p.toks.len() {
        return Err("unexpected text at the end".into());
    }
    let value = match (unit.logic, q.k) {
        (units::Logic::Bool, K::Bool) => Value::Bool(q.v != 0.0),
        (units::Logic::Text, K::Text(t)) => Value::Text(t),
        (units::Logic::Num, K::Num) => {
            let v = if q.dim == units::NONE {
                unit.from_display(q.v)
            } else if unit.dim == units::NONE || units::compatible(q.dim, unit.dim) {
                q.v
            } else {
                return Err(format!(
                    "the result is {}, the parameter is {}",
                    units::category(q.dim),
                    units::category(unit.dim)
                ));
            };
            if !v.is_finite() {
                return Err("the result is not a finite number".into());
            }
            Value::Num(v, unit.dim)
        }
        (want, got) => {
            let got = match got {
                K::Num => units::category(q.dim),
                K::Bool => "true / false".into(),
                K::Text(_) => "a text".into(),
            };
            let want = match want {
                units::Logic::Bool => "true / false",
                units::Logic::Text => "a text",
                units::Logic::Num => "a number",
            };
            return Err(format!("the result is {got}, the parameter is {want}"));
        }
    };
    Ok((value, p.used))
}

/// [`eval_value`] as a number (true / false as 1 / 0).
pub fn eval(
    text: &str,
    unit: impl Into<UnitDef>,
    vars: &Vars<'_>,
) -> Result<(Real, Vec<String>), ExprError> {
    eval_value(text, unit, vars).map(|(v, used)| (v.number(), used))
}

/// [`eval`] where every name has the dimension of `unit` (fields that
/// only meet values of their own kind, tests).
pub fn eval_simple(
    text: &str,
    unit: impl Into<UnitDef>,
    vars: &dyn Fn(&str) -> Option<Real>,
) -> Result<(Real, Vec<String>), ExprError> {
    let unit = unit.into();
    eval(text, unit, &|n| vars(n).map(|v| Value::Num(v, unit.dim)))
}

/// The parameter names an expression refers to (no evaluation errors for
/// unknown names). A unit right after a value (`10 mm`) is not one.
pub fn names_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(toks) = lex(text) {
        for (i, t) in toks.iter().enumerate() {
            let after_value = i > 0 && matches!(toks[i - 1], Tok::Num(_) | Tok::Op(')'));
            if let Tok::Name(n) = t
                && toks.get(i + 1) != Some(&Tok::Op('('))
                && !(after_value && units::named(n).is_some())
                && !reserved_unit(n)
                && !keyword(n)
            {
                out.push(n.clone());
            }
        }
    }
    out
}

/// Is `text` a plain number (possibly with a unit), not an equation?
pub fn is_plain_number(text: &str) -> bool {
    match lex(text).as_deref() {
        Ok([Tok::Num(_)]) | Ok([Tok::Op('-'), Tok::Num(_)]) => true,
        Ok([Tok::Num(_), Tok::Name(n)]) | Ok([Tok::Op('-'), Tok::Num(_), Tok::Name(n)]) => {
            units::named(n).is_some()
        }
        _ => false,
    }
}

/// Is `text` a plain value: a number (with a unit), `true` / `false` or a
/// text in quotes (not an equation)?
pub fn is_plain_value(text: &str) -> bool {
    is_plain_number(text)
        || matches!(lex(text).as_deref(), Ok([Tok::Str(_)]))
        || matches!(lex(text).as_deref(), Ok([Tok::Name(n)]) if matches!(n.as_str(), "true" | "false" | "True" | "False"))
}

/// Is `name` usable as a parameter name?
pub fn valid_name(name: &str) -> bool {
    let mut cs = name.chars();
    cs.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && cs.all(|c| c.is_alphanumeric() || c == '_')
        && !reserved_unit(name)
        && !FUNCTIONS.contains(&name)
        && !keyword(name)
}

/// The functions of an expression (also the suggestions while typing).
pub const FUNCTIONS: &[&str] = &[
    "if", "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh",
    "atanh", "sqrt", "abs", "floor", "ceil", "round", "sign", "exp", "ln", "log", "pow", "min",
    "max",
];

/// Evaluates a set of named expressions in dependency order. Returns the
/// values and per-name errors (cycles, unknown names, bad syntax, a result
/// of the wrong kind). A name whose expression fails keeps no value, and so
/// do the names using it.
pub fn eval_all(
    exprs: &[(String, String, UnitDef)],
) -> (HashMap<String, Value>, HashMap<String, ExprError>) {
    eval_all_with(exprs, &HashMap::new())
}

/// [`eval_all`] with read-only values `fixed` the expressions may use
/// (the part's physical properties).
pub fn eval_all_with(
    exprs: &[(String, String, UnitDef)],
    fixed: &HashMap<String, Value>,
) -> (HashMap<String, Value>, HashMap<String, ExprError>) {
    let by_name: HashMap<&str, usize> = exprs
        .iter()
        .enumerate()
        .map(|(i, (n, _, _))| (n.as_str(), i))
        .collect();
    let mut values: HashMap<String, Value> = fixed.clone();
    let mut errors: HashMap<String, ExprError> = HashMap::new();
    // 0 = new, 1 = in progress, 2 = done
    let mut state = vec![0u8; exprs.len()];
    type Values = HashMap<String, Value>;
    fn visit(
        i: usize,
        exprs: &[(String, String, UnitDef)],
        by_name: &HashMap<&str, usize>,
        state: &mut [u8],
        values: &mut Values,
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
            match eval_value(text, *unit, &|n| values.get(n).cloned()) {
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
    for k in fixed.keys() {
        if !by_name.contains_key(k.as_str()) {
            values.remove(k);
        }
    }
    (values, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str, u: Unit) -> Real {
        let vars = |n: &str| match n {
            "d0" => Some(Value::Num(40.0, Unit::Mm.def().dim)),
            "a" => Some(Value::Num(30f64.to_radians(), Unit::Deg.def().dim)),
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
        for bad in ["", "2 +", "(1", "foo", "1 / 0", "2 3", "1 kg"] {
            assert!(eval(bad, Unit::Mm, &|_| None).is_err(), "{bad}");
        }
        assert!(is_plain_number("12.5") && is_plain_number("3 mm") && !is_plain_number("d0/2"));
        assert!(valid_name("wall_2") && !valid_name("2x") && !valid_name("mm"));
    }

    #[test]
    fn dependency_order_and_cycles() {
        let e = |n: &str, t: &str| (n.to_owned(), t.to_owned(), Unit::Mm.def());
        let (v, err) = eval_all(&[e("c", "b * 2"), e("b", "a + 1"), e("a", "10")]);
        assert_eq!(
            (v["a"].number(), v["b"].number(), v["c"].number()),
            (10.0, 11.0, 22.0)
        );
        assert!(err.is_empty());
        let (v, err) = eval_all(&[e("x", "y"), e("y", "x"), e("z", "5")]);
        assert!(err.contains_key("x") && err.contains_key("y"));
        assert_eq!(v["z"].number(), 5.0);
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    /// A bare number takes the parameter's unit; every unit converts to
    /// mm / rad and back.
    #[test]
    fn parameter_units_convert() {
        let none = |_: &str| -> Option<Value> { None };
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
        assert_eq!(Unit::ALL.len(), 10);
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

#[cfg(test)]
mod dim_tests {
    use super::*;

    fn spec(s: &str) -> UnitDef {
        units::parse_spec(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    /// Units in equations, checked and converted by dimension: a force over
    /// a length into N/m converts from N/mm; a wrong kind is an error.
    #[test]
    fn quantities_in_equations() {
        let force = spec("N");
        let len = spec("mm");
        let vars = |n: &str| match n {
            "F" => Some(Value::Num(force.from_display(500.0), force.dim)),
            "L" => Some(Value::Num(len.from_display(250.0), len.dim)),
            _ => None,
        };
        let shown = |text: &str, unit: &str| {
            let u = spec(unit);
            let (v, _) = eval(text, u, &vars).unwrap_or_else(|e| panic!("{text}: {e}"));
            u.to_display(v)
        };
        let close = |a: Real, b: Real| (a - b).abs() <= 1e-9 * b.abs().max(1.0);
        assert!(close(shown("F / L", "N/mm"), 2.0));
        assert!(close(shown("F / L", "N/m"), 2000.0));
        assert!(close(shown("F * L", "N*m"), 125.0));
        assert!(close(shown("5 N / 2 mm", "N/mm"), 2.5));
        assert!(close(shown("100 km/h", "m/s"), 100.0 / 3.6));
        assert!(close(shown("2 kg * 9.81 m/s^2", "N"), 19.62));
        assert!(close(shown("10 mm^2 * 3 mm", "mm^3"), 30.0));
        assert!(close(shown("sqrt(16 mm^2)", "mm"), 4.0));
        assert!(close(shown("F / (10 mm^2)", "MPa"), 50.0));
        assert!(close(shown("20 degC", "degF"), 68.0));
        // A bare number takes the parameter's unit.
        assert!(close(shown("3", "N/m"), 3.0));
        // Wrong kinds.
        let bad = |text: &str, unit: &str| eval(text, spec(unit), &vars).is_err();
        assert!(bad("F / L", "mm"), "a force per length is not a length");
        assert!(bad("F + L", "N"), "cannot add a force and a length");
        assert!(bad("L ^ 1.5", "mm"), "whole powers only");
        assert!(bad("sin(L)", "ul"), "sin of a length");
        // A unitless parameter takes any value (as before).
        assert!(eval("L / 10", spec("ul"), &vars).is_ok());
        assert!(names_in("F / L * 2 mm").contains(&"F".to_owned()));
        assert!(!names_in("2 mm + 3 kg").contains(&"mm".to_owned()));
    }
}

#[cfg(test)]
mod logic_tests {
    use super::*;

    /// `if`, comparisons, and / or / not, true / false and texts, with
    /// kinds checked.
    #[test]
    fn decisions_in_equations() {
        let len = Unit::Mm.def();
        let vars = |n: &str| match n {
            "Width" => Some(Value::Num(150.0, len.dim)),
            "Heavy" => Some(Value::Bool(true)),
            "Material" => Some(Value::Text("Steel".into())),
            _ => None,
        };
        let num = |t: &str, u: Unit| eval(t, u, &vars).unwrap_or_else(|e| panic!("{t}: {e}")).0;
        let val = |t: &str, u: Unit| {
            eval_value(t, u, &vars)
                .unwrap_or_else(|e| panic!("{t}: {e}"))
                .0
        };
        assert_eq!(num("if(Width > 100 mm, 5 mm, 3 mm)", Unit::Mm), 5.0);
        assert_eq!(
            num("if(Width > 20 cm, 5, 3)", Unit::Mm),
            3.0,
            "units compared"
        );
        assert_eq!(num("if(Heavy and Width >= 150, 8, 4)", Unit::Ul), 8.0);
        assert_eq!(num("if(not Heavy or Width < 1, 8, 4)", Unit::Ul), 4.0);
        assert_eq!(num("if(Material == \"Steel\", 7.85, 2.7)", Unit::Ul), 7.85);
        assert_eq!(val("Width > 1 m", Unit::Bool), Value::Bool(false));
        assert_eq!(
            val("Material + \" S235\"", Unit::Text),
            Value::Text("Steel S235".into())
        );
        assert_eq!(
            val("\"say \"\"hi\"\"\"", Unit::Text),
            Value::Text("say \"hi\"".into())
        );
        // Only the branch taken counts: no division by zero here.
        assert_eq!(
            num("if(Width == 0, 0, 1 / Width * 1 mm^2)", Unit::Mm),
            1.0 / 150.0
        );
        // Kinds checked.
        let bad = |t: &str, u: Unit| eval_value(t, u, &vars).is_err();
        assert!(
            bad("if(Width, 1, 2)", Unit::Ul),
            "a condition is true / false"
        );
        assert!(
            bad("if(Heavy, 5 mm, 3 kg)", Unit::Mm),
            "branches of one kind"
        );
        assert!(bad("Width > 1 kg", Unit::Bool), "a length against a mass");
        assert!(bad("Heavy", Unit::Mm), "true / false into a length");
        assert!(bad("Width", Unit::Bool));
        assert!(
            bad("Material < \"Z\"", Unit::Bool),
            "texts compare for equality only"
        );
        assert!(!valid_name("if") && !valid_name("true") && !valid_name("and"));
        assert!(names_in("if(Heavy and Width > 2 mm, 1, 0)").contains(&"Heavy".to_owned()));
        assert!(is_plain_value("true") && is_plain_value("\"x\"") && !is_plain_value("not Heavy"));
    }
}
