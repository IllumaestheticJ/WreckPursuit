//! Fixed-time signal controller evaluation. Signals run on simulation seconds (not game time).

use wp_contracts::SignalState;

use crate::network::SignalController;

/// State of `group` at simulation time `t_s`, plus the seconds until that state ends.
///
/// Each phase is green → yellow → all-red. A group that is also green in the following phase
/// stays green through the transition.
pub fn group_state(sc: &SignalController, group: u8, t_s: f64) -> (SignalState, f32) {
    let cycle = sc.cycle_s() as f64;
    let local = (t_s - sc.offset_s as f64).rem_euclid(cycle) as f32;
    let n = sc.phases.len();
    let mut start = 0.0f32;
    for (i, p) in sc.phases.iter().enumerate() {
        let end = start + p.green_s + p.yellow_s + p.all_red_s;
        if local < end || i == n - 1 {
            let in_this = p.green_groups.contains(&group);
            let next = &sc.phases[(i + 1) % n];
            let in_next = next.green_groups.contains(&group) && n > 1;
            let t = local - start;
            return if in_this {
                if t < p.green_s {
                    let rem = if in_next {
                        end - local + next.green_s
                    } else {
                        p.green_s - t
                    };
                    (SignalState::Green, rem)
                } else if in_next {
                    (SignalState::Green, end - local + next.green_s)
                } else if t < p.green_s + p.yellow_s {
                    (SignalState::Yellow, p.green_s + p.yellow_s - t)
                } else {
                    (SignalState::Red, end - local)
                }
            } else {
                (SignalState::Red, end - local)
            };
        }
        start = end;
    }
    (SignalState::Off, 0.0)
}

/// Start time (within the cycle, before offset) of the first phase in which `group` is green.
pub fn group_green_start(sc: &SignalController, group: u8) -> Option<f32> {
    let mut start = 0.0;
    for p in &sc.phases {
        if p.green_groups.contains(&group) {
            return Some(start);
        }
        start += p.green_s + p.yellow_s + p.all_red_s;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::SignalPhase;

    fn two_phase() -> SignalController {
        SignalController {
            intersection: 0,
            groups: 4,
            offset_s: 0.0,
            phases: vec![
                SignalPhase {
                    green_groups: vec![0, 2],
                    green_s: 20.0,
                    yellow_s: 3.0,
                    all_red_s: 2.0,
                },
                SignalPhase {
                    green_groups: vec![1, 3],
                    green_s: 15.0,
                    yellow_s: 3.0,
                    all_red_s: 2.0,
                },
            ],
        }
    }

    #[test]
    fn phase_sequence() {
        let sc = two_phase();
        assert_eq!(sc.cycle_s(), 45.0);
        assert_eq!(group_state(&sc, 0, 5.0).0, SignalState::Green);
        assert_eq!(group_state(&sc, 1, 5.0).0, SignalState::Red);
        assert_eq!(group_state(&sc, 0, 21.0).0, SignalState::Yellow);
        assert_eq!(group_state(&sc, 0, 24.0).0, SignalState::Red);
        assert_eq!(group_state(&sc, 1, 24.0).0, SignalState::Red);
        assert_eq!(group_state(&sc, 1, 26.0).0, SignalState::Green);
        // Wraps.
        assert_eq!(group_state(&sc, 0, 45.0 + 5.0).0, SignalState::Green);
        let (st, rem) = group_state(&sc, 0, 21.0);
        assert_eq!(st, SignalState::Yellow);
        assert!((rem - 2.0).abs() < 1e-4);
    }

    #[test]
    fn offset_shifts_cycle() {
        let mut sc = two_phase();
        sc.offset_s = 10.0;
        assert_eq!(group_state(&sc, 0, 5.0).0, SignalState::Red);
        assert_eq!(group_state(&sc, 0, 12.0).0, SignalState::Green);
        assert_eq!(group_green_start(&sc, 3), Some(25.0));
    }
}
