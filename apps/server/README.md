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
| `--city` | `city` | — | city file; checked to exist, not consumed until `wp_world` has a loader |
| `--seed` | `seed` | 1 | instance seed (sim RNG, reconnect tokens, bots) |
| `--time-ratio` | `time_ratio` | 10 | in-game seconds per real second |
| `--time-of-day` | `time_of_day_h` | 21 | start hour (night races cost more) |
| `--mode` | `mode` | arcade | `arcade` or `sim` damage tables |
| `--bots N` | `bots` | 0 | connect N scripted websocket clients (drive a random route, crash sometimes; every third asks for police) |
| `--arena R` | `arena_radius_m` | — | crash arena of radius R m (`wp_net::testkit::ArenaSim`: wall + car contacts, toy lattice damage) instead of the plain `wp_sim` world. Bots then use the toy lattice; browser clients use `wp_damage` and will resync often |
| `--data` | `data_dir` | data | data directory |

## Admin endpoint
`GET /status` → JSON: `players, connected, player_list[{id,name}], tick, tick_rate_hz,
ms_per_tick_avg, ms_per_tick_max, downlink_bps_avg, downlink_bps_max, bytes_sent_total,
inputs_missing, inputs_rejected, decode_errors, lattice_resyncs, uptime_s` (refreshed every
second). Anything else → 404.

## Logs
One JSON object per line on stdout: `{"ts": unix_seconds, "event": …, …fields}`. Events:
`listening`, `connect`, `disconnect`, `handshake_failed`, `tick_overrun`, `status` (every
10 s), `city_not_loaded`, `bot_connect_failed`.

## Tests

```sh
cargo test -p wp_server
```
Real websockets on ephemeral ports: a native client joins, drives and is reported by
`/status`; `--bots 6` connects six scripted clients within budget; config parsing.

## Measurements (release, cloud container)
32 websocket bots in the same process, 120 m arena: 60.0 net ticks/s, 0.94 ms per net tick
average (4.2 ms worst), worst client down-link 26 kB/s (budget 40 kB/s).

## Known gaps
- One instance per process; no matchmaking/instance directory.
- Accounts (cash, rep, cop rank) live in memory per instance (`wp_economy::InMemoryStore`).
- The admin endpoint is plain HTTP without auth: bind it to localhost in production.
