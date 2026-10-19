# Meta services integration notes

- **Workstream / prompt:** 18 — meta services (accounts, persistence, matchmaking, leaderboards, admin) — machine O
- **Branch:** `ws/meta`
- **Crates / folders owned:** `apps/meta` (crate `wp_meta`, binary `wp-meta`), `tools/meta-cli`
- **Contract version built against:** 0.2.0

Design, deploy notes, perf and gaps: `apps/meta/README.md`. HTTP API: `apps/meta/openapi.yaml`.

## Exposes

### For `apps/server` (workstream 07 / machine F)

Use `wp_meta::client::MetaClient` (blocking; call from `spawn_blocking` or a side thread, never
from the tick) or speak HTTP directly. All instance calls send `Authorization: Bearer <token>`.

| When | Call | Notes |
|---|---|---|
| startup | `POST /v1/instances` with the shared `WP_META_INSTANCE_KEY` → `{instance_id, instance_token, heartbeat_interval_ms}` | body: name, region, city, `mode: DamageMode`, bracket (`D`..`S`/`open`), connect address, `slots` per role (max 1 captain) |
| every `heartbeat_interval_ms` (5 s) | `POST /v1/instances/{id}/heartbeat` `{population, players: [account_id], draining, tick_ms}` → `{kicks, flags}` | **disconnect every `kicks[].account_id`** (show `reason`); apply `flags` (e.g. `illegal_day_race_frequency`). 3 missed beats = down: unclaimed seats go back to the queue. **Always list connected players in `players`**: a verified join holds its seat until a heartbeat lists that player (or 2 min pass), so the matcher never overbooks between join and heartbeat |
| client connects | client sends the `join_token` it got from matchmaking; server calls `POST /v1/instances/{id}/join/verify` → `{account_id, display_name, role, party_id, reconnect_token, reconnect_expires_ms}` | 401 `invalid_join_token` → refuse the connection. Give `reconnect_token` to the client. Spawn in `role`; keep `party_id` members together |
| player's economy | `wp_meta::client::RemoteAccountStore::new(client, instance_id)` implements `wp_economy::AccountStore` (`load` / `commit` / `events_since`) | drop-in for `InMemoryStore`. Key: `PlayerId(account_id)`. Accounts already exist (created with starting cash at signup). Only the player's **current** host may read/write (403 `not_hosted`). Commit conflicts come back as `StoreError::SeqConflict` → reload and redo |
| race ends | `POST /v1/instances/{id}/results/race` `RaceReport { report_key, route_id, race_type, mode, bracket, entries: [{account_id, position (0 = DNF), finish_time_ms, clean}] }` | **economy first** (`settle_race` + commit), then the report. `report_key` makes retries safe. `clean` = `RaceResult::is_clean()`. `race_type: "time_attack"` → time boards only |
| pursuit ends | `POST /v1/instances/{id}/results/pursuit` `{report_key, racer_id, max_wanted: WantedLevel, bounty, outcome: escaped/captured, captured_by, assists}` | feeds heat history + bounty/captures boards |
| district changes hands | `POST /v1/instances/{id}/territory` `{district_id, crew_id \| null}` | city = the instance's city |
| shutdown | `DELETE /v1/instances/{id}` | stop heartbeating first or after; token is revoked |

Reconnect: a dropped client calls `POST /v1/matchmaking/reconnect {reconnect_token}` with its
session token, gets a fresh join token for the same instance and connects again (the instance
verifies it like any join). Keep listing connected players in heartbeats — that keeps their
reconnect tokens alive (10 min after they vanish).

### For clients (testbed / UI, workstreams 04 / 16)
Signup/login → session token; `GET /v1/me`, `/v1/me/economy`, `/v1/me/events?from_seq=`;
parties (`POST /v1/parties`, `/v1/parties/{id}/join {code}`); `POST /v1/matchmaking/queue
{prefs: {role, mode, bracket, region, city?}}`, poll `GET /v1/matchmaking/status` until
`assignment` → connect to `assignment.address` with `assignment.join_token`; leaderboards
`GET /v1/leaderboards/{clean_rating|bounty|captures|time_attack:<route>}?period=all|daily|weekly`.
No CORS headers yet (see gaps).

### Library
`wp_meta::{api, client::{MetaClient, RemoteAccountStore}, store::MetaStore}`. `MetaStore` is an
in-process `AccountStore` over the meta DB.

## Needs

| From | What | Workaround now |
|---|---|---|
| F (07, net) | handshake carries the meta `join_token`; server calls verify + heartbeat + `RemoteAccountStore`, reports race/pursuit results | `tests/common/mod.rs::FakeInstance` shows the exact sequence; `meta-cli smoke` exercises it against a deployment |
| F / lead | per-instance `PlayerId` vs durable id (MC-1) | meta's `account_id` is used as `PlayerId` for economy accounts |
| E (06, police) | capture/escape events with the capturing cop's identity | instances fill `PursuitReport` themselves |
| 15 (race events) | which `race_type` ids exist; route ids | free strings; `time_attack` special-cased |

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| MC-1 | `AccountId(u32)` in `wp_contracts::ids` — the durable identity allocated by meta (signup), distinct from the per-instance `PlayerId` slot. `wp_economy::PlayerAccount::player` and `EconomyEvent::player` become `AccountId`; the server keeps a `PlayerId → AccountId` map from join verification. Same as F's NET-5 (accepted for v0.2.0) — this names meta as the allocator. | accounts, cop rank, leaderboards must survive instances; today `PlayerId(account_id)` is overloaded | lead, G (08), F (07) | accepted as NET-5, not in v0.2.0 applied — waiting; meta keeps `PlayerId(account_id)` |
| MC-2 | Typed `RaceEvent` (race finished: route, type, mode, bracket, per-racer position / time / clean) next to the planned typed `PoliceEvent` / `EconomyEvent` | the prompt asks for an append-only `RaceEvent` archive; today meta defines `wp_meta::api::RaceReport` itself. With a contract type, `apps/server` emits it once and meta archives it as-is | lead, 15, F, meta | proposed — not in v0.2.0; please add to the v0.3.0 batch with typed `EconomyEvent` |
| MC-3 | `WantedLevel` in contracts (already accepted, G #5) — meta would then use it in `PursuitReport` instead of `u8` | one wanted scale everywhere | E, F, G, meta | **done** — `PursuitReport.max_wanted` is `wp_contracts::WantedLevel` since v0.2.0 (JSON `"None"`, `"L1"`..`"L5"`) |
| MC-4 | `ClassBracket` (`D, C, B, A, S`) or a doc note that bracket names are `data/economy/market.toml` `class_index.brackets[].name` | matchmaking, instances and race reports exchange brackets as strings | G, F, meta | proposed (low priority) |
| MC-5 | `DistrictId` (G #6, deferred to 05) — territory uses `district_id: String` until then | crews/territory | D, G, meta | waiting |

## Changes outside my crates
| File | Change | Why |
|---|---|---|
| `Cargo.toml` | added `apps/meta`, `tools/meta-cli` to `members` | new crates |
| `Cargo.lock` | new dependencies (axum 0.8, argon2 0.5, sha2, hex, getrandom 0.3, ureq 2, clap 4; rusqlite 0.40 shared with `wp_economy`) | — |
| `justfile` | `just meta` recipe (dev secrets, fast hashing) | run the service locally |
| `.gitignore` | `meta-dev.sqlite*` | dev database from `just meta` |

`wp_economy` and `wp_contracts` untouched. Nothing here is reachable from `wp_wasm`
(`cargo tree -p wp_wasm` unchanged).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| queue + match, 2000 tickets / 100 instances | cloud container, release | 0.98 ms/call | n/a | none set (≈1000 calls/s per process) |
| race report ≤ 8 racers (archive + rating + boards) | cloud container, release | 0.15 ms | n/a | — |
| 20-bot smoke over HTTP, production Argon2id | cloud container, release | 0.55 s total | n/a | — |

## Known gaps
- Single process / single SQLite connection; Postgres port is a mechanical SQL change (README).
- No CORS (the browser testbed calling meta directly needs `tower-http` CORS or a same-origin
  proxy); no TLS (terminate at a proxy).
- OAuth linking is a stub (501); no password reset or email verification.
- Crews: open membership, no invites. Territory: last report wins.
- Matching is greedy oldest-first inside region/mode/bracket; the clean rating is stored but not
  yet used for skill matching.

## Lead feedback
(Written by the integration lead.)

- Run 3 (STATUS.md): "one flaky test under load (`twenty_bots_…` heartbeat)".

### Response (machine O, after v0.2.0 merge)
- **Root cause was a real overbooking bug, not test flakiness.** Between join verification
  (ticket `assigned` → `joined`, so no longer reserved) and the instance's next heartbeat
  (population still old), a seat was counted by neither and the matcher could hand it out again.
  Under load the bots test hit it (7 racers on a 6-slot instance → heartbeat 400). Reproduced 1/40
  with 8 busy-loop CPU hogs on 4 cores.
- Fix: migration `0005_joined_seat_hold.sql` adds `tickets.joined_ms` / `reported`; a joined
  ticket keeps holding its seat until a heartbeat lists the player (or `UNREPORTED_JOIN_HOLD_MS`
  = 2 min passes). Regression tests `joined_players_hold_their_seat_until_reported`,
  `unreported_join_holds_expire`. After the fix: 0/60 under the same load.
- Note: `main` merged `f991041`; the later `ws/meta` commits (password hashing off the DB lock,
  9× faster matching, only-current-host account access, perf test, docs, `just meta`) come with
  this merge.
- v0.2.0: switched `PursuitReport.max_wanted` to `wp_contracts::WantedLevel` (MC-3). Nothing else
  in v0.2.0 touches `apps/meta`.
