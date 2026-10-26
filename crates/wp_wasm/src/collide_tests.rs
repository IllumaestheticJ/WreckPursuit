//! The player hits traffic (playtest row 149, "I currently now can't hit traffic cars"): in a
//! live city session the player driven at a traffic car makes contact with it.

use wp_contracts::{DamageMode, EntityId, SimEvent, VehicleInput};

use crate::scenario::free_drive;
use crate::session::{set_street_racers_enabled, Session};

/// Outcome of one ram attempt.
#[derive(Debug)]
struct Ram {
    /// The target agent was a sim vehicle (physical) when the player reached it.
    promoted: bool,
    /// A `Contact` event between the player and the target's vehicle.
    contact: bool,
    /// Street racers in the session.
    street: usize,
    /// Most world-physical traffic agents without a sim vehicle at any checked tick.
    orphans: usize,
    /// Most world-physical traffic agents at any checked tick.
    physical: usize,
    /// Most sim vehicles that are not traffic at any checked tick.
    others: usize,
}

/// City free drive or the night run, street racers on or off; after `warm_s` put the player
/// 25 m behind the nearest moving traffic agent, on its lane, 12 m/s faster, and drive straight
/// for 4 s (up to three tries).
fn ram(chase: bool, street_on: bool, extra: usize, warm_s: u32) -> Ram {
    set_street_racers_enabled(street_on);
    let mut sc = if chase {
        crate::scenario::night_run("debug_sedan", DamageMode::Arcade)
    } else {
        free_drive("debug_sedan", DamageMode::Arcade, true)
    };
    sc.seed = 7;
    let mut s = Session::new(sc, false).unwrap();
    set_street_racers_enabled(true);
    let p = EntityId(1);
    // `extra` stand-in vehicles far away (police, racers, wrecks): the traffic slots run short.
    if extra > 0 {
        let car = crate::data::load_car("debug_sedan").unwrap();
        let sim = s.debug_sim_mut().unwrap();
        for k in 0..extra {
            let at = [5000.0 + 10.0 * k as f32, 5000.0, 0.5];
            sim.spawn_vehicle(&car, at, 0.0).unwrap();
        }
    }
    let mut orphans = 0usize;
    let mut physical = 0usize;
    let mut others = 0usize;
    for _ in 0..(warm_s * 60) {
        s.run_ticks(4);
        orphans = orphans.max(orphaned(&s));
        physical = physical.max(physical_count(&s));
        others = others.max(non_traffic(&s));
    }
    let street = s.street().map_or(0, |st| st.roster().len());
    // Up to five tries, each at the nearest moving traffic car (a car can turn off or stop
    // before the player reaches it): placed 25 m behind it on its lane, 12 m/s faster. Three
    // were not enough after the ABS rework of 2026-10-26 reshuffled the traffic (the
    // starved-slots case made no contact in three).
    let mut promoted = false;
    let mut contact = false;
    for _attempt in 0..5 {
        let pp = s
            .sim()
            .vehicle_states()
            .into_iter()
            .find(|(e, _)| *e == p)
            .map(|(_, st)| st.pos)
            .unwrap();
        let (agent, pos, yaw, speed) = s
            .sim()
            .world()
            .agents()
            .filter(|a| a.speed_mps > 3.0)
            .min_by(|a, b| {
                let da = (a.pos[0] - pp[0]).powi(2) + (a.pos[1] - pp[1]).powi(2);
                let db = (b.pos[0] - pp[0]).powi(2) + (b.pos[1] - pp[1]).powi(2);
                da.total_cmp(&db)
            })
            .map(|a| (a.id, a.pos, a.yaw, a.speed_mps))
            .expect("a moving traffic agent");
        // Behind it along its heading (yaw 0 = +Y).
        let (sn, cs) = yaw.sin_cos();
        let back = [pos[0] + sn * 25.0, pos[1] - cs * 25.0];
        {
            let sim = s.debug_sim_mut().unwrap();
            sim.place_on_ground(p, back, yaw, speed + 12.0, Some(pos[2]))
                .unwrap();
        }
        s.drain_events();
        promoted = false;
        for _ in 0..(240 * 4) {
            s.set_input(
                p,
                VehicleInput {
                    throttle: 0.6,
                    ..Default::default()
                },
            )
            .unwrap();
            s.run_ticks(1);
            let entity = s
                .sim()
                .vehicles()
                .iter()
                .find(|v| v.traffic_agent == Some(agent))
                .map(|v| v.id);
            promoted |= entity.is_some();
            orphans = orphans.max(orphaned(&s));
            physical = physical.max(physical_count(&s));
            others = others.max(non_traffic(&s));
            for e in s.drain_events() {
                if let SimEvent::Contact { a, b, .. } = e {
                    if entity.is_some_and(|t| (a == p && b == t) || (a == t && b == p)) {
                        contact = true;
                    }
                }
            }
            if contact {
                break;
            }
        }
        if contact {
            break;
        }
    }
    Ram {
        promoted,
        contact,
        street,
        orphans,
        physical,
        others,
    }
}

/// Traffic agents physical in the world.
fn physical_count(s: &Session) -> usize {
    s.sim()
        .world()
        .agents()
        .filter(|a| a.lod == wp_world::Lod::Physical)
        .count()
}

/// Sim vehicles that are not traffic (player, racers, police).
fn non_traffic(s: &Session) -> usize {
    s.sim()
        .vehicles()
        .iter()
        .filter(|v| v.traffic_agent.is_none())
        .count()
}

/// Traffic agents physical in the world without a sim vehicle (no collider).
fn orphaned(s: &Session) -> usize {
    s.sim()
        .world()
        .agents()
        .filter(|a| a.lod == wp_world::Lod::Physical)
        .filter(|a| {
            !s.sim()
                .vehicles()
                .iter()
                .any(|v| v.traffic_agent == Some(a.id))
        })
        .count()
}

/// Row 149: driven into a traffic car from behind, the player hits it (it is promoted to a
/// physics vehicle near the player and the two collide), with and without street racers.
#[test]
fn player_collides_with_traffic_in_the_city() {
    for (chase, street) in [(false, false), (false, true), (true, false), (true, true)] {
        let r = ram(chase, street, 0, 4);
        println!("night run {chase}, street racers {street}: {r:?}");
        assert!(
            r.promoted,
            "the traffic car near the player is physical: {r:?}"
        );
        assert!(r.contact, "the player hits the traffic car: {r:?}");
        assert_eq!(r.orphans, 0, "every physical traffic car has a body: {r:?}");
        if street {
            assert!(r.street > 0, "street racers spawned: {r:?}");
        }
    }
}

/// Row 149, the cause: with the vehicle slots nearly used up (street racers, police, wrecks)
/// the world promoted traffic past what the sim could embody, and those cars had no collider.
/// Now promotion is capped at the slots left: no physical car without a body, and the car the
/// player drives into still gets one.
#[test]
fn traffic_near_the_player_keeps_a_body_when_vehicle_slots_run_short() {
    // Free drive by day has up to 27 non-traffic vehicles (player, 13 street racers with 5 in
    // the lockup, 13 police; 0.12.49 had 30 and promoted 16 traffic cars where 10 had room).
    // Eleven stand-ins more left 2, and the car the player drives into must take one.
    // 2026-10-23 (owner: fewer patrols by day) about 6 fewer police are about: 17 stand-ins.
    let r = ram(false, true, 17, 40);
    println!("{r:?}");
    assert_eq!(r.orphans, 0, "every physical traffic car has a body: {r:?}");
    assert!(
        r.others >= 36,
        "the slots really ran short (4 or fewer left): {r:?}"
    );
    assert!(r.physical > 0, "traffic still promoted: {r:?}");
    assert!(
        r.promoted && r.contact,
        "the player still hits traffic: {r:?}"
    );
}
