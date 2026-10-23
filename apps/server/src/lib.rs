//! # wp_server
//!
//! The `wp-server` application (workstream 07): loads data, builds one instance
//! ([`wp_net::server::ServerInstance`] over `wp_sim`, or over the crash arena for load tests),
//! runs it on a 60 Hz game-loop thread, serves websockets, an admin HTTP status endpoint and
//! optional scripted bots. Everything here is native-only.

#![warn(missing_docs)]

pub mod meta;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wp_contracts::{DamageMode, LoadedCar, ModeTable, Role, TimeOfDay};
use wp_net::bot::{random_route, BotDriver};
use wp_net::client::{default_damage_factory, Client, ClientConfig, Phase};
use wp_net::native::{
    connect_ws, serve_ws, spawn_game_loop, spawn_game_loop_with, LogFn, StatusReport,
};
use wp_net::netsim::{lane_spawn_points, server_world_config, CitySetup, WpSim};
use wp_net::server::{ServerConfig, ServerInstance};
use wp_net::testkit::{load_data, ArenaSim, ToyLattice, AI_POLICE_CAR, TRAFFIC_CAR};
use wp_net::Transport;
use wp_sim::SimConfig;

/// Command-line help.
pub const USAGE: &str = "\
usage: wp-server [--config FILE.toml] [--port N] [--admin-port N] [--bind ADDR]
                 [--max-players N] [--city FILE] [--seed N] [--time-ratio X]
                 [--time-of-day H] [--mode arcade|sim|extreme] [--bots N] [--arena RADIUS_M]
                 [--weather KIND] [--data DIR]

  --port         websocket port (default 8080; 0 = any)
  --admin-port   admin HTTP port, GET /status (default 8081; 0 = any)
  --max-players  instance size (default 32)
  --city         city: a .road.json / .road.bin file, `procedural` (12x12 blocks) or
                 `vesper_v2` (Port Vesper v2, terrain + elevation);
                 adds physical traffic and AI police cars, players spawn on its lanes
  --seed         instance seed (default 1)
  --time-ratio   in-game seconds per real second (default 10)
  --time-of-day  start hour (default 21)
  --mode         damage mode (default arcade)
  --bots N       connect N scripted websocket clients (load testing)
  --arena R      crash-arena sim of radius R m (contacts + toy lattice damage) instead of
                 the plain wp_sim world; use with --bots for damage-replication load tests
  --weather      with --city: run server weather starting in KIND (clear, overcast,
                 light_rain, heavy_rain, fog; schedule runs), replicated to clients
  --data         data directory (default data)
A TOML file may set the same keys (snake_case, e.g. admin_port = 9000); flags win.

Meta service (accounts, matchmaking): set WP_META_URL and WP_META_INSTANCE_KEY (optional
WP_META_INSTANCE_NAME, WP_META_REGION, WP_META_CITY, WP_META_BRACKET, WP_META_PUBLIC_ADDRESS).
The server then registers, admits only players with a valid matchmaking join token, heartbeats
its population, carries out kicks and reports pursuits.";

/// Server application configuration (TOML keys = field names).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    /// Websocket port (0 = any free port).
    pub port: u16,
    /// Admin HTTP port (0 = any free port).
    pub admin_port: u16,
    /// Bind address.
    pub bind: String,
    /// Instance size.
    pub max_players: usize,
    /// City: a `.road.json` / `.road.bin` path, `procedural` or `vesper_v2`; `None` = flat
    /// world.
    pub city: Option<String>,
    /// Instance seed.
    pub seed: u64,
    /// In-game seconds per simulated second.
    pub time_ratio: f32,
    /// Start time of day, hours.
    pub time_of_day_h: f32,
    /// `arcade` or `sim`.
    pub mode: String,
    /// Scripted bot clients to connect.
    pub bots: usize,
    /// Crash-arena radius; `None` = plain `wp_sim` world.
    pub arena_radius_m: Option<f32>,
    /// Starting weather for a city instance (`None` = no weather); see [`parse_weather`].
    pub weather: Option<String>,
    /// Data directory.
    pub data_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            admin_port: 8081,
            bind: "0.0.0.0".into(),
            max_players: 32,
            city: None,
            seed: 1,
            time_ratio: 10.0,
            time_of_day_h: 21.0,
            mode: "arcade".into(),
            bots: 0,
            arena_radius_m: None,
            weather: None,
            data_dir: "data".into(),
        }
    }
}

fn parse<T: std::str::FromStr>(flag: &str, v: Option<&String>) -> Result<T, String> {
    v.ok_or_else(|| format!("{flag} needs a value"))?
        .parse()
        .map_err(|_| format!("bad value for {flag}"))
}

impl AppConfig {
    /// Build from command-line arguments (without the program name).
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let mut cfg = match args.iter().position(|a| a == "--config") {
            Some(i) => {
                let path = args.get(i + 1).ok_or("--config needs a path")?;
                let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
                toml::from_str(&text).map_err(|e| format!("{path}: {e}"))?
            }
            None => Self::default(),
        };
        let mut i = 0;
        while i < args.len() {
            let flag = args[i].as_str();
            let v = args.get(i + 1);
            match flag {
                "--config" => {}
                "--port" => cfg.port = parse(flag, v)?,
                "--admin-port" => cfg.admin_port = parse(flag, v)?,
                "--bind" => cfg.bind = parse(flag, v)?,
                "--max-players" => cfg.max_players = parse(flag, v)?,
                "--city" => cfg.city = Some(parse(flag, v)?),
                "--seed" => cfg.seed = parse(flag, v)?,
                "--time-ratio" => cfg.time_ratio = parse(flag, v)?,
                "--time-of-day" => cfg.time_of_day_h = parse(flag, v)?,
                "--mode" => cfg.mode = parse(flag, v)?,
                "--bots" => cfg.bots = parse(flag, v)?,
                "--arena" => cfg.arena_radius_m = Some(parse(flag, v)?),
                "--weather" => cfg.weather = Some(parse(flag, v)?),
                "--data" => cfg.data_dir = parse(flag, v)?,
                other => return Err(format!("unknown flag {other}")),
            }
            i += 2;
        }
        Ok(cfg)
    }

    fn damage_mode(&self) -> Result<DamageMode, String> {
        match self.mode.as_str() {
            "arcade" => Ok(DamageMode::Arcade),
            "sim" | "simulation" => Ok(DamageMode::Simulation),
            "extreme" => Ok(DamageMode::Extreme),
            m => Err(format!("unknown mode {m} (arcade|sim|extreme)")),
        }
    }
}

/// JSON-lines logger on stdout: `{"ts":…,"event":…,<fields>}`.
pub fn json_log() -> LogFn {
    Arc::new(|event: &str, fields: Vec<(&'static str, String)>| {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let mut obj = serde_json::Map::new();
        obj.insert(
            "ts".into(),
            serde_json::json!((ts * 1000.0).round() / 1000.0),
        );
        obj.insert("event".into(), event.into());
        for (k, v) in fields {
            obj.insert(k.into(), v.into());
        }
        println!("{}", serde_json::Value::Object(obj));
    })
}

/// A logger that discards everything (tests).
pub fn null_log() -> LogFn {
    Arc::new(|_: &str, _: Vec<(&'static str, String)>| {})
}

/// Handles of a running server.
pub struct Running {
    /// Websocket address.
    pub ws_addr: SocketAddr,
    /// Admin HTTP address.
    pub admin_addr: SocketAddr,
    /// Live status (updated once per second).
    pub status: Arc<Mutex<StatusReport>>,
}

/// Variable lookup for the meta link (`WP_META_*`); see [`run_with`].
pub type MetaVars = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Start everything; returns once the sockets are bound. Links to the meta service when
/// `WP_META_URL` is set in the environment.
pub async fn run(cfg: AppConfig, log: LogFn) -> Result<Running, String> {
    run_with(cfg, log, Arc::new(|k: &str| std::env::var(k).ok())).await
}

/// [`run`] with an explicit `WP_META_*` variable lookup (tests run meta in-process).
pub async fn run_with(cfg: AppConfig, log: LogFn, meta_vars: MetaVars) -> Result<Running, String> {
    let mode = cfg.damage_mode()?;
    let data_dir = PathBuf::from(&cfg.data_dir);
    let (cars, modes) =
        load_data(&data_dir).map_err(|e| format!("loading {}: {e}", data_dir.display()))?;
    let sim_cfg = SimConfig {
        seed: cfg.seed,
        mode: modes[&mode].clone(),
        time_of_day: TimeOfDay::from_hours(cfg.time_of_day_h),
        time_ratio: cfg.time_ratio,
    };
    let mut server_cfg = ServerConfig {
        max_players: cfg.max_players,
        seed: cfg.seed,
        ..Default::default()
    };
    let weather = cfg.weather.as_deref().map(parse_weather).transpose()?;
    if weather.is_some() && cfg.city.is_none() {
        return Err("--weather needs --city".into());
    }
    let world = match &cfg.city {
        Some(city) => {
            let world = load_city(city, &sim_cfg)?;
            let spawns = lane_spawn_points(&world, cfg.max_players.max(1));
            if spawns.is_empty() {
                return Err(format!("city {city} has no lane long enough to spawn on"));
            }
            log(
                "city_loaded",
                vec![
                    ("city", city.clone()),
                    ("lanes", world.network().lanes.len().to_string()),
                    ("spawn_points", spawns.len().to_string()),
                ],
            );
            server_cfg.spawn_points = spawns;
            Some(world)
        }
        None => None,
    };
    // Bots predict on the city's own ground (cities with terrain).
    let city_ground = world.as_ref().and_then(wp_net::netsim::city_ground);
    // Bots drive around the first spawn point (the origin without a city).
    let bot_center = server_cfg
        .spawn_points
        .first()
        .filter(|_| world.is_some())
        .map_or([0.0, 0.0], |(p, _)| [p[0], p[1]]);

    let ws = TcpListener::bind((cfg.bind.as_str(), cfg.port))
        .await
        .map_err(|e| format!("bind ws {}:{}: {e}", cfg.bind, cfg.port))?;
    let admin = TcpListener::bind((cfg.bind.as_str(), cfg.admin_port))
        .await
        .map_err(|e| format!("bind admin {}:{}: {e}", cfg.bind, cfg.admin_port))?;
    let ws_addr = ws.local_addr().map_err(|e| e.to_string())?;
    let admin_addr = admin.local_addr().map_err(|e| e.to_string())?;

    // Meta link: register now (fails loudly on a bad URL or key), then verify every join.
    let meta = {
        let slots = wp_meta::api::RoleCounts {
            racer: cfg.max_players as u32,
            police_chase: (cfg.max_players as f32 * server_cfg.roles.police_ratio) as u32,
            police_captain: 1,
            spectator: cfg.max_players as u32,
        };
        let address = format!("ws://{ws_addr}");
        let vars = meta_vars.clone();
        match wp_meta::instance::MetaLinkConfig::from_vars(move |k| vars(k), mode, slots, &address)
        {
            None => None,
            Some(c) => {
                let c = c?;
                let bracket = c.register.bracket.clone();
                let worker =
                    tokio::task::spawn_blocking(move || wp_meta::instance::MetaWorker::start(&c))
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| format!("meta registration failed: {e}"))?;
                log(
                    "meta_registered",
                    vec![("instance", worker.instance_id().to_string())],
                );
                server_cfg.require_join_token = true;
                Some((worker, bracket))
            }
        }
    };

    let tap_events = meta.is_some();
    let status = Arc::new(Mutex::new(StatusReport::default()));
    let (tx, rx) = std_mpsc::channel();
    let net_hz = wp_contracts::NET_TICK_HZ;
    if meta.is_some() && cfg.arena_radius_m.is_some() {
        return Err("the meta link needs the real sim (drop --arena)".into());
    }
    match cfg.arena_radius_m {
        Some(radius) => {
            let cars = cars.clone();
            spawn_game_loop(
                move || ServerInstance::new(server_cfg, ArenaSim::new(sim_cfg, cars, radius)),
                rx,
                status.clone(),
                log.clone(),
                net_hz,
            );
        }
        None => {
            let cars = cars.clone();
            spawn_game_loop_with(
                move || {
                    let sim = match world {
                        Some(world) => {
                            let city = CitySetup {
                                world,
                                police_car: cars.get(AI_POLICE_CAR).cloned(),
                                traffic_car: cars.get(TRAFFIC_CAR).cloned(),
                            };
                            let ratio = sim_cfg.time_ratio;
                            let seed = sim_cfg.seed;
                            let mut sim = WpSim::with_city(sim_cfg, cars, city);
                            if let Some(kind) = weather {
                                sim.enable_weather(seed, kind, 0.7, ratio);
                            }
                            sim
                        }
                        None => WpSim::new(sim_cfg, cars),
                    };
                    let mut inst = ServerInstance::new(server_cfg, sim);
                    inst.set_event_tap(tap_events);
                    inst
                },
                rx,
                status.clone(),
                log.clone(),
                net_hz,
                {
                    let mut hook =
                        meta.map(|(w, bracket)| meta::MetaHook::new(w, log.clone(), mode, bracket));
                    move |inst: &mut ServerInstance<WpSim>| {
                        if let Some(h) = hook.as_mut() {
                            h.tick(inst);
                        }
                    }
                },
            );
        }
    }
    tokio::spawn(serve_ws(ws, tx, log.clone()));
    tokio::spawn(serve_admin(admin, status.clone()));
    log(
        "listening",
        vec![
            ("ws", format!("ws://{ws_addr}")),
            ("admin", format!("http://{admin_addr}/status")),
            ("mode", cfg.mode.clone()),
            ("max_players", cfg.max_players.to_string()),
            ("arena", format!("{:?}", cfg.arena_radius_m)),
        ],
    );

    let url = format!("ws://127.0.0.1:{}", ws_addr.port());
    for i in 0..cfg.bots {
        let bot = BotSpec {
            index: i,
            city_ground: city_ground.clone(),
            seed: cfg.seed.wrapping_mul(1000) + i as u64,
            route_center: bot_center,
            route_radius: cfg
                .arena_radius_m
                .map_or(if cfg.city.is_some() { 150.0 } else { 300.0 }, |r| r * 0.8),
            arena: cfg.arena_radius_m.is_some(),
        };
        tokio::spawn(run_bot(
            url.clone(),
            bot,
            cars.clone(),
            modes.clone(),
            log.clone(),
        ));
    }
    Ok(Running {
        ws_addr,
        admin_addr,
        status,
    })
}

/// Parse a weather kind name: `clear`, `overcast`, `light_rain`, `heavy_rain`, `fog`
/// (case and `_` / `-` ignored).
pub fn parse_weather(name: &str) -> Result<wp_contracts::WeatherKind, String> {
    let norm = |s: &str| s.to_lowercase().replace(['_', '-'], "");
    wp_contracts::WeatherKind::ALL
        .into_iter()
        .find(|k| norm(&format!("{k:?}")) == norm(name))
        .ok_or_else(|| format!("unknown weather {name}"))
}

/// Load a city for the server: `procedural` (the 12×12 procedural city), `vesper_v2` (Port
/// Vesper v2: curves, elevation, terrain), `ashpeak` (Port Vesper + Ashpeak County) or a road
/// file.
pub fn load_city(city: &str, sim_cfg: &SimConfig) -> Result<wp_world::World, String> {
    let wcfg = server_world_config(sim_cfg);
    if city == wp_world::vesper::CITY_ID || city == "vesper" {
        return Ok(wp_world::World::vesper(
            &wp_world::vesper::VesperConfig::default(),
            wcfg,
        ));
    }
    if city == wp_world::ashpeak::CITY_ID {
        return Ok(wp_world::World::ashpeak(
            &wp_world::ashpeak::default_config(),
            wcfg,
        ));
    }
    if city == "procedural" {
        let pg = wp_world::procgen::ProcGenConfig::default();
        return Ok(wp_world::World::procedural(&pg, wcfg));
    }
    wp_world::World::load(city, wcfg).map_err(|e| format!("loading city {city}: {e}"))
}

/// Status as JSON (admin endpoint body).
pub fn status_json(s: &StatusReport) -> serde_json::Value {
    serde_json::json!({
        "players": s.stats.players,
        "connected": s.stats.connected,
        "player_list": s.players.iter().map(|(id, name)| serde_json::json!({"id": id, "name": name})).collect::<Vec<_>>(),
        "tick": s.stats.tick,
        "tick_rate_hz": s.tick_rate_hz,
        "ms_per_tick_avg": s.ms_per_tick_avg,
        "ms_per_tick_max": s.ms_per_tick_max,
        "downlink_bps_avg": s.stats.downlink_bps_avg,
        "downlink_bps_max": s.stats.downlink_bps_max,
        "bytes_sent_total": s.stats.bytes_sent_total,
        "inputs_missing": s.stats.inputs_missing,
        "inputs_rejected": s.stats.inputs_rejected,
        "decode_errors": s.stats.decode_errors,
        "lattice_resyncs": s.stats.lattice_resyncs,
        "uptime_s": s.uptime_s,
    })
}

async fn serve_admin(listener: TcpListener, status: Arc<Mutex<StatusReport>>) {
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            continue;
        };
        let status = status.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            let mut len = 0;
            while len < buf.len() {
                match stream.read(&mut buf[len..]).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => len += n,
                }
                if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&buf[..len]);
            let path = request.split_whitespace().nth(1).unwrap_or("");
            let (code, body) = if path == "/status" {
                let s = status.lock().map(|s| s.clone()).unwrap_or_default();
                ("200 OK", status_json(&s).to_string())
            } else {
                ("404 Not Found", "{\"error\":\"try /status\"}".to_string())
            };
            let resp = format!(
                "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes()).await;
            let _ = stream.shutdown().await;
        });
    }
}

struct BotSpec {
    index: usize,
    city_ground: Option<wp_net::predict::CityGround>,
    seed: u64,
    route_center: [f32; 2],
    route_radius: f32,
    arena: bool,
}

async fn run_bot(
    url: String,
    spec: BotSpec,
    cars: BTreeMap<String, LoadedCar>,
    modes: BTreeMap<DamageMode, ModeTable>,
    log: LogFn,
) {
    let mut transport = match connect_ws(&url).await {
        Ok(t) => t,
        Err(e) => {
            log(
                "bot_connect_failed",
                vec![("bot", spec.index.to_string()), ("error", e.to_string())],
            );
            return;
        }
    };
    let mut client = Client::new(ClientConfig {
        name: format!("bot{}", spec.index),
        input_source: wp_contracts::InputSource::Keyboard,
        cars,
        modes,
        damage_factory: if spec.arena {
            ToyLattice::factory
        } else {
            default_damage_factory
        },
        city_ground: spec.city_ground.clone(),
    });
    let mut driver = BotDriver::new(
        spec.seed,
        random_route(spec.seed, spec.route_center, spec.route_radius, 16),
    );
    let start = Instant::now();
    client.connect(0.0, 0);
    let mut requested = false;
    let mut interval = tokio::time::interval(Duration::from_secs_f64(1.0 / 60.0));
    loop {
        interval.tick().await;
        let now = start.elapsed().as_secs_f64() * 1000.0;
        loop {
            match transport.poll() {
                Ok(Some(frame)) => {
                    let _ = client.receive(&frame, now);
                }
                Ok(None) => break,
                Err(_) => return,
            }
        }
        if !requested && *client.phase() == Phase::Joined {
            let role = if spec.index % 3 == 2 {
                Role::PoliceChase
            } else {
                Role::Racer
            };
            client.request_role(role, now);
            requested = true;
        }
        if matches!(client.phase(), Phase::Rejected(_)) {
            return;
        }
        let input = client
            .predicted_state()
            .map(|s| driver.input(&s, 4))
            .unwrap_or_default();
        client.update(now, &input);
        for (channel, bytes) in client.drain_outgoing() {
            if transport.send(channel, &bytes).is_err() {
                return;
            }
        }
    }
}
