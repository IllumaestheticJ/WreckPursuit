//! Runs `examples/weather_in_sim.rs` (weather composed with `wp_sim` from the outside) and checks
//! its numbers, so the code the lead moves into `wp_sim` stays correct on every `cargo test`.

#[path = "../examples/weather_in_sim.rs"]
mod example;

#[test]
fn weather_in_sim_example_holds() {
    let r = example::run();
    eprintln!("{r:#?}");
    let (sim, dry, clear, wet) = (r.sim_dry, r.rig_dry, r.rig_clear, r.rig_wet);
    // The rig is Sim's vehicle path: the dry rig stops where the dry Sim car stops.
    assert!(
        (dry.distance_m - sim.distance_m).abs() <= 0.05 * sim.distance_m,
        "rig dry {:.2} m vs Sim dry {:.2} m",
        dry.distance_m,
        sim.distance_m
    );
    // Weather switched on but clear and dry changes nothing, bit for bit.
    assert_eq!(dry.distance_m, clear.distance_m);
    assert_eq!(dry.time_s, clear.time_s);
    // A soaked road brakes clearly longer (tests/vehicle_grip.rs: 42.9 → 54.1 m on the rig).
    let ratio = wet.distance_m / dry.distance_m;
    assert!((1.15..=1.7).contains(&ratio), "wet/dry = {ratio:.2}");
    assert!((30.0..=60.0).contains(&dry.distance_m));
    // Every Sim wheel sits on soaked asphalt: the tires get WetAsphalt today and the full model
    // (after W-1) costs at least as much grip as the interim remap.
    for w in &r.sim_wheels {
        assert_eq!(w.base_surface, wp_contracts::SurfaceType::Asphalt);
        assert_eq!(w.interim_surface, wp_contracts::SurfaceType::WetAsphalt);
        assert!(w.conditions.wetness > 0.9, "{:?}", w.conditions);
        assert!(w.full_grip_mult < 0.9, "{}", w.full_grip_mult);
    }
    // Deterministic: a second run gives the same weather hash and stops.
    let again = example::run();
    assert_eq!(r.sim_weather_hash, again.sim_weather_hash);
    assert_eq!(wet.distance_m, again.rig_wet.distance_m);
    // Police sight cap: 800 m clear, fog the shortest.
    let vis = |k| r.visibility_m.iter().find(|(kk, _)| *kk == k).unwrap().1;
    use wp_weather::WeatherKind as K;
    assert!((vis(K::Clear) - 800.0).abs() < 1.0);
    assert!(vis(K::Fog) < 160.0);
    assert!(vis(K::HeavyRain) < vis(K::LightRain));
}
