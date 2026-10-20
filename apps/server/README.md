# wp_server (`wp-server` binary)

**Owned by workstream 07 (multiplayer netcode).**

Headless authoritative server for one city instance: `wp_net::server::ServerInstance` on a
60 Hz game-loop thread (240 Hz physics inside), websocket transport, an admin HTTP status
endpoint, JSON-lines logs and optional scripted bots.

```sh
just server 8080                                   # = cargo run -p wp_server --bin wp-server -- --port 8080
cargo run --release -p wp_server --bin wp-server -- --bots 32 --arena 120   # load test
curl localhost:8081/status                         # admin
```

Then open the browser client: `just web` and visit
`http://localhost:5173/net.html?server=ws://localhost:8080&name=me`.

## Configuration
Flags (or the same keys in a TOML file passed with `--config`, flags win):

| flag | TOML key | default | meaning |
|---|---|---|---|
| `--port` | `port` | 8080 | websocket port (0 = any) |
| `--admin-port` | `admin_port` | 8081 | admin HTTP port, `GET /status` |
| `--bind` | `bind` | 0.0.0.0 | bind address |
| `--max-players` | `max_players` | 32 | instance size |
| `--city` | `city` | — | city: a `.road.json` / `.road.bin` file, `procedural` or `vesper_v2` (Port Vesper v2: terrain, elevation, buildings); physical + kinematic traffic, AI police, players spawn on lanes (settled onto the road / terrain) |
| `--weather KIND` | `weather` | — | with `--city`: server weather starting in `clear` / `overcast` / `light_rain` / `heavy_rain` / `fog` (schedule runs), sent to clients on join and on change (protocol v4) |
| `--seed` | `seed` | 1 | instance seed (sim RNG, reconnect tokens, bots) |
| `--time-ratio` | `time_ratio` | 10 | in-game seconds per real second |
| `--time-of-day` | `time_of_day_h` | 21 | start hour (night races cost more) |
| `--mode` | `mode` | arcade | `arcade` or `sim` damage tables |
| `--bots N` | `bots` | 0 | connect N scripted websocket clients (drive a random route, crash sometimes; every third asks for police) |
| `--arena R` | `arena_radius_m` | — | crash arena of radius R m (`wp_net::testkit::ArenaSim`: wall + car contacts, toy lattice damage) instead of the plain `wp_sim` world. Bots then use the toy lattice; browser clients use `wp_damage` and will resync often |
| `--data` | `data_dir` | data | data directory |

## Meta service (matchmaking, accounts)
Set `WP_META_URL` and `WP_META_INSTANCE_KEY` (optional `WP_META_INSTANCE_NAME`,
`WP_META_REGION`, `WP_META_CITY`, `WP_META_BRACKET`, `WP_META_PUBLIC_ADDRESS`) and the server
links to `apps/meta` (`src/meta.rs`, steps 1–7 of `docs/integration/meta.md`):

- registers at startup (a bad URL or key stops the server);
- admits only players whose `Hello.join_token` meta verifies; they join under their meta
  name, in the role meta reserved, and get meta's reconnect token in `Welcome`; a missing or
  forged token is rejected. The u64 in-window reattach needs no new token;
- heartbeats population per role and the connected accounts every second; heartbeat kicks
  disconnect the player for good;
- **economy write-through**: each verified player's account is loaded from meta and becomes
  the instance's copy (changes made while it loads wait); every stake, payout and rep change
  is one ledger event, committed to meta in order with one commit in flight per account. On
  `seq_conflict` (meta changed the account meanwhile, e.g. an admin adjustment) the account
  is reloaded and the server's unconfirmed changes are applied again on top as new events;
- **race reports**: every finished lobby race (after its payouts) with each meta player's
  position, time and clean flag (no damaging contact, no violation beyond speeding);
  `report_key` = instance + race id; route id = hash of laps + checkpoints;
- reports every pursuit of a player racer (highest wanted level, bounty, escape/capture,
  player cops credited with the capture and assists).

Not wired: territory reports (no district-control source yet).
Without `WP_META_URL` the server runs standalone as before.

## Admin endpoint
`GET /status` → JSON: `players, connected, player_list[{id,name}], tick, tick_rate_hz,
ms_per_tick_avg, ms_per_tick_max, downlink_bps_avg, downlink_bps_max, bytes_sent_total,
inputs_missing, inputs_rejected, decode_errors, lattice_resyncs, uptime_s` (refreshed every
second). Anything else → 404.

## Logs
One JSON object per line on stdout: `{"ts": unix_seconds, "event": …, …fields}`. Events:
`listening`, `connect`, `disconnect`, `handshake_failed`, `tick_overrun`, `status` (every
10 s), `city_loaded`, `bot_connect_failed`; with meta: `meta_registered`, `meta_join`,
`meta_join_refused`, `meta_kick`, `meta_heartbeat_failed`, `meta_account_missing`,
`meta_account_load_failed`, `meta_commit_failed`, `meta_race_report_failed`,
`meta_pursuit_report_failed`.

## Tests

```sh
cargo test -p wp_server
```
Real websockets on ephemeral ports: a native client joins, drives and is reported by
`/status`; `--bots 6` connects six scripted clients within budget; config parsing;
`tests/meta_link.rs` runs the real meta service in-process: a matchmade player joins with its
join token (meta name, reserved role, meta reconnect token), forged and missing tokens are
rejected, an admin kick in meta disconnects the player; two matchmade racers run a lobby
sprint: stakes and payouts land in meta, an admin adjustment made mid-race conflicts with
the payout commit and both survive (reload + re-apply), the race is rated on meta's
`clean_rating` board.

The browser check `just web-e2e-net` builds and starts `wp-server`, opens two tabs of
`net.html` and crashes their cars into each other (see `crates/wp_net/README.md`).

## Measurements (release, cloud container)
32 websocket bots in the same process, 120 m arena: 60.0 net ticks/s, 0.94 ms per net tick
average (4.2 ms worst), worst client down-link 26 kB/s (budget 40 kB/s).

## Known gaps
- One instance per process (meta provides the directory and matchmaking).
- Without meta, accounts (cash, rep, cop rank) live in memory per instance
  (`wp_economy::InMemoryStore`).
- The admin endpoint is plain HTTP without auth: bind it to localhost in production.
