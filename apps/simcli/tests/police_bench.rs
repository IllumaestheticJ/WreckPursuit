//! CI-light subset of the police benchmark (`simcli police-bench`, see
//! `docs/integration/police-bench.md`): 3 cells × 2 seeds, 2 minutes each, with generous ranges
//! so only a big regression in police competence (or a broken evader) fails. The full numbers
//! come from `just police-bench`. Ranges are about 2× this subset's numbers on
//! lead/police-bench after merging ws/police @ 759d850: 3.2 crashes/min, 0.21 civilian hits/min,
//! 6.1 stuck records/min, 63 % line of sight, all 6 runs engaged, median first unit within
//! 100 m at 6 s. (On lead/round12 alone: 2.6 / 0.14 / 1.8 / 90 %; see
//! docs/integration/police-bench.md.)

use simcli::police_bench::{run_bench, run_one, summarize, BenchCell, BenchCity, BenchOptions};
use simcli::FsData;
use wp_contracts::DamageMode;
use wp_opponent::EvaderSkill;

fn data() -> FsData {
    FsData::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data"))
}

fn cell(city: BenchCity, night: bool, rain: bool, level: u8, skill: EvaderSkill) -> BenchCell {
    BenchCell {
        city,
        night,
        rain,
        level,
        skill,
    }
}

#[test]
fn police_bench_light() {
    let opts = BenchOptions {
        cells: vec![
            // The reference spot (testbed city chase), an easy grid day and a hard expert run.
            cell(BenchCity::Vesper, true, false, 2, EvaderSkill::Good),
            cell(BenchCity::Grid, false, true, 2, EvaderSkill::Good),
            cell(BenchCity::Vesper, false, false, 3, EvaderSkill::Expert),
        ],
        seeds: vec![1, 2],
        duration_s: 120.0,
        mode: DamageMode::Arcade,
        threads: 0,
        progress: false,
    };
    let runs = run_bench(&data(), &opts).expect("bench runs");
    let all: Vec<_> = runs.iter().collect();
    let s = summarize("light", &all);
    let report = simcli::police_bench::table(std::slice::from_ref(&s));
    eprintln!("{report}");
    for r in &runs {
        eprintln!(
            "{} seed {}: {:?} at {:.0} s, near {:?}, pursuit {:?}",
            r.cell.label(),
            r.seed,
            r.end,
            r.duration_s,
            r.first_near_s,
            r.pursuit_start_s
        );
    }
    // Police find the evader: a pursuit in most runs, a unit close in most runs, quickly.
    assert!(
        s.engaged >= 4,
        "pursuits in {} of 6 runs\n{report}",
        s.engaged
    );
    assert!(
        s.runs - s.never_near >= 4,
        "a unit within 100 m in {} of 6 runs\n{report}",
        s.runs - s.never_near
    );
    let near = s.first_near_med_s.expect("some unit came near");
    assert!(
        near <= 60.0,
        "median first unit within 100 m at {near:.0} s\n{report}"
    );
    // They keep it in sight for a fair share of the pursuit.
    assert!(
        s.los_frac >= 0.3,
        "line of sight {:.2}\n{report}",
        s.los_frac
    );
    // They do not wreck themselves or the city doing it.
    assert!(
        s.static_crashes_per_min <= 6.0,
        "police crashes into the city {:.2}/min\n{report}",
        s.static_crashes_per_min
    );
    assert!(
        s.civilian_per_min <= 3.0,
        "police hit civilians {:.2}/min\n{report}",
        s.civilian_per_min
    );
    assert!(
        s.stuck_per_min <= 12.0,
        "police stuck {:.2}/min\n{report}",
        s.stuck_per_min
    );
    // The evader is not a sitting duck: a capture takes a chase, not a few seconds. (This was
    // "not captured in every run"; since rows 131/140 the one grid run that used to time out
    // still in pursuit is captured, 6 of 6, at 25-78 s after the pursuit started.)
    let capture = s.capture_med_s.unwrap_or(f32::INFINITY);
    assert!(
        capture >= 15.0,
        "median capture {capture:.0} s after the pursuit started — is the evader driving?\n{report}"
    );
}

#[test]
fn police_bench_run_is_deterministic() {
    let c = cell(BenchCity::Grid, true, false, 2, EvaderSkill::Expert);
    let a = run_one(&data(), c, 5, DamageMode::Arcade, 30.0).expect("run");
    let b = run_one(&data(), c, 5, DamageMode::Arcade, 30.0).expect("run");
    assert_eq!(a, b);
}

#[test]
fn default_matrix_is_the_documented_twelve_cells() {
    let all = BenchCell::matrix();
    assert_eq!(all.len(), 72);
    let fast: Vec<_> = all.iter().filter(|c| !c.slow()).collect();
    assert_eq!(fast.len(), 12);
    // Every skill, level, city, phase and weather is in the default set.
    for k in EvaderSkill::ALL {
        assert!(fast.iter().any(|c| c.skill == k));
    }
    for l in 1..=3 {
        assert!(fast.iter().any(|c| c.level == l));
    }
    for city in BenchCity::ALL {
        assert!(fast.iter().any(|c| c.city == city));
    }
    assert!(fast.iter().any(|c| c.night) && fast.iter().any(|c| !c.night));
    assert!(fast.iter().any(|c| c.rain) && fast.iter().any(|c| !c.rain));
}
