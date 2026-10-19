//! Test harness: run the real service on an ephemeral port in a background runtime, driven by
//! a manual clock; plus a fake instance server that speaks the instance API.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use wp_contracts::{DamageMode, Role};
use wp_meta::api::*;
use wp_meta::client::MetaClient;
use wp_meta::clock::ManualClock;
use wp_meta::config::Config;
use wp_meta::AppState;

/// 2026-10-19 12:00 UTC (a Monday).
pub const START_MS: i64 = 20_745 * 86_400_000 + 12 * 3_600_000;

/// A running meta service.
pub struct TestServer {
    pub base: String,
    pub clock: Arc<ManualClock>,
    pub config: Config,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TestServer {
    pub fn start() -> Self {
        Self::start_with(Config::test())
    }

    pub fn start_with(config: Config) -> Self {
        let clock = Arc::new(ManualClock::new(START_MS));
        let state = AppState::new(config.clone(), clock.clone()).expect("state");
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .expect("runtime");
            rt.block_on(wp_meta::serve(
                state,
                move |a| addr_tx.send(a).expect("send addr"),
                async {
                    let _ = stop_rx.await;
                },
            ))
            .expect("serve");
        });
        let addr = addr_rx.recv().expect("server bound");
        Self {
            base: format!("http://{addr}"),
            clock,
            config,
            stop: Some(stop_tx),
            thread: Some(thread),
        }
    }

    /// Anonymous client.
    pub fn client(&self) -> MetaClient {
        MetaClient::new(&self.base)
    }

    /// Admin client.
    pub fn admin(&self) -> MetaClient {
        self.client()
            .with_token(&self.config.admin_token)
            .with_admin_actor("test-admin")
    }

    /// Advance the service clock.
    pub fn advance(&self, ms: i64) {
        self.clock.advance(ms);
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Sign up a player; returns (account id, authenticated client).
pub fn player(srv: &TestServer, name: &str) -> (AccountId, MetaClient) {
    let s = srv
        .client()
        .signup(&format!("{name}@example.test"), "hunter22-password", name)
        .expect("signup");
    (s.account_id, srv.client().with_token(&s.token))
}

/// Default racer prefs.
pub fn prefs(role: Role, mode: DamageMode, bracket: &str) -> MatchPrefs {
    MatchPrefs {
        role,
        mode,
        bracket: bracket.into(),
        region: "eu-west".into(),
        city: None,
    }
}

/// A connected player as the fake instance sees it.
#[derive(Debug, Clone)]
pub struct Seat {
    pub account_id: AccountId,
    pub role: Role,
    pub party_id: Option<u32>,
    pub reconnect_token: String,
}

/// A fake city server: registers, verifies join tokens, heartbeats its population.
pub struct FakeInstance {
    pub id: InstanceId,
    pub client: MetaClient,
    pub seats: Mutex<BTreeMap<AccountId, Seat>>,
}

impl FakeInstance {
    pub fn register(
        srv: &TestServer,
        name: &str,
        mode: DamageMode,
        bracket: &str,
        slots: RoleCounts,
    ) -> Arc<Self> {
        let reg = srv
            .client()
            .with_token(&srv.config.instance_key)
            .register_instance(&RegisterInstanceRequest {
                name: name.into(),
                region: "eu-west".into(),
                city: "harbor_city".into(),
                mode,
                bracket: bracket.into(),
                address: format!("ws://127.0.0.1:9{}", name.len()),
                slots,
            })
            .expect("register");
        Arc::new(Self {
            id: reg.instance_id,
            client: srv.client().with_token(&reg.instance_token),
            seats: Mutex::new(BTreeMap::new()),
        })
    }

    /// A client connects with a join token.
    pub fn connect(&self, join_token: &str) -> Result<Seat, wp_meta::client::ClientError> {
        let v = self.client.verify_join(self.id, join_token)?;
        let seat = Seat {
            account_id: v.account_id,
            role: v.role,
            party_id: v.party_id,
            reconnect_token: v.reconnect_token,
        };
        self.seats
            .lock()
            .unwrap()
            .insert(seat.account_id, seat.clone());
        Ok(seat)
    }

    /// A client drops.
    pub fn disconnect(&self, account: AccountId) {
        self.seats.lock().unwrap().remove(&account);
    }

    pub fn population(&self) -> RoleCounts {
        let mut p = RoleCounts::default();
        for s in self.seats.lock().unwrap().values() {
            *p.get_mut(s.role) += 1;
        }
        p
    }

    pub fn heartbeat(&self) -> HeartbeatResponse {
        let players = self.seats.lock().unwrap().keys().copied().collect();
        self.client
            .heartbeat(
                self.id,
                &HeartbeatRequest {
                    population: self.population(),
                    players,
                    draining: false,
                    tick_ms: 1.5,
                },
            )
            .expect("heartbeat")
    }

    pub fn players_with_role(&self, role: Role) -> Vec<AccountId> {
        self.seats
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.role == role)
            .map(|s| s.account_id)
            .collect()
    }
}

/// Queue (if `queue`) and poll until assigned, then connect to the right instance.
pub fn matchmake(c: &MetaClient, p: Option<&MatchPrefs>, instances: &[Arc<FakeInstance>]) -> Seat {
    if let Some(p) = p {
        c.queue(p).expect("queue");
    }
    for _ in 0..200 {
        let st = c.ticket_status().expect("status");
        if let Some(a) = st.assignment {
            let inst = instances
                .iter()
                .find(|i| i.id == a.instance_id)
                .expect("assigned to a known instance");
            return inst.connect(&a.join_token).expect("verify join");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("never assigned");
}
