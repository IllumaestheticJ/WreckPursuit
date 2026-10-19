//! End-to-end weather tests: determinism, bounds, drying, schedule statistics, hooks.

use wp_contracts::{SignalState, SurfaceType, TimeOfDay, WorldQuery, PHYSICS_DT};
use wp_weather::{
    GridSpec, Regime, Season, WeatherConfig, WeatherData, WeatherKind, WeatherSim, WithWeather,
};

fn grid() -> GridSpec {
    GridSpec::covering([-200.0, -200.0], [200.0, 200.0], 8.0)
}

fn sim(seed: u64, cfg: impl FnOnce(&mut WeatherConfig)) -> WeatherSim {
    sim_on(grid(), seed, cfg)
}

fn sim_on(g: GridSpec, seed: u64, cfg: impl FnOnce(&mut WeatherConfig)) -> WeatherSim {
    let mut c = WeatherConfig {
        seed,
        ..WeatherConfig::default()
    };
    cfg(&mut c);
    WeatherSim::uniform(WeatherData::embedded(), c, g, SurfaceType::Asphalt)
}

/// Some moving vehicles on a ring, deterministic.
fn vehicles(tick: u32) -> Vec<[f32; 3]> {
    (0..24)
        .map(|i| {
            let a = i as f32 * 0.26 + tick as f32 * 1e-3;
            let (s, c) = wp_math::sin_cos(a);
            [120.0 * c, 120.0 * s, 0.0]
        })
        .collect()
}

/// Run `hours` of in-game time in 1-minute updates.
fn run_hours(w: &mut WeatherSim, start_h: f32, hours: f32, mut check: impl FnMut(&WeatherSim)) {
    let steps = (hours * 60.0) as u32;
    for i in 0..steps {
        let t = TimeOfDay::from_hours(start_h + i as f32 / 60.0);
        w.update(60.0, t, &vehicles(i));
        check(w);
    }
}

#[test]
fn same_seed_same_weather_different_seed_different_weather() {
    let run = |seed| {
        let mut w = sim(seed, |_| {});
        let mut hashes = Vec::new();
        run_hours(&mut w, 0.0, 72.0, |w| hashes.push(w.state_hash()));
        hashes
    };
    let a = run(7);
    assert_eq!(a, run(7));
    assert_ne!(a, run(8));
}

#[test]
fn per_tick_stepping_is_deterministic() {
    let run = || {
        let mut w = sim(3, |c| c.start = Regime::new(WeatherKind::HeavyRain, 0.8));
        let mut t = TimeOfDay::from_hours(21.0);
        for tick in 0..240 * 20 {
            t = t.advanced(PHYSICS_DT * 10.0);
            w.step(PHYSICS_DT, t, 10.0, &vehicles(tick));
        }
        w.state_hash()
    };
    assert_eq!(run(), run());
}

#[test]
fn wetness_and_water_stay_in_bounds() {
    let d = WeatherData::embedded();
    for seed in 1..=12 {
        let mut w = sim(seed, |c| c.season = Season::ALL[seed as usize % 4]);
        run_hours(&mut w, seed as f32, 48.0, |w| {
            let g = &w.state().wetness_by_cell;
            for (wm, cap) in g.water_mm().iter().zip(g.capacity_mm()) {
                assert!(*wm >= 0.0 && *wm <= *cap + 1e-6, "{wm} > {cap}");
                assert!(*cap <= d.surfaces.max_water_mm);
            }
            let st = w.state();
            assert!((0.0..=1.0).contains(&st.mean_road_wetness));
            assert!((0.0..=1.0).contains(&st.intensity));
            assert!((0.0..=1.0).contains(&st.blend));
            for i in [0, 100, 2000] {
                let xy = g.spec().cell_center(i);
                let c = w.conditions_at([xy[0], xy[1], 0.0]);
                assert!((0.0..=1.0).contains(&c.wetness));
                assert!(c.standing_water_mm >= 0.0);
                assert!((0.0..=1.0).contains(&c.sight_multiplier));
            }
        });
    }
}

#[test]
fn drying_is_monotone_once_rain_stops() {
    for season in Season::ALL {
        let mut w = sim(11, |c| {
            c.season = season;
            c.locked = true;
            c.start = Regime::new(WeatherKind::HeavyRain, 1.0);
        });
        run_hours(&mut w, 18.0, 1.0, |_| {});
        assert!(w.state().mean_road_wetness > 0.99);
        // Rain stops immediately and the schedule is released: later rolls may bring rain back,
        // but while the rain rate is zero no cell may gain water.
        w.force(Regime::new(WeatherKind::Clear, 0.8), true);
        w.set_locked(false);
        let mut prev = w.state().wetness_by_cell.water_mm().to_vec();
        run_hours(&mut w, 19.0, 12.0, |w| {
            let now = w.state().wetness_by_cell.water_mm();
            if w.state().rain_mm_per_h == 0.0 {
                for (a, b) in now.iter().zip(&prev) {
                    assert!(a <= b, "{season:?}: water rose without rain: {b} -> {a}");
                }
            }
            prev = now.to_vec();
        });
    }
}

#[test]
fn roads_dry_within_hours_and_puddles_last_longer() {
    for season in [Season::Spring, Season::Summer, Season::Autumn] {
        let mut w = sim(11, |c| {
            c.season = season;
            c.locked = true;
            c.start = Regime::new(WeatherKind::Clear, 0.8);
            c.start_water_mm = 10.0;
        });
        let film_cap = w
            .data()
            .surfaces
            .surface(SurfaceType::Asphalt)
            .film_capacity_mm;
        let normal: Vec<usize> = (0..grid().len())
            .filter(|i| w.state().wetness_by_cell.capacity_mm()[*i] <= film_cap)
            .collect();
        let puddle: Vec<usize> = (0..grid().len())
            .filter(|i| w.state().wetness_by_cell.capacity_mm()[*i] > film_cap + 2.0)
            .collect();
        assert!(!puddle.is_empty());
        run_hours(&mut w, 10.0, 1.5, |_| {});
        let g = &w.state().wetness_by_cell;
        assert!(
            puddle.iter().any(|i| g.water_at_cell(*i) > 1.0),
            "{season:?}"
        );
        run_hours(&mut w, 11.5, 1.5, |_| {});
        let g = &w.state().wetness_by_cell;
        assert!(
            normal.iter().all(|i| g.water_at_cell(*i) == 0.0),
            "{season:?}"
        );
        run_hours(&mut w, 13.0, 21.0, |_| {});
        assert!(w.state().mean_road_wetness < 0.01, "{season:?}");
    }
}

#[test]
fn heavy_rain_soaks_and_makes_puddles_light_rain_is_damp() {
    let mut heavy = sim(5, |c| {
        c.locked = true;
        c.start = Regime::new(WeatherKind::HeavyRain, 1.0);
    });
    run_hours(&mut heavy, 14.0, 0.5, |_| {});
    let puddles = heavy.puddle_cells(1.0);
    assert!(!puddles.is_empty());
    assert!(puddles.iter().all(|p| p.depth_mm >= 1.0));

    let mut light = sim(5, |c| {
        c.locked = true;
        c.start = Regime::new(WeatherKind::LightRain, 0.0);
    });
    run_hours(&mut light, 14.0, 0.5, |_| {});
    let m = light.state().mean_road_wetness;
    assert!(m > 0.2, "{m}");
    assert!(light.puddle_cells(1.0).len() < puddles.len());
}

#[test]
fn schedule_visits_every_regime_and_fog_favours_dawn() {
    let tiny = GridSpec::covering([0.0, 0.0], [16.0, 16.0], 8.0);
    let mut w = sim_on(tiny, 42, |c| c.season = Season::Autumn);
    let mut count = [0u32; 5];
    let mut fog_dawn = 0u32;
    let mut fog_day = 0u32;
    let days = 60.0;
    run_hours(&mut w, 0.0, 24.0 * days, |w| {
        count[w.state().kind.code() as usize] += 1;
    });
    // Re-run counting fog by phase (dawn has 2 h/day, day 11 h/day: compare per hour).
    let tiny = GridSpec::covering([0.0, 0.0], [16.0, 16.0], 8.0);
    let mut w = sim_on(tiny, 42, |c| c.season = Season::Autumn);
    let steps = (24.0 * days * 60.0) as u32;
    for i in 0..steps {
        let h = (i as f32 / 60.0) % 24.0;
        let t = TimeOfDay::from_hours(h);
        w.update(60.0, t, &[]);
        if w.state().kind == WeatherKind::Fog {
            match t.phase() {
                wp_contracts::DayPhase::Dawn => fog_dawn += 1,
                wp_contracts::DayPhase::Day => fog_day += 1,
                _ => {}
            }
        }
    }
    for (k, n) in WeatherKind::ALL.iter().zip(count) {
        assert!(n > 0, "{k:?} never happened in {days} days");
    }
    let per_hour_dawn = fog_dawn as f32 / 2.0;
    let per_hour_day = fog_day as f32 / 11.0;
    assert!(
        per_hour_dawn > per_hour_day,
        "{per_hour_dawn} vs {per_hour_day}"
    );
    // Clear is the most common Autumn regime... or at least not rarer than heavy rain.
    assert!(count[0] > count[3]);
}

#[test]
fn locked_schedule_never_changes() {
    let mut w = sim(9, |c| {
        c.locked = true;
        c.start = Regime::new(WeatherKind::Fog, 0.7);
    });
    run_hours(&mut w, 0.0, 48.0, |w| {
        assert_eq!(w.state().kind, WeatherKind::Fog)
    });
    assert!(w.drain_changes().is_empty());
}

#[test]
fn transitions_blend_smoothly() {
    let mut w = sim(1, |c| c.locked = true);
    let t = TimeOfDay::from_hours(12.0);
    w.update(1.0, t, &[]);
    w.force(Regime::new(WeatherKind::HeavyRain, 1.0), false);
    let mut prev = w.state().rain_mm_per_h;
    assert_eq!(prev, 0.0);
    for _ in 0..20 {
        w.update(60.0, t, &[]);
        let r = w.state().rain_mm_per_h;
        assert!(r >= prev && r - prev <= 16.0 * 60.0 / 600.0 + 1e-3);
        prev = r;
    }
    assert!((prev - 16.0).abs() < 1e-3);
    let ch = w.drain_changes();
    assert_eq!(ch.len(), 1);
    assert!(ch[0].forced);
}

#[test]
fn winter_night_wet_road_freezes() {
    let mut w = sim(2, |c| {
        c.season = Season::Winter;
        c.locked = true;
        c.start = Regime::new(WeatherKind::Clear, 1.0);
        c.start_water_mm = 0.8;
    });
    // 03:00, clear sky: coldest point of the day plus radiative cooling.
    w.update(1.0, TimeOfDay::from_hours(3.0), &[]);
    let c = w.conditions_at([0.0, 0.0, 0.0]);
    assert!(c.road_temp_c < 0.0, "{}", c.road_temp_c);
    let t = w.data().surfaces.clone();
    let g = t.surface_grip_multiplier(SurfaceType::Asphalt, c.wetness, c.road_temp_c);
    assert!(g < 0.3, "black ice grip {g}");
    // Same road at 15:00 is just wet.
    w.update(1.0, TimeOfDay::from_hours(15.0), &[]);
    let c = w.conditions_at([0.0, 0.0, 0.0]);
    let g = t.surface_grip_multiplier(SurfaceType::Asphalt, c.wetness, c.road_temp_c);
    assert!(g > 0.6, "{g}");
}

#[test]
fn police_and_economy_hooks_follow_the_weather() {
    let mut w = sim(4, |c| c.locked = true);
    w.update(1.0, TimeOfDay::from_hours(12.0), &[]);
    let dry = (w.police_modifiers(), w.economy_modifiers());
    assert_eq!(dry.0.patrol_speed_cap_mult, 1.0);
    assert_eq!(dry.1.payout_mult, 1.0);
    w.force(Regime::new(WeatherKind::HeavyRain, 0.8), true);
    w.update(60.0, TimeOfDay::from_hours(12.0), &[]);
    let p = w.police_modifiers();
    assert!(p.patrol_speed_cap_mult < 0.9 && p.pursuit_speed_cap_mult < 0.9);
    assert!(p.crash_incident_rate_mult > 1.5);
    assert!(p.sight_multiplier < 1.0);
    assert!(w.economy_modifiers().payout_mult > 1.2);
    let tr = w.traffic_modifiers();
    assert!(tr.desired_speed_mult < 1.0 && tr.time_headway_mult > 1.0);
}

struct FlatWorld;
impl WorldQuery for FlatWorld {
    fn surface_at(&self, p: [f32; 3]) -> SurfaceType {
        if p[0] > 100.0 {
            SurfaceType::Grass
        } else {
            SurfaceType::Asphalt
        }
    }
    fn speed_limit_at(&self, _p: [f32; 3]) -> Option<f32> {
        Some(13.9)
    }
    fn signal_state_ahead(&self, _p: [f32; 3], _h: [f32; 3]) -> Option<SignalState> {
        None
    }
    fn time_of_day(&self) -> TimeOfDay {
        TimeOfDay::from_hours(9.0)
    }
    /// A wall along x = 200 blocks every sightline that crosses it.
    fn line_of_sight(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        (a[0] < 200.0) == (b[0] < 200.0)
    }
}

#[test]
fn adapter_forwards_line_of_sight() {
    let world = FlatWorld;
    let w = sim(1, |_| {});
    let q = WithWeather::new(&world, &w);
    assert!(q.line_of_sight([0.0; 3], [150.0, 0.0, 0.0]));
    assert!(!q.line_of_sight([0.0; 3], [250.0, 0.0, 0.0]));
}

#[test]
fn fog_shortens_police_sight_and_walls_still_block() {
    let world = FlatWorld;
    let mut w = sim(1, |c| c.locked = true);
    let t = TimeOfDay::from_hours(12.0);
    w.update(1.0, t, &[]);
    let a = [0.0, 0.0, 1.0];
    let near = [0.0, 100.0, 1.0];
    let mid = [0.0, 250.0, 1.0];
    let walled = [210.0, 0.0, 1.0];
    assert!(w.can_see(&world, a, near, 300.0));
    assert!(w.can_see(&world, a, mid, 300.0));
    assert!(!w.can_see(&world, a, walled, 300.0));
    w.force(Regime::new(WeatherKind::Fog, 1.0), true);
    w.update(1.0, t, &[]);
    // Thick fog: visibility ~130 m → multiplier ~0.16 → 300 m base sight shrinks to ~49 m.
    assert!(w.sight_multiplier() < 0.2);
    assert!(!w.can_see(&world, a, near, 300.0));
    assert!(!w.can_see(&world, a, mid, 300.0));
    assert!(w.can_see(&world, a, [0.0, 30.0, 1.0], 300.0));
}

#[test]
fn adapter_line_of_sight_is_cut_by_fog_range() {
    let world = FlatWorld;
    let mut w = sim(1, |c| c.locked = true);
    let t = TimeOfDay::from_hours(12.0);
    w.update(1.0, t, &[]);
    let a = [0.0, 0.0, 1.0];
    let far = [0.0, 300.0, 1.0];
    // Clear: the full 800 m reference range, so nothing a police range uses is cut.
    assert!((w.sight_range_m() - 800.0).abs() < 1e-3);
    assert!(WithWeather::new(&world, &w).line_of_sight(a, far));
    w.force(Regime::new(WeatherKind::Fog, 1.0), true);
    w.update(1.0, t, &[]);
    let r = w.sight_range_m();
    assert!((100.0..160.0).contains(&r), "{r}");
    let q = WithWeather::new(&world, &w);
    assert!(!q.line_of_sight(a, far));
    assert!(q.line_of_sight(a, [0.0, 100.0, 1.0]));
    // Walls still block inside the fog range; the geometric test alone ignores fog.
    assert!(!q.line_of_sight(a, [205.0, 0.0, 1.0]));
    let geo = WithWeather {
        weather_los: false,
        ..WithWeather::new(&world, &w)
    };
    assert!(geo.line_of_sight(a, far));
}

#[test]
fn visibility_m_is_the_police_sight_cap_in_every_regime() {
    let world = FlatWorld;
    let t = TimeOfDay::from_hours(12.0);
    let p = [10.0, -20.0, 1.0];
    // No weather and clear weather report the same visibility (behaviour unchanged).
    assert_eq!(
        wp_weather::LocalConditions::DRY.visibility_m,
        wp_weather::CLEAR_VISIBILITY_M
    );
    let mut w = sim(1, |c| c.locked = true);
    w.update(1.0, t, &[]);
    assert!((w.visibility_m(p) - wp_weather::CLEAR_VISIBILITY_M).abs() < 1e-3);
    let mut last = f32::INFINITY;
    for (kind, lo, hi) in [
        (WeatherKind::Clear, 799.0, 801.0),
        (WeatherKind::LightRain, 500.0, 800.0),
        (WeatherKind::HeavyRain, 300.0, 600.0),
        (WeatherKind::Fog, 100.0, 160.0),
    ] {
        w.force(Regime::new(kind, 1.0), true);
        w.update(1.0, t, &[]);
        let v = w.visibility_m(p);
        assert!((lo..=hi).contains(&v), "{kind:?}: {v} m");
        assert!(
            v <= last,
            "{kind:?} should not see further than the previous regime"
        );
        last = v;
        // One value everywhere it is exposed.
        assert_eq!(v, w.conditions_at(p).visibility_m);
        assert_eq!(v, w.sight_range_m());
        assert_eq!(v, WithWeather::new(&world, &w).visibility_m(p));
        // The adapter's LOS is cut exactly at it.
        let q = WithWeather::new(&world, &w);
        assert!(q.line_of_sight(p, [p[0], p[1] + v * 0.99, p[2]]));
        assert!(!q.line_of_sight(p, [p[0], p[1] + v * 1.01, p[2]]));
    }
}

#[test]
fn adapter_reports_wet_asphalt_only_when_wet() {
    let world = FlatWorld;
    let mut w = WeatherSim::from_world(
        WeatherData::embedded(),
        WeatherConfig::default(),
        grid(),
        &world,
    );
    let q = WithWeather::new(&world, &w);
    assert_eq!(q.surface_at([0.0, 0.0, 0.0]), SurfaceType::Asphalt);
    assert_eq!(q.speed_limit_at([0.0; 3]), Some(13.9));
    assert_eq!(q.time_of_day(), TimeOfDay::from_hours(9.0));
    w.set_all_water_mm(1.0);
    let q = WithWeather::new(&world, &w);
    assert_eq!(q.surface_at([0.0, 0.0, 0.0]), SurfaceType::WetAsphalt);
    assert_eq!(q.surface_at([150.0, 0.0, 0.0]), SurfaceType::Grass);
    assert!(q.weather_at([0.0; 3]).wetness > 0.99);
    let off = WithWeather {
        interim_surface: false,
        ..WithWeather::new(&world, &w)
    };
    assert_eq!(off.surface_at([0.0, 0.0, 0.0]), SurfaceType::Asphalt);
}

#[test]
fn snapshot_round_trip_reproduces_conditions() {
    let mut a = sim(6, |c| c.start = Regime::new(WeatherKind::LightRain, 0.7));
    run_hours(&mut a, 6.0, 2.0, |_| {});
    let mut b = sim(99, |c| c.locked = true);
    b.update(1.0, TimeOfDay::from_hours(8.0), &[]);
    assert!(b.apply_snapshot(&a.snapshot()));
    let p = [10.0, -30.0, 0.0];
    let (ca, cb) = (a.conditions_at(p), b.conditions_at(p));
    assert_eq!(ca.kind, cb.kind);
    assert!((ca.water_mm - cb.water_mm).abs() <= 0.025 + 1e-6);
    assert!((ca.rain_mm_per_h - cb.rain_mm_per_h).abs() < 1e-4);
}

#[test]
fn disk_tables_match_embedded() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    assert_eq!(WeatherData::load(&root).unwrap(), WeatherData::embedded());
}

#[test]
fn puddle_map_depends_on_seed_only() {
    let caps = |seed| {
        sim(seed, |_| {})
            .state()
            .wetness_by_cell
            .capacity_mm()
            .to_vec()
    };
    assert_eq!(caps(1), caps(1));
    assert_ne!(caps(1), caps(2));
}
