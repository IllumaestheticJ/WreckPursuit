//! Contract `RaceKind` / `Legality` pricing (race.md R5, agreed with workstream 15).
//!
//! The economy prices every `wp_race` kind from `stakes.toml [race_kinds]` and `[legality]`.
//! Its relative purses must stay close to `wp_race`'s advertised ones
//! (`data/events/stakes.toml [kinds]`), so the lobby and the payout agree on which race is worth
//! more.

mod common;

use common::*;
use wp_contracts::{DamageMode, DayPhase, Legality, RaceKind, VehicleClass};
use wp_economy::{EventReason, RaceResult, RaceType};

fn kind(key: &str) -> RaceKind {
    RaceKind::from_key(key).unwrap()
}

/// Winner purse for a night, Compact, Arcade street race of `km`.
fn purse(
    e: &wp_economy::Economy,
    k: RaceKind,
    legality: Legality,
    phase: DayPhase,
    km: f32,
) -> f32 {
    let s = e.race_setup(
        k,
        legality,
        VehicleClass::Compact,
        km * 1000.0,
        phase,
        DamageMode::Arcade,
    );
    e.base_purse(&s) as f32
}

#[test]
fn every_kind_maps_onto_the_same_race_type_as_wp_race() {
    // Same mapping as `wp_race::bridge::economy_race_type` (the ledger records this type).
    let e = econ();
    let expect = [
        ("sprint", RaceType::Sprint),
        ("circuit", RaceType::Circuit),
        ("outrun", RaceType::Touge),
        ("checkpoint", RaceType::Sprint),
        ("cannonball", RaceType::Sprint),
        ("delivery", RaceType::Sprint),
        ("drag", RaceType::Drag),
        ("time_attack", RaceType::Circuit),
    ];
    assert_eq!(expect.len(), RaceKind::KEYS.len());
    for (key, rt) in expect {
        assert_eq!(e.race_type_for(&kind(key)), rt, "{key}");
    }
}

#[test]
fn relative_purses_follow_wp_race_stakes() {
    let e = econ();
    let text = std::fs::read_to_string(repo().join("data/events/stakes.toml")).unwrap();
    let table: toml::Value = toml::from_str(&text).unwrap();
    let base = |key: &str| table["kinds"][key]["base_purse"].as_integer().unwrap() as f32;
    let km = |key: &str| if key == "drag" { 0.402 } else { 9.0 };
    let econ_sprint = purse(&e, kind("sprint"), Legality::Street, DayPhase::Night, 9.0);
    for key in RaceKind::KEYS {
        let ours = purse(&e, kind(key), Legality::Street, DayPhase::Night, km(key)) / econ_sprint;
        let theirs = base(key) / base("sprint");
        assert!(
            (ours / theirs - 1.0).abs() <= 0.15,
            "{key}: economy {ours:.2} of a sprint, wp_race {theirs:.2}"
        );
    }
}

#[test]
fn legality_orders_purses_and_buy_ins() {
    let e = econ();
    let sprint = kind("sprint");
    let night_street = purse(&e, sprint, Legality::Street, DayPhase::Night, 9.0);
    let night_sanctioned = purse(&e, sprint, Legality::Sanctioned, DayPhase::Night, 9.0);
    let day_illegal = purse(&e, sprint, Legality::IllegalDaytime, DayPhase::Day, 9.0);
    assert!(night_sanctioned < night_street);
    // wp_race's feel target: a rare illegal daytime race pays 2.5-4x a night street race.
    let ratio = day_illegal / night_street;
    assert!((2.4..=4.0).contains(&ratio), "{ratio:.2}");

    let setup = |l| {
        e.race_setup(
            sprint,
            l,
            VehicleClass::Compact,
            9000.0,
            DayPhase::Day,
            DamageMode::Arcade,
        )
    };
    // Sanctioned events charge more to enter for less prize money.
    let fee_frac = |l| e.entry_fee(&setup(l)) as f32 / e.base_purse(&setup(l)) as f32;
    assert!(fee_frac(Legality::Sanctioned) > fee_frac(Legality::Street));
}

#[test]
fn kinds_without_a_setup_change_nothing_for_plain_race_types() {
    // `kind: None` + `Street` is exactly the old pricing.
    let e = econ();
    let plain = wp_economy::RaceSetup {
        race_type: RaceType::Sprint,
        kind: None,
        legality: Legality::Street,
        class: VehicleClass::Compact,
        route_length_m: 9000.0,
        phase: DayPhase::Night,
        mode: DamageMode::Arcade,
    };
    let kinded = e.race_setup(
        kind("sprint"),
        Legality::Street,
        VehicleClass::Compact,
        9000.0,
        DayPhase::Night,
        DamageMode::Arcade,
    );
    assert_eq!(e.base_purse(&plain), e.base_purse(&kinded));
    assert_eq!(e.entry_fee(&plain), e.entry_fee(&kinded));
}

#[test]
fn outrun_is_recorded_as_a_touge_race_in_the_ledger() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 20_000);
    // A caller that leaves the old race type in place still gets the kind's row.
    let mut s = e.race_setup(
        RaceKind::Outrun { lead_m: 300.0 },
        Legality::Street,
        VehicleClass::Compact,
        6000.0,
        DayPhase::Night,
        DamageMode::Arcade,
    );
    s.race_type = RaceType::Sprint;
    let (_, entered) = e.enter_race(&mut a, &s, car).unwrap();
    let r = RaceResult {
        placement: 1,
        contacts: 0,
        violations: Default::default(),
        wanted: wp_contracts::WantedLevel::None,
    };
    let (_, settled) = e.settle_race(&mut a, &s, car, &r).unwrap();
    let races: Vec<&str> = entered
        .iter()
        .chain(&settled)
        .filter_map(|ev| match &ev.reason {
            EventReason::RaceEntryFee { race } | EventReason::RacePurse { race, .. } => {
                Some(race.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(races, ["touge", "touge"]);
}
