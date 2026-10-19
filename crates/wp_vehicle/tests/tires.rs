//! Every tire compound in `data/tires/` is internally consistent: the declared peak slip
//! (used to normalise combined slip) matches the peak of its own curve.

use wp_vehicle::tire::combined_forces;

#[test]
fn declared_peaks_match_curves() {
    let dir = format!("{}/../../data/tires", env!("CARGO_MANIFEST_DIR"));
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let t = wp_contracts::load_tire_def(&p).unwrap();
        let peak_of = |f: &dyn Fn(f32) -> f32| {
            (1..2000)
                .map(|i| i as f32 * 0.0005)
                .fold((0.0f32, 0.0f32), |best, x| {
                    if f(x) > best.1 {
                        (x, f(x))
                    } else {
                        best
                    }
                })
                .0
        };
        let a = peak_of(&|x| -combined_forces(&t, 0.0, x, t.fz0_n, 1.0).1);
        let k = peak_of(&|x| combined_forces(&t, x, 0.0, t.fz0_n, 1.0).0);
        assert!(
            (a - t.peak_slip_angle_rad).abs() < 0.01,
            "{}: alpha peak {a}",
            t.id
        );
        assert!(
            (k - t.peak_slip_ratio).abs() < 0.01,
            "{}: kappa peak {k}",
            t.id
        );
        n += 1;
    }
    assert!(n >= 8, "expected all compounds, found {n}");
}
