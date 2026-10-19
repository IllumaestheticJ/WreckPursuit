//! Judge cost benchmark: `cargo run --release -p wp_race --example judge_bench`.
//! A 12-car, 3-lap circuit on a 64-point loop, judged every physics tick (240 Hz), plus
//! calendar generation for 365 days. Prints µs per judged tick and per car.

use std::time::Instant;

use wp_contracts::EntityId;
use wp_race::trace::{simulate, ScriptedRacer};
use wp_race::{
    Course, EventId, MeetHeat, RaceData, RaceJudge, RaceKind, Scheduler, Venue, VenueId,
};

fn main() {
    let data = RaceData::embedded();
    // 64-point rounded loop, ~2.4 km.
    let n = 64;
    let pts: Vec<[f32; 3]> = (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            [380.0 * a.cos(), 380.0 * a.sin(), 0.0]
        })
        .collect();
    let course = Course::closed(&pts, 14.0);
    let racers: Vec<ScriptedRacer> = (0..12)
        .map(|i| ScriptedRacer::new(i + 1, 5.0, 36.0 + i as f32 * 0.25))
        .collect();
    let judge = RaceJudge::new(
        EventId(1),
        data.rules.for_kind(RaceKind::Circuit { laps: 3 }),
        course,
    );
    let t0 = Instant::now();
    let out = simulate(judge, &racers, 1, 240 * 1200);
    let dt = t0.elapsed();
    let ticks = out.ticks as f64;
    println!(
        "circuit: {} ticks, {} cars: {:.2} µs/tick, {:.3} µs/car/tick (includes the scripted driver)",
        out.ticks,
        racers.len(),
        dt.as_secs_f64() * 1e6 / ticks,
        dt.as_secs_f64() * 1e6 / ticks / racers.len() as f64
    );
    assert!(out.results.iter().all(|r| r.laps_completed == 3));
    let _ = EntityId(0);

    let venues: Vec<Venue> = (0..24)
        .map(|i| Venue {
            id: VenueId(i),
            name: format!("v{i}"),
            pos: [i as f32 * 100.0, 0.0, 0.0],
            district: None,
            closed_circuit: i % 6 == 0,
        })
        .collect();
    let s = Scheduler {
        rules: &data.schedule,
        stakes: &data.stakes,
        seed: 1,
    };
    let t0 = Instant::now();
    let mut events = 0;
    for day in 0..365 {
        events += s
            .generate_day(day, &venues, &MeetHeat::default())
            .events
            .len();
    }
    println!(
        "scheduler: 365 days, {events} events: {:.1} µs/day",
        t0.elapsed().as_secs_f64() * 1e6 / 365.0
    );
}
