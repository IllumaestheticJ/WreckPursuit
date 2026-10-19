//! 32 bot clients in one instance: every client's down-link stays within the 40 kB/s budget,
//! and the server net tick stays within its time budget (asserted in release builds only).

mod common;

use std::time::Instant;

use common::Harness;
use wp_contracts::Role;
use wp_net::server::ServerConfig;
use wp_net::sim_link::LinkConfig;
use wp_net::DOWNLINK_BUDGET_BPS;

#[test]
fn thirty_two_bots_within_bandwidth_and_tick_budget() {
    let mut h = Harness::new(ServerConfig::default(), 120.0);
    for i in 0..32u64 {
        let link = LinkConfig {
            latency_ms: 30.0,
            jitter_ms: 10.0,
            loss: 0.01,
            retransmit_ms: 100.0,
            seed: i,
        };
        h.add_peer(
            &format!("bot{i}"),
            link,
            LinkConfig {
                seed: 100 + i,
                ..link
            },
        );
    }
    h.wait_joined();
    for i in 0..32 {
        // 12 police requests; the 40 % cap admits 12 of 32.
        let role = if i % 8 < 3 {
            Role::PoliceChase
        } else {
            Role::Racer
        };
        h.request_role(i, role);
        h.give_bot(i, 1000 + i as u64, 100.0);
    }
    h.run_secs(2.0);

    let mut tick_times = Vec::new();
    for _ in 0..30 * 60 {
        let t0 = Instant::now();
        h.step();
        tick_times.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    // `step` includes 32 client updates; time the server alone separately.
    let mut server_times = Vec::new();
    for _ in 0..600 {
        let t0 = Instant::now();
        h.server.net_tick();
        server_times.push(t0.elapsed().as_secs_f64() * 1000.0);
        let _ = h.server.drain_outgoing();
    }
    server_times.sort_by(f64::total_cmp);
    let avg = server_times.iter().sum::<f64>() / server_times.len() as f64;
    let p99 = server_times[server_times.len() * 99 / 100];

    let worst = h
        .peers
        .iter()
        .flat_map(|p| p.down_per_second.iter().skip(3).copied())
        .max()
        .unwrap();
    let mean = h
        .peers
        .iter()
        .flat_map(|p| p.down_per_second.iter().skip(3).copied())
        .sum::<u64>() as f64
        / h.peers
            .iter()
            .map(|p| p.down_per_second.len().saturating_sub(3))
            .sum::<usize>() as f64;
    let stats = h.server.stats();
    println!(
        "32 bots: down-link mean {:.0} B/s, worst second {worst} B/s (budget {DOWNLINK_BUDGET_BPS}); \
         server net tick avg {avg:.3} ms p99 {p99:.3} ms; police {}",
        mean,
        h.server.roles().police_count()
    );
    assert_eq!(stats.players, 32);
    assert_eq!(h.server.roles().police_count(), 12);
    assert!(
        worst as usize <= DOWNLINK_BUDGET_BPS,
        "worst second {worst} B/s"
    );
    assert!(stats.downlink_bps_max <= DOWNLINK_BUDGET_BPS as f64);
    // Every client sees most of the other 31 cars.
    for p in &h.peers {
        assert!(
            p.client.remotes().len() >= 20,
            "{}",
            p.client.remotes().len()
        );
    }
    if !cfg!(debug_assertions) {
        // Half a 60 Hz frame for the whole instance (4 physics ticks + 32 clients' output).
        assert!(avg < 8.0, "server net tick avg {avg} ms");
    }
}
