//! The meta-service link (`wp_meta::instance`, machine O's hook) wired into the game loop:
//! "Server hookup" steps 1–4 and 7 of `docs/integration/meta.md`.
//!
//! - **Startup**: `run` registers the instance (`MetaWorker::start`) when `WP_META_URL` is set
//!   and turns on [`wp_net::server::ServerConfig::require_join_token`].
//! - **Handshake**: every new player's `Hello.join_token` is verified with meta
//!   (`VerifyJoin`); verified players join under their meta display name, are seated in the
//!   role meta reserved and receive meta's reconnect token in `Welcome`; anything else is
//!   rejected. The in-window u64 reattach needs no verification.
//! - **Presence**: once a second (and right after joins): population per role and the meta
//!   account ids of connected players.
//! - **Kicks**: heartbeat `kicks` disconnect the player for good.
//! - **Pursuits**: every `PursuitEnded` (escape or capture) of a player racer is reported with
//!   the highest wanted level it reached, its bounty and the player cops credited.
//!
//! Economy write-through (step 5) and race reports (step 6) are not wired yet (the in-instance
//! lobby keeps its in-memory accounts); see `docs/integration/net.md`.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use wp_contracts::{EntityId, PlayerId, PoliceEvent, PursuitOutcome, Role, SimEvent};
use wp_meta::api::{AccountId, HeartbeatRequest, PursuitReport, RoleCounts};
use wp_meta::instance::{MetaReply, MetaRequest, MetaWorker};
use wp_net::native::LogFn;
use wp_net::netsim::NetSim;
use wp_net::server::ServerInstance;

/// Per-tick meta link state (lives on the game thread).
pub struct MetaHook {
    worker: MetaWorker,
    log: LogFn,
    /// Meta account of every verified player.
    accounts: BTreeMap<PlayerId, AccountId>,
    /// Highest wanted level each racer reached in its current pursuit.
    max_wanted: BTreeMap<EntityId, wp_contracts::WantedLevel>,
    last_presence: Option<Instant>,
    presence_dirty: bool,
    pursuits_reported: u32,
}

impl MetaHook {
    /// Hook over a started worker.
    pub fn new(worker: MetaWorker, log: LogFn) -> Self {
        Self {
            worker,
            log,
            accounts: BTreeMap::new(),
            max_wanted: BTreeMap::new(),
            last_presence: None,
            presence_dirty: true,
            pursuits_reported: 0,
        }
    }

    /// Meta account of a verified player.
    pub fn account_of(&self, pid: PlayerId) -> Option<AccountId> {
        self.accounts.get(&pid).copied()
    }

    /// Run once per net tick on the game thread (never blocks).
    pub fn tick<S: NetSim>(&mut self, inst: &mut ServerInstance<S>) {
        // Handshake: verify new players' join tokens.
        for req in inst.take_join_requests() {
            if req.join_token.is_empty() {
                inst.reject_join(req.conn, "a matchmaking join token is required");
                continue;
            }
            self.worker.send(MetaRequest::VerifyJoin {
                tag: req.conn as u64,
                join_token: req.join_token,
            });
        }
        for reply in self.worker.poll() {
            match reply {
                MetaReply::Joined { tag, result } => {
                    let conn = tag as u32;
                    match result {
                        Ok(v) => {
                            let name = v.display_name.clone();
                            if let Some(pid) =
                                inst.approve_join(conn, Some(v.display_name), v.reconnect_token)
                            {
                                self.accounts.insert(pid, v.account_id);
                                inst.assign_role(pid, v.role);
                                self.presence_dirty = true;
                                (self.log)(
                                    "meta_join",
                                    vec![
                                        ("player", pid.0.to_string()),
                                        ("account", v.account_id.to_string()),
                                        ("name", name),
                                        ("role", format!("{:?}", v.role)),
                                    ],
                                );
                            }
                        }
                        Err(e) => {
                            (self.log)(
                                "meta_join_refused",
                                vec![("conn", conn.to_string()), ("error", e.to_string())],
                            );
                            inst.reject_join(conn, format!("join refused: {e}"));
                        }
                    }
                }
                MetaReply::Heartbeat(Ok(h)) => {
                    for k in h.kicks {
                        let pid = self
                            .accounts
                            .iter()
                            .find(|(_, a)| **a == k.account_id)
                            .map(|(p, _)| *p);
                        if let Some(pid) = pid {
                            inst.kick(pid, k.reason.clone());
                            self.accounts.remove(&pid);
                            self.presence_dirty = true;
                            (self.log)(
                                "meta_kick",
                                vec![("player", pid.0.to_string()), ("reason", k.reason)],
                            );
                        }
                    }
                }
                MetaReply::Heartbeat(Err(e)) => {
                    (self.log)("meta_heartbeat_failed", vec![("error", e.to_string())]);
                }
                MetaReply::PursuitReported(Err(e)) => {
                    (self.log)("meta_pursuit_report_failed", vec![("error", e.to_string())]);
                }
                _ => {}
            }
        }

        // Pursuits of player racers.
        for ev in inst.take_tapped_events() {
            let SimEvent::Police(pe) = ev else { continue };
            match pe {
                PoliceEvent::WantedLevelChanged { racer, level } => {
                    let m = self.max_wanted.entry(racer).or_default();
                    *m = (*m).max(level);
                }
                PoliceEvent::PursuitEnded {
                    racer,
                    outcome,
                    level,
                    capturer,
                    assists,
                } => {
                    let max = self
                        .max_wanted
                        .remove(&racer)
                        .unwrap_or_default()
                        .max(level);
                    let Some(racer_id) = inst.owner_of(racer).and_then(|p| self.account_of(p))
                    else {
                        continue;
                    };
                    let outcome = match outcome {
                        PursuitOutcome::Captured | PursuitOutcome::CapturedTotalLoss => {
                            wp_meta::api::PursuitOutcome::Captured
                        }
                        // Escape, safehouse or call-off: the racer got away.
                        _ => wp_meta::api::PursuitOutcome::Escaped,
                    };
                    self.pursuits_reported += 1;
                    let accounts = &self.accounts;
                    let cop = |u| {
                        inst.player_of_unit(u)
                            .and_then(|p| accounts.get(&p).copied())
                    };
                    let report = PursuitReport {
                        report_key: format!(
                            "{}-{}-{}",
                            self.worker.instance_id(),
                            inst.sim().tick(),
                            racer.0
                        ),
                        racer_id,
                        max_wanted: max,
                        bounty: inst.sim().bounty(racer),
                        outcome,
                        captured_by: capturer.and_then(cop),
                        assists: assists.into_iter().filter_map(cop).collect(),
                    };
                    self.worker.send(MetaRequest::Pursuit(report));
                }
                _ => {}
            }
        }

        // Presence.
        let due = self
            .last_presence
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(1));
        if due || self.presence_dirty {
            self.presence_dirty = false;
            self.last_presence = Some(Instant::now());
            let mut population = RoleCounts::default();
            let mut players = Vec::new();
            for (pid, role) in inst.connected_players() {
                match role {
                    Role::Racer => population.racer += 1,
                    Role::PoliceChase => population.police_chase += 1,
                    Role::PoliceCaptain => population.police_captain += 1,
                    Role::Spectator => population.spectator += 1,
                }
                if let Some(a) = self.account_of(pid) {
                    players.push(a);
                }
            }
            self.worker.set_presence(HeartbeatRequest {
                population,
                players,
                draining: false,
                tick_ms: 0.0,
            });
        }
    }
}
