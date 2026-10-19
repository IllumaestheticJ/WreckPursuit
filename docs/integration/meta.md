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

### Server hookup (for F — `apps/server` / `wp_net`; meta does not edit these)

Decided with the lead (run 4): meta ships an opt-in hook, F wires it. The hook is
`wp_meta::instance` and is tested against the real service in `apps/meta/tests/hook.rs`, which plays
the server's side step by step and is the reference for the wiring.

| Item | Use |
|---|---|
| `MetaLinkConfig::from_env(mode, slots, default_address) -> Option<Result<..>>` | `None` when `WP_META_URL` is unset → **run standalone exactly as today**. Also reads `WP_META_INSTANCE_KEY` (required with the URL), `WP_META_INSTANCE_NAME`, `WP_META_REGION`, `WP_META_CITY`, `WP_META_BRACKET`, `WP_META_PUBLIC_ADDRESS` |
| `MetaWorker::start(&cfg)` | registers (blocking, once at startup; fail loudly) and starts a background thread |
| `worker.set_presence(HeartbeatRequest { population, players, draining, tick_ms })` | never blocks; the worker heartbeats on its own every 5 s with the latest presence |
| `worker.send(MetaRequest::…)` | never blocks: `VerifyJoin { tag, join_token }`, `LoadAccount`, `Commit`, `Race`, `Pursuit`, `Territory` |
| `worker.poll() -> Vec<MetaReply>` | drain once per tick: `Joined`, `AccountLoaded`, `Committed`, `Heartbeat` (kicks, flags), `RaceReported`, … |
| `worker.shutdown()` | finishes queued work, deregisters |
| `instance::ticks_to_ms(ticks)` | race times for `RaceEntry::finish_time_ms` |
| `MetaLink` | the same calls, blocking (tools, tests) |

Steps, keyed to F's current code:

1. **Startup** (`apps/server` `run` / `AppConfig`): `if let Some(cfg) = MetaLinkConfig::from_env(mode,
   slots, &public_ws_url)` → `MetaWorker::start(&cfg?)`. `slots` from `ServerConfig::max_players`
   and `RoleConfig` (`racer`, `police_chase`, `police_captain` ≤ 1, `spectator`). Add `wp_meta` as a
   dependency of `apps/server` only (native; never reachable from `wp_wasm`).
2. **Handshake** (`Server::receive`, `ClientMsg::Hello` arm): with the hook on, a new player
   (`reconnect_token == 0`) must present a meta join token. Needs **MC-6** (below): `Hello` gains
   `join_token: String`. Instead of calling `join` directly, park the connection and
   `send(VerifyJoin { tag: conn, join_token })`; on `MetaReply::Joined { tag: conn, result }`:
   `Ok(v)` → `join(conn, v.display_name, assists)`, remember `PlayerId → v.account_id`, start the
   player in `v.role` (`RoleManager`), keep `v.party_id` members' spawns together, and send
   `v.reconnect_token` to the client (MC-6: `Welcome` gains `meta_reconnect_token: String`);
   `Err(e)` → `ServerMsg::Reject { reason: e.to_string() }`. Hook off → today's path unchanged.
   F's own u64 in-window reattach (`reconnect_token != 0`) stays as is; the meta reconnect token
   is for after that window (client calls `POST /v1/matchmaking/reconnect`, gets a new join token).
3. **Presence** (on join, `disconnect`, role change; or once per second): `set_presence` with
   per-role population from `RoleManager` and `players` = meta account ids of **connected**
   players. Listing players matters: a verified join holds its matchmaking seat until a
   heartbeat lists it (that is the overbooking fix).
4. **Heartbeat replies**: for each `MetaReply::Heartbeat(Ok(h))`: `h.kicks` → find the player by
   account id, send `Reject { reason }`, drop the connection (no reconnect window); store `h.flags`
   (e.g. `illegal_day_race_frequency` for 15's race scheduler). `Err` → log; the worker retries next
   beat (3 missed = meta marks the instance down and re-queues unclaimed seats).
5. **Accounts** (`Server::account`, `credit`, `accounts: InMemoryStore`): keep the in-memory store as
   the tick-time cache, keyed `PlayerId(account_id)` until MC-1 lands. On `Joined` send
   `LoadAccount { account }`; on `AccountLoaded` put it in the cache. After each economy operation
   (`settle_race`, capture consequences, … — NET-N7) commit locally **and**
   `send(Commit { account, events })`. `Committed` with code `seq_conflict` → reload and redo.
   Today's `credit()` commits with no events; with meta on, switch to the `wp_economy` calls so
   the events exist (meta's archive is the audit log).
6. **Race results** (`RaceEvent::Finished`): after the economy commits, `send(Race(RaceReport {
   report_key: format!("{instance_id}-{race}"), route_id, race_type, mode, bracket, entries }))`
   with one `RaceEntry { account_id, position: place, finish_time_ms: Some(ticks_to_ms(time_ticks))
   (None for DNF, position 0), clean }` per racer. `clean` = `wp_economy::RaceResult::is_clean()`
   (no contacts, no non-speeding violations): F's race layer needs to track contacts/violations per
   racer (not a contract change). Resending after an error is safe (`report_key` dedupes).
7. **Pursuits** (when E's capture/escape events reach the server): `send(Pursuit(PursuitReport {
   report_key, racer_id, max_wanted: WantedLevel, bounty, outcome, captured_by, assists }))` with
   meta account ids.
8. **Territory** (when D/15 decide district control): `send(Territory(TerritoryReport { .. }))`.
9. **Shutdown**: `set_presence(.. draining: true ..)` to stop new matches, then `worker.shutdown()`.

### For clients (testbed / UI, workstreams 04 / 16)
Signup/login → session token; `GET /v1/me`, `/v1/me/economy`, `/v1/me/events?from_seq=`;
parties (`POST /v1/parties`, `/v1/parties/{id}/join {code}`); `POST /v1/matchmaking/queue
{prefs: {role, mode, bracket, region, city?}}`, poll `GET /v1/matchmaking/status` until
`assignment` → connect to `assignment.address` with `assignment.join_token`; leaderboards
`GET /v1/leaderboards/{clean_rating|bounty|captures|time_attack:<route>}?period=all|daily|weekly`.
**Browser:** run meta with `WP_META_CORS_ORIGINS=http://localhost:5173` (`just meta` does) or
proxy `/v1` through Vite; example `fetch` client and both setups in `apps/meta/docs/browser.md`.
CORS covers the player API only (never `/v1/admin/*` or the instance API).

### Library
`wp_meta::{api, client::{MetaClient, RemoteAccountStore}, store::MetaStore}`. `MetaStore` is an
in-process `AccountStore` over the meta DB.

## Needs

| From | What | Workaround now |
|---|---|---|
| F (07, net) | wire the hook: "Server hookup" steps 1–9 above; MC-6 protocol fields; per-racer `clean` tracking in races | `apps/meta/tests/hook.rs` plays the server side against the real service; `meta-cli smoke` checks a deployment |
| F / lead | per-instance `PlayerId` vs durable id (MC-1) | meta's `account_id` is used as `PlayerId` for economy accounts |
| E (06, police) | capture/escape events with the capturing cop's identity | instances fill `PursuitReport` themselves |
| 15 (race events) | which `race_type` ids exist; route ids | free strings; `time_attack` special-cased |

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| MC-1 | `AccountId(u32)` in `wp_contracts::ids` — the durable identity allocated by meta (signup), distinct from the per-instance `PlayerId` slot. `wp_economy::PlayerAccount::player` and `EconomyEvent::player` become `AccountId`; the server keeps a `PlayerId → AccountId` map from join verification. Same as F's NET-5 (accepted for v0.2.0) — this names meta as the allocator. | accounts, cop rank, leaderboards must survive instances; today `PlayerId(account_id)` is overloaded | lead, G (08), F (07) | accepted as NET-5, not in v0.2.0 applied — waiting; meta keeps `PlayerId(account_id)` |
| MC-2 | Typed `RaceEvent` (race finished: route, type, mode, bracket, per-racer position / time / clean) next to the planned typed `PoliceEvent` / `EconomyEvent` | the prompt asks for an append-only `RaceEvent` archive; today meta defines `wp_meta::api::RaceReport` itself. With a contract type, `apps/server` emits it once and meta archives it as-is | lead, 15, F, meta | proposed — not in v0.2.0; please add to the v0.3.0 batch with typed `EconomyEvent` |
| MC-3 | `WantedLevel` in contracts (already accepted, G #5) — meta would then use it in `PursuitReport` instead of `u8` | one wanted scale everywhere | E, F, G, meta | **done** — `PursuitReport.max_wanted` is `wp_contracts::WantedLevel` since v0.2.0 (JSON `"None"`, `"L1"`..`"L5"`) |
| MC-6 | Net protocol (v0.3.0 batch, F's list): `ClientMsg::Hello` gains `join_token: String` (empty = none; required for new players when the server runs with meta) and `ServerMsg::Welcome` gains `meta_reconnect_token: String` (empty when meta is off) | step 2 of the server hookup: the server must check the matchmaking join token before admitting a player, and hand the client the token for rejoining after the in-window reattach expires | F (protocol + client), C/04 (testbed client), meta | proposed — for F's v0.3.0 protocol list; wp_net untouched by meta |
| MC-4 | `ClassBracket` (`D, C, B, A, S`) or a doc note that bracket names are `data/economy/market.toml` `class_index.brackets[].name` | matchmaking, instances and race reports exchange brackets as strings | G, F, meta | proposed (low priority) |
| MC-5 | `DistrictId` (G #6, deferred to 05) — territory uses `district_id: String` until then | crews/territory | D, G, meta | waiting |

## Changes outside my crates
| File | Change | Why |
|---|---|---|
| `Cargo.toml` | added `apps/meta`, `tools/meta-cli` to `members` | new crates |
| `Cargo.lock` | new dependencies (axum 0.8, argon2 0.5, sha2, hex, getrandom 0.3, ureq 2, clap 4; rusqlite 0.40 shared with `wp_economy`) | — |
| `justfile` | `just meta` recipe (dev secrets, fast hashing, CORS for the testbed's 5173/4173) | run the service locally |
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
- No TLS (terminate at a proxy). CORS is an exact-origin allowlist (no wildcard subdomains).
- OAuth linking is a stub (501); no password reset or email verification.
- Crews: open membership, no invites. Territory: last report wins.
- Matching is greedy oldest-first inside region/mode/bracket. Skill matching covers racers only
  (clean rating); cops are unrated.

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

### Response (machine O, lead run 4 follow-up)
- Server hookup: per the lead, meta-side only. Added `wp_meta::instance` (`MetaLinkConfig`,
  `MetaLink`, non-blocking `MetaWorker`) + `tests/hook.rs`, and the "Server hookup" steps for F
  above. `apps/server` and `wp_net` are untouched; the protocol fields are requested as MC-6.
- The bots test: the run-3 failure predates the seat-hold fix (lead confirmed). One confirming
  full-workspace run on the run-4 `main` merge (`cargo test --workspace`, all crates in
  parallel): all 94 test binaries pass, `twenty_bots_signup_matchmake_results_leaderboard` and
  `server_hook_end_to_end` included; fmt, clippy `-D warnings`, wasm build clean.


## Lead feedback (round 6b, 2026-10-19) — head-start tasks

Contract **v0.3.0** is being applied in parallel by the lead (branch `lead/contract-v0.3.0`). Do not edit `wp_contracts`; build your work so it drops onto v0.3.0 (inherent methods / local types now, trait wiring after). Merge `origin/claude/laughing-newton-0xqr6q` (the lead branch, ahead of `main` until today's gate finishes) before starting. Goal of the project right now: the **single-player greybox demo**: drive in the city, raise heat, get chased by physical police, escape or get caught, and pay the repair bill.

1. Fix the flaky `twenty_bots_…` heartbeat test under load (root cause, not retries).
2. CORS (same-origin proxy or `tower-http` CORS) so the browser testbed can call meta. Add a small `docs` example.
3. Use the stored clean rating in matchmaking (skill brackets).

### Response (machine O, round 6b)
1. **`twenty_bots_…` flake — already root-caused and fixed in run 3/4** (overbooking between join
   verification and the next heartbeat; migration `0005_joined_seat_hold.sql`, see the first
   response above). STATUS.md row O still quotes the run-3 note. Re-checked on today's merge
   (`main` + lead branch): **0 failures in 40 runs** of `twenty_bots_signup_matchmake_results_leaderboard`
   with 8 busy-loop CPU hogs on 4 cores. The test has no wall-clock dependence left (all expiry
   on `ManualClock`; poll loops are bounded by iteration count, not time). **Please update
   STATUS.md row O.**
2. **CORS:** `wp_meta::cors`, a small allowlist middleware (no new dependency). Config
   `WP_META_CORS_ORIGINS` (comma-separated `http(s)://host[:port]`, or `*` for dev); off when
   unset. Player paths only: `/v1/admin/*` and `/v1/instances/{id}/…` never get CORS; bearer
   auth, no credentials. Preflight → 204 with methods/headers, 10 min max-age. `just meta` allows
   `http://localhost:5173` and `:4173`. Docs + TypeScript `fetch` example (and the Vite-proxy
   alternative for the testbed owner): `apps/meta/docs/browser.md`. Test:
   `cors_for_allowed_origins_on_player_paths_only`.
3. **Skill matching:** racers are matched on the stored clean rating. A ticket stores the
   rating it queued with (party = members' mean; migration `0006_skill_matching.sql`). An
   instance's racer rating = racers listed in its last heartbeat (current ratings) + racer seats
   reserved since. An instance with racers fits only within the ticket's window: 200 points,
   +10 per second waited (`Config::skill_window` / `skill_widen_per_s`). Order among fits:
   fullest, then closest rating, then lowest id (fill-first is kept). Cops/spectators unrated.
   Admin flag `skill_matching_enabled` (default true). `InstanceInfo.racer_rating` is new
   (optional; health JSON and `/admin` dashboard). Test:
   `racers_match_by_clean_rating_with_a_widening_window` (separation, widening, flag off,
   police unaffected).

Nothing outside `apps/meta` changed except the `just meta` line. The single-player greybox
demo does not need meta (the server runs standalone when `WP_META_URL` is unset).
