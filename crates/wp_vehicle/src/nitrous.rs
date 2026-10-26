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
    /// Fire only at least this far behind the target, m.
    pub fire_gap_m: f32,
    /// Gap at which the scale reaches `far`, m.
    pub gap_full_m: f32,
    /// Never fire further behind than this (not racing that target any more), m.
    pub max_gap_m: f32,
    /// Throttle the AI must be giving, 0..=1.
    pub min_throttle: f32,
    /// Largest |steer| the AI may be giving (no nitrous mid-corner).
    pub max_steer: f32,
    /// Slowest speed to fire at, m/s.
    pub min_speed_mps: f32,
    /// Torque gain multiplier.
    pub strength: Band,
    /// Refill speed multiplier.
    pub refill: Band,
}

impl AiNitrous {
    /// The bottle scale for an AI `gap_m` behind its target (negative = ahead of it).
    pub fn scale(&self, gap_m: f32) -> NitrousScale {
        let t = gap_m / self.gap_full_m.max(1.0);
        NitrousScale {
            strength: self.strength.at(t),
            refill: self.refill.at(t),
        }
    }

    /// Whether an AI `gap_m` behind its target, driving `input` at `speed_mps`, burns now.
    pub fn wants_fire(&self, gap_m: f32, input: &VehicleInput, speed_mps: f32) -> bool {
        gap_m >= self.fire_gap_m
            && gap_m <= self.max_gap_m
            && input.throttle >= self.min_throttle
            && input.steer.abs() <= self.max_steer
            && input.brake <= 0.0
            && input.handbrake <= 0.0
            && speed_mps >= self.min_speed_mps
    }
}

/// `data/tune/nitrous.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NitrousTune {
    /// The bottle.
    pub bottle: BottleDef,
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
        Ok(t)
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
}

impl NitrousScale {
    /// The unscaled bottle.
    pub const PLAYER: Self = Self {
        strength: 1.0,
        refill: 1.0,
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
}

impl Default for Nitrous {
    fn default() -> Self {
        Self::full()
    }
}

impl Nitrous {
    /// Number of floats in [`Nitrous::to_array`].
    pub const LEN: usize = 6;

    /// A full, armed bottle at the player's scale.
    pub fn full() -> Self {
        Self {
            fill: 1.0,
            burning: false,
            armed: true,
            since_burn_s: f32::MAX,
            scale: NitrousScale::PLAYER,
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
        self.burning =
            request && can_burn && self.armed && self.fill > 0.0 && throttle >= def.min_throttle;
        if self.burning {
            self.fill = (self.fill - dt / def.burn_s).max(0.0);
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
            },
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
            let ahead = ai.scale(-50.0);
            let level = ai.scale(0.0);
            let mid = ai.scale(ai.gap_full_m * 0.5);
            let far = ai.scale(ai.gap_full_m * 3.0);
            assert_eq!(ahead, level);
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
    fn ai_fires_only_when_trailing_on_a_straight() {
        let ai = tune().racer;
        let flat_out = VehicleInput {
            throttle: 1.0,
            ..Default::default()
        };
        assert!(ai.wants_fire(ai.fire_gap_m + 1.0, &flat_out, 30.0));
        assert!(!ai.wants_fire(0.0, &flat_out, 30.0), "level");
        assert!(!ai.wants_fire(-20.0, &flat_out, 30.0), "ahead");
        assert!(
            !ai.wants_fire(ai.max_gap_m + 1.0, &flat_out, 30.0),
            "out of range"
        );
        assert!(!ai.wants_fire(50.0, &flat_out, 3.0), "too slow");
        let cornering = VehicleInput {
            steer: 0.6,
            ..flat_out
        };
        assert!(!ai.wants_fire(50.0, &cornering, 30.0));
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
            },
        };
        assert_eq!(Nitrous::from_array(n.to_array()), n);
    }
}
