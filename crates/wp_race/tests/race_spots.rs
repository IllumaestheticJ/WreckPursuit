//! Honk-to-start meet spots (`wp_race::spots`) on the real maps: every Port Vesper race has a
//! spot on Port Vesper alone (`vesper_v2`) and at the same place on the combined Port Vesper +
//! Ashpeak County map (`ashpeak`), and its course plans there with the same length.

use std::sync::OnceLock;

use wp_race::catalog::{self, RaceCity};
use wp_race::spots::{race_spots, RaceSpot, SPOT_RADIUS_M};
use wp_world::{World, WorldConfig};

fn cfg() -> WorldConfig {
    WorldConfig {
        parked_cars: true,
        ..Default::default()
    }
}

fn vesper() -> &'static World {
    static W: OnceLock<World> = OnceLock::new();
    W.get_or_init(|| World::vesper(&wp_world::vesper::VesperConfig::default(), cfg()))
}

fn ashpeak() -> &'static World {
    static W: OnceLock<World> = OnceLock::new();
    W.get_or_init(|| World::ashpeak(&wp_world::ashpeak::default_config(), cfg()))
}

fn spot<'a>(spots: &'a [RaceSpot], id: &str) -> &'a RaceSpot {
    spots
        .iter()
        .find(|s| s.race_id == id)
        .unwrap_or_else(|| panic!("no spot for {id}"))
}

#[test]
fn every_port_vesper_race_has_a_spot_on_port_vesper_and_on_the_combined_map() {
    let on_vesper = race_spots(vesper());
    let on_ashpeak = race_spots(ashpeak());
    let vesper_races: Vec<_> = catalog::races()
        .into_iter()
        .filter(|r| r.city == RaceCity::Vesper)
        .collect();
    assert!(vesper_races.len() >= 3);
    for def in &vesper_races {
        let a = spot(&on_vesper, def.id);
        let b = spot(&on_ashpeak, def.id);
        assert_eq!(a.radius_m, SPOT_RADIUS_M);
        // Same place, same direction, same course length: the race is the same race.
        let d = (a.pos[0] - b.pos[0]).hypot(a.pos[1] - b.pos[1]);
        assert!(d < 1.0, "{}: {:?} vs {:?}", def.id, a.pos, b.pos);
        assert!((a.heading_rad - b.heading_rad).abs() < 0.05, "{}", def.id);
        for s in [a, b] {
            assert!(
                (s.length_m - def.length_hint_m).abs() < 0.02 * def.length_hint_m,
                "{}: {:.0} m vs hint {}",
                def.id,
                s.length_m,
                def.length_hint_m
            );
        }
        // The spot is the pole position of the planned course.
        let race = catalog::plan_course(vesper(), def).unwrap();
        assert_eq!(a.pos, catalog::venue_pos(&race));
    }
    // Port Vesper alone has only Port Vesper's races.
    assert!(on_vesper
        .iter()
        .all(|s| catalog::race(s.race_id).unwrap().city == RaceCity::Vesper));
}

#[test]
fn spots_do_not_overlap() {
    for spots in [race_spots(vesper()), race_spots(ashpeak())] {
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[i + 1..] {
                let d = (a.pos[0] - b.pos[0]).hypot(a.pos[1] - b.pos[1]);
                assert!(
                    d > a.radius_m + b.radius_m || (a.pos[2] - b.pos[2]).abs() > 4.0,
                    "{} and {} overlap ({d:.0} m)",
                    a.race_id,
                    b.race_id
                );
            }
        }
    }
}

/// Ashpeak County's own races plan on the combined map, start where their spot is and match
/// their length hints. Needs the district roads (mountains, farms, metro, freeways).
#[test]
#[ignore = "needs the Ashpeak districts merged"]
fn every_ashpeak_race_has_a_spot_and_plans_to_its_length_hint() {
    let spots = race_spots(ashpeak());
    let mut off = Vec::new();
    for def in catalog::races_on("ashpeak")
        .into_iter()
        .filter(|r| r.city == RaceCity::Ashpeak)
    {
        let s = spot(&spots, def.id);
        if (s.length_m - def.length_hint_m).abs() >= 0.02 * def.length_hint_m {
            off.push(format!(
                "{}: planned {:.0} m, hint {}",
                def.id, s.length_m, def.length_hint_m
            ));
        }
    }
    assert!(off.is_empty(), "length hints to update: {off:#?}");
}
