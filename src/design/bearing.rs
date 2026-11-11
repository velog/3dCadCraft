//! Rolling bearing life (spec 6.2-design-calculators §3.5).
//!
//! Source: B. J. Hamrock, W. J. Anderson, *Rolling-Element Bearings*, NASA
//! Reference Publication 1105, 1983 (public domain), §12: the basic life
//! L = (C / Fe)^m with m = 3 for ball (elliptical contacts) and 10/3 for roller
//! bearings (rectangular contacts), the equivalent load Fe = X·Fr + Y·Fa, the
//! Weibull relation of survival and life (eq. 76) and the adjusted life
//! LA = D·E·F·G·H·L10 (§12.5). The Weibull slopes are those NASA RP-1152 uses
//! for bearings in its system-life example (§5.3): 10/9 for ball and 3/2 for
//! roller bearings. ISO 281's modified life (a_ISO) is not in these sources.
//! Forces in N, lives in millions of revolutions unless named in hours.

/// Ball or roller bearing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// Point (elliptical) contacts.
    #[default]
    Ball,
    /// Line (rectangular) contacts.
    Roller,
}

impl Kind {
    /// The load-life exponent m of L = (C / Fe)^m.
    pub fn exponent(self) -> f64 {
        match self {
            Kind::Ball => 3.0,
            Kind::Roller => 10.0 / 3.0,
        }
    }

    /// The Weibull slope e of the lives' scatter.
    pub fn weibull(self) -> f64 {
        match self {
            Kind::Ball => 10.0 / 9.0,
            Kind::Roller => 1.5,
        }
    }
}

/// Equivalent load Fe = X·Fr + Y·Fa (§12.4; X and Y from the bearing's
/// catalogue), never below the radial load alone.
pub fn equivalent_load(fr: f64, fa: f64, x: f64, y: f64) -> f64 {
    (x * fr + y * fa).max(fr)
}

/// Basic L10 life, millions of revolutions: (C / Fe)^m.
pub fn l10(c: f64, fe: f64, kind: Kind) -> f64 {
    if fe <= 0.0 {
        return f64::INFINITY;
    }
    (c / fe).powf(kind.exponent())
}

/// A life in millions of revolutions as hours at `rpm`.
pub fn hours(life: f64, rpm: f64) -> f64 {
    life * 1e6 / (rpm * 60.0)
}

/// The life at reliability `s` (0..1) from the 90 % life, by eq. 76:
/// ln ln(1/S) = e ln(L/A), so L_S = L10 (ln(1/S) / ln(1/0.9))^(1/e).
pub fn life_at(l10: f64, s: f64, kind: Kind) -> f64 {
    l10 * ((1.0 / s).ln() / (1.0f64 / 0.9).ln()).powf(1.0 / kind.weibull())
}

/// The dynamic capacity a bearing needs for life `l` (millions of revolutions)
/// under Fe: C = Fe · L^(1/m).
pub fn required_capacity(fe: f64, l: f64, kind: Kind) -> f64 {
    fe * l.powf(1.0 / kind.exponent())
}

/// The results of one bearing's check.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BearingLife {
    /// Equivalent load, N.
    pub fe: f64,
    /// Basic L10 life, millions of revolutions.
    pub l10: f64,
    /// The same in hours.
    pub l10_hours: f64,
    /// Life at the required reliability, adjusted (LA), hours.
    pub adjusted_hours: f64,
    /// The capacity the required life needs, N.
    pub required_c: f64,
}

/// The whole check: loads, speed, capacity, the product of the life
/// adjustment factors D·E·F·G·H, the required reliability and life (hours).
#[allow(clippy::too_many_arguments)]
pub fn check(
    fr: f64,
    fa: f64,
    x: f64,
    y: f64,
    c: f64,
    rpm: f64,
    kind: Kind,
    factors: f64,
    reliability: f64,
    required_hours: f64,
) -> Result<BearingLife, String> {
    if c.is_nan() || c <= 0.0 {
        return Err("the basic dynamic load rating C must be positive".into());
    }
    if rpm.is_nan() || rpm <= 0.0 {
        return Err("the speed must be positive".into());
    }
    if !(fr >= 0.0 && fa >= 0.0) || fr + fa <= 0.0 {
        return Err("give a radial or an axial load".into());
    }
    if !(reliability > 0.0 && reliability < 1.0) {
        return Err("the reliability must lie between 0 and 100 %".into());
    }
    let fe = equivalent_load(fr, fa, x, y);
    let l = l10(c, fe, kind);
    let adjusted = factors * life_at(l, reliability, kind);
    // The L10 the required life at this reliability needs.
    let need_l10 = required_hours * rpm * 60.0 / 1e6 / factors / life_at(1.0, reliability, kind);
    Ok(BearingLife {
        fe,
        l10: l,
        l10_hours: hours(l, rpm),
        adjusted_hours: hours(adjusted, rpm),
        required_c: required_capacity(fe, need_l10, kind),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs()
    }

    /// L = (C / Fe)^m: a ball bearing at half its rating lives 8 million
    /// revolutions, a roller bearing 2^(10/3); the required capacity inverts it.
    #[test]
    fn basic_life() {
        assert!(close(l10(20_000.0, 10_000.0, Kind::Ball), 8.0, 1e-12));
        assert!(close(
            l10(20_000.0, 10_000.0, Kind::Roller),
            2f64.powf(10.0 / 3.0),
            1e-12
        ));
        let c = required_capacity(3000.0, 500.0, Kind::Roller);
        assert!(close(l10(c, 3000.0, Kind::Roller), 500.0, 1e-12));
        assert!(close(hours(60.0, 1000.0), 1000.0, 1e-12));
        // Fe never drops below Fr.
        assert_eq!(equivalent_load(1000.0, 0.0, 0.56, 1.5), 1000.0);
        assert!(close(
            equivalent_load(1000.0, 800.0, 0.56, 1.5),
            1760.0,
            1e-12
        ));
    }

    /// Eq. 76 at the slopes of RP-1152 §5.3: a ball bearing at 95 % lives
    /// (ln(1/0.95) / ln(1/0.9))^(9/10) = 0.5232 of its L10, a roller bearing
    /// (…)^(2/3) = 0.6188; at 90 % the L10 itself.
    #[test]
    fn reliability() {
        assert!(close(life_at(1.0, 0.9, Kind::Ball), 1.0, 1e-12));
        assert!(close(life_at(1.0, 0.95, Kind::Ball), 0.5232, 1e-3));
        assert!(close(life_at(1.0, 0.95, Kind::Roller), 0.6188, 1e-3));
        // The same Weibull relation as the system life of RP-1152 (eq. 75)
        // for one component.
        let one = crate::design::gear::system_life(&[(1200.0, 10.0 / 9.0)], 0.95);
        assert!(close(one, life_at(1200.0, 0.95, Kind::Ball), 1e-6));
    }

    #[test]
    fn whole_check() {
        let r = check(
            4000.0,
            1000.0,
            0.56,
            1.6,
            30_000.0,
            1500.0,
            Kind::Ball,
            1.0,
            0.9,
            10_000.0,
        )
        .unwrap();
        assert!(close(r.fe, 4000.0, 1e-12));
        assert!(close(r.l10, 7.5f64.powi(3), 1e-12));
        assert!(close(r.l10_hours, 421.875 * 1e6 / 90_000.0, 1e-9));
        // 10 000 h at 1500 rpm = 900 million revolutions: C = 4000 · 900^(1/3).
        assert!(close(r.required_c, 4000.0 * 900f64.cbrt(), 1e-9));
        assert!(check(1.0, 0.0, 1.0, 0.0, 0.0, 1.0, Kind::Ball, 1.0, 0.9, 1.0).is_err());
    }
}
