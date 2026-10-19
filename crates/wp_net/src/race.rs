//! Lightweight race lobby inside an instance: create (from client-supplied checkpoints —
//! `wp_world`'s route helper will produce them), invite/join, countdown, server-side
//! checkpoint validation, results and payouts.
//!
//! Stakes: `base_stake × (1 + night_factor)`, so a midnight race costs (and pays) twice a noon
//! race. Payout split of the pot by place: 1 racer 100 %, 2 racers 70/30, 3+ racers 60/30/10.
//! These numbers are placeholders until `wp_police`/`wp_economy` publish their stake tables
//! (contract request NET-4).

use std::collections::BTreeMap;

use wp_contracts::{PlayerId, TimeOfDay};

use crate::protocol::{RaceInfo, RaceResultEntry, RaceState};

/// Checkpoint capture radius, m.
pub const CHECKPOINT_RADIUS_M: f32 = 15.0;
/// Countdown length, physics ticks (3 s).
pub const COUNTDOWN_TICKS: u64 = 3 * crate::TICK_HZ;
/// Races are cut off (remaining racers DNF) after this many ticks (10 min).
pub const RACE_TIME_LIMIT_TICKS: u64 = 600 * crate::TICK_HZ;
/// Base entry stake (day).
pub const BASE_STAKE: i64 = 500;

/// Something the server must broadcast after a race step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaceEvent {
    /// Race info changed (send to members + anyone interested).
    Updated(RaceInfo),
    /// A racer reached a checkpoint.
    Progress {
        /// Race.
        race: u32,
        /// Player.
        player: PlayerId,
        /// Next checkpoint index (across laps).
        next_checkpoint: u32,
    },
    /// Stakes were charged at the start.
    StakesCharged {
        /// Race.
        race: u32,
        /// `(player, amount)`.
        charges: Vec<(PlayerId, i64)>,
    },
    /// Race finished.
    Finished {
        /// Race.
        race: u32,
        /// Results by place (DNF last), payouts filled in.
        results: Vec<RaceResultEntry>,
    },
}

#[derive(Debug, Clone)]
struct Racer {
    next_checkpoint: u32,
    finish_tick: Option<u64>,
    left: bool,
}

#[derive(Debug, Clone)]
struct Race {
    info: RaceInfo,
    racers: BTreeMap<PlayerId, Racer>,
}

impl Race {
    fn total_checkpoints(&self) -> u32 {
        self.info.checkpoints.len() as u32 * self.info.laps.max(1) as u32
    }
}

/// Every race in one instance.
#[derive(Debug, Clone, Default)]
pub struct RaceLobby {
    races: BTreeMap<u32, Race>,
    next_id: u32,
}

/// Why a lobby request was refused.
pub type RaceError = String;

impl RaceLobby {
    /// Empty lobby.
    pub fn new() -> Self {
        Self {
            races: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Stake for a race created at `time`.
    pub fn stake_for(time: TimeOfDay) -> i64 {
        (BASE_STAKE as f32 * (1.0 + time.night_factor())).round() as i64
    }

    /// Race a player is in (not finished), if any.
    pub fn race_of(&self, player: PlayerId) -> Option<u32> {
        self.races
            .values()
            .find(|r| {
                r.info.state != RaceState::Finished
                    && r.racers.get(&player).is_some_and(|x| !x.left)
            })
            .map(|r| r.info.race)
    }

    /// Public info of a race.
    pub fn info(&self, race: u32) -> Option<&RaceInfo> {
        self.races.get(&race).map(|r| &r.info)
    }

    /// Create a race hosted by `host` (who joins it).
    pub fn create(
        &mut self,
        host: PlayerId,
        laps: u8,
        checkpoints: Vec<[i32; 2]>,
        time: TimeOfDay,
    ) -> Result<RaceEvent, RaceError> {
        if self.race_of(host).is_some() {
            return Err("already in a race".into());
        }
        if !(2..=crate::protocol::MAX_CHECKPOINTS).contains(&checkpoints.len()) {
            return Err("a race needs 2..=64 checkpoints".into());
        }
        if !(1..=10).contains(&laps) {
            return Err("laps must be 1..=10".into());
        }
        for pair in checkpoints.windows(2) {
            let dx = (pair[1][0] - pair[0][0]) as f32 / 10.0;
            let dy = (pair[1][1] - pair[0][1]) as f32 / 10.0;
            if (dx * dx + dy * dy).sqrt() < 2.0 * CHECKPOINT_RADIUS_M {
                return Err("checkpoints closer than 30 m".into());
            }
        }
        let race = self.next_id;
        self.next_id += 1;
        let info = RaceInfo {
            race,
            host: host.0,
            state: RaceState::Lobby,
            start_tick: 0,
            stake: Self::stake_for(time),
            laps,
            checkpoints,
            members: vec![host.0],
        };
        let mut racers = BTreeMap::new();
        racers.insert(
            host,
            Racer {
                next_checkpoint: 0,
                finish_tick: None,
                left: false,
            },
        );
        self.races.insert(
            race,
            Race {
                info: info.clone(),
                racers,
            },
        );
        Ok(RaceEvent::Updated(info))
    }

    /// Join a race still in its lobby.
    pub fn join(&mut self, player: PlayerId, race: u32) -> Result<RaceEvent, RaceError> {
        if self.race_of(player).is_some() {
            return Err("already in a race".into());
        }
        let r = self.races.get_mut(&race).ok_or("no such race")?;
        if r.info.state != RaceState::Lobby {
            return Err("race already started".into());
        }
        if r.racers.len() >= 16 {
            return Err("race full".into());
        }
        r.racers.insert(
            player,
            Racer {
                next_checkpoint: 0,
                finish_tick: None,
                left: false,
            },
        );
        r.info.members.push(player.0);
        Ok(RaceEvent::Updated(r.info.clone()))
    }

    /// Check an invitation is allowed (inviter is a member of a lobby-state race).
    pub fn can_invite(&self, from: PlayerId, race: u32) -> bool {
        self.races
            .get(&race)
            .is_some_and(|r| r.info.state == RaceState::Lobby && r.racers.contains_key(&from))
    }

    /// Host starts the countdown; stakes are charged when the race goes live.
    pub fn start(
        &mut self,
        player: PlayerId,
        race: u32,
        tick: u64,
    ) -> Result<RaceEvent, RaceError> {
        let r = self.races.get_mut(&race).ok_or("no such race")?;
        if r.info.host != player.0 {
            return Err("only the host can start".into());
        }
        if r.info.state != RaceState::Lobby {
            return Err("race already started".into());
        }
        r.info.state = RaceState::Countdown;
        r.info.start_tick = tick + COUNTDOWN_TICKS;
        Ok(RaceEvent::Updated(r.info.clone()))
    }

    /// Leave a race. In the lobby you are removed; once running you DNF.
    pub fn leave(&mut self, player: PlayerId, race: u32) -> Option<RaceEvent> {
        let r = self.races.get_mut(&race)?;
        if r.info.state == RaceState::Lobby || r.info.state == RaceState::Countdown {
            r.racers.remove(&player)?;
            r.info.members.retain(|m| *m != player.0);
            if r.racers.is_empty() {
                self.races.remove(&race);
                return None;
            }
            if r.info.host == player.0 {
                r.info.host = r.info.members[0];
            }
            return Some(RaceEvent::Updated(r.info.clone()));
        }
        if let Some(x) = r.racers.get_mut(&player) {
            x.left = true;
        }
        None
    }

    /// Advance all races to `tick`. `position_of` returns a player's vehicle XY (m).
    pub fn update(
        &mut self,
        tick: u64,
        position_of: impl Fn(PlayerId) -> Option<[f32; 2]>,
    ) -> Vec<RaceEvent> {
        let mut events = Vec::new();
        let mut finished = Vec::new();
        for (id, r) in &mut self.races {
            match r.info.state {
                RaceState::Countdown if tick >= r.info.start_tick => {
                    r.info.state = RaceState::Running;
                    events.push(RaceEvent::StakesCharged {
                        race: *id,
                        charges: r.racers.keys().map(|p| (*p, r.info.stake)).collect(),
                    });
                    events.push(RaceEvent::Updated(r.info.clone()));
                }
                RaceState::Running => {
                    let total = r.total_checkpoints();
                    let n = r.info.checkpoints.len() as u32;
                    for (p, racer) in r.racers.iter_mut() {
                        if racer.left || racer.finish_tick.is_some() {
                            continue;
                        }
                        let Some(pos) = position_of(*p) else { continue };
                        let cp = r.info.checkpoints[(racer.next_checkpoint % n) as usize];
                        let dx = pos[0] - cp[0] as f32 / 10.0;
                        let dy = pos[1] - cp[1] as f32 / 10.0;
                        if dx * dx + dy * dy <= CHECKPOINT_RADIUS_M * CHECKPOINT_RADIUS_M {
                            racer.next_checkpoint += 1;
                            events.push(RaceEvent::Progress {
                                race: *id,
                                player: *p,
                                next_checkpoint: racer.next_checkpoint,
                            });
                            if racer.next_checkpoint >= total {
                                racer.finish_tick = Some(tick);
                            }
                        }
                    }
                    let done = r.racers.values().all(|x| x.left || x.finish_tick.is_some());
                    let timed_out = tick >= r.info.start_tick + RACE_TIME_LIMIT_TICKS;
                    if done || timed_out {
                        finished.push(*id);
                    }
                }
                _ => {}
            }
        }
        for id in finished {
            let r = self.races.get_mut(&id).expect("race exists");
            r.info.state = RaceState::Finished;
            let results = results(r);
            events.push(RaceEvent::Updated(r.info.clone()));
            events.push(RaceEvent::Finished { race: id, results });
            self.races.remove(&id);
        }
        events
    }
}

fn results(r: &Race) -> Vec<RaceResultEntry> {
    let mut finishers: Vec<(u64, PlayerId)> = r
        .racers
        .iter()
        .filter_map(|(p, x)| x.finish_tick.map(|t| (t, *p)))
        .collect();
    finishers.sort();
    let pot = r.info.stake * r.racers.len() as i64;
    let split: &[i64] = match r.racers.len() {
        1 => &[100],
        2 => &[70, 30],
        _ => &[60, 30, 10],
    };
    let mut out = Vec::new();
    for (i, (t, p)) in finishers.iter().enumerate() {
        out.push(RaceResultEntry {
            player: p.0,
            place: (i + 1).min(255) as u8,
            time_ticks: t - r.info.start_tick,
            payout: split.get(i).map_or(0, |pct| pot * pct / 100),
        });
    }
    // Integer rounding remainder goes to the winner when every paid place was taken.
    if finishers.len() >= split.len() {
        let paid: i64 = out.iter().map(|e| e.payout).sum();
        out[0].payout += pot - paid;
    }
    for p in r.racers.keys() {
        if !finishers.iter().any(|(_, f)| f == p) {
            out.push(RaceResultEntry {
                player: p.0,
                place: 0,
                time_ticks: 0,
                payout: 0,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn full_race_flow_with_checkpoints_and_payouts() {
        let mut lobby = RaceLobby::new();
        let noon = TimeOfDay::from_hours(12.0);
        let cps = vec![[0, 1000], [0, 2000]]; // (0,100 m) then (0,200 m)
        let RaceEvent::Updated(info) = lobby.create(PlayerId(1), 1, cps, noon).unwrap() else {
            panic!()
        };
        assert_eq!(info.stake, BASE_STAKE);
        lobby.join(PlayerId(2), info.race).unwrap();
        assert!(
            lobby.join(PlayerId(2), info.race).is_err(),
            "no double join"
        );
        assert!(
            lobby.start(PlayerId(2), info.race, 0).is_err(),
            "only host starts"
        );
        lobby.start(PlayerId(1), info.race, 0).unwrap();

        // Both racers drive up +Y; player 1 is faster.
        let pos = RefCell::new(BTreeMap::from([(1u32, 0.0f32), (2u32, 0.0f32)]));
        let mut all = Vec::new();
        for tick in (0..20_000u64).step_by(4) {
            all.extend(lobby.update(tick, |p| pos.borrow().get(&p.0).map(|y| [0.0, *y])));
            if tick >= COUNTDOWN_TICKS {
                let mut m = pos.borrow_mut();
                *m.get_mut(&1).unwrap() += 0.2;
                *m.get_mut(&2).unwrap() += 0.15;
            }
        }
        assert!(all
            .iter()
            .any(|e| matches!(e, RaceEvent::StakesCharged { charges, .. } if charges.len() == 2)));
        let results = all
            .iter()
            .find_map(|e| match e {
                RaceEvent::Finished { results, .. } => Some(results.clone()),
                _ => None,
            })
            .expect("race finished");
        assert_eq!(results[0].player, 1);
        assert_eq!(results[0].place, 1);
        assert_eq!(results[0].payout, 700);
        assert_eq!(results[1].payout, 300);
        assert!(results[0].time_ticks < results[1].time_ticks);
        assert_eq!(lobby.race_of(PlayerId(1)), None);
    }

    #[test]
    fn night_races_cost_more_and_bad_routes_are_rejected() {
        assert_eq!(
            RaceLobby::stake_for(TimeOfDay::from_hours(0.0)),
            2 * BASE_STAKE
        );
        let mut lobby = RaceLobby::new();
        let t = TimeOfDay::from_hours(12.0);
        assert!(lobby.create(PlayerId(1), 1, vec![[0, 0]], t).is_err());
        assert!(lobby
            .create(PlayerId(1), 1, vec![[0, 0], [10, 10]], t)
            .is_err());
        assert!(lobby
            .create(PlayerId(1), 0, vec![[0, 0], [0, 1000]], t)
            .is_err());
    }

    #[test]
    fn leaving_running_race_is_dnf() {
        let mut lobby = RaceLobby::new();
        let t = TimeOfDay::from_hours(12.0);
        lobby
            .create(PlayerId(1), 1, vec![[0, 1000], [0, 2000]], t)
            .unwrap();
        lobby.join(PlayerId(2), 1).unwrap();
        lobby.start(PlayerId(1), 1, 0).unwrap();
        lobby.update(COUNTDOWN_TICKS, |_| None);
        lobby.leave(PlayerId(2), 1);
        let ev = lobby.update(COUNTDOWN_TICKS + 4, |p| {
            (p == PlayerId(1)).then_some([0.0, 100.0])
        });
        assert!(ev.iter().all(|e| !matches!(e, RaceEvent::Finished { .. })));
        let ev = lobby.update(COUNTDOWN_TICKS + 8, |p| {
            (p == PlayerId(1)).then_some([0.0, 200.0])
        });
        let results = ev
            .iter()
            .find_map(|e| match e {
                RaceEvent::Finished { results, .. } => Some(results.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(results[1].place, 0, "leaver is DNF");
        assert_eq!(results[0].payout, 700);
    }
}
