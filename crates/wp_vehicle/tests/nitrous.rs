//! Nitrous on the full vehicle (owner request 2026-10-26): holding the nitrous tool with the
//! throttle down makes the car accelerate harder, a rubberbanded (AI) bottle harder still,
//! the bottle empties and refills, and a snapshot carries it.

use wp_contracts::{LoadedCar, VehicleInput};
use wp_vehicle::metrics::metric_assists;
use wp_vehicle::nitrous::{tune, NitrousScale, NITROUS_TOOL};
use wp_vehicle::rig::Rig;

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

fn input(nitrous: bool) -> VehicleInput {
    VehicleInput {
        throttle: 1.0,
        tool_use: if nitrous { NITROUS_TOOL } else { 0 },
        ..Default::default()
    }
}

/// Speed after `seconds` flat out from rest, with nitrous from `from_s` on.
fn launch(c: &LoadedCar, scale: Option<NitrousScale>, from_s: f32, seconds: f32) -> (f32, Rig) {
    let a = metric_assists();
    let mut rig = Rig::new(c);
    if let Some(s) = scale {
        rig.vehicle.set_nitrous_scale(s);
    }
    for i in 0..(seconds * 240.0) as usize {
        let t = i as f32 / 240.0;
        rig.step(&input(scale.is_some() && t >= from_s), &a);
    }
    (rig.state().speed_mps, rig)
}

#[test]
fn nitrous_accelerates_harder_and_the_ai_bottle_harder_still() {
    for id in ["debug_sedan", "kestrel_ts", "ironside_440"] {
        let c = car(id);
        // Rolling: 6 s flat out, then 6 s with nitrous (off the line the extra torque mostly
        // spins the tyres of the stronger cars).
        let (plain, _) = launch(&c, None, 0.0, 12.0);
        let (player, rig) = launch(&c, Some(NitrousScale::PLAYER), 6.0, 12.0);
        let far = tune().racer.scale(tune().racer.gap_full_m);
        let (ai, _) = launch(&c, Some(far), 6.0, 12.0);
        assert!(
            player > plain + 1.0,
            "{id}: nitrous {player} vs {plain} m/s"
        );
        assert!(ai > player + 0.3, "{id}: rubberbanded {ai} vs {player} m/s");
        let n = rig.vehicle.nitrous();
        assert!(
            n.fill < 0.1,
            "{id}: 6 s of burn used the bottle ({})",
            n.fill
        );
    }
}

#[test]
fn no_nitrous_without_the_throttle() {
    let c = car("debug_sedan");
    let a = metric_assists();
    let mut rig = Rig::new(&c);
    let coast = VehicleInput {
        tool_use: NITROUS_TOOL,
        ..Default::default()
    };
    for _ in 0..240 {
        rig.step(&coast, &a);
    }
    assert_eq!(rig.vehicle.nitrous().fill, 1.0);
    assert!(!rig.vehicle.telemetry().nitrous_on);
}

#[test]
fn snapshot_carries_the_bottle() {
    let c = car("debug_sedan");
    let a = metric_assists();
    let (_, mut rig) = launch(&c, Some(NitrousScale::PLAYER), 1.0, 3.0);
    let snap = rig.snapshot();
    let run = |rig: &mut Rig| {
        for _ in 0..480 {
            rig.step(&input(true), &a);
        }
        (rig.vehicle.nitrous(), rig.state_hash())
    };
    let truth = run(&mut rig);
    rig.restore(&snap).unwrap();
    rig.set_ticks(720);
    assert_eq!(run(&mut rig), truth);
}
