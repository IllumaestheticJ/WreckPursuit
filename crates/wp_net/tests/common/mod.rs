//! In-process harness: one `ServerInstance` (the crash arena by default, or any `NetSim`, e.g.
//! the real `wp_sim` city) and N clients connected through simulated links, all driven by one
//! virtual clock in 1/60 s steps.

#![allow(dead_code)]

use std::collections::BTreeMap;

use wp_contracts::{DamageMode, LoadedCar, ModeTable, Role, TimeOfDay, VehicleInput};
use wp_net::bot::{random_route, BotDriver};
use wp_net::client::{Client, ClientConfig, DamageFactory, Phase};
use wp_net::netsim::NetSim;
use wp_net::server::{ConnId, ServerConfig, ServerInstance};
use wp_net::sim_link::{LinkConfig, Pipe};
use wp_net::testkit::{load_data, repo_data_dir, ArenaSim, ToyLattice};
use wp_sim::SimConfig;

pub const STEP_MS: f64 = 1000.0 / 60.0;

pub struct Peer {
    pub client: Client,
    pub conn: Option<ConnId>,
    pub up: Pipe,
    pub down: Pipe,
    pub bot: Option<BotDriver>,
    pub manual_input: VehicleInput,
    pub online: bool,
    /// Down-link bytes delivered per 1 s window (filled by `Harness::step`).
    pub down_per_second: Vec<u64>,
    window_bytes: u64,
}

pub struct Harness<S: NetSim = ArenaSim> {
    pub server: ServerInstance<S>,
    pub peers: Vec<Peer>,
    pub now_ms: f64,
    pub steps: u64,
    pub cars: BTreeMap<String, LoadedCar>,
    pub modes: BTreeMap<DamageMode, ModeTable>,
    pub factory: DamageFactory,
    /// Wall time of each `server.net_tick()` in `step`, ms (test measurement only).
    pub server_ms: Vec<f64>,
}

pub fn data() -> (BTreeMap<String, LoadedCar>, BTreeMap<DamageMode, ModeTable>) {
    load_data(&repo_data_dir()).expect("repo data loads")
}

impl Harness<ArenaSim> {
    pub fn new(cfg: ServerConfig, arena_radius_m: f32) -> Self {
        let (cars, modes) = data();
        let sim_cfg = SimConfig {
            seed: 7,
            mode: modes[&DamageMode::Arcade].clone(),
            time_of_day: TimeOfDay::from_hours(21.0),
            time_ratio: 10.0,
        };
        let sim = ArenaSim::new(sim_cfg, cars.clone(), arena_radius_m);
        Self {
            server: ServerInstance::new(cfg, sim),
            peers: Vec::new(),
            now_ms: 0.0,
            steps: 0,
            cars,
            modes,
            factory: ToyLattice::factory,
            server_ms: Vec::new(),
        }
    }
}

impl<S: NetSim> Harness<S> {
    /// Harness over any sim; clients replicate damage with `factory`.
    pub fn with_sim(
        cfg: ServerConfig,
        sim: S,
        cars: BTreeMap<String, LoadedCar>,
        modes: BTreeMap<DamageMode, ModeTable>,
        factory: DamageFactory,
    ) -> Self {
        Self {
            server: ServerInstance::new(cfg, sim),
            peers: Vec::new(),
            now_ms: 0.0,
            steps: 0,
            cars,
            modes,
            factory,
            server_ms: Vec::new(),
        }
    }

    /// Add a client over links `up`/`down`; it says Hello immediately.
    pub fn add_peer(&mut self, name: &str, up: LinkConfig, down: LinkConfig) -> usize {
        let client = Client::new(ClientConfig {
            name: name.into(),
            input_source: wp_contracts::InputSource::Keyboard,
            cars: self.cars.clone(),
            modes: self.modes.clone(),
            damage_factory: self.factory,
        });
        self.peers.push(Peer {
            client,
            conn: None,
            up: Pipe::new(up),
            down: Pipe::new(down),
            bot: None,
            manual_input: VehicleInput::default(),
            online: false,
            down_per_second: Vec::new(),
            window_bytes: 0,
        });
        let i = self.peers.len() - 1;
        self.connect(i, 0);
        i
    }

    /// [`Harness::add_peer`] presenting a meta join token in `Hello` (MC-6).
    pub fn add_peer_with_join_token(
        &mut self,
        name: &str,
        join_token: &str,
        up: LinkConfig,
        down: LinkConfig,
    ) -> usize {
        let i = self.add_peer(name, up, down);
        // Replace the plain Hello queued by `add_peer` with one carrying the token.
        let p = &mut self.peers[i];
        p.client.drain_outgoing();
        p.client.connect_with_join_token(self.now_ms, 0, join_token);
        i
    }

    /// (Re)connect peer `i` with a reconnect token.
    pub fn connect(&mut self, i: usize, token: u64) {
        let conn = self.server.connect();
        let p = &mut self.peers[i];
        p.conn = Some(conn);
        p.online = true;
        p.client.connect(self.now_ms, token);
    }

    /// Drop peer `i`'s transport (no Bye).
    pub fn drop_link(&mut self, i: usize) {
        if let Some(c) = self.peers[i].conn.take() {
            self.server.disconnect(c);
        }
        self.peers[i].online = false;
    }

    pub fn give_bot(&mut self, i: usize, seed: u64, radius: f32) {
        self.give_bot_at(i, seed, [0.0, 0.0], radius);
    }

    pub fn give_bot_at(&mut self, i: usize, seed: u64, center: [f32; 2], radius: f32) {
        let mut bot = BotDriver::new(seed, random_route(seed, center, radius, 16));
        bot.crash_chance_per_s = 0.08;
        self.peers[i].bot = Some(bot);
    }

    pub fn request_role(&mut self, i: usize, role: Role) {
        let now = self.now_ms;
        self.peers[i].client.request_role(role, now);
    }

    /// Advance one 1/60 s step: deliver uplink, server net tick, deliver downlink, client
    /// updates.
    pub fn step(&mut self) {
        self.now_ms += STEP_MS;
        let now = self.now_ms;
        for p in &mut self.peers {
            let frames = p.up.recv(now);
            if let (true, Some(conn)) = (p.online, p.conn) {
                for f in frames {
                    self.server.receive(conn, &f);
                }
            }
        }
        let t0 = std::time::Instant::now();
        self.server.net_tick();
        self.server_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        for out in self.server.drain_outgoing() {
            if let Some(p) = self.peers.iter_mut().find(|p| p.conn == Some(out.conn)) {
                p.down.send(now, out.channel, out.bytes);
            }
        }
        self.steps += 1;
        let second_boundary = self.steps.is_multiple_of(60);
        for p in &mut self.peers {
            if !p.online {
                let _ = p.down.recv(now);
                continue;
            }
            for f in p.down.recv(now) {
                p.window_bytes += f.len() as u64;
                p.client.receive(&f, now).expect("server frames decode");
            }
            if second_boundary {
                p.down_per_second.push(p.window_bytes);
                p.window_bytes = 0;
            }
            let input = match (&mut p.bot, p.client.predicted_state()) {
                (Some(bot), Some(s)) => bot.input(&s, 4),
                _ => p.manual_input,
            };
            p.client.update(now, &input);
            for (ch, bytes) in p.client.drain_outgoing() {
                p.up.send(now, ch, bytes);
            }
        }
    }

    pub fn run_secs(&mut self, secs: f64) {
        let n = (secs * 60.0).round() as u64;
        for _ in 0..n {
            self.step();
        }
    }

    /// Step until every online peer is joined.
    pub fn wait_joined(&mut self) {
        for _ in 0..600 {
            if self
                .peers
                .iter()
                .filter(|p| p.online)
                .all(|p| *p.client.phase() == Phase::Joined)
            {
                return;
            }
            self.step();
        }
        panic!("peers did not join");
    }
}
