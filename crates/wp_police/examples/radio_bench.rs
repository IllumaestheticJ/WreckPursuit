//! `cargo run --release -p wp_police --example radio_bench -- <scenario.ron> [--text-tiers] [--log N]`
//!
//! Radio priority bench (owner playtest row 144). Runs a police scenario and prints:
//! - spoken lines per minute per tier (1 actionable .. 5 routine) at L0 / L1 / L2 / L3+ (the
//!   highest wanted level of any racer while the line was spoken; minutes are the time spent at
//!   that level);
//! - deployment lines per minute ("en route", "responding", "in pursuit", "moving to
//!   intercept", "sending unit", "setting up a roadblock", ...) at L0 / L1 / L2+;
//! - the delay from a roadblock / spike deploy decision (`RoadblockPlaced` / `SpikesPlaced`)
//!   to the first spoken line about it, by the racer's level at the decision.
//!
//! Tiers come from the radio log (`RadioLine::tier`). `--text-tiers` classifies lines from
//! their text instead, with the same rules as the tier table in `docs/integration/police-radio-v2.md`:
//! that is how the "before" numbers were taken on a base without priority tiers. `--log N`
//! prints the first N runs' transcripts. With `WP_RADIO_DEBUG` set, deploy decisions spoken
//! more than 1 s late are listed on stderr.

use std::collections::BTreeMap;

use wp_contracts::SimEvent;
use wp_police::{PoliceEvent, PoliceEventExt, PoliceSystem};

/// Text classifier for logs without tiers (and a cross-check of the tier tags).
fn text_tier(t: &str) -> u8 {
    // Whole words only ("downtown" is not "down", "pursuit" is not "pit").
    let words: String = t
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '#' || c == '\'' || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let l = format!(
        " {} ",
        words.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    let has = |k: &[&str]| k.iter().any(|w| l.contains(&format!(" {w} ")));
    if has(&[
        "roadblock",
        "deploying spikes",
        "spike strip",
        "spikes going",
        "air unit requested",
    ]) || has(&[
        "air unit inbound",
        "overhead",
        "eyes on racer",
        "spotlight on",
        "reacquired",
    ]) || has(&[
        "moving to intercept",
        "lost visual on racer",
        "losing him",
        "ahead of racer",
    ]) {
        return 1;
    }
    if has(&[
        "10-80",
        "now level",
        "down to level",
        "taking over",
        "authorised",
        "authorized",
    ]) || has(&[
        "in custody",
        "we've lost",
        "terminated",
        "went to ground",
        "i'm on it",
    ]) || has(&["blew past me", "captain", "vehicle disabled"])
    {
        return 2;
    }
    if has(&[
        "hit",
        "rammed",
        "down",
        "wrecked",
        "pit",
        "spun",
        "ems",
        "front end",
        "crash",
    ]) || has(&[
        "took out",
        "lost my",
        "contact",
        "out of the chase",
        "bent wheel",
    ]) {
        return 3;
    }
    if has(&[
        "caller reports",
        "camera",
        "suspicious",
        "checking",
        "searching",
        "last seen",
    ]) || has(&[
        "lost visual",
        "eyes on him",
        "tunnel",
        "reckless driver",
        "units responding",
    ]) || has(&[
        "still on",
        "unavailable",
        "on his bumper",
        "boxing",
        "requesting backup",
    ]) {
        return 4;
    }
    5
}

fn is_deploy(t: &str) -> bool {
    let l = t.to_lowercase();
    // Call announcements ("caller reports ..., Unit 2 responding") are investigation, not
    // deployment chatter; the one "units responding" summary is counted on its own.
    if l.contains("caller reports") || l.contains("camera") || l.contains("units responding") {
        return false;
    }
    [
        "en route",
        "responding",
        "in pursuit of",
        "moving to intercept",
        "sending unit",
        "more units",
        "setting up a roadblock",
        "deploying spikes",
        "requesting backup",
        "air unit requested",
        "searching",
    ]
    .iter()
    .any(|w| l.contains(w))
}

fn bucket(level: u8) -> usize {
    (level as usize).min(3)
}

#[derive(Default)]
struct Acc {
    /// Seconds per level bucket (L0, L1, L2, L3+).
    secs: [f64; 4],
    /// Lines per [bucket][tier 1..=5].
    lines: [[u32; 6]; 4],
    deploy: [u32; 4],
    /// Deploy decision → spoken delays by level at the decision.
    delays: BTreeMap<u8, Vec<f32>>,
    /// Open decisions: (time, level, keyword).
    open: Vec<(f32, u8, &'static str)>,
    /// Decisions of finished runs that were never spoken.
    unspoken: usize,
    last_t: f32,
    run: u32,
    log: Vec<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .expect("usage: radio_bench <scenario.ron> [--text-tiers]");
    let text_tiers = args.iter().any(|a| a == "--text-tiers");
    let log_runs: u32 = args
        .iter()
        .position(|a| a == "--log")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let sc = wp_police::scenario::load_scenario(std::path::Path::new(path)).expect("scenario");
    let cfg = wp_police::PoliceConfig::builtin();
    let mut acc = Acc::default();
    let mut hook = |p: &PoliceSystem, out: &[SimEvent]| {
        let t = p.now().secs();
        if t < acc.last_t {
            acc.run += 1;
            acc.unspoken += acc.open.len();
            acc.open.clear();
            if acc.run < log_runs {
                acc.log.push(format!("--- run {}", acc.run + 1));
            }
        }
        let dt = if t >= acc.last_t { t - acc.last_t } else { t };
        acc.last_t = t;
        let level = p.racers_iter().map(|r| r.level.number()).max().unwrap_or(0);
        acc.secs[bucket(level)] += dt as f64;
        let mut radio_k = 0usize;
        let new_lines: Vec<_> = {
            let n = out
                .iter()
                .filter(|e| {
                    matches!(
                        PoliceEvent::from_sim_event(e),
                        Some(PoliceEvent::Radio { .. })
                    )
                })
                .count();
            let log: Vec<_> = p.radio_log().collect();
            log[log.len().saturating_sub(n)..]
                .iter()
                .map(|l| (*l).clone())
                .collect()
        };
        for e in out {
            let Some(pe) = PoliceEvent::from_sim_event(e) else {
                continue;
            };
            match pe {
                PoliceEvent::RoadblockPlaced { racer, .. } => {
                    let lv = p.wanted_level(racer).number();
                    acc.open.push((t, lv, "roadblock"));
                }
                PoliceEvent::SpikesPlaced { racer, .. } => {
                    let lv = p.wanted_level(racer).number();
                    acc.open.push((t, lv, "spike"));
                }
                PoliceEvent::Radio { text, .. } => {
                    let line = new_lines.get(radio_k);
                    radio_k += 1;
                    let tier = if text_tiers {
                        text_tier(&text)
                    } else {
                        line.map_or(text_tier(&text), |l| l.tier.number())
                    };
                    acc.lines[bucket(level)][tier as usize] += 1;
                    if is_deploy(&text) {
                        acc.deploy[bucket(level)] += 1;
                    }
                    let l = text.to_lowercase();
                    let mut k = 0;
                    while k < acc.open.len() {
                        let (t0, lv, key) = acc.open[k];
                        // A line more than 20 s later is about another tool.
                        if t - t0 > 20.0 {
                            acc.unspoken += 1;
                            acc.open.remove(k);
                        } else if l.contains(key) && !l.contains("spikes hit") {
                            if std::env::var("WP_RADIO_DEBUG").is_ok() && t - t0 > 1.0 {
                                eprintln!("late {:.1} s: decided {t0:.1} L{lv}: {text}", t - t0);
                            }
                            acc.delays.entry(lv).or_default().push(t - t0);
                            acc.open.remove(k);
                        } else {
                            k += 1;
                        }
                    }
                    if acc.run < log_runs {
                        let cut = line.is_some_and(|l| l.cut_in);
                        acc.log.push(format!(
                            "{t:>7.1} s  L{level}  T{tier}{}  {text}",
                            if cut { " cut" } else { "    " }
                        ));
                    }
                }
                _ => {}
            }
        }
    };
    let _ = wp_police::scenario::run_scenario_with_hook(&sc, &cfg, &mut hook);
    for l in &acc.log {
        println!("{l}");
    }
    println!(
        "\n{} ({} runs), tiers from {}",
        sc.name,
        sc.runs.max(1),
        if text_tiers { "text" } else { "the radio log" }
    );
    println!("\nlines/min by tier | L0 | L1 | L2 | L3+");
    println!("---|---|---|---|---");
    for tier in 1..=5 {
        let row: Vec<String> = (0..4)
            .map(|b| per_min(acc.lines[b][tier], acc.secs[b]))
            .collect();
        println!("T{tier} | {}", row.join(" | "));
    }
    let all: Vec<String> = (0..4)
        .map(|b| per_min(acc.lines[b].iter().sum(), acc.secs[b]))
        .collect();
    println!("all | {}", all.join(" | "));
    let mins: Vec<String> = (0..4)
        .map(|b| format!("{:.1}", acc.secs[b] / 60.0))
        .collect();
    println!("(minutes) | {}", mins.join(" | "));
    println!("\ndeploy lines/min | L0 | L1 | L2+");
    println!("---|---|---|---");
    let hot_s = acc.secs[2] + acc.secs[3];
    println!(
        "deploy | {} | {} | {}",
        per_min(acc.deploy[0], acc.secs[0]),
        per_min(acc.deploy[1], acc.secs[1]),
        per_min(acc.deploy[2] + acc.deploy[3], hot_s)
    );
    println!("\ndeploy decision → spoken | n | median s | max s | never spoken");
    println!("---|---|---|---|---");
    let unspoken = acc.unspoken + acc.open.len();
    for lv in [1u8, 2, 3, 4, 5] {
        let Some(v) = acc.delays.get_mut(&lv) else {
            println!("L{lv} | 0 | - | - |");
            continue;
        };
        v.sort_by(|a, b| a.total_cmp(b));
        println!(
            "L{lv} | {} | {:.1} | {:.1} |",
            v.len(),
            v[v.len() / 2],
            v[v.len() - 1]
        );
    }
    println!("(decisions never spoken: {unspoken})");
}

fn per_min(n: u32, secs: f64) -> String {
    if secs < 1.0 {
        "-".into()
    } else {
        format!("{:.1}", n as f64 * 60.0 / secs)
    }
}
