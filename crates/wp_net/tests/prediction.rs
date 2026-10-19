//! Client prediction under a bad link: 120 ms RTT, 2 % loss, 30 ms jitter (netem-style
//! simulated transport). Reconciliation corrections must stay below 0.3 m RMS.

mod common;

use common::Harness;
use wp_contracts::Role;
use wp_net::server::ServerConfig;
use wp_net::sim_link::LinkConfig;

#[test]
fn corrections_stay_small_at_120ms_rtt_with_loss_and_jitter() {
    let mut h = Harness::new(ServerConfig::default(), 400.0);
    let bad = |seed| LinkConfig {
        latency_ms: 60.0,
        jitter_ms: 30.0,
        loss: 0.02,
        retransmit_ms: 200.0,
        seed,
    };
    h.add_peer("laggy", bad(1), bad(2));
    // A few well-connected bots for company.
    for i in 0..3 {
        h.add_peer(
            &format!("bot{i}"),
            LinkConfig::ideal(10.0),
            LinkConfig::ideal(10.0),
        );
    }
    h.wait_joined();
    for i in 0..4 {
        h.request_role(i, Role::Racer);
        h.give_bot(i, 50 + i as u64, 150.0);
    }
    h.run_secs(90.0);

    let s = h.peers[0].client.stats();
    println!(
        "rtt {:.1} ms jitter {:.1} ms, reconciles {}, mispredictions {}, rms {:.4} m, max {:.4} m, resets {}",
        s.rtt_ms,
        s.jitter_ms,
        s.reconciles,
        s.mispredictions,
        s.correction_rms_m(),
        s.max_correction_m,
        s.prediction_resets
    );
    assert!((100.0..160.0).contains(&s.rtt_ms), "rtt {}", s.rtt_ms);
    assert!(s.reconciles > 1500, "own-state reconciles {}", s.reconciles);
    assert!(
        s.correction_rms_m() < 0.3,
        "RMS correction {}",
        s.correction_rms_m()
    );
    assert!(
        (s.mispredictions as f64) < 0.05 * s.reconciles as f64,
        "{} of {} reconciles mispredicted",
        s.mispredictions,
        s.reconciles
    );
    // The bad link really was bad.
    assert!(h.peers[0].down.dropped > 0 && h.peers[0].up.dropped > 0);
}
