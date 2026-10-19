# apps/meta — meta services (`wp_meta`, binary `wp-meta`)

Everything a persistent online game needs that is not the simulation. City servers
(`apps/server`) run the game; this service runs around them:

| Area | What | Module |
|---|---|---|
| Accounts | email + password (Argon2id), session tokens, bans, rate limits, OAuth-ready identities table, account-linking stub | `auth`, `ratelimit` |
| Persistence | `wp_economy::AccountStore` over SQLite: `PlayerAccount` (garage, mods, bounty, cop rank, heat history), append-only `EconomyEvent` archive | `store` |
| Archives | race results, pursuit outcomes (bounty / heat history), audit log — all append-only (DB triggers) | `results`, `admin` |
| Crews | crews, members, districts held (territory reported by instances) | `crews` |
| Directory | instance registration, heartbeats (population, drain, tick time), liveness, kicks + flags pushed in heartbeat replies | `directory` |
| Matchmaking | tickets by role / mode / class bracket / region (/ city), parties (all-or-nothing), one-time join tokens, reconnect tokens | `matchmaking` |
| Leaderboards | clean-race rating (Elo), bounty, captures, time attack per route; all-time / daily / weekly | `leaderboards` |
| Admin | ban / unban / kick, audited economy adjustments, feature flags, board resets, audit log, health JSON + `/admin` HTML dashboard | `admin`, `routes` |

API: `openapi.yaml` (also served at `GET /openapi.yaml`). Rust client: `wp_meta::client::MetaClient`
(blocking, `ureq`) and `RemoteAccountStore` (an `AccountStore` over HTTP for `apps/server`).
CLI: `tools/meta-cli`. Integration contract for `apps/server`: `docs/integration/meta.md`.

## Run

```sh
just meta                       # dev: 127.0.0.1:8090, meta-dev.sqlite, dev secrets, fast hashing
cargo run --release -p meta-cli -- smoke --instance-key dev-instance-key-000 --bots 20
open http://127.0.0.1:8090/admin   # paste the admin token (dev-admin-token-0000)
```

## Configuration (environment)

| Variable | Default | Notes |
|---|---|---|
| `WP_META_LISTEN` | `127.0.0.1:8090` | bind address |
| `WP_META_DB` | `meta.sqlite` | SQLite file; `:memory:` for throwaway runs. WAL mode. |
| `WP_META_INSTANCE_KEY` | — (required, ≥ 16 chars) | shared key city servers use to register |
| `WP_META_ADMIN_TOKEN` | unset = admin API off | ≥ 16 chars; send `X-Admin-Actor: <name>` to name yourself in the audit log |
| `WP_META_DATA` | embedded tables | `data/` root for `wp_economy` tables (starting cash, adjustments) |
| `WP_META_FAST_HASH=1` | off | minimal Argon2 cost — **development only** |
| `WP_META_NO_RATE_LIMIT=1` | off | disables rate limits — **load tests only** |

Fixed defaults (in `Config::local`): sessions 30 days, join tokens 60 s, reconnect tokens 10 min
(refreshed while the player appears in heartbeats), queue tickets 5 min, heartbeat every 5 s
(down after 3 missed).

## Deploy notes

- **One process per database.** The service holds one SQLite connection behind a mutex; run a
  single replica (it is light — see Perf). Back it up with `sqlite3 meta.sqlite ".backup ..."`
  or volume snapshots; WAL mode allows online backups.
- **TLS** is not built in (the client has no TLS either): put it behind a reverse proxy
  (Caddy / nginx / a cloud LB) that terminates TLS. Rate limits key on the socket IP; behind a
  proxy, also rate-limit at the proxy (or extend `routes::ClientIp` to trust
  `X-Forwarded-For` from the proxy only).
- **Secrets:** generate `WP_META_INSTANCE_KEY` and `WP_META_ADMIN_TOKEN` randomly
  (`openssl rand -hex 32`). Tokens are stored only as SHA-256 hashes; passwords as Argon2id PHC
  strings. Never set `WP_META_FAST_HASH` in production.
- **Migrations** run automatically at startup (`migrations/*.sql`, recorded in
  `schema_migrations`). Never edit a released migration; add the next number.
- **Smoke test** a deployment with `meta-cli smoke --instance-key ... --bots 8` (creates throwaway
  `smk*` accounts in region `smoke`, registers and deregisters a temporary instance).
- Graceful shutdown on Ctrl-C / SIGINT. Liveness probe: `GET /healthz`.

### Postgres

The prompt asked for sqlx (SQLite now, Postgres-ready). sqlx's SQLite driver links its own
`libsqlite3-sys`, which conflicts with the `rusqlite` version `wp_economy` (feature `native`)
already pins — Cargo allows only one crate linking `sqlite3`. So this crate uses `rusqlite`
directly and keeps the door open differently:

- all SQL lives in one module per domain behind plain functions taking a connection;
- the schema uses only `INTEGER` / `TEXT` / `REAL`, `ON CONFLICT ... DO UPDATE` upserts and
  standard triggers semantics that exist in Postgres;
- SQLite-specific bits to change when porting: `INTEGER PRIMARY KEY` → `BIGSERIAL PRIMARY KEY`,
  `COLLATE NOCASE` → `lower()` / `citext`, trigger bodies (`RAISE(ABORT, ...)` → a PL/pgSQL
  function), `last_insert_rowid()` → `RETURNING id`, `?N` placeholders → `$N`.

## Tests

```sh
cargo test -p wp_meta                       # unit + API + 20-bot end-to-end (~1 s)
cargo test --release -p wp_meta --test perf -- --ignored --nocapture   # throughput numbers
```

- `tests/bots.rs` — the prompt's integration test: real service on a socket, three fake
  instances, 20 bots through signup → login → party → matchmake → join → race (real
  `wp_economy::settle_race` committed through `RemoteAccountStore`) → pursuit → leaderboards,
  plus reconnect, kick, ban and an audited adjustment.
- `tests/api.rs` — auth errors, session expiry, rate limits, sequence conflicts, instance auth,
  bracket/mode/capacity gating, join-token rules, ticket/seat expiry, dead and draining
  instances, parties, feature flags, admin auth, temporary bans, leaderboard periods, crews,
  restart persistence, and "every route is in `openapi.yaml`".
- `store::tests::account_store_contract` — the same `AccountStore` contract the economy's own
  stores satisfy.

All time-dependent behaviour runs on `clock::ManualClock`, so the tests never sleep for expiry.

## Perf

Release build, cloud container, 2026-10-19 (`tests/perf.rs`, domain functions without HTTP):

| Measurement | Result |
|---|---|
| full `meta-cli smoke` flow, 20 bots, production Argon2id, over HTTP | 0.55 s total (≈ 27 ms per bot; the password hash dominates and runs off the DB lock) |
| signup, fast hash ×2000 | 0.12 ms each |
| queue + match ×2000 against 100 instances | 0.98 ms each (grows slowly with ticket history) |
| status + verify join ×2000 | 0.08 ms each |
| race report (≤ 8 racers, rating + boards) | 0.15 ms each |
| leaderboard top 100 | 0.3 ms |

There is no frame budget for meta services; the target is "one small VM serves a launch-size
player base". At these numbers one process handles ~1000 matchmaking calls/s.

## Known gaps

- Single-process (SQLite + in-memory rate limiter). Scaling out needs Postgres and a shared
  limiter (or rate limiting at the proxy).
- Account linking is a stub (`POST /v1/auth/link` records the request and answers 501). No
  password reset / email verification (needs a mail sender).
- Crews have open membership (no invites / kicking members); territory is whatever instances
  report (no conflict resolution between instances of the same city).
- `ensure_hosted` trusts registered instances: an instance can commit any economy change for the
  players it hosts. Instances are first-party servers holding the instance key.
- Matching is greedy oldest-first; no skill-based matching inside a bracket yet (the clean rating
  is there to feed it), no backfill priority for parties beyond all-or-nothing.
- Leaderboard ties break by earliest update, then account id. Daily/weekly periods are UTC.
- `PlayerId` doubles as the durable account id until the contract splits them (MC-1 in
  `docs/integration/meta.md`).
