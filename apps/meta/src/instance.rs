//! Opt-in meta hook for city servers (`apps/server`): everything an instance needs from the meta
//! service, behind an API that never blocks the server tick.
//!
//! - [`MetaLinkConfig::from_env`] returns `None` when `WP_META_URL` is unset: the server runs
//!   standalone exactly as before.
//! - [`MetaLink`] is the blocking connection (register, heartbeat, verify joins, economy, results).
//! - [`MetaWorker`] runs a `MetaLink` on its own thread: the tick loop calls
//!   [`MetaWorker::set_presence`] / [`MetaWorker::send`] (never block) and drains
//!   [`MetaWorker::poll`] once per tick. The worker heartbeats on its own schedule and reports
//!   kicks and flags back as [`MetaReply::Heartbeat`].
//!
//! Wiring steps for the server are listed in `docs/integration/meta.md` ("Server hookup").

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use wp_contracts::{DamageMode, PHYSICS_DT};
use wp_economy::{EconomyEvent, PlayerAccount};

use crate::api::*;
use crate::client::{ClientError, ClientResult, MetaClient, RemoteAccountStore};

/// Everything needed to register an instance.
#[derive(Debug, Clone)]
pub struct MetaLinkConfig {
    /// Meta service base URL (`WP_META_URL`).
    pub url: String,
    /// Instance registration key (`WP_META_INSTANCE_KEY`).
    pub instance_key: String,
    /// Registration details.
    pub register: RegisterInstanceRequest,
    /// Heartbeat period; `None` = the interval the service asks for at registration.
    pub heartbeat_every: Option<Duration>,
}

impl MetaLinkConfig {
    /// Read the hook configuration from the process environment. `None` when `WP_META_URL`
    /// is unset (standalone server). See [`MetaLinkConfig::from_vars`] for the variables.
    pub fn from_env(
        mode: DamageMode,
        slots: RoleCounts,
        default_address: &str,
    ) -> Option<Result<Self, String>> {
        Self::from_vars(|k| std::env::var(k).ok(), mode, slots, default_address)
    }

    /// Same as [`MetaLinkConfig::from_env`] with an explicit variable lookup (tests).
    ///
    /// | Variable | Default |
    /// |---|---|
    /// | `WP_META_URL` | unset = no hook |
    /// | `WP_META_INSTANCE_KEY` | required when the URL is set |
    /// | `WP_META_INSTANCE_NAME` | `wp-server` |
    /// | `WP_META_REGION` | `local` |
    /// | `WP_META_CITY` | `default` |
    /// | `WP_META_BRACKET` | `open` |
    /// | `WP_META_PUBLIC_ADDRESS` | `default_address` (what clients connect to) |
    pub fn from_vars(
        var: impl Fn(&str) -> Option<String>,
        mode: DamageMode,
        slots: RoleCounts,
        default_address: &str,
    ) -> Option<Result<Self, String>> {
        let get = |k: &str| var(k).filter(|v| !v.trim().is_empty());
        let url = get("WP_META_URL")?;
        let Some(instance_key) = get("WP_META_INSTANCE_KEY") else {
            return Some(Err(
                "WP_META_URL is set but WP_META_INSTANCE_KEY is not".into()
            ));
        };
        let or = |k: &str, d: &str| get(k).unwrap_or_else(|| d.to_string());
        Some(Ok(Self {
            url,
            instance_key,
            register: RegisterInstanceRequest {
                name: or("WP_META_INSTANCE_NAME", "wp-server"),
                region: or("WP_META_REGION", "local"),
                city: or("WP_META_CITY", "default"),
                mode,
                bracket: or("WP_META_BRACKET", "open"),
                address: or("WP_META_PUBLIC_ADDRESS", default_address),
                slots,
            },
            heartbeat_every: None,
        }))
    }
}

/// A registered instance's blocking connection to the meta service.
#[derive(Clone)]
pub struct MetaLink {
    client: MetaClient,
    instance_id: InstanceId,
    heartbeat_every: Duration,
}

impl MetaLink {
    /// Register the instance.
    pub fn register(cfg: &MetaLinkConfig) -> ClientResult<Self> {
        let reg = MetaClient::new(&cfg.url)
            .with_token(&cfg.instance_key)
            .register_instance(&cfg.register)?;
        Ok(Self {
            client: MetaClient::new(&cfg.url).with_token(&reg.instance_token),
            instance_id: reg.instance_id,
            heartbeat_every: cfg.heartbeat_every.unwrap_or(Duration::from_millis(
                reg.heartbeat_interval_ms.max(100) as u64,
            )),
        })
    }

    /// Id the service allocated.
    pub fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    /// How often to heartbeat.
    pub fn heartbeat_every(&self) -> Duration {
        self.heartbeat_every
    }

    /// Send a heartbeat.
    pub fn heartbeat(&self, presence: &HeartbeatRequest) -> ClientResult<HeartbeatResponse> {
        self.client.heartbeat(self.instance_id, presence)
    }

    /// Check a client's join token.
    pub fn verify_join(&self, join_token: &str) -> ClientResult<VerifyJoinResponse> {
        self.client.verify_join(self.instance_id, join_token)
    }

    /// `AccountStore` over HTTP for players this instance hosts (blocking).
    pub fn account_store(&self) -> RemoteAccountStore {
        RemoteAccountStore::new(self.client.clone(), self.instance_id)
    }

    /// Load a hosted player's economy account.
    pub fn load_account(&self, account: AccountId) -> ClientResult<Option<PlayerAccount>> {
        self.client.load_economy(self.instance_id, account)
    }

    /// Commit a hosted player's economy change.
    pub fn commit(&self, account: &PlayerAccount, events: &[EconomyEvent]) -> ClientResult<()> {
        self.client
            .commit_economy(self.instance_id, account, events)
    }

    /// Report a race.
    pub fn report_race(&self, report: &RaceReport) -> ClientResult<RaceReportResponse> {
        self.client.report_race(self.instance_id, report)
    }

    /// Report a pursuit.
    pub fn report_pursuit(&self, report: &PursuitReport) -> ClientResult<PursuitReportResponse> {
        self.client.report_pursuit(self.instance_id, report)
    }

    /// Report a territory change.
    pub fn report_territory(&self, report: &TerritoryReport) -> ClientResult<()> {
        self.client.report_territory(self.instance_id, report)
    }

    /// Deregister (the token stops working).
    pub fn deregister(&self) -> ClientResult<()> {
        self.client.deregister(self.instance_id)
    }
}

/// Race time in physics ticks → milliseconds (for [`RaceEntry::finish_time_ms`]).
pub fn ticks_to_ms(ticks: u64) -> u64 {
    (ticks as f64 * PHYSICS_DT as f64 * 1000.0).round() as u64
}

/// Work for the [`MetaWorker`]. `tag`s are opaque to the worker and come back in the reply.
#[derive(Debug, Clone)]
pub enum MetaRequest {
    /// Verify a join token (from the client's handshake). `tag` = the server's connection id.
    VerifyJoin {
        /// Caller's handle (connection id).
        tag: u64,
        /// Token the client presented.
        join_token: String,
    },
    /// Load a hosted player's economy account.
    LoadAccount {
        /// Account.
        account: AccountId,
    },
    /// Commit a hosted player's economy change (write-through from the server's local copy).
    Commit {
        /// Account after the change.
        account: Box<PlayerAccount>,
        /// Events the change posted.
        events: Vec<EconomyEvent>,
    },
    /// Report a race.
    Race(RaceReport),
    /// Report a pursuit.
    Pursuit(PursuitReport),
    /// Report a territory change.
    Territory(TerritoryReport),
}

/// Results coming back from the [`MetaWorker`], in completion order.
#[derive(Debug, Clone)]
pub enum MetaReply {
    /// A heartbeat went through: carry out `kicks`, apply `flags`.
    Heartbeat(Result<HeartbeatResponse, ClientError>),
    /// Join verification finished. `Err` → reject the connection.
    Joined {
        /// The `tag` of the request.
        tag: u64,
        /// Who joined, or why not.
        result: Result<VerifyJoinResponse, ClientError>,
    },
    /// Account loaded (`Ok(None)`: no economy account).
    AccountLoaded {
        /// Account.
        account: AccountId,
        /// Result.
        result: Result<Option<PlayerAccount>, ClientError>,
    },
    /// Commit finished. `seq_conflict` → reload the account and redo the operation.
    Committed {
        /// Account.
        account: AccountId,
        /// Result.
        result: Result<(), ClientError>,
    },
    /// Race report finished (safe to resend the same report on error; `report_key` dedupes).
    RaceReported(Result<RaceReportResponse, ClientError>),
    /// Pursuit report finished.
    PursuitReported(Result<PursuitReportResponse, ClientError>),
    /// Territory report finished.
    TerritoryReported(Result<(), ClientError>),
}

enum Cmd {
    Work(MetaRequest),
    Stop,
}

/// Runs a [`MetaLink`] on a background thread so the server tick never waits on HTTP.
pub struct MetaWorker {
    instance_id: InstanceId,
    presence: Arc<Mutex<HeartbeatRequest>>,
    cmd_tx: Sender<Cmd>,
    reply_rx: Receiver<MetaReply>,
    thread: Option<JoinHandle<()>>,
}

impl MetaWorker {
    /// Register the instance and start the worker. Registration happens on the calling thread
    /// (once, at startup) so a bad URL or key fails loudly.
    pub fn start(cfg: &MetaLinkConfig) -> ClientResult<Self> {
        let link = MetaLink::register(cfg)?;
        Ok(Self::with_link(link))
    }

    /// Start the worker on an already-registered link.
    pub fn with_link(link: MetaLink) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (reply_tx, reply_rx) = mpsc::channel::<MetaReply>();
        let presence = Arc::new(Mutex::new(HeartbeatRequest::default()));
        let instance_id = link.instance_id();
        let shared = Arc::clone(&presence);
        let thread = std::thread::Builder::new()
            .name("wp-meta-link".into())
            .spawn(move || run(link, shared, cmd_rx, reply_tx))
            .expect("spawn meta worker thread");
        Self {
            instance_id,
            presence,
            cmd_tx,
            reply_rx,
            thread: Some(thread),
        }
    }

    /// Id the service allocated.
    pub fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    /// Update what the next heartbeat reports. Call whenever players join/leave (cheap).
    /// List every connected meta account in `players`: a verified join holds its seat until a
    /// heartbeat lists the player.
    pub fn set_presence(&self, presence: HeartbeatRequest) {
        *self.presence.lock().unwrap_or_else(|p| p.into_inner()) = presence;
    }

    /// Queue work (never blocks).
    pub fn send(&self, req: MetaRequest) {
        let _ = self.cmd_tx.send(Cmd::Work(req));
    }

    /// Replies that arrived since the last call (never blocks).
    pub fn poll(&self) -> Vec<MetaReply> {
        let mut out = Vec::new();
        loop {
            match self.reply_rx.try_recv() {
                Ok(r) => out.push(r),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return out,
            }
        }
    }

    /// Finish queued work, deregister and stop the thread (blocks; call at shutdown).
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let _ = self.cmd_tx.send(Cmd::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for MetaWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(
    link: MetaLink,
    presence: Arc<Mutex<HeartbeatRequest>>,
    cmds: Receiver<Cmd>,
    replies: Sender<MetaReply>,
) {
    let beat = || {
        let p = presence.lock().unwrap_or_else(|p| p.into_inner()).clone();
        MetaReply::Heartbeat(link.heartbeat(&p))
    };
    let mut next_beat = Instant::now();
    loop {
        let now = Instant::now();
        if now >= next_beat {
            if replies.send(beat()).is_err() {
                break;
            }
            next_beat = now + link.heartbeat_every();
        }
        let wait = next_beat.saturating_duration_since(Instant::now());
        match cmds.recv_timeout(wait) {
            Ok(Cmd::Work(req)) => {
                if replies.send(handle(&link, req)).is_err() {
                    break;
                }
            }
            Ok(Cmd::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    // Drain work queued before the stop (e.g. final commits), then leave the directory.
    while let Ok(Cmd::Work(req)) = cmds.try_recv() {
        let _ = replies.send(handle(&link, req));
    }
    let _ = link.deregister();
}

fn handle(link: &MetaLink, req: MetaRequest) -> MetaReply {
    match req {
        MetaRequest::VerifyJoin { tag, join_token } => MetaReply::Joined {
            tag,
            result: link.verify_join(&join_token),
        },
        MetaRequest::LoadAccount { account } => MetaReply::AccountLoaded {
            account,
            result: link.load_account(account),
        },
        MetaRequest::Commit { account, events } => MetaReply::Committed {
            account: account.player.0,
            result: link.commit(&account, &events),
        },
        MetaRequest::Race(r) => MetaReply::RaceReported(link.report_race(&r)),
        MetaRequest::Pursuit(r) => MetaReply::PursuitReported(link.report_pursuit(&r)),
        MetaRequest::Territory(r) => MetaReply::TerritoryReported(link.report_territory(&r)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn hook_is_off_without_url() {
        let none = MetaLinkConfig::from_vars(
            |_| None,
            DamageMode::Arcade,
            RoleCounts::default(),
            "ws://x",
        );
        assert!(none.is_none());
    }

    #[test]
    fn config_from_vars() {
        let vars: BTreeMap<&str, &str> = [
            ("WP_META_URL", "http://meta:8090"),
            ("WP_META_INSTANCE_KEY", "k"),
            ("WP_META_REGION", "eu-west"),
            ("WP_META_BRACKET", "C"),
        ]
        .into();
        let cfg = MetaLinkConfig::from_vars(
            |k| vars.get(k).map(|v| v.to_string()),
            DamageMode::Simulation,
            RoleCounts {
                racer: 8,
                ..Default::default()
            },
            "ws://10.0.0.5:8080",
        )
        .unwrap()
        .unwrap();
        assert_eq!(cfg.url, "http://meta:8090");
        assert_eq!(cfg.register.region, "eu-west");
        assert_eq!(cfg.register.bracket, "C");
        assert_eq!(cfg.register.city, "default");
        assert_eq!(cfg.register.address, "ws://10.0.0.5:8080");
        assert_eq!(cfg.register.mode, DamageMode::Simulation);

        let missing_key = MetaLinkConfig::from_vars(
            |k| (k == "WP_META_URL").then(|| "http://meta".to_string()),
            DamageMode::Arcade,
            RoleCounts::default(),
            "ws://x",
        );
        assert!(matches!(missing_key, Some(Err(_))));
    }

    #[test]
    fn ticks_convert_to_ms() {
        assert_eq!(ticks_to_ms(240), 1000);
        assert_eq!(ticks_to_ms(0), 0);
        assert_eq!(ticks_to_ms(43_200), 180_000);
    }
}
