//! Kerb / ride numbers of `tests/ride.rs` on the shipped handling (unify re-range helper).
use wp_contracts::LoadedCar;
use wp_vehicle::ride::{axle_ride, kerb, ride_assists};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

fn main() {
    let reference = ["hakone_gt", "kestrel_ts", "rampart_50", "sentinel_cruiser"];
    let a = ride_assists();
    let (mut lift, mut rise10, mut over, mut settle, mut air10, mut roll) =
        (0f32, 0f32, 0u32, 0f32, 0f32, 0f32);
    for id in reference {
        let c = car(id);
        for kmh in [30.0, 60.0] {
            let k = kerb(&c, kmh, 0.10, 0.0, &a);
            lift = lift.max(k.wheel_lift_s);
            rise10 = rise10.max(k.peak_rise_m);
            over = over.max(k.overshoots);
            settle = settle.max(k.settle_s.unwrap_or(9.0));
            air10 = air10.max(k.airborne_s);
            println!("10cm {id} {kmh}: {k:?}");
        }
        let k = kerb(&c, 60.0, 0.10, 20.0, &a);
        air10 = air10.max(k.airborne_s);
        roll = roll.max(k.peak_roll_rate);
    }
    println!("10cm worst: wheel_lift {lift:.3} rise {rise10:.4} overshoots {over} settle {settle:.2} airborne {air10:.3} angled roll {roll:.2}");
    let roster = std::env::args().skip(1).collect::<Vec<_>>();
    let (mut air_ref, mut air_other, mut rise_ref, mut rise_other) = (0f32, 0f32, 0f32, 0f32);
    for id in &roster {
        let k = kerb(&car(id), 60.0, 0.15, 0.0, &a);
        if reference.contains(&id.as_str()) {
            air_ref = air_ref.max(k.airborne_s);
            rise_ref = rise_ref.max(k.peak_rise_m);
        } else {
            air_other = air_other.max(k.airborne_s);
            rise_other = rise_other.max(k.peak_rise_m);
        }
        println!(
            "15cm {id}: air {:.3} rise {:.3}",
            k.airborne_s, k.peak_rise_m
        );
        for (i, ax) in axle_ride(&car(id)).iter().enumerate() {
            println!(
                "   axle {i}: hz {:.2} zb {:.3} zr {:.3}",
                ax.ride_hz, ax.zeta_bump, ax.zeta_rebound
            );
        }
    }
    println!("15cm worst: ref air {air_ref:.3} rise {rise_ref:.3}; others air {air_other:.3} rise {rise_other:.3}");
}
