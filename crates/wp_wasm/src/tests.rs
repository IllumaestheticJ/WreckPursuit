//! Native tests of the session core behind the wasm surface: replays are canonical
//! `replay.bin` that `simcli replay` verifies, scripted sessions match `simcli run`.

use wp_contracts::{AssistProfile, DamageMode, EntityId, VehicleInput};

use simcli::replay::{self, Replay};

use crate::data::mem_data;
use crate::scenario::{free_drive, from_ron, with_cruise_speed};
use crate::session::Session;

fn drive(throttle: f32, steer: f32) -> VehicleInput {
    VehicleInput {
        throttle,
        steer,
        ..Default::default()
    }
}

fn scenario_file(rel: &str) -> simcli::Scenario {
    let path = format!(
        "{}/../../apps/simcli/scenarios/{rel}",
        env!("CARGO_MANIFEST_DIR")
    );
    from_ron(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A free-drive session with a second car, inputs, a gear edge, a reset, an assists change
/// and a mode switch, stepped with irregular frame times.
fn live_session(frames: usize) -> Session {
    let mut s = Session::new(free_drive("debug_sedan", DamageMode::Arcade, false), false).unwrap();
    let p = EntityId(1);
    let second = s.spawn("debug_coupe", [6.0, 0.0, 0.0], 0.3).unwrap();
    for i in 0..frames {
        let steer = if (i / 20) % 2 == 0 { 0.4 } else { -0.3 };
        s.set_input(p, drive(1.0, steer)).unwrap();
        s.set_input(second, drive(0.6, -steer)).unwrap();
        match i {
            30 => {
                s.set_input(
                    p,
                    VehicleInput {
                        gear_up: true,
                        ..drive(1.0, steer)
                    },
                )
                .unwrap();
            }
            50 => s.set_assists(p, AssistProfile::gamepad_default()).unwrap(),
            70 => s.set_mode(DamageMode::Simulation).unwrap(),
            90 => s.reset_vehicle(second).unwrap(),
            _ => {}
        }
        s.step(if i % 3 == 0 { 1.0 / 30.0 } else { 1.0 / 60.0 });
    }
    s
}

#[test]
fn free_drive_moves_the_player() {
    let mut s = Session::new(free_drive("debug_sedan", DamageMode::Arcade, false), false).unwrap();
    s.set_input(EntityId(1), drive(1.0, 0.0)).unwrap();
    for _ in 0..60 {
        s.step(1.0 / 60.0);
    }
    assert_eq!(s.tick(), 240);
    assert!(s.sim().vehicle_states()[0].1.speed_mps > 1.0);
}

#[test]
fn recording_is_a_canonical_replay_that_simcli_verifies() {
    let mut live = live_session(120);
    let live_hash = live.sim().state_hash();
    let bytes = live.record_stop().unwrap().encode();
    let golden = Replay::decode(&bytes).unwrap();
    assert_eq!(golden.tick_hashes.last(), Some(&live_hash));
    assert_eq!(golden.ops.len(), 3, "assists, mode, reset");
    let report = replay::verify(&golden, mem_data()).unwrap();
    assert!(report.matched, "{}", report.to_text());
}

#[test]
fn browser_playback_matches_every_tick() {
    let mut live = live_session(120);
    let live_hash = live.sim().state_hash();
    let bytes = live.record_stop().unwrap().encode();
    let mut play = Session::from_replay_bytes(&bytes).unwrap();
    play.replay_play(3.0).unwrap();
    for _ in 0..400 {
        play.step(1.0 / 60.0);
    }
    let st = play.replay_status().unwrap();
    assert_eq!(st.tick, st.end_tick);
    assert_eq!(st.drift_count, 0, "drift at {:?}", st.drift_ticks);
    assert_eq!(st.verified_ticks, st.end_tick);
    assert_eq!(play.sim().state_hash(), live_hash);
    assert_eq!(play.mode(), DamageMode::Simulation, "mode op replayed");
}

#[test]
fn backward_seek_resimulates_identically() {
    let mut live = live_session(100);
    let bytes = live.record_stop().unwrap().encode();
    let mut play = Session::from_replay_bytes(&bytes).unwrap();
    play.replay_seek(300).unwrap();
    let h300 = play.sim().state_hash();
    play.replay_seek(400).unwrap();
    play.replay_seek(300).unwrap();
    assert_eq!(play.sim().state_hash(), h300);
    assert_eq!(play.replay_status().unwrap().drift_count, 0);
}

#[test]
fn simcli_golden_replays_play_back_in_the_browser_core() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/simcli/golden");
    for name in ["golden_frontal_56kmh", "golden_tbone"] {
        let bytes = std::fs::read(format!("{dir}/{name}.replay.bin")).unwrap();
        let mut play = Session::from_replay_bytes(&bytes).unwrap();
        let end = play.playback().unwrap().replay.ticks;
        play.replay_seek(end).unwrap();
        let st = play.replay_status().unwrap();
        assert_eq!(st.drift_count, 0, "{name}: drift at {:?}", st.drift_ticks);
        assert_eq!(st.verified_ticks, end);
    }
}

#[test]
fn live_control_is_refused_during_playback_and_spawn_after_start() {
    let mut live = live_session(10);
    assert!(live.spawn("debug_sedan", [0.0; 3], 0.0).is_err());
    let bytes = live.record_stop().unwrap().encode();
    let mut play = Session::from_replay_bytes(&bytes).unwrap();
    assert!(play.set_input(EntityId(1), drive(1.0, 0.0)).is_err());
    assert!(play.reset_vehicle(EntityId(1)).is_err());
}

#[test]
fn gear_up_is_a_one_tick_edge() {
    let mut s = Session::new(free_drive("debug_sedan", DamageMode::Arcade, false), false).unwrap();
    s.set_input(
        EntityId(1),
        VehicleInput {
            gear_up: true,
            ..Default::default()
        },
    )
    .unwrap();
    s.run_ticks(1);
    assert!(!s.input(EntityId(1)).gear_up);
}

#[test]
fn scripted_crash_scenarios_match_simcli_and_pass() {
    for rel in [
        "crash/barrier_frontal.ron",
        "crash/barrier_frontal_sim.ron",
        "crash/head_on.ron",
    ] {
        let sc = scenario_file(rel);
        let ticks = sc.ticks();
        let mut s = Session::new(sc, true).unwrap();
        // Scripted entities ignore live input while the scenario runs.
        assert!(!s.set_input(EntityId(1), drive(0.0, 1.0)).unwrap());
        s.run_ticks(ticks);
        let r = s.scenario_result().unwrap();
        assert!(r.done, "{rel}");
        assert_eq!(
            r.matches_simcli,
            Some(true),
            "{rel}: live run differs from simcli"
        );
        assert!(r.pass, "{rel}: {:?}", r.summary.map(|x| x.failures));
        // Live input works after the script ends.
        assert!(s.set_input(EntityId(1), drive(0.0, 1.0)).unwrap());
    }
}

#[test]
fn speed_override_reaches_the_cruise_target() {
    let sc = with_cruise_speed(scenario_file("crash/barrier_frontal.ron"), 10.0);
    let mut s = Session::new(sc, true).unwrap();
    s.run_ticks(240 * 3);
    let v = s.sim().vehicle_states()[0].1.speed_mps;
    assert!((9.0..11.0).contains(&v), "speed {v}");
}

#[test]
fn city_free_drive_has_traffic_and_replays_in_the_browser_core() {
    let mut s = Session::new(free_drive("debug_sedan", DamageMode::Arcade, true), false).unwrap();
    assert!(s.has_city());
    let spawn = s.spec(EntityId(1)).unwrap().pos;
    assert!(spawn != [0.0; 3], "player spawned on the race grid");
    s.set_input(EntityId(1), drive(0.5, 0.0)).unwrap();
    s.run_ticks(240 * 3);
    assert!(s.sim().world().agent_count() > 10, "traffic spawned");
    let bytes = s.record_stop().unwrap().encode();
    let mut play = Session::from_replay_bytes(&bytes).unwrap();
    play.replay_seek(240 * 3).unwrap();
    assert_eq!(play.replay_status().unwrap().drift_count, 0);
}

#[test]
fn csv_export_has_one_row_per_sample() {
    let mut live = live_session(60);
    let r = live.record_stop().unwrap();
    let end = r.ticks;
    let csv = crate::export_csv(&r.encode(), 1, 4).unwrap();
    let rows = csv.lines().count() - 1;
    assert_eq!(rows as u64, end.div_ceil(4) + 1);
    assert!(csv.starts_with("tick,t_s,speed_mps"));
}

#[test]
fn city_chase_speeding_player_gets_heat() {
    let sc = crate::scenario::city_chase("debug_sedan", DamageMode::Arcade);
    let mut s = Session::new(sc, false).unwrap();
    let p = EntityId(1);
    assert!(s.sim().world().agent_count() == 0 || s.has_city());
    s.set_input(p, drive(0.7, 0.0)).unwrap();
    let mut max_heat = 0.0f32;
    let mut violations = 0;
    for _ in 0..(240 * 8 / 60) {
        s.run_ticks(60);
        violations += s
            .drain_events()
            .iter()
            .filter(|e| matches!(e, wp_contracts::SimEvent::TrafficViolation { vehicle, .. } if *vehicle == p))
            .count();
        max_heat = max_heat.max(crate::police::status(s.sim().police(), p).heat);
    }
    let st = crate::police::status(s.sim().police(), p);
    eprintln!(
        "violations {violations}, max heat {max_heat}, status {st:?}, units {}",
        s.sim().police().units().len()
    );
    assert!(violations > 0, "speeding player produced no violations");
    assert!(max_heat > 0.0, "violations produced no heat");
}

#[test]
fn city_signal_heads_cycle_and_sit_on_the_road() {
    let mut s = Session::new(free_drive("debug_sedan", DamageMode::Arcade, true), false).unwrap();
    s.run_ticks(240);
    let heads = crate::signals::heads(s.sim().world());
    let stride = crate::signals::HEAD_STRIDE;
    assert_eq!(heads.len() % stride, 0);
    let n = heads.len() / stride;
    assert!(n > 8, "city has signalised approaches: {n}");
    for h in heads.chunks(stride) {
        assert!(
            h[4] <= 0.0 && h[5] >= 0.0 && h[5] - h[4] > 2.0,
            "stop line extent {h:?}"
        );
        assert!(h.iter().all(|v| v.is_finite()));
    }
    let first = crate::signals::states(s.sim().world());
    assert_eq!(first.len(), n);
    assert!(first.iter().all(|&c| c <= crate::signals::code::GREEN));
    assert!(first.contains(&crate::signals::code::GREEN));
    assert!(first.contains(&crate::signals::code::RED));
    // Over one long cycle every head changes state at least once.
    assert_eq!(
        first,
        crate::signals::states_at(s.sim().world(), s.sim().world().tick())
    );
    let mut changed = vec![false; n];
    for sec in 1..=60u64 {
        let tick = s.sim().world().tick() + sec * 240;
        for (i, c) in crate::signals::states_at(s.sim().world(), tick)
            .iter()
            .enumerate()
        {
            changed[i] |= *c != first[i];
        }
    }
    assert!(changed.iter().all(|&c| c), "every head changes within 60 s");
}

#[test]
fn flat_world_has_no_signal_heads() {
    let s = live_session(1);
    assert!(crate::signals::heads(s.sim().world()).is_empty());
    assert!(crate::signals::states(s.sim().world()).is_empty());
}

#[test]
fn vesper_free_drive_stays_on_the_ground_with_traffic() {
    let sc = crate::scenario::free_drive_in(
        "debug_sedan",
        DamageMode::Arcade,
        crate::scenario::city_spec("vesper_v2"),
    );
    let mut s = Session::new(sc, false).unwrap();
    assert!(s.has_city());
    for _ in 0..4 {
        s.run_ticks(240);
        let st = s.sim().vehicle_states()[0].1.clone();
        let ground = s
            .sim()
            .world()
            .map()
            .ground_height([st.pos[0], st.pos[1]], Some(st.pos[2]));
        assert!(
            (st.pos[2] - ground).abs() < 0.5,
            "player {} m off the ground",
            st.pos[2] - ground
        );
    }
    assert!(s.sim().world().agent_count() > 10, "traffic spawned");
}

#[test]
fn one_click_assist_presets_set_mode_and_assists_together() {
    use wp_contracts::InputSource;
    use wp_vehicle::AssistPreset;
    let mut s = Session::new(free_drive("debug_sedan", DamageMode::Arcade, false), false).unwrap();
    let p = EntityId(1);
    for preset in AssistPreset::ALL {
        // What the testbed's preset button does: damage mode + assists in one click.
        s.set_mode(preset.damage_mode()).unwrap();
        s.set_assists(p, preset.assists(InputSource::Keyboard))
            .unwrap();
        s.step(1.0 / 60.0);
        assert_eq!(s.mode(), preset.damage_mode());
        let a = s.assists(p).unwrap();
        assert_eq!(
            AssistPreset::matching(&a, s.mode(), InputSource::Keyboard),
            Some(preset)
        );
    }
    // Ticking one box afterwards makes it a custom setup.
    let mut a = s.assists(p).unwrap();
    a.abs = wp_contracts::AbsLevel::Full;
    s.set_assists(p, a).unwrap();
    assert_eq!(
        AssistPreset::matching(&s.assists(p).unwrap(), s.mode(), InputSource::Keyboard),
        None
    );
}

/// Reset in place (playtest row 65): after the barrier crash the car is put back upright near
/// the crash, clear of the barrier, at rest and repaired; without repair (career) the damage
/// stays. The `ResetAt` op replays bit-exactly in simcli and in the browser core.
#[test]
fn reset_in_place_repairs_near_the_crash_and_replays() {
    let sc = scenario_file("crash/barrier_frontal.ron");
    let barrier = sc.barriers[0];
    for repair in [true, false] {
        let mut s = Session::new(sc.clone(), true).unwrap();
        let p = EntityId(1);
        // Scripted crash (7 s), then live.
        s.run_ticks(7 * 240 + 1);
        s.record_start();
        let before = s.sim().vehicle_states()[0].1.clone();
        assert!(
            before.health < 0.99,
            "the crash damaged the car ({})",
            before.health
        );
        let pose = s.reset_in_place(p, repair).unwrap();
        assert_eq!(
            s.sim().ghost_ticks(p),
            Some(u32::from(crate::reset::GHOST_TICKS)),
            "a reset car is a ghost for 1 s"
        );
        let d =
            ((pose.xy[0] - before.pos[0]).powi(2) + (pose.xy[1] - before.pos[1]).powi(2)).sqrt();
        assert!(d <= crate::reset::SNAP_RADIUS_M, "moved {d} m");
        assert!(
            crate::reset::is_clear(
                pose.xy,
                pose.yaw,
                &[crate::reset::Obstacle {
                    pos: barrier.pos,
                    yaw: barrier.yaw,
                    half: [barrier.size[0] * 0.5, barrier.size[1] * 0.5],
                }]
            ),
            "reset pose overlaps the barrier: {pose:?}"
        );
        // The scripted cruise input is still applied: hold the car still.
        s.set_input(p, VehicleInput::default()).unwrap();
        s.run_ticks(240);
        let after = s.sim().vehicle_states()[0].1.clone();
        // Upright: chassis up axis (from the quaternion) points up.
        let q = after.rot;
        let up_z = 1.0 - 2.0 * (q[0] * q[0] + q[1] * q[1]);
        assert!(up_z > 0.95, "upright after reset (up·z {up_z})");
        assert!(
            after.speed_mps.abs() < 1.0,
            "at rest ({} m/s)",
            after.speed_mps
        );
        if repair {
            assert!(after.health > 0.999, "repaired ({})", after.health);
        } else {
            assert!(
                (after.health - before.health).abs() < 1e-3,
                "career reset keeps the damage ({} → {})",
                before.health,
                after.health
            );
        }
        let bytes = s.record_stop().unwrap().encode();
        let rp = Replay::decode(&bytes).unwrap();
        assert!(rp.ops.iter().any(
            |o| matches!(o.op, simcli::replay::OpKind::ResetAt { repair: r, .. } if r == repair)
        ));
        assert!(rp.ops.iter().any(|o| matches!(
            o.op,
            simcli::replay::OpKind::Ghost { ticks } if ticks == crate::reset::GHOST_TICKS
        )));
        // 240 ticks ran since the reset: the window is over (the car is clear of the barrier).
        assert_eq!(
            s.sim().ghost_ticks(p),
            Some(0),
            "solid again after the window"
        );
        let report = replay::verify(&rp, mem_data()).unwrap();
        assert!(report.matched, "{}", report.to_text());
    }
}

/// In a city chase, reset in place lands on a lane near the car, heading its way, and keeps
/// the police heat.
#[test]
fn reset_in_place_in_the_city_snaps_to_a_lane_and_keeps_heat() {
    let sc = crate::scenario::city_chase("debug_sedan", DamageMode::Arcade);
    let mut s = Session::new(sc, false).unwrap();
    let p = EntityId(1);
    s.set_input(p, drive(0.6, 0.15)).unwrap();
    s.run_ticks(240 * 3);
    s.debug_sim_mut().unwrap().police_mut().set_heat(p, 0.6);
    let heat0 = crate::police::status(s.sim().police(), p).heat;
    let before = s.sim().vehicle_states()[0].1.clone();
    let pose = s.reset_in_place(p, true).unwrap();
    let heat1 = crate::police::status(s.sim().police(), p).heat;
    assert!(pose.on_lane, "city reset lands on a lane: {pose:?}");
    let d = ((pose.xy[0] - before.pos[0]).powi(2) + (pose.xy[1] - before.pos[1]).powi(2)).sqrt();
    assert!(d <= crate::reset::SNAP_RADIUS_M, "moved {d} m");
    assert!(
        (heat1 - heat0).abs() < 1e-6,
        "heat kept ({heat0} → {heat1})"
    );
    let st = s.sim().vehicle_states()[0].1.clone();
    assert!(st.speed_mps.abs() < 0.01 && st.health > 0.999);
}

#[test]
fn city_chase_uses_mixed_police_and_traffic_fleets() {
    let sc = crate::scenario::city_chase("debug_sedan", DamageMode::Arcade);
    assert!(sc.car_mix);
    let mut s = Session::new(sc, false).unwrap();
    s.set_input(EntityId(1), drive(0.6, 0.0)).unwrap();
    let traffic: Vec<String> = s
        .sim()
        .traffic_cars()
        .iter()
        .map(|c| c.def.id.clone())
        .collect();
    assert_eq!(traffic, simcli::run::CAR_MIX_TRAFFIC);
    let police: Vec<String> = s
        .sim()
        .police_fleet()
        .iter()
        .map(|m| m.car.def.id.clone())
        .collect();
    assert_eq!(police.len(), 5);
    let mut models_seen = std::collections::BTreeSet::new();
    for _ in 0..8 {
        s.run_ticks(120);
        // Every physical traffic car is the model its agent shows while kinematic.
        for v in s.sim().vehicles() {
            if let Some(a) = v.traffic_agent {
                let m = s.sim().world().traffic_model_of(a);
                assert_eq!(traffic[m], v.car_id);
            }
            if let Some(u) = v.police_unit {
                let m = s.sim().police_unit_model(u).unwrap();
                assert_eq!(police[m], v.car_id);
            }
        }
        for a in s.sim().world().agents() {
            models_seen.insert(s.sim().world().traffic_model_of(a.id));
        }
    }
    assert!(
        models_seen.len() >= 4,
        "traffic models in use: {models_seen:?}"
    );
    // Non-sim police units carry their stable id and model in the stream.
    let units = crate::police::units(s.sim().police(), &|_| 0.0, &|u| {
        s.sim().police_unit_model(u).map_or(-1.0, |m| m as f32)
    });
    assert_eq!(units.len() % crate::police::UNIT_STRIDE, 0);
    for u in units.chunks(crate::police::UNIT_STRIDE) {
        assert!(u[6] >= 0.0 && u[7] >= -1.0 && u[7] < 5.0, "{u:?}");
    }
}
