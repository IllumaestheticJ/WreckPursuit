//! The police commander (owner 2026-10-27): the background "desk" that decides how many police
//! the city may put out and where, and reacts to what dispatch hears.
//!
//! Every think the commander works out a [`CommanderBudget`] from the time of day, the highest
//! wanted level and the highest bounty of any racer the police know about (wanted, or called
//! in recently; career bounty carried in counts). Day is the richest phase. The budget caps:
//! - ground units out at once ([`PoliceSystem::unit_cap`]), on top of `presence.toml`'s own
//!   limits;
//! - captain roadblocks, spike strips and helicopters out at once, on top of `tools.toml` /
//!   `presence.toml` (it never unlocks anything earlier than `[unlocks]`);
//! - the extra units it may add to a reckless-driver call and the extra patrols it may put
//!   into a district.
//!
//! **Dispatch intel.** Every reckless-driver report (civilian call or traffic camera) is logged
//! ([`PoliceSystem::commander_note_call`]). When one driver has been called in
//! `calls.escalate_calls` times inside `calls.window_s`, the commander adds up to
//! `calls.extra_units` responders to the open call (idle units close by, else spawned out of
//! sight ahead of the driver), so units converge on the driver's predicted path from several
//! sides. When a district gets `calls.surge_calls` calls, it puts `calls.surge_units` extra
//! patrols there for `calls.surge_hold_s` after the last call. Each decision is a
//! [`CommanderOrder`] in the [`CommanderView`] (what the renderer / testbed / captain screen
//! shows) and one radio line.
//!
//! **Races.** A race's own police profile (`races.toml`) is the commander's operation order:
//! while a race is policed the commander neither cuts it nor adds call units or surges.
//!
//! Tables: `data/police/commander.toml`.

use serde::{Deserialize, Serialize};
use wp_contracts::{DayPhase, EntityId, WorldQuery};

use crate::config::PhaseTable;
use crate::dispatch::IncidentKind;
use crate::events::UnitKind;
use crate::graph::{dist, District};
use crate::heat::WantedLevel;
use crate::radio::{RadioCue, RadioTier};
use crate::time::SimTime;
use crate::unit::{Assignment, UnitRole};
use crate::PoliceSystem;

/// Orders kept for the view.
const ORDER_LOG: usize = 16;

/// How much of one resource the commander can afford (see `commander.toml`):
/// `base[phase] + per_level[level] + min(floor(bounty / bounty_step), bounty_max)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allotment {
    /// Amount by time of day.
    pub base: PhaseTable<u32>,
    /// Added by the highest wanted level L0..L5.
    pub per_level: [u32; 6],
    /// One more per this much bounty (0 = bounty doesn't count).
    pub bounty_step: f32,
    /// At most this many from bounty.
    pub bounty_max: u32,
}

impl Allotment {
    /// The amount for `phase`, wanted `level` (0..=5) and `bounty`.
    pub fn amount(&self, phase: DayPhase, level: usize, bounty: f32) -> u32 {
        let from_bounty = if self.bounty_step > 0.0 {
            ((bounty.max(0.0) / self.bounty_step).floor() as u32).min(self.bounty_max)
        } else {
            0
        };
        self.base.get(phase) + self.per_level[level.min(5)] + from_bounty
    }
}

/// `[calls]` in `commander.toml`: how the commander reacts to reckless-driver calls.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallsConfig {
    /// Calls are remembered this long, s.
    pub window_s: f32,
    /// Reports about one driver closer together than this are one call, s.
    pub min_gap_s: f32,
    /// Calls on one driver inside the window before units are added to the call.
    pub escalate_calls: u32,
    /// Calls inside the window from one district before it gets extra patrols.
    pub surge_calls: u32,
    /// A surge holds this long after the district's last call, s.
    pub surge_hold_s: f32,
    /// Extra units on an escalated call.
    pub extra_units: Allotment,
    /// Extra patrols in a district with a surge.
    pub surge_units: Allotment,
}

/// `commander.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommanderConfig {
    /// Off: no budget caps, no call escalation, no surges (the police before the commander).
    pub enabled: bool,
    /// Ground units out at once.
    pub ground_units: Allotment,
    /// Captain roadblocks out at once.
    pub roadblocks: Allotment,
    /// Spike strips out at once.
    pub spikes: Allotment,
    /// Helicopters up at once.
    pub helicopters: Allotment,
    /// Reckless-driver calls.
    pub calls: CallsConfig,
}

/// What the commander can afford right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CommanderBudget {
    /// Ground units out at once (patrols included).
    pub ground_units: u32,
    /// Captain roadblocks out at once.
    pub roadblocks: u32,
    /// Spike strips out at once.
    pub spikes: u32,
    /// Helicopters up at once.
    pub helicopters: u32,
    /// Extra units on an escalated reckless-driver call.
    pub call_units: u32,
    /// Extra patrols in a district with a surge of calls.
    pub surge_units: u32,
}

/// What a [`CommanderOrder`] did.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OrderKind {
    /// More units on a reckless-driver call (`units` added, about `racer`).
    ReinforceCall {
        /// The reported driver.
        racer: EntityId,
    },
    /// Extra patrols into a district.
    Surge {
        /// The district.
        district: District,
    },
}

/// One deployment decision.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CommanderOrder {
    /// When.
    pub at: SimTime,
    /// What.
    pub kind: OrderKind,
    /// Where the units are sent (the call's position, or the district's latest call), m.
    pub pos: [f32; 2],
    /// Units committed by this order.
    pub units: u32,
}

/// One reckless-driver call as the commander logged it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CallIntel {
    /// The reported driver.
    pub racer: EntityId,
    /// When.
    pub at: SimTime,
    /// Where, m.
    pub pos: [f32; 2],
    /// District of the call.
    pub district: District,
    /// A traffic camera flagged it (otherwise a civilian called).
    pub camera: bool,
}

/// Everything the commander decided, for the renderer, testbed and captain screen
/// ([`PoliceSystem::commander`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommanderView {
    /// Budget caps are in force (enabled, and no race profile governs).
    pub active: bool,
    /// A race's police profile governs (the budget shows the profile's limits).
    pub race: bool,
    /// Time of day the budget was worked out for.
    pub phase: DayPhase,
    /// Highest wanted level (0..=5).
    pub level: u8,
    /// Highest bounty counted.
    pub bounty: f32,
    /// What it can afford.
    pub budget: CommanderBudget,
    /// What is out now (`call_units`: extra units on calls; `surge_units`: surge patrols).
    pub in_use: CommanderBudget,
    /// Calls inside the window, oldest first.
    pub calls: Vec<CallIntel>,
    /// Districts with a surge and its seconds left.
    pub surges: Vec<(District, f32)>,
    /// Latest decisions, oldest first.
    pub orders: Vec<CommanderOrder>,
}

/// Commander state inside [`PoliceSystem`].
#[derive(Debug, Clone)]
pub struct Commander {
    pub(crate) budget: CommanderBudget,
    pub(crate) level: u8,
    pub(crate) bounty: f32,
    pub(crate) calls: Vec<CallIntel>,
    /// Surge per district (`District::ALL` order): until when.
    pub(crate) surge_until: [Option<SimTime>; 3],
    pub(crate) orders: Vec<CommanderOrder>,
}

impl Commander {
    /// Before the first think: no caps (the tables' own limits apply).
    pub(crate) fn new() -> Self {
        Self {
            budget: CommanderBudget {
                ground_units: u32::MAX,
                roadblocks: u32::MAX,
                spikes: u32::MAX,
                helicopters: u32::MAX,
                call_units: 0,
                surge_units: 0,
            },
            level: 0,
            bounty: 0.0,
            calls: Vec::new(),
            surge_until: [None; 3],
            orders: Vec::new(),
        }
    }
}

fn district_index(d: District) -> usize {
    match d {
        District::Core => 0,
        District::Suburb => 1,
        District::Industrial => 2,
    }
}

fn district_words(d: District) -> &'static str {
    match d {
        District::Core => "downtown",
        District::Suburb => "the suburbs",
        District::Industrial => "the industrial district",
    }
}

impl PoliceSystem {
    /// The commander's budget caps apply: enabled, and no race profile governs.
    pub(crate) fn commander_active(&self) -> bool {
        self.cfg.commander.enabled && self.race.is_none()
    }

    /// What the commander can afford now (see [`crate::commander`]).
    pub fn commander_budget(&self) -> CommanderBudget {
        self.commander.budget
    }

    /// The commander's decisions and budget (see [`crate::commander`]).
    pub fn commander(&self) -> CommanderView {
        let c = &self.commander;
        let mut surges = Vec::new();
        for d in District::ALL {
            if let Some(t) = c.surge_until[district_index(d)] {
                if self.now < t {
                    surges.push((d, t.since(self.now)));
                }
            }
        }
        CommanderView {
            active: self.commander_active(),
            race: self.race.is_some(),
            phase: self.phase,
            level: c.level,
            bounty: c.bounty,
            budget: c.budget,
            in_use: CommanderBudget {
                ground_units: self.ground_units_alive(),
                roadblocks: self.roadblocks.iter().filter(|r| !r.scripted).count() as u32,
                spikes: (self.spikes.iter().filter(|s| !s.scripted).count() + self.spike_runs.len())
                    as u32,
                helicopters: self
                    .units
                    .iter()
                    .filter(|u| u.kind == UnitKind::Helicopter && !u.wrecked)
                    .count() as u32
                    + self.helo_requests.len() as u32,
                call_units: self
                    .incidents
                    .iter()
                    .map(|inc| self.extra_on_call(inc.id, inc.unit))
                    .sum(),
                surge_units: District::ALL
                    .iter()
                    .map(|d| {
                        self.pool_count(*d)
                            .saturating_sub(self.cfg.presence.patrols.get(*d).get(self.phase))
                    })
                    .sum(),
            },
            calls: c.calls.clone(),
            surges,
            orders: c.orders.clone(),
        }
    }

    /// Log a reckless-driver report about `racer` at `pos` (called by dispatch for every
    /// report no unit witnessed). Reports closer than `calls.min_gap_s` to the driver's last
    /// one are the same call.
    pub(crate) fn commander_note_call(
        &mut self,
        racer: EntityId,
        pos: [f32; 2],
        district: District,
        camera: bool,
    ) {
        let now = self.now;
        let gap = self.cfg.commander.calls.min_gap_s;
        if self
            .commander
            .calls
            .iter()
            .any(|c| c.racer == racer && now.since(c.at) < gap)
        {
            return;
        }
        self.commander.calls.push(CallIntel {
            racer,
            at: now,
            pos,
            district,
            camera,
        });
    }

    /// Extra patrols the commander wants in `d` now (0 without a surge).
    pub(crate) fn commander_surge(&self, d: District) -> u32 {
        if !self.commander_active() {
            return 0;
        }
        match self.commander.surge_until[district_index(d)] {
            Some(t) if self.now < t => self.commander.budget.surge_units,
            _ => 0,
        }
    }

    /// Units on incident `id` besides its own dispatched unit `primary`.
    fn extra_on_call(&self, id: crate::events::IncidentId, primary: Option<crate::UnitId>) -> u32 {
        self.units
            .iter()
            .filter(|u| {
                Some(u.id) != primary && u.assignment == Assignment::Respond { incident: id }
            })
            .count() as u32
    }

    /// Work out the budget, then act on the calls (think rate, before the pool and dispatch).
    pub(crate) fn commander_think(&mut self, world: &dyn WorldQuery) {
        let now = self.now;
        let window = self.cfg.commander.calls.window_s;
        self.commander.calls.retain(|c| now.since(c.at) <= window);
        self.commander_budget_update();
        if !self.commander_active() {
            return;
        }
        self.commander_surges();
        self.commander_reinforce_calls(world);
    }

    fn commander_budget_update(&mut self) {
        let level = self
            .racers
            .values()
            .map(|r| r.level.number())
            .max()
            .unwrap_or(0);
        let calls = &self.commander.calls;
        let bounty = self
            .racers
            .values()
            .filter(|r| r.level > WantedLevel::None || calls.iter().any(|c| c.racer == r.entity))
            .map(|r| r.bounty.bounty(&self.cfg.heat))
            .fold(0.0_f32, f32::max);
        let cc = &self.cfg.commander;
        let (phase, li) = (self.phase, level as usize);
        let budget = if self.commander_active() {
            CommanderBudget {
                ground_units: cc.ground_units.amount(phase, li, bounty),
                roadblocks: cc.roadblocks.amount(phase, li, bounty),
                spikes: cc.spikes.amount(phase, li, bounty),
                helicopters: cc.helicopters.amount(phase, li, bounty),
                call_units: cc.calls.extra_units.amount(phase, li, bounty),
                surge_units: cc.calls.surge_units.amount(phase, li, bounty),
            }
        } else {
            // Off, or a race profile governs: the tables' own limits.
            CommanderBudget {
                ground_units: self.cfg.presence.max_ground_units,
                roadblocks: self.cfg.tools.roadblock.max_active,
                spikes: self.cfg.tools.spikes.max_active,
                helicopters: self.cfg.presence.helicopter.count.get(phase),
                call_units: 0,
                surge_units: 0,
            }
        };
        let c = &mut self.commander;
        c.level = level;
        c.bounty = bounty;
        c.budget = budget;
    }

    /// Districts with `surge_calls` calls inside the window get extra patrols (the pool
    /// spawns them out of sight, [`Self::commander_surge`]).
    fn commander_surges(&mut self) {
        let cc = self.cfg.commander.calls;
        if self.commander.budget.surge_units == 0 || cc.surge_calls == 0 {
            return;
        }
        for d in District::ALL {
            let calls: Vec<&CallIntel> = self
                .commander
                .calls
                .iter()
                .filter(|c| c.district == d)
                .collect();
            if (calls.len() as u32) < cc.surge_calls {
                continue;
            }
            let last = calls.last().expect("surge_calls > 0");
            let (pos, until) = (last.pos, last.at.plus(cc.surge_hold_s));
            let k = district_index(d);
            let fresh = self.commander.surge_until[k].is_none_or(|t| self.now >= t);
            self.commander.surge_until[k] = Some(until);
            if fresh {
                let units = self.commander.budget.surge_units;
                self.commander_order(OrderKind::Surge { district: d }, pos, units);
                self.radio(RadioCue::new(
                    RadioTier::Routine,
                    None,
                    format!(
                        "Command: string of reckless driving calls in {}, putting {} more {} on patrol there.",
                        district_words(d),
                        units,
                        if units == 1 { "unit" } else { "units" }
                    ),
                ));
            }
        }
    }

    /// Open reckless-driver calls on a driver called in `escalate_calls` times get up to
    /// `call_units` more responders: idle units within the district's response radius first,
    /// else units spawned out of sight ahead of the driver, while the ground budget allows.
    fn commander_reinforce_calls(&mut self, world: &dyn WorldQuery) {
        let cc = self.cfg.commander.calls;
        let want = self.commander.budget.call_units;
        if want == 0 || cc.escalate_calls == 0 {
            return;
        }
        for k in 0..self.incidents.len() {
            let inc = self.incidents[k];
            let IncidentKind::Report { racer } = inc.kind else {
                continue;
            };
            // Dispatch sends its unit first; the commander adds to a call already worked.
            if inc.unit.is_none() || self.racers.get(&racer).is_none_or(|r| r.pursued()) {
                continue;
            }
            let calls = self
                .commander
                .calls
                .iter()
                .filter(|c| c.racer == racer)
                .count() as u32;
            if calls < cc.escalate_calls {
                continue;
            }
            let have = self.extra_on_call(inc.id, inc.unit);
            if have >= want {
                continue;
            }
            let radius = self.cfg.presence.response.radius_m.get(inc.district);
            let mut sent = 0;
            for _ in have..want {
                let unit = match self.nearest_free_near(inc.pos, radius) {
                    Some(i) => Some(i),
                    None if self.ground_units_alive() < self.unit_cap() => {
                        self.rebuild_obstacles();
                        self.response_spawn_point(world, racer)
                            .map(|(pos, heading)| {
                                let i = self.spawn_unit(
                                    UnitKind::Car,
                                    UnitRole::Responder,
                                    pos,
                                    heading,
                                );
                                self.record_spawn(world, i);
                                i
                            })
                    }
                    None => None,
                };
                let Some(i) = unit else {
                    break;
                };
                self.assign(i, Assignment::Respond { incident: inc.id });
                sent += 1;
            }
            if sent > 0 {
                self.commander_order(OrderKind::ReinforceCall { racer }, inc.pos, sent);
                let place = self.place_phrase(inc.pos, [0.0, 0.0]);
                self.radio(RadioCue::new(
                    RadioTier::Investigation,
                    None,
                    format!(
                        "Command: more calls on that reckless driver, {place}. Sending {} more {} to cut him off.",
                        sent,
                        if sent == 1 { "unit" } else { "units" }
                    ),
                ));
            }
        }
    }

    /// The idle pool / responder / reinforcement unit nearest `p` within `max_m` (straight
    /// line), not booking anyone.
    fn nearest_free_near(&self, p: [f32; 2], max_m: f32) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for (i, u) in self.units.iter().enumerate() {
            if !u.available()
                || u.player.is_some()
                || self.booking(u.id)
                || !matches!(
                    u.role,
                    UnitRole::Pool(_) | UnitRole::Reinforcement | UnitRole::Responder
                )
            {
                continue;
            }
            let d = dist(u.body.pos, p);
            if d <= max_m && best.is_none_or(|b| d < b.0) {
                best = Some((d, i));
            }
        }
        best.map(|b| b.1)
    }

    fn commander_order(&mut self, kind: OrderKind, pos: [f32; 2], units: u32) {
        let o = &mut self.commander.orders;
        o.push(CommanderOrder {
            at: self.now,
            kind,
            pos,
            units,
        });
        if o.len() > ORDER_LOG {
            o.remove(0);
        }
        self.stats.commander_orders += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoliceConfig;

    #[test]
    fn day_affords_the_most() {
        let cc = PoliceConfig::builtin().commander;
        for a in [
            cc.ground_units,
            cc.roadblocks,
            cc.spikes,
            cc.helicopters,
            cc.calls.extra_units,
            cc.calls.surge_units,
        ] {
            for level in 0..=5 {
                let day = a.amount(DayPhase::Day, level, 0.0);
                for p in [DayPhase::Dawn, DayPhase::Dusk, DayPhase::Night] {
                    assert!(a.amount(p, level, 0.0) <= day, "{p:?} L{level}: {a:?}");
                }
            }
        }
        assert!(
            cc.ground_units.amount(DayPhase::Night, 1, 0.0)
                < cc.ground_units.amount(DayPhase::Day, 1, 0.0)
        );
    }

    #[test]
    fn wanted_level_and_bounty_raise_the_budget() {
        let a = PoliceConfig::builtin().commander.ground_units;
        let night = |l, b| a.amount(DayPhase::Night, l, b);
        assert!(night(3, 0.0) > night(1, 0.0));
        assert!(night(1, 5000.0) > night(1, 0.0));
        // The bounty part is capped.
        assert_eq!(
            night(1, 1.0e9) - night(1, 0.0),
            a.bounty_max,
            "bounty part capped at bounty_max"
        );
    }

    #[test]
    fn night_air_support_and_roadblocks_need_a_bounty() {
        let cc = PoliceConfig::builtin().commander;
        assert_eq!(cc.helicopters.amount(DayPhase::Night, 5, 0.0), 0);
        assert!(cc.helicopters.amount(DayPhase::Night, 5, 3000.0) >= 1);
        assert_eq!(cc.roadblocks.amount(DayPhase::Night, 3, 0.0), 0);
        assert!(cc.roadblocks.amount(DayPhase::Night, 3, 2000.0) >= 1);
        assert!(cc.helicopters.amount(DayPhase::Day, 5, 0.0) >= 1);
    }
}
