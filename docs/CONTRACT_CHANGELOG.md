# Contract Changelog

**Rule:** any change to `crates/wp_contracts` or `schemas/` bumps `wp_contracts::CONTRACT_VERSION`
(semver: breaking → minor while < 1.0, additive → patch) and gets an entry here stating what
changed, why, and which workstreams must update. Only the integration lead edits the contract.
Every session reads this file at start.

## v0.3.0 — applied 2026-10-19 (branch `lead/contract-v0.3.0`)

- Amendment (lead round 8, additive): `DnfReason::Fled` (AI entrants abandoning a race to flee police; machine L).

The deferred v0.2.0 batch, using each owner's final list. Behaviour is unchanged: no
simulation code path reads the new data yet (only new fields are filled), so every golden state
hash is identical; only the `contract=` header of `hashes.txt` changes (a header-only
`[bless]` commit). Schemas regenerated (`tire_def`, `lattice_def`). New modules in
`wp_contracts`: `police`, `economy`, `race`, `damage`, `world`, `weather`; all re-exported at
the crate root.

**Breaking — `SimEvent`:**
| Change | Who | What to do |
|---|---|---|
| `SimEvent::PoliceEvent { kind, payload }` → `SimEvent::Police(PoliceEvent)` | E emits; C, F, I, J, K, M, O, P consume | match `SimEvent::Police(e)`. Kind codes: `PoliceEvent::kind()` / `wp_contracts::police_kind` (same numbers; `wp_police::events::kind` re-exports it). `wp_police::PoliceEventExt` keeps `to_sim_event` / `from_sim_event` for one release. |
| `SimEvent::EconomyEvent { kind, payload }` → `SimEvent::Economy(EconomyEvent)` | G emits; C, F, M, P consume | `wp_economy::EconomyEvent::{to_contract, from_contract, to_sim_event, from_sim_event}` convert; the ledger type is unchanged (meta's archive is unaffected). |
| New `SimEvent::Race(RaceEvent)` | L emits; F, G, E, C, P consume | `wp_race::RaceSignal::{to_event, to_sim_event}`. |
| New `SimEvent::WeatherChanged { kind, intensity, forced }` (W-2) | N emits; C, J, P consume | `wp_weather::WeatherChange::to_sim_event` for whoever drains `drain_changes`. |
| simcli `event_name` keeps `"PoliceEvent"` / `"EconomyEvent"` for the typed variants | 10 | scenario event counts are unchanged; new names `"RaceEvent"`, `"WeatherChanged"`. |

**Police (E's final lists; police.md #2, #3, #5; race R2):** `UnitId`, `ToolId`, `IncidentId`,
`PursuitOutcome` (+ `is_capture`), `DisableCause`, `UnitKind`, `PoliceEvent` (13 variants, codes
1–13), `UnitOrder`, `CaptainCommand`, `SiteError`, `CaptainError`, `PolicePresence`, `RoadEdgeId`.
`wp_police` re-exports them from the old paths (`wp_police::{events, captain, tools}::*`).
- `PoliceEvent::PursuitEnded` gains `level: WantedLevel` (before the reset), `capturer:
  Option<UnitId>`, `assists: Vec<UnitId>` (economy #06 row). `wp_police` fills them: capturer =
  closest in-service ground unit assigned to the racer (else closest ground unit), assists = the
  other assigned units, ascending; non-captures get `None` / empty. Read-only; no state change.
- `CaptainCommand::{to_bytes, from_bytes}` (inherent) → free functions
  `wp_police::{command_to_bytes, command_from_bytes}` (same JSON, still decodes `segment`).
- **Not taken:** audio A-2 (`Radio { priority }`) and design C9 (line id) — not in E's final list;
  E decides. UI-2 (`UnitOrder::Hold`) — optional, E decides.

**Economy (G's final list, economy.md #1, #3, #6; F NET-4, NET-5; meta MC-1):** `AccountId(u32)`
(in `ids.rs`), `CarUid`, `RaceType` (+ `ALL`, `id`, `from_id`), `CopXpReason`, `EconomyReason`
(codes as before; `RaceEntryFee` → `StakeCharged { race: RaceType }`, `RacePurse` →
`RacePayout { race, placement }`), `EconomyEvent { account, seq, day, reason, car, deltas,
cash_after }`. `wp_economy` re-exports `CarUid`, `CopXpReason`, `RaceType`; `PursuitEnd` is now
an alias of the contract `PursuitOutcome`; new `PoliceSignal::from_sim_event`.
- `ShopReported.district` stays a `String`: `prices.toml [district_labour]` keys (`Docks`,
  `Hills`, `default`) are not all `DistrictId`s. G aligns the keys, then the field becomes
  `DistrictId` in a later patch.
- Not done: G switching its ledger from `PlayerId` to `AccountId` (conversion uses the same
  number today); race R5 (economy `RaceType` keyed by the race `RaceKind`) needs G + L to agree
  on the purse table first.

**Race (L's final list, race.md R1, R2, R6; meta MC-2):** `RaceEventId`, `VenueId`, `CrewId`,
`RaceKind` (+ `key`, `KEYS`, `from_key`, `uses_corridor`, `is_head_to_head`), `Legality`
(+ `key`), `DnfReason`, `DqReason`, `RacerStatus` (+ `class`), `RaceResultRow`, `RaceEvent`
(12 variants), `RaceCommand { Enter, Withdraw, Ready }`. `wp_race` re-exports them under its old
names (`EventId` = `RaceEventId`, local `DistrictId` = contract `ZoneId`); `RacerResult` stays
in `wp_race` and converts with `From<&RacerResult> for RaceResultRow`.
- `RaceEvent::TerritoryChanged` uses `zone: ZoneId` (R1 said `district`): territory is per
  zone; `DistrictId` is the district *kind* (see world). R3 is answered by `ZoneId`.

**Damage (B's final shapes, damage.md #4–#7):** `DamageSnapshot`, `DamageSummary`;
`DamageSim` gains `summary()`, `save_state()`, `restore_state(&DamageSnapshot) ->
Result<(), String>`, `apply_repair(&[RepairItem])`; `BodyRegion::Accessory`.
- `wp_damage` implements them (no inherent versions existed on `ws/damage`; new
  `wp_damage/src/snapshot.rs`): restore starts from the pristine model and leaves it asleep;
  repair edits a snapshot (region / group nodes and their beams back to rest) and recomputes
  health and total loss with the last mode table it was stepped with. `Accessory` has health
  weight 0 and no handling effect. Tests: round trip, size / lattice refusal, repair-all.
- H: the cruiser push bar can move from `FrontBumper` + group 8 to `Accessory` (data change);
  B: `gen-lattice --accessory`.

**World (D's proposals, world.md #6–#8; police #5):** `DistrictId` (= the old
`wp_world::network::DistrictKind`, which is now a re-export), `ZoneId(u32)`, `RoadEdgeId`,
`RoadClass`, `RoadEdgeInfo`, `PoiKind`, `PoiInfo`, trait `RoadNetworkQuery`;
`TrafficAgentSnapshot` + `traffic_flags` (in `net.rs`).
- `wp_world::RoadNetworkIndex::new(&CityMap)` implements `RoadNetworkQuery` (lead's first cut:
  brute-force `nearest_on_road`, cameras at every third signalised junction like E's graph).
  `From<&AgentView> for TrafficAgentSnapshot` (no headlights bit yet: `AgentView` has none).
- E still builds its own `RoadGraph`; moving it onto the trait is E's step.

**Weather (weather.md W-1…W-5):** `WeatherKind`, `LocalConditions` (+ `DRY`) moved from
`wp_weather` (re-exported there); `WorldQuery::weather_at(p) -> LocalConditions` with a default
returning `DRY` (additive: every implementor still compiles); `SurfaceType::PaintedLine`;
`TireDef::treaded: Option<bool>` (serde-default; `wp_weather::TireProfile` honours it, else
infers as before); `ServerToClient::Weather(WeatherSnapshot)` (W-5).
- `wp_weather::{WithWeather, Weathered}` answer `weather_at`; `wp_vehicle` does not call it yet
  (A's step), so the interim `WetAsphalt` remap still decides grip. `surfaces.toml` has no
  `PaintedLine` row yet (N adds it; `SurfaceTable::surface` falls back to asphalt). D reports
  markings; H sets `treaded: false` on `semi_slick.json`.
- Exhaustive `SurfaceType` matches got arms: `wp_vehicle` snapshot code 8, `wp_audio` sounds
  like asphalt.

**Net (F's NET-1, NET-3, NET-4, NET-5):** NET-1 decision: `wp_net::protocol` is the source of
truth for the **wire** (handshake, acks, time sync, quantised snapshots, lobby — the quantised
types stay in `wp_net`, NET-2 rejected); the contract keeps the *logical* messages with typed
payloads: `ClientToServer::CaptainCommand(CaptainCommand)` (was `Vec<u8>`),
`ClientToServer::RaceCommand(RaceCommand)`, `ServerToClient::Snapshot { .., traffic:
Vec<TrafficAgentSnapshot> (serde-default), .. }`, `ServerToClient::CaptainReply(Result<(),
CaptainError>)`, `ServerToClient::Weather(WeatherSnapshot)`. NET-4 = `StakeCharged` /
`RacePayout`; NET-5 = `AccountId`. Meta MC-6 (`Hello.join_token`, `Welcome.meta_reconnect_token`)
is wire-only, so it belongs in `wp_net::protocol` (F).

**Opponent #2 (AI racer flag):** no contract change needed: `wp_sim::VehicleKind::AiRacer`
exists; whether `wp_world::TrackedVehicle` carries it is D's call.

**Migration checklist per workstream:**
- A (vehicle): optionally call `world.weather_at(wheel_pos)`; handle `PaintedLine` grip.
- B (damage): done here; review `snapshot.rs` and own it; `gen-lattice --accessory`.
- C (testbed): no TS mirrors events; new `SimEvent` variants appear in JSON event feeds.
- D (world): `DistrictKind` is the contract `DistrictId`; own `RoadNetworkIndex`; report
  `PaintedLine`; headlights in `AgentView`.
- E (police): types moved (re-exported); `PursuitEnded` fields filled; consume
  `RoadNetworkQuery`, `PolicePresence`.
- F (net): send typed `CaptainCommand` / `RaceCommand`; replicate `TrafficAgentSnapshot`;
  `CaptainReply`; weather snapshot; `join_token` in your protocol.
- G (economy): conversions in place; switch the ledger to `AccountId`, `ShopReported` to
  `DistrictId`, pay cops from `capturer` / `assists`.
- H (cars): `treaded: false` on slicks; push bar → `Accessory`.
- J (audio), O (opponent), K/M/P: match the typed variants (done here where code existed).
- L (race): types moved (re-exported); emit `SimEvent::Race` from the server.
- N (weather): `PaintedLine` row in `surfaces.toml`; emit `WeatherChanged` from the server loop.
- UI kit (16): run `npm run gen:types` and switch `decodePoliceEvent` / `decodeEconomy` from
  payload bytes to the typed variants; re-record fixtures (`gen:police`, `gen:fixtures`).

## v0.2.0 — applied 2026-10-19 (lead run 3)

Physics is unchanged: every golden state hash is identical before and after (only the contract
version in file headers changed). Schemas regenerated; all `data/` validates.

**Breaking — you must update if you implement or call these:**
| Change | Who | What to do |
|---|---|---|
| `VehicleSim::step(.., world, ground: &dyn GroundQuery, dt, out)` + new `GroundQuery` trait / `GroundHit` in `traits.rs` | A, D, F, anyone stepping a vehicle via the trait | pass a ground: `wp_vehicle::FlatGround` (tests) or `RapierGround`; both implement `GroundQuery`. `wp_vehicle::ContractGround` adapts the other way. |
| `WorldQuery::line_of_sight(a, b) -> bool` (required) | every `WorldQuery` impl (D, E, K, test worlds) | implement it; worlds without occluders return `true`. D's `CityMap` tests building footprints (lead's first cut). |
| `SimEvent::TrafficViolation` gains `pos: [f32; 3]` | D (emits), E, L (match) | fill the world position; match with `{ vehicle, kind, .. }`. |
| New `SimEvent::GlassShattered { vehicle, region }`, `RegionDamaged { vehicle, region, severity }` | B (emits), C, F, J, M | exhaustive matches need arms. B should start emitting them. |
| `PriceTable` gains `labour_hours: BTreeMap<RepairItem, f32>` and `repair_parts_fraction: Option<f32>` (serde-defaulted) | B, G | struct literals need the fields. `wp_damage` now uses both; G should fill them from `prices.toml`. |
| `RepairLine::parts_cost` / `labour_hours` are **unscaled**; only `total` includes `repair_cost_scale` | B, G | `wp_damage` changed to match (it used to scale `parts_cost`). |
| `WantedLevel` moved to `wp_contracts` (same variants and methods) | E, G, F, M | `wp_police` re-exports it, so `wp_police::WantedLevel` still works; prefer `wp_contracts::WantedLevel`. G can drop its `wp_police` dependency. |

**Additive (optional fields, serde-defaulted, old data still loads):**
- `VehicleDef::fuel_capacity_l: Option<f32>` (H fills; fuel in `mass_kg` at 50 %).
- `WheelDef::inertia_kgm2: Option<f32>` (default `0.7·m·r²`), `DrivetrainDef::efficiency: Option<f32>`
  (default 0.85) — both honoured by `wp_vehicle` now.
- `DrivetrainDef::torque_converter: Option<TorqueConverterDef { stall_rpm, stall_torque_ratio,
  coupling_rpm }>` — **A implements** (ignored today); H then sets it on the 5 automatics.
- `DriveLayout::AwdOnDemand { base_front_split, max_rear_split, engage_slip }` — lead's first-cut
  coupling in `wp_vehicle::base_split`; A owns tuning; H can switch Lindwurm / Wolfhound.
- `WheelState { fx_n, fy_n }` — applied tire-frame forces, filled by `wp_vehicle`. C can drop its
  force estimate; F should decide whether they go on the wire.
- `BrakeDef` docs: capacity-per-wheel + pedal-split semantics with A's worked example.
- `wp_math::{atan, atan2}` (moved from `wp_world::geom`, which re-exports them).

**Deferred to v0.3.0** (need final variant lists from their owners; send them in your integration
doc): typed `PoliceEvent` + `CaptainCommand` (E, with F/I), typed `EconomyEvent` incl.
`StakeCharged`/`RacePayout` (G, F), full net protocol message sets (F), `DamageSim::{save_state,
restore_state, apply_repair}` + `DamageSummary` (B, G), shared road-network trait (D, E),
`TrafficAgentSnapshot` (D, F), `Accessory` body region (B, H), `DistrictId` (D, G).

## v0.2.0 — original plan (2026-10-19; see the applied entry above for what landed)

**Still v0.1.0 on the branch.** These are the decisions on every request filed so far, published
early so running workstreams can code toward them. All requesting workstreams (A–I) have now
reported; the bump is the next lead run's first job. Requests filed by 05/07/13/14/15
before then join it.

| Change | From | Decision | Who must update |
|---|---|---|---|
| `GroundQuery` trait (`cast_ray(origin, dir, max_m) -> Option<GroundHit { dist_m, point, normal }>`); `VehicleSim::step` gains `ground: &dyn GroundQuery` | 02 #1 | accepted (breaking) | 02, lead (`wp_sim`) |
| `BrakeDef` doc: bias semantics as implemented by 02, with a worked example | 02 #2, 09 #4 | accepted (doc) | 09 re-checks data |
| `WheelDef::inertia_kgm2: Option<f32>` (default `0.7·m·r²`) | 02 #3 | accepted | 02 |
| `DrivetrainDef::efficiency: Option<f32>` (default 0.85) | 02 #4 | accepted | 02 |
| `DriveLayout::AwdOnDemand { base_front_split, max_rear_split, engage_slip }` | 02 #5, 09 #2 | accepted | 02, 09, 08 |
| `DrivetrainDef::torque_converter: Option<TorqueConverterDef { stall_rpm, stall_torque_ratio, coupling_rpm }>` | 09 #1 | accepted | 02, 09 |
| `VehicleDef::fuel_capacity_l: Option<f32>` (fuel in `mass_kg` at 50 %) | 09 #8 | accepted | 02, 03, 09 |
| `BodyRegion::Accessory` (push bars, body kits) | 09 #7 | accepted in principle; shape with 03 | 03, 08, 09, 04 |
| `WheelState { fx_n, fy_n }` (applied tire-frame forces) | 04 TB-1 | accepted | 02, 04, 07 |
| `DamageSim` region energy / node strain accessors | 04 TB-2 | accepted in principle; shape with 03 | 03, 04 |
| Typed `PoliceEvent` (replaces `{kind, payload}`, gives stable kind codes) | 10 H1, 06 | accepted; designed with 06 | 06, 10, 08 |
| Typed `EconomyEvent` (from `wp_economy::EventReason`) | 08 #1 | accepted; same bump as police/race events | 08, 07, 04 |
| `RepairLine` unscaled-cost doc; `PriceTable::labour_hours: BTreeMap<RepairItem, f32>` | 08 #2 | accepted | 03, 08 |
| `DamageSim::{save_state, restore_state, apply_repair}` | 08 #4 | accepted | 03, 08 |
| `WantedLevel` moves into contracts | 08 #5 | accepted | 06, 07, 08 |
| `DamageSummary { health, total_loss }` next to the estimate | 08 #3 | deferred to 03's merge | 03, 08 |
| `DistrictId` | 08 #6 | deferred to 05's merge | 05, 08 |
| `Sim::debug_intents()` | 04 TB-5 | deferred to 05/06 | 05, 06 |
| `SimEvent::GlassShattered { vehicle, region }`, `SimEvent::RegionDamaged { vehicle, region, severity }` | B #1, #3 | accepted | B, C, F, E |
| `PriceTable::repair_parts_fraction: Option<f32>` | B #2 | accepted (with G #2's labour map) | B, G |
| `SimEvent::TrafficViolation` gains `pos: [f32; 3]` | D #2 | accepted | D, E, F |
| `TrafficAgentSnapshot` + snapshot field | D #3 | accepted; designed with F | D, F, C |
| `wp_math::atan2` (moved from `wp_world::geom`) | D #4 | accepted (lead) | D, A, E |
| Full net protocol message sets into contracts | F NET-1 | accepted in principle; F sends final list | F, C |
| Durable account id separate from per-instance `PlayerId` | F NET-5 | accepted | F, G |
| `StakeCharged` / `RacePayout` economy events | F NET-4 | accepted (inside typed `EconomyEvent`) | G, F |
| Quantised wire types in contracts | F NET-2 | rejected for now (stay in `wp_net`) | — |
| `VehicleSim::teleport` | D #1 | rejected: inherent `Vehicle::set_pose_and_speed` is enough | — |
| Typed `PoliceEvent` + `CaptainCommand` (from `wp_police`; no JSON payloads) | E #2, #3, F NET-3, I H1 | accepted | E, F, G, I |
| `WorldQuery::line_of_sight(a, b) -> bool` | E #4 | accepted (D implements) | D, E |
| Shared read-only road-network trait | E #5 | accepted in principle; shape from D + E | D, E |
| Rear drum brakes + fade inputs | 09 #3 | deferred (post Checkpoint 1) | — |
| Solid axles; roll-centre heights | 09 #5, #6 | deferred (post Checkpoint 2) | — |
| `VehicleState::ffb` | 02 #6 | rejected (FFB is local-only; keep `VehicleSim::ffb`) | — |
| `Systems` registry in contracts | 10 H2 | rejected for now | — |

Done without a contract change (2026-10-19, `wp_sim`): `Sim::add_static_box` (TB-3),
`Sim::set_mode_table` (TB-4), `Sim::place_vehicle`; lattice nodes in the state hash (TB-6, by 10's
hash spec v1); Rapier contacts → `ContactImpulse` → damage + `SimEvent::Contact`.

**Not a contract, but shared format decisions (2026-10-19):** simcli scenario format **v2**
(`apps/simcli/SCENARIOS.md`) is the only scenario format; simcli `replay.bin` v1 is the canonical
replay format (the testbed will write it).

## v0.1.0 — initial contract (prompt 01)

Everything specified in `prompts/01_architecture_contracts.md`, plus these deliberate additions
(flagged so nobody is surprised):

- `VehicleDef::body_size_m: [w, l, h]` — needed for the debug box, lattice generation and the
  fallback chassis collider; the spec listed no overall dimensions.
- `VehicleDef::aero_downforce_coeffs` is `[front, rear]` Cl·A in m² (positive = downforce), added
  on top of `lift_front/lift_rear`.
- `ModeTable::mode` (which mode the table is) and `ModeTable::cabin_intrusion_total_loss_m`
  (prompt 03 asks for the Sim 0.25 m / Arcade 0.5 m cabin limit to live in the tables).
- `ModeTable::wheel_detach_scale` semantics: susceptibility — the wheel mount threshold is
  *divided* by it, so Arcade's 0.5 keeps wheels on longer.
- `HandlingEffects` made exhaustive up front: added `tire_puncture: [f32; 4]` (prompt 06's spike
  strips), `gearbox_health`, `fuel_leak_rate`, `engine_dead`, `lights_broken` (prompt 03's
  component effects). `HandlingEffects::NEUTRAL` / `Default` = undamaged.
- `ContactImpulse` (input of `DamageSim::apply_contacts`): chassis-space point/normal, normal and
  tangential impulse, closing speed, other entity, other mass (`0.0` = static).
- `PriceTable` / `RepairEstimate` / `RepairLine` / `RepairItem` live in contracts because the
  `DamageSim` trait signature needs them (prompt 03 suggested `wp_damage`; this is the promoted
  form). Minimal — 03 and 08 should request refinements.
- `SignalState { Green, Yellow, Red, Off }`; `WORLD_ENTITY = EntityId(u32::MAX)` for contacts with
  static geometry.
- `SimEvent::PoliceEvent { kind: u16, payload: Vec<u8> }` and `EconomyEvent { kind, payload }`
  are opaque until 06/08 request concrete payloads. Same for `ClientToServer::CaptainCommand`
  and `ServerToClient::Baseline`. `ClientToServer::RoleRequest { role }`,
  `ServerToClient::RoleAssigned { player, role, vehicle }`.
- `AssistProfile::ai()` and `AssistProfile::for_source()` presets (prompt 05/06 use `ai()`).
  Note `AssistProfile` lives in contracts (needed by the `VehicleSim` trait) and is re-exported
  by `wp_vehicle`.
- Constants `PHYSICS_TICKS_PER_NET_TICK = 4`, `GRAVITY_MPS2 = 9.81`, wheel index constants
  `WHEEL_FL..WHEEL_RR`.
- Loaders: `load_car`, `load_{vehicle,tire,material,lattice}_def`, `load_mode_table`,
  `parse_*` + `resolve_car` (same validation, for embedded/browser data), `validate_*`,
  typed `LoadError`.
- Serde: data structs `deny_unknown_fields`; enums externally tagged (binary-codec friendly).
- `TimeOfDay::night_factor()` is a smoothstep polynomial (no transcendental functions) so it is
  bit-identical on all targets; `DayPhase` boundaries: Dawn 05–07, Day 07–18, Dusk 18–20,
  Night 20–05.
