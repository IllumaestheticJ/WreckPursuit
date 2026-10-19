//! Captain interface: the commands a police captain (human or AI) can issue, their validation
//! against budgets, cooldowns and unlocks, and the [`CaptainView`] the captain client shows.
//!
//! This is the entire server-side model `wp_net` (prompt 07) replicates to the captain client.
//! Commands travel inside the opaque `ClientToServer::CaptainCommand(Vec<u8>)`; encode them with
//! [`CaptainCommand::to_bytes`]. If no human captain is seated the dispatcher acts as captain
//! through the same validated paths.
//!
//! Player chase units receive [`UnitOrder`]s as **suggestions** ([`PoliceSystem::unit_order`]);
//! the captain never takes control of a player's car.

use serde::{Deserialize, Serialize};
use wp_contracts::{DayPhase, EntityId};

use crate::dispatch::Incident;
use crate::events::{IncidentId, PursuitOutcome, ToolId, UnitId, UnitKind};
use crate::graph::{District, SegmentId};
use crate::heat::{PursuitStatus, WantedLevel};
use crate::tools::SiteError;
use crate::unit::{Assignment, Tactic, UnitRole};
use crate::PoliceSystem;

/// An order for one unit (forced on AI units for `captain_order_hold_s`, a suggestion for
/// player units).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum UnitOrder {
    /// Resume patrol.
    Patrol,
    /// Pursue a racer.
    Pursue {
        /// Racer.
        target: EntityId,
    },
    /// Intercept a racer via the road graph.
    Intercept {
        /// Racer.
        target: EntityId,
    },
    /// Respond to an incident.
    Respond {
        /// Incident.
        incident: IncidentId,
    },
    /// Pull over and stay out of the chase.
    Disengage,
}

/// Captain command.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CaptainCommand {
    /// Give a unit an order.
    AssignUnit {
        /// Unit.
        unit: UnitId,
        /// Order.
        order: UnitOrder,
    },
    /// Roadblock ahead of a racer (on a specific segment, or the best site).
    RequestRoadblock {
        /// Racer.
        target: EntityId,
        /// Segment, or `None` for automatic placement.
        segment: Option<SegmentId>,
    },
    /// Spike strip ahead of a racer.
    RequestSpikes {
        /// Racer.
        target: EntityId,
        /// Segment, or `None` for automatic placement.
        segment: Option<SegmentId>,
    },
    /// Helicopter on a racer.
    RequestHelicopter {
        /// Racer.
        target: EntityId,
    },
    /// Racer whose reinforcements are dispatched first (`None` clears).
    SetPriorityTarget {
        /// Racer.
        target: Option<EntityId>,
    },
    /// Call off the pursuit of a racer.
    CallOff {
        /// Racer.
        target: EntityId,
    },
}

/// Why a command was refused.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CaptainError {
    /// No such unit.
    UnknownUnit,
    /// No such racer.
    UnknownTarget,
    /// No such incident.
    UnknownIncident,
    /// The racer is not being pursued.
    NotPursued,
    /// The tool unlocks at a higher wanted level.
    Locked {
        /// Required wanted level (1..=5).
        need_level: u8,
    },
    /// Tool still cooling down.
    Cooldown {
        /// Seconds left.
        remaining_s: f32,
    },
    /// Too many active tools / helicopters.
    Budget,
    /// Placement rejected.
    Site(SiteError),
    /// The helicopter is unavailable (night availability roll).
    Unavailable,
    /// The unit cannot take orders (wrecked, parked at a tool, helicopter).
    UnitUnavailable,
    /// Command bytes did not decode.
    Malformed,
}

impl CaptainCommand {
    /// Encode for `ClientToServer::CaptainCommand`.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("CaptainCommand always serializes")
    }

    /// Decode from `ClientToServer::CaptainCommand`.
    pub fn from_bytes(b: &[u8]) -> Result<Self, CaptainError> {
        serde_json::from_slice(b).map_err(|_| CaptainError::Malformed)
    }
}

/// One unit in the captain view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitView {
    /// Unit.
    pub unit: UnitId,
    /// Car / helicopter.
    pub kind: UnitKind,
    /// Player-driven.
    pub player_driven: bool,
    /// Position, m.
    pub pos: [f32; 2],
    /// Heading.
    pub heading: [f32; 2],
    /// Speed, m/s.
    pub speed_mps: f32,
    /// Health 0..=1.
    pub health: f32,
    /// Remaining risk budget.
    pub risk: f32,
    /// Assignment name.
    pub assignment: String,
    /// Racer the unit is on.
    pub target: Option<EntityId>,
    /// Tactic name when pursuing.
    pub tactic: Option<String>,
    /// Suggested order (player units).
    pub suggestion: Option<UnitOrder>,
}

/// One racer as the police know it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RacerSighting {
    /// Racer.
    pub racer: EntityId,
    /// Wanted level.
    pub level: WantedLevel,
    /// Heat.
    pub heat: f32,
    /// Pursuit status.
    pub status: PursuitStatus,
    /// Last known position, m (`None` if never seen).
    pub last_known: Option<[f32; 2]>,
    /// Seconds since that position was current.
    pub staleness_s: Option<f32>,
    /// Bounty.
    pub bounty: f32,
    /// Ground units on it.
    pub units_assigned: u32,
}

/// Tool readiness.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ToolReadiness {
    /// Seconds until a roadblock may be placed.
    pub roadblock_s: f32,
    /// Seconds until spikes may be placed.
    pub spikes_s: f32,
    /// Seconds until a helicopter may be requested.
    pub helo_s: f32,
    /// Active roadblocks / max.
    pub roadblocks: [u32; 2],
    /// Active spike strips / max.
    pub spikes: [u32; 2],
}

/// Placed tool summary.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ToolView {
    /// Tool.
    pub tool: ToolId,
    /// Roadblock (true) or spikes (false).
    pub roadblock: bool,
    /// Position, m.
    pub pos: [f32; 2],
    /// Seconds left.
    pub remaining_s: f32,
}

/// Everything the captain screen needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptainView {
    /// Sim time, s.
    pub time_s: f64,
    /// Day phase.
    pub phase: DayPhase,
    /// A human captain is seated.
    pub human_captain: bool,
    /// Priority target.
    pub priority_target: Option<EntityId>,
    /// Units.
    pub units: Vec<UnitView>,
    /// Incidents.
    pub incidents: Vec<Incident>,
    /// Heat map: total racer heat per district (by last known position) plus 5 per open incident.
    pub heat_by_district: Vec<(District, f32)>,
    /// Racers.
    pub racers: Vec<RacerSighting>,
    /// Tools in place.
    pub tools: Vec<ToolView>,
    /// Tool readiness.
    pub readiness: ToolReadiness,
}

fn tactic_name(t: Tactic) -> &'static str {
    match t {
        Tactic::Shadow => "shadow",
        Tactic::BoxIn(_) => "box-in",
        Tactic::Pit => "pit",
        Tactic::RollingBlock => "rolling-block",
    }
}

impl PoliceSystem {
    /// Seat or unseat a human captain. Without one, the dispatcher places tools and requests
    /// the helicopter itself.
    pub fn set_human_captain(&mut self, seated: bool) {
        self.captain.human = seated;
    }

    /// Validate and execute a captain command.
    pub fn captain_command(&mut self, cmd: CaptainCommand) -> Result<(), CaptainError> {
        match cmd {
            CaptainCommand::AssignUnit { unit, order } => self.order_unit(unit, order),
            CaptainCommand::RequestRoadblock { target, segment } => {
                self.place_roadblock(target, segment).map(|_| ())
            }
            CaptainCommand::RequestSpikes { target, segment } => {
                self.place_spikes(target, segment).map(|_| ())
            }
            CaptainCommand::RequestHelicopter { target } => {
                let r = self
                    .racers
                    .get(&target)
                    .ok_or(CaptainError::UnknownTarget)?;
                if !r.pursued() {
                    return Err(CaptainError::NotPursued);
                }
                self.request_helo(target)
            }
            CaptainCommand::SetPriorityTarget { target } => {
                if let Some(t) = target {
                    if !self.racers.contains_key(&t) {
                        return Err(CaptainError::UnknownTarget);
                    }
                }
                self.captain.priority = target;
                self.radio(
                    None,
                    match target {
                        Some(t) => format!("Captain: all units, priority is racer #{}.", t.0),
                        None => "Captain: priority cleared.".to_string(),
                    },
                );
                Ok(())
            }
            CaptainCommand::CallOff { target } => {
                let r = self
                    .racers
                    .get(&target)
                    .ok_or(CaptainError::UnknownTarget)?;
                if !r.pursued() {
                    return Err(CaptainError::NotPursued);
                }
                self.end_pursuit(target, PursuitOutcome::CalledOff);
                Ok(())
            }
        }
    }

    fn order_unit(&mut self, unit: UnitId, order: UnitOrder) -> Result<(), CaptainError> {
        let i = self.unit_index(unit).ok_or(CaptainError::UnknownUnit)?;
        let u = &self.units[i];
        if u.wrecked
            || u.kind != UnitKind::Car
            || matches!(
                u.assignment,
                Assignment::RoadblockAt { .. } | Assignment::SpikeAt { .. }
            )
        {
            return Err(CaptainError::UnitUnavailable);
        }
        let a = match order {
            UnitOrder::Patrol => match u.role {
                UnitRole::Pool(_) | UnitRole::Player => Assignment::Idle,
                _ => Assignment::OutOfService,
            },
            UnitOrder::Pursue { target } | UnitOrder::Intercept { target } => {
                let r = self
                    .racers
                    .get(&target)
                    .ok_or(CaptainError::UnknownTarget)?;
                if !r.pursued() {
                    return Err(CaptainError::NotPursued);
                }
                let point = r.last_known.unwrap_or(u.body.pos);
                if matches!(order, UnitOrder::Pursue { .. }) {
                    Assignment::Pursue { target }
                } else {
                    Assignment::Intercept { target, point }
                }
            }
            UnitOrder::Respond { incident } => {
                let k = self
                    .incidents
                    .iter()
                    .position(|x| x.id == incident)
                    .ok_or(CaptainError::UnknownIncident)?;
                self.incidents[k].unit = Some(unit);
                Assignment::Respond { incident }
            }
            UnitOrder::Disengage => Assignment::OutOfService,
        };
        let hold = self.cfg.tools.captain_order_hold_s as f64;
        self.units[i].captain_hold_until_s = self.time_s + hold;
        self.assign(i, a);
        Ok(())
    }

    /// The order a player chase unit should follow (derived from its assignment).
    pub fn unit_order(&self, unit: UnitId) -> Option<UnitOrder> {
        let u = &self.units[self.unit_index(unit)?];
        Some(match u.assignment {
            Assignment::Pursue { target } => UnitOrder::Pursue { target },
            Assignment::Intercept { target, .. } => UnitOrder::Intercept { target },
            Assignment::Respond { incident } => UnitOrder::Respond { incident },
            Assignment::OutOfService => UnitOrder::Disengage,
            _ => UnitOrder::Patrol,
        })
    }

    /// Build the captain view.
    pub fn captain_view(&self) -> CaptainView {
        let now = self.time_s;
        let units = self
            .units
            .iter()
            .map(|u| UnitView {
                unit: u.id,
                kind: u.kind,
                player_driven: u.player.is_some(),
                pos: u.body.pos,
                heading: u.body.heading,
                speed_mps: u.body.speed,
                health: u.health,
                risk: u.risk,
                assignment: u.assignment.name().to_string(),
                target: u.assignment.target(),
                tactic: matches!(u.assignment, Assignment::Pursue { .. })
                    .then(|| tactic_name(u.tactic).to_string()),
                suggestion: u.player.and_then(|_| self.unit_order(u.id)),
            })
            .collect();
        let mut heat = [0.0f32; 3];
        let idx = |d: District| District::ALL.iter().position(|x| *x == d).unwrap_or(0);
        for r in self.racers.values() {
            if let Some(p) = r.last_known {
                heat[idx(self.graph.district_at(p))] += r.heat;
            }
        }
        for inc in &self.incidents {
            heat[idx(inc.district)] += 5.0;
        }
        let racers = self
            .racers
            .values()
            .map(|r| RacerSighting {
                racer: r.entity,
                level: r.level,
                heat: r.heat,
                status: r.status,
                last_known: r.last_known,
                staleness_s: r.last_seen_s.map(|t| (now - t) as f32),
                bounty: r.bounty.bounty(&self.cfg.heat),
                units_assigned: self.units_on(r.entity),
            })
            .collect();
        let mut tools: Vec<ToolView> = self
            .roadblocks
            .iter()
            .map(|r| ToolView {
                tool: r.id,
                roadblock: true,
                pos: r.site.pos,
                remaining_s: (r.expires_s - now) as f32,
            })
            .collect();
        tools.extend(self.spikes.iter().map(|s| ToolView {
            tool: s.id,
            roadblock: false,
            pos: s.site.pos,
            remaining_s: (s.expires_s - now) as f32,
        }));
        let left = |t: f64| ((t - now) as f32).max(0.0);
        CaptainView {
            time_s: now,
            phase: self.phase,
            human_captain: self.captain.human,
            priority_target: self.captain.priority,
            units,
            incidents: self.incidents.clone(),
            heat_by_district: District::ALL.iter().map(|d| (*d, heat[idx(*d)])).collect(),
            racers,
            tools,
            readiness: ToolReadiness {
                roadblock_s: left(self.captain.roadblock_ready_s),
                spikes_s: left(self.captain.spikes_ready_s),
                helo_s: left(self.captain.helo_ready_s),
                roadblocks: [
                    self.roadblocks.len() as u32,
                    self.cfg.tools.roadblock.max_active,
                ],
                spikes: [self.spikes.len() as u32, self.cfg.tools.spikes.max_active],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_bytes_roundtrip() {
        let c = CaptainCommand::RequestSpikes {
            target: EntityId(3),
            segment: Some(SegmentId(9)),
        };
        assert_eq!(CaptainCommand::from_bytes(&c.to_bytes()), Ok(c));
        assert_eq!(
            CaptainCommand::from_bytes(b"nope"),
            Err(CaptainError::Malformed)
        );
    }
}
