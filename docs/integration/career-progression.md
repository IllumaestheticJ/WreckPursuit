# Career progression integration notes

- **Workstream:** career progression (rep tiers, credits, unlocks)
- **Branch:** `claude/project-thread-elq8kz`
- **Crates / folders owned:** `crates/wp_economy/src/progression.rs`, `data/economy/career.toml`
  (plus small hooks in `wp_economy` data/ledger/error and `wp_wasm::career`)
- **Contract version built against:** current `wp_contracts` (no contract edits)

## Exposes — the API other threads should use

Credits are the account's cash and rep is `PlayerAccount::rep`; nothing was renamed or moved.
`wp_economy::Credits` is an alias of `Money` (`i64`). The new piece is a **tier ladder** on top
of rep, and gates keyed by tier instead of raw rep numbers. **Please gate on these calls rather
than hard-coding rep thresholds**, so tuning lives in one file (`data/economy/career.toml`).

```rust
use wp_economy::{Economy, PlayerAccount, RepTier, Feature, BetVenue};

econ.rep_tier(&acct) -> RepTier                    // Unknown < Street < Known < Respected < Feared < Legend
econ.is_unlocked(&acct, Feature::TougeBetting) -> bool
econ.require_unlocked(&acct, Feature::PinkSlips) -> Result<(), EconomyError>   // Err(Locked { what, needs, needed_rep, rep })
econ.class_unlocked(&acct, VehicleClass::Exotic) -> bool  // dealer gate; require_class(..) for the Result form
econ.max_bet(&acct, BetVenue::City) -> Money       // 0 = locked / broke / in debt
econ.place_bet(&mut acct, BetVenue::Touge, stake, "race-12") -> EconResult<BetTicket>   // takes the stake
econ.settle_bet(&mut acct, &ticket, gross_payout) -> EconResult<Money>                 // 0 payout = lost; returns net
econ.award_rep(&mut acct, amount, "boss:pass-3") -> EconResult<ProgressUpdate>          // extra rep (bosses, challenges)
econ.award_credits(&mut acct, amount, "boss:pass-3") -> EconResult<()>                  // extra credits
econ.update_progression(&mut acct) -> EconResult<ProgressUpdate>  // pays first-time tier rewards; call after rep changes
econ.career_progress(&acct) -> CareerProgress      // view: tier, next tier + rep needed, features, classes, bet caps
```

All calls follow the crate's rules: all-or-nothing, return the `EconomyEvent`s posted, no
wall-clock, no randomness.

### Tiers (`career.toml`, tunable)
| Tier | Rep | One-off reward | Max single bet |
|---|---|---|---|
| Unknown | 0 | 0 | — |
| Street | 500 | 2 000 | 1 000 |
| Known | 2 000 | 5 000 | 3 000 |
| Respected | 5 000 | 12 000 | 8 000 |
| Feared | 10 000 | 25 000 | 20 000 |
| Legend | 20 000 | 50 000 | 60 000 |

Pacing: the scripted mid-skill racer (3 races a night) makes ~250 rep a night: Street after
~2 nights, Known ~8, Respected ~20, Feared ~40, Legend ~80 (`tests/progression.rs::tier_pacing`).
Boss / touge wins (`award_rep`) shorten that.

A stake is also capped at 50 % of cash on hand, and betting is refused while in debt.

### Feature gates
| Feature | Opens at |
|---|---|
| `CityRaces` | Unknown |
| `CityBetting` | Street |
| `TougeRaces` | Known |
| `TougeBetting` | Respected |
| `BossRaces` | Respected |
| `PinkSlips` | Feared |

Dealer: Beater / Compact / Truck at Unknown, Sport / Muscle at Known, Exotic at Feared; police
classes never.

Tiers follow **current** rep: a capture or a string of DNFs can drop a tier and close its
features again (`ProgressUpdate::relocked`). Rewards are paid once per tier, tracked by
`PlayerAccount::career.peak_tier` (serde-default, old saves load at Unknown and are paid the
rewards they already qualify for at their next update).

### For the other threads
- **City betting (`wp_race::betting`)**: implement `BetWallet::rep` via `econ.rep_tier` /
  `econ.max_bet(acct, BetVenue::City)`; `debit`/`credit` map onto `place_bet` / `settle_bet`.
- **Touge**: gate the touge menu with `Feature::TougeRaces`, side bets with
  `Feature::TougeBetting` + `max_bet(acct, BetVenue::Touge)`, the pink-slip option with
  `Feature::PinkSlips`, bosses with `Feature::BossRaces`. Your own `touge_stake` /
  `settle_touge_wager` can keep posting `RaceEntryFee` / `RacePurse`; just call
  `require_unlocked` first and `update_progression` after any rep change.
- **Time/day-night**: nothing to call; progression does not depend on time.
- **Pink slips today** (`stakes.toml [pink_slip] modes = ["Simulation"]`, `min_rep = 500`)
  are owned by the economy settlement; the touge thread decides whether to widen the modes. The
  career gate (`Feature::PinkSlips`) sits on top.

### Ledger notes
Until the contract has typed variants, entries use `EventReason::Adjustment` with a note;
decode with `wp_economy::career_note(&reason) -> Option<(kind, detail)>`:

| Note | kind | Cash / rep |
|---|---|---|
| `career:tier_reward:<tier>` | `tier_reward` | +reward |
| `career:tier:<tier>` | `tier` | none (promotion to a tier already rewarded, or demotion) |
| `career:rep:<label>` | `rep` | ±rep |
| `career:credits:<label>` | `credits` | +credits |
| `bet:stake:<venue>:<label>` | `bet_stake` | −stake |
| `bet:payout:<venue>:<label>` | `bet_payout` | +payout |

### Browser (`wp_wasm::career`)
- `career_view_json()` gains `progress: CareerProgress`; dealer entries gain `locked` and
  `unlock_tier`.
- `career_buy_car` refuses classes above the player's tier.
- `career_end_run()` and `career_next_night()` run `update_progression`; `RunClose` gains
  `progress: ProgressUpdate` so the end-of-run panel can show a promotion.

## Needs
| From | What | Workaround now |
|---|---|---|
| Lead (contracts) | Typed reasons (request #1 below) | `Adjustment` notes |

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | `EconomyReason::{CareerTierReward { tier: String }, CareerTier { tier: String }, CareerRep { label: String }, CareerCredits { label: String }, BetStaked { venue: String, label: String }, BetPaid { venue: String, label: String }}` | the client feed should not parse notes | 07 net, testbed UI | proposed |
