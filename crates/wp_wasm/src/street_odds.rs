//! Street racer odds (lead): what beating a roaming street racer pays, from the racer's driver
//! level and the car they bring, against the player's car (owner request 2026-10-24: "street
//! racers should scale in reward and difficulty depending on driver level / car they bring").
//!
//! - **Strength.** A racer's strength is an Elo-like rating: their level's
//!   [`Level::rating`](crate::street::Level::rating) plus [`CAR_POINTS_PER_CLASS`] for every
//!   class-index point their car has over the player's (`wp_economy::Economy::class_index`,
//!   the lobby performance index), capped at ±[`CAR_CAP`]. The player is [`PLAYER_RATING`].
//! - **Odds.** The player's chance is the Elo expectation
//!   `p = 1 / (1 + 10^((racer − player) / 400))`; a win pays back `1 / (p · (1 + MARGIN))`
//!   times the stake (stake included), rounded down to [`ODDS_STEP_MILLI`] and kept within
//!   [`MIN_ODDS_MILLI`]..=[`MAX_ODDS_MILLI`]. Equal cars pay about 1.8× (Street) to 3.9×
//!   (Legend); a much faster car than the racer's pays as little as 1.2×, a much slower one up
//!   to 8×.
//! - **Rep.** The level's `win_rep`, scaled by how far the car gap moved the odds from the
//!   level's equal-car odds (`odds / par_odds`, within 0.5..=2).
//!
//! The odds are fixed when the challenge opens and locked into the race at accept, so a
//! garage visit in between cannot change a race already agreed.

use serde::Serialize;
use wp_contracts::VehicleDef;
use wp_economy::estimate::bracket;

use crate::street::Level;

/// The player's rating in the matchup (the racers' level ratings are set around it).
pub const PLAYER_RATING: f32 = 1000.0;
/// Rating points per class-index point the racer's car has over the player's.
pub const CAR_POINTS_PER_CLASS: f32 = 1.0;
/// The car gap moves the racer's rating at most this far either way.
pub const CAR_CAP: f32 = 250.0;
/// House margin on the offered odds (like `data/events/betting.toml` `h2h_margin`).
pub const MARGIN: f32 = 0.10;
/// Shortest odds a win pays, thousandths (1.2×: a win always makes money).
pub const MIN_ODDS_MILLI: u32 = 1_200;
/// Longest odds a win pays, thousandths (8×).
pub const MAX_ODDS_MILLI: u32 = 8_000;
/// Odds are rounded down to a multiple of this, thousandths (0.05).
pub const ODDS_STEP_MILLI: u32 = 50;
/// The car gap scales the level's `win_rep` by at least this much …
pub const REP_SCALE_MIN: f32 = 0.5;
/// … and at most this much.
pub const REP_SCALE_MAX: f32 = 2.0;

/// One challenge priced: the two cars, the player's chance and what a win pays (JSON for the
/// challenge card).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Matchup {
    /// The racer's car class index (`wp_economy` performance index).
    pub racer_class: u16,
    /// Its lobby bracket letter (D, C, B, A, S, X).
    pub racer_bracket: char,
    /// The player's car class index.
    pub player_class: u16,
    /// Its lobby bracket letter.
    pub player_bracket: char,
    /// The racer's strength: level rating plus the car gap, Elo points.
    pub racer_rating: f32,
    /// The player's estimated chance to win, 0..=1.
    pub win_chance: f32,
    /// A win pays the stake times this, thousandths, stake included (2950 = 2.95×).
    pub odds_milli: u32,
    /// Rep for the win.
    pub win_rep: i64,
}

impl Matchup {
    /// Gross return of a won `stake` (stake included), rounded down.
    pub fn payout(&self, stake: i64) -> i64 {
        stake * self.odds_milli as i64 / 1000
    }
}

/// The class index of a (possibly modified) car definition, from the embedded economy tables.
pub fn class_index(def: &VehicleDef) -> u16 {
    crate::garage::with_economy(|e| e.class_index(def))
}

/// The bracket letter of a class index.
fn bracket_of(index: u16) -> char {
    crate::garage::with_economy(|e| {
        bracket(index, &e.market.class_index)
            .chars()
            .next()
            .unwrap_or('?')
    })
}

/// Offered odds (thousandths) at the player's win chance `p`.
fn odds_at(p: f32) -> u32 {
    let fair = 1.0 / (p.max(1e-3) * (1.0 + MARGIN));
    let milli = (fair * 1000.0) as u32 / ODDS_STEP_MILLI * ODDS_STEP_MILLI;
    milli.clamp(MIN_ODDS_MILLI, MAX_ODDS_MILLI)
}

/// The player's Elo expectation against a racer rated `racer`.
fn chance(racer: f32) -> f32 {
    1.0 / (1.0 + 10f32.powf((racer - PLAYER_RATING) / 400.0))
}

/// Price a challenge against a racer of level `l` in a car of class `racer_class`, the player
/// in a car of class `player_class`.
pub fn matchup(l: &Level, racer_class: u16, player_class: u16) -> Matchup {
    let gap = (racer_class as f32 - player_class as f32) * CAR_POINTS_PER_CLASS;
    let racer_rating = l.rating + gap.clamp(-CAR_CAP, CAR_CAP);
    let win_chance = chance(racer_rating);
    let odds_milli = odds_at(win_chance);
    let par = odds_at(chance(l.rating));
    let scale = (odds_milli as f32 / par as f32).clamp(REP_SCALE_MIN, REP_SCALE_MAX);
    Matchup {
        racer_class,
        racer_bracket: bracket_of(racer_class),
        player_class,
        player_bracket: bracket_of(player_class),
        racer_rating,
        win_chance,
        odds_milli,
        win_rep: (l.win_rep as f32 * scale).round() as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::street::{level, LEVELS};

    #[test]
    fn equal_cars_pay_more_at_higher_levels() {
        let mut last = 0;
        for l in &LEVELS {
            let m = matchup(l, 500, 500);
            assert!(m.odds_milli > last, "level {}: {m:?}", l.level);
            assert_eq!(m.win_rep, l.win_rep, "par rep at equal cars");
            last = m.odds_milli;
        }
        assert!((1_700..=1_900).contains(&matchup(level(2), 500, 500).odds_milli));
        assert!((3_700..=4_100).contains(&matchup(level(5), 500, 500).odds_milli));
    }

    #[test]
    fn a_faster_racer_car_pays_more_a_slower_one_less() {
        let l = level(4);
        let par = matchup(l, 550, 550);
        let faster = matchup(l, 650, 550);
        let slower = matchup(l, 450, 550);
        assert!(faster.odds_milli > par.odds_milli && faster.win_rep > par.win_rep);
        assert!(slower.odds_milli < par.odds_milli && slower.win_rep < par.win_rep);
        assert!(faster.win_chance < par.win_chance && par.win_chance < slower.win_chance);
    }

    #[test]
    fn odds_stay_in_bounds_and_a_win_always_pays() {
        for l in &LEVELS {
            for (r, p) in [(100, 999), (999, 100), (500, 500)] {
                let m = matchup(l, r, p);
                assert!((MIN_ODDS_MILLI..=MAX_ODDS_MILLI).contains(&m.odds_milli));
                assert_eq!(m.odds_milli % ODDS_STEP_MILLI, 0);
                assert!(m.payout(1_000) > 1_000);
                assert!(m.win_rep >= (l.win_rep as f32 * REP_SCALE_MIN).round() as i64);
                assert!(m.win_rep <= (l.win_rep as f32 * REP_SCALE_MAX).round() as i64);
            }
        }
    }

    /// Owner request 2026-10-24: a higher level brings a faster car. Every car of a level's
    /// pool is faster (class index) than every car of the level below.
    #[test]
    fn car_pools_climb_with_the_level() {
        let class = |id: &str| {
            let car = simcli::data::load_car(crate::data::mem_data(), id).expect(id);
            class_index(&car.def)
        };
        let mut below = 0;
        for l in &LEVELS {
            let pis: Vec<u16> = l.cars.iter().map(|c| class(c)).collect();
            let (lo, hi) = (*pis.iter().min().unwrap(), *pis.iter().max().unwrap());
            assert!(lo > below, "level {} pool {:?} = {pis:?}", l.level, l.cars);
            below = hi;
        }
    }

    #[test]
    fn payout_is_integer_and_rounds_down() {
        let m = Matchup {
            odds_milli: 2_950,
            ..matchup(level(3), 500, 500)
        };
        assert_eq!(m.payout(1_000), 2_950);
        assert_eq!(m.payout(333), 982);
    }
}
