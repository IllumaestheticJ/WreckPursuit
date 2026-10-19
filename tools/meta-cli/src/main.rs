//! `meta-cli`: command-line client for the meta service (`apps/meta`).
//!
//! ```text
//! meta-cli --url http://127.0.0.1:8090 signup a@b.c 'password123' Racer1
//! WP_META_TOKEN=wps_... meta-cli me
//! WP_META_TOKEN=<admin token> meta-cli admin health
//! meta-cli smoke --instance-key <key> --bots 20      # end-to-end check of a deployment
//! ```

use clap::{Parser, Subcommand};
use serde_json::Value;
use wp_contracts::{DamageMode, Role};
use wp_meta::api::*;
use wp_meta::client::{ClientResult, MetaClient};

#[derive(Parser)]
#[command(name = "meta-cli", about = "WreckPursuit meta service client")]
struct Cli {
    /// Service base URL.
    #[arg(long, env = "WP_META_URL", default_value = "http://127.0.0.1:8090")]
    url: String,
    /// Bearer token (player session, instance token or admin token).
    #[arg(long, env = "WP_META_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// Name recorded in the audit log for admin commands.
    #[arg(long, env = "WP_META_ACTOR", default_value = "meta-cli")]
    actor: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create an account; prints the session token.
    Signup {
        email: String,
        password: String,
        display_name: String,
    },
    /// Log in; prints the session token.
    Login { email: String, password: String },
    /// Own profile (player token).
    Me,
    /// Own economy account (player token).
    Economy,
    /// Live instances.
    Instances,
    /// Feature flags.
    Flags,
    /// A leaderboard.
    Leaderboard {
        board: String,
        #[arg(long, default_value = "all")]
        period: String,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Queue for a match (player token).
    Queue {
        #[arg(long, default_value = "racer")]
        role: String,
        #[arg(long, default_value = "arcade")]
        mode: String,
        #[arg(long, default_value = "open")]
        bracket: String,
        #[arg(long, default_value = "eu-west")]
        region: String,
        #[arg(long)]
        city: Option<String>,
    },
    /// Ticket status (player token).
    Status,
    /// Leave the queue (player token).
    Cancel,
    /// Admin commands (admin token).
    #[command(subcommand)]
    Admin(AdminCmd),
    /// End-to-end smoke test: registers a temporary instance and runs bots through signup →
    /// matchmake → join → race report → leaderboard, then deregisters.
    Smoke {
        /// Instance registration key (`WP_META_INSTANCE_KEY` of the service).
        #[arg(long, env = "WP_META_INSTANCE_KEY", hide_env_values = true)]
        instance_key: String,
        #[arg(long, default_value_t = 8)]
        bots: u32,
    },
}

#[derive(Subcommand)]
enum AdminCmd {
    /// Service + instance health.
    Health,
    /// Account view.
    Account { id: AccountId },
    /// Ban (permanent unless --duration-s).
    Ban {
        id: AccountId,
        reason: String,
        #[arg(long)]
        duration_s: Option<i64>,
    },
    /// Lift a ban.
    Unban { id: AccountId },
    /// Kick from the current instance.
    Kick { id: AccountId, reason: String },
    /// Audited cash adjustment.
    Adjust {
        id: AccountId,
        #[arg(allow_hyphen_values = true)]
        cash: i64,
        note: String,
    },
    /// Set a flag to a JSON value, e.g. `flag-set illegal_day_race_frequency 0.2`.
    FlagSet { key: String, value: String },
    /// Delete a flag.
    FlagDelete { key: String },
    /// Audit log.
    Audit {
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Reset the current period of a board.
    ResetBoard { board: String, period: String },
}

fn print(v: impl serde::Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(&v).unwrap_or_else(|e| format!("<unprintable: {e}>"))
    );
}

fn parse_role(s: &str) -> Result<Role, String> {
    wp_meta::api::parse_role(s)
        .ok_or_else(|| format!("unknown role {s} (racer, police_chase, police_captain, spectator)"))
}

fn parse_mode(s: &str) -> Result<DamageMode, String> {
    wp_meta::api::parse_mode(s).ok_or_else(|| format!("unknown mode {s} (arcade, simulation)"))
}

fn run(cli: Cli) -> Result<(), String> {
    let base = MetaClient::new(&cli.url);
    let c = match &cli.token {
        Some(t) => base.with_token(t),
        None => base.clone(),
    }
    .with_admin_actor(&cli.actor);
    let e = |r: ClientResult<()>| r.map_err(|e| e.to_string());
    match cli.cmd {
        Cmd::Signup {
            email,
            password,
            display_name,
        } => print(
            c.signup(&email, &password, &display_name)
                .map_err(|e| e.to_string())?,
        ),
        Cmd::Login { email, password } => {
            print(c.login(&email, &password).map_err(|e| e.to_string())?)
        }
        Cmd::Me => print(c.me().map_err(|e| e.to_string())?),
        Cmd::Economy => print(c.my_economy().map_err(|e| e.to_string())?),
        Cmd::Instances => print(c.instances().map_err(|e| e.to_string())?),
        Cmd::Flags => print(c.flags().map_err(|e| e.to_string())?),
        Cmd::Leaderboard {
            board,
            period,
            limit,
        } => print(
            c.leaderboard(&board, &period, limit)
                .map_err(|e| e.to_string())?,
        ),
        Cmd::Queue {
            role,
            mode,
            bracket,
            region,
            city,
        } => print(
            c.queue(&MatchPrefs {
                role: parse_role(&role)?,
                mode: parse_mode(&mode)?,
                bracket,
                region,
                city,
            })
            .map_err(|e| e.to_string())?,
        ),
        Cmd::Status => print(c.ticket_status().map_err(|e| e.to_string())?),
        Cmd::Cancel => e(c.cancel_queue())?,
        Cmd::Admin(a) => match a {
            AdminCmd::Health => print(c.admin_health().map_err(|e| e.to_string())?),
            AdminCmd::Account { id } => print(c.admin_account(id).map_err(|e| e.to_string())?),
            AdminCmd::Ban {
                id,
                reason,
                duration_s,
            } => print(
                c.admin_ban(id, &reason, duration_s)
                    .map_err(|e| e.to_string())?,
            ),
            AdminCmd::Unban { id } => print(c.admin_unban(id).map_err(|e| e.to_string())?),
            AdminCmd::Kick { id, reason } => e(c.admin_kick(id, &reason))?,
            AdminCmd::Adjust { id, cash, note } => {
                print(c.admin_adjust(id, cash, &note).map_err(|e| e.to_string())?)
            }
            AdminCmd::FlagSet { key, value } => {
                let v: Value = serde_json::from_str(&value)
                    .map_err(|err| format!("value must be JSON ({err}); quote strings: '\"x\"'"))?;
                e(c.admin_set_flag(&key, v))?
            }
            AdminCmd::FlagDelete { key } => e(c.admin_delete_flag(&key))?,
            AdminCmd::Audit { limit } => print(c.admin_audit(limit).map_err(|e| e.to_string())?),
            AdminCmd::ResetBoard { board, period } => print(
                c.admin_reset_board(&board, &period)
                    .map_err(|e| e.to_string())?,
            ),
        },
        Cmd::Smoke { instance_key, bots } => smoke(&base, &instance_key, bots)?,
    }
    Ok(())
}

/// Turn a client error into a message prefixed with what was being done.
fn err(what: &'static str) -> impl Fn(wp_meta::client::ClientError) -> String {
    move |e| format!("{what}: {e}")
}

fn smoke(base: &MetaClient, instance_key: &str, bots: u32) -> Result<(), String> {
    let reg = base
        .with_token(instance_key)
        .register_instance(&RegisterInstanceRequest {
            name: "meta-cli-smoke".into(),
            region: "smoke".into(),
            city: "smoke_city".into(),
            mode: DamageMode::Arcade,
            bracket: "open".into(),
            address: "ws://smoke.invalid".into(),
            slots: RoleCounts {
                racer: bots,
                ..Default::default()
            },
        })
        .map_err(err("register instance"))?;
    let inst = base.with_token(&reg.instance_token);
    let run_id = format!("{:x}", std::process::id());
    let mut entries = Vec::new();
    let mut players = Vec::new();
    for i in 0..bots {
        let name = format!("smk{run_id}_{i}");
        let s = base
            .signup(&format!("{name}@smoke.invalid"), "smoke-password-1", &name)
            .map_err(err("signup"))?;
        let p = base.with_token(&s.token);
        let st = p
            .queue(&MatchPrefs {
                role: Role::Racer,
                mode: DamageMode::Arcade,
                bracket: "C".into(),
                region: "smoke".into(),
                city: None,
            })
            .map_err(err("queue"))?;
        let a = st
            .assignment
            .ok_or_else(|| format!("bot {i} not assigned: {:?}", st.state))?;
        if a.instance_id != reg.instance_id {
            return Err(format!(
                "bot {i} assigned to another instance {}",
                a.instance_id
            ));
        }
        let v = inst
            .verify_join(reg.instance_id, &a.join_token)
            .map_err(err("verify join"))?;
        players.push(v.account_id);
        entries.push(RaceEntry {
            account_id: v.account_id,
            position: i + 1,
            finish_time_ms: Some(120_000 + u64::from(i) * 1_000),
            clean: true,
        });
    }
    inst.heartbeat(
        reg.instance_id,
        &HeartbeatRequest {
            population: RoleCounts {
                racer: bots,
                ..Default::default()
            },
            players: players.clone(),
            draining: false,
            tick_ms: 0.0,
        },
    )
    .map_err(err("heartbeat"))?;
    let r = inst
        .report_race(
            reg.instance_id,
            &RaceReport {
                report_key: format!("smoke-{run_id}"),
                route_id: "smoke_route".into(),
                race_type: "sprint".into(),
                mode: DamageMode::Arcade,
                bracket: "C".into(),
                entries,
            },
        )
        .map_err(err("report race"))?;
    let board = base
        .leaderboard("clean_rating", "daily", 200)
        .map_err(err("leaderboard"))?;
    let on_board = board
        .rows
        .iter()
        .filter(|r| players.contains(&r.account_id))
        .count();
    inst.deregister(reg.instance_id)
        .map_err(err("deregister"))?;
    println!(
        "smoke ok: {bots} bots, race {} rated {} players, {on_board} on today's clean_rating board",
        r.race_id,
        r.ratings.len()
    );
    Ok(())
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("meta-cli: {e}");
        std::process::exit(1);
    }
}
