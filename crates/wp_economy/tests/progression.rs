//! Career progression: rep tiers, feature / dealer gates, betting wallet, one-off tier rewards.

use wp_contracts::{DamageMode, PlayerId, VehicleClass};
use wp_economy::career::{simulate_career, CareerConfig};
use wp_economy::{career_note, BetVenue, Economy, EconomyError, Feature, PlayerAccount, RepTier};

fn account(e: &Economy, rep: i64, cash: i64) -> PlayerAccount {
    let (mut a, _) = e.open_account(PlayerId(7));
    a.rep = rep;
    a.cash = cash;
    a
}

#[test]
fn embedded_table_is_valid_and_ordered() {
    let e = Economy::embedded();
    let r = &e.progression;
    r.validate().unwrap();
    assert_eq!(r.tier_for_rep(0), RepTier::Unknown);
    for w in r.tiers.windows(2) {
        assert_eq!(r.tier_for_rep(w[1].min_rep - 1), w[0].tier);
        assert_eq!(r.tier_for_rep(w[1].min_rep), w[1].tier);
    }
    assert_eq!(r.tier_for_rep(i64::MAX), RepTier::Legend);
    // The juiced loop: touge betting and pink slips sit above plain touge racing.
    assert!(r.feature_tier(Feature::TougeBetting) > r.feature_tier(Feature::TougeRaces));
    assert!(r.feature_tier(Feature::PinkSlips) >= r.feature_tier(Feature::TougeBetting));
    assert!(r.feature_tier(Feature::CityBetting) > RepTier::Unknown);
}

#[test]
fn broken_tables_are_refused() {
    let e = Economy::embedded();
    let mut r = e.progression.clone();
    r.tiers.swap(1, 2);
    assert!(r.validate().is_err());
    let mut r = e.progression.clone();
    r.tiers[3].min_rep = r.tiers[2].min_rep;
    assert!(r.validate().is_err());
    let mut r = e.progression.clone();
    r.unlocks.remove(&Feature::PinkSlips);
    assert!(r.validate().is_err());
    let mut r = e.progression.clone();
    r.class_unlocks
        .insert(VehicleClass::PoliceSuv, RepTier::Legend);
    assert!(r.validate().is_err());
    let mut r = e.progression.clone();
    r.unlocks.insert(Feature::CityBetting, RepTier::Unknown); // max_bet 0 there
    assert!(r.validate().is_err());
}

#[test]
fn features_follow_current_rep() {
    let e = Economy::embedded();
    let need = |f| {
        e.progression
            .tier_def(e.progression.feature_tier(f))
            .min_rep
    };
    let pink = need(Feature::PinkSlips);
    let low = account(&e, pink - 1, 10_000);
    assert!(!e.is_unlocked(&low, Feature::PinkSlips));
    match e.require_unlocked(&low, Feature::PinkSlips) {
        Err(EconomyError::Locked {
            needs,
            needed_rep,
            rep,
            ..
        }) => {
            assert_eq!(needs, RepTier::Feared);
            assert_eq!(needed_rep, pink);
            assert_eq!(rep, pink - 1);
        }
        other => panic!("expected Locked, got {other:?}"),
    }
    let high = account(&e, pink, 10_000);
    assert!(e.is_unlocked(&high, Feature::PinkSlips));
    e.require_unlocked(&high, Feature::PinkSlips).unwrap();
    assert!(e.is_unlocked(&account(&e, 0, 0), Feature::CityRaces));
}

#[test]
fn dealer_classes_have_no_rep_gate() {
    let e = Economy::embedded();
    // No class has a rep gate: price is the only thing holding a car back.
    let a = account(&e, 0, 100_000);
    for class in [
        VehicleClass::Beater,
        VehicleClass::Compact,
        VehicleClass::Truck,
        VehicleClass::Sport,
        VehicleClass::Muscle,
        VehicleClass::Exotic,
    ] {
        assert!(e.class_unlocked(&a, class), "{class:?}");
        e.require_class(&a, class).unwrap();
    }
    // Police classes are still never for sale, whatever the rep.
    assert!(!e.class_unlocked(&a, VehicleClass::PoliceInterceptor));
    let legend = account(&e, 1_000_000, 0);
    assert!(!e.class_unlocked(&legend, VehicleClass::PoliceInterceptor));
    assert!(matches!(
        e.require_class(&legend, VehicleClass::PoliceCruiser),
        Err(EconomyError::InvalidInput(_))
    ));
}

#[test]
fn bet_limits() {
    let e = Economy::embedded();
    let street = e.progression.tier_def(RepTier::Street).clone();
    // Locked below Street, and touge betting locked at Street.
    assert_eq!(e.max_bet(&account(&e, 0, 50_000), BetVenue::City), 0);
    let a = account(&e, street.min_rep, 50_000);
    assert_eq!(e.max_bet(&a, BetVenue::City), street.max_bet);
    assert_eq!(e.max_bet(&a, BetVenue::Touge), 0);
    // Capped by cash.
    let poor = account(&e, street.min_rep, 600);
    assert_eq!(e.max_bet(&poor, BetVenue::City), 300);
    // Nothing while in debt.
    assert_eq!(
        e.max_bet(&account(&e, street.min_rep, -10), BetVenue::City),
        0
    );
}

/// Street-racer challenges (round12's roaming racers) stay open from the start: no tier, no
/// tier cap, no cash fraction; only cash covers the stake and debt still blocks.
#[test]
fn street_racer_bets_are_open_from_the_start() {
    let e = Economy::embedded();
    assert_eq!(
        e.progression.feature_tier(Feature::StreetBetting),
        RepTier::Unknown
    );
    let mut a = account(&e, 0, 3_000);
    assert_eq!(e.max_bet(&a, BetVenue::Street), 3_000);
    assert_eq!(
        e.max_bet(&a, BetVenue::City),
        0,
        "city betting still waits for Street"
    );
    let (ticket, ev) = e
        .place_bet(&mut a, BetVenue::Street, 2_500, "vs Ace (Elite)")
        .unwrap();
    assert_eq!(a.cash, 500);
    assert_eq!(
        career_note(&ev[0].reason),
        Some(("bet_stake", "street:vs Ace (Elite)"))
    );
    assert!(e.place_bet(&mut a, BetVenue::Street, 501, "x").is_err());
    e.settle_bet(&mut a, &ticket, 5_000).unwrap();
    assert_eq!(a.cash, 5_500);
    let mut debt = account(&e, 0, -1);
    assert_eq!(e.max_bet(&debt, BetVenue::Street), 0);
    assert!(e.place_bet(&mut debt, BetVenue::Street, 1, "x").is_err());
}

#[test]
fn bets_take_and_pay_credits() {
    let e = Economy::embedded();
    let rep = e.progression.tier_def(RepTier::Respected).min_rep;
    let mut a = account(&e, rep, 20_000);
    let (ticket, ev) = e
        .place_bet(&mut a, BetVenue::Touge, 5_000, "pass-1")
        .unwrap();
    assert_eq!(a.cash, 15_000);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].cash_delta, -5_000);
    assert_eq!(
        career_note(&ev[0].reason),
        Some(("bet_stake", "touge:pass-1"))
    );

    let (net, ev) = e.settle_bet(&mut a, &ticket, 9_000).unwrap();
    assert_eq!(net, 4_000);
    assert_eq!(a.cash, 24_000);
    assert_eq!(
        career_note(&ev[0].reason),
        Some(("bet_payout", "touge:pass-1"))
    );

    let (ticket, _) = e.place_bet(&mut a, BetVenue::City, 100, "r").unwrap();
    let (net, ev) = e.settle_bet(&mut a, &ticket, 0).unwrap();
    assert_eq!(net, -100);
    assert!(ev.is_empty());
}

#[test]
fn refused_bets_leave_the_account_untouched() {
    let e = Economy::embedded();
    let street = e.progression.tier_def(RepTier::Street).min_rep;
    let mut a = account(&e, street, 10_000);
    let before = a.clone();
    assert!(matches!(
        e.place_bet(&mut a, BetVenue::Touge, 100, "x"),
        Err(EconomyError::Locked { .. })
    ));
    assert!(e.place_bet(&mut a, BetVenue::City, 0, "x").is_err());
    assert!(e.place_bet(&mut a, BetVenue::City, 1_001, "x").is_err());
    assert_eq!(a, before);
    let mut debt = account(&e, street, -50);
    assert!(matches!(
        e.place_bet(&mut debt, BetVenue::City, 10, "x"),
        Err(EconomyError::InDebt { .. })
    ));
}

#[test]
fn tier_rewards_are_paid_once() {
    let e = Economy::embedded();
    let r = &e.progression;
    let mut a = account(&e, 0, 1_000);
    // Jump straight to Known: Street and Known rewards both paid.
    let (up, ev) = e
        .award_rep(&mut a, r.tier_def(RepTier::Known).min_rep, "boss:test")
        .unwrap();
    let paid = r.tier_def(RepTier::Street).reward + r.tier_def(RepTier::Known).reward;
    assert!(up.promoted());
    assert_eq!(up.first_reached, vec![RepTier::Street, RepTier::Known]);
    assert_eq!(up.reward, paid);
    assert_eq!(a.cash, 1_000 + paid);
    assert_eq!(ev.len(), 3); // rep note + two rewards
    assert_eq!(career_note(&ev[0].reason), Some(("rep", "boss:test")));
    assert_eq!(career_note(&ev[2].reason), Some(("tier_reward", "known")));
    assert!(up.unlocked.contains(&Feature::CityBetting));
    assert!(up.unlocked.contains(&Feature::TougeRaces));

    // Nothing more on a second update.
    let (up, ev) = e.update_progression(&mut a).unwrap();
    assert!(ev.is_empty());
    assert!(!up.promoted() && !up.demoted());

    // Drop a tier (capture), climb back: no second reward, a plain tier note instead.
    let (up, ev) = e.award_rep(&mut a, -200, "capture").unwrap();
    assert!(up.demoted());
    assert_eq!(up.after, RepTier::Street);
    assert!(up.relocked.contains(&Feature::TougeRaces));
    assert_eq!(career_note(&ev[1].reason), Some(("tier", "street")));
    assert_eq!(a.career.peak_tier, RepTier::Known);
    let cash = a.cash;
    let (up, _) = e.award_rep(&mut a, 200, "win").unwrap();
    assert!(up.promoted());
    assert_eq!(up.reward, 0);
    assert_eq!(a.cash, cash);
}

#[test]
fn progress_view() {
    let e = Economy::embedded();
    let r = &e.progression;
    let known = r.tier_def(RepTier::Known).min_rep;
    let resp = r.tier_def(RepTier::Respected).min_rep;
    let a = account(&e, (known + resp) / 2, 30_000);
    let p = e.career_progress(&a);
    assert_eq!(p.tier, RepTier::Known);
    let n = p.next.as_ref().unwrap();
    assert_eq!(n.tier, RepTier::Respected);
    assert_eq!(n.rep_needed, resp - a.rep);
    assert!((p.tier_fraction - 0.5).abs() < 0.01);
    assert_eq!(p.max_bet_touge, 0);
    assert!(p.max_bet_city > 0);
    assert!(p
        .features
        .iter()
        .any(|f| f.feature == Feature::TougeRaces && f.unlocked));
    assert!(p
        .classes
        .iter()
        .any(|c| c.class == VehicleClass::Exotic && c.unlocked));
    let legend = e.career_progress(&account(&e, 1_000_000, 0));
    assert!(legend.next.is_none());
    assert_eq!(legend.tier_fraction, 1.0);
}

#[test]
fn old_saves_load_at_unknown() {
    let e = Economy::embedded();
    let a = account(&e, 2_500, 0);
    let mut v = serde_json::to_value(&a).unwrap();
    v.as_object_mut().unwrap().remove("career");
    let mut back: PlayerAccount = serde_json::from_value(v).unwrap();
    assert_eq!(back.career.peak_tier, RepTier::Unknown);
    // The first update pays what they already qualify for.
    let (up, _) = e.update_progression(&mut back).unwrap();
    assert_eq!(up.first_reached, vec![RepTier::Street, RepTier::Known]);
}

/// Pacing check against the scripted tuning career (mid skill, 3 races a night, Arcade):
/// median rep after N nights lands in the tier the README promises.
#[test]
fn tier_pacing() {
    let e = Economy::embedded();
    let median_rep = |nights: u32| {
        let mut reps: Vec<i64> = (1..=16)
            .map(|s| {
                let mut cfg = CareerConfig::mid_skill(DamageMode::Arcade, s);
                cfg.nights = nights;
                simulate_career(&e, &cfg).account.rep
            })
            .collect();
        reps.sort();
        reps[reps.len() / 2]
    };
    for (nights, tier) in [
        (3, RepTier::Street),
        (10, RepTier::Known),
        (24, RepTier::Respected),
        (45, RepTier::Feared),
    ] {
        let rep = median_rep(nights);
        println!("{nights} nights: median rep {rep}");
        assert_eq!(
            e.progression.tier_for_rep(rep),
            tier,
            "{nights} nights, rep {rep}"
        );
    }
}
