//! Turn-signal timing (owner playtest 2026-10-21, "traffic doesn't use turn signals").
//!
//! Plain kinematic traffic (nobody tracked, so no physical agents, sirens or obstructions) on
//! small test networks; the indicator each agent shows ([`AgentView::turn_signal`]) is checked
//! against its own state every traffic step.

use std::collections::BTreeMap;

use super::*;
use crate::builder::{self, BuildOptions, NodeSpec, RoadClass, RoadSpec};
use crate::network::{DistrictKind, RoadNetwork, Zone};

fn node(x: f32, y: f32, signal: bool) -> NodeSpec {
    NodeSpec {
        pos: [x, y],
        z: 0.0,
        signal,
        control: None,
    }
}

fn network(name: &str, nodes: Vec<NodeSpec>, roads: Vec<RoadSpec>, poly: Vec<V2>) -> RoadNetwork {
    let built = builder::build(&nodes, &roads, &BuildOptions::default());
    let zone = Zone {
        kind: DistrictKind::Residential,
        density_base: 60.0,
        polygon: poly.clone(),
    };
    let c = [
        poly.iter().map(|p| p[0]).sum::<f32>() / poly.len() as f32,
        poly.iter().map(|p| p[1]).sum::<f32>() / poly.len() as f32,
    ];
    built.into_network(name, "test", None, c, vec![zone], Vec::new(), Vec::new())
}

/// 3 × 3 grid of two-lane (1+1) streets, 200 m blocks, signalised crossroads in the middle.
fn grid() -> RoadNetwork {
    let mut nodes = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            nodes.push(node(i as f32 * 200.0, j as f32 * 200.0, i == 1 && j == 1));
        }
    }
    let two = |a, b| RoadSpec::new(a, b, RoadClass::Local, 1, 1);
    let mut roads = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            let n = j * 3 + i;
            if i < 2 {
                roads.push(two(n, n + 1));
            }
            if j < 2 {
                roads.push(two(n, n + 3));
            }
        }
    }
    let poly = vec![
        [-60.0, -60.0],
        [460.0, -60.0],
        [460.0, 460.0],
        [-60.0, 460.0],
    ];
    network("signal_grid", nodes, roads, poly)
}

/// 1 km × 150 m loop whose west side is a 2+2 arterial (lane changes happen there).
fn arterial_loop() -> RoadNetwork {
    let nodes = vec![
        node(0.0, 0.0, false),
        node(0.0, 1000.0, false),
        node(150.0, 1000.0, false),
        node(150.0, 0.0, false),
    ];
    let two = |a, b| RoadSpec::new(a, b, RoadClass::Local, 1, 1);
    let roads = vec![
        two(1, 2),
        two(2, 3),
        two(3, 0),
        RoadSpec::new(0, 1, RoadClass::Arterial, 2, 2),
    ];
    let poly = vec![
        [-210.0, -60.0],
        [210.0, -60.0],
        [210.0, 1060.0],
        [-210.0, 1060.0],
    ];
    network("arterial_loop", nodes, roads, poly)
}

/// Run `seconds` of traffic on `net`, calling `f` after every traffic step with the agents and
/// their views (same order).
fn run(
    net: RoadNetwork,
    seed: u64,
    density: f32,
    seconds: f32,
    mut f: impl FnMut(u64, &CityMap, &[Agent], &[AgentView]),
) {
    let mut map = CityMap::new(net);
    let cfg = TrafficConfig {
        density_scale: density,
        ..Default::default()
    };
    let mut t = Traffic::new(&map, cfg, seed);
    let closed = vec![false; map.n_lanes()];
    let inp = TrafficInputs {
        tracked: &[],
        hazards: &[],
        route_focus: &[],
        closed: &closed,
    };
    t.populate(&map, &inp);
    let mut ev = Vec::new();
    for _ in 0..(seconds / TRAFFIC_DT) as usize {
        map.signal_tick += TICKS_PER_TRAFFIC_STEP;
        t.step(&map, &inp, &mut ev);
        let views: Vec<AgentView> = t.views(&map).collect();
        f(t.step_count(), &map, t.agents(), &views);
    }
}

/// No indicator cause other than the junction ahead / the connector it is in.
fn junction_only(a: &Agent) -> bool {
    a.ghost.is_none()
        && a.lc_signal == 0
        && a.pending_lc.is_none()
        && a.siren.phase == YieldPhase::None
        && a.manoeuvre.is_none()
        && !a.disabled
}

fn turn_of(map: &CityMap, seg: SegId) -> Option<TurnKind> {
    match map.seg(seg).kind {
        SegKind::Connector { turn, .. } => Some(turn),
        _ => None,
    }
}

#[test]
fn lead_distance_is_three_seconds_clamped() {
    assert_eq!(turn_signal_lead_m(0.0), TURN_SIGNAL_MIN_M);
    assert_eq!(turn_signal_lead_m(14.0), 42.0);
    assert_eq!(turn_signal_lead_m(30.0), TURN_SIGNAL_MAX_M);
}

/// A car approaching a turn indicates 30–50 m (about 3 s) before the lane end, not at 80 m,
/// keeps indicating through the junction and switches off on the exit lane.
#[test]
fn junction_turns_are_indicated_before_and_through_the_turn() {
    // Per agent: the connector whose indicator onset was recorded.
    let mut approach: BTreeMap<u32, SegId> = BTreeMap::new();
    // Right turns: (onset distance, speed at onset), and whether the car was seen 80 m out
    // with the indicator off.
    let mut right_onsets: Vec<(f32, f32)> = Vec::new();
    let mut seen_off_at_80: BTreeMap<u32, SegId> = BTreeMap::new();
    let mut right_80_then_on = 0;
    let mut connector_steps = [0usize; 2];
    let mut exit_checks = 0;
    for seed in [1u64, 2] {
        run(grid(), seed, 2.0, 90.0, |_, map, agents, views| {
            for (a, v) in agents.iter().zip(views) {
                if !junction_only(a) {
                    approach.remove(&a.id);
                    continue;
                }
                let seg = map.seg(a.seg);
                if let Some(turn) = turn_of(map, a.seg) {
                    // In the junction: the connector's own turn, all the way through.
                    assert_eq!(
                        v.turn_signal,
                        turn_indicator(turn),
                        "agent {} in connector",
                        a.id
                    );
                    if turn != TurnKind::Straight {
                        connector_steps[(turn == TurnKind::Right) as usize] += 1;
                    }
                    continue;
                }
                let d_end = seg.length - a.s;
                let ahead = (a.plan[0] != NONE)
                    .then(|| turn_of(map, a.plan[0]))
                    .flatten();
                match ahead {
                    Some(turn) if turn != TurnKind::Straight => {
                        let dir = turn_indicator(turn);
                        if d_end > TURN_SIGNAL_MAX_M + 0.5 {
                            assert_eq!(v.turn_signal, 0, "agent {} at {d_end:.1} m", a.id);
                            if turn == TurnKind::Right && (75.0..85.0).contains(&d_end) {
                                seen_off_at_80.insert(a.id, a.plan[0]);
                            }
                        } else if d_end < turn_signal_lead_m(a.v) {
                            assert_eq!(v.turn_signal, dir, "agent {} at {d_end:.1} m", a.id);
                        }
                        if v.turn_signal == dir && approach.get(&a.id) != Some(&a.plan[0]) {
                            // First step with the indicator on for this turn.
                            approach.insert(a.id, a.plan[0]);
                            if turn == TurnKind::Right {
                                right_onsets.push((d_end, a.v));
                                if seen_off_at_80.get(&a.id) == Some(&a.plan[0]) {
                                    right_80_then_on += 1;
                                }
                            }
                        }
                    }
                    _ => {
                        // Straight on / exit lane / no junction ahead: off.
                        assert_eq!(v.turn_signal, 0, "agent {} at {d_end:.1} m", a.id);
                        if a.came_from != NONE
                            && turn_of(map, a.came_from).is_some_and(|t| t != TurnKind::Straight)
                        {
                            exit_checks += 1;
                        }
                    }
                }
            }
        });
    }
    println!(
        "right-turn onsets: {} (80 m off then on: {right_80_then_on}), connector steps \
         left {} right {}, exit-lane checks {exit_checks}",
        right_onsets.len(),
        connector_steps[0],
        connector_steps[1]
    );
    assert!(
        right_80_then_on >= 3,
        "too few right turns observed from 80 m out"
    );
    assert!(connector_steps[0] > 0 && connector_steps[1] > 0);
    assert!(exit_checks > 0);
    // Spawned inside the zone or on approach: the onset is within the lead distance, and for a
    // car that came from further out at speed it is 30–50 m (≥ 2 s) before the lane end.
    let mut timely = 0;
    for &(d, v) in &right_onsets {
        assert!(d <= TURN_SIGNAL_MAX_M + 0.5, "onset at {d:.1} m");
        if d >= TURN_SIGNAL_MIN_M - 0.5 {
            assert!(d / v.max(0.1) >= 2.0, "onset {d:.1} m at {v:.1} m/s");
            timely += 1;
        }
    }
    assert!(timely >= 3, "{right_onsets:?}");
}

/// Discretionary lane changes are indicated for at least [`LANE_CHANGE_SIGNAL_S`] before the
/// car moves over, and while it moves over.
#[test]
fn lane_changes_are_indicated_before_and_during_the_move() {
    // Per agent: (direction, step the current indicator run started).
    let mut on_since: BTreeMap<u32, (i8, u64)> = BTreeMap::new();
    let mut changes = 0;
    let mut during = 0;
    for seed in [1u64, 2, 3] {
        run(
            arterial_loop(),
            seed,
            2.0,
            120.0,
            |step, _map, agents, views| {
                for (a, v) in agents.iter().zip(views) {
                    let prev = on_since.get(&a.id).copied();
                    if a.last_lane_change == step && a.ghost.is_some() {
                        let dir = a.lc_signal;
                        assert_ne!(dir, 0);
                        assert_eq!(v.turn_signal, dir);
                        let (pdir, since) = prev.expect("lane change without indicating first");
                        assert_eq!(pdir, dir, "agent {} indicated the other way", a.id);
                        assert!(
                            steps_to_s(step - since) >= LANE_CHANGE_SIGNAL_S,
                            "agent {} indicated only {:.2} s",
                            a.id,
                            steps_to_s(step - since)
                        );
                        changes += 1;
                    }
                    if a.lc_signal != 0 {
                        // Moving over: on until centred in the new lane.
                        assert_eq!(v.turn_signal, a.lc_signal);
                        if a.ghost.is_none() {
                            assert!(a.lat.abs() >= LANE_CHANGE_SIGNAL_OFF_M - 0.05);
                            during += 1;
                        }
                    }
                    if v.turn_signal == 0 {
                        on_since.remove(&a.id);
                    } else if prev.is_none_or(|p| p.0 != v.turn_signal) {
                        on_since.insert(a.id, (v.turn_signal, step));
                    }
                }
            },
        );
    }
    println!("lane changes {changes}, steps indicated after the ghost cleared {during}");
    assert!(changes >= 3, "too few lane changes: {changes}");
    assert!(during > 0);
}
