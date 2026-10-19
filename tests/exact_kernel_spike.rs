//! Spike: the owner's acceptance part built with the exact kernel
//! (solidcraft-kernel on truck): a 40×30×20 box, r = 5 fillets on the four
//! vertical and four bottom edges, a 2 mm chamfer on the whole top loop.
//! Compares volume with the closed form, and reports face count and time.

use std::f64::consts::PI;
use std::time::Instant;

use solidcraft_geom::Vec3;
use solidcraft_kernel as k;

#[test]
fn rounded_box_with_chamfered_top_exact() {
    let (a, b, h, r, d) = (40.0, 30.0, 20.0, 5.0, 2.0);
    let t0 = Instant::now();
    let body = k::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(a, b, h)).unwrap();
    // Points on the edges pick them.
    let edges = [
        Vec3::new(a, 0.0, h / 2.0),
        Vec3::new(a, b, h / 2.0),
        Vec3::new(0.0, b, h / 2.0),
        Vec3::new(0.0, 0.0, h / 2.0),
        Vec3::new(a / 2.0, 0.0, 0.0),
        Vec3::new(a, b / 2.0, 0.0),
        Vec3::new(a / 2.0, b, 0.0),
        Vec3::new(0.0, b / 2.0, 0.0),
    ];
    let filleted = k::fillet(&body, &edges, r).unwrap();
    let t_fillet = t0.elapsed();
    let m = k::measure(&filleted).unwrap();
    let v_fillet = a * b * h
        - (4.0 - PI) * r * r * h
        - (1.0 - PI / 4.0) * r * r * 2.0 * (a + b - 4.0 * r)
        - 4.0 * PI * r * r * r / 12.0;
    println!(
        "fillet: {:?}, faces {}, volume {} (exact {})",
        t_fillet, m.faces, m.volume, v_fillet
    );
    assert!((m.volume - v_fillet).abs() / v_fillet < 1e-3);

    // The top loop: 4 straight parts and 4 arcs (midpoints).
    let c = |x: f64, y: f64| Vec3::new(x, y, h);
    let s = 0.5f64.sqrt();
    let top = [
        c(a / 2.0, 0.0),
        c(a, b / 2.0),
        c(a / 2.0, b),
        c(0.0, b / 2.0),
        c(a - r + r * s, r - r * s),
        c(a - r + r * s, b - r + r * s),
        c(r - r * s, b - r + r * s),
        c(r - r * s, r - r * s),
    ];
    let t1 = Instant::now();
    let done = k::chamfer(&filleted, &top, d).unwrap();
    let t_chamfer = t1.elapsed();
    let m2 = k::measure(&done).unwrap();
    let removed =
        d * d / 2.0 * 2.0 * (a + b - 4.0 * r) + 4.0 * (d * d / 2.0) * (PI / 2.0) * (r - d / 3.0);
    let exact = v_fillet - removed;
    println!(
        "chamfer: {:?}, faces {}, volume {} (exact {})",
        t_chamfer, m2.faces, m2.volume, exact
    );
    assert!((m2.volume - exact).abs() / exact < 1e-3);
}
