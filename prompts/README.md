# Prompt files

Paste one file per Claude Code session. Order and machine assignments are in `docs/DESIGN_OVERVIEW.md`.

1. `01_architecture_contracts.md` — run first, alone, on one machine; push before anything else starts.
2. `02`–`10` — run in parallel, one per machine, each on its own `ws/<name>` branch.
3. `11` — after Checkpoint 2.
4. `12_integration_lead.md` — run daily on the lead machine.
5. `13`–`20` — optional extra workstreams, all independent of each other and of 02–10; start any time after 01.
6. `21_car_models_panels.md` (machine R, `ws/panels`) — destructible body panels.
7. `22_onboarding_flow.md` (machine S, `ws/onboarding`) — first-time player flow (backlog B08); start after L's race-in-city wiring if possible, it composes race, police, economy and UI.

Every session starts by reading `CLAUDE.md` (automatic), then `docs/INTEGRATION.md` and `docs/CONTRACT_CHANGELOG.md` (created by prompt 01).
