//! `cargo run --release -p wp_opponent --example race -- [laps] [racers] [traffic] [reverse_grid] [seed]`
//!
//! Runs AI racers on the arterial square of the 8×8 procedural city in the full sim and
//! prints lap times, contacts and passes. Skill falls from the front of the grid to the back
//! (or rises with `reverse_grid = 1`).

#[path = "../tests/common/mod.rs"]
mod common;

use wp_opponent::DriverProfile;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize, d: f32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let laps = arg(1, 2.0) as u32;
    let n = arg(2, 8.0) as usize;
    let traffic = arg(3, 0.0);
    let reverse = arg(4, 0.0) > 0.5;
    let seed = arg(5, 7.0) as u64;
    let mut drivers: Vec<DriverProfile> = (0..n)
        .map(|i| DriverProfile {
            skill: 0.95 - 0.1 * i as f32,
            ..DriverProfile::default()
        })
        .collect();
    if reverse {
        drivers.reverse();
    }
    let setup = common::Setup {
        seed,
        laps,
        drivers: drivers.clone(),
        traffic,
        closed_roads: traffic == 0.0,
        max_s: 120.0 * laps as f32 + 60.0,
        grid: if std::env::var("WP_SINGLE").is_ok() {
            wp_opponent::GridLayout::SingleFile
        } else {
            wp_opponent::GridLayout::TwoWide
        },
        cars: std::env::var("WP_CARS")
            .map(|v| v.split(',').map(String::from).collect())
            .unwrap_or_default(),
        ..Default::default()
    };
    let t0 = std::time::Instant::now();
    for i in 0..n {
        let id = setup.cars.get(i).unwrap_or(&setup.car);
        let cap = common::capability(&common::car(id), true);
        if i == 0 || setup.cars.get(i).is_some() {
            println!("{id}: {cap:?}");
        }
    }
    println!(
        "capabilities measured in {:.1} s",
        t0.elapsed().as_secs_f32()
    );
    let t1 = std::time::Instant::now();
    let mut last_print = 0u64;
    let dump = std::env::var("WP_DUMP").ok();
    let mut traj = String::from("t,entity,x,y,tactic,v,vt,limit,s,n,ncmd\n");
    let out = common::run(&setup, &wp_opponent::NoPolice, &mut |sim, field| {
        if dump.is_some() && sim.tick() % 24 == 0 {
            for (k, (e, s)) in sim.vehicle_states().iter().enumerate() {
                let d = field.debug_of(k);
                traj += &format!(
                    "{:.2},{},{:.2},{:.2},{:?},{:.1},{:.1},{:?},{:.1},{:.2},{:.2}\n",
                    sim.tick() as f32 / 240.0,
                    e.0,
                    s.pos[0],
                    s.pos[1],
                    field.tactic_of(k),
                    s.speed_mps,
                    d.v_target,
                    d.limit,
                    d.s,
                    d.n,
                    d.n_cmd
                )
                .replace("Obstacle(", "Obs")
                .replace("BackOut(", "Back")
                .replace(')', "");
            }
            let states = sim.vehicle_states();
            for a in sim.world().agents() {
                if states.iter().any(|(_, s)| {
                    let dx = s.pos[0] - a.pos[0];
                    let dy = s.pos[1] - a.pos[1];
                    dx * dx + dy * dy < 900.0
                }) {
                    traj += &format!(
                        "{:.2},c{},{:.2},{:.2},Civ,{:.1},{:.3},yaw,0,0,0\n",
                        sim.tick() as f32 / 240.0,
                        a.id,
                        a.pos[0],
                        a.pos[1],
                        a.speed_mps,
                        a.yaw
                    );
                }
            }
        }
        if sim.tick() >= last_print + 240 * 10 {
            last_print = sim.tick();
            let st = field.standings();
            let s: Vec<String> = st
                .iter()
                .map(|x| format!("{}:{:.0}", x.entity.0, x.progress_m))
                .collect();
            let tac: Vec<String> = (0..drivers.len())
                .map(|k| format!("{:?}", field.tactic_of(k)))
                .collect();
            println!(
                "t={:.0}s {} | {}",
                sim.tick() as f32 / 240.0,
                s.join(" "),
                tac.join(",")
            );
        }
    });
    let wall = t1.elapsed().as_secs_f32();
    println!(
        "sim {:.1} s in {:.1} s wall ({:.3} ms/tick)",
        out.sim_s,
        wall,
        wall * 1000.0 / (out.sim_s * 240.0)
    );
    let frames = out.sim_s as f64 * 60.0;
    println!(
        "AI (Field::tick): {:.4} ms per 60 Hz frame for {n} racers ({:.4} ms/racer)",
        out.field_wall_s * 1000.0 / frames,
        out.field_wall_s * 1000.0 / frames / n as f64
    );
    let line = out.field.line_of(0);
    println!(
        "corridor {:.0} m, line {:.0} m, max curvature {:.3}, predicted lap (racer 0) {:.1} s",
        out.field.corridor().length_m,
        line.length_m(true),
        line.max_curvature(),
        out.field.race_profile_of(0).lap_time_s(line, true)
    );
    for (k, s) in out.field.standings().iter().enumerate() {
        let gi = out.entities.iter().position(|e| *e == s.entity).unwrap();
        let best = s.lap_times_s.iter().cloned().fold(f32::INFINITY, f32::min);
        println!(
            "P{} entity {} skill {:.2} {:?} laps {} best {:.2} laps {:?} passes {}",
            k + 1,
            s.entity.0,
            drivers[gi].skill,
            s.status,
            s.laps,
            best,
            s.lap_times_s
                .iter()
                .map(|t| format!("{t:.1}"))
                .collect::<Vec<_>>(),
            out.field.passes_by(gi)
        );
        println!("    pass checks {:?}", out.field.debug_of(gi).pass_checks);
    }
    let mut field = out.field.clone();
    for ev in field.drain_events() {
        if !matches!(ev, wp_opponent::OpponentEvent::LapCompleted { .. }) {
            println!("event {ev:?}");
        }
    }
    let big: Vec<_> = out
        .contacts
        .iter()
        .filter(|c| c.3 >= wp_opponent::AI_CONTACT_IMPULSE_NS)
        .collect();
    println!(
        "racer contacts: {} events (>= {} N·s: {}), max {:.0} N·s; other contacts {}",
        out.contacts.len(),
        wp_opponent::AI_CONTACT_IMPULSE_NS,
        big.len(),
        out.contacts.iter().map(|c| c.3).fold(0.0, f32::max),
        out.other_contacts
    );
    for c in big.iter().take(10) {
        println!(
            "  tick {} ({:.1}s) {}-{} {:.0}",
            c.0,
            c.0 as f32 / 240.0,
            c.1 .0,
            c.2 .0,
            c.3
        );
    }
    if let Some(dir) = &dump {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(format!("{dir}/traj.csv"), &traj).unwrap();
        let c = out.field.corridor();
        let line = out.field.line_of(0);
        let prof = out.field.speed_profile_of(0);
        let mut s = String::from("i,cx,cy,lx,ly,rx,ry,nx,ny,v,k,junction\n");
        for (i, smp) in c.samples.iter().enumerate() {
            let l = smp.at(-smp.left_m);
            let r = smp.at(smp.right_m);
            let n = smp.at(line.offset[i]);
            s += &format!(
                "{i},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.4},{}\n",
                smp.pos[0],
                smp.pos[1],
                l[0],
                l[1],
                r[0],
                r[1],
                n[0],
                n[1],
                prof.v[i],
                line.curvature[i],
                smp.junction as u8
            );
        }
        std::fs::write(format!("{dir}/corridor.csv"), s).unwrap();
        let w = common::world(&setup);
        let mut s = String::from("lane,kind,x,y\n");
        for (i, l) in w.network().lanes.iter().enumerate() {
            for p in &l.polyline {
                s += &format!("{i},{:?},{:.1},{:.1}\n", l.kind, p[0], p[1]);
            }
        }
        std::fs::write(format!("{dir}/lanes.csv"), s).unwrap();
        let mut s = String::from("x,y,hx,hy\n");
        for p in w.static_props() {
            s += &format!(
                "{:.1},{:.1},{:.1},{:.1}\n",
                p.pos[0], p.pos[1], p.half_extents[0], p.half_extents[1]
            );
        }
        std::fs::write(format!("{dir}/props.csv"), s).unwrap();
    }
    println!(
        "hash sim {:016x} field {:016x}",
        out.sim_hash, out.field_hash
    );
}
