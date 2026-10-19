//! Traffic violation detection for tracked vehicles: speeding, red-light running, wrong-way
//! driving, reckless driving (near-miss count) and collisions with civilian traffic. Emits
//! [`SimEvent::TrafficViolation`]; `wp_police` turns them into heat.

use std::collections::{BTreeMap, BTreeSet};

use wp_contracts::{EntityId, SignalState, SimEvent, ViolationKind};

use crate::geom::{self, Obb};
use crate::map::{CityMap, SegId};
use crate::traffic::{AgentId, TrackedVehicle, Traffic};

/// Detection thresholds.
#[derive(Debug, Clone, PartialEq)]
pub struct ViolationConfig {
    /// Speeding when over the limit by more than `max(margin, ratio · limit)`, m/s.
    pub speeding_margin_mps: f32,
    /// Relative speeding margin (fraction of the limit).
    pub speeding_ratio: f32,
    /// Must be speeding this long before it counts, s.
    pub speeding_hold_s: f32,
    /// Sustained speeding / wrong-way re-emits this often, s.
    pub repeat_s: f32,
    /// Wrong-way must last this long, s.
    pub wrong_way_hold_s: f32,
    /// Box-to-box clearance counted as a near miss, m.
    pub near_miss_m: f32,
    /// Minimum relative speed for a near miss, m/s.
    pub near_miss_rel_speed: f32,
    /// Near misses within the window that make a `Reckless` violation.
    pub near_misses_for_reckless: usize,
    /// Near-miss window, s.
    pub reckless_window_s: f32,
}

impl Default for ViolationConfig {
    fn default() -> Self {
        Self {
            speeding_margin_mps: 2.24,
            speeding_ratio: 0.1,
            speeding_hold_s: 1.0,
            repeat_s: 5.0,
            wrong_way_hold_s: 1.5,
            near_miss_m: 1.5,
            near_miss_rel_speed: 8.0,
            near_misses_for_reckless: 3,
            reckless_window_s: 10.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct VState {
    lane: Option<SegId>,
    last_s: f32,
    over_s: f32,
    last_speeding: Option<u64>,
    wrong_s: f32,
    last_wrong: Option<u64>,
    near_misses: Vec<u64>,
    near_miss_seen: BTreeMap<AgentId, u64>,
    touching: BTreeSet<AgentId>,
}

/// Per-entity violation state machine.
#[derive(Debug, Clone, Default)]
pub struct ViolationTracker {
    /// Thresholds.
    pub config: ViolationConfig,
    states: BTreeMap<EntityId, VState>,
}

impl ViolationTracker {
    /// Update every tracked vehicle; `now` is the traffic step count (steps of `dt`).
    pub fn update(
        &mut self,
        map: &CityMap,
        tracked: &[TrackedVehicle],
        traffic: &Traffic,
        dt: f32,
        now: u64,
        out: &mut Vec<SimEvent>,
    ) {
        let cfg = self.config.clone();
        // Seconds since an earlier step.
        let since = |t0: u64| now.saturating_sub(t0) as f32 * dt;
        let live: BTreeSet<EntityId> = tracked.iter().map(|t| t.entity).collect();
        self.states.retain(|e, _| live.contains(e));
        let civ: Vec<(AgentId, Obb, f32, [f32; 2])> = traffic
            .agents()
            .iter()
            .map(|a| {
                let (obb, z) = traffic.agent_obb(map, a);
                (a.id, obb, z, geom::scale(obb.f, a.v))
            })
            .collect();
        for t in tracked {
            let st = self.states.entry(t.entity).or_default();
            let mut emit = |kind| {
                out.push(SimEvent::TrafficViolation {
                    vehicle: t.entity,
                    kind,
                })
            };
            let q = geom::xy(t.pos);
            let vel = [t.vel[0], t.vel[1]];
            let speed = geom::len(vel);
            let fwd = t.forward;

            // Red light: crossing the stop line of the lane we were on while it is red.
            if let Some(l) = st.lane {
                let seg = map.seg(l);
                let pr = seg.project(q);
                if pr.s > seg.length && st.last_s <= seg.length && pr.dist < 8.0 {
                    if map.lane_signal(l).map(|s| s.0) == Some(SignalState::Red) {
                        emit(ViolationKind::RedLight);
                    }
                    st.lane = None;
                }
            }
            let here = map.locate(t.pos, Some(fwd), 0.5, false, false);
            match here {
                Some(loc) => {
                    st.lane = Some(loc.seg);
                    st.last_s = loc.s;
                }
                None if st.lane.is_some() => {
                    // Keep the lane while near its end so the crossing is seen next update.
                    let l = st.lane.unwrap();
                    let seg = map.seg(l);
                    let pr = seg.project(q);
                    if pr.dist > 8.0 || pr.s < seg.length - 30.0 {
                        st.lane = None;
                    }
                }
                None => {}
            }

            // Speeding.
            let limit = here.map(|l| map.seg(l.seg).speed_limit);
            match limit {
                Some(lim)
                    if speed > lim + cfg.speeding_margin_mps.max(lim * cfg.speeding_ratio) =>
                {
                    st.over_s += dt;
                    let due = st.last_speeding.is_none_or(|t0| since(t0) >= cfg.repeat_s);
                    if st.over_s >= cfg.speeding_hold_s && due {
                        emit(ViolationKind::Speeding {
                            over_mps: speed - lim,
                        });
                        st.last_speeding = Some(now);
                    }
                }
                _ => st.over_s = 0.0,
            }

            // Wrong way: on a lane pointing the other way and on no lane pointing our way.
            let against = here.is_none()
                && speed > 3.0
                && map
                    .locate(t.pos, None, 0.0, true, false)
                    .is_some_and(|l| geom::dot(l.tangent, fwd) < -0.5);
            if against {
                st.wrong_s += dt;
                let due = st.last_wrong.is_none_or(|t0| since(t0) >= cfg.repeat_s);
                if st.wrong_s >= cfg.wrong_way_hold_s && due {
                    emit(ViolationKind::WrongWay);
                    st.last_wrong = Some(now);
                }
            } else {
                st.wrong_s = 0.0;
            }

            // Collisions and near misses with civilians.
            let me = Obb {
                c: q,
                f: fwd,
                hl: 2.3,
                hw: 0.95,
            };
            let near = Obb {
                hl: me.hl + cfg.near_miss_m,
                hw: me.hw + cfg.near_miss_m,
                ..me
            };
            let mut touching = BTreeSet::new();
            for (id, obb, z, cv) in &civ {
                if (z - t.pos[2]).abs() > 3.0 || geom::dist(obb.c, q) > 12.0 {
                    continue;
                }
                if geom::obb_overlap(&me, obb) {
                    touching.insert(*id);
                    if !st.touching.contains(id) {
                        emit(ViolationKind::Collision);
                    }
                } else if geom::obb_overlap(&near, obb)
                    && geom::len(geom::sub(vel, *cv)) > cfg.near_miss_rel_speed
                    && st.near_miss_seen.get(id).is_none_or(|t0| since(*t0) > 3.0)
                {
                    st.near_miss_seen.insert(*id, now);
                    st.near_misses.push(now);
                }
            }
            st.touching = touching;
            let window = cfg.reckless_window_s;
            st.near_misses.retain(|t0| since(*t0) <= window);
            st.near_miss_seen.retain(|_, t0| since(*t0) <= window);
            if st.near_misses.len() >= cfg.near_misses_for_reckless {
                emit(ViolationKind::Reckless);
                st.near_misses.clear();
            }
        }
    }
}
