//! Betting on picker races (`data/events/betting.toml`): the player backs themselves at odds.
//! In the browser the stakes go through the one career wallet (`wp_economy` `place_bet`, venue
//! `City`), the same wallet that takes the roaming street racers' head-to-head stakes (venue
//! `Street`, even money, `wp_wasm::street`).
//!
//! A [`Market`] prices one race from its entrants' form ratings ([`crate::roster`]):
//!
//! - **Win**: Plackett–Luce, `P(i wins) = sᵢ / Σs` with strength `s = 10^(rating/400)`.
//! - **Podium** (top three): exact Plackett–Luce over every ordered top three (fields of
//!   `podium_min_field` or more).
//! - **Head-to-head**: `P(i ahead of j) = sᵢ / (sᵢ + sⱼ)` (the Elo expectation).
//!
//! Offered decimal odds are `1 / (p · (1 + margin))`, rounded *down* to `odds_step` and
//! clamped to `[min_odds, max_odds]`, and stored as integer thousandths ([`Odds`]) so a
//! ticket's payout is exact integer arithmetic on every target.
//!
//! A [`Book`] takes bets on one market until the green light ([`Book::close`]), then settles
//! them from the judged result ([`Book::settle`]) or refunds everything if the race never ran
//! ([`Book::void_all`]). Money moves only through a [`BetWallet`]: the career economy decides
//! whether the player may bet at all and how much (rep tier, cash, debt), the book only
//! decides what it offers.
//!
//! **Integrity rule:** a bettor who is *in* the race may only back themselves (win, podium,
//! or a head-to-head with themselves as the pick). Betting against yourself, or on a rival
//! you could hand the win to, is not offered.
//!
//! Settlement: a win ticket pays if the racer finished first; a podium ticket if they finished
//! in the top three; a head-to-head if the pick finished ahead of the opponent (a finisher is
//! ahead of any DNF/DQ). A head-to-head where neither finishes is void (stake back), as is any
//! ticket on a racer who never started (withdrawn, missing from the result).

use serde::{Deserialize, Serialize};
use thiserror::Error;
use wp_contracts::EntityId;
use wp_economy::Money;

use crate::error::RaceDataError;
use crate::roster::{pow10, Placing, RatingConfig, PLAYER_KEY};

const FILE: &str = "betting.toml";

/// `betting.toml [book]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookRules {
    /// House margin on the win market.
    pub win_margin: f32,
    /// House margin on the podium market.
    pub podium_margin: f32,
    /// House margin on head-to-heads.
    pub h2h_margin: f32,
    /// Shortest decimal odds offered.
    pub min_odds: f32,
    /// Longest decimal odds offered.
    pub max_odds: f32,
    /// Odds are rounded down to a multiple of this.
    pub odds_step: f32,
    /// Smallest stake taken, credits.
    pub min_stake: Money,
    /// Tickets one bettor may hold on one race.
    pub max_tickets: u32,
    /// Smallest field with a podium market.
    pub podium_min_field: usize,
}

/// The whole `betting.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BettingRules {
    /// Book settings.
    pub book: BookRules,
    /// Rating settings (also used by [`crate::roster::RosterState::record_race`]).
    pub rating: RatingConfig,
}

impl BettingRules {
    /// The repository's `data/events/betting.toml`, embedded at compile time.
    pub const EMBEDDED_TOML: &'static str = include_str!("../../../data/events/betting.toml");

    /// Parse and validate.
    pub fn from_toml(text: &str) -> Result<BettingRules, RaceDataError> {
        let r: BettingRules = toml::from_str(text).map_err(|e| RaceDataError::parse(FILE, e))?;
        let bad = |what: &str, why: &str| Err(RaceDataError::invalid(FILE, what, why));
        let b = &r.book;
        for (what, m) in [
            ("win_margin", b.win_margin),
            ("podium_margin", b.podium_margin),
            ("h2h_margin", b.h2h_margin),
        ] {
            if !(0.0..=0.5).contains(&m) {
                return bad(what, "margin must be 0..=0.5");
            }
        }
        if !(b.min_odds > 1.0 && b.max_odds > b.min_odds && b.odds_step > 0.0) {
            return bad(
                "min_odds/max_odds/odds_step",
                "need 1 < min < max and step > 0",
            );
        }
        if b.min_stake <= 0 || b.max_tickets == 0 || b.podium_min_field < 4 {
            return bad(
                "min_stake/max_tickets/podium_min_field",
                "min_stake > 0, max_tickets > 0, podium_min_field >= 4",
            );
        }
        if r.rating.k <= 0 || r.rating.car_cap < 0 || r.rating.car_weight < 0.0 {
            return bad("rating", "k > 0, car_cap >= 0, car_weight >= 0");
        }
        Ok(r)
    }

    /// The embedded rules (panics only if the shipped data is invalid, which the tests rule
    /// out).
    pub fn embedded() -> BettingRules {
        BettingRules::from_toml(Self::EMBEDDED_TOML).expect("data/events/betting.toml is valid")
    }
}

/// Decimal odds in integer thousandths (`2350` = 2.35: a winning 100 pays 235 back).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Odds(pub u32);

impl Odds {
    /// Decimal odds (display only).
    pub fn decimal(&self) -> f32 {
        self.0 as f32 / 1000.0
    }

    /// Gross return of a winning `stake` (stake included), rounded down.
    pub fn payout(&self, stake: Money) -> Money {
        stake * self.0 as Money / 1000
    }

    /// Offered odds for probability `p` under `margin`, rounded down to the step and clamped.
    pub fn offer(p: f32, margin: f32, rules: &BookRules) -> Odds {
        let fair = if p > 0.0 {
            1.0 / (p * (1.0 + margin))
        } else {
            rules.max_odds
        };
        let step = (rules.odds_step * 1000.0) as u32;
        let raw = (fair.clamp(rules.min_odds, rules.max_odds) * 1000.0) as u32;
        let lo = (rules.min_odds * 1000.0) as u32;
        Odds((raw / step * step).max(lo))
    }
}

/// What a ticket backs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Selection {
    /// This racer wins.
    Win(String),
    /// This racer finishes in the top three.
    Podium(String),
    /// `backed` finishes ahead of `opponent`.
    HeadToHead {
        /// The pick.
        backed: String,
        /// Who they must beat.
        opponent: String,
    },
}

impl Selection {
    /// The racer the ticket backs.
    pub fn backed(&self) -> &str {
        match self {
            Selection::Win(k) | Selection::Podium(k) => k,
            Selection::HeadToHead { backed, .. } => backed,
        }
    }

    /// Short label for ledgers and menus, e.g. `win:vic`, `h2h:player>mara`.
    pub fn label(&self) -> String {
        match self {
            Selection::Win(k) => format!("win:{k}"),
            Selection::Podium(k) => format!("podium:{k}"),
            Selection::HeadToHead { backed, opponent } => format!("h2h:{backed}>{opponent}"),
        }
    }
}

/// One racer in a market.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketEntrant {
    /// Roster id or [`PLAYER_KEY`].
    pub key: String,
    /// Display name.
    pub name: String,
    /// Vehicle entity in the race, when known (settling from judged results).
    pub entity: Option<EntityId>,
    /// Rating the odds are priced from (form + course + car adjustments).
    pub rating: i32,
}

/// One race's prices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Market {
    /// Race / event id (display, ledgers).
    pub race: String,
    /// The field.
    pub entrants: Vec<MarketEntrant>,
    /// Win probability per entrant.
    pub win_p: Vec<f32>,
    /// Podium probability per entrant (`None` = no podium market).
    pub podium_p: Option<Vec<f32>>,
    /// Pricing rules.
    pub rules: BookRules,
}

impl Market {
    /// Price `entrants` under `rules`.
    pub fn new(race: impl Into<String>, entrants: Vec<MarketEntrant>, rules: &BookRules) -> Market {
        let s: Vec<f32> = entrants
            .iter()
            .map(|e| pow10(e.rating as f32 / 400.0))
            .collect();
        let total: f32 = s.iter().sum();
        let win_p = s.iter().map(|x| x / total).collect();
        let podium_p = (entrants.len() >= rules.podium_min_field).then(|| podium_probabilities(&s));
        Market {
            race: race.into(),
            entrants,
            win_p,
            podium_p,
            rules: rules.clone(),
        }
    }

    /// Index of `key` in the field.
    pub fn index_of(&self, key: &str) -> Option<usize> {
        self.entrants.iter().position(|e| e.key == key)
    }

    /// Probability that `sel` wins (`None` if not offered: unknown racer, podium market
    /// closed, head-to-head against yourself).
    pub fn probability(&self, sel: &Selection) -> Option<f32> {
        match sel {
            Selection::Win(k) => Some(self.win_p[self.index_of(k)?]),
            Selection::Podium(k) => Some(self.podium_p.as_ref()?[self.index_of(k)?]),
            Selection::HeadToHead { backed, opponent } => {
                let (i, j) = (self.index_of(backed)?, self.index_of(opponent)?);
                if i == j {
                    return None;
                }
                Some(self.win_p[i] / (self.win_p[i] + self.win_p[j]))
            }
        }
    }

    /// Odds offered on `sel` (`None` if not offered).
    pub fn odds(&self, sel: &Selection) -> Option<Odds> {
        let p = self.probability(sel)?;
        let margin = match sel {
            Selection::Win(_) => self.rules.win_margin,
            Selection::Podium(_) => self.rules.podium_margin,
            Selection::HeadToHead { .. } => self.rules.h2h_margin,
        };
        Some(Odds::offer(p, margin, &self.rules))
    }

    /// The board: win and podium odds per entrant, in field order.
    pub fn board(&self) -> Vec<BoardLine> {
        self.entrants
            .iter()
            .map(|e| BoardLine {
                key: e.key.clone(),
                name: e.name.clone(),
                rating: e.rating,
                win: self.odds(&Selection::Win(e.key.clone())).expect("in field"),
                podium: self.odds(&Selection::Podium(e.key.clone())),
            })
            .collect()
    }
}

/// One row of the odds board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoardLine {
    /// Racer key.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Priced rating.
    pub rating: i32,
    /// Win odds.
    pub win: Odds,
    /// Podium odds (`None` without a podium market).
    pub podium: Option<Odds>,
}

/// P(each entrant finishes top three) under Plackett–Luce with strengths `s` (exact
/// enumeration of ordered top threes: n·(n−1)·(n−2) terms, 1 320 for a 12-car field).
fn podium_probabilities(s: &[f32]) -> Vec<f32> {
    let n = s.len();
    let total: f32 = s.iter().sum();
    let mut p = vec![0.0f32; n];
    for a in 0..n {
        let pa = s[a] / total;
        let rest_a = total - s[a];
        for b in 0..n {
            if b == a {
                continue;
            }
            let pb = pa * s[b] / rest_a;
            let rest_b = rest_a - s[b];
            for c in 0..n {
                if c == a || c == b {
                    continue;
                }
                let pc = pb * s[c] / rest_b;
                p[a] += pc;
                p[b] += pc;
                p[c] += pc;
            }
        }
    }
    p
}

/// Where a bet's money comes from and goes to. The career economy implements this on the
/// player's account (see `docs/integration/race.md`, "Betting"); [`CashWallet`] is a plain
/// in-memory stand-in for tests and tools.
pub trait BetWallet {
    /// What the wallet hands back for a taken stake and needs again to pay it out (the
    /// economy's bet ticket).
    type Receipt: Clone;

    /// Largest stake the bettor may place right now (0 = betting locked, broke or in debt).
    fn max_stake(&self) -> Money;

    /// Take `stake` for the ticket described by `label`.
    fn take_stake(&mut self, stake: Money, label: &str) -> Result<Self::Receipt, String>;

    /// Pay `gross` back for the ticket (stake included; 0 = lost, the stake for a void).
    fn pay_out(&mut self, receipt: &Self::Receipt, gross: Money);
}

/// An in-memory wallet: cash plus a fixed stake cap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashWallet {
    /// Cash on hand.
    pub cash: Money,
    /// Stake cap (0 = locked).
    pub cap: Money,
}

impl BetWallet for CashWallet {
    type Receipt = Money;

    fn max_stake(&self) -> Money {
        self.cap.min(self.cash.max(0))
    }

    fn take_stake(&mut self, stake: Money, _label: &str) -> Result<Money, String> {
        if stake > self.cash {
            return Err(format!("needs {stake}, has {}", self.cash));
        }
        self.cash -= stake;
        Ok(stake)
    }

    fn pay_out(&mut self, _receipt: &Money, gross: Money) {
        self.cash += gross;
    }
}

/// Why a bet was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BetError {
    /// Betting closed at the green light.
    #[error("betting is closed")]
    Closed,
    /// The book does not offer this selection (unknown racer, no podium market, …).
    #[error("not offered: {0}")]
    NotOffered(String),
    /// A racer in the race may only back themselves.
    #[error("racers may only bet on themselves")]
    MustBackYourself,
    /// Below the book's minimum.
    #[error("minimum stake is {min}")]
    StakeTooLow {
        /// Book minimum.
        min: Money,
    },
    /// Above what the wallet allows (0 = betting locked for this bettor).
    #[error("maximum stake is {max}")]
    OverLimit {
        /// Wallet maximum.
        max: Money,
    },
    /// Already holding the maximum number of tickets on this race.
    #[error("at most {max} tickets per race")]
    TooManyTickets {
        /// Book maximum.
        max: u32,
    },
    /// The wallet refused the stake.
    #[error("wallet: {0}")]
    Wallet(String),
}

/// A placed bet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ticket<R> {
    /// Ticket number within the book (1-based).
    pub id: u32,
    /// What it backs.
    pub selection: Selection,
    /// Stake, credits.
    pub stake: Money,
    /// Odds locked in at placement.
    pub odds: Odds,
    /// Gross return if it wins.
    pub potential: Money,
    /// The wallet's receipt.
    pub receipt: R,
}

/// How a ticket ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    /// Paid at the ticket's odds.
    Won,
    /// Stake lost.
    Lost,
    /// Stake returned.
    Void,
}

/// One settled ticket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settlement {
    /// Ticket number.
    pub ticket: u32,
    /// What it backed.
    pub selection: Selection,
    /// Stake.
    pub stake: Money,
    /// Result.
    pub outcome: Outcome,
    /// Paid back (stake included).
    pub paid: Money,
}

impl Settlement {
    /// Profit (negative for a loss, 0 for a void).
    pub fn net(&self) -> Money {
        self.paid - self.stake
    }
}

/// One bettor's tickets on one race.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Book<R> {
    /// The prices.
    pub market: Market,
    /// The bettor's key when they are racing in it (only self-backing bets allowed).
    pub bettor_racing: Option<String>,
    /// Bets are still taken.
    pub open: bool,
    /// Placed tickets, in order.
    pub tickets: Vec<Ticket<R>>,
    /// Settled already (settle and void are once-only).
    pub settled: bool,
}

impl<R: Clone> Book<R> {
    /// An open book on `market`. `bettor_racing` is the bettor's own key when they race in it
    /// ([`PLAYER_KEY`] for the player).
    pub fn new(market: Market, bettor_racing: Option<&str>) -> Self {
        Self {
            market,
            bettor_racing: bettor_racing.map(str::to_string),
            open: true,
            tickets: Vec::new(),
            settled: false,
        }
    }

    /// An open book for the player racing in `market`.
    pub fn for_player(market: Market) -> Self {
        Self::new(market, Some(PLAYER_KEY))
    }

    /// The selections a racing bettor may back (everything when spectating): win, podium if
    /// offered, and a head-to-head against each rival.
    pub fn offered(&self) -> Vec<(Selection, Odds)> {
        let keys: Vec<String> = match &self.bettor_racing {
            Some(k) => vec![k.clone()],
            None => self.market.entrants.iter().map(|e| e.key.clone()).collect(),
        };
        let mut out = Vec::new();
        for k in keys {
            for sel in [Selection::Win(k.clone()), Selection::Podium(k.clone())] {
                if let Some(o) = self.market.odds(&sel) {
                    out.push((sel, o));
                }
            }
            for e in &self.market.entrants {
                let sel = Selection::HeadToHead {
                    backed: k.clone(),
                    opponent: e.key.clone(),
                };
                if let Some(o) = self.market.odds(&sel) {
                    out.push((sel, o));
                }
            }
        }
        out
    }

    /// Place a bet: checks the book is open, the selection is offered (and self-backing for
    /// a racing bettor), the stake is within `[min_stake, wallet.max_stake()]` and the ticket
    /// count, then takes the stake from the wallet and locks in the current odds.
    pub fn place<W: BetWallet<Receipt = R>>(
        &mut self,
        wallet: &mut W,
        selection: Selection,
        stake: Money,
    ) -> Result<&Ticket<R>, BetError> {
        self.place_quoted(wallet, selection, stake, None)
    }

    /// [`Book::place`] at `quoted` odds, the price the bettor was shown when they signed up
    /// (a picker card priced before the race was set up), instead of the market's current
    /// one. The quote is held to the book's odds bounds and step; `None` takes the market's
    /// price. The ticket keeps these odds through settlement.
    pub fn place_quoted<W: BetWallet<Receipt = R>>(
        &mut self,
        wallet: &mut W,
        selection: Selection,
        stake: Money,
        quoted: Option<Odds>,
    ) -> Result<&Ticket<R>, BetError> {
        if !self.open || self.settled {
            return Err(BetError::Closed);
        }
        if let Some(me) = &self.bettor_racing {
            if selection.backed() != me {
                return Err(BetError::MustBackYourself);
            }
        }
        let odds = self
            .market
            .odds(&selection)
            .ok_or_else(|| BetError::NotOffered(selection.label()))?;
        let rules = &self.market.rules;
        let odds = quoted.map_or(odds, |q| {
            let step = (rules.odds_step * 1000.0) as u32;
            let lo = (rules.min_odds * 1000.0) as u32;
            let hi = (rules.max_odds * 1000.0) as u32;
            Odds((q.0.clamp(lo, hi) / step * step).max(lo))
        });
        if self.tickets.len() as u32 >= rules.max_tickets {
            return Err(BetError::TooManyTickets {
                max: rules.max_tickets,
            });
        }
        if stake < rules.min_stake {
            return Err(BetError::StakeTooLow {
                min: rules.min_stake,
            });
        }
        let max = wallet.max_stake();
        if stake > max {
            return Err(BetError::OverLimit { max });
        }
        let label = format!("{} {}", self.market.race, selection.label());
        let receipt = wallet.take_stake(stake, &label).map_err(BetError::Wallet)?;
        self.tickets.push(Ticket {
            id: self.tickets.len() as u32 + 1,
            selection,
            stake,
            odds,
            potential: odds.payout(stake),
            receipt,
        });
        Ok(self.tickets.last().expect("just pushed"))
    }

    /// Stop taking bets (call at the green light).
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Total staked.
    pub fn staked(&self) -> Money {
        self.tickets.iter().map(|t| t.stake).sum()
    }

    /// Settle every ticket against the race's placings (every racer who started; see the
    /// module docs for the rules) and pay winners and voids through `wallet`. Once only:
    /// later calls return nothing.
    pub fn settle<W: BetWallet<Receipt = R>>(
        &mut self,
        wallet: &mut W,
        placings: &[Placing],
    ) -> Vec<Settlement> {
        if self.settled {
            return Vec::new();
        }
        self.settled = true;
        self.open = false;
        let find = |k: &str| placings.iter().find(|p| p.key == k);
        let mut out = Vec::with_capacity(self.tickets.len());
        for t in &self.tickets {
            let outcome = match &t.selection {
                Selection::Win(k) => match find(k) {
                    None => Outcome::Void,
                    Some(p) if p.finished && p.position == 1 => Outcome::Won,
                    Some(_) => Outcome::Lost,
                },
                Selection::Podium(k) => match find(k) {
                    None => Outcome::Void,
                    Some(p) if p.finished && p.position <= 3 => Outcome::Won,
                    Some(_) => Outcome::Lost,
                },
                Selection::HeadToHead { backed, opponent } => {
                    match (find(backed), find(opponent)) {
                        (Some(a), Some(b)) => match (a.finished, b.finished) {
                            (false, false) => Outcome::Void,
                            (true, false) => Outcome::Won,
                            (false, true) => Outcome::Lost,
                            (true, true) => {
                                if a.position < b.position {
                                    Outcome::Won
                                } else {
                                    Outcome::Lost
                                }
                            }
                        },
                        _ => Outcome::Void,
                    }
                }
            };
            let paid = match outcome {
                Outcome::Won => t.potential,
                Outcome::Lost => 0,
                Outcome::Void => t.stake,
            };
            wallet.pay_out(&t.receipt, paid);
            out.push(Settlement {
                ticket: t.id,
                selection: t.selection.clone(),
                stake: t.stake,
                outcome,
                paid,
            });
        }
        out
    }

    /// The race never ran (abandoned, server shut down): refund every stake. Once only.
    pub fn void_all<W: BetWallet<Receipt = R>>(&mut self, wallet: &mut W) -> Vec<Settlement> {
        self.settle(wallet, &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_price_is_kept_through_settlement() {
        let mut book: Book<Money> = Book::for_player(market());
        let mut w = CashWallet {
            cash: 10_000,
            cap: 5_000,
        };
        let sel = Selection::Win(PLAYER_KEY.into());
        let market_odds = book.market.odds(&sel).unwrap();
        let quote = Odds(market_odds.0 + 1_000);
        let t = book.place_quoted(&mut w, sel, 500, Some(quote)).unwrap();
        assert_eq!(t.odds, quote);
        assert_eq!(t.potential, quote.payout(500));
        // Off-step and out-of-bounds quotes are held to the book's rules.
        let t = book
            .place_quoted(
                &mut w,
                Selection::Podium(PLAYER_KEY.into()),
                100,
                Some(Odds(99_999)),
            )
            .unwrap();
        assert_eq!(t.odds, Odds(30_000));
    }

    fn entrant(key: &str, rating: i32) -> MarketEntrant {
        MarketEntrant {
            key: key.into(),
            name: key.into(),
            entity: None,
            rating,
        }
    }

    fn market() -> Market {
        let rules = BettingRules::embedded();
        Market::new(
            "test",
            vec![
                entrant("nina", 1300),
                entrant("mara", 1180),
                entrant("vic", 1120),
                entrant("tito", 880),
                entrant(PLAYER_KEY, 1000),
            ],
            &rules.book,
        )
    }

    fn pl(k: &str, p: u8, f: bool) -> Placing {
        Placing {
            key: k.into(),
            position: p,
            finished: f,
        }
    }

    #[test]
    fn embedded_rules_parse() {
        let r = BettingRules::embedded();
        assert!(r.book.win_margin > r.book.h2h_margin);
    }

    #[test]
    fn probabilities_are_consistent() {
        let m = market();
        let sum: f32 = m.win_p.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);
        let pod = m.podium_p.as_ref().unwrap();
        let sum3: f32 = pod.iter().sum();
        assert!((sum3 - 3.0).abs() < 1e-4, "{sum3}");
        for (w, p) in m.win_p.iter().zip(pod) {
            assert!(p > w && *p <= 1.0);
        }
        // Stronger is shorter.
        let b = m.board();
        assert!(b[0].win < b[1].win && b[1].win < b[2].win && b[2].win < b[3].win);
        // The book always keeps an edge: implied probabilities sum above 1.
        let implied: f32 = b.iter().map(|l| 1.0 / l.win.decimal()).sum();
        assert!(implied > 1.0, "{implied}");
        // Head-to-head pair odds both above evens only for a near-even match.
        let h = |a: &str, o: &str| {
            m.odds(&Selection::HeadToHead {
                backed: a.into(),
                opponent: o.into(),
            })
            .unwrap()
        };
        assert!(h("tito", "nina") > h("nina", "tito"));
        assert!(m
            .odds(&Selection::HeadToHead {
                backed: "vic".into(),
                opponent: "vic".into()
            })
            .is_none());
    }

    #[test]
    fn odds_round_down_and_clamp() {
        let r = BettingRules::embedded().book;
        assert_eq!(Odds::offer(0.5, 0.0, &r), Odds(2000));
        assert_eq!(Odds::offer(0.999, 0.15, &r), Odds(1050));
        assert_eq!(Odds::offer(0.0001, 0.15, &r), Odds(30000));
        let o = Odds::offer(0.3, 0.15, &r); // fair 2.898…
        assert_eq!(o, Odds(2850));
        assert_eq!(o.payout(100), 285);
        assert_eq!(o.payout(333), 949);
    }

    #[test]
    fn racer_may_only_back_themselves() {
        let mut book: Book<Money> = Book::for_player(market());
        let mut w = CashWallet {
            cash: 10_000,
            cap: 5_000,
        };
        assert_eq!(
            book.place(&mut w, Selection::Win("nina".into()), 500)
                .unwrap_err(),
            BetError::MustBackYourself
        );
        assert_eq!(
            book.place(
                &mut w,
                Selection::HeadToHead {
                    backed: "mara".into(),
                    opponent: PLAYER_KEY.into()
                },
                500
            )
            .unwrap_err(),
            BetError::MustBackYourself
        );
        assert!(book.offered().iter().all(|(s, _)| s.backed() == PLAYER_KEY));
        // win + podium + 4 head-to-heads
        assert_eq!(book.offered().len(), 6);
        book.place(&mut w, Selection::Win(PLAYER_KEY.into()), 500)
            .unwrap();
        assert_eq!(w.cash, 9_500);
    }

    #[test]
    fn limits_are_enforced() {
        let mut book: Book<Money> = Book::new(market(), None);
        let mut locked = CashWallet {
            cash: 10_000,
            cap: 0,
        };
        assert_eq!(
            book.place(&mut locked, Selection::Win("vic".into()), 100)
                .unwrap_err(),
            BetError::OverLimit { max: 0 }
        );
        let mut w = CashWallet {
            cash: 10_000,
            cap: 2_000,
        };
        assert_eq!(
            book.place(&mut w, Selection::Win("vic".into()), 50)
                .unwrap_err(),
            BetError::StakeTooLow { min: 100 }
        );
        assert_eq!(
            book.place(&mut w, Selection::Win("vic".into()), 2_001)
                .unwrap_err(),
            BetError::OverLimit { max: 2_000 }
        );
        assert!(matches!(
            book.place(&mut w, Selection::Win("nobody".into()), 200),
            Err(BetError::NotOffered(_))
        ));
        for _ in 0..3 {
            book.place(&mut w, Selection::Podium("vic".into()), 200)
                .unwrap();
        }
        assert_eq!(
            book.place(&mut w, Selection::Win("vic".into()), 200)
                .unwrap_err(),
            BetError::TooManyTickets { max: 3 }
        );
        book.close();
        let mut b2: Book<Money> = Book::new(market(), None);
        b2.close();
        assert_eq!(
            b2.place(&mut w, Selection::Win("vic".into()), 200)
                .unwrap_err(),
            BetError::Closed
        );
    }

    #[test]
    fn settlement_pays_wins_refunds_voids() {
        let mut book: Book<Money> = Book::new(market(), None);
        let mut w = CashWallet {
            cash: 10_000,
            cap: 10_000,
        };
        let win_odds = book.market.odds(&Selection::Win("tito".into())).unwrap();
        book.place(&mut w, Selection::Win("tito".into()), 1_000)
            .unwrap();
        book.place(&mut w, Selection::Podium("nina".into()), 1_000)
            .unwrap();
        book.place(
            &mut w,
            Selection::HeadToHead {
                backed: "vic".into(),
                opponent: "mara".into(),
            },
            1_000,
        )
        .unwrap();
        assert_eq!(w.cash, 7_000);
        book.close();
        // Tito wins, Nina DNF, Vic and Mara both DNF → H2H void.
        let s = book.settle(
            &mut w,
            &[
                pl("tito", 1, true),
                pl(PLAYER_KEY, 2, true),
                pl("nina", 3, false),
                pl("vic", 4, false),
                pl("mara", 5, false),
            ],
        );
        assert_eq!(
            s.iter().map(|x| x.outcome).collect::<Vec<_>>(),
            vec![Outcome::Won, Outcome::Lost, Outcome::Void]
        );
        assert_eq!(s[0].paid, win_odds.payout(1_000));
        assert_eq!(w.cash, 7_000 + win_odds.payout(1_000) + 1_000);
        // Once only.
        assert!(book.settle(&mut w, &[]).is_empty());
    }

    #[test]
    fn void_all_refunds_everything() {
        let mut book: Book<Money> = Book::for_player(market());
        let mut w = CashWallet {
            cash: 1_000,
            cap: 1_000,
        };
        book.place(&mut w, Selection::Win(PLAYER_KEY.into()), 400)
            .unwrap();
        book.place(&mut w, Selection::Podium(PLAYER_KEY.into()), 300)
            .unwrap();
        let s = book.void_all(&mut w);
        assert!(s.iter().all(|x| x.outcome == Outcome::Void));
        assert_eq!(w.cash, 1_000);
    }

    #[test]
    fn small_fields_have_no_podium_market() {
        let rules = BettingRules::embedded();
        let m = Market::new(
            "duel",
            vec![entrant("a", 1000), entrant("b", 1000), entrant("c", 1000)],
            &rules.book,
        );
        assert!(m.podium_p.is_none());
        assert!(m.odds(&Selection::Podium("a".into())).is_none());
    }

    #[test]
    fn book_is_profitable_in_the_long_run() {
        // Back every win line with 100 over many simulated races: the bettor loses about the
        // margin on average.
        use crate::roster::resolve_offscreen;
        let m = market();
        let field: Vec<(String, i32, f32)> = m
            .entrants
            .iter()
            .map(|e| (e.key.clone(), e.rating, 0.0))
            .collect();
        let mut net: i64 = 0;
        for seed in 0..2_000 {
            let r = resolve_offscreen(&field, seed);
            let winner = &r[0].key;
            for e in &m.entrants {
                let o = m.odds(&Selection::Win(e.key.clone())).unwrap();
                net += if &e.key == winner {
                    o.payout(100) - 100
                } else {
                    -100
                };
            }
        }
        let per_bet = net as f32 / (2_000.0 * 5.0);
        assert!((-25.0..-5.0).contains(&per_bet), "{per_bet}");
    }
}
