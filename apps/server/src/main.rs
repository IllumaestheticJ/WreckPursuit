//! `wp-server` — headless authoritative server for one city instance (workstream 07).
//! Run `wp-server --help` for flags; see `apps/server/README.md`.

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", wp_server::USAGE);
        return;
    }
    let cfg = match wp_server::AppConfig::from_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("wp-server: {e}\n\n{}", wp_server::USAGE);
            std::process::exit(2);
        }
    };
    let log = wp_server::json_log();
    let running = match wp_server::run(cfg, log.clone()).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("wp-server: {e}");
            std::process::exit(1);
        }
    };
    // Periodic status line; the admin endpoint has the same data on demand.
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        let s = running.status.lock().map(|s| s.clone()).unwrap_or_default();
        log(
            "status",
            vec![
                ("players", s.stats.players.to_string()),
                ("tick", s.stats.tick.to_string()),
                ("tick_rate_hz", format!("{:.1}", s.tick_rate_hz)),
                ("ms_per_tick_avg", format!("{:.3}", s.ms_per_tick_avg)),
                (
                    "downlink_bps_max",
                    format!("{:.0}", s.stats.downlink_bps_max),
                ),
            ],
        );
    }
}
