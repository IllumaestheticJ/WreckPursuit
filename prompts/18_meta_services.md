# PROMPT 18 — Meta services: accounts, persistence, matchmaking, leaderboards, admin (`apps/meta`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`. You own a new binary `apps/meta` (Rust, axum + SQLite now / Postgres-ready via sqlx) and `tools/meta-cli`. Branch `ws/meta`. `apps/server` (prompt 07) runs a city instance; you run everything around it. Read `wp_economy`'s `AccountStore` trait and implement it against your DB (do not edit the crate).

## Goal
Everything a persistent online game needs that isn't the simulation: identity, progression storage, instance directory, matchmaking into instances by region/mode/class bracket, leaderboards, moderation/admin.

## Work
- Auth: email/password + session tokens now, OAuth-ready; rate limits; account linking stub.
- Persistence: `PlayerAccount`, garage, mods, bounty/heat history, cop rank, crews/territory (schema migrations), event log (append-only `EconomyEvent`/`RaceEvent` archive).
- Instance directory + matchmaking: instance servers register (region, city, mode, population, role ratios); players request join by preference (racer/cop, Arcade/Sim, class bracket) → ticket → instance assignment; party join; reconnect tokens.
- Leaderboards: clean-race rating, bounty, captures, time-attack per route; daily/weekly resets.
- Admin/moderation: ban/kick, economy adjustments with audit, instance health dashboard (JSON + a tiny HTML page), feature flags (e.g. illegal-day-race frequency).
- Integration test: spin `apps/meta` + a fake instance, run 20 bots through signup → matchmake → results → leaderboard.

## Deliver
Service, migrations, OpenAPI spec, `tools/meta-cli`, README (deploy notes), `docs/integration/meta.md` (API `apps/server` calls: register, report results, fetch/save accounts).
