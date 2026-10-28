# data/economy — economy tables

Owned by workstream 08. Loaded and validated by `wp_economy::Economy` (every class / wanted level /
day phase / mode must be present). Formulas and the intended feel of each lever are documented
in `crates/wp_economy/README.md` ("Tuning levers").

| File | What |
|---|---|
| `prices.toml` | repair parts by region / component, class scaling, labour, districts, shops, towing, the tow service (`[tow]`: call-out, per km, road factor), scrap, insurance |
| `stakes.toml` | purses, buy-ins, rep, day-phase / class / wanted / mode multipliers, debt, pink slips, bounties, informants |
| `consequences.toml` | fines, bail, rep loss, impound, Arcade vs Simulation capture rules, cop XP and ranks |
| `market.toml` | starting cash, dealer price bands, resale, economy-floor car, class index, tire grip |
| `lots.toml` | dealerships (tiers), used car lots, junkyards: what each carries, daily fill, condition / tread / price bands, junkyard odds (dead, missing parts, flat tyres, rare finds), rare variants, tyre prices and wear |

After changing numbers run `cargo test -p wp_economy --test career -- --nocapture` and update the
"Tuning results" table in the crate README.
