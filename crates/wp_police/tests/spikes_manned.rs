//! Row 165 ("unaccompanied spikes"): across the police scenarios that use spike strips, no
//! strip ever lies on the road without its police unit beside it (within the leash of its
//! post), checked after every step, and no spike car appears where a racer sees it (the row
//! 131 pop-in audit), unless it is `spikes.appear_far_m` or more away (owner, 2026-10-25). Prints spike hits and how the strips went down (cars appearing at a
//! hidden post, cars driving in, strips given up) for the integration notes.

use std::path::Path;
use wp_police::dispatch::SPIKE_UNIT_LEASH_M;
use wp_police::graph::dist;
use wp_police::scenario::{load_scenario, run_scenario_with_hook};
use wp_police::{Assignment, PoliceConfig, PoliceSystem, UnitRole};

/// Per-run totals collected from the hook.
#[derive(Default, Debug)]
struct Seen {
    steps: u64,
    strip_steps: u64,
    tool_spawns: usize,
    tool_spawns_in_sight: usize,
    tool_spawns_seen_soon: usize,
    laid: u32,
    drive_ins: u32,
    given_up: u32,
}

/// Assert every strip is manned; return (tool spawns, in sight, seen within the audit window).
fn check_step(p: &PoliceSystem, name: &str) -> (usize, usize, usize) {
    for s in p.spike_strips() {
        let uid = s
            .unit
            .unwrap_or_else(|| panic!("{name}: strip {:?} without its unit", s.id));
        let u = p
            .units()
            .iter()
            .find(|u| u.id == uid)
            .unwrap_or_else(|| panic!("{name}: strip {:?}'s unit {uid:?} is gone", s.id));
        assert!(
            !u.wrecked && matches!(u.assignment, Assignment::SpikeAt { tool, .. } if tool == s.id),
            "{name}: strip {:?}'s unit no longer on it ({:?})",
            s.id,
            u.assignment
        );
        let post = s.post.expect("strip without a post");
        let d = dist(u.body.pos, post);
        assert!(
            d <= SPIKE_UNIT_LEASH_M,
            "{name}: strip {:?}'s unit {d:.1} m from its post",
            s.id
        );
    }
    let tool: Vec<_> = p
        .stats()
        .spawns
        .iter()
        .filter(|r| r.role == UnitRole::Tool)
        .collect();
    // Owner (2026-10-25): on a long straight a spike car may appear in plain view, but only
    // `spikes.appear_far_m` or further out (a speck in the distance), already set up.
    let far = PoliceConfig::builtin().tools.spikes.appear_far_m;
    (
        tool.len(),
        tool.iter()
            .filter(|r| r.in_sight && r.nearest_racer_m < far)
            .count(),
        tool.iter().filter(|r| r.seen_after_s.is_some()).count(),
    )
}

fn run(file: &str, runs: u32) -> Seen {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scenarios")
        .join(file);
    let mut sc = load_scenario(&path).unwrap();
    sc.runs = runs;
    let name = sc.name.clone();
    let mut seen = Seen::default();
    // Per-run counters restart with each run's fresh system: keep the last values of the run.
    let mut last = (0usize, 0usize, 0usize, 0u32, 0u32, 0u32);
    let flush = |seen: &mut Seen, l: (usize, usize, usize, u32, u32, u32)| {
        seen.tool_spawns += l.0;
        seen.tool_spawns_in_sight += l.1;
        seen.tool_spawns_seen_soon += l.2;
        seen.laid += l.3;
        seen.drive_ins += l.4;
        seen.given_up += l.5;
    };
    let summary = run_scenario_with_hook(&sc, &PoliceConfig::builtin(), &mut |p, _| {
        seen.steps += 1;
        seen.strip_steps += !p.spike_strips().is_empty() as u64;
        let (n, in_sight, soon) = check_step(p, &name);
        let st = p.stats();
        let now = (
            n,
            in_sight,
            soon,
            st.spikes,
            st.spike_drive_ins,
            st.spikes_given_up,
        );
        if now.0 < last.0 || now.3 < last.3 {
            flush(&mut seen, last);
        }
        last = now;
    });
    flush(&mut seen, last);
    let hits = summary.metrics.get("spike_hits").copied().unwrap_or(0.0);
    eprintln!(
        "{name}: {runs} run(s), spike hits {hits}, strips requested {}, drive-ins {}, given up {}, \
         steps with a strip down {} / {}, tool spawns {} (in sight {}, seen within the audit window {})",
        seen.laid,
        seen.drive_ins,
        seen.given_up,
        seen.strip_steps,
        seen.steps,
        seen.tool_spawns,
        seen.tool_spawns_in_sight,
        seen.tool_spawns_seen_soon
    );
    seen
}

#[test]
fn tool_placement_strips_are_always_manned() {
    let s = run("tool_placement.ron", 2);
    assert!(s.strip_steps > 0, "no strip went down at all");
    assert_eq!(
        s.tool_spawns_in_sight, 0,
        "a tool car appeared in a racer's sight"
    );
}

#[test]
fn radio_city_chase_strips_are_always_manned() {
    let s = run("radio_city_chase.ron", 2);
    assert!(s.strip_steps > 0, "no strip went down at all");
    assert_eq!(
        s.tool_spawns_in_sight, 0,
        "a tool car appeared in a racer's sight"
    );
}
