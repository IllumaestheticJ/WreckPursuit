//! Tuning tables loaded from `data/police/*.toml`.
//!
//! Every number that differs between day and night is a [`PhaseTable`]; every number that
//! differs between districts is a [`DistrictTable`]. Code never branches on `DayPhase` itself,
//! it only looks values up here. See the crate README for what each table means.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use wp_contracts::{DamageMode, DayPhase};

use crate::graph::District;

/// One value per [`DayPhase`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseTable<T> {
    /// 05:00–07:00.
    pub dawn: T,
    /// 07:00–18:00.
    pub day: T,
    /// 18:00–20:00.
    pub dusk: T,
    /// 20:00–05:00.
    pub night: T,
}

impl<T: Copy> PhaseTable<T> {
    /// Value for `phase`.
    pub fn get(&self, phase: DayPhase) -> T {
        match phase {
            DayPhase::Dawn => self.dawn,
            DayPhase::Day => self.day,
            DayPhase::Dusk => self.dusk,
            DayPhase::Night => self.night,
        }
    }
}

/// One value per [`District`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistrictTable<T> {
    /// Dense downtown.
    pub core: T,
    /// Residential ring.
    pub suburb: T,
    /// Warehouses, docks, wide roads.
    pub industrial: T,
}

impl<T: Copy> DistrictTable<T> {
    /// Value for `district`.
    pub fn get(&self, district: District) -> T {
        match district {
            District::Core => self.core,
            District::Suburb => self.suburb,
            District::Industrial => self.industrial,
        }
    }
}

/// One value per [`DamageMode`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeValue<T> {
    /// Arcade damage mode.
    pub arcade: T,
    /// Simulation damage mode.
    pub simulation: T,
    /// Extreme damage mode.
    pub extreme: T,
}

impl<T: Copy> ModeValue<T> {
    /// Value for `mode`.
    pub fn get(&self, mode: DamageMode) -> T {
        match mode {
            DamageMode::Arcade => self.arcade,
            DamageMode::Simulation => self.simulation,
            DamageMode::Extreme => self.extreme,
        }
    }
}

/// Inclusive `[min, max]` range.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    /// Lower bound.
    pub min: f32,
    /// Upper bound.
    pub max: f32,
}

// ---------------------------------------------------------------------------------------------
// heat.toml
// ---------------------------------------------------------------------------------------------

/// Heat added per violation (before the witness factor).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViolationWeights {
    /// Heat per speeding event, flat part.
    pub speeding_base: f32,
    /// Heat per speeding event per m/s over the limit.
    pub speeding_per_mps: f32,
    /// Red-light run.
    pub red_light: f32,
    /// Wrong-way driving.
    pub wrong_way: f32,
    /// Collision with a road user / property.
    pub collision: f32,
    /// Reckless driving / near misses.
    pub reckless: f32,
}

/// Heat accumulation, decay and wanted levels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeatConfig {
    /// Heat ceiling.
    pub max_heat: f32,
    /// Heat at which L1..L5 begin (ascending, 5 entries).
    pub level_thresholds: [f32; 5],
    /// Fraction of a violation's heat that counts when no unit or camera saw it ("reports").
    pub unseen_factor: f32,
    /// A violation nobody saw only becomes a call (a unit sent to look) once the racer's heat
    /// is at least this, so a little unseen speeding is not investigated.
    pub call_min_heat: f32,
    /// Same violation kind from the same racer is counted at most once per this many seconds.
    pub repeat_cooldown_s: f32,
    /// Heat per violation kind.
    pub violation: ViolationWeights,
    /// Range within which a unit with line of sight witnesses a violation, m.
    pub witness_range_m: PhaseTable<f32>,
    /// Range within which a unit with line of sight recognises a wanted car and starts a
    /// pursuit, m. Much shorter in the dark.
    pub recognition_range_m: PhaseTable<f32>,
    /// Seconds of continuous recognition-range line of sight needed before a pursuit starts
    /// (a glimpse at night is not enough).
    pub recognition_time_s: PhaseTable<f32>,
    /// Heat decay per second while unseen and not pursued.
    pub decay_per_s: PhaseTable<f32>,
    /// Multiplier on decay by the district the racer is in.
    pub decay_district_scale: DistrictTable<f32>,
    /// Seconds after the last sighting before decay starts.
    pub decay_grace_s: f32,
    /// Heat added per second while actively pursued (long chases escalate).
    pub pursuit_heat_per_s: f32,
    /// Heat added when the racer disables (total-losses) a police unit by contact.
    pub cop_disabled_heat: f32,
    /// Hitting a police car (assaulting an officer).
    pub assault: AssaultConfig,
    /// Blatant speeding right past a unit (owner playtest row 60).
    pub blatant: BlatantConfig,
    /// What a ground unit can see (owner playtest row 89).
    pub sight: SightConfig,
    /// How long recognising a wanted car takes (owner playtest rows 80 / 89).
    pub recognition: RecognitionConfig,
    /// Units clock speeders themselves (owner report 2026-10-21, "noticed but no pursuit").
    pub radar: RadarConfig,
}

/// A ground unit's field of view (owner playtest row 89, "if I can see them, they can see
/// me"): line of sight within the phase's range, in a forward cone (the windscreen and side
/// windows) and all round close by (mirrors, a glance over the shoulder).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SightConfig {
    /// Width of the forward cone, degrees (360 = all round).
    pub fov_deg: f32,
    /// Seen in any direction this close, m.
    pub all_round_m: f32,
}

/// A ground unit's radar (owner report 2026-10-21: "cops see me but no pursuit"). A unit that
/// can see a racer (`sight`, line of sight, within `witness_range_m`) clocks it: over the limit
/// where it drives by more than `max(margin_mps, ratio × limit)` is a witnessed speeding
/// violation, at most once per `repeat_s` of no counted speeding. `wp_world` only reports speeding
/// against a lane pointing the car's way, every 5 s: a car on the wrong side of the road, or one
/// that passed a unit between two reports, was seen but never clocked. Only racers not wanted
/// yet are clocked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadarConfig {
    /// Absolute margin over the limit, m/s (`wp_world`'s speeding margin).
    pub margin_mps: f32,
    /// Relative margin (fraction of the limit).
    pub ratio: f32,
    /// A racer is clocked only when no speeding violation counted for this long (`wp_world`'s
    /// sustained-speeding repeat), s.
    pub repeat_s: f32,
    /// Speeding in a unit's sight this long before it is clocked (`wp_world`'s hold), s.
    pub hold_s: f32,
}

/// Recognising an already-wanted car a unit did not see offend (rows 80 / 89):
/// `recognition_time_s` within `near_m`, rising linearly by `far_extra_s` to the edge of
/// `recognition_range_m`. Losing sight drains the timer at `decay_per_s` instead of resetting
/// it, so a one-frame line-of-sight drop costs nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecognitionConfig {
    /// Recognition takes `recognition_time_s` this close, m.
    pub near_m: f32,
    /// Extra seconds at the edge of the recognition range.
    pub far_extra_s: f32,
    /// Seconds of the recognition timer lost per second out of sight.
    pub decay_per_s: f32,
}

/// Speeding far over the limit right past a unit that can see it is recognised at once, day or
/// night (owner playtest row 60, "I sped right by a cop with no chase": at night a pursuit
/// otherwise needs the racer within `recognition_range_m` (40 m) for `recognition_time_s`
/// (4 s), which a car passing at 150 km/h never is).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlatantConfig {
    /// Speeding at least this far over the limit, m/s.
    pub over_mps: f32,
    /// ...within this distance of an in-service ground unit with line of sight, m.
    pub range_m: f32,
    /// Wanted level (1–5) the racer is raised to at least.
    pub min_level: u8,
}

/// A racer hitting a police unit is assaulting an officer (owner playtest row 24): heat jumps
/// to at least `min_level` and the hit unit (plus units nearby with line of sight) recognises
/// the racer at once, so a pursuit starts immediately, day or night.
///
/// A contact counts when, within one contact episode, the summed normal impulse reaches
/// `min_impulse_ns`, the speed of the two cars relative to each other reaches
/// `min_rel_speed_mps`, and the racer itself was moving at `min_racer_speed_mps` or more.
/// Parking-lot nudges stay below these. Only physics contacts count (the headless units'
/// proximity stand-in never does).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssaultConfig {
    /// Summed normal impulse of one contact episode, N·s.
    pub min_impulse_ns: f32,
    /// Highest speed of the racer relative to the unit in the episode (the larger of the car
    /// velocities' difference and the contact's normal closing speed), m/s. A glancing
    /// sideswipe has little normal closing speed but a large relative speed.
    pub min_rel_speed_mps: f32,
    /// Racer speed at the contact, m/s (a car standing still is the one being hit).
    pub min_racer_speed_mps: f32,
    /// Contacts between the same racer and unit closer together than this are one episode, s.
    pub episode_s: f32,
    /// The same racer–unit pair counts as a new assault at most once per this many seconds.
    pub repeat_cooldown_s: f32,
    /// Heat added per assault.
    pub heat: f32,
    /// Wanted level (1–5) the racer is raised to at least.
    pub min_level: u8,
    /// Other ground units within this range with line of sight also recognise the racer, m.
    pub nearby_units_m: f32,
}

/// Escape (breaking contact) rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscapeConfig {
    /// Seconds without line of sight needed to escape, per wanted level L1..L5.
    pub base_s: [f32; 5],
    /// Multiplier on `base_s` by phase (night is easier).
    pub phase_scale: PhaseTable<f32>,
}

/// Capture rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureConfig {
    /// Racer counts as stopped below this speed, m/s.
    pub stop_speed_mps: f32,
    /// Seconds stopped needed for a capture.
    pub stop_time_s: f32,
    /// A unit must be within this distance, m.
    pub unit_radius_m: f32,
    /// Also captured: boxed in or pinned, never getting farther than this from one spot
    /// (rocking back and forth) for `pinned_time_s` with a unit within `pinned_unit_radius_m`,
    /// m.
    pub pinned_radius_m: f32,
    /// See `pinned_radius_m`, s.
    pub pinned_time_s: f32,
    /// See `pinned_radius_m`: wider than `unit_radius_m`, so a racer stuck in a jam with the
    /// police held up behind other cars 20 m away is captured rather than waited for forever
    /// (Hollins hairpin, lead/opponent-pace), m.
    pub pinned_unit_radius_m: f32,
    /// Row 168: both rules wait at least this long, s, when the only units that count stand
    /// in the racer's lane in front of it (a pursuer, interceptor or patrol, not a roadblock,
    /// spike car or box-in).
    #[serde(default = "default_blocked_ahead_stop_time_s")]
    pub blocked_ahead_stop_time_s: f32,
}

fn default_blocked_ahead_stop_time_s() -> f32 {
    12.0
}

/// Safehouse rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafehouseConfig {
    /// Seconds inside the safehouse needed.
    pub dwell_s: f32,
    /// No unit may be within this distance, m.
    pub clear_radius_m: f32,
    /// Heat is multiplied by this when a safehouse escape succeeds.
    pub heat_scale_on_enter: f32,
}

/// Bounty formula and payouts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BountyConfig {
    /// Bounty per "heat-second" (heat/100 integrated over seconds while wanted).
    pub per_heat_second: f32,
    /// Bounty per race won while wanted.
    pub per_race_won_wanted: f32,
    /// Bounty per police unit disabled.
    pub per_cop_disabled: f32,
    /// Multiplier on race payouts / stakes by phase (night pays more).
    pub payout_scale: PhaseTable<f32>,
    /// Bounty at which a racer gets maximum reinforcement priority.
    pub max_priority_bounty: f32,
}

/// Contents of `data/police/heat.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeatFile {
    /// Heat model.
    pub heat: HeatConfig,
    /// Escape rule.
    pub escape: EscapeConfig,
    /// Capture rule.
    pub capture: CaptureConfig,
    /// Safehouse rule.
    pub safehouse: SafehouseConfig,
    /// Bounty.
    pub bounty: BountyConfig,
}

// ---------------------------------------------------------------------------------------------
// presence.toml
// ---------------------------------------------------------------------------------------------

fn default_search_extra_units() -> [u32; 5] {
    [1, 2, 2, 3, 3]
}

/// Reinforcement dispatch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReinforcementConfig {
    /// Response-time range per phase, s (uniformly sampled).
    pub response_s: PhaseTable<Range>,
    /// Max fractional reduction of response time for a max-priority (high bounty) racer.
    pub priority_max_reduction: f32,
    /// Ground units wanted on a racer per wanted level L1..L5 (the level's cap: idle units
    /// nearby join at once, the rest spawn out of sight after `response_s`).
    pub units_per_level: [u32; 5],
    /// Idle and patrolling units within this legal driving distance of a pursued racer join
    /// the pursuit at once (intercepting its predicted position), per level L1..L5, m.
    pub backup_radius_m: [f32; 5],
    /// From this wanted level, backup comes from every district; below it only from the
    /// racer's own district.
    pub backup_all_districts_level: u8,
    /// Units on a racer farther than `backup_radius_m` + this from it (straight line) don't
    /// count toward `units_per_level`, m.
    pub far_margin_m: f32,
    /// ... and are stood down when still that far this long after joining, s.
    pub far_release_s: f32,
    /// Fallback spawn for a reinforcement when [`ResponseConfig`] finds no hidden spot in its
    /// window: the nearest node at least this far from the racer, out of its sight, m.
    pub spawn_min_distance_m: f32,
    /// Extra units on a racer while it is lost (Searching), on top of `units_per_level`,
    /// per level L1..L5 (owner playtest row 84: "more cops should spawn and go to the site").
    #[serde(default = "default_search_extra_units")]
    pub search_extra_units: [u32; 5],
}

/// Dispatch-driven response to reckless-driving calls (owner playtest 2026-10-20: "spawn them
/// in when there's public safety calls"). A call (a violation no unit saw) is assigned to the
/// nearest available unit within `radius_m`; if there is none, a responder **spawns** out of
/// every racer's sight, `spawn_min_m..spawn_max_m` along the roads, and drives at the racer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseConfig {
    /// Call taken → unit assigned, s, per phase (the dispatcher's part of the response time).
    pub call_delay_s: PhaseTable<f32>,
    /// Added to `call_delay_s` per district (stations are downtown), s.
    pub district_delay_s: DistrictTable<f32>,
    /// An available unit closer than this (straight line) takes the call; otherwise one
    /// spawns, per district, m.
    pub radius_m: DistrictTable<f32>,
    /// Spawn window along the roads (legal driving distance to the racer), m.
    pub spawn_min_m: f32,
    /// See `spawn_min_m`, m.
    pub spawn_max_m: f32,
    /// A fast racer stretches `spawn_max_m` by this many seconds of its travel, so a
    /// responder can still appear ahead of it (out of sight on a long straight), s.
    pub spawn_lead_s: f32,
    /// A unit never appears (or vanishes) closer than this to any racer, straight line, m.
    pub never_closer_m: f32,
    /// Ground units allowed at once by the highest wanted level, L0..L5 (patrols included;
    /// also capped by `max_ground_units`). Responders and reinforcements wait for room.
    pub max_units_by_level: [u32; 6],
    /// Idle responders, reinforcements and surplus patrols despawn once out of every racer's
    /// sight and at least this far away, m.
    pub despawn_min_m: f32,
    /// A call's unit still farther than `redispatch_far_m` from the racer this long after
    /// taking it is released and the call dispatched again (the racer turned away), s.
    pub redispatch_s: f32,
    /// See `redispatch_s`, m.
    pub redispatch_far_m: f32,
}

/// Helicopter availability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeloAvailability {
    /// Helicopters available.
    pub count: PhaseTable<u32>,
    /// Time from request to on-station launch, s.
    pub launch_s: PhaseTable<f32>,
    /// Chance a request finds the helicopter unavailable (rolled per request).
    pub unavailable_chance: PhaseTable<f32>,
    /// Lowest wanted level (1..=5) at which the AI captain requests a helicopter.
    pub min_level: u8,
}

/// Wanted level (1..=5) at which each tool unlocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolUnlocks {
    /// Spike strips.
    pub spikes_level: u8,
    /// Roadblocks.
    pub roadblock_level: u8,
    /// PIT manoeuvre.
    pub pit_level: u8,
    /// Box-in.
    pub boxin_level: u8,
    /// Rolling block.
    pub rolling_block_level: u8,
}

/// Contents of `data/police/presence.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresenceFile {
    /// Patrol unit count per district per phase.
    pub patrols: DistrictTable<PhaseTable<u32>>,
    /// Hard cap on simultaneously existing ground units (patrol + reinforcement + tools).
    pub max_ground_units: u32,
    /// Reinforcements.
    pub reinforcement: ReinforcementConfig,
    /// Response to reckless-driving calls (responder spawning).
    pub response: ResponseConfig,
    /// Helicopter.
    pub helicopter: HeloAvailability,
    /// Tool unlocks by wanted level.
    pub unlocks: ToolUnlocks,
    /// How long a crash scene keeps a unit busy, s.
    pub crash_scene_s: PhaseTable<f32>,
    /// Minimum closing speed of a civilian-civilian contact that creates a crash incident, m/s.
    pub crash_incident_min_speed_mps: f32,
}

// ---------------------------------------------------------------------------------------------
// pursuit.toml
// ---------------------------------------------------------------------------------------------

/// Unit vehicle capabilities for the built-in kinematic driver.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitDynamics {
    /// Max acceleration, m/s².
    pub accel_mps2: f32,
    /// Comfortable braking used for planning, m/s².
    pub brake_mps2: f32,
    /// Absolute tire grip limit, g. Demands above this lose the car.
    pub grip_g: f32,
    /// Off-road distance from the lane centre at which the car hits something, m.
    pub crash_offset_m: f32,
    /// Health lost per (impact m/s)² / 1000 in a self-crash, per mode.
    pub crash_health_per_kmps2: ModeValue<f32>,
    /// Health lost per (contact closing m/s)² / 1000 in a racer contact, per mode.
    pub contact_health_per_kmps2: ModeValue<f32>,
    /// A unit at or below this health is a total loss, per mode.
    pub total_loss_health: ModeValue<f32>,
}

/// Risk budget: spent by dangerous driving; when exhausted the unit only shadows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskConfig {
    /// Budget per unit.
    pub budget: f32,
    /// Lateral acceleration above which time is charged, g.
    pub lateral_g: f32,
    /// Cost per second above `lateral_g`.
    pub cost_per_s_lateral: f32,
    /// Cost per near miss.
    pub cost_near_miss: f32,
    /// Cost per collision.
    pub cost_collision: f32,
    /// Regeneration per second while not pursuing.
    pub regen_per_s: f32,
}

/// Background traffic hazards for the built-in kinematic driver (stand-in for real traffic
/// until prompt 05's agents are physical). Probabilities per km driven.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HazardConfig {
    /// Near misses per km at the speed limit, by phase (traffic density).
    pub near_miss_per_km: PhaseTable<f32>,
    /// Integer exponent on (speed / limit) for the near-miss rate.
    pub speed_exponent: u32,
    /// Chance a near miss becomes a collision at 2× the limit (scales linearly with excess).
    pub collision_chance_at_2x: f32,
    /// Closing speed of a traffic collision as a fraction of own speed (uniformly sampled:
    /// glancing swipe … T-bone).
    pub collision_speed_fraction: Range,
}

/// Tactic parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TacticConfig {
    /// Shadow distance band behind the racer, m.
    pub shadow_m: Range,
    /// Box-in only below this racer speed, km/h.
    pub boxin_max_kmh: f32,
    /// Lateral/longitudinal offset of box-in slots, m.
    pub boxin_offset_m: [f32; 2],
    /// Rolling block sits this far in front of the racer, m.
    pub rolling_block_lead_m: f32,
    /// Rolling block speed as a fraction of the racer's speed.
    pub rolling_block_speed_factor: f32,
    /// Intercept prediction horizon, s.
    pub intercept_horizon_s: Range,
    /// Units further than this behind the racer try to intercept instead of following, m.
    pub intercept_min_gap_m: f32,
    /// Disengage below this health, per mode. Arcade health levels off around 0.2–0.3 under
    /// repeated hits (H, round 8), so Arcade units stay in until they are properly wrecked.
    pub disengage_health: ModeValue<f32>,
    /// Pull over when any |toe delta| exceeds this (bent wheel), rad.
    pub bent_wheel_toe_rad: f32,
    /// Pull over when any tire puncture exceeds this.
    pub puncture_pull_over: f32,
}

/// Pursuit aggression by wanted level (owner playtest row 63: "police are not aggressive
/// enough"). Arrays are indexed by wanted level L1..L5. Same values in both damage modes: the
/// modes differ only in what the contact does to the cars.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChaseConfig {
    /// Lateral acceleration pursuing / intercepting units plan corners with (budget left), g.
    pub cornering_g: f32,
    /// Braking pursuing / intercepting units plan corners and junctions with, m/s².
    pub brake_mps2: f32,
    /// Closing on the racer: up to its speed × (1 + this).
    pub speed_margin: f32,
    /// Shadow (tail) gap band behind the racer per level, m: nearest.
    pub shadow_min_m: [f32; 5],
    /// ... farthest.
    pub shadow_max_m: [f32; 5],
    /// No deliberate contact (PIT, ram, box-in) with a civilian vehicle this close to the
    /// racer, per level, m (more risk tolerance at higher levels).
    pub civilian_clear_m: [f32; 5],
    /// Risk a unit may spend before it only shadows: `risk.budget` × this, per level.
    pub risk_budget_scale: [f32; 5],
    /// Risk cost of a contact the unit meant (PIT, ram, box-in), instead of
    /// `risk.cost_collision`.
    pub tactic_contact_cost: f32,
    /// PIT allowed below this racer speed per level, km/h (0 = no PIT at that level).
    pub pit_max_kmh: [f32; 5],
    /// PIT needs this much straight road ahead of the racer, m.
    pub pit_min_straight_m: f32,
    /// PIT line: this far beside the racer's line while lining up, m.
    pub pit_side_offset_m: f32,
    /// PIT: turn in once the unit's centre is this far behind the racer's, m.
    pub pit_turn_in_gap_m: f32,
    /// Seconds before a unit tries another PIT or ram after a contact (re-engage).
    pub reengage_s: f32,
    /// Rear-quarter rams from this level.
    pub rear_ram_level: u8,
    /// Hard rams and side slams from this level.
    pub hard_ram_level: u8,
    /// Second pursuer drives alongside the racer (flank) from this level.
    pub flank_level: u8,
    /// Flank line: this far beside the racer's line, m.
    pub flank_offset_m: f32,
    /// Closing speed of a ram over the racer's speed: rear quarter, side slam, hard, m/s.
    pub ram_closing_mps: [f32; 3],
    /// Interceptors park across the junction ahead of the racer from this level.
    pub junction_block_level: u8,
}

/// Contents of `data/police/pursuit.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PursuitFile {
    /// AI decision rate, Hz (driving integrates every tick).
    pub think_hz: f32,
    /// Patrol speed as a fraction of the limit.
    pub patrol_speed_factor: f32,
    /// Responding (lights, no pursuit) speed as a fraction of the limit.
    pub respond_speed_factor: f32,
    /// Pursuers may go this fraction faster than the target.
    pub target_speed_margin: f32,
    /// Lateral acceleration the AI plans corners with, g, while patrolling.
    pub patrol_cornering_g: f32,
    /// Lateral acceleration the AI plans corners with, g, while pursuing (budget left).
    pub pursuit_cornering_g: f32,
    /// Lateral acceleration the AI plans corners with, g, when the risk budget is spent.
    pub shadow_cornering_g: f32,
    /// Ground unit line-of-sight range, m.
    pub los_range_m: PhaseTable<f32>,
    /// Unit dynamics.
    pub unit: UnitDynamics,
    /// Risk budget.
    pub risk: RiskConfig,
    /// Traffic hazards.
    pub hazard: HazardConfig,
    /// Tactics.
    pub tactics: TacticConfig,
    /// Pursuit aggression by wanted level.
    pub chase: ChaseConfig,
    /// Obstacle awareness (following, passing, spawn clearance).
    pub avoid: AvoidConfig,
    /// Pro driving of physical units: racing line, grip-limit speed profile, slide control
    /// ([`crate::pro`]). Defaults when the table has no `[pro]` section.
    #[serde(default)]
    pub pro: crate::pro::ProParams,
}

/// Obstacle awareness for ground units (see [`crate::avoid`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AvoidConfig {
    /// Bumper-to-bumper gap kept to a stopped vehicle, m.
    pub standstill_gap_m: f32,
    /// Reaction time added to the stopping distance, s.
    pub reaction_s: f32,
    /// Deceleration the follow speed is planned with, m/s².
    pub decel_mps2: f32,
    /// Extra lateral clearance for the lane corridor, m.
    pub lateral_margin_m: f32,
    /// Anything this close ahead of the bumper (in the unit's own heading) also blocks, m.
    pub bumper_m: f32,
    /// Shortest look-ahead, m.
    pub min_horizon_m: f32,
    /// Longest look-ahead, m.
    pub max_horizon_m: f32,
    /// A vehicle slower than this counts as stopped, m/s.
    pub stopped_mps: f32,
    /// Only a stopped vehicle closer than this can be passed, m.
    pub pass_gap_m: f32,
    /// Wait this long behind a stopped vehicle before passing (no lights), s.
    pub pass_wait_s: f32,
    /// Wait this long with lights and siren, s.
    pub pass_wait_siren_s: f32,
    /// Lane shift to the left for passing, m.
    pub pass_shift_m: f32,
    /// Second try: shift this much further (a vehicle straddling the centreline), m.
    pub pass_extra_shift_m: f32,
    /// Clear road needed beyond the passed vehicle, m.
    pub pass_clear_m: f32,
    /// No oncoming vehicle this much further away, m.
    pub oncoming_clear_m: f32,
    /// Speed cap while passing, m/s.
    pub pass_speed_mps: f32,
    /// While pulling out to pass, creep at this speed past anything still ahead, m/s ...
    pub pass_creep_mps: f32,
    /// ... while it is further than this, m.
    pub pass_creep_gap_m: f32,
    /// Look this far ahead on the track for the next junction, m.
    pub junction_lookahead_m: f32,
    /// Stop line distance before the junction centre, m.
    pub stop_line_back_m: f32,
    /// Speed is down to the junction speed this far before the centre, m.
    pub junction_slow_m: f32,
    /// Patrol speed through an unsignalised junction, m/s.
    pub yield_mps: f32,
    /// Speed through a turn at a junction, m/s.
    pub turn_mps: f32,
    /// With lights: speed through a turn at a junction, m/s.
    pub siren_turn_mps: f32,
    /// With lights: speed through a junction, m/s.
    pub siren_junction_mps: f32,
    /// With lights: speed through a junction against a red, m/s.
    pub siren_red_mps: f32,
    /// Give way: check cross traffic within this distance of the junction centre, m.
    pub yield_radius_m: f32,
    /// ... once the junction is this close, m.
    pub yield_lookahead_m: f32,
    /// ... and wait while it arrives less than this after the unit would, s.
    pub yield_gap_s: f32,
    /// Radius of the junction box (anything inside it blocks), m.
    pub box_radius_m: f32,
    /// Longest wait for cross traffic without lights, s.
    pub yield_max_s: f32,
    /// Longest wait with lights (at a red), s.
    pub yield_max_siren_s: f32,
    /// Without lights, no passing a stopped vehicle this close to a junction, m.
    pub no_pass_near_junction_m: f32,
    /// Stopped behind another police unit this long, the lower id creeps on, s.
    pub gridlock_s: f32,
    /// Pursuing / intercepting units: crossing speed through a junction, m/s.
    pub chase_junction_mps: f32,
    /// ... against a red light, m/s.
    pub chase_red_mps: f32,
    /// ... turning, m/s.
    pub chase_turn_mps: f32,
    /// ... longest wait for cross traffic, s.
    pub chase_yield_max_s: f32,
    /// ... braking planned for a junction, m/s².
    pub chase_decel_mps2: f32,
    /// ... lateral clearance for the lane corridor (negative: squeeze past a car that only
    /// clips the lane edge), m.
    pub chase_lateral_margin_m: f32,
    /// ... wait behind a stopped vehicle before passing it, s.
    pub chase_pass_wait_s: f32,
    /// Push-through (owner playtest row 61): a unit with lights blocked this long by a stopped
    /// civilian or parked car it can't pass shoves it aside, s.
    pub push_after_s: f32,
    /// Push speed (below 15 km/h), m/s.
    pub push_mps: f32,
    /// A push lasts at most this long, s.
    pub push_max_s: f32,
    /// A push that moved the unit less than this is a failure (reverse / re-route next), m.
    pub push_min_progress_m: f32,
    /// No new push for this long after one, s.
    pub push_cooldown_s: f32,
    /// Static feelers (owner playtest row 66): look this far ahead beyond the stopping
    /// distance for walls and buildings, m.
    pub wall_margin_m: f32,
    /// Feeler angle either side of the heading, rad.
    pub wall_feeler_rad: f32,
    /// Braking planned for a wall ahead, m/s².
    pub wall_decel_mps2: f32,
    /// Steer away from a wall on one side by this much, m.
    pub wall_shift_m: f32,
    /// Units spawn at least this far from any vehicle, m.
    pub spawn_clear_m: f32,
    /// ... and not where a moving vehicle will be within this time, s.
    pub spawn_path_s: f32,
    /// Half width of that path, m.
    pub spawn_path_half_width_m: f32,
}

// ---------------------------------------------------------------------------------------------
// tools.toml
// ---------------------------------------------------------------------------------------------

/// Roadblock tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadblockConfig {
    /// Placed at least this far ahead on the predicted path, m.
    pub min_ahead_m: f32,
    /// Placed at most this far ahead, m.
    pub max_ahead_m: f32,
    /// Required straight approach sight distance, m.
    pub min_sight_m: f32,
    /// Parked units per roadblock `[min, max]`.
    pub units: [u32; 2],
    /// Barrier half extents `[x, y, z]`, m.
    pub barrier_half_extents_m: [f32; 3],
    /// Barrier mass, kg.
    pub barrier_mass_kg: f32,
    /// Parked unit mass, kg.
    pub unit_mass_kg: f32,
    /// Lifetime, s.
    pub lifetime_s: f32,
    /// Roadblocks are never placed on highways when the wanted level is above this (1..=5).
    pub highway_max_level: u8,
    /// Cooldown between roadblocks, s.
    pub cooldown_s: PhaseTable<f32>,
    /// Max roadblocks active at once.
    pub max_active: u32,
}

/// Spike strip tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpikeConfig {
    /// Placed at least this far ahead, m.
    pub min_ahead_m: f32,
    /// Placed at most this far ahead, m.
    pub max_ahead_m: f32,
    /// Required straight approach sight distance, m.
    pub min_sight_m: f32,
    /// Strip length across the road, m.
    pub length_m: f32,
    /// Seconds for a punctured tire to go fully flat.
    pub deflate_s: f32,
    /// Lifetime, s.
    pub lifetime_s: f32,
    /// Cooldown between strips, s.
    pub cooldown_s: PhaseTable<f32>,
    /// Max strips active at once.
    pub max_active: u32,
}

/// Helicopter entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeloConfig {
    /// Cruise speed, m/s.
    pub speed_mps: f32,
    /// Altitude, m.
    pub altitude_m: f32,
    /// Horizontal range within which the helicopter keeps visual, m.
    pub los_range_m: PhaseTable<f32>,
    /// Spotlight cone half angle, rad.
    pub spotlight_half_angle_rad: f32,
    /// Max time on a single track before returning to refuel, s.
    pub max_track_s: f32,
    /// While it keeps visual, the helicopter radios the racer's position this often, s.
    pub position_report_s: f32,
    /// Visual counts as lost (radio "lost visual") after this long without line of sight, s.
    pub lost_visual_after_s: f32,
}

/// Racer-side gadgets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GadgetConfig {
    /// Radar detector range per tier (index = tier), m.
    pub radar_range_m: Vec<f32>,
    /// Scanner delay per tier (index = tier), s.
    pub scanner_delay_s: Vec<f32>,
    /// Radio log length kept for scanners and the captain.
    pub radio_log_len: usize,
}

/// Contents of `data/police/tools.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsFile {
    /// Roadblocks.
    pub roadblock: RoadblockConfig,
    /// Spike strips.
    pub spikes: SpikeConfig,
    /// Helicopter.
    pub helicopter: HeloConfig,
    /// Radar / scanner.
    pub gadgets: GadgetConfig,
    /// Captain tool cooldown for helicopter requests, s.
    pub helo_request_cooldown_s: f32,
    /// How long a captain's unit order overrides the AI, s.
    pub captain_order_hold_s: f32,
}

// ---------------------------------------------------------------------------------------------

/// The complete police tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoliceConfig {
    /// `heat.toml`.
    pub heat: HeatFile,
    /// `presence.toml`.
    pub presence: PresenceFile,
    /// `pursuit.toml`.
    pub pursuit: PursuitFile,
    /// `tools.toml`.
    pub tools: ToolsFile,
    /// `radio.toml` (radio pacing; optional in a tables directory, built-in when missing).
    pub radio: crate::radio::RadioConfig,
}

/// Error loading police tables.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// File unreadable.
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    /// TOML malformed or a field missing / unknown.
    #[error("{0}: {1}")]
    Parse(String, toml::de::Error),
    /// Values parse but make no sense.
    #[error("invalid police config: {0}")]
    Invalid(String),
}

const BUILTIN_HEAT: &str = include_str!("../../../data/police/heat.toml");
const BUILTIN_PRESENCE: &str = include_str!("../../../data/police/presence.toml");
const BUILTIN_PURSUIT: &str = include_str!("../../../data/police/pursuit.toml");
const BUILTIN_TOOLS: &str = include_str!("../../../data/police/tools.toml");
const BUILTIN_RADIO: &str = include_str!("../../../data/police/radio.toml");

fn parse_one<T: for<'de> Deserialize<'de>>(name: &str, text: &str) -> Result<T, ConfigError> {
    toml::from_str(text).map_err(|e| ConfigError::Parse(name.to_string(), e))
}

/// The patrol counts shipped before the owner's 2026-10-23 cut (Core 2/8/6/3, Suburb 1/3/2/1,
/// Industrial 1/2/2/1 by dawn/day/dusk/night). For tests and scenarios that check how units
/// behave with several close to the racer, not how many patrol the city.
pub fn dense_patrols() -> DistrictTable<PhaseTable<u32>> {
    let t = |dawn, day, dusk, night| PhaseTable {
        dawn,
        day,
        dusk,
        night,
    };
    DistrictTable {
        core: t(2, 8, 6, 3),
        suburb: t(1, 3, 2, 1),
        industrial: t(1, 2, 2, 1),
    }
}

impl PoliceConfig {
    /// The built-in tables with [`dense_patrols`] in place of the shipped patrol counts.
    pub fn builtin_dense_patrols() -> Self {
        let mut cfg = Self::builtin();
        cfg.presence.patrols = dense_patrols();
        cfg
    }

    /// Parse from the four TOML texts (heat, presence, pursuit, tools), with the built-in
    /// `radio.toml`.
    pub fn parse(
        heat: &str,
        presence: &str,
        pursuit: &str,
        tools: &str,
    ) -> Result<Self, ConfigError> {
        Self::parse_with_radio(heat, presence, pursuit, tools, BUILTIN_RADIO)
    }

    /// Parse from all five TOML texts (heat, presence, pursuit, tools, radio).
    pub fn parse_with_radio(
        heat: &str,
        presence: &str,
        pursuit: &str,
        tools: &str,
        radio: &str,
    ) -> Result<Self, ConfigError> {
        let cfg = Self {
            heat: parse_one("heat.toml", heat)?,
            presence: parse_one("presence.toml", presence)?,
            pursuit: parse_one("pursuit.toml", pursuit)?,
            tools: parse_one("tools.toml", tools)?,
            radio: parse_one("radio.toml", radio)?,
        };
        cfg.validate()?;
        Ok(cfg)
    }

    /// Load from a directory containing `heat.toml`, `presence.toml`, `pursuit.toml`, `tools.toml`
    /// and optionally `radio.toml` (the built-in one when missing).
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let dir = dir.as_ref();
        let read = |name: &str| {
            let p = dir.join(name);
            std::fs::read_to_string(&p).map_err(|e| ConfigError::Io(p.display().to_string(), e))
        };
        let radio = if dir.join("radio.toml").exists() {
            read("radio.toml")?
        } else {
            BUILTIN_RADIO.to_string()
        };
        Self::parse_with_radio(
            &read("heat.toml")?,
            &read("presence.toml")?,
            &read("pursuit.toml")?,
            &read("tools.toml")?,
            &radio,
        )
    }

    /// The tables in `data/police/` as compiled into the crate (used by the browser build and
    /// by [`crate::PoliceSystem::new`]).
    pub fn builtin() -> Self {
        Self::parse(
            BUILTIN_HEAT,
            BUILTIN_PRESENCE,
            BUILTIN_PURSUIT,
            BUILTIN_TOOLS,
        )
        .expect("data/police/*.toml compiled into wp_police must be valid")
    }

    /// Sanity checks that would otherwise show up as odd behaviour.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let bad = |m: &str| Err(ConfigError::Invalid(m.to_string()));
        let r = &self.radio;
        if r.global_gap_s
            .iter()
            .chain(r.unit_gap_s.iter())
            .chain(
                [
                    r.global_gap_calm_s,
                    r.collapse_window_s,
                    r.earshot_m,
                    r.min_gap_s,
                    r.line_pad_s,
                    r.stale_critical_s,
                ]
                .iter(),
            )
            .any(|v| !(v.is_finite() && *v >= 0.0))
            || r.stale_s < r.collapse_window_s
        {
            return bad(
                "radio: gaps, window and earshot must be >= 0 and stale_s >= collapse_window_s",
            );
        }
        let a = &self.pursuit.avoid;
        if a.decel_mps2 <= 0.0 || a.min_horizon_m <= 0.0 || a.max_horizon_m < a.min_horizon_m {
            return bad("pursuit.avoid: decel and horizons must be positive and ordered");
        }
        let t = &self.heat.heat.level_thresholds;
        if t.windows(2).any(|w| w[0] >= w[1]) || t[0] <= 0.0 || t[4] > self.heat.heat.max_heat {
            return bad("heat.level_thresholds must ascend within (0, max_heat]");
        }
        let a = &self.heat.heat.assault;
        if !(1..=5).contains(&a.min_level)
            || [
                a.min_impulse_ns,
                a.min_rel_speed_mps,
                a.min_racer_speed_mps,
                a.episode_s,
                a.repeat_cooldown_s,
                a.heat,
                a.nearby_units_m,
            ]
            .iter()
            .any(|v| !(v.is_finite() && *v >= 0.0))
        {
            return bad("heat.assault: min_level must be 1..=5 and every value >= 0");
        }
        if !(0.0..=1.0).contains(&self.heat.heat.unseen_factor) {
            return bad("heat.unseen_factor must be in 0..=1");
        }
        if self.heat.heat.call_min_heat < 0.0 {
            return bad("heat.call_min_heat must be >= 0");
        }
        if self.pursuit.think_hz <= 0.0 {
            return bad("pursuit.think_hz must be > 0");
        }
        for r in [
            self.presence.reinforcement.response_s.dawn,
            self.presence.reinforcement.response_s.day,
            self.presence.reinforcement.response_s.dusk,
            self.presence.reinforcement.response_s.night,
        ] {
            if r.min > r.max || r.min < 0.0 {
                return bad("reinforcement.response_s ranges must have 0 <= min <= max");
            }
        }
        let rs = &self.presence.response;
        if !(rs.never_closer_m >= 0.0
            && rs.spawn_min_m >= rs.never_closer_m
            && rs.spawn_max_m >= rs.spawn_min_m
            && rs.despawn_min_m >= rs.never_closer_m)
            || rs.max_units_by_level.windows(2).any(|w| w[0] > w[1])
        {
            return bad("response: need never_closer_m <= spawn_min_m <= spawn_max_m, despawn_min_m >= never_closer_m, unit caps ascending");
        }
        if self.tools.roadblock.units[0] == 0
            || self.tools.roadblock.units[0] > self.tools.roadblock.units[1]
        {
            return bad("roadblock.units must be [min>=1, max>=min]");
        }
        let h = &self.tools.helicopter;
        if !(h.position_report_s > 0.0 && h.lost_visual_after_s >= 0.0) {
            return bad("helicopter: position_report_s must be > 0, lost_visual_after_s >= 0");
        }
        if self.tools.gadgets.radar_range_m.is_empty()
            || self.tools.gadgets.scanner_delay_s.is_empty()
        {
            return bad("gadgets tiers must not be empty");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tables_parse_and_validate() {
        let c = PoliceConfig::builtin();
        assert_eq!(c.presence.patrols.core.day, 3);
        assert_eq!(c.presence.patrols.suburb.night, 0);
        // The data dir on disk matches the compiled-in copy.
        let disk =
            PoliceConfig::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/police")).unwrap();
        assert_eq!(c, disk);
    }

    #[test]
    fn pro_table_matches_the_code_defaults() {
        // `[pro]` spells every knob out; keep it and `ProParams::default` in step.
        assert_eq!(
            PoliceConfig::builtin().pursuit.pro,
            crate::pro::ProParams::default()
        );
    }

    #[test]
    fn phase_lookup() {
        let t = PhaseTable {
            dawn: 1,
            day: 2,
            dusk: 3,
            night: 4,
        };
        assert_eq!(t.get(DayPhase::Night), 4);
        assert_eq!(t.get(DayPhase::Day), 2);
    }
}
