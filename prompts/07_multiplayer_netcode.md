# PROMPT 07 — Multiplayer: authoritative server, prediction, police roles, interest management (`wp_net`, `apps/server`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`. You own `wp_net` and `apps/server`, and the client-side networking module in `apps/testbed-web/src/net/`. You may read all crates. Branch `ws/net`.

## Goal
32-player city instances (up to 64 later) with racers, player police chase units, one player captain, AI police and traffic — on an authoritative server with client prediction for the local car, deterministic damage replication via events, and interest management so a browser client isn't drowned. Prototype transport: WebSocket (binary). Design for a later swap to WebTransport/UDP.

## Architecture
- **Server owns truth**: runs `wp_sim` at 240 Hz physics / 60 Hz net tick, headless, natively. Traffic and police run server-side only.
- **Clients predict only their own vehicle** using the same `wp_vehicle` + `wp_damage` code (WASM). Remote vehicles are interpolated 100 ms behind with extrapolation ≤ 50 ms. On misprediction beyond a position/rotation tolerance, rewind and re-simulate from the server tick using the input ring buffer (keep 1 s of inputs at 240 Hz = 240 frames; store inputs, not states).
- **Damage replication**: do **not** send node positions. Server sends `SimEvent::Contact` (quantised point/normal/impulse) and `PartDetached/ComponentDamaged/WheelDetached/TotalLoss` events; clients re-run `wp_damage::apply_contacts` deterministically. Every 2 s per visible vehicle, include a 32-bit lattice hash; on mismatch the client requests a `LatticeSnapshot` (quantised node positions, delta-compressed) for that vehicle only. Local-car predicted contacts are reconciled the same way.
- **Snapshots**: per-client, interest-filtered, delta-encoded against the last acked baseline; quantise positions to 1 cm, rotations as smallest-three 10-bit, velocities 16-bit. Priority: own car > racers in same race > pursuers/pursued near the client > traffic within 120 m > everything else at low rate (1–5 Hz for the minimap). Budget 40 kB/s down per client; measure.
- **Roles**: `RoleRequest` → server assigns `Racer | PoliceChase | PoliceCaptain | Spectator` with a per-instance ratio (default: police ≤ 40% of humans, exactly one captain, captain seat offered to highest cop-rank player). Captain client gets `CaptainView` deltas at 2 Hz and a map-only interest set, and sends `CaptainCommand`s (validated by `wp_police`). Chase units are ordinary predicted vehicles with a `UnitOrder` stream.
- **Races**: a lightweight lobby inside the instance (create race from route helper, invite/join, countdown, checkpoint validation server-side, results + payouts handed to `wp_economy`). Night vs day stakes come from `wp_police`/`wp_economy` tables.
- **Join/leave/reconnect**: baseline snapshot on join; 30 s reconnect window keeps the car in-world.
- **Anti-cheat baseline**: inputs clamped and rate-checked; server rejects impossible input timing; all economy on server.
- **Transport abstraction**: `trait Transport { send(reliable|unreliable, bytes); poll() }` with a WebSocket impl now (both channels reliable, fine for prototype) so UDP/WebTransport is a drop-in later.
- **Time sync**: clients estimate server tick with RTT; `tick_offset` smoothing.

## Server app
`apps/server`: config (port, instance size, city file, seed, time ratio), admin HTTP endpoint for status (players, tick rate, ms/tick, bandwidth), structured logs, a `--bots N` flag that connects N scripted clients (drive a route, crash sometimes) for load testing.

## Client module (`apps/testbed-web/src/net/`)
Connect, role picker, prediction/reconciliation around the existing WASM calls (add `wp_wasm` functions you need — coordinate with prompt 04 by listing them in your integration doc; add them yourself if that branch is quiet), remote-vehicle interpolation, captain map view (2D canvas: roads from the road graph, units, incidents, heat map, command radial). Debug overlay: RTT, misprediction count, correction magnitude, bandwidth.

## Tests
- Deterministic lockstep test: server + 4 headless clients in-process, 2 min of scripted driving and crashing; assert every client's lattice hashes match the server's except transiently (< 1% of hash checks mismatch, all resolved by a snapshot within 2 s).
- Prediction: artificial 120 ms RTT + 2% loss + 30 ms jitter (netem-style simulated transport); corrections stay < 0.3 m RMS.
- Bandwidth under 32 bots within budget; ms/tick within budget.
- Role assignment and captain hand-off cases.

## Deliver
`wp_net`, `apps/server`, client net module, tests, README (protocol spec with message tables and byte layouts), `docs/integration/net.md` (exposes / needs: `wp_sim` hooks for input injection + event drain + snapshot extraction, `wp_police` captain API, `wp_economy` payout API; contract requests: any new message or quantisation types).
