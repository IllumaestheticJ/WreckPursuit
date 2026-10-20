//! One-click assist presets (GDD §6.1, backlog B10): **Rookie**, **Street** (default) and
//! **Sim**. A preset sets the damage mode and the [`AssistProfile`] together; damage mode and
//! assists stay independent axes in the sim, and the player can change any toggle afterwards.
//!
//! | Preset | Damage mode | Assists |
//! |---|---|---|
//! | Rookie | Arcade | ABS, TCS, ESC, countersteer help, speed-sensitive steering, auto gears |
//! | Street | Arcade | ABS + TCS; ESC off, no countersteer help |
//! | Sim | Simulation | everything off, manual gears |
//!
//! Input shaping that makes a device usable at all is kept from
//! [`AssistProfile::for_source`] instead of being treated as an assist: keyboard pedal filters
//! and speed-sensitive steering (GDD §6.2: "keyboard gets filtered input by default"), and the
//! automatic clutch. Gamepad and wheel get raw input except where a preset turns help on.
//!
//! The GDD asks for "ESC low" in Street; `AssistProfile::esc` is a switch, so Street leaves it
//! off until the contract has an ESC strength (requested in `docs/integration/police.md`).

use wp_contracts::{AssistProfile, DamageMode, InputSource};

/// A one-click assist preset.
/// (No serde derive: `wp_vehicle` has no serde dependency; front ends pass [`AssistPreset::name`].)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum AssistPreset {
    /// New players: Arcade damage, every assist on.
    Rookie,
    /// The default: Arcade damage, ABS + TCS.
    #[default]
    Street,
    /// Simulation damage, assists off.
    Sim,
}

impl AssistPreset {
    /// All presets, in menu order.
    pub const ALL: [AssistPreset; 3] = [
        AssistPreset::Rookie,
        AssistPreset::Street,
        AssistPreset::Sim,
    ];

    /// Display name (`"Rookie"`, `"Street"`, `"Sim"`).
    pub fn name(self) -> &'static str {
        match self {
            AssistPreset::Rookie => "Rookie",
            AssistPreset::Street => "Street",
            AssistPreset::Sim => "Sim",
        }
    }

    /// Parse a display name (case-insensitive).
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(name.trim()))
    }

    /// Damage mode this preset selects.
    pub fn damage_mode(self) -> DamageMode {
        match self {
            AssistPreset::Rookie | AssistPreset::Street => DamageMode::Arcade,
            AssistPreset::Sim => DamageMode::Simulation,
        }
    }

    /// Assists for a driver on `source`.
    pub fn assists(self, source: InputSource) -> AssistProfile {
        let device = AssistProfile::for_source(source);
        let keyboard = source == InputSource::Keyboard;
        // Device shaping kept by every preset (keyboard only; pads and wheels are raw).
        let shaping = |mut p: AssistProfile| {
            if keyboard {
                p.steer_speed_sensitivity = device.steer_speed_sensitivity;
                p.throttle_filter = device.throttle_filter;
                p.brake_filter = device.brake_filter;
            }
            p.auto_clutch = true;
            p
        };
        let off = AssistProfile {
            abs: false,
            tcs: false,
            esc: false,
            steer_speed_sensitivity: 0.0,
            countersteer_help: 0.0,
            throttle_filter: 0.0,
            brake_filter: 0.0,
            auto_gears: false,
            auto_clutch: true,
        };
        match self {
            AssistPreset::Rookie => shaping(AssistProfile {
                abs: true,
                tcs: true,
                esc: true,
                steer_speed_sensitivity: device.steer_speed_sensitivity.max(0.7),
                countersteer_help: device.countersteer_help.max(0.6),
                throttle_filter: device.throttle_filter.max(0.3),
                brake_filter: device.brake_filter.max(0.2),
                auto_gears: true,
                ..off
            }),
            AssistPreset::Street => shaping(AssistProfile {
                abs: true,
                tcs: true,
                auto_gears: device.auto_gears,
                ..off
            }),
            AssistPreset::Sim => shaping(off),
        }
    }

    /// The preset that `assists` + `mode` exactly match for `source`, if any (`None` = the
    /// player customised it).
    pub fn matching(
        assists: &AssistProfile,
        mode: DamageMode,
        source: InputSource,
    ) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.damage_mode() == mode && p.assists(source) == *assists)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCES: [InputSource; 3] = [
        InputSource::Keyboard,
        InputSource::Gamepad,
        InputSource::Wheel,
    ];

    #[test]
    fn presets_follow_the_gdd_table() {
        for s in SOURCES {
            let r = AssistPreset::Rookie.assists(s);
            assert!(
                r.abs && r.tcs && r.esc && r.auto_gears && r.countersteer_help > 0.0,
                "{s:?}"
            );
            let st = AssistPreset::Street.assists(s);
            assert!(
                st.abs && st.tcs && !st.esc && st.countersteer_help == 0.0,
                "{s:?}"
            );
            let sim = AssistPreset::Sim.assists(s);
            assert!(
                !sim.abs && !sim.tcs && !sim.esc && !sim.auto_gears && sim.countersteer_help == 0.0
            );
            assert!(sim.auto_clutch);
        }
        assert_eq!(AssistPreset::Rookie.damage_mode(), DamageMode::Arcade);
        assert_eq!(AssistPreset::Street.damage_mode(), DamageMode::Arcade);
        assert_eq!(AssistPreset::Sim.damage_mode(), DamageMode::Simulation);
        assert_eq!(AssistPreset::default(), AssistPreset::Street);
    }

    #[test]
    fn keyboard_keeps_filtered_input_pads_and_wheels_are_raw_in_sim() {
        let kb = AssistPreset::Sim.assists(InputSource::Keyboard);
        assert!(kb.throttle_filter > 0.0 && kb.steer_speed_sensitivity > 0.0);
        let wheel = AssistPreset::Sim.assists(InputSource::Wheel);
        assert_eq!(
            (wheel.throttle_filter, wheel.steer_speed_sensitivity),
            (0.0, 0.0)
        );
    }

    #[test]
    fn matching_round_trips_and_detects_custom() {
        for s in SOURCES {
            for p in AssistPreset::ALL {
                assert_eq!(
                    AssistPreset::matching(&p.assists(s), p.damage_mode(), s),
                    Some(p)
                );
            }
            let mut custom = AssistPreset::Street.assists(s);
            custom.esc = true;
            assert_eq!(AssistPreset::matching(&custom, DamageMode::Arcade, s), None);
            // Same assists, other damage mode: not a preset.
            let street = AssistPreset::Street.assists(s);
            assert_eq!(
                AssistPreset::matching(&street, DamageMode::Simulation, s),
                None
            );
        }
        assert_eq!(
            AssistPreset::from_name(" rookie "),
            Some(AssistPreset::Rookie)
        );
        assert_eq!(AssistPreset::from_name("hard"), None);
    }
}
