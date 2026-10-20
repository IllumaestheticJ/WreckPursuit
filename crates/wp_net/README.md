# wp_net

**Owned by workstream 07 (multiplayer netcode).**

Authoritative-server multiplayer for one city instance: binary protocol, quantisation,
per-client interest-filtered delta snapshots, client prediction with rewind/replay, damage
replication by contact events with lattice-hash verification, roles (racer / police chase /
captain / spectator), an in-instance race lobby, and anti-cheat basics.

Everything except `native` is **sans-IO**: state machines fed with received frames and a
caller-supplied clock (server: simulation ticks; client: milliseconds). No wall clock, no
threads, no sockets — so the same code runs in the native server, in the browser (via
`wp_wasm`) and in deterministic in-process tests.

| module | what |
|---|---|
| `wire` | varint / zigzag byte reader + writer |
| `quant` | quantisation of positions, rotations, velocities, inputs, contacts, kinematic traffic (`QTrafficAgent`) |
| `snapshot` | per-entity delta compression (`DeltaVehicle`), full-precision `OwnState` |
| `lattice` | 32-bit lattice hash, `LatticeSnapshot`, `ReplicaDamage` trait |
| `protocol` | `ClientMsg` / `ServerMsg` + codec (tables below); contract types travel as `typed<T>` (postcard) |
| `interest` | interest categories, priority accumulators, byte budget |
| `roles` | `RoleManager`: police ratio, single captain, captain-seat offers |
| `race` | `RaceLobby`: create/invite/join, countdown, checkpoints, results, payouts |
| `netsim` | `NetSim` trait (what the server needs from a sim) + `WpSim` adapter over `wp_sim::Sim` (`with_city`: city + AI police + physical traffic, `lane_spawn_points`; kinematic traffic agents, optional server weather `set_weather` / `enable_weather` / `city_weather`, captain commands into `wp_police`) |
| `server` | `ServerInstance<S: NetSim>`: the authoritative instance |
| `client` | `Client`: prediction, interpolation, damage replicas, time sync, lobby state |
| `predict`, `interp`, `timesync` | client building blocks |
| `sim_link` | in-memory netem-style link (latency, jitter, loss) implementing `Transport` |
| `bot` | scripted driver (`wp-server --bots`, tests) |
| `testkit` | `ArenaSim` crash arena + `ToyLattice` (cheap damage traffic for load tests), `load_data` |
| `native` (feature) | tokio game-loop thread, websocket acceptor, websocket client `Transport` |

The `native` feature (tokio, tungstenite) must never be enabled by anything reachable from
`wp_wasm`; `cargo tree -p wp_wasm --target wasm32-unknown-unknown` shows no tokio.

## Tests

```sh
cargo test -p wp_net                      # unit + integration (≈4 min debug; city_e2e ≈90 s)
cargo test -p wp_net --release -- --nocapture   # prints bandwidth / timing / hash numbers
cargo test -p wp_server                   # real websockets end-to-end, admin endpoint, --bots, meta link
cargo test -p wp_net --release --test vesper_e2e -- --nocapture   # step-2 numbers below
just web-e2e-net                          # two browser tabs on a local wp-server
```

| test | asserts |
|---|---|
| `tests/vesper_e2e.rs` | **step 2: asymmetric multiplayer on the real game.** Port Vesper v2 (terrain, buildings) at 23:00 with physical + kinematic traffic and AI police; 4 clients on lossy 20–60 ms links for **3 minutes**: 2 racer bots (port loop / downtown loop), a **player cop** bot steering from what its own client sees and calling in a roadblock and spikes, a **captain** ordering the cop onto racer 0 and setting the priority target; racer 1 drops its link for 3 s and reconnects with its token. Asserts: prediction RMS < 0.5 m, no resets (one allowed for the reconnect), no decode errors; contact-only replicas never diverge, < 5 % hash mismatches; every vehicle state any client holds (racers, the cop, AI police, physical traffic) equals the server's quantised state for that tick; vehicles within 60 m for 1 s are known; kinematic traffic equals the server's; the captain map sits exactly where the server had each unit; after settling every damage replica equals the server's lattice; the pursuit of racer 0 is started by the player cop's unit, roadblock/spikes are placed, and the capture (crediting the player cop) reaches all 4 clients; worst second ≤ 40 kB/s; net tick avg < 8 ms, p99 < 16.7 ms (release) |
| `tests/city_e2e.rs` | **real `wp_sim`**: procedural city at 23:00 with physical traffic and AI police cars, 3 wanted racers + 1 police seat (bots, 20–60 ms links, 1 % loss) for 60 s: every client holding an AI police unit's state holds exactly the server's quantised state, police within 60 m of a client are known to it, replicas built from contacts never mismatch, overall lattice mismatches < 5 % (all after dented-snapshot restores, NET-N5), prediction RMS < 0.5 m with no resets (rams are not predicted), worst second ≤ 40 kB/s per client |
| `tests/city_v03.rs` | **protocol v4 on the real `wp_sim`** (6×6 city, 22:00, weather, 2 racers + captain, 20 s): every kinematic traffic agent a racer holds equals the server's quantised agent at that tick and lies within the interest radius, the captain gets none; weather on join and a forced regime change as `WeatherChanged` + snapshot; the wanted racer gets its `WantedLevelChanged`, radio only to the captain; typed `CaptainCommand`s reach `wp_police` (`Ok`, `UnknownTarget`, `UnknownUnit` replies) and AI units carry their unit ids on the map; down-link ≤ 40 kB/s |
| `tests/lockstep.rs` | server + 4 clients, 2 min scripted driving/crashing in the arena: < 1 % of lattice-hash checks mismatch, every mismatch resynced within 2 s, all replicas equal the server at the end (one desync is injected deliberately) |
| `tests/prediction.rs` | 120 ms RTT, 2 % loss, ±30 ms jitter: correction RMS < 0.3 m, < 5 % of reconciles mispredict |
| `tests/bandwidth.rs` | 32 bots: every client's worst second ≤ 40 kB/s, police cap = 12 of 32, net tick < 8 ms (release) |
| `tests/session.rs` | reconnect within 30 s keeps the car, expiry removes it; Bye; full instance; version mismatch; input timing/rate anti-cheat; role ratio, typed captain orders to player seats with replies (ok / unknown unit / unknown target / no police system / not captain), captain hand-off; race lobby via `RaceCommand` → countdown → checkpoints → results → payouts as typed `EconomyEvent`s (own account only, gap-free seq); meta join-token gate (park, approve with display name + meta token, reject, reattach without a new token) |
| unit tests | codec round-trips for every message + truncation, quantisation bounds, delta encode/decode, interest priorities, roles, races, time sync, prediction replay, interpolation |

## Measurements

| Measurement | Machine | Native (release) | Budget |
|---|---|---|---|
Real vehicle (A) and damage (B) models on the integrated build (lead run 2 merge):

| Measurement | Machine | Native (release) | Budget |
|---|---|---|---|
| Server net tick, 32 clients (4 physics ticks of 32 Rapier cars + all output), in-process | cloud container | avg 3.4 ms, p99 9.0 ms | 16.7 ms frame (asserted < 8 ms avg) |
| Down-link per client, 32 bots, 120 m arena (all within interest), incl. 5 Hz own snapshots | in-process | mean 29.7 kB/s, worst second 33.9 kB/s | 40 kB/s |
| Prediction at 120 ms RTT / 2 % loss / ±30 ms jitter, 3 other cars (real collisions) | in-process | RMS 0.11 m, max 0.92 m, 260 corrections in 2631 reconciles | < 0.3 m RMS |
| Same link, alone | in-process | RMS 0.15 m, 4 corrections in 2634 reconciles | < 2 % mispredict |
| Snapshot rewind + 10-tick replay vs server `Rig` | unit test | 0.0 m (bit-exact); `set_state` fallback ≈1.4 cm | exact |
| Lattice hash checks (4 clients, 2 min) | in-process | 1 mismatch in 992 (the injected one), resynced in 16 ticks | < 1 %, < 2 s |

**4-player city test on the real `wp_sim`** (`tests/city_e2e.rs`, round 6b; release and
debug give identical numbers — the run is deterministic): 8×8-block procedural city at
23:00, up to 16 physical traffic cars and 9 AI police cars, 3 racers starting at wanted
level 3 (reached 5) + 1 player police seat, links 20/40/60/35 ms one-way, ±10 ms jitter,
1 % loss, 60 s.

| Measurement | Native (release) | Budget |
|---|---|---|
| Server net tick (4 physics ticks of ~30 vehicles + output for 4 clients) | avg 2.34 ms, p99 7.7 ms | 16.7 ms (asserted < 8 ms avg) |
| Down-link per client | mean 13.9–16.0 kB/s, worst second 20.6–21.4 kB/s | 40 kB/s |
| Of all down-link bytes: snapshots / lattice snapshots / damage contacts | 3.5 MB / 0.22 MB / 19 kB | — |
| Damage contacts before `damage_relevant` (same run) | 6.5 MB (2/3 of all bytes; worst second 87 kB/s) | — |
| AI police states held by clients vs server (same tick) | 2261 compared, 0 differ; 127/127 police within 60 m known | exact |
| Lattice hash checks | 63 / 2386 mismatched (2.6 %), **0 on contact-only replicas**; all resynced, max 80 ticks | contact-only = 0 |
| Prediction (chase: rams and traffic hits are not predicted) | RMS 0.17 / 0.39 / 0.30 / 0.32 m, max 3.0 m, 3–7 % of reconciles, 0 resets | < 0.5 m RMS |

**Protocol v4 (round 8, contract v0.3.0)** — same tests, release:

| Measurement | Native (release) | Budget |
|---|---|---|
| `city_e2e` down-link per client (now incl. kinematic traffic, 10 Hz, ≤ 48 agents ≤ 120 m) | mean 15.6–17.4 kB/s, worst second 22.6–24.5 kB/s | 40 kB/s |
| `city_e2e` server net tick | avg 1.61 ms, p99 5.9 ms | 16.7 ms |
| `city_e2e` lattice hash checks / AI police states | 0 / 2116 mismatched; 1062 police states compared, 0 differ | — |
| `city_v03` kinematic traffic | 27 465 agent states checked against the server, 0 differ; up to 48 held | exact |
| `city_v03` worst second (racer with 48 agents in view) | 21.6 kB/s | 40 kB/s |
| Weather message, 13 689-cell water grid (run-length encoded) | 25 B (uniform grid); every snapshot ≤ cells × 2 + 30 B | — |
| Typed events (20 s, 3 clients, police + weather) | 39 events, 994 B | — |
| `bandwidth` (32 bots, arena, no traffic) | mean 29.8 kB/s, worst second 33.2 kB/s; net tick avg 4.1 ms | 40 kB/s |

**Step 2 — Port Vesper v2, 4 clients, 3 minutes** (`tests/vesper_e2e.rs`, protocol v5;
release and debug identical, the run is deterministic; debug ≈ 42 s, release ≈ 30 s).
Up to 26 vehicles (10 AI police cars, 14 physical traffic), links 20/32/45/60 ms one-way,
±8 ms jitter, 1 % loss.

| Measurement | Native (release) | Budget |
|---|---|---|
| Server net tick (4 physics ticks + output for 4 clients) | avg 1.77 ms, p99 4.59 ms, max 12.2 ms | 16.7 ms |
| Down-link per client: racer 0 / racer 1 (reconnects) / cop / captain | mean 10.5 / 12.1 / 9.8 / 1.0 kB/s; worst second 13.0 / 34.9 (the reconnect second) / 11.9 / 1.2 kB/s | 40 kB/s |
| Prediction on terrain (client drives on the server's own city colliders) | RMS 0.028 / 0.056 / 0.123 m, max 1.24 m; 0 resets (racer 1: 1, its reconnect) | 0.5 m |
| Vehicle states held by clients vs server (same tick) | 14 124 compared (racers, player cop, AI police, traffic), 0 differ; 4350/4350 nearby vehicles known | exact |
| Kinematic traffic / captain map | 1858 agents, 0 differ / 4248 map units, 0 differ | exact |
| Lattice hash checks | **0 / 3844 mismatched**; 46 replicas checked after settling (39 dented), all equal the server | contact-only = 0 |
| Dented-car lattice snapshot (nodes + full damage state) | ≤ 3.5 kB + ≤ 10 kB | sliding 1 s window ≤ 38 kB |
| Pursuit | started by the player cop's unit 3 s after racer 0 became wanted; roadblock + spikes placed on Gantry Row; capture credited to the player cop, seen by all 4 clients | — |

**Browser** (`just web-e2e-net`, headless Chromium, release wasm, local `wp-server`): two
tabs join as racers, each sees 2 player cars; tab B steers into tab A's car; both cars dent
on both pages (243 / 163 nodes, max 21 / 17 cm) with identical lattice hashes on both
pages; a dropped socket reconnects with its token and keeps its car; ~9 kB/s down per tab.
Screenshots: `docs/media/net_two_tabs_a.png`, `net_two_tabs_b.png`.

Earlier numbers on the kinematic stub (run 1): net tick 0.39 ms, worst client 33 kB/s,
RMS correction 0.14 m.

## Architecture

```
client (browser/bot)                                    server (one instance, 60 Hz)
───────────────────                                     ─────────────────────────────
keyboard → VehicleInput ─quantise→ Predictor.step (240 Hz, ahead by RTT/2 + 2·jitter)
         ─ Inputs{ack, first_tick, ≤24 redundant} ───► per-player input buffer by tick
                                                         4 × { set_input(tick) ; sim.step }
                                                         contacts/events per tick
◄─ Damage{vehicle,tick,contacts,events} (R) ────────── for vehicles the client tracks
◄─ LatticeHash (R, every 2 s per vehicle) ──────────── staggered
   mismatch → LatticeRequest ───────────────────────►
◄─ LatticeSnapshot (R) ────────────────────────────── exact positions of dented nodes
◄─ EntityEnter/Leave (R) ──────────────────────────── interest changes
◄─ Snapshot (U): OwnState @30 Hz + DeltaVehicles ──── priority order until byte budget
   OwnState → reconcile (rewind + replay inputs)
   DeltaVehicles → StateBuffer → render 100 ms behind, extrapolate ≤ 50 ms
Ping (U) ⇄ Pong (U) → TimeSync (server tick estimate, RTT, jitter)
```

### Time base
- Physics tick `T` = number of 240 Hz steps the server has run. Input tagged `t` is applied in
  the step `t → t+1`; a snapshot at `T` is the state after `T` steps.
- The client estimates the server tick from pongs and predicts
  `lead = (RTT/2 + 2·jitter + 4 ms)·240 + 4 + 1` ticks ahead, so inputs arrive before the
  server simulates them (it simulates a net tick's four physics ticks at once).
- The server never reads a clock. The reconnect window, offers and races count ticks.

### Prediction and reconciliation
The client runs the same `wp_vehicle` code on its own single-car Rapier world
(`predict::RigVehicle`, stepped like `wp_sim`), keeps 240 ticks of **inputs** (plus the pose after
each, only to measure error) and, on each `OwnState` (30 Hz, full `f32`), compares the
prediction for that tick. Beyond 5 cm / 0.02 rad it rewinds to the server state and
re-simulates the stored inputs. Every 12th net tick (5 Hz) the snapshot also carries the own
car's bit-exact `wp_vehicle::VehicleSnapshot` (`own_full`, ≈0.9 kB); with it the rewind is
lossless (`Rig::restore`, replay reproduces the server bit for bit), without it the rewind
uses `set_state` (≈1 cm off after 10 ticks: filters, clutch, tire relaxation are not in
`VehicleState`). The first own state with a snapshot also initialises the predictor
exactly. The down-link budget is a per-client byte bucket (refill 85 % of 40 kB/s, burst one
tick), so the snapshot spike is repaid by the following ticks. Lattice snapshots (up to
≈3.7 kB for a dented car) queue behind the bucket: while one waits, entity updates shrink to
the minimum and the credit may grow to its size, so a 1 s window stays under budget.
Collisions with other cars are not predicted (the client's rig holds only its own car). The visual jump is absorbed by a render offset that decays
with a 100 ms time constant. Inputs are quantised (`quant::quantize_input`) **before** the
client simulates them, so both sides simulate identical values.

### Damage replication
Node positions never travel in normal play. Per physics tick the server reports the contacts
applied to each vehicle's damage model (`NetSim::drain_applied_contacts`). Contacts are
quantised in chassis space (`QContact`) and the authority **applies the dequantised value**,
so a client re-running `apply_contacts` with the same contacts at the same tick gets the
same lattice. On the real sim, `WpSim` installs `QContact::round_trip` as `wp_sim`'s contact
filter and drains `Sim::drain_applied_contacts`. Contacts `wp_damage` provably ignores
(closing speed under `min_contact_speed_mps`, no impulse, or a whole tick below
`min_contact_energy_j`) are not sent (`lattice::damage_relevant`; a unit test checks they
are no-ops): resting contacts between cars come every tick and were 2/3 of all down-link
bytes in a city chase. Replicas run every empty step for ticks without contacts
(`wp_damage` solves every 2nd step, so skipping steps would shift the phase), and lattice
snapshots carry the server model's step count so a new replica starts in phase. Every 2 s per
tracked vehicle the server sends a 32-bit hash of node displacements quantised to 1 mm; a
client that disagrees requests a `LatticeSnapshot` of that vehicle only. Snapshots carry
exact `f32` positions of displaced nodes (bitmap-delta against the rest pose), because a
rounded restore drifts again on the next contact.

**Full damage state (v5, NET-N5 closed).** A snapshot of a *dented* car also carries
`wp_damage`'s lossless `FullDamageSnapshot` (beam rest lengths, plastic and compressive
strain, broken/buckled beams, components, detached parts, glass, health), sparse against the
pristine car (`lattice::encode_damage_state`; the per-beam plastic-work ledger is left out —
it only feeds a diagnostic total). The replica restores it with `restore_full`, so the next
hit behaves as on the server. A snapshot taken while the server's model still has an impact
in progress (`NetSim::damage_settled` false: hit and awake) is marked **provisional**; the
server sends a final one as soon as the model settles. The client's own car is replicated the
same way; its handling effects feed the predictor. Big lattice snapshots wait in a queue that
respects both the byte credit and a sliding 1 s window (≤ 95 % of the budget).

## Protocol

One transport frame = one message: `u8 tag` + body. Integers in bodies are little-endian
fixed width or LEB128 `varint`; `zz` = zigzag varint; `str`/`bytes` = varint length + data;
`opt` = varint, 0 = none, else value + 1. Decoders reject unknown tags, oversize lengths
and trailing bytes. `PROTOCOL_VERSION = 5`; `Hello` must match it and the contract version.

`typed<T>` = `varint len` + the [postcard](https://docs.rs/postcard) encoding of the contract
type `T` (enums: varint variant index in declaration order, then fields; integers varint /
zigzag; `f32` 4 bytes LE; `Option` 1 byte tag; `Vec`/`String` varint length). The body must
fill `len` exactly. Because the layout follows the contract, `Hello` also checks
`CONTRACT_VERSION`. Used for `CaptainCommand`, `RaceCommand`, `UnitOrder`,
`Result<(), CaptainError>` and `SimEvent`.

v3 → v4: `Hello` + `join_token`, `Welcome` + `meta_reconnect_token` (MC-6); `CaptainCommand`
is typed with a `seq`; `RaceCommand` (0x0D) replaces `RaceJoin` 0x08 / `RaceStart` 0x0A /
`RaceLeave` 0x0B; `Snapshot` + `traffic`; `UnitOrder` typed; `MapUnit` + `unit`; new
`Events` 0x94, `CaptainReply` 0x95, `Weather` 0x96.

v4 → v5 (step 2): `Inputs` + `missing` (entities whose delta baseline the client lacks; the
server forgets those baselines and sends them in full — before, an entity whose
`EntityEnter` arrived after its first deltas froze for that client for good);
`LatticeSnapshot` + `u8 flags` (bit 0 full damage state follows as `bytes`, bit 1
provisional).

### Client → server

| tag | message | channel | body |
|---|---|---|---|
| 0x01 | `Hello` | R | `u16 protocol, str contract (≤16), str name (≤32), u8 input_source, u64 reconnect_token, str join_token (≤512; "" = none)` |
| 0x02 | `Inputs` | U | `varint ack_snapshot_tick, varint first_tick, u8 n (≤32), n × QInput, varint m (≤64), m × varint missing_entity` |
| 0x03 | `RoleRequest` | R | `u8 role` (0 racer, 1 police chase, 2 captain, 3 spectator) |
| 0x04 | `CaptainCommand` | R | `varint seq, typed<CaptainCommand> (≤256)` → `CaptainReply` |
| 0x05 | `Ping` | U | `u32 client_ms` |
| 0x06 | `LatticeRequest` | R | `varint vehicle` |
| 0x07 | `RaceCreate` | R | `u8 laps, varint n (≤64), n × (zz x_dm, zz y_dm)` |
| 0x09 | `RaceInvite` | R | `varint race, varint player` |
| 0x0C | `Bye` | R | — (no reconnect window) |
| 0x0D | `RaceCommand` | R | `typed<RaceCommand> (≤256)`: `Enter { event }` joins lobby race `event`, `Withdraw { event }` leaves (DNF once running), `Ready` from the host starts the countdown |

`QInput` (7 bytes): `i8 steer×127, u8 throttle×255, u8 brake×255, u8 handbrake×255,
u8 clutch×255, u8 buttons (1 gear up, 2 gear down, 4 horn, 8 headlights, 16 siren), u8 tool_use`.


### Server → client

| tag | message | channel | body |
|---|---|---|---|
| 0x81 | `Welcome` | R | `varint player, u64 token, varint server_tick, u8 mode, u16 protocol, f32 time_of_day_h, str meta_reconnect_token (≤512)` |
| 0x82 | `Reject` | R | `str reason` — connection closed after it |
| 0x83 | `RoleAssigned` | R | `varint player, u8 role, opt vehicle, str car_id` |
| 0x84 | `CaptainOffer` | R | `varint expires_tick` |
| 0x85 | `EntityEnter` | R | `varint entity, str car_id, u8 kind (0 racer, 1 police chase, 2 AI police, 3 traffic, 4 AI racer), opt owner` |
| 0x86 | `EntityLeave` | R | `varint entity` |
| 0x87 | `Snapshot` | U | `varint tick, varint input_ack, u8 has_own, [OwnState], u8 has_full, [bytes own_full ≤4096: wp_vehicle::VehicleSnapshot], varint n, n × DeltaVehicle, u8 has_traffic, [varint k (≤256), k × QTrafficAgent]` |
| 0x88 | `Damage` | R | `varint vehicle, varint tick, varint n, n × QContact, varint m, m × DamageEvent` |
| 0x89 | `LatticeHash` | R | `varint vehicle, varint tick, u32 hash` |
| 0x8A | `LatticeSnapshot` | R | `varint vehicle, varint tick, LatticeSnapshot, opt damage_steps, u8 flags (1 = state, 2 = provisional), [bytes state]` |
| 0x8B | `Pong` | U | `u32 client_ms, varint server_tick` |
| 0x8C | `CaptainView` | R | `varint tick, u8 full, varint n, n × MapUnit, varint m, m × varint removed, varint k, k × Incident, varint h, h × (i16 cell_x, i16 cell_y, u8 heat)` |
| 0x8D | `UnitOrder` | R | `typed<UnitOrder>` |
| 0x8E | `RaceUpdate` | R | `varint race, varint host, u8 state (0 lobby, 1 countdown, 2 running, 3 finished), varint start_tick, zz stake, u8 laps, varint n, n × (zz x_dm, zz y_dm), varint m, m × varint member` |
| 0x8F | `RaceProgress` | R | `varint race, varint player, varint next_checkpoint` |
| 0x90 | `RaceResults` | R | `varint race, varint n, n × (varint player, u8 place (0 = DNF), varint time_ticks, zz payout)` |
| 0x91 | `RaceInvited` | R | `varint race, varint from` |
| 0x92 | `Account` | R | `zz cash, zz rep, varint cop_rank, varint bounty` |
| 0x93 | `Notice` | R | `str text` |
| 0x94 | `Events` | R | `varint tick, varint n, n × typed<SimEvent> (≤8192 each)` — police / race / economy / weather |
| 0x95 | `CaptainReply` | R | `varint seq, typed<Result<(), CaptainError>>` |
| 0x96 | `Weather` | R | `u8 kind, f32 intensity, u8 prev_kind, f32 prev_intensity, f32 blend, f32 wind_heading_rad, varint cells, varint runs, runs × (varint len, u8 water_q8)` (kinds = `WeatherKind::code`; runs must cover `cells` exactly) |

`OwnState` (113 bytes, all `f32` unless noted): `pos[3], rot[4], lin_vel[3], ang_vel[3],
speed_mps, engine_rpm, i8 gear, wheels 4 × [omega, steer_rad, compression_m], health`.

`DeltaVehicle`: `varint entity, varint baseline_age (0 = absolute), u8 mask`, then per mask
bit: `0x01` pos `3 × zz` (cm), `0x02` rot `u32` (smallest-three), `0x04` vel `3 × zz`
(cm/s), `0x08` ang `3 × zz` (mrad/s), `0x10` misc `i8 gear, u8 rpm/50, u8 health×255,
i8 steer (±127 = ±0.8 rad), u8 flags (bits 0–3 wheel detached)`. With a baseline the numeric
fields are deltas; absolute entries must carry every field.

Smallest-three rotation (`u32`): bits 30–31 index of the dropped (largest) component, then
three 10-bit fields (bits 20–29, 10–19, 0–9) for the remaining components in order, each
`round(c / (1/√2) × 511) + 511` (511 = exactly 0); the sign makes the dropped component
positive.

`QContact` (chassis space of the damaged vehicle): `3 × i16 point (mm), 3 × i16 normal
×32767 (into the vehicle), varint impulse (0.1 N·s), 3 × zz tangent (0.1 N·s),
zz rel_speed (cm/s), opt other, u16 other_mass_kg`.

`DamageEvent`: `u8 kind` + `0 PartDetached u16 group | 1 ComponentDamaged u8 component,
u8 health×255 | 2 ComponentFailed u8 component | 3 WheelDetached u8 wheel | 4 TotalLoss`.
Component indices follow the declaration order of `wp_contracts::Component` (Engine = 0 …
Lights = 20).

`LatticeSnapshot`: `varint node_count, ceil(n/8) bitmap bytes (bit i%8 of byte i/8 = node i
displaced), 3 × f32 position per displaced node`.

`MapUnit`: `varint entity, u8 kind, zz x_dm, zz y_dm, u8 heading (256 per turn, 0 = +Y,
CCW), u8 wanted, u8 order (0 none, 1 patrol, 2 pursue, 3 intercept, 4 respond,
5 disengage), opt unit` (police unit id for `AssignUnit`; player seats `0x8000_0000 + entity`).
`QTrafficAgent`: `varint id, 3 × zz pos (cm), u16 yaw (65536 per turn, 0 = +Y, CCW),
varint speed (cm/s), u8 flags (wp_contracts::traffic_flags), u8 variant`. `Incident`: `zz x_dm, zz y_dm, u8 kind (0 crash), u8 age_s`.

Lattice hash: FNV-1a 64 over `u32 node_count` then, per node, the displacement from rest
per axis as `i32 = round(Δ × 1000)` little-endian; folded to 32 bits as `lo ^ hi`.

### Channels and transports
`Transport { send(Channel, bytes), poll() }`. WebSocket (prototype): every frame is one
binary message, both channels on one TCP stream. Everything marked U is designed to survive
loss (redundant inputs, delta baselines from acks only, pings) so a UDP/WebTransport
implementation only has to provide unordered datagrams for U and an ordered reliable stream
for R. `sim_link` already tests the protocol under that model.

## Known gaps
- Damage: a dented car's snapshot restores the full damage state, but not node velocities
  or impactors of a crash in progress — such snapshots are provisional and followed up once
  the car settles; until then that replica may differ (hash-checked and resynced). The
  per-beam plastic-work ledger is not replicated (diagnostic only).
- Kinematic traffic is display-only and straight-line extrapolated between 10 Hz updates
  (cuts corners slightly). It is capped at the nearest 48 agents within 120 m.
- Server weather is replicated but does not change grip on the server yet (`wp_vehicle`
  does not call `weather_at`; machine A's step).
- Race events: the in-instance lobby still uses its own `RaceUpdate` / `RaceProgress` /
  `RaceResults` messages; contract `RaceEvent`s are forwarded when the sim emits them
  (`wp_race` is not in `wp_sim` yet). `RaceCommand::Ready` from a non-host is acknowledged
  by a notice only.
- Meta link (`apps/server`): join tokens, presence, kicks and pursuit reports are wired;
  economy write-through (accounts stay in memory per instance) and race reports are not.
- Player police seats are `wp_police` player units and `VehicleKind::Police` in the sim;
  that takes them out of the traffic spawn ring (NET-N9: a lone player cop sees no new
  traffic spawn around it).
- Prediction steps the real vehicle (`predict::RigVehicle` over `wp_vehicle::rig::Rig`) on
  the server's own static city ground (cities with terrain) — rewinds are exact at the 5 Hz
  snapshot ticks; collisions with other cars and surfaces other than asphalt are corrected,
  not predicted (NET-N11).
- Local-car *predicted* contacts are not applied; the own car's damage is server-driven
  (arrives one RTT late) and resynced by the same hash path.
- Police roadblocks / spikes need 120 m of approach sight without side streets
  (`wp_police` site rules): downtown Port Vesper (100 m blocks) never qualifies.
- Heat map and incidents on the captain map are still the net layer's crash density; stakes
  and payout splits are the lobby's own table (NET-N7).
- WebSocket only (both channels reliable). No encryption/auth beyond the reconnect and join
  tokens.
