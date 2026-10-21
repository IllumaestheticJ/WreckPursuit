//! Debug: print a car's cross-section rings at a few stations.
//! `cargo run -p car-assets --example ring_dump -- <car> y0 y1 ...`
use car_assets::profile::BodyProfile;
use car_assets::shell::Shell;
use car_assets::{load_car, load_style, repo_root};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root();
    let car = load_car(&root, &a[0]).unwrap();
    let style = load_style(&root, &a[0]).unwrap();
    let p = BodyProfile::measure(&car.def, &car.lattice, &style);
    let sh = Shell::new(p, &style);
    for y in a[1..].iter().map(|s| s.parse::<f32>().unwrap()) {
        let r = sh.ring(y);
        let pts: Vec<String> = r
            .iter()
            .map(|q| format!("({:.3},{:.3})", q[0], q[2]))
            .collect();
        println!("y {y:.2}: {}", pts.join(" "));
    }
}
