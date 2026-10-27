//! Nitrous: one bottle per car that burns for extra engine torque and refills by itself.
//!
//! The driver asks for it with tool [`NITROUS_TOOL`] in `VehicleInput::tool_use` (the player's
//! nitrous key; AI get it set by `wp_sim`). While it burns, the engine makes
//! [`BottleDef::torque_gain`] × [`NitrousScale::strength`] more torque. AI racers and police
//! get a stronger, faster-refilling bottle the further they trail their target
//! ([`AiNitrous::scale`]): the owner's rubberbanding (2026-10-26). All values live in
//! `data/tune/nitrous.toml`.

use std::sync::OnceLock;

use serde::Deserialize;
use wp_contracts::VehicleInput;

/// `VehicleInput::tool_use` value that asks for nitrous.
pub const NITROUS_TOOL: u8 = 1;

/// The nitrous tables embedded at build time from `data/tune/nitrous.toml`.
pub const NITROUS_TOML: &str = include_str!("../../../data/tune/nitrous.toml");

/// The bottle every car carries (the player's, unscaled).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BottleDef {
    /// Seconds of burn from a full bottle.
    pub burn_s: f32,
    /// Seconds to refill an empty bottle.
    pub refill_s: f32,
    /// Seconds after the last burn before the bottle starts refilling.
    pub refill_delay_s: f32,
    /// Extra engine torque fraction while burning (0.4 = +40%).
    pub torque_gain: f32,
    /// Throttle needed to burn, 0..=1.
    pub min_throttle: f32,
    /// An emptied bottle fires again once refilled to this fraction.
    pub rearm_fill: f32,
}

/// A value that runs from `near` (gap 0 or ahead) to `far` (the full gap or more).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    /// Value level with the target or ahead of it.
    pub near: f32,
    /// Value at [`AiNitrous::gap_full_m`] behind or more.
    pub far: f32,
}

impl Band {
    /// Linear blend, `t` clamped to 0..=1.
    pub fn at(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        self.near + (self.far - self.near) * t
    }
}

/// How one kind of AI (racers or police) uses nitrous.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiNitrous {
    /// From this far behind the target the bottle is rubberbanded (stronger, faster refill);
    /// closer, level or ahead it is the player's ([`NitrousScale::PLAYER`]), m.
    pub fire_gap_m: f32,
    /// Gap at which the scale reaches `far`, m.
    pub gap_full_m: f32,
    /// Never fire further from the target than this, ahead or behind (not racing it any
    /// more), m.
    pub max_gap_m: f32,
    /// Also fire level with or ahead of the target while it is at most this far behind, at
    /// the player's scale (racers defending a lead, owner 2026-10-26), m. 0 = only from
    /// `fire_gap_m` behind. A leader further ahead never burns, so it cannot run away.
    pub ahead_gap_m: f32,
    /// On a call (a target, or the siren on) fire below this speed to get up to speed
    /// (police: owner 2026-10-26), m/s. 0 = off.
    pub up_to_speed_mps: f32,
    /// Throttle the AI must be giving, 0..=1.
    pub min_throttle: f32,
    /// Largest |steer| the AI may be giving (no nitrous mid-corner).
    pub max_steer: f32,
    /// Slowest speed to fire at, m/s.
    pub min_speed_mps: f32,
    /// Torque gain multiplier, `near` at `fire_gap_m` behind.
    pub strength: Band,
    /// Refill speed multiplier, `near` at `fire_gap_m` behind.
    pub refill: Band,
}

impl AiNitrous {
    /// The bottle scale for an AI `gap_m` behind its target (negative = ahead of it): the
    /// player's bottle closer than `fire_gap_m`, then `near` .. `far` by `gap_full_m`.
    pub fn scale(&self, gap_m: f32) -> NitrousScale {
        if gap_m < self.fire_gap_m {
            return NitrousScale::PLAYER;
        }
        let t = (gap_m - self.fire_gap_m) / (self.gap_full_m - self.fire_gap_m).max(1.0);
        NitrousScale {
            strength: self.strength.at(t),
            refill: self.refill.at(t),
            capacity: 1.0,
        }
    }

    /// Whether an AI burns now: `gap_m` behind its target (`None` = no target), `on_call`
    /// (a target or the siren on), driving `input` at `speed_mps`.
    pub fn wants_fire(
        &self,
        gap_m: Option<f32>,
        on_call: bool,
        input: &VehicleInput,
        speed_mps: f32,
    ) -> bool {
        let driving = input.throttle >= self.min_throttle
            && input.steer.abs() <= self.max_steer
            && input.brake <= 0.0
            && input.handbrake <= 0.0
            && speed_mps >= self.min_speed_mps;
        let racing = gap_m.is_some_and(|g| {
            (self.fire_gap_m..=self.max_gap_m).contains(&g)
                || (self.ahead_gap_m > 0.0 && g < self.fire_gap_m && -g <= self.ahead_gap_m)
        });
        let launching = on_call && speed_mps < self.up_to_speed_mps;
        driving && (racing || launching)
    }
}

/// One Nitrous Kit tier's bottle multipliers.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KitTier {
    /// Torque gain multiplier.
    pub strength: f32,
    /// Burn time multiplier (bottle size).
    pub capacity: f32,
}

/// The garage's Nitrous Kit tiers (`wp_economy::ToolLoadout::nitrous`).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KitTiers {
    /// `elec_nitrous_1`.
    pub tier1: KitTier,
    /// `elec_nitrous_2`.
    pub tier2: KitTier,
}

/// `data/tune/nitrous.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NitrousTune {
    /// The bottle.
    pub bottle: BottleDef,
    /// Nitrous Kit tiers (player bottle upgrades).
    pub kit: KitTiers,
    /// AI street racers.
    pub racer: AiNitrous,
    /// AI police.
    pub police: AiNitrous,
}

impl NitrousTune {
    /// Parse and check a table: positive times, AI never weaker than the player, and the
    /// gaps in order.
    pub fn parse(text: &str) -> Result<Self, String> {
        let t: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        let b = &t.bottle;
        if !(b.burn_s > 0.0 && b.refill_s > 0.0 && b.refill_delay_s >= 0.0) {
            return Err("bottle burn_s / refill_s must be > 0, refill_delay_s >= 0".into());
        }
        if !(b.torque_gain >= 0.0 && (0.0..=1.0).contains(&b.rearm_fill)) {
            return Err("bottle torque_gain must be >= 0 and rearm_fill in 0..=1".into());
        }
        for (name, ai) in [("racer", &t.racer), ("police", &t.police)] {
            if !(0.0 <= ai.fire_gap_m
                && ai.fire_gap_m < ai.gap_full_m
                && ai.gap_full_m <= ai.max_gap_m)
            {
                return Err(format!(
                    "{name}: need 0 <= fire_gap_m < gap_full_m <= max_gap_m"
                ));
            }
            for (what, band) in [("strength", ai.strength), ("refill", ai.refill)] {
                if !(1.0 <= band.near && band.near <= band.far) {
                    return Err(format!("{name}.{what}: need 1 <= near <= far"));
                }
            }
        }
        let (k1, k2) = (t.kit.tier1, t.kit.tier2);
        if !(1.0 <= k1.strength
            && k1.strength <= k2.strength
            && 1.0 <= k1.capacity
            && k1.capacity <= k2.capacity)
        {
            return Err("kit: need 1 <= tier1 <= tier2 (strength and capacity)".into());
        }
        Ok(t)
    }

    /// The player's bottle scale with Nitrous Kit `tier` fitted (tier 1 and 0 = the base
    /// bottle; above 2 counts as 2). Without a kit a player's car has no bottle at all
    /// ([`Nitrous::fitted`]).
    pub fn kit_scale(&self, tier: u8) -> NitrousScale {
        let k = match tier {
            0 => return NitrousScale::PLAYER,
            1 => self.kit.tier1,
            _ => self.kit.tier2,
        };
        NitrousScale {
            strength: k.strength,
            refill: 1.0,
            capacity: k.capacity,
        }
    }
}

/// The embedded table, parsed once.
///
/// # Panics
/// If `data/tune/nitrous.toml` is invalid (a unit test guards it).
pub fn tune() -> &'static NitrousTune {
    static TUNE: OnceLock<NitrousTune> = OnceLock::new();
    TUNE.get_or_init(|| NitrousTune::parse(NITROUS_TOML).expect("data/tune/nitrous.toml"))
}

/// Multipliers on a car's bottle (the AI rubberbanding). [`NitrousScale::PLAYER`] = as is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NitrousScale {
    /// Torque gain multiplier.
    pub strength: f32,
    /// Refill speed multiplier.
    pub refill: f32,
    /// Bottle size (burn time) multiplier: the Nitrous Kit.
    pub capacity: f32,
}

impl NitrousScale {
    /// The unscaled (stock) bottle.
    pub const PLAYER: Self = Self {
        strength: 1.0,
        refill: 1.0,
        capacity: 1.0,
    };
}

impl Default for NitrousScale {
    fn default() -> Self {
        Self::PLAYER
    }
}

/// Runtime bottle state of one car (in snapshots).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Nitrous {
    /// Bottle level, 0 (empty) ..= 1 (full).
    pub fill: f32,
    /// Burning this step.
    pub burning: bool,
    /// May fire (false from empty until refilled to [`BottleDef::rearm_fill`]).
    pub armed: bool,
    /// Seconds since the last burn (capped at the refill delay).
    pub since_burn_s: f32,
    /// Current scale (set by `wp_sim` every tick for AI).
    pub scale: NitrousScale,
    /// A bottle is fitted. AI always carry one; a player's car only with the garage's Nitrous
    /// Kit (owner decision 2026-10-26, set by `wp_sim`).
    pub fitted: bool,
}

impl Default for Nitrous {
    fn default() -> Self {
        Self::full()
    }
}

impl Nitrous {
    /// Number of floats in [`Nitrous::to_array`].
    pub const LEN: usize = 8;

    /// A full, armed, fitted bottle at the player's scale.
    pub fn full() -> Self {
        Self {
            fill: 1.0,
            burning: false,
            armed: true,
            since_burn_s: f32::MAX,
            scale: NitrousScale::PLAYER,
            fitted: true,
        }
    }

    /// Advance one step of `dt`. `request` = the key is held, `can_burn` = engine running in
    /// a forward gear. Returns the engine torque multiplier (1 = not burning).
    pub fn step(
        &mut self,
        def: &BottleDef,
        request: bool,
        throttle: f32,
        can_burn: bool,
        dt: f32,
    ) -> f32 {
        if !self.armed && self.fill >= def.rearm_fill {
            self.armed = true;
        }
        self.burning = self.fitted
            && request
            && can_burn
            && self.armed
            && self.fill > 0.0
            && throttle >= def.min_throttle;
        if self.burning {
            self.fill = (self.fill - dt / (def.burn_s * self.scale.capacity)).max(0.0);
            if self.fill <= 0.0 {
                self.armed = false;
            }
            self.since_burn_s = 0.0;
            1.0 + def.torque_gain * self.scale.strength
        } else {
            self.since_burn_s = (self.since_burn_s + dt).min(def.refill_delay_s);
            if self.since_burn_s >= def.refill_delay_s {
                self.fill = (self.fill + dt * self.scale.refill / def.refill_s).min(1.0);
            }
            1.0
        }
    }

    /// Snapshot encoding.
    pub fn to_array(&self) -> [f32; Self::LEN] {
        [
            self.fill,
            f32::from(u8::from(self.burning)),
            f32::from(u8::from(self.armed)),
            self.since_burn_s,
            self.scale.strength,
            self.scale.refill,
            self.scale.capacity,
            f32::from(u8::from(self.fitted)),
        ]
    }

    /// Inverse of [`Nitrous::to_array`].
    pub fn from_array(a: [f32; Self::LEN]) -> Self {
        Self {
            fill: a[0],
            burning: a[1] != 0.0,
            armed: a[2] != 0.0,
            since_burn_s: a[3],
            scale: NitrousScale {
                strength: a[4],
                refill: a[5],
                capacity: a[6],
            },
            fitted: a[7] != 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 240.0;

    fn run(n: &mut Nitrous, request: bool, secs: f32) -> f32 {
        let mut gain = 1.0;
        for _ in 0..(secs / DT).round() as usize {
            gain = n.step(&tune().bottle, request, 1.0, true, DT);
        }
        gain
    }

    #[test]
    fn embedded_table_parses() {
        let t = tune();
        assert!(t.bottle.torque_gain > 0.0);
        assert!(t.police.strength.far >= t.racer.strength.near);
    }

    #[test]
    fn burns_down_and_refills_after_the_delay() {
        let b = tune().bottle;
        let mut n = Nitrous::full();
        let gain = run(&mut n, true, b.burn_s * 0.5);
        assert!((gain - (1.0 + b.torque_gain)).abs() < 1e-6);
        assert!((n.fill - 0.5).abs() < 0.01, "{}", n.fill);
        let half = n.fill;
        run(&mut n, false, b.refill_delay_s * 0.9);
        assert_eq!(n.fill, half, "no refill inside the delay");
        run(&mut n, false, b.refill_delay_s * 0.1 + b.refill_s * 0.25);
        assert!((n.fill - (half + 0.25)).abs() < 0.01, "{}", n.fill);
    }

    #[test]
    fn empty_bottle_waits_for_rearm() {
        let b = tune().bottle;
        let mut n = Nitrous::full();
        run(&mut n, true, b.burn_s + 0.1);
        assert_eq!(n.fill, 0.0);
        assert!(!n.armed);
        // Key still held: refill only starts after the delay once the burn stops, and the key
        // held through the refill fires again only at the rearm level.
        let gain = run(&mut n, true, b.refill_delay_s + 0.5);
        assert_eq!(gain, 1.0);
        assert!(n.fill > 0.0 && n.fill < b.rearm_fill);
        run(&mut n, true, b.refill_s * b.rearm_fill);
        assert!(n.burning, "re-armed at {}", n.fill);
    }

    #[test]
    fn no_burn_off_throttle_or_out_of_gear() {
        let b = tune().bottle;
        let mut n = Nitrous::full();
        assert_eq!(n.step(&b, true, 0.2, true, DT), 1.0);
        assert_eq!(n.step(&b, true, 1.0, false, DT), 1.0);
        assert_eq!(n.fill, 1.0);
    }

    #[test]
    fn ai_scale_grows_with_the_gap_and_never_drops_below_the_player() {
        for ai in [tune().racer, tune().police] {
            // Ahead, level or close: the player's bottle (normal refill).
            assert_eq!(ai.scale(-50.0), NitrousScale::PLAYER);
            assert_eq!(ai.scale(0.0), NitrousScale::PLAYER);
            let level = ai.scale(ai.fire_gap_m);
            let mid = ai.scale(ai.gap_full_m * 0.5);
            let far = ai.scale(ai.gap_full_m * 3.0);
            assert!(level.strength >= 1.0 && level.refill >= 1.0);
            assert!(mid.strength > level.strength && mid.refill > level.refill);
            assert!(far.strength > mid.strength && far.refill > mid.refill);
            assert_eq!(far, ai.scale(ai.gap_full_m));
        }
    }

    #[test]
    fn faster_refill_for_a_trailing_ai() {
        let b = tune().bottle;
        let mut player = Nitrous::full();
        let mut ai = Nitrous {
            scale: tune().racer.scale(tune().racer.gap_full_m),
            ..Nitrous::full()
        };
        for n in [&mut player, &mut ai] {
            run(n, true, b.burn_s + 0.1);
            run(n, false, b.refill_delay_s + 5.0);
        }
        assert!(
            ai.fill > 2.0 * player.fill,
            "{} vs {}",
            ai.fill,
            player.fill
        );
    }

    #[test]
    fn racers_fire_on_a_straight_ahead_or_behind() {
        let ai = tune().racer;
        let flat_out = VehicleInput {
            throttle: 1.0,
            ..Default::default()
        };
        let fire = |g: Option<f32>, i: &VehicleInput, v: f32| ai.wants_fire(g, g.is_some(), i, v);
        assert!(fire(Some(ai.fire_gap_m + 1.0), &flat_out, 30.0), "behind");
        assert!(fire(Some(0.0), &flat_out, 30.0), "level");
        assert!(
            fire(Some(-20.0), &flat_out, 30.0),
            "ahead (owner 2026-10-26)"
        );
        assert!(!fire(None, &flat_out, 30.0), "no player near");
        assert!(
            !fire(Some(ai.max_gap_m + 1.0), &flat_out, 30.0),
            "out of range"
        );
        assert!(
            !fire(Some(-ai.ahead_gap_m - 1.0), &flat_out, 30.0),
            "a clear leader"
        );
        assert!(!fire(Some(50.0), &flat_out, 3.0), "too slow");
        let cornering = VehicleInput {
            steer: 0.6,
            ..flat_out
        };
        assert!(!fire(Some(50.0), &cornering, 30.0));
    }

    #[test]
    fn police_fire_to_get_up_to_speed_and_to_catch_up() {
        let ai = tune().police;
        let flat_out = VehicleInput {
            throttle: 1.0,
            ..Default::default()
        };
        let slow = ai.min_speed_mps + 1.0;
        assert!(slow < ai.up_to_speed_mps);
        assert!(
            ai.wants_fire(None, true, &flat_out, slow),
            "siren on, pulling away"
        );
        assert!(
            ai.wants_fire(Some(-30.0), true, &flat_out, slow),
            "intercepting, slow"
        );
        assert!(!ai.wants_fire(None, false, &flat_out, slow), "patrolling");
        let fast = ai.up_to_speed_mps + 5.0;
        assert!(
            !ai.wants_fire(Some(0.0), true, &flat_out, fast),
            "up to speed, level"
        );
        assert!(
            ai.wants_fire(Some(ai.fire_gap_m + 1.0), true, &flat_out, fast),
            "behind"
        );
    }

    #[test]
    fn kit_tiers_make_a_bigger_stronger_bottle() {
        let t = tune();
        assert_eq!(t.kit_scale(0), NitrousScale::PLAYER);
        let (k1, k2) = (t.kit_scale(1), t.kit_scale(2));
        assert!(k1.strength >= 1.0 && k2.strength > k1.strength && k2.capacity > k1.capacity);
        assert_eq!(t.kit_scale(5), k2);
        let b = t.bottle;
        let mut stock = Nitrous::full();
        let mut kit = Nitrous {
            scale: k2,
            ..Nitrous::full()
        };
        let gain = run(&mut kit, true, b.burn_s);
        run(&mut stock, true, b.burn_s);
        assert_eq!(stock.fill, 0.0);
        assert!((kit.fill - (1.0 - 1.0 / k2.capacity)).abs() < 0.01);
        assert!((gain - (1.0 + b.torque_gain * k2.strength)).abs() < 1e-6);
    }

    #[test]
    fn no_bottle_never_burns() {
        let b = tune().bottle;
        let mut n = Nitrous {
            fitted: false,
            ..Nitrous::full()
        };
        assert_eq!(n.step(&b, true, 1.0, true, DT), 1.0);
        assert!(!n.burning);
        assert_eq!(n.fill, 1.0);
    }

    #[test]
    fn rejects_an_ai_weaker_than_the_player() {
        let bad = NITROUS_TOML.replace("near = 1.10", "near = 0.90");
        assert!(NitrousTune::parse(&bad).is_err());
    }

    #[test]
    fn array_round_trip() {
        let n = Nitrous {
            fill: 0.3,
            burning: true,
            armed: false,
            since_burn_s: 0.5,
            scale: NitrousScale {
                strength: 1.2,
                refill: 2.0,
                capacity: 1.0,
            },
            fitted: false,
        };
        assert_eq!(Nitrous::from_array(n.to_array()), n);
    }
}
