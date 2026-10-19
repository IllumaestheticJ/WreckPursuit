//! Account stores and the event log.

mod common;

use common::*;
use wp_contracts::{PlayerId, SimEvent};
use wp_economy::{AccountStore, EconomyEvent, InMemoryStore, StoreError};

fn exercise(store: &mut dyn AccountStore) {
    let e = econ();
    assert_eq!(store.load(PlayerId(9)).unwrap(), None);
    let (mut a, mut log) = e.open_account(PlayerId(9));
    a.player = PlayerId(9);
    for ev in log.iter_mut() {
        ev.player = PlayerId(9);
    }
    store.commit(&a, &log).unwrap();
    assert_eq!(store.load(PlayerId(9)).unwrap().as_ref(), Some(&a));

    let (_, ev) = e.buy_informant_tip(&mut a, "whisper").unwrap();
    store.commit(&a, &ev).unwrap();
    let all = store.events_since(PlayerId(9), 0).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all.iter().map(|x| x.seq).collect::<Vec<_>>(), vec![0, 1]);
    assert_eq!(store.events_since(PlayerId(9), 1).unwrap(), ev);

    // Replaying the same commit is a sequence conflict (lost update / double commit).
    assert!(matches!(
        store.commit(&a, &ev),
        Err(StoreError::SeqConflict { .. })
    ));
    // So is skipping events.
    let (_, ev2) = e.buy_informant_tip(&mut a, "whisper").unwrap();
    let (_, ev3) = e.buy_informant_tip(&mut a, "whisper").unwrap();
    assert!(store.commit(&a, &ev3).is_err());
    let mut both = ev2.clone();
    both.extend(ev3);
    store.commit(&a, &both).unwrap();
    assert_eq!(store.load(PlayerId(9)).unwrap().unwrap().cash, a.cash);
    // The log explains the balance.
    let sum: i64 = store
        .events_since(PlayerId(9), 0)
        .unwrap()
        .iter()
        .map(|x| x.cash_delta)
        .sum();
    assert_eq!(sum, a.cash);
}

#[test]
fn in_memory_store() {
    exercise(&mut InMemoryStore::default());
}

#[cfg(feature = "native")]
#[test]
fn sqlite_store() {
    exercise(&mut wp_economy::sqlite::SqliteStore::open_in_memory().unwrap());
}

#[cfg(feature = "native")]
#[test]
fn sqlite_store_persists_across_reopen() {
    let dir = std::env::temp_dir().join(format!("wp_economy_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("accounts.db");
    let _ = std::fs::remove_file(&path);
    let e = econ();
    let (acct, ev) = e.open_account(PlayerId(4));
    {
        let mut s = wp_economy::sqlite::SqliteStore::open(&path).unwrap();
        s.commit(&acct, &ev).unwrap();
    }
    let s = wp_economy::sqlite::SqliteStore::open(&path).unwrap();
    assert_eq!(s.load(PlayerId(4)).unwrap(), Some(acct));
    assert_eq!(s.events_since(PlayerId(4), 0).unwrap(), ev);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn events_round_trip_through_sim_event() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 50_000);
    let (_, ev) = e
        .install_mod(
            &mut a,
            car,
            "engine_intake_1",
            wp_contracts::DamageMode::Arcade,
        )
        .unwrap();
    for x in &ev {
        let se = x.to_sim_event();
        match &se {
            SimEvent::Economy(e) => assert_eq!(e.reason.kind_code(), x.reason.kind_code()),
            _ => panic!("wrong variant"),
        }
        assert_eq!(EconomyEvent::from_sim_event(&se).as_ref(), Some(x));
    }
    assert_eq!(
        EconomyEvent::from_sim_event(&SimEvent::TotalLoss {
            vehicle: wp_contracts::EntityId(1)
        }),
        None
    );
}
