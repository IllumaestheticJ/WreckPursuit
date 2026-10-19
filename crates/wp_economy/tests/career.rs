//! Prompt 08 scenario: 20 simulated nights of a mid-skill racer (scripted outcomes) end
//! solvent in Arcade and marginal in Simulation. Results over 64 seeds are recorded in the
//! README ("Tuning results"); run with `--nocapture` to print them.

use wp_contracts::DamageMode;
use wp_economy::career::{simulate_career, CareerConfig, CareerReport};
use wp_economy::Economy;

const SEEDS: u64 = 64;

struct Summary {
    start: i64,
    net_worth: Vec<i64>,
    cash: Vec<i64>,
}

impl Summary {
    fn pct(&self, p: usize) -> i64 {
        self.net_worth[(self.net_worth.len() - 1) * p / 100]
    }
    fn mean(&self) -> i64 {
        self.net_worth.iter().sum::<i64>() / self.net_worth.len() as i64
    }
}

fn run(e: &Economy, mode: DamageMode) -> (Summary, Vec<CareerReport>) {
    let reports: Vec<CareerReport> = (1..=SEEDS)
        .map(|s| simulate_career(e, &CareerConfig::mid_skill(mode, s)))
        .collect();
    let mut net_worth: Vec<i64> = reports.iter().map(|r| r.end_net_worth).collect();
    net_worth.sort();
    let cash = reports.iter().map(|r| r.end_cash).collect();
    let start = reports[0].start_net_worth;
    (
        Summary {
            start,
            net_worth,
            cash,
        },
        reports,
    )
}

fn print(mode: DamageMode, s: &Summary, r: &[CareerReport]) {
    let avg = |f: &dyn Fn(&CareerReport) -> i64| r.iter().map(f).sum::<i64>() / r.len() as i64;
    println!(
        "{mode:?}: start {} | end net worth p10 {} p25 {} median {} p75 {} mean {} | in debt {}/{} \
         | avg earned {} fees {} repairs {} penalties {} | avg captures {:.2} lost {:.2} scrapped {:.2} floor {:.2}",
        s.start, s.pct(10), s.pct(25), s.pct(50), s.pct(75), s.mean(),
        s.cash.iter().filter(|c| **c < 0).count(), s.cash.len(),
        avg(&|x| x.total_earned), avg(&|x| x.total_entry_fees), avg(&|x| x.total_repairs), avg(&|x| x.total_penalties),
        r.iter().map(|x| x.captures as f32).sum::<f32>() / r.len() as f32,
        r.iter().map(|x| x.cars_lost as f32).sum::<f32>() / r.len() as f32,
        r.iter().map(|x| x.cars_scrapped as f32).sum::<f32>() / r.len() as f32,
        r.iter().map(|x| x.floor_cars as f32).sum::<f32>() / r.len() as f32,
    );
}

#[test]
fn twenty_nights_solvent_in_arcade_marginal_in_simulation() {
    let e = Economy::embedded();
    let (arc, arc_r) = run(&e, DamageMode::Arcade);
    let (sim, sim_r) = run(&e, DamageMode::Simulation);
    print(DamageMode::Arcade, &arc, &arc_r);
    print(DamageMode::Simulation, &sim, &sim_r);
    let start = arc.start as f32;
    assert_eq!(arc.start, sim.start);

    // Arcade: solvent. Nobody ends in debt, the typical racer grows their net worth a lot, and
    // even the unlucky 10 % do not end below where they started.
    assert!(
        arc.cash.iter().all(|c| *c >= 0),
        "an Arcade career ended in debt"
    );
    assert!(
        arc.pct(50) as f32 >= 1.6 * start,
        "Arcade median {}",
        arc.pct(50)
    );
    assert!(
        arc.pct(10) as f32 >= 1.0 * start,
        "Arcade p10 {}",
        arc.pct(10)
    );

    // Simulation: marginal. The typical racer roughly breaks even; a real fraction go backwards.
    let med = sim.pct(50) as f32;
    assert!(
        (0.7 * start..=1.3 * start).contains(&med),
        "Simulation median {med}"
    );
    let mean = sim.mean() as f32;
    assert!(
        (0.7 * start..=1.3 * start).contains(&mean),
        "Simulation mean {mean}"
    );
    assert!(sim.pct(25) < sim.start, "Simulation p25 should be a loss");
    assert!(sim.pct(50) as f32 <= 0.6 * arc.pct(50) as f32);
}

#[test]
fn careers_are_deterministic() {
    let e = Economy::embedded();
    for mode in [DamageMode::Arcade, DamageMode::Simulation] {
        let a = simulate_career(&e, &CareerConfig::mid_skill(mode, 11));
        let b = simulate_career(&e, &CareerConfig::mid_skill(mode, 11));
        assert_eq!(a, b);
    }
}

/// The same 20-night career on each street-racing roster car (prices from `data/cars`).
/// Arcade must stay solvent on every car; Simulation is reported only (it is meant to be
/// marginal, and expensive cars are meant to be riskier).
#[test]
fn twenty_nights_on_roster_cars() {
    const CARS: &[&str] = &[
        "vireo_xs",
        "kestrel_ts",
        "shiro_turbo",
        "lindwurm_4t",
        "sperber_32",
        "hakone_gt",
        "ironside_440",
        "rampart_50",
        "lucciola_350",
    ];
    let e = Economy::embedded();
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/cars");
    for id in CARS {
        let car = wp_contracts::load_car(root.join(format!("{id}.json"))).unwrap();
        let spec = wp_economy::CarSpec::from(&car.def);
        let mut line = format!("{id:14}");
        for mode in [DamageMode::Arcade, DamageMode::Simulation] {
            let mut reports: Vec<CareerReport> = (1..=32)
                .map(|s| {
                    let mut cfg = CareerConfig::mid_skill(mode, s);
                    cfg.car = spec.clone();
                    simulate_career(&e, &cfg)
                })
                .collect();
            reports.sort_by_key(|r| r.end_net_worth);
            let start = reports[0].start_net_worth;
            let med = reports[reports.len() / 2].end_net_worth;
            let p10 = reports[reports.len() / 10].end_net_worth;
            let debt = reports.iter().filter(|r| r.end_cash < 0).count();
            line += &format!(
                " | {mode:?} start {start:7} median {med:7} ({:+.0}%) p10 {p10:7} debt {debt:2}/32",
                100.0 * (med - start) as f32 / start as f32
            );
            if mode == DamageMode::Arcade {
                assert!(
                    med as f32 >= 1.2 * start as f32,
                    "{id}: Arcade median {med} vs start {start}"
                );
            }
        }
        println!("{line}");
    }
}
