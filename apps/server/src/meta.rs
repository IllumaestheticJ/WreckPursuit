//! The meta-service link (`wp_meta::instance`, machine O's hook) wired into the game loop:
//! "Server hookup" steps 1–7 of `docs/integration/meta.md` (territory, step 8, waits for a
//! district-control source).
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
//! - **Economy write-through**: a verified player's account is loaded from meta
//!   (`LoadAccount`) and installed as the instance's copy (money changes made while it loads
//!   wait and are applied on arrival). Every change — stake, payout, rep — is one ledger event
//!   committed locally and sent to meta (`Commit`) in order. On `seq_conflict` (another host
//!   wrote first) the account is reloaded and the changes meta has not taken are applied again
//!   on top, as new events.
//! - **Race results**: every finished lobby race is reported (`Race`) with each meta player's
//!   position, time and clean flag (no damaging contact, no violation beyond speeding), after
//!   the payouts were committed. `report_key` = instance + lobby race id, so retries dedupe.
//! - **Pursuits**: every `PursuitEnded` (escape or capture) of a player racer is reported with
//!   the highest wanted level it reached, its bounty and the player cops credited.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use wp_contracts::{DamageMode, EntityId, PlayerId, PoliceEvent, PursuitOutcome, Role, SimEvent};
use wp_meta::api::{AccountId, HeartbeatRequest, PursuitReport, RaceEntry, RaceReport, RoleCounts};
use wp_meta::instance::{ticks_to_ms, MetaReply, MetaRequest, MetaWorker};
use wp_net::native::LogFn;
use wp_net::netsim::NetSim;
use wp_net::server::EconomyCommit;
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
    /// Instance damage mode and class bracket (race reports).
    mode: DamageMode,
    bracket: String,
    /// Commits not yet confirmed by meta, per account, in order. Only the front one is in
    /// flight: the next goes out after it is confirmed, so a conflict is seen before a later
    /// commit (built on the same stale account) could be accepted on top of it.
    in_flight: BTreeMap<AccountId, VecDeque<EconomyCommit>>,
    /// Accounts being reloaded after a sequence conflict: the changes to apply again.
    redo: BTreeMap<AccountId, Vec<EconomyCommit>>,
    /// Account loads that failed, retried at this time.
    retry_load: BTreeMap<AccountId, Instant>,
    /// Counters for logs and tests.
    pub stats: MetaStats,
}

/// What the hook has done (logged, read by tests).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetaStats {
    /// Accounts loaded from meta.
    pub accounts_loaded: u32,
    /// Economy commits confirmed by meta.
    pub commits_ok: u32,
    /// Sequence conflicts resolved by reload + re-apply.
    pub commit_conflicts: u32,
    /// Commits meta refused for another reason (logged, dropped).
    pub commits_failed: u32,
    /// Race reports sent.
    pub races_reported: u32,
    /// Pursuit reports sent.
    pub pursuits_reported: u32,
}

impl MetaHook {
    /// Hook over a started worker; `mode` and `bracket` are the instance's (race reports).
    pub fn new(worker: MetaWorker, log: LogFn, mode: DamageMode, bracket: String) -> Self {
        Self {
            mode,
            bracket,
            in_flight: BTreeMap::new(),
            redo: BTreeMap::new(),
            retry_load: BTreeMap::new(),
            stats: MetaStats::default(),
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

    fn player_of(&self, account: AccountId) -> Option<PlayerId> {
        self.accounts
            .iter()
            .find(|(_, a)| **a == account)
            .map(|(p, _)| *p)
    }

    /// Send the front of `account`'s commit queue (if any).
    fn send_next_commit(&mut self, account: AccountId) {
        if let Some(c) = self.in_flight.get(&account).and_then(|q| q.front()) {
            self.worker.send(MetaRequest::Commit {
                account: Box::new(c.account.clone()),
                events: c.events.clone(),
            });
        }
    }

    fn load(&mut self, account: AccountId) {
        self.retry_load.remove(&account);
        self.worker.send(MetaRequest::LoadAccount { account });
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
                                // Money for this player lives in meta from now on.
                                inst.expect_external_account(pid);
                                self.load(v.account_id);
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
                MetaReply::AccountLoaded { account, result } => {
                    let Some(pid) = self.player_of(account) else {
                        continue;
                    };
                    match result {
                        Ok(Some(acct)) => {
                            self.stats.accounts_loaded += 1;
                            inst.set_external_account(pid, acct);
                            // After a conflict: apply what meta did not take, as new events.
                            for c in self.redo.remove(&account).unwrap_or_default() {
                                for e in &c.events {
                                    let reason = e.to_contract().reason;
                                    inst.credit_player(pid, e.cash_delta, e.rep_delta, reason);
                                }
                            }
                        }
                        Ok(None) => {
                            (self.log)(
                                "meta_account_missing",
                                vec![("account", account.to_string())],
                            );
                            let empty = wp_economy::PlayerAccount::empty(PlayerId(account));
                            inst.set_external_account(pid, empty);
                        }
                        Err(e) => {
                            (self.log)(
                                "meta_account_load_failed",
                                vec![("account", account.to_string()), ("error", e.to_string())],
                            );
                            self.retry_load
                                .insert(account, Instant::now() + Duration::from_secs(1));
                        }
                    }
                }
                MetaReply::Committed { account, result } => {
                    let queue = self.in_flight.entry(account).or_default();
                    match result {
                        Ok(()) => {
                            self.stats.commits_ok += 1;
                            queue.pop_front();
                            self.send_next_commit(account);
                        }
                        Err(e) if e.code() == Some("seq_conflict") => {
                            // Someone else wrote first: reload, then apply this change and
                            // every queued one again on top of meta's account.
                            self.stats.commit_conflicts += 1;
                            let again: Vec<EconomyCommit> =
                                self.in_flight.remove(&account).unwrap_or_default().into();
                            self.redo.entry(account).or_default().extend(again);
                            if let Some(pid) = self.player_of(account) {
                                inst.expect_external_account(pid);
                            }
                            self.load(account);
                        }
                        Err(e) => {
                            self.stats.commits_failed += 1;
                            queue.pop_front();
                            (self.log)(
                                "meta_commit_failed",
                                vec![("account", account.to_string()), ("error", e.to_string())],
                            );
                            self.send_next_commit(account);
                        }
                    }
                }
                MetaReply::RaceReported(Err(e)) => {
                    (self.log)("meta_race_report_failed", vec![("error", e.to_string())]);
                }
                MetaReply::PursuitReported(Err(e)) => {
                    (self.log)("meta_pursuit_report_failed", vec![("error", e.to_string())]);
                }
                _ => {}
            }
        }

        // Account loads to retry.
        let due: Vec<AccountId> = self
            .retry_load
            .iter()
            .filter(|(_, t)| **t <= Instant::now())
            .map(|(a, _)| *a)
            .collect();
        for a in due {
            self.load(a);
        }

        // Economy write-through, in commit order.
        for c in inst.take_economy_commits() {
            let account = c.account.player.0;
            if self.redo.contains_key(&account) {
                // Reloading after a conflict: this change is applied again once loaded.
                self.redo.entry(account).or_default().push(c);
                continue;
            }
            let queue = self.in_flight.entry(account).or_default();
            queue.push_back(c);
            if queue.len() == 1 {
                self.send_next_commit(account);
            }
        }

        // Finished races (their payouts were committed above, before the report).
        for race in inst.take_finished_races() {
            let entries: Vec<RaceEntry> = race
                .results
                .iter()
                .filter_map(|r| {
                    let account_id = self.account_of(PlayerId(r.player))?;
                    let finished = r.place > 0;
                    Some(RaceEntry {
                        account_id,
                        position: r.place as u32,
                        finish_time_ms: finished.then(|| ticks_to_ms(r.time_ticks)),
                        clean: finished && race.clean.contains(&PlayerId(r.player)),
                    })
                })
                .collect();
            if entries.is_empty() {
                continue;
            }
            self.stats.races_reported += 1;
            self.worker.send(MetaRequest::Race(RaceReport {
                report_key: format!("{}-race-{}", self.worker.instance_id(), race.race),
                route_id: route_id(race.laps, &race.checkpoints),
                race_type: format!("{:?}", race.race_type).to_lowercase(),
                mode: self.mode,
                bracket: self.bracket.clone(),
                entries,
            }));
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
                    self.stats.pursuits_reported += 1;
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

/// Route id of a lobby race: laps + checkpoints (dm), so the same course on any instance
/// shares a time board. FNV-1a over the values.
pub fn route_id(laps: u8, checkpoints: &[[i32; 2]]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: &[u8]| {
        for x in b {
            h ^= *x as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(&[laps]);
    for c in checkpoints {
        eat(&c[0].to_le_bytes());
        eat(&c[1].to_le_bytes());
    }
    format!("lobby-{:016x}", h)
}
