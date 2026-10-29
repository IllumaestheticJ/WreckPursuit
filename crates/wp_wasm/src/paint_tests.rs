//! Native tests that a career car has one colour everywhere it is drawn (owner report on
//! v0.14.4: "the car is white in garage and orange in freeroam"). The garage view
//! (`CareerCarView::paint`, which the garage bay, workshop and paint shop draw) and every session
//! of the car (Free Roam, the workshop showroom, a lot visit) carry the same colour:
//! `wp_economy::OwnedCar::body_paint`.

use wp_contracts::DamageMode;
use wp_economy::{CarUid, Condition, Economy, LotKind, FACTORY_PAINTS};

use crate::career::Career;
use crate::scenario::free_drive;
use crate::session::Session;

fn econ() -> Economy {
    crate::garage::with_economy(|e| e.clone())
}

/// A career with plenty of cash and every class unlocked, no car yet.
fn rich() -> Career {
    let mut c = Career::new(econ(), DamageMode::Arcade);
    c.profile_mut().account.cash = 5_000_000;
    c.profile_mut().account.rep = 1_000_000;
    c
}

/// The colour the garage view draws car `uid` in.
fn garage_paint(c: &mut Career, uid: CarUid) -> u32 {
    c.view()
        .cars
        .iter()
        .find(|x| x.uid == uid.0)
        .expect("car in the garage view")
        .paint
}

/// The colour the session's player entity carries (what the renderer draws it in).
fn player_paint(s: &Session) -> Option<u32> {
    let player = *s.entities().first().expect("player entity");
    s.spec(player).expect("scenario entity").paint
}

/// The colour car `uid` gets on a Free Roam run (the run is finished again afterwards).
fn run_paint(c: &mut Career, uid: CarUid) -> Option<u32> {
    c.set_active(uid).unwrap();
    let mut s = c
        .start_run(free_drive("ignored", DamageMode::Arcade, false))
        .unwrap();
    let paint = player_paint(&s);
    s.run_ticks(2);
    c.finish_run(&mut s).unwrap();
    paint
}

/// The colour car `uid` gets in the workshop's 3D bay (the showroom session).
fn bay_paint(c: &Career, uid: CarUid) -> Option<u32> {
    let s = c
        .showroom(uid, free_drive("ignored", DamageMode::Arcade, false))
        .unwrap();
    player_paint(&s)
}

/// The garage view, the workshop bay and a run all draw car `uid` in `expected`.
fn assert_one_colour(c: &mut Career, uid: CarUid, expected: u32, what: &str) {
    assert_eq!(garage_paint(c, uid), expected, "{what}: garage view");
    assert_eq!(bay_paint(c, uid), Some(expected), "{what}: workshop bay");
    assert_eq!(run_paint(c, uid), Some(expected), "{what}: Free Roam run");
    assert_eq!(
        garage_paint(c, uid),
        expected,
        "{what}: garage view after the run"
    );
}

/// The first used-lot listing on offer today.
fn a_listing(c: &mut Career) -> (String, wp_economy::Listing) {
    let lot = c
        .economy()
        .lots
        .lots
        .iter()
        .find(|l| l.kind == LotKind::UsedLot)
        .expect("a used lot")
        .id
        .clone();
    let (l, _) = c
        .lot_stock(&lot)
        .unwrap()
        .into_iter()
        .find(|(_, sold)| !sold)
        .expect("a car for sale");
    (lot, l)
}

#[test]
fn a_lot_bought_car_keeps_its_listing_colour_in_every_view() {
    let mut c = rich();
    let (lot, l) = a_listing(&mut c);
    let uid = c.buy_listing(&lot, l.slot).unwrap();
    assert_eq!(c.profile().account.car(uid).unwrap().paint, Some(l.paint));
    assert_one_colour(&mut c, uid, l.paint, "lot car");
}

#[test]
fn the_street_beater_has_one_colour_in_every_view() {
    let mut c = Career::new(econ(), DamageMode::Arcade);
    let uid = c.floor_car().unwrap();
    let car = c.profile().account.car(uid).unwrap().clone();
    assert_eq!(car.def_id, "junkyard_beater");
    assert_eq!(car.paint, None, "the beater has no saved paint");
    let expected = car.body_paint();
    assert!(FACTORY_PAINTS.contains(&expected));
    assert_one_colour(&mut c, uid, expected, "street beater");
}

#[test]
fn a_resprayed_car_shows_its_new_paint_in_every_view() {
    let mut c = rich();
    let uid = c.buy_car("kestrel_ts", Condition::Used).unwrap();
    let before = garage_paint(&mut c, uid);
    let paint = if before == 0x123456 {
        0x654321
    } else {
        0x123456
    };
    c.respray(uid, paint).unwrap();
    assert_eq!(c.profile().account.car(uid).unwrap().paint, Some(paint));
    assert_one_colour(&mut c, uid, paint, "resprayed car");
}

/// Dealer cars and saves from before paint was stored (`paint: None`): one default per car,
/// fixed by its uid, shared by every view; different uids may differ.
#[test]
fn a_car_with_no_paint_gets_the_same_default_in_every_view() {
    let mut c = rich();
    let a = c.buy_car("kestrel_ts", Condition::Used).unwrap();
    let (lot, l) = a_listing(&mut c);
    let b = c.buy_listing(&lot, l.slot).unwrap();
    // An old save: the lot car lost its paint.
    c.profile_mut().account.car_mut(b).unwrap().paint = None;
    for uid in [a, b] {
        let car = c.profile().account.car(uid).unwrap().clone();
        assert_eq!(car.paint, None);
        let expected = FACTORY_PAINTS[uid.0 as usize % FACTORY_PAINTS.len()];
        assert_eq!(car.body_paint(), expected);
        assert_one_colour(&mut c, uid, expected, "car without paint");
    }
}

/// A car lot visit draws the player's own car in its garage colour and every listing in the
/// factory paint it is sold in (and keeps once bought).
#[test]
fn a_lot_visit_draws_the_own_car_and_the_listings_in_their_colours() {
    let mut c = rich();
    let uid = c.floor_car().unwrap();
    c.set_active(uid).unwrap();
    let own = garage_paint(&mut c, uid);
    let (lot, _) = a_listing(&mut c);
    let (s, visit) = c.visit_lot(&lot).unwrap();
    let own_entity = wp_contracts::EntityId(visit.own_car.expect("arrived in the car"));
    assert_eq!(s.spec(own_entity).unwrap().paint, Some(own));
    let stock = c.lot_stock(&lot).unwrap();
    for car in &visit.cars {
        let (l, _) = stock.iter().find(|(l, _)| l.slot == car.slot).unwrap();
        let e = wp_contracts::EntityId(car.entity);
        assert_eq!(s.spec(e).unwrap().paint, Some(l.paint), "space {}", l.slot);
    }
}
