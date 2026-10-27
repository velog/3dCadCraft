//! ISO 286 limits and fits for drawing dimensions (Edit Dimension › Limits /
//! Fits): the standard tolerance grades IT3–IT12 and the fundamental
//! deviations of the common hole (C–U) and shaft (c–u) letters, for nominal
//! sizes up to 500 mm. A size on a step's boundary belongs to the lower step
//! ("over 10 up to and including 18").

/// Upper bounds of the main size steps (mm).
const MAIN: [f64; 13] = [
    3.0, 6.0, 10.0, 18.0, 30.0, 50.0, 80.0, 120.0, 180.0, 250.0, 315.0, 400.0, 500.0,
];

/// Upper bounds of the intermediate steps (mm), for the letters whose
/// deviation changes inside a main step.
const FINE: [f64; 25] = [
    3.0, 6.0, 10.0, 14.0, 18.0, 24.0, 30.0, 40.0, 50.0, 65.0, 80.0, 100.0, 120.0, 140.0, 160.0,
    180.0, 200.0, 225.0, 250.0, 280.0, 315.0, 355.0, 400.0, 450.0, 500.0,
];

/// Standard tolerances (µm), grades IT3..=IT12, per main step.
const IT: [[f64; 13]; 10] = [
    [
        2.0, 2.5, 2.5, 3.0, 4.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 13.0, 15.0,
    ],
    [
        3.0, 4.0, 4.0, 5.0, 6.0, 7.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0, 20.0,
    ],
    [
        4.0, 5.0, 6.0, 8.0, 9.0, 11.0, 13.0, 15.0, 18.0, 20.0, 23.0, 25.0, 27.0,
    ],
    [
        6.0, 8.0, 9.0, 11.0, 13.0, 16.0, 19.0, 22.0, 25.0, 29.0, 32.0, 36.0, 40.0,
    ],
    [
        10.0, 12.0, 15.0, 18.0, 21.0, 25.0, 30.0, 35.0, 40.0, 46.0, 52.0, 57.0, 63.0,
    ],
    [
        14.0, 18.0, 22.0, 27.0, 33.0, 39.0, 46.0, 54.0, 63.0, 72.0, 81.0, 89.0, 97.0,
    ],
    [
        25.0, 30.0, 36.0, 43.0, 52.0, 62.0, 74.0, 87.0, 100.0, 115.0, 130.0, 140.0, 155.0,
    ],
    [
        40.0, 48.0, 58.0, 70.0, 84.0, 100.0, 120.0, 140.0, 160.0, 185.0, 210.0, 230.0, 250.0,
    ],
    [
        60.0, 75.0, 90.0, 110.0, 130.0, 160.0, 190.0, 220.0, 250.0, 290.0, 320.0, 360.0, 400.0,
    ],
    [
        100.0, 120.0, 150.0, 180.0, 210.0, 250.0, 300.0, 350.0, 400.0, 460.0, 520.0, 570.0, 630.0,
    ],
];

/// Upper deviations `es` of shafts d–g (µm, ≤ 0), per main step.
const D: [f64; 13] = [
    -20.0, -30.0, -40.0, -50.0, -65.0, -80.0, -100.0, -120.0, -145.0, -170.0, -190.0, -210.0,
    -230.0,
];
const E: [f64; 13] = [
    -14.0, -20.0, -25.0, -32.0, -40.0, -50.0, -60.0, -72.0, -85.0, -100.0, -110.0, -125.0, -135.0,
];
const F: [f64; 13] = [
    -6.0, -10.0, -13.0, -16.0, -20.0, -25.0, -30.0, -36.0, -43.0, -50.0, -56.0, -62.0, -68.0,
];
const G: [f64; 13] = [
    -2.0, -4.0, -5.0, -6.0, -7.0, -9.0, -10.0, -12.0, -14.0, -15.0, -17.0, -18.0, -20.0,
];

/// Lower deviations `ei` of shafts k (grades 4–7), m, n, p (µm), per main
/// step.
const K: [f64; 13] = [
    0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0, 5.0,
];
const M: [f64; 13] = [
    2.0, 4.0, 6.0, 7.0, 8.0, 9.0, 11.0, 13.0, 15.0, 17.0, 20.0, 21.0, 23.0,
];
const N: [f64; 13] = [
    4.0, 8.0, 10.0, 12.0, 15.0, 17.0, 20.0, 23.0, 27.0, 31.0, 34.0, 37.0, 40.0,
];
const P: [f64; 13] = [
    6.0, 12.0, 15.0, 18.0, 22.0, 26.0, 32.0, 37.0, 43.0, 50.0, 56.0, 62.0, 68.0,
];

/// `es` of shaft c and `ei` of shafts r, s, t, u (µm), per intermediate
/// step (`NAN`: not defined there).
const C: [f64; 25] = [
    -60.0, -70.0, -80.0, -95.0, -95.0, -110.0, -110.0, -120.0, -130.0, -140.0, -150.0, -170.0,
    -180.0, -200.0, -210.0, -230.0, -240.0, -260.0, -280.0, -300.0, -330.0, -360.0, -400.0, -440.0,
    -480.0,
];
const R: [f64; 25] = [
    10.0, 15.0, 19.0, 23.0, 23.0, 28.0, 28.0, 34.0, 34.0, 41.0, 43.0, 51.0, 54.0, 63.0, 65.0, 68.0,
    77.0, 80.0, 84.0, 94.0, 98.0, 108.0, 114.0, 126.0, 132.0,
];
const S: [f64; 25] = [
    14.0, 19.0, 23.0, 28.0, 28.0, 35.0, 35.0, 43.0, 43.0, 53.0, 59.0, 71.0, 79.0, 92.0, 100.0,
    108.0, 122.0, 130.0, 140.0, 158.0, 170.0, 190.0, 208.0, 232.0, 252.0,
];
const T: [f64; 25] = [
    f64::NAN,
    f64::NAN,
    f64::NAN,
    f64::NAN,
    f64::NAN,
    f64::NAN,
    41.0,
    48.0,
    54.0,
    66.0,
    75.0,
    91.0,
    104.0,
    122.0,
    134.0,
    146.0,
    166.0,
    180.0,
    196.0,
    218.0,
    240.0,
    268.0,
    294.0,
    330.0,
    360.0,
];
const U: [f64; 25] = [
    18.0, 23.0, 28.0, 33.0, 33.0, 41.0, 48.0, 60.0, 70.0, 87.0, 102.0, 124.0, 144.0, 170.0, 190.0,
    210.0, 236.0, 258.0, 284.0, 315.0, 350.0, 390.0, 435.0, 490.0, 540.0,
];

/// The step of `size` in `bounds`.
fn step(bounds: &[f64], size: f64) -> Option<usize> {
    if size.is_nan() || size <= 0.0 {
        return None;
    }
    bounds.iter().position(|&b| size <= b + 1e-9)
}

/// Standard tolerance IT`grade` of `size` (µm).
pub fn it(grade: u8, size: f64) -> Option<f64> {
    let row = IT.get(usize::from(grade).checked_sub(3)?)?;
    Some(row[step(&MAIN, size)?])
}

/// A fit code split: letters, grade, hole (upper case) or shaft.
pub fn parse(code: &str) -> Option<(String, u8, bool)> {
    let code = code.trim();
    let split = code.find(|c: char| c.is_ascii_digit())?;
    let (letters, grade) = code.split_at(split);
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let hole = letters.chars().all(|c| c.is_ascii_uppercase());
    let shaft = letters.chars().all(|c| c.is_ascii_lowercase());
    if !(hole || shaft) {
        return None;
    }
    Some((letters.to_ascii_lowercase(), grade.parse().ok()?, hole))
}

/// Shaft `letter` IT`grade`: (es, ei) in µm.
fn shaft(letter: &str, grade: u8, size: f64) -> Option<(f64, f64)> {
    let t = it(grade, size)?;
    let m = step(&MAIN, size)?;
    let f = step(&FINE, size)?;
    let upper = |es: f64| Some((es, es - t));
    let lower = |ei: f64| (!ei.is_nan()).then_some((ei + t, ei));
    match letter {
        "c" => upper(C[f]),
        "d" => upper(D[m]),
        "e" => upper(E[m]),
        "f" => upper(F[m]),
        "g" => upper(G[m]),
        "h" => upper(0.0),
        "js" => Some((t / 2.0, -t / 2.0)),
        "k" => lower(if (4..=7).contains(&grade) { K[m] } else { 0.0 }),
        "m" => lower(M[m]),
        "n" => lower(N[m]),
        "p" => lower(P[m]),
        "r" => lower(R[f]),
        "s" => lower(S[f]),
        "t" => lower(T[f]),
        "u" => lower(U[f]),
        _ => None,
    }
}

/// Hole `letter` IT`grade`: (ES, EI) in µm.
fn hole(letter: &str, grade: u8, size: f64) -> Option<(f64, f64)> {
    let t = it(grade, size)?;
    let m = step(&MAIN, size)?;
    let small = size <= 3.0 + 1e-9;
    // Δ = IT n − IT (n − 1), none up to 3 mm.
    let delta = || -> Option<f64> {
        if small {
            Some(0.0)
        } else {
            Some(t - it(grade.checked_sub(1)?, size)?)
        }
    };
    let from_es = |es: f64| Some((es, es - t));
    match letter {
        "c" | "d" | "e" | "f" | "g" | "h" => {
            let (es, _) = shaft(letter, grade, size)?;
            Some((-es + t, -es))
        }
        "js" => Some((t / 2.0, -t / 2.0)),
        "k" => from_es(if grade <= 8 && !small {
            -K[m] + delta()?
        } else {
            0.0
        }),
        // ISO's exception: M6 over 250 up to 315 mm.
        "m" if grade == 6 && m == 10 => from_es(-9.0),
        "m" => from_es(if grade <= 8 && !small {
            -M[m] + delta()?
        } else {
            -M[m]
        }),
        "n" => from_es(if grade <= 8 {
            -N[m] + delta()?
        } else if small {
            -N[m]
        } else {
            0.0
        }),
        "p" | "r" | "s" | "t" | "u" => {
            let (_, ei) = shaft(letter, grade, size)?;
            from_es(if grade <= 7 { -ei + delta()? } else { -ei })
        }
        _ => None,
    }
}

/// The deviations of fit `code` ("H7", "g6") at nominal `size` (mm):
/// (upper, lower) in mm. `None` for unknown codes and sizes past 500 mm.
pub fn deviations(code: &str, size: f64) -> Option<(f64, f64)> {
    let (letter, grade, is_hole) = parse(code)?;
    let size = size.abs();
    let (u, l) = if is_hole {
        hole(&letter, grade, size)?
    } else {
        shaft(&letter, grade, size)?
    };
    Some((u / 1000.0, l / 1000.0))
}

/// Hole codes offered in Edit Dimension.
pub const HOLES: [&str; 20] = [
    "H6", "H7", "H8", "H9", "H10", "H11", "G6", "G7", "F7", "F8", "E8", "E9", "D9", "D10", "C11",
    "JS7", "K7", "M7", "N7", "P7",
];

/// Shaft codes offered in Edit Dimension.
pub const SHAFTS: [&str; 22] = [
    "h5", "h6", "h7", "h8", "h9", "h11", "g5", "g6", "f6", "f7", "e8", "e9", "d9", "d10", "c11",
    "js6", "k6", "m6", "n6", "p6", "r6", "s6",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn um(code: &str, size: f64) -> (i64, i64) {
        let (u, l) = deviations(code, size).unwrap();
        ((u * 1000.0).round() as i64, (l * 1000.0).round() as i64)
    }

    #[test]
    fn common_fits() {
        // Inventor's values for the usual hole-basis fits.
        assert_eq!(um("H7", 20.0), (21, 0));
        assert_eq!(um("g6", 20.0), (-7, -20));
        assert_eq!(um("H7", 2.0), (10, 0));
        assert_eq!(um("H11", 2.0), (60, 0));
        assert_eq!(um("f7", 50.0), (-25, -50));
        assert_eq!(um("k6", 25.0), (15, 2));
        assert_eq!(um("p6", 40.0), (42, 26));
        assert_eq!(um("K7", 25.0), (6, -15));
        assert_eq!(um("M7", 60.0), (0, -30));
        assert_eq!(um("N7", 100.0), (-10, -45));
        assert_eq!(um("P7", 30.0), (-14, -35));
        let (u, l) = deviations("JS7", 20.0).unwrap();
        assert!((u - 0.0105).abs() < 1e-12 && (l + 0.0105).abs() < 1e-12);
        assert_eq!(um("M6", 300.0), (-9, -41));
        assert!(deviations("t6", 20.0).is_none());
        assert_eq!(um("s6", 150.0), (125, 100));
        // A boundary size belongs to the lower step.
        assert_eq!(um("H7", 18.0), (18, 0));
        assert!(deviations("H7", 600.0).is_none());
        assert!(deviations("Hg7", 20.0).is_none());
        assert!(deviations("x7", 20.0).is_none());
    }

    #[test]
    fn every_offered_code_has_values() {
        for c in HOLES.iter().chain(SHAFTS.iter()) {
            for s in [2.0, 20.0, 150.0, 480.0] {
                let (u, l) = deviations(c, s).unwrap();
                assert!(u > l, "{c} {s}");
            }
        }
    }
}
