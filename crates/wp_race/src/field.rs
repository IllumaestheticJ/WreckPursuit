//! Picker race fields scaled to the player (owner, 2026-10-27: "have the cars scale with both
//! reputation and the car the user is bringing (if im bringing an exotic, so should they) and
//! have the reputation points scale accordingly as well").
//!
//! - **Target.** The field aims at the player's car class index (`wp_economy` performance
//!   index, mods included) moved by the player's rep tier ([`TIER_TARGETS`]): a little below it
//!   for a new name, above it for a feared one, and never below the tier's floor, so a legend
//!   meets fast cars whatever they bring and anyone who brings an exotic meets exotics.
//! - **Spread.** The regulars line up around the target by their starting form rating
//!   (`data/events/racers.toml`): the strongest aims [`SPREAD`] above it, the weakest [`SPREAD`]
//!   below.
//! - **Cars.** Each regular keeps their own car when it is within [`KEEP_WITHIN`] of their aim
//!   (their name and car stay together where they can), else takes the [`FIELD_CARS`] car
//!   nearest their aim that nobody else in the field drives.
//! - **Rep.** [`rep_scale`] multiplies the rep a result earns by how strong the field is: its
//!   mean class index (an exotic field earns more than twice a hatchback one), and more again
//!   when it out-guns the player's car.
//!
//! Pure functions of their inputs (class indices come from the caller, who has the cars
//! loaded), so the browser's picker card, the race and a replay pick the same field.

use wp_economy::progression::RepTier;

use crate::catalog::{AiDriverDef, RaceDef};
use crate::roster::Roster;

/// Cars a scaled field picks from (analog names, `data/cars`): street cars from the beater up
/// to the exotics. No police, trucks or `debug_*` cars, and no Ironside 440 (it weaves into the
/// Loop 9 walls, see `docs/integration/opponent-pace.md`).
pub const FIELD_CARS: [&str; 24] = [
    "junkyard_beater",
    "tansy_13",
    "brigand_400",
    "stoat_16v",
    "marten_20",
    "vireo_xs",
    "bulwark_400",
    "kestrel_ts",
    "shiro_turbo",
    "talus_20t",
    "sperber_32",
    "rampart_50",
    "hakone_gt",
    "lindwurm_4t",
    "arashi_t4",
    "lucciola_350",
    "merlo_20t",
    "kagero_hx",
    "ostwind_6t",
    "morrigan_55",
    "fulmine_v10",
    "halberd_40",
    "ombra_v12",
    "nyx_proto",
];

/// Where a rep tier puts the field: `offset` class-index points from the player's car, never
/// below `floor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierTarget {
    /// Class-index points added to the player's car.
    pub offset: i32,
    /// Lowest field target at this tier.
    pub floor: u16,
}

/// Field target per rep tier, `RepTier::ALL` order (Unknown … Legend). Class indices at
/// 2026-10-27: hatchbacks 170-330, sport coupes 430-570, exotics 620-999 (the fastest three are
/// capped at 999). A Known name never races beaters, a Respected one races sport coupes or
/// better, a Legend races exotics whatever they bring.
pub const TIER_TARGETS: [TierTarget; 6] = [
    TierTarget {
        offset: -10,
        floor: 0,
    },
    TierTarget {
        offset: 0,
        floor: 0,
    },
    TierTarget {
        offset: 10,
        floor: 400,
    },
    TierTarget {
        offset: 25,
        floor: 480,
    },
    TierTarget {
        offset: 40,
        floor: 560,
    },
    TierTarget {
        offset: 60,
        floor: 650,
    },
];

/// The strongest regular aims this far above the target, the weakest this far below.
pub const SPREAD: f32 = 30.0;

/// A regular keeps their own car when its class index is within this of their aim.
pub const KEEP_WITHIN: u16 = 40;

/// Rep scale bounds ([`rep_scale`]).
pub const REP_SCALE_MIN: f32 = 0.75;
/// See [`REP_SCALE_MIN`].
pub const REP_SCALE_MAX: f32 = 2.5;

/// The class index a field aims at for a player of rep tier `tier` in a car of class
/// `player_class`.
pub fn target_class(tier: RepTier, player_class: u16) -> u16 {
    let t = TIER_TARGETS[tier.index()];
    let aim = (player_class as i32 + t.offset).max(t.floor as i32);
    aim.clamp(0, u16::MAX as i32) as u16
}

/// Each regular's aim in `def`'s field (`def.ai` order) around `target`: spread by starting
/// form rating, strongest highest. A field of one aims at the target.
pub fn aims(def: &RaceDef, target: u16) -> Vec<u16> {
    let roster = Roster::embedded();
    let ratings: Vec<i32> = def
        .ai
        .iter()
        .map(|d| roster.get(d.id).map_or(1000, |r| r.rating))
        .collect();
    let (lo, hi) = (
        ratings.iter().copied().min().unwrap_or(1000),
        ratings.iter().copied().max().unwrap_or(1000),
    );
    ratings
        .iter()
        .map(|&r| {
            let k = if hi > lo {
                (r - lo) as f32 / (hi - lo) as f32 * 2.0 - 1.0
            } else {
                0.0
            };
            (target as f32 + k * SPREAD).round().max(0.0) as u16
        })
        .collect()
}

/// The cars of `def`'s field (`def.ai` order) aimed at `target`: each regular's own car when
/// it is close enough to their [`aims`], else the nearest unused [`FIELD_CARS`] car.
/// `class_of` gives a car's class index (`None`: the car is left out of the pool). Regulars
/// pick strongest first, so the best driver gets first call on the car nearest their aim
/// (after the regulars who keep their own).
pub fn field_cars(
    def: &RaceDef,
    target: u16,
    class_of: impl Fn(&str) -> Option<u16>,
) -> Vec<&'static str> {
    let aims = aims(def, target);
    let pool: Vec<(&'static str, u16)> = FIELD_CARS
        .iter()
        .filter_map(|c| class_of(c).map(|k| (*c, k)))
        .collect();
    let mut order: Vec<usize> = (0..def.ai.len()).collect();
    // Highest aim first; ties in grid order (stable sort).
    order.sort_by_key(|&i| std::cmp::Reverse(aims[i]));
    // First everyone whose own car fits keeps it (so nobody takes a regular's car from them),
    // then the rest pick, strongest first.
    let keeps: Vec<bool> = def
        .ai
        .iter()
        .zip(&aims)
        .map(|(d, &aim)| class_of(d.car).is_some_and(|k| k.abs_diff(aim) <= KEEP_WITHIN))
        .collect();
    let mut cars: Vec<&'static str> = def.ai.iter().map(|d| d.car).collect();
    let mut taken: Vec<&'static str> = (0..def.ai.len())
        .filter(|&i| keeps[i])
        .map(|i| def.ai[i].car)
        .collect();
    for i in order.into_iter().filter(|&i| !keeps[i]) {
        let pick = pool
            .iter()
            .filter(|(c, _)| !taken.contains(c))
            .min_by_key(|(_, k)| k.abs_diff(aims[i]))
            .map(|(c, _)| *c);
        if let Some(c) = pick {
            cars[i] = c;
            taken.push(c);
        }
    }
    cars
}

/// `def` with its field in `cars` (`def.ai` order; the drivers, profiles and grid unchanged).
pub fn with_cars(def: &RaceDef, cars: &[&'static str]) -> RaceDef {
    let mut out = def.clone();
    out.ai = def
        .ai
        .iter()
        .zip(cars)
        .map(|(d, c)| AiDriverDef { car: c, ..*d })
        .collect();
    out
}

/// How much the rep a race result earns is multiplied by for a field of class indices `field`
/// against the player's car of class `player_class`:
/// - the field's class: 1 + (mean − [`REP_PAR_CLASS`]) / 350, either side: the catalog's own
///   Loop 9 field of sport coupes (about 490) earns about 1.1×, a field of exotics about 1.7-1.9×,
///   one of hatchbacks 0.75×;
/// - out-gunned: 1 + (field mean − player) / 250 when the field is faster than the player's
///   car, at most 1.5 (a faster car than the field costs nothing more: the class already
///   counts it).
///
/// The product is kept within [`REP_SCALE_MIN`]..=[`REP_SCALE_MAX`]. An empty field is 1.
pub fn rep_scale(field: &[u16], player_class: u16) -> f32 {
    if field.is_empty() {
        return 1.0;
    }
    let mean = field.iter().map(|&k| k as f32).sum::<f32>() / field.len() as f32;
    let class = 1.0 + (mean - REP_PAR_CLASS) / 350.0;
    let gap = (1.0 + (mean - player_class as f32).max(0.0) / 250.0).min(1.5);
    (class * gap).clamp(REP_SCALE_MIN, REP_SCALE_MAX)
}

/// The field class index that earns the stake table's rep ([`rep_scale`]).
pub const REP_PAR_CLASS: f32 = 450.0;

/// `rep` scaled by `scale` ([`rep_scale`]), rounded. Only rep earned is scaled: a loss of rep
/// (a DNF) stays as the stake table has it.
pub fn scaled_rep(rep: i64, scale: f32) -> i64 {
    if rep > 0 {
        (rep as f32 * scale).round() as i64
    } else {
        rep
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

    /// `wp_economy`'s class indices of the pool (2026-10-27), for pure tests.
    fn class_of(id: &str) -> Option<u16> {
        Some(match id {
            "junkyard_beater" => 173,
            "tansy_13" => 224,
            "brigand_400" => 245,
            "stoat_16v" => 303,
            "marten_20" => 315,
            "vireo_xs" => 297,
            "bulwark_400" => 332,
            "kestrel_ts" => 427,
            "shiro_turbo" => 460,
            "talus_20t" => 480,
            "sperber_32" => 500,
            "rampart_50" => 569,
            "hakone_gt" => 494,
            "lindwurm_4t" => 465,
            "arashi_t4" => 465,
            "lucciola_350" => 619,
            "merlo_20t" => 682,
            "kagero_hx" => 674,
            "ostwind_6t" => 792,
            "morrigan_55" => 999,
            "fulmine_v10" => 780,
            "halberd_40" => 999,
            "ombra_v12" => 858,
            "nyx_proto" => 999,
            _ => return None,
        })
    }

    fn loop9() -> RaceDef {
        catalog::race("loop9_circuit").unwrap()
    }

    fn mean(cars: &[&str]) -> f32 {
        cars.iter()
            .map(|c| class_of(c).unwrap() as f32)
            .sum::<f32>()
            / cars.len() as f32
    }

    #[test]
    fn targets_climb_with_rep_and_with_the_car() {
        let mut last = 0;
        for t in RepTier::ALL {
            let k = target_class(t, 500);
            assert!(k > last, "{t:?}: {k}");
            last = k;
        }
        assert!(target_class(RepTier::Known, 800) > target_class(RepTier::Known, 500));
        // A legend in a beater still meets exotics.
        assert!(target_class(RepTier::Legend, 170) >= 620);
        // A new name in an exotic meets exotics.
        assert!(target_class(RepTier::Unknown, 780) >= 620);
    }

    #[test]
    fn bring_an_exotic_face_exotics() {
        let def = loop9();
        let cars = field_cars(&def, target_class(RepTier::Unknown, 780), class_of);
        assert_eq!(cars.len(), def.ai.len());
        for c in &cars {
            assert!(
                class_of(c).unwrap() >= 619,
                "{c} in an exotic field: {cars:?}"
            );
        }
        // And a hatchback faces hatchbacks, not coupes or exotics.
        let cars = field_cars(&def, target_class(RepTier::Unknown, 297), class_of);
        for c in &cars {
            assert!(
                class_of(c).unwrap() < 400,
                "{c} in a hatchback field: {cars:?}"
            );
        }
    }

    #[test]
    fn rep_raises_the_field_in_the_same_car() {
        let def = loop9();
        let mut last = 0.0;
        for t in RepTier::ALL {
            let m = mean(&field_cars(&def, target_class(t, 427), class_of));
            assert!(m >= last, "{t:?}: mean {m} < {last}");
            last = m;
        }
        let low = mean(&field_cars(
            &def,
            target_class(RepTier::Unknown, 427),
            class_of,
        ));
        assert!(last > low + 100.0, "legend {last} vs unknown {low}");
    }

    #[test]
    fn no_car_twice_and_regulars_keep_their_own_when_it_fits() {
        let def = loop9();
        for t in RepTier::ALL {
            for p in [173, 297, 427, 500, 619, 780, 999] {
                let cars = field_cars(&def, target_class(t, p), class_of);
                let mut sorted = cars.clone();
                sorted.sort();
                sorted.dedup();
                assert_eq!(sorted.len(), cars.len(), "{t:?} {p}: {cars:?}");
            }
        }
        // Aimed at the catalog field's own level, everybody keeps their car.
        let own: Vec<&str> = def.ai.iter().map(|d| d.car).collect();
        let cars = field_cars(&def, mean(&own).round() as u16, class_of);
        let kept = cars.iter().zip(&own).filter(|(a, b)| a == b).count();
        assert!(kept >= 2, "{cars:?} vs {own:?}");
    }

    #[test]
    fn the_best_regular_gets_the_fastest_car() {
        let def = loop9();
        let cars = field_cars(&def, 760, class_of);
        let best = def
            .ai
            .iter()
            .position(|d| d.id == "mara")
            .expect("Mara races Loop 9");
        let worst = def.ai.iter().position(|d| d.id == "tito").unwrap();
        assert!(class_of(cars[best]) > class_of(cars[worst]), "{cars:?}");
    }

    #[test]
    fn with_cars_swaps_only_the_cars() {
        let def = loop9();
        let cars = field_cars(&def, 800, class_of);
        let scaled = with_cars(&def, &cars);
        for ((a, b), c) in def.ai.iter().zip(&scaled.ai).zip(&cars) {
            assert_eq!((a.id, a.name, a.profile), (b.id, b.name, b.profile));
            assert_eq!(b.car, *c);
        }
        assert_eq!(scaled.id, def.id);
    }

    #[test]
    fn tougher_fields_pay_more_rep() {
        // Same gap, faster class: more; a field of hatchbacks: less.
        assert!(rep_scale(&[780; 5], 780) > 1.7);
        assert!(rep_scale(&[780; 5], 780) > rep_scale(&[490; 5], 490));
        assert!(rep_scale(&[250; 5], 250) < 1.0);
        // Same class, out-gunned: more; a faster car than the field: the class alone.
        assert!(rep_scale(&[600; 5], 450) > rep_scale(&[600; 5], 600));
        assert_eq!(rep_scale(&[600; 5], 780), rep_scale(&[600; 5], 600));
        // A field at par pays the table.
        assert!((rep_scale(&[450; 5], 450) - 1.0).abs() < 1e-6);
        for f in [100, 450, 999] {
            for p in [100, 450, 999] {
                let s = rep_scale(&[f; 3], p);
                assert!((REP_SCALE_MIN..=REP_SCALE_MAX).contains(&s), "{f} {p}: {s}");
            }
        }
        assert_eq!(rep_scale(&[], 500), 1.0);
    }

    #[test]
    fn only_rep_earned_is_scaled() {
        assert_eq!(scaled_rep(100, 1.55), 155);
        assert_eq!(scaled_rep(-50, 2.0), -50);
        assert_eq!(scaled_rep(0, 2.0), 0);
    }
}
