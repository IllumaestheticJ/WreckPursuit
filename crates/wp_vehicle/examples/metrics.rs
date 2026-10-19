//! Print the handling metrics for every car in `data/cars/` (or the paths given).
//! `cargo run --release -p wp_vehicle --example metrics [data/cars/x.json ...]`

fn main() {
    let mut paths: Vec<std::path::PathBuf> = std::env::args().skip(1).map(Into::into).collect();
    if paths.is_empty() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/cars");
        paths = std::fs::read_dir(dir)
            .expect("data/cars")
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        paths.sort();
    }
    for p in paths {
        let car = wp_contracts::load_car(&p).expect("car loads");
        let t = std::time::Instant::now();
        let r = wp_vehicle::metrics::report(&car);
        println!(
            "{} ({:.1} s to compute)\n{r:#?}\n",
            car.def.id,
            t.elapsed().as_secs_f32()
        );
    }
}
