//! Where and why police cars sit still in the browser city chase.
//!
//! `cargo run --release -p wp_wasm --example cop_stuck_probe -- [seconds] [drive|park] [heat] [unit]`
//!
//! Runs the city chase (`drive`: the player weaves at 0.7 throttle; `park`: it brakes in place,
//! so police only patrol unless `heat` is given), samples every police car once a second and
//! reports each stretch a car spent below 0.5 m/s for more than 8 s outside a roadblock, spike
//! strip, crash scene or arrest hold: where, for how long, its pedals, what its avoidance logic
//! says, and the civilians around it. With `unit`, also traces that unit every second. Ends with
//! totals (stuck car-seconds per car-minute).
//!
//! Before the 2026-10-20 fixes, `240 park` had all 5 cars stuck for good (38 s per car-minute).

use std::collections::BTreeMap;

use wp_contracts::{DamageMode, EntityId, VehicleInput, VehicleSim};
use wp_police::unit::{Assignment, Unit};
use wp_police::UnitKind;
use wp_wasm::scenario::city_chase;
use wp_wasm::session::Session;

/// Stopped this long counts as stuck, s.
const STUCK_AFTER_S: f32 = 8.0;

#[derive(Default)]
struct Still {
    spawn: [f32; 2],
    secs: f32,
    at: [f32; 2],
    why: String,
    counted: bool,
}

fn player_input(drive: bool, frame: u32) -> VehicleInput {
    if drive {
        VehicleInput {
            throttle: 0.7,
            steer: if (frame / 240).is_multiple_of(2) {
                0.2
            } else {
                -0.2
            },
            ..Default::default()
        }
    } else {
        VehicleInput {
            brake: 1.0,
            ..Default::default()
        }
    }
}

/// Holds where standing still is the job.
fn holding(u: &Unit) -> bool {
    matches!(
        u.assignment,
        Assignment::RoadblockAt { .. }
            | Assignment::SpikeAt { .. }
            | Assignment::OutOfService
            | Assignment::Respond { .. }
    ) && !u.cmd.track.is_drivable()
}

/// One line on a unit: pedals, avoidance state, civilians within 14 m (in its frame).
fn describe(s: &Session, u: &Unit) -> String {
    let veh = s
        .sim()
        .vehicles()
        .iter()
        .find(|v| Some(v.id) == u.entity)
        .expect("bound unit has a vehicle");
    let i = veh.input;
    let st = veh.vehicle.state();
    let (_, lat) = u.cmd.track.project(u.body.pos, u.track_s, 30.0);
    let h = u.body.heading;
    let civilians: Vec<String> = s
        .sim()
        .world()
        .agents()
        .filter(|a| (a.pos[0] - st.pos[0]).hypot(a.pos[1] - st.pos[1]) < 14.0)
        .map(|a| {
            let r = [a.pos[0] - st.pos[0], a.pos[1] - st.pos[1]];
            let (sn, cs) = wp_math::sin_cos(a.yaw);
            format!(
                "#{}(ahead {:.1} right {:.1} facing {:.2} v {:.1}{})",
                a.id,
                r[0] * h[0] + r[1] * h[1],
                r[0] * h[1] - r[1] * h[0],
                -sn * h[0] + cs * h[1],
                a.speed_mps,
                if a.yielding { " yielding" } else { "" }
            )
        })
        .collect();
    format!(
        "{} th={:.2} br={:.2} st={:.2} gear={} wheels={} lat={lat:.1} | limit={:.1} by={:?} \
         junction={:?} pass={:?} | {civilians:?}",
        u.assignment.name(),
        i.throttle,
        i.brake,
        i.steer,
        st.gear,
        st.wheel_states.iter().filter(|w| w.contact).count(),
        u.avoid.limit_mps,
        u.avoid.blocked_by,
        u.avoid
            .junction
            .map(|j| (j.s - u.track_s, j.light, j.turn_dir)),
        u.avoid.pass_offset_m,
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seconds: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(240);
    let drive = args.get(2).map(String::as_str) != Some("park");
    let heat: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let trace: Option<u32> = args.get(4).and_then(|s| s.parse().ok());
    let mut s =
        Session::new(city_chase("debug_sedan", DamageMode::Arcade), false).expect("session");
    let player = EntityId(1);
    let mut still: BTreeMap<u32, Still> = BTreeMap::new();
    let (mut car_s, mut stuck_s, mut episodes) = (0.0f32, 0.0f32, 0u32);
    let report = |when: String, id: u32, t: &Still| {
        let d = (t.at[0] - t.spawn[0]).hypot(t.at[1] - t.spawn[1]);
        println!(
            "{when} unit {id:>3} still {:>4.0} s at ({:>7.1},{:>7.1}), {d:>5.0} m from spawn | {}",
            t.secs, t.at[0], t.at[1], t.why
        );
    };
    for sec in 0..seconds {
        if sec == 2 && heat > 0.0 {
            s.debug_sim_mut()
                .unwrap()
                .police_mut()
                .set_heat(player, heat);
        }
        for f in 0..60 {
            let _ = s.set_input(player, player_input(drive, sec * 60 + f));
            s.step(1.0 / 60.0);
        }
        let units: Vec<Unit> = s
            .sim()
            .police()
            .units()
            .iter()
            .filter(|u| u.kind == UnitKind::Car && u.entity.is_some())
            .cloned()
            .collect();
        for u in &units {
            car_s += 1.0;
            if trace == Some(u.id.0) {
                println!(
                    "trace t={sec} pos=({:.1},{:.1}) v={:.1} {}",
                    u.body.pos[0],
                    u.body.pos[1],
                    u.body.speed,
                    describe(&s, u)
                );
            }
            let t = still.entry(u.id.0).or_insert_with(|| Still {
                spawn: u.body.pos,
                ..Default::default()
            });
            if u.body.speed.abs() < 0.5 && !holding(u) {
                if t.secs == 0.0 {
                    t.at = u.body.pos;
                }
                t.secs += 1.0;
                t.why = describe(&s, u);
                if t.secs > STUCK_AFTER_S {
                    stuck_s += 1.0;
                    if !t.counted {
                        t.counted = true;
                        episodes += 1;
                    }
                }
            } else {
                if t.secs > STUCK_AFTER_S {
                    report(format!("t={sec:>4}s"), u.id.0, t);
                }
                t.secs = 0.0;
                t.counted = false;
            }
        }
    }
    for (id, t) in &still {
        if t.secs > STUCK_AFTER_S {
            report("END    ".into(), *id, t);
        }
    }
    println!(
        "{} police cars, {car_s:.0} car-s; {episodes} stuck episodes, {stuck_s:.0} car-s stuck \
         ({:.1} s per car-minute); racer {:?}",
        still.len(),
        stuck_s / car_s.max(1.0) * 60.0,
        s.sim().police().racer(player).map(|r| (r.heat, r.status))
    );
}
