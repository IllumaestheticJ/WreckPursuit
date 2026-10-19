//! System registry: which subsystems [`crate::Sim::step_fixed`] runs. A harness hook so that
//! scenarios can exclude a crate that is still a stub (`simcli --systems vehicle,world`).

use std::fmt;

/// One steppable simulation system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SystemId {
    /// `wp_vehicle` dynamics.
    Vehicle,
    /// `wp_damage` lattice + handling effects.
    Damage,
    /// `wp_world` clock, signals and traffic.
    World,
    /// `wp_police` heat, dispatch and pursuit.
    Police,
}

impl SystemId {
    /// Every system, in step order.
    pub const ALL: [SystemId; 4] = [
        SystemId::World,
        SystemId::Vehicle,
        SystemId::Damage,
        SystemId::Police,
    ];

    /// Lower-case name used on command lines and in scenario files.
    pub fn name(self) -> &'static str {
        match self {
            SystemId::Vehicle => "vehicle",
            SystemId::Damage => "damage",
            SystemId::World => "world",
            SystemId::Police => "police",
        }
    }

    /// Parse a lower-case system name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// Set of enabled systems. Rapier always steps (it only holds the ground until vehicles become
/// bodies), so it is not listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Systems {
    /// Step vehicle dynamics.
    pub vehicle: bool,
    /// Step damage and push handling effects to vehicles.
    pub damage: bool,
    /// Step the world clock, signals and traffic.
    pub world: bool,
    /// Step police.
    pub police: bool,
}

impl Systems {
    /// Everything on (the default for games and the testbed).
    pub const ALL: Self = Self {
        vehicle: true,
        damage: true,
        world: true,
        police: true,
    };

    /// Everything off.
    pub const NONE: Self = Self {
        vehicle: false,
        damage: false,
        world: false,
        police: false,
    };

    /// Whether `id` is enabled.
    pub fn contains(&self, id: SystemId) -> bool {
        match id {
            SystemId::Vehicle => self.vehicle,
            SystemId::Damage => self.damage,
            SystemId::World => self.world,
            SystemId::Police => self.police,
        }
    }

    /// Enable or disable `id`.
    pub fn set(&mut self, id: SystemId, on: bool) {
        match id {
            SystemId::Vehicle => self.vehicle = on,
            SystemId::Damage => self.damage = on,
            SystemId::World => self.world = on,
            SystemId::Police => self.police = on,
        }
    }

    /// Parse a comma-separated list such as `"vehicle,damage,world"`. `"all"` = [`Systems::ALL`],
    /// `""`/`"none"` = [`Systems::NONE`]. Unknown names are an error.
    pub fn parse(list: &str) -> Result<Self, String> {
        let list = list.trim();
        if list == "all" {
            return Ok(Self::ALL);
        }
        let mut out = Self::NONE;
        for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            if name == "none" {
                continue;
            }
            let id = SystemId::from_name(name).ok_or_else(|| {
                format!("unknown system {name:?} (expected vehicle, damage, world, police)")
            })?;
            out.set(id, true);
        }
        Ok(out)
    }
}

impl Default for Systems {
    fn default() -> Self {
        Self::ALL
    }
}

impl fmt::Display for Systems {
    /// Comma-separated enabled systems in step order (`"none"` when empty).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = SystemId::ALL
            .into_iter()
            .filter(|s| self.contains(*s))
            .map(SystemId::name)
            .collect();
        if names.is_empty() {
            f.write_str("none")
        } else {
            f.write_str(&names.join(","))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display_round_trip() {
        let s = Systems::parse("damage, vehicle").unwrap();
        assert!(s.vehicle && s.damage && !s.world && !s.police);
        assert_eq!(s.to_string(), "vehicle,damage");
        assert_eq!(
            Systems::parse(&Systems::ALL.to_string()).unwrap(),
            Systems::ALL
        );
        assert_eq!(Systems::parse("none").unwrap().to_string(), "none");
        assert!(Systems::parse("vehicle,lasers").is_err());
    }
}
