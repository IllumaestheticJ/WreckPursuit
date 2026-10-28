//! The career tow service (lead, owner request 2026-10-28: "tow broken cars to repair shops").
//!
//! - **In a run**: when the career car cannot be driven (totaled, engine dead, a wheel off,
//!   flipped or stuck: [`TowWatch`]) the testbed offers "Call a tow truck"; the player can also
//!   ask at any time (pause menu, or the tow key). Picking a destination ends the run and the
//!   car arrives there with its damage (`career_tow`).
//! - **In the garage**: a car that cannot be driven and is not at a repair shop (a wreck left
//!   on the road, a dead car at home) gets "Tow to …". Everything else stays drive-to.
//!
//! Destinations ([`destinations`]): the nearest repair shop, any repair shop, or one of the
//! player's unlocked garages, each with its road distance and price (`wp_economy::tow`). The
//! nearest shop is **on account**: it tows even when the player cannot pay (the shortfall is
//! debt). The car then sits there (`CareerProfile::car_spots`) and the player is "at" that
//! place in the drive-to sense, so its repairs and workshop open.
//!
//! Police: a car in the pound is not towable (the impound flow), and a player who is wanted or
//! chased is refused ([`POLICE_REFUSAL`]) unless the car is totaled; a totaled car keeps the
//! capture rules (`crate::run`): if the police take it, it is in the pound and not towable.
//!
//! Distances are straight lines between fixed points of the combined world (the garage screen
//! has no world loaded: shop and garage positions from [`crate::garage_visit`]) × the data's
//! road factor. Everything here is outside the deterministic sim step: it only reads the sim.

use serde::Serialize;
use wp_contracts::HandlingEffects;
use wp_economy::{CarSpot, Economy, Money};

use crate::garage_visit::{home_spot, PlaceKind, HOME_GARAGES, WORLD_SHOPS};
use crate::run::LedgerLine;

/// Why a tow is refused while the police are on the player.
pub const POLICE_REFUSAL: &str = "Not with the police on you.";

/// Why a car in the pound is not towed.
pub const POUND_REFUSAL: &str = "The car is in the pound: release it from the Impound tab.";

/// Slower than this counts as standing still for [`TowWatch`], m/s.
const STILL_SPEED_MPS: f32 = 0.6;

/// Throttle held at a standstill this long means stuck, physics ticks (5 s).
const STUCK_TICKS: u64 = 5 * 240;

/// Chassis up axis below this z (tipped past ~70°) means flipped.
const FLIPPED_UP_Z: f32 = 0.35;

/// A car this close to one of the player's garages is already there (the garage's ring sits on
/// the lane nearest its anchor, a few metres off), m.
const AT_HOME_M: f32 = 25.0;

/// On its side or roof this long means flipped, physics ticks (2 s).
const FLIPPED_TICKS: u64 = 2 * 240;

/// Why the career car cannot be driven on (the in-run "Call a tow truck" prompt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TowNeed {
    /// A total loss.
    Wrecked,
    /// The engine is dead.
    EngineDead,
    /// A wheel came off.
    WheelOff,
    /// On its side or roof.
    Flipped,
    /// Throttle on and going nowhere.
    Stuck,
}

impl TowNeed {
    /// What the damage alone says (`None`: it can be driven).
    pub fn of_damage(total_loss: bool, h: &HandlingEffects) -> Option<Self> {
        if total_loss {
            Some(TowNeed::Wrecked)
        } else if h.engine_dead {
            Some(TowNeed::EngineDead)
        } else if h.wheel_lost.contains(&true) {
            Some(TowNeed::WheelOff)
        } else {
            None
        }
    }
}

/// Watches the career car for flipped / stuck (sampled whenever the testbed asks; only the
/// sim tick is used for time, so a paused game never gets "stuck").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TowWatch {
    still_since: Option<u64>,
    flipped_since: Option<u64>,
}

impl TowWatch {
    /// Sample at `tick`: speed (m/s), the chassis up axis' world z, and the throttle held.
    /// Returns `Flipped` / `Stuck` once the condition has lasted long enough.
    pub fn observe(&mut self, tick: u64, speed: f32, up_z: f32, throttle: f32) -> Option<TowNeed> {
        let still = speed < STILL_SPEED_MPS;
        if still && up_z < FLIPPED_UP_Z {
            let t = *self.flipped_since.get_or_insert(tick);
            if tick >= t + FLIPPED_TICKS {
                return Some(TowNeed::Flipped);
            }
        } else {
            self.flipped_since = None;
        }
        if still && throttle > 0.3 {
            let t = *self.still_since.get_or_insert(tick);
            if tick >= t + STUCK_TICKS {
                return Some(TowNeed::Stuck);
            }
        } else {
            self.still_since = None;
        }
        None
    }
}

/// World z of a body's up axis for rotation `q` (`[x, y, z, w]`).
pub fn up_z(q: [f32; 4]) -> f32 {
    let [x, y, _, _] = q;
    1.0 - 2.0 * (x * x + y * y)
}

/// A place a tow ends at, as the career screen names it (`CareerPlace`: kind, id, name).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TowPlace {
    /// `Garage` (a repair shop) or `Home` (one of the player's garages).
    pub kind: PlaceKind,
    /// Shop name, or home garage id.
    pub id: String,
    /// Display name.
    pub name: String,
}

impl TowPlace {
    /// The place as a car spot (saved with the career).
    pub fn spot(&self) -> CarSpot {
        match self.kind {
            PlaceKind::Home => CarSpot::Home {
                id: self.id.clone(),
            },
            _ => CarSpot::Shop {
                name: self.id.clone(),
            },
        }
    }
}

/// One destination of a tow offer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TowDestination {
    /// Where to.
    pub place: TowPlace,
    /// City it is in (label).
    pub city_label: String,
    /// Road distance, m.
    pub metres: u32,
    /// Price (call-out + per km).
    pub price: Money,
    /// The nearest repair shop: towed even without the cash (debt).
    pub nearest: bool,
    /// The player can take this tow now (cash, or the nearest shop on account).
    pub available: bool,
}

/// What "Call a tow truck" / "Tow to …" offers for one car (`career_tow_offer_json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TowOffer {
    /// Car uid.
    pub car: u32,
    /// Car name.
    pub car_name: String,
    /// Where the car is, `[x, y]` m.
    pub from: [f32; 2],
    /// Why it cannot be driven (`None`: it can; the player asked).
    pub need: Option<TowNeed>,
    /// Why no tow is possible now (police, pound); the destinations are then empty.
    pub refused: Option<String>,
    /// Cash on hand.
    pub cash: Money,
    /// The tow ends the run being driven.
    pub ends_run: bool,
    /// Destinations, nearest repair shop first, then the other shops and the garages by
    /// distance.
    pub destinations: Vec<TowDestination>,
}

/// What a tow did (`career_tow`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TowReceipt {
    /// Car uid.
    pub car: u32,
    /// Where the car (and the player) now are.
    pub place: TowPlace,
    /// Road distance, m.
    pub metres: u32,
    /// Price charged.
    pub price: Money,
    /// Cash after (negative: the shortfall became debt).
    pub cash_after: Money,
    /// The ledger line(s) it posted.
    pub ledger: Vec<LedgerLine>,
    /// The run it ended (a tow called from a run), as `career_end_run` reports it.
    pub run: Option<crate::career::RunClose>,
}

/// Where a spot is, `[x, y]` m (`home` = the career's home garage id for a car with no spot).
pub fn spot_pos(spot: &CarSpot) -> Option<[f32; 2]> {
    match spot {
        CarSpot::Road { pos } => Some(*pos),
        CarSpot::Shop { name } => WORLD_SHOPS.iter().find(|s| s.name == name).map(|s| s.pos),
        CarSpot::Home { id } => home_spot(id).map(|h| h.anchor),
    }
}

/// The spot is a repair shop.
pub fn is_shop(spot: &CarSpot) -> bool {
    matches!(spot, CarSpot::Shop { .. })
}

fn city_label(city: &str) -> String {
    crate::scenario::spawn(city)
        .map_or(city, |s| s.label)
        .to_string()
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Every tow destination from `from`: the repair shops (the nearest first, on account) and the
/// player's unlocked garages `homes` (`(id, name)`), priced by `econ`, except the spot the
/// car is already at (`here`). `cash` decides what the player can take.
pub fn destinations(
    econ: &Economy,
    from: [f32; 2],
    here: Option<&CarSpot>,
    homes: &[(String, String)],
    cash: Money,
) -> Vec<TowDestination> {
    let price_of = |pos: [f32; 2]| {
        let metres = econ.tow_metres(dist(from, pos));
        (metres, econ.tow_price(metres))
    };
    let mut shops: Vec<TowDestination> = WORLD_SHOPS
        .iter()
        .filter(|s| !matches!(here, Some(CarSpot::Shop { name }) if name == s.name))
        .map(|s| {
            let (metres, price) = price_of(s.pos);
            TowDestination {
                place: TowPlace {
                    kind: PlaceKind::Garage,
                    id: s.name.to_string(),
                    name: s.name.to_string(),
                },
                city_label: city_label(s.city),
                metres,
                price,
                nearest: false,
                available: cash >= price,
            }
        })
        .collect();
    // Stable sort by distance (ties keep the table order).
    shops.sort_by_key(|d| d.metres);
    if let Some(first) = shops.first_mut() {
        first.nearest = true;
        first.available = true;
    }
    let mut garages: Vec<TowDestination> = homes
        .iter()
        .filter(|(id, _)| !matches!(here, Some(CarSpot::Home { id: h }) if h == id))
        .filter_map(|(id, name)| {
            let spot = HOME_GARAGES.iter().find(|h| h.id == id)?;
            // Already there (a run starts in the garage's ring): no tow to it.
            if dist(from, spot.anchor) < AT_HOME_M {
                return None;
            }
            let (metres, price) = price_of(spot.anchor);
            Some(TowDestination {
                place: TowPlace {
                    kind: PlaceKind::Home,
                    id: id.clone(),
                    name: name.clone(),
                },
                city_label: city_label(spot.city),
                metres,
                price,
                nearest: false,
                available: cash >= price,
            })
        })
        .collect();
    garages.sort_by_key(|d| d.metres);
    shops.extend(garages);
    shops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn econ() -> Economy {
        crate::garage::with_economy(|e| e.clone())
    }

    #[test]
    fn the_nearest_shop_comes_first_and_is_on_account() {
        let e = econ();
        let homes = vec![
            ("vesper_lockup".to_string(), "Dockside Lock-up".to_string()),
            ("spire_loft".to_string(), "Spire Loft".to_string()),
        ];
        // Next to Gantry Garage, broke.
        let d = destinations(&e, [690.0, -900.0], None, &homes, 0);
        assert_eq!(d[0].place.name, "Gantry Garage");
        assert!(d[0].nearest && d[0].available);
        assert!(d[1..].iter().all(|x| !x.nearest && !x.available));
        assert_eq!(
            d.iter()
                .filter(|x| x.place.kind == PlaceKind::Garage)
                .count(),
            5
        );
        assert_eq!(
            d.iter().filter(|x| x.place.kind == PlaceKind::Home).count(),
            2
        );
        // Prices grow with distance; each is the economy's formula.
        for x in &d {
            assert_eq!(x.price, e.tow_price(x.metres));
        }
        let shops: Vec<_> = d
            .iter()
            .filter(|x| x.place.kind == PlaceKind::Garage)
            .collect();
        assert!(shops.windows(2).all(|w| w[0].metres <= w[1].metres));
        // With cash, everything within reach is available.
        let rich = destinations(&e, [690.0, -900.0], None, &homes, 100_000);
        assert!(rich.iter().all(|x| x.available));
    }

    #[test]
    fn the_spot_the_car_is_at_is_not_a_destination() {
        let e = econ();
        let homes = vec![("vesper_lockup".to_string(), "Dockside Lock-up".to_string())];
        let here = CarSpot::Home {
            id: "vesper_lockup".into(),
        };
        let from = spot_pos(&here).unwrap();
        let d = destinations(&e, from, Some(&here), &homes, 10_000);
        assert!(d.iter().all(|x| x.place.kind == PlaceKind::Garage));
        assert_eq!(d.len(), 5);
        // Standing in the garage's ring (a run starts there): not a destination either.
        let d = destinations(&e, [from[0] + 2.0, from[1]], None, &homes, 10_000);
        assert!(d.iter().all(|x| x.place.kind == PlaceKind::Garage));
    }

    #[test]
    fn flipped_and_stuck_need_time_on_the_sim_clock() {
        let mut w = TowWatch::default();
        // Upright, throttle on, not moving: stuck after 5 s, not before.
        assert_eq!(w.observe(0, 0.0, 1.0, 1.0), None);
        assert_eq!(w.observe(4 * 240, 0.0, 1.0, 1.0), None);
        assert_eq!(w.observe(5 * 240, 0.0, 1.0, 1.0), Some(TowNeed::Stuck));
        // Moving again resets it.
        assert_eq!(w.observe(5 * 240 + 1, 5.0, 1.0, 1.0), None);
        assert_eq!(w.observe(6 * 240, 0.0, 1.0, 1.0), None);
        // Waiting at a light (no throttle) is never stuck.
        let mut w = TowWatch::default();
        assert_eq!(w.observe(0, 0.0, 1.0, 0.0), None);
        assert_eq!(w.observe(60 * 240, 0.0, 1.0, 0.0), None);
        // On the roof for 2 s: flipped.
        let mut w = TowWatch::default();
        assert_eq!(w.observe(100, 0.1, -1.0, 0.0), None);
        assert_eq!(
            w.observe(100 + 2 * 240, 0.1, -1.0, 0.0),
            Some(TowNeed::Flipped)
        );
        // The up axis of a car on its roof (180° about the forward axis).
        assert!(up_z([0.0, 1.0, 0.0, 0.0]) < -0.99);
        assert!((up_z([0.0, 0.0, 0.0, 1.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn damage_that_stops_the_car() {
        let mut h = HandlingEffects::default();
        assert_eq!(TowNeed::of_damage(false, &h), None);
        assert_eq!(TowNeed::of_damage(true, &h), Some(TowNeed::Wrecked));
        h.wheel_lost[2] = true;
        assert_eq!(TowNeed::of_damage(false, &h), Some(TowNeed::WheelOff));
        h.engine_dead = true;
        assert_eq!(TowNeed::of_damage(false, &h), Some(TowNeed::EngineDead));
    }
}
