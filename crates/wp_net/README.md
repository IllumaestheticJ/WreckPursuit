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
| `quant` | quantisation of positions, rotations, velocities, inputs, contacts |
| `snapshot` | per-entity delta compression (`DeltaVehicle`), full-precision `OwnState` |
| `lattice` | 32-bit lattice hash, `LatticeSnapshot`, `ReplicaDamage` trait |
| `protocol` | `ClientMsg` / `ServerMsg` + codec (tables below) |
| `interest` | interest categories, priority accumulators, byte budget |
| `roles` | `RoleManager`: police ratio, single captain, captain-seat offers |
| `race` | `RaceLobby`: create/invite/join, countdown, checkpoints, results, payouts |
| `netsim` | `NetSim` trait (what the server needs from a sim) + `WpSim` adapter over `wp_sim::Sim` |
| `server` | `ServerInstance<S: NetSim>`: the authoritative instance |
| `client` | `Client`: prediction, interpolation, damage replicas, time sync, lobby state |
| `predict`, `interp`, `timesync` | client building blocks |
| `sim_link` | in-memory netem-style link (latency, jitter, loss) implementing `Transport` |
| `bot` | scripted driver (`wp-server --bots`, tests) |
| `testkit` | `ArenaSim` crash arena + `ToyLattice` (damage traffic while `wp_sim`/`wp_damage` are stubs) |
| `native` (feature) | tokio game-loop thread, websocket acceptor, websocket client `Transport` |

The `native` feature (tokio, tungstenite) must never be enabled by anything reachable from
`wp_wasm`; `cargo tree -p wp_wasm --target wasm32-unknown-unknown` shows no tokio.

## Tests

```sh
cargo test -p wp_net                      # unit + integration (≈25 s debug)
cargo test -p wp_net --release -- --nocapture   # prints bandwidth / timing / hash numbers
cargo test -p wp_server                   # real websockets end-to-end, admin endpoint, --bots
```

| test | asserts |
|---|---|
| `tests/lockstep.rs` | server + 4 clients, 2 min scripted driving/crashing in the arena: < 1 % of lattice-hash checks mismatch, every mismatch resynced within 2 s, all replicas equal the server at the end (one desync is injected deliberately) |
| `tests/prediction.rs` | 120 ms RTT, 2 % loss, ±30 ms jitter: correction RMS < 0.3 m, < 5 % of reconciles mispredict |
| `tests/bandwidth.rs` | 32 bots: every client's worst second ≤ 40 kB/s, police cap = 12 of 32, net tick < 8 ms (release) |
| `tests/session.rs` | reconnect within 30 s keeps the car, expiry removes it; Bye; full instance; version mismatch; input timing/rate anti-cheat; role ratio, captain orders, captain hand-off; race lobby → countdown → checkpoints → results → payouts |
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
exactly. The down-link budget is a per-client byte bucket (refill 95 % of 40 kB/s, burst one
tick), so the snapshot spike is repaid by the following ticks. The visual jump is absorbed by a render offset that decays
with a 100 ms time constant. Inputs are quantised (`quant::quantize_input`) **before** the
client simulates them, so both sides simulate identical values.

### Damage replication
Node positions never travel in normal play. Per physics tick the server reports the contacts
applied to each vehicle's damage model (`NetSim::drain_applied_contacts`). Contacts are
quantised in chassis space (`QContact`) and the authority **applies the dequantised value**,
so a client re-running `apply_contacts` with the same contacts at the same tick gets the
same lattice. Replicas run empty steps for ticks without contacts (capped at 240 catch-up
steps). Every 2 s per tracked vehicle the server sends a 32-bit hash of node displacements
quantised to 1 mm; a client that disagrees requests a `LatticeSnapshot` of that vehicle only.
Snapshots carry exact `f32` positions of displaced nodes (bitmap-delta against the rest
pose), because a rounded restore drifts again on the next contact (measured: 9 mismatches
per desync with 1 mm snapshots vs 1 with exact ones). The client's own car is replicated the
same way; its handling effects feed the predictor.

### Interest management
Per client and net tick: own car → `OwnState`; others classified as **SameRace** (weight 1.0,
≤ 60 Hz) > **Pursuit** (police↔racer within 300 m, 0.8, ≤ 60 Hz) > **Near** (< 120 m incl.
traffic, 0.5, ≤ 30 Hz) > **Far** (players and police only, 0.05, ≤ 5 Hz; distant traffic is
not replicated). Accumulators grow by weight each tick; eligible entities go out in
descending order until `40 000 / 60` bytes minus this tick's reliable bytes are used. The
captain gets no 3D entities: a 2 Hz `CaptainView` delta instead.

### Delta compression
Each entity is delta-encoded against the newest state of that entity the client has acked
(the client acks the newest snapshot tick in every `Inputs`; baselines older than 1 s are not
used). Unchanged fields are masked out: an unchanged car costs 3 bytes.

### Roles
`RoleRequest` → `RoleManager`: Racer/Spectator always; police (chase + captain) ≤ 40 % of
humans (min 1 seat); one captain. A vacant captain seat is offered to the highest cop-rank
chase unit (10 s, then the next); a disconnected captain keeps the seat for the reconnect
window. Captains do not drive; chase units are ordinary predicted vehicles receiving
`UnitOrder`s.

### Anti-cheat baseline
Inputs are clamped by quantisation; ticks already simulated are ignored; ticks more than
1 s ahead and more than 300 new ticks per second are rejected and counted; all money and
reputation changes happen on the server; captain commands are role-checked, rate-limited
(10/s) and validated (units must be police chase units, pursuit targets must exist).

## Protocol

One transport frame = one message: `u8 tag` + body. Integers in bodies are little-endian
fixed width or LEB128 `varint`; `zz` = zigzag varint; `str`/`bytes` = varint length + data;
`opt` = varint, 0 = none, else value + 1. Decoders reject unknown tags, oversize lengths
and trailing bytes. `PROTOCOL_VERSION = 2`; `Hello` must match it and the contract version.

### Client → server

| tag | message | channel | body |
|---|---|---|---|
| 0x01 | `Hello` | R | `u16 protocol, str contract (≤16), str name (≤32), u8 input_source, u64 reconnect_token` |
| 0x02 | `Inputs` | U | `varint ack_snapshot_tick, varint first_tick, u8 n (≤32), n × QInput` |
| 0x03 | `RoleRequest` | R | `u8 role` (0 racer, 1 police chase, 2 captain, 3 spectator) |
| 0x04 | `CaptainCommand` | R | `bytes payload (≤256)` — see `CaptainOrder` |
| 0x05 | `Ping` | U | `u32 client_ms` |
| 0x06 | `LatticeRequest` | R | `varint vehicle` |
| 0x07 | `RaceCreate` | R | `u8 laps, varint n (≤64), n × (zz x_dm, zz y_dm)` |
| 0x08 | `RaceJoin` | R | `varint race` |
| 0x09 | `RaceInvite` | R | `varint race, varint player` |
| 0x0A | `RaceStart` | R | `varint race` (host only) |
| 0x0B | `RaceLeave` | R | `varint race` |
| 0x0C | `Bye` | R | — (no reconnect window) |

`QInput` (7 bytes): `i8 steer×127, u8 throttle×255, u8 brake×255, u8 handbrake×255,
u8 clutch×255, u8 buttons (1 gear up, 2 gear down, 4 horn, 8 headlights, 16 siren), u8 tool_use`.

`CaptainOrder` payload (placeholder until `wp_police` owns it, NET-3): `u8 kind (1 move,
2 block, 3 pursue, 4 hold), varint n (≤64), n × varint unit`, then for move/block
`zz x_dm, zz y_dm`, for pursue `varint target`.

### Server → client

| tag | message | channel | body |
|---|---|---|---|
| 0x81 | `Welcome` | R | `varint player, u64 token, varint server_tick, u8 mode, u16 protocol, f32 time_of_day_h` |
| 0x82 | `Reject` | R | `str reason` — connection closed after it |
| 0x83 | `RoleAssigned` | R | `varint player, u8 role, opt vehicle, str car_id` |
| 0x84 | `CaptainOffer` | R | `varint expires_tick` |
| 0x85 | `EntityEnter` | R | `varint entity, str car_id, u8 kind (0 racer, 1 police chase, 2 AI police, 3 traffic), opt owner` |
| 0x86 | `EntityLeave` | R | `varint entity` |
| 0x87 | `Snapshot` | U | `varint tick, varint input_ack, u8 has_own, [OwnState], u8 has_full, [bytes own_full ≤4096: wp_vehicle::VehicleSnapshot], varint n, n × DeltaVehicle` |
| 0x88 | `Damage` | R | `varint vehicle, varint tick, varint n, n × QContact, varint m, m × DamageEvent` |
| 0x89 | `LatticeHash` | R | `varint vehicle, varint tick, u32 hash` |
| 0x8A | `LatticeSnapshot` | R | `varint vehicle, varint tick, LatticeSnapshot` |
| 0x8B | `Pong` | U | `u32 client_ms, varint server_tick` |
| 0x8C | `CaptainView` | R | `varint tick, u8 full, varint n, n × MapUnit, varint m, m × varint removed, varint k, k × Incident, varint h, h × (i16 cell_x, i16 cell_y, u8 heat)` |
| 0x8D | `UnitOrder` | R | `u8 kind, zz x_dm, zz y_dm, varint target` |
| 0x8E | `RaceUpdate` | R | `varint race, varint host, u8 state (0 lobby, 1 countdown, 2 running, 3 finished), varint start_tick, zz stake, u8 laps, varint n, n × (zz x_dm, zz y_dm), varint m, m × varint member` |
| 0x8F | `RaceProgress` | R | `varint race, varint player, varint next_checkpoint` |
| 0x90 | `RaceResults` | R | `varint race, varint n, n × (varint player, u8 place (0 = DNF), varint time_ticks, zz payout)` |
| 0x91 | `RaceInvited` | R | `varint race, varint from` |
| 0x92 | `Account` | R | `zz cash, zz rep, varint cop_rank, varint bounty` |
| 0x93 | `Notice` | R | `str text` |

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
CCW), u8 wanted, u8 order`. `Incident`: `zz x_dm, zz y_dm, u8 kind (0 crash), u8 age_s`.

Lattice hash: FNV-1a 64 over `u32 node_count` then, per node, the displacement from rest
per axis as `i32 = round(Δ × 1000)` little-endian; folded to 32 bits as `lo ^ hi`.

### Channels and transports
`Transport { send(Channel, bytes), poll() }`. WebSocket (prototype): every frame is one
binary message, both channels on one TCP stream. Everything marked U is designed to survive
loss (redundant inputs, delta baselines from acks only, pings) so a UDP/WebTransport
implementation only has to provide unordered datagrams for U and an ordered reliable stream
for R. `sim_link` already tests the protocol under that model.

## Known gaps
- `wp_sim` does not extract contacts or offer a contact-quantisation hook, so on the real
  stack no damage is replicated yet (NET-N1); `testkit::ArenaSim` exercises the path.
- `Sim` has no despawn: `WpSim` parks and reuses vehicles (MAX_VEHICLES = 64 limits churn).
- Prediction steps the real vehicle (`predict::RigVehicle` over `wp_vehicle::rig::Rig`,
  bit-identical to `wp_sim` for a lone car on flat ground). Rewinds are exact at the 5 Hz
  snapshot ticks only; in between they use `set_state`. The client has no city geometry,
  surface map or other cars, so collisions are corrected, not predicted.
- Local-car *predicted* contacts are not applied; the own car's damage is server-driven
  (arrives one RTT late) and resynced by the same hash path.
- No traffic/AI police exist yet; `NetSim::ai_kind` defaults to traffic.
- Captain command payload, wanted levels, heat and stakes are placeholders until
  `wp_police`/`wp_economy` expose them (NET-3, NET-4). Accounts are per instance, in memory.
- WebSocket only (both channels reliable). No encryption/auth beyond the reconnect token.
