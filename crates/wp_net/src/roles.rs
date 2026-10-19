//! Role assignment for one instance.
//!
//! Rules (prompt 07):
//! - `Racer` and `Spectator` are always granted.
//! - Police (chase units + the captain) may hold at most `police_ratio` of the humans in the
//!   instance, rounded down, but at least one seat (so a lone cop can test).
//! - There is at most one `PoliceCaptain`. When the seat is vacant it is **offered** to the
//!   highest cop-rank chase unit (ties → lowest player id). The offer lasts
//!   `offer_ticks`; accepting = sending `RoleRequest(PoliceCaptain)`, declining = requesting
//!   anything else or letting it expire, after which the next-ranked unit is offered.
//! - A direct captain request for a vacant seat is granted only if nobody with a higher cop
//!   rank is a chase unit (they get the offer first; the requester becomes a chase unit).
//! - Nobody is forcibly demoted when players leave; the ratio applies to new requests.

use std::collections::{BTreeMap, BTreeSet};

use wp_contracts::{PlayerId, Role};

/// Role policy for an instance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoleConfig {
    /// Maximum police share of humans (default 0.4).
    pub police_ratio: f32,
    /// How long a captain offer stays open, physics ticks (default 10 s).
    pub offer_ticks: u64,
}

impl Default for RoleConfig {
    fn default() -> Self {
        Self {
            police_ratio: 0.4,
            offer_ticks: 10 * crate::TICK_HZ,
        }
    }
}

/// Something the server must tell clients after a role operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleEvent {
    /// `player` now holds `role`; `reason` explains a fallback.
    Assigned {
        /// Player.
        player: PlayerId,
        /// New role.
        role: Role,
        /// Why the requested role was not granted, if it was not.
        reason: Option<String>,
    },
    /// The captain seat is offered to `player` until `expires_tick`.
    Offer {
        /// Player.
        player: PlayerId,
        /// Expiry tick.
        expires_tick: u64,
    },
}

#[derive(Debug, Clone, Copy)]
struct Seat {
    role: Role,
    cop_rank: u32,
}

/// Tracks every human's role in one instance.
#[derive(Debug, Clone, Default)]
pub struct RoleManager {
    cfg: RoleConfig,
    seats: BTreeMap<PlayerId, Seat>,
    offer: Option<(PlayerId, u64)>,
    declined: BTreeSet<PlayerId>,
}

impl RoleManager {
    /// New manager.
    pub fn new(cfg: RoleConfig) -> Self {
        Self {
            cfg,
            ..Default::default()
        }
    }

    /// Add a player (starts as `Spectator` until they request a role).
    pub fn join(&mut self, player: PlayerId, cop_rank: u32) {
        self.seats.insert(
            player,
            Seat {
                role: Role::Spectator,
                cop_rank,
            },
        );
    }

    /// Update a player's cop rank (from the economy store).
    pub fn set_cop_rank(&mut self, player: PlayerId, cop_rank: u32) {
        if let Some(s) = self.seats.get_mut(&player) {
            s.cop_rank = cop_rank;
        }
    }

    /// Current role of a player.
    pub fn role(&self, player: PlayerId) -> Option<Role> {
        self.seats.get(&player).map(|s| s.role)
    }

    /// The captain, if the seat is filled.
    pub fn captain(&self) -> Option<PlayerId> {
        self.seats
            .iter()
            .find(|(_, s)| s.role == Role::PoliceCaptain)
            .map(|(p, _)| *p)
    }

    /// Outstanding captain offer `(player, expires_tick)`.
    pub fn offer(&self) -> Option<(PlayerId, u64)> {
        self.offer
    }

    /// Humans in the instance.
    pub fn humans(&self) -> usize {
        self.seats.len()
    }

    /// Players currently holding a police role (chase or captain).
    pub fn police_count(&self) -> usize {
        self.seats.values().filter(|s| is_police(s.role)).count()
    }

    /// Police seats allowed right now.
    pub fn max_police(&self) -> usize {
        ((self.cfg.police_ratio * self.humans() as f32).floor() as usize).max(1)
    }

    fn police_allowed(&self, player: PlayerId) -> bool {
        let already = self.seats.get(&player).is_some_and(|s| is_police(s.role));
        already || self.police_count() < self.max_police()
    }

    fn set(&mut self, player: PlayerId, role: Role, reason: Option<String>) -> RoleEvent {
        if let Some(s) = self.seats.get_mut(&player) {
            s.role = role;
        }
        RoleEvent::Assigned {
            player,
            role,
            reason,
        }
    }

    /// Highest-ranked chase unit that has not declined, excluding `except`.
    fn best_candidate(&self, except: Option<PlayerId>) -> Option<(PlayerId, u32)> {
        self.seats
            .iter()
            .filter(|(p, s)| {
                s.role == Role::PoliceChase && !self.declined.contains(p) && Some(**p) != except
            })
            // max_by_key keeps the *last* maximum; iterate in reverse so ties pick the lowest id.
            .rev()
            .max_by_key(|(_, s)| s.cop_rank)
            .map(|(p, s)| (*p, s.cop_rank))
    }

    /// Handle a `RoleRequest`. Returns the events to send (assignment first, then any offer).
    pub fn request(&mut self, player: PlayerId, wanted: Role, tick: u64) -> Vec<RoleEvent> {
        let Some(seat) = self.seats.get(&player).copied() else {
            return Vec::new();
        };
        let mut events = Vec::new();
        // Any request other than "captain" from the offered player declines the offer.
        if let Some((offered, _)) = self.offer {
            if offered == player && wanted != Role::PoliceCaptain {
                self.offer = None;
                self.declined.insert(player);
            }
        }
        let event = match wanted {
            Role::Racer | Role::Spectator => self.set(player, wanted, None),
            Role::PoliceChase => {
                if self.police_allowed(player) {
                    self.set(player, Role::PoliceChase, None)
                } else {
                    self.set(
                        player,
                        Role::Racer,
                        Some(format!(
                            "police seats full ({} of {})",
                            self.police_count(),
                            self.max_police()
                        )),
                    )
                }
            }
            Role::PoliceCaptain => self.request_captain(player, seat, tick),
        };
        events.push(event);
        events.extend(self.refresh_offer(tick));
        events
    }

    fn request_captain(&mut self, player: PlayerId, seat: Seat, tick: u64) -> RoleEvent {
        let fallback = |mgr: &mut Self, why: String| {
            if mgr.police_allowed(player) {
                mgr.set(player, Role::PoliceChase, Some(why))
            } else {
                mgr.set(
                    player,
                    Role::Racer,
                    Some(format!("{why}; police seats full")),
                )
            }
        };
        if let Some(c) = self.captain() {
            if c == player {
                return self.set(player, Role::PoliceCaptain, None);
            }
            return fallback(self, "captain seat taken".into());
        }
        if !self.police_allowed(player) {
            return self.set(player, Role::Racer, Some("police seats full".into()));
        }
        match self.offer {
            Some((offered, expires)) if offered == player && tick <= expires => {
                self.offer = None;
                self.set(player, Role::PoliceCaptain, None)
            }
            Some(_) => fallback(self, "captain seat offered to another unit".into()),
            None => match self.best_candidate(Some(player)) {
                Some((_, rank)) if rank > seat.cop_rank => {
                    fallback(self, "captain seat offered to a higher-ranked unit".into())
                }
                _ => self.set(player, Role::PoliceCaptain, None),
            },
        }
    }

    /// Open a captain offer if the seat is vacant and there is a candidate.
    fn refresh_offer(&mut self, tick: u64) -> Vec<RoleEvent> {
        if self.captain().is_some() {
            self.offer = None;
            self.declined.clear();
            return Vec::new();
        }
        if self.offer.is_some() {
            return Vec::new();
        }
        match self.best_candidate(None) {
            Some((p, _)) => {
                let expires_tick = tick + self.cfg.offer_ticks;
                self.offer = Some((p, expires_tick));
                vec![RoleEvent::Offer {
                    player: p,
                    expires_tick,
                }]
            }
            None => Vec::new(),
        }
    }

    /// Advance offer timers. Call once per net tick.
    pub fn update(&mut self, tick: u64) -> Vec<RoleEvent> {
        if let Some((p, expires)) = self.offer {
            if tick > expires {
                self.offer = None;
                self.declined.insert(p);
            }
        }
        self.refresh_offer(tick)
    }

    /// Remove a player (left for good). Vacating the captain seat starts a hand-off.
    pub fn leave(&mut self, player: PlayerId, tick: u64) -> Vec<RoleEvent> {
        let was_captain = self.role(player) == Some(Role::PoliceCaptain);
        self.seats.remove(&player);
        self.declined.remove(&player);
        if self.offer.is_some_and(|(p, _)| p == player) {
            self.offer = None;
        }
        if was_captain {
            self.declined.clear();
        }
        self.refresh_offer(tick)
    }
}

fn is_police(r: Role) -> bool {
    matches!(r, Role::PoliceChase | Role::PoliceCaptain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mgr(n: u32) -> RoleManager {
        let mut m = RoleManager::new(RoleConfig::default());
        for i in 0..n {
            m.join(PlayerId(i), i * 10);
        }
        m
    }

    fn assigned(ev: &[RoleEvent]) -> Role {
        match &ev[0] {
            RoleEvent::Assigned { role, .. } => *role,
            e => panic!("expected assignment, got {e:?}"),
        }
    }

    #[test]
    fn police_ratio_caps_chase_units() {
        let mut m = mgr(10); // 40% of 10 = 4 police seats
        for i in 0..4 {
            assert_eq!(
                assigned(&m.request(PlayerId(i), Role::PoliceChase, 0)),
                Role::PoliceChase
            );
        }
        let ev = m.request(PlayerId(4), Role::PoliceChase, 0);
        assert_eq!(assigned(&ev), Role::Racer);
        assert_eq!(m.police_count(), 4);
        // Re-requesting police while already police is fine.
        assert_eq!(
            assigned(&m.request(PlayerId(0), Role::PoliceChase, 0)),
            Role::PoliceChase
        );
        // Racer/Spectator always granted.
        assert_eq!(
            assigned(&m.request(PlayerId(5), Role::Spectator, 0)),
            Role::Spectator
        );
    }

    #[test]
    fn lone_player_may_be_police() {
        let mut m = mgr(1);
        assert_eq!(m.max_police(), 1);
        assert_eq!(
            assigned(&m.request(PlayerId(0), Role::PoliceCaptain, 0)),
            Role::PoliceCaptain
        );
    }

    #[test]
    fn exactly_one_captain() {
        let mut m = mgr(10);
        assert_eq!(
            assigned(&m.request(PlayerId(9), Role::PoliceCaptain, 0)),
            Role::PoliceCaptain
        );
        let ev = m.request(PlayerId(8), Role::PoliceCaptain, 0);
        assert_eq!(assigned(&ev), Role::PoliceChase);
        assert_eq!(m.captain(), Some(PlayerId(9)));
    }

    #[test]
    fn vacant_seat_is_offered_to_highest_rank_then_next_on_expiry() {
        let mut m = mgr(10);
        // Ranks are id*10: player 3 outranks 1 and 2.
        for p in [1, 3, 2] {
            m.request(PlayerId(p), Role::PoliceChase, 0);
        }
        // First chase request with a vacant seat already opened an offer to player 1; it is
        // only re-targeted once it lapses. Let it expire.
        let first = m.offer().unwrap().0;
        assert_eq!(first, PlayerId(1));
        let ev = m.update(m.offer().unwrap().1 + 1);
        assert_eq!(
            ev,
            vec![RoleEvent::Offer {
                player: PlayerId(3),
                expires_tick: m.offer().unwrap().1
            }]
        );
        // Player 3 declines by asking to stay a chase unit → offer goes to 2.
        let ev = m.request(PlayerId(3), Role::PoliceChase, 3000);
        assert!(matches!(
            ev[1],
            RoleEvent::Offer {
                player: PlayerId(2),
                ..
            }
        ));
        // Player 2 accepts.
        assert_eq!(
            assigned(&m.request(PlayerId(2), Role::PoliceCaptain, 3001)),
            Role::PoliceCaptain
        );
        assert_eq!(m.offer(), None);
    }

    #[test]
    fn captain_leaving_hands_seat_to_highest_rank() {
        let mut m = mgr(10);
        m.request(PlayerId(9), Role::PoliceCaptain, 0);
        m.request(PlayerId(2), Role::PoliceChase, 0);
        m.request(PlayerId(7), Role::PoliceChase, 0);
        assert_eq!(m.offer(), None, "no offer while the seat is filled");
        let ev = m.leave(PlayerId(9), 100);
        assert_eq!(
            ev,
            vec![RoleEvent::Offer {
                player: PlayerId(7),
                expires_tick: 100 + RoleConfig::default().offer_ticks
            }]
        );
        // A lower-ranked unit asking directly does not jump the queue.
        let ev = m.request(PlayerId(2), Role::PoliceCaptain, 101);
        assert_eq!(assigned(&ev), Role::PoliceChase);
        assert_eq!(
            assigned(&m.request(PlayerId(7), Role::PoliceCaptain, 102)),
            Role::PoliceCaptain
        );
    }

    #[test]
    fn direct_request_defers_to_higher_ranked_chase_unit() {
        let mut m = mgr(10);
        m.request(PlayerId(8), Role::PoliceChase, 0);
        // Offer to 8 is open; 8 declines.
        m.request(PlayerId(8), Role::Racer, 1);
        assert_eq!(m.offer(), None);
        // Seat vacant, no offer, nobody outranks 5 among chase units → granted.
        assert_eq!(
            assigned(&m.request(PlayerId(5), Role::PoliceCaptain, 2)),
            Role::PoliceCaptain
        );
    }
}
