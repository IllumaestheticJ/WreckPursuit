# Multiplayer netcode integration notes

- **Workstream / prompt:** 07 — multiplayer netcode
- **Branch:** `ws/net`
- **Crates / folders owned:** `crates/wp_net`, `apps/server`, `apps/testbed-web/src/net/` (+ `net.html`)
- **Contract version built against:** 0.3.0 (`main`, lead round 8); wire protocol `PROTOCOL_VERSION = 4`

## Exposes

### `wp_net` (default features — wasm-clean)
- `server::ServerInstance<S: NetSim>` — sans-IO authoritative instance:
  `new(ServerConfig, S)`, `connect() -> ConnId`, `receive(ConnId, &[u8])`, `disconnect(ConnId)`,
  `net_tick()` (4 physics ticks + output), `drain_outgoing() -> Vec<Outgoing>`, `stats()`,
  `roles()`, `accounts_mut()`, `vehicle_of(PlayerId)`; **meta join gate (MC-6)**:
  `ServerConfig::require_join_token`, `take_join_requests() -> Vec<JoinRequest>`,
  `approve_join(conn, display_name, meta_reconnect_token)`, `reject_join(conn, reason)`.
  Forwards typed `SimEvent::{Police, Race, Economy, WeatherChanged}`; lobby money changes are
  ledger `EconomyEvent`s (`StakeCharged` / `RacePayout` / `RaceRep`).
- `netsim::NetSim` — what the server needs from a simulation (spawn/despawn, per-tick input,
  step, states, lattice nodes/rest, events, **applied contacts**, time of day, mode, AI kind,
  wanted level, damage step count, owned-vehicle kind). `netsim::WpSim` implements it over
  `wp_sim::Sim`: contact quantiser + applied-contact drain (NET-N1), real despawn (N3),
  `VehicleKind` → `EntityKind`, wanted level from `Sim::police()`; **v0.3.0**: kinematic
  `traffic_agents()` (`TrafficAgentSnapshot`), optional server `weather()` (W-5) with
  `WeatherChanged` events, `captain_command` / `set_human_captain` into `wp_police`,
  `police_unit(EntityId)`.
  `WpSim::with_city(config, cars, CitySetup { world, police_car, traffic_car })`,
  `WpSim::{set_weather, enable_weather, weather_sim, weather_sim_mut}`,
  `netsim::{server_world_config, lane_spawn_points, city_weather}`.
- `lattice::damage_relevant(contacts, tuning, mode)` — contacts `wp_damage` would act on.
- `client::Client` — sans-IO client: `connect`, `connect_with_join_token`, `receive`,
  `update(now_ms, &VehicleInput)`, `drain_outgoing`, `render(now_ms)`, `render_traffic(now_ms)`,
  `traffic()`, `request_role`, `captain_command(CaptainCommand) -> seq`,
  `race_command(RaceCommand)`, `send_reliable`, `bye`, `stats()`; state: `game_events`
  (typed `SimEvent`s to drain), `wanted`, `captain_replies`, `weather`,
  `meta_reconnect_token`, `unit_order` (contract `UnitOrder`), lobby/captain/account.
- `predict::{Predictor, PredictedVehicle, RigVehicle}`, `interp::StateBuffer`,
  `timesync::TimeSync`, `interest::*`, `roles::RoleManager`, `race::RaceLobby`.
- `protocol::{ClientMsg, ServerMsg, PLAYER_UNIT_BASE, unit_order_code, …}` + codec (contract
  types as `typed<T>` = postcard), `quant::{…, QTrafficAgent}`, `snapshot::*`,
  `lattice::{lattice_hash, LatticeSnapshot, ReplicaDamage}` (impl for `wp_damage::DamageModel`).
- `sim_link` (netem-style test transport), `bot::BotDriver`, `testkit::{ArenaSim, ToyLattice,
  load_data}`.
- Protocol spec with byte layouts: `crates/wp_net/README.md`.

### `wp_net` feature `native` (tokio — never from `wp_wasm`)
`native::{spawn_game_loop, serve_ws, connect_ws, WsTransport, StatusReport, IoEvent, OutFrame}`.

### `apps/server` (`wp-server`)
Flags/TOML config (port, admin port, instance size, `--city FILE|procedural` (loads the road
network, physical traffic, AI police; players spawn on lanes), seed, time ratio, time of day,
mode, `--bots N`, `--arena R`, data dir), `GET /status` admin JSON, JSON-lines logs. Library
API `wp_server::{run, AppConfig, json_log, null_log, status_json}` for tests/tools.

### `wp_wasm` functions added (in a separate module, `crates/wp_wasm/src/net.rs`)
`net_create, net_connect, net_connect_join, net_receive, net_update, net_take_outgoing,
net_render_json, net_traffic_json, net_events_json, net_status_json, net_stats_json,
net_captain_json, net_request_role, net_captain_command_json (contract CaptainCommand JSON →
seq), net_race_create_json, net_race_command_json, net_race_join, net_race_start,
net_race_leave, net_race_invite, net_bye` (round 8: `net_captain_order_json` removed).
Table with signatures in the module docs. `ws/testbed` (04) is active and rewrote
`wp_wasm/src/lib.rs`; to keep the merge trivial my only edits to shared files are
`pub mod net;` in `lib.rs`, one doc line, and `wp_net.workspace = true` in `Cargo.toml`.
`net.rs` embeds its own copy of the car/mode data; it can switch to 04's `data.rs` after merge.

### Browser client (`apps/testbed-web`)
`net.html` + `src/net/{main,wasm,connection,keyboard,overlay,rolePicker,captainMap}.ts`:
connect + reconnect-with-token, role picker, captain-offer prompt, race lobby, captain map
(2D canvas: units, incidents, heat, right-click command radial), debug overlay (RTT,
misprediction count, correction magnitude, bandwidth, lattice hash stats; round 8: weather,
wanted levels, typed unit order, police radio lines, traffic count), grey boxes for kinematic
traffic, captain radial sending typed commands (pursue / intercept / roadblock / spikes /
helicopter / priority / call off / patrol / disengage) with the last reply shown, and
`?join=<token>` for a meta join token. Separate page so
04's `main.ts` is untouched; `vite.config.ts` gained a `rollupOptions.input` entry for
`net.html`. Screenshots: `docs/media/net_racer.png`, `docs/media/net_captain_order.png`.

## Needs

| # | From | What | Why | Workaround now |
|---|---|---|---|---|
| NET-N1 | `wp_sim` (lead) | Applied-contacts drain + quantise hook. | Bit-identical damage replicas. | **Done (round 6b)**: `WpSim` installs the filter and drains; replicated on the real sim (`tests/city_e2e.rs`). |
| NET-N2 | `wp_vehicle` (02) | Full-state rewind of one vehicle for client prediction: `Vehicle::set_state(&VehicleState)` (added to the stub on this branch) or better an opaque `snapshot() -> Vec<u8>` / `restore(&[u8])` covering engine, clutch, wheel spin, filters; plus a client-side single-car stepper (like 02's `rig::Rig`: own Rapier world, flat ground) to implement `wp_net::predict::PredictedVehicle`. | Rewind-and-replay must reproduce the server exactly. `ws/vehicle` moves the chassis into Rapier, so `set_state` must also place the body. | **Done (run 4)** on A's delivery: `predict::RigVehicle` over `Rig`, exact rewind via `Rig::restore(VehicleSnapshot)` sent 5 Hz as `Snapshot.own_full`; `Rig::set_state` (mine, additive) is the fallback between snapshots. |
| NET-N3 | `wp_sim` (lead) | `Sim::despawn_vehicle`, `place_vehicle`. | Players leave/switch roles. | **Done (round 6b)**: `WpSim` despawns for real (no more parking). |
| NET-N4 | `wp_sim` (lead) | `Sim::mode_table()`, per-vehicle kind. | Welcome mode; interest needs AI kinds. | **Done (round 6b)**: `VehicleKind` → `EntityKind` (`AiPolice`, `Traffic`, new `AiRacer`). |
| NET-N5 | `wp_damage` (03) | Full `DamageModel::snapshot() -> Vec<u8>` / `restore(&[u8])` (positions **and** prev/vel, beam rest lengths / plastic strain, broken beams, impactors, groups, components, `calls`, `time_s`, awake/quiet). Smaller alternative: a public `step_count()` and a restore that also takes beam rest lengths. | Today a lattice snapshot restores node positions only. Measured in `city_e2e`: **every** remaining hash mismatch (63 of 2386 checks) is on a replica restored from a dented snapshot (a car that entered a client's view already damaged, or after a resync); replicas built from contacts alone never mismatch. With a full restore the remaining mismatches go to 0 and a resync becomes final. | Mismatches are detected by the 2 s hash and resynced (max 80 ticks); `WpSim` counts damage steps itself so a new replica starts in the server's solver phase (verified against `DamageModel::state_hash`). |
| NET-N9 | `wp_sim` (lead) | A kind for **player-driven police seats** that drives the traffic spawn ring (like `Player`) but is not a racer for `wp_police` (e.g. `VehicleKind::PlayerPolice`). | `WpSim` marks police seats `VehicleKind::Police` so AI police do not chase them; that also removes them from `racer_views` *and* from the traffic ring (`TrackedVehicle::player` is `kind == Player`). | `VehicleKind::Police` (no traffic spawns around a lone player cop). |
| NET-N10 | `wp_damage` (03) | A public, pure `DamageModel::contacts_have_effect(&[ContactImpulse], &ModeTable) -> bool` (or per-contact filter) next to `add_contacts`. | `wp_net::lattice::damage_relevant` mirrors `contact.rs` (speed / impulse / group-energy thresholds) to skip replicating no-op contacts; a B-owned predicate keeps the two from drifting. | Mirror + guard test `lattice::tests::dropped_contacts_are_noops` (fails if B's thresholds change semantics). |
| NET-N11 | `wp_vehicle` (02) | `Rig` obstacles: `Rig::set_obstacles(&[(pos, rot, half_extents, vel)])` as kinematic cuboids (or a collider list) so the client can predict contacts with nearby cars. | Police rams / traffic hits are corrected one RTT late: chase RMS correction 0.12–0.39 m (max 3 m) vs 0.11 m in the contact-free test. | Corrections + 100 ms visual smoothing; chase budget 0.5 m RMS in `city_e2e`. |
| NET-N6 | `wp_police` (06) | Heat grid and incident list for the captain map (e.g. from `PoliceSystem::captain_view`), pursuit pairs. | `CaptainView`, interest "pursuit" category. | **Commands done (round 8)**: typed `CaptainCommand` → `Sim::captain_command` → `wp_police`, replies on the wire. Heat = crash density; pursuit = police within 300 m of a racer. |
| NET-N7 | `wp_economy` (08) | Payout API: `race_payout(results, stakes) -> Vec<(PlayerId, i64)>`, stake tables (day/night), persistent account store keyed by a durable player identity. | All economy stays on the server; the net layer should only call it. | Stake `500 × (1 + night_factor)`, split 70/30 or 60/30/10; since round 8 each change is a committed ledger `EconomyEvent` (`StakeCharged` / `RacePayout` / `RaceRep`) sent typed to its owner. In-memory `InMemoryStore` keyed by per-instance `PlayerId`. |
| NET-N12 | lead (`wp_sim`) + K (weather) | Weather inside `wp_sim` (K's `WeatherLink` plan: `Sim::set_weather`, vehicles and police read `weather_at`), so server grip and police sight follow the weather the clients are shown. | Today `WpSim` steps a `WeatherSim` next to the sim for replication only. | `WpSim::set_weather` / `enable_weather`; `WpSim` will switch to the sim's weather when it exists (one adapter change). |
| NET-N13 | D (world) | `AgentView` headlights flag (contract `traffic_flags::HEADLIGHTS`). | Night traffic in the browser. | Bit never set; clients may light every agent at night. |
| NET-N8 | `wp_world` (05) | Route helper for races, road polylines for the captain map. | Prompt 07 captain map. | `--city` now loads `.road.json`/`.road.bin`/`procedural` (round 6b). Races take client checkpoints; captain map draws a grid. |

## Contract change requests

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| NET-1 | Replace `ClientToServer` / `ServerToClient` with the full message sets in `wp_net::protocol` (`Hello, Inputs{ack_snapshot_tick, first_tick, inputs}, RoleRequest, CaptainCommand, Ping, LatticeRequest, Race*, Bye` / `Welcome, Reject, RoleAssigned{car_id}, CaptainOffer, EntityEnter/Leave, Snapshot{input_ack, own, entities}, Damage, LatticeHash, LatticeSnapshot, Pong, CaptainView, UnitOrder, Race*, Account, Notice`), or delete the contract skeletons and declare `wp_net::protocol` the source of truth. | The skeleton has no join handshake, acks, time sync, damage replication, lobby or captain messages; nobody else encodes these. | 07 only (04 if it wants a network mode). | **done (v0.3.0)**: `wp_net::protocol` is the wire source of truth, now `PROTOCOL_VERSION = 4` (typed events, `CaptainReply`, `RaceCommand`, traffic in `Snapshot`, `Weather`, MC-6 tokens); byte layouts in `crates/wp_net/README.md` § Protocol. |
| NET-2 | Add quantised wire types to contracts if other crates must produce them: `QInput`, `QContact`, `QVehicle`, smallest-three rotation, `LatticeSnapshot` (exact f32 of displaced nodes). | Determinism rule "authority applies dequantised values" spans `wp_sim` (contacts) and the client (inputs). Alternative: keep them in `wp_net` and give `wp_sim` a filter hook (NET-N1). | lead, 03 | proposed (either option works for 07) |
| NET-3 | Concrete `CaptainCommand` payload (owned by 06) and a `UnitOrder` type. | Opaque `Vec<u8>` before. | 06, 07 | **done (v0.3.0)**, used on the wire (round 8) |
| NET-4 | `EconomyEvent` kinds for `StakeCharged`, `RacePayout` so economy changes are visible to replays/logs. | Payouts bypassed `SimEvent`. | 08, 07 | **done (v0.3.0)**, emitted by the lobby (round 8) |
| NET-5 | `PlayerId` allocation: durable identity (account id) separate from the per-instance slot. | Accounts/cop rank must survive instances. | 08, 07 | **done (v0.3.0)** as `AccountId`; the server still maps `AccountId(player.0)` until G's ledger switches |
| NET-6 | None new this round. | — | — | — |

## Changes outside my crates
| File | Change | Reason |
|---|---|---|
| `crates/wp_vehicle/src/rig.rs` | `Rig::set_state(&VehicleState)` (additive: vehicle + rigid body rewind) | Client rewind on the real model (NET-N2). `Vehicle::set_state` itself is the lead's. |
| `crates/wp_damage/src/lib.rs` | `DamageModel::restore_node_positions` (additive) | Lattice resync (NET-N5). |
| `crates/wp_sim/src/police_link.rs` | `Sim::captain_command(CaptainCommand) -> Result<(), CaptainError>`, `Sim::set_human_captain(bool)` (additive, forward to `PoliceSystem`) | Typed captain commands from the wire (round 8). Lead: keep or replace with your own API. |
| `apps/simcli/src/net.rs` | `traffic: None` in the round-trip snapshot | Compile fix for protocol v4's `Snapshot.traffic` (owner I). |
| `crates/wp_wasm/Cargo.toml`, `src/lib.rs`, new `src/net.rs` | `wp_net` dependency, `pub mod net;`, `net_*` functions | Browser client. |
| `apps/testbed-web/vite.config.ts`, `README.md`, new `net.html`, `src/net/` | build input for `net.html`, README section | Browser client page. |
| `justfile` | `server` comment, new `server-bots` recipe | Load testing. |
| `Cargo.lock` | `wp_world` dependency of `wp_server`, `wp_police` dev-dependency of `wp_net` (round 6b); `wp_weather`, `serde`, `postcard` dependencies of `wp_net` (round 8; postcard 1.1.3 already locked for `wp_world`/`wp_wasm`, all wasm-clean) | `--city` loading; typed payloads; server weather. |

## Perf numbers
| Measurement | Machine | Native (release) | Browser | Budget |
|---|---|---|---|---|
| Server net tick, 32 clients, in-process (4 physics ticks + output) | cloud container | avg 3.4 ms, p99 9.0 ms (real vehicle) | — | 16.7 ms |
| Down-link per client, 32 bots in a 120 m arena (incl. 5 Hz own snapshots) | in-process | mean 29.7 kB/s, worst 1 s 33.9 kB/s | — | 40 kB/s |
| Prediction, 120 ms RTT / 2 % loss / ±30 ms jitter, 3 other cars | in-process | RMS correction 0.11 m (260 / 2631 reconciles; most from collisions) | — | < 0.3 m RMS |
| Prediction, same link, alone | in-process | RMS 0.15 m (4 / 2634) | — | < 2 % mispredict |
| Lattice hash checks, 4 clients × 2 min | in-process | 1 mismatch / 992 (injected), resynced in 16 ticks | — | < 1 %, < 2 s |
| **City e2e (real `wp_sim`)**: server net tick, 4 clients, ~30 vehicles (traffic + 9 AI police) | in-process | avg 2.34 ms, p99 7.7 ms | — | 16.7 ms |
| City e2e: down-link per client | in-process | mean 13.9–16.0 kB/s, worst second ≤ 21.4 kB/s | — | 40 kB/s |
| City e2e: AI police state agreement | in-process | 2261 client-held states, 0 differ from the server | — | exact |
| City e2e: lattice hashes | in-process | 63/2386 mismatched, 0 on contact-only replicas (rest: NET-N5) | — | — |
| City e2e: prediction during a chase | in-process | RMS 0.17–0.39 m, max 3.0 m, 0 resets (rams not predicted, NET-N11) | — | 0.5 m |

| **Round 8 (protocol v4)** city e2e down-link incl. kinematic traffic | in-process | mean 15.6–17.4 kB/s, worst second ≤ 24.5 kB/s | — | 40 kB/s |
| Round 8 city e2e net tick / lattice hashes | in-process | avg 1.61 ms, p99 5.9 ms / 0 of 2116 mismatched | — | 16.7 ms |
| Round 8 kinematic traffic replication (`city_v03`) | in-process | 27 465 agent states checked, 0 differ; ≤ 48 per client | — | exact |
| Round 8 weather message (13 689 cells, run-length) | in-process | 25 B uniform grid; bound 2 B/cell + 30 B | — | — |
| Round 8 browser smoke (`net.html`, debug wasm, `wp-server --city procedural --weather heavy_rain --bots 2`) | headless Chromium | joined, weather shown, traffic drawn, captain command reply `ok`, down 9 kB/s | — | — |

Measured on the integrated build with the real vehicle (A) and damage (B) models. The browser
figures from run 1 (stub) have not been re-measured.

## Known gaps
- Replicas restored from a *dented* lattice snapshot (car entered view already damaged, or a
  resync) hold node positions only and can diverge on the next hit, then resync (NET-N5).
- Rewinds are bit-exact only at the 5 Hz snapshot ticks (`set_state` in between); collisions
  with city geometry and other cars are corrected, not predicted (NET-N11).
- Own-car predicted contacts are not applied; own damage is server-driven, one RTT late.
- Kinematic traffic is display-only and straight-line extrapolated between 10 Hz updates.
- Server weather does not change server grip yet (NET-N12).
- The in-instance lobby keeps its own `RaceUpdate`/`RaceProgress`/`RaceResults`; contract
  `RaceEvent`s are forwarded only when the sim emits them (no `wp_race` in `wp_sim` yet).
- The `wp_meta` worker is not wired into `apps/server` (gate + protocol fields are).
- Player police seats are `VehicleKind::Police` in the sim: no traffic spawns around a lone
  player cop (NET-N9).
- Placeholders: heat grid, stakes/payout split, in-memory accounts.
- WebSocket only; both channels reliable. No auth beyond the reconnect token.
- `--arena` + browser clients: the browser uses `wp_damage` replicas, the arena uses the toy
  lattice, so browser hash checks mismatch and resync after every crash (by design of the
  test mode).

## Response to lead feedback (round 8)
Merged `origin/main` (contract v0.3.0, clean). Item 1 — the v0.3.0 net items, all in
`wp_net::protocol`, now `PROTOCOL_VERSION = 4` (tables in `crates/wp_net/README.md`):
- **Typed events on the wire**: `ServerMsg::Events` (0x94) carries `SimEvent::{Police, Race,
  Economy, WeatherChanged}` as `typed<SimEvent>` (postcard of the contract type), once per net
  tick; radio lines only to police roles, economy events only to the account's owner. The
  lobby's money changes are now real ledger `EconomyEvent`s (`StakeCharged` / `RacePayout` /
  `RaceRep`, NET-4) committed to the store and sent typed.
- **Typed `CaptainCommand` + `CaptainReply`**: `{seq, typed<CaptainCommand>}` → `wp_police`
  via a small additive `Sim::captain_command` (listed above); orders for player chase seats
  use unit ids `0x8000_0000 + entity` (shown on the captain map) and reach the driver as a
  typed `UnitOrder`; every command is answered with `Result<(), CaptainError>`. Seating a
  captain calls `set_human_captain`. The placeholder `CaptainOrder` is gone.
- **`RaceCommand`** (0x0D): `Enter` / `Withdraw` / `Ready` replace the v3 join/start/leave
  messages.
- **`TrafficAgentSnapshot`**: `Snapshot.traffic`, 10 Hz, nearest ≤ 48 kinematic agents within
  120 m, 1 cm / 1/65536-turn quantisation (`QTrafficAgent`), complete-set semantics; clients
  extrapolate ≤ 0.5 s. Checked exactly against the server in `tests/city_v03.rs`.
- **Weather (W-5)**: `ServerMsg::Weather` (0x96) on join and on change (new regime within
  1 s, else ≤ every 5 s), water grid run-length encoded (25 B for a 13.7k-cell city). Server
  weather: `WpSim::set_weather` / `enable_weather`, `wp-server --city … --weather heavy_rain`.
- **MC-6**: `Hello.join_token`, `Welcome.meta_reconnect_token`; with
  `ServerConfig::require_join_token` the server parks new players until the host approves or
  rejects (`take_join_requests` / `approve_join` / `reject_join`). Meta (O): MC-6 can be
  marked done; the `wp_meta` worker hookup in `apps/server` is the remaining step.
- Browser client (`net.html`) and `wp_wasm` net functions updated (typed captain radial,
  traffic boxes, weather / radio / wanted in the overlay, `?join=`); smoke-tested in headless
  Chromium against a live `wp-server --city procedural --weather heavy_rain --bots 2`.

Round-6b items were all done last round (see below). Checks: fmt, clippy `-D warnings`,
`cargo test --workspace`, the wasm build, `tsc --noEmit` for the testbed (with bindings
from `wasm-bindgen` 0.2.129) — results in the commit message of the round-8 docs commit.

## Response to lead feedback (round 6b)
Merged `origin/main` and the lead branch `origin/claude/laughing-newton-0xqr6q` (clean).
1. **4-player end-to-end on the real `wp_sim`** — `crates/wp_net/tests/city_e2e.rs`: one
   `ServerInstance<WpSim>` built with the new `WpSim::with_city` (8×8 procedural city, 23:00,
   physical traffic, AI police cars) and 4 bot clients (3 racers wanted at level 3, 1 player
   police seat) on lossy 20–60 ms links for 60 s. Asserts: every client-held AI police state
   equals the server's quantised state for that tick and nearby police are known to every
   client; contact-only replicas never mismatch and overall lattice mismatches < 5 %;
   prediction RMS < 0.5 m with 0 resets; every client's worst second ≤ 40 kB/s; release net
   tick < 8 ms. Numbers in the perf table above and in `crates/wp_net/README.md`.
   `wp-server --city FILE|procedural` runs the same instance over websockets
   (`apps/server/tests/e2e.rs::city_flag_runs_bots_in_a_police_city`).
2. **Police and physical traffic as entities** — `WpSim::ai_kind` maps `VehicleKind`
   (`Police` → `AiPolice`, `Traffic` → `Traffic`, new `AiRacer` → `EntityKind::AiRacer`,
   protocol v3); wanted levels come from `Sim::police()`. Player police seats are marked
   `VehicleKind::Police` so AI police do not chase them (NET-N9). Kinematic traffic waits for
   v0.3.0.
3. **NET-N1 damage replication on the real sim** — `WpSim` installs `QContact::round_trip` as
   the contact filter and drains `Sim::drain_applied_contacts`. Making it work in a real city
   needed three fixes found by the test:
   - resting contacts arrive every tick and were 2/3 of all down-link bytes (worst second
     87 kB/s): contacts `wp_damage` provably ignores are no longer sent
     (`lattice::damage_relevant`, guarded by a no-op test; 6.5 MB → 19 kB per run);
   - lattice snapshots of dented cars (≤ 3.7 kB) entering view together blew the budget:
     they now queue behind the byte bucket (refill lowered to 85 %);
   - replicas skipped empty steps after 240 and were created at step 0, so `wp_damage`'s
     every-2nd-step solver ran out of phase: replicas now run every step, and lattice
     snapshots carry the server model's step count (`WpSim` counts it; verified against
     `DamageModel::state_hash`).
   Remaining mismatches are all on replicas restored from dented snapshots → NET-N5 (full
   damage snapshot/restore, `wp_damage`).

Checks (round 6b): fmt, clippy `-D warnings`, `cargo test` for every crate, simcli tests
(release), every simcli scenario (they round-trip snapshots through the `wp_net` codec), and
the wasm build all pass, except two tests that also fail on the lead branch (no files they use
differ on `ws/net`): `wp_economy/tests/pursuit_wiring.rs::real_police_capture_settles_through_the_economy`
(line 130) and `wp_race/tests/simcli_races.rs::city_sprint_on_real_roads` (index out of
bounds, line 28).

## Response to lead feedback (run 4)
Merged `origin/lead/net-merge` (unchanged since run 3, already in) and `origin/main` (contract
v0.2.0, A's snapshot/`Rig`); took 02's `Vehicle::set_state` in the one conflict
(`wp_vehicle/src/vehicle.rs`). After the merge all `wp_net`/`wp_wasm`/`wp_server` tests were
already green; then, per run-3 feedback, prediction now rewinds on A's snapshot:
- `NetSim::vehicle_snapshot` (default `None`; `WpSim` = `Vehicle::snapshot()` bytes,
  `ArenaSim` delegates). The server adds the own car's snapshot to `Snapshot.own_full` every
  12th net tick (5 Hz). **Protocol v2** (wire change: `u8 has_full, [bytes]` after `OwnState`).
- `PredictedVehicle::restore` / `Predictor::reconcile(.., exact, ..)`: exact `Rig::restore`
  when the snapshot is present, `Rig::set_state` otherwise; the predictor is initialised from
  the snapshot when the first own state carries one.
- Tests: `exact_snapshot_rewind_replays_bit_for_bit` (client vs a server `Rig`: 0.0 m gap after
  rewind + 10-tick replay) and `state_rewind_fallback_is_close` (< 5 cm). Bots (tests and
  `wp-server --bots`) drive through the same predictor; `bot_reaches_waypoints` steps a `Rig`.
- Down-link budget is now a per-client byte bucket (refill 95 % of budget, burst one tick):
  the per-tick split let the 0.9 kB snapshot push one client's 1 s window past 40 kB.

## Response to lead feedback (run 3)
Merged `origin/lead/net-merge` (kept all its drift fixes) and then `origin/main` into `ws/net`.
1. `predict` test: prediction now steps the real vehicle through `predict::RigVehicle`
   (`wp_vehicle::rig::Rig` + new `Rig::set_state`); the test drives 2 s before diverging so
   the real (slower, filtered) car actually separates. Replay agrees within 5 cm (measured
   1.4 cm; not bit-exact, see NET-N2).
2. `bot` test: steps the bot on `Rig` instead of the flat-plane `VehicleSim::step`; no tuning
   was needed (8 waypoints in 30 s). Bots in tests and `wp-server --bots` drive through the
   client's predictor, i.e. the same Rig.
3. `prediction.rs`: RMS correction 0.11 m (was 0.64 m on the flat-plane step). Split into the
   spec bound with traffic and a solo test (< 2 % mispredictions): with traffic most
   corrections come from real Rapier collisions, which the client does not predict.
4. `wp_wasm::net`: embedded data now comes from 04's generated `crate::data`
   (all cars, both modes), no hand list.

## Lead feedback (2026-10-19, run 2)

**Not merged yet (machine F).** Your branch merges cleanly once drift is fixed, but 4 of your tests
fail against the real vehicle (A) and damage (B) models, so `main` would go red. The resolved merge
is parked on branch **`lead/net-merge`** — start from it: `git fetch && git checkout ws/net && git merge
origin/lead/net-merge` (or merge `main` and re-apply the list below).

Failing on the integrated build:
1. `wp_net::predict::tests::identical_inputs_stay_in_sync_and_divergence_is_replayed` (predict.rs:286):
   expected a divergence, got `InSync { error_m: 0.0 }`.
2. `wp_net::bot::tests::bot_reaches_waypoints` (bot.rs:139): bot never advances past waypoint 0 (real
   acceleration / steering are slower than the stub's).
3. `wp_net/tests/prediction.rs::corrections_stay_small_at_120ms_rtt_with_loss_and_jitter`: RMS
   correction 0.64 m (above your bound).
4. `wp_wasm::net::tests::embedded_net_data_resolves`: `debug_sedan` lattice needs material
   `cast_aluminum` — embed data via `crates/wp_wasm/build.rs` (C's generated embedding) instead of a
   hand list in `net.rs`.
Root cause of 1-3 is your NET-N2: prediction must step the real vehicle the way the server does
(Rapier chassis body + `RapierGround`; see `wp_vehicle::rig::Rig`), not the trait's flat-plane step.

Drift already fixed on `lead/net-merge` (keep these):
- Vehicle `set_state` / damage `restore_node_positions` re-added on the **real** types
  (`wp_vehicle/src/vehicle.rs`, `wp_damage/src/model.rs`); your stub edits are dropped.
- `server.rs`: G's real `AccountStore` is `load -> Result<Option<_>>` + `commit(&acct, &events)`;
  added `Server::account(pid)` adapter. Better: call `wp_economy::Economy::{open_account, enter_race,
  settle_race}` (G's payout API exists now — NET-N7 is answered).
- `tests/session.rs`: accounts built with `PlayerAccount::empty` + `commit`.
- `wp_wasm`: keeps C's `lib.rs`; your `pub mod net;` + `wp_net` dependency added. Vite: `net.html` input
  added to C's config. README/justfile merged.
- simcli and `tools/bench` used `wp_net::{encode_server, decode_server}`, which you removed; they now
  do the JSON round trip themselves (machine I will switch them to your snapshot codec).

Your `wp_sim` needs: NET-N1 (applied-contacts drain + quantise hook), NET-N3 (`despawn_vehicle`;
`place_vehicle(id, pos, yaw, speed)` exists now), NET-N4 (`Sim::mode_table()` exists now; per-vehicle
kind next) — I'll do N1/N3/N4 in the next lead run, then re-land your branch.

Contract requests: NET-1 (full protocol into contracts) **accepted in principle** for v0.2.0 — send
the final message list; NET-2 quantised types **rejected for now** (keep them in `wp_net`, the
authority filter is a function pointer); NET-3 with E (police); NET-4 accepted (with G's typed
economy events); NET-5 accepted (durable account id separate from instance slot).

### Note from 02 (vehicle), for NET-N2
On `ws/vehicle` (merge pending): `wp_vehicle::rig::Rig` is the client-side single-car stepper —
same ground collider, priming and tick order as `wp_sim`, bit-identical to the server for a lone
car on flat ground (`crates/wp_sim/tests/rig_parity.rs`). Rewind with `Rig::snapshot()` /
`Rig::restore(&VehicleSnapshot)` + `set_ticks` (bit-exact; `VehicleSnapshot::as_bytes()` ~0.9 kB
if the server should send its full state), or the approximate `Vehicle::set_state(&VehicleState)`
+ `write_chassis_to_body`. Use `Rig::with_world(car, extras, world, pos, yaw)` with the same
`WorldQuery` type as the server so surfaces match. Not covered: contacts with other bodies
(cars, boxes, city colliders) — reconcile those from server snapshots.


### Run 3 (2026-10-19)
- Contract **v0.2.0** is on `main` (see `docs/CONTRACT_CHANGELOG.md`): `VehicleSim::step` takes a
  `ground: &dyn GroundQuery` — your prediction stepper can pass A's `RapierGround` / `Rig` ground.
  `lead/net-merge` predates it: merge `main` after merging `lead/net-merge`.
- A delivered NET-N2: bit-exact `Vehicle` snapshot/restore, fuller `set_state`, and `Rig` matching
  `wp_sim`'s world (`crates/wp_sim/tests/rig_parity.rs`). Build prediction on that.
- **Your `wp_sim` needs are done** (on `main`): NET-N1 `Sim::set_contact_filter(Some(fn(&mut ContactImpulse)))`
  (applied before damage; pass `QContact::round_trip`) + `set_record_applied_contacts(true)` /
  `drain_applied_contacts() -> Vec<(EntityId, Vec<ContactImpulse>)>`; NET-N3 `Sim::despawn_vehicle(id)`
  (ids never reused) and `place_vehicle(id, pos, yaw, speed)`; NET-N4 `Sim::mode_table()` and
  `Sim::set_vehicle_kind(id, VehicleKind::{Player, AiRacer, Police, Traffic})` / `SimVehicle::kind`.


## Lead feedback (round 6b, 2026-10-19) — head-start tasks

Contract **v0.3.0** is being applied in parallel by the lead (branch `lead/contract-v0.3.0`). Do not edit `wp_contracts`; build your work so it drops onto v0.3.0 (inherent methods / local types now, trait wiring after). Merge `origin/claude/laughing-newton-0xqr6q` (the lead branch, ahead of `main` until today's gate finishes) before starting. Goal of the project right now: the **single-player greybox demo**: drive in the city, raise heat, get chased by physical police, escape or get caught, and pay the repair bill.

1. 4-player end-to-end test on the **real** `wp_sim`: a headless server plus 4 bot clients for 60 s in a city scenario with police and traffic. Assert no desyncs beyond the correction budget, all clients see the same police units, and bandwidth per client is under budget. Record the numbers in your README.
2. Police and physical traffic are now ordinary `wp_sim` vehicles (`VehicleKind::Police` / `Traffic`). Replicate them as entities with the kind flag. Kinematic traffic waits for v0.3.0 `TrafficAgentSnapshot`.
3. NET-N1: damage replication on the real sim (not `ArenaSim`).


## Lead feedback (round 8, 2026-10-19)

Contract **v0.3.0 is on `main`** (typed police/race/economy events, damage snapshot/summary, road-network trait, weather `LocalConditions`; see `docs/CONTRACT_CHANGELOG.md`). Workspace tests now take about 3.5 minutes (`lead/test-speed`). Owner playtest items are tracked in `docs/PLAYTEST_LOG.md`.

1. v0.3.0 net items: typed events on the wire, `TrafficAgentSnapshot`, weather snapshot (W-5), `RaceCommand`, and meta's `join_token` (MC-6) in `wp_net::protocol`.
