//! `tuning-probe` CLI. See `tools/tuning-dash/README.md`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tuning_probe::{diff, overrides, path, probe, promote};

const USAGE: &str = "\
tuning-probe — measurement backend of tools/tuning-dash

  tuning-probe run <job.json|-> [--data DIR] [--result FILE]   measure one sweep point
  tuning-probe resolve <file:path>... [--data DIR]             list the values a target selects
  tuning-probe diff <fileA> <fileB> [--data DIR] [--limit N]   field diff of two data files
  tuning-probe promote <request.json|-> [--data DIR]           write values back + changelog

Files are data-root relative (`cars/x.json`) or shorthands (`car:x`, `tire:street`, `mode:sim`).
Output is JSON on stdout. Exit 0 ok, 1 measured/refused, 2 usage error.";

fn default_data() -> PathBuf {
    let local = PathBuf::from("data");
    if local.join("cars").is_dir() {
        local
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../data")
    }
}

fn read_arg(p: &str) -> Result<String, String> {
    if p == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|e| e.to_string())?;
        Ok(s)
    } else {
        std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))
    }
}

fn print_json<T: serde::Serialize>(v: &T) {
    println!("{}", serde_json::to_string_pretty(v).expect("serializes"));
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut pos = Vec::new();
    let mut data = None;
    let mut result_file = None;
    let mut limit = 2000usize;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--data" | "--result" | "--limit" if i + 1 >= args.len() => {
                eprintln!("{} needs a value\n\n{USAGE}", args[i]);
                return ExitCode::from(2);
            }
            "--data" => {
                data = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--result" => {
                result_file = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--limit" => {
                limit = args[i + 1].parse().unwrap_or(limit);
                i += 1;
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            a => pos.push(a.to_string()),
        }
        i += 1;
    }
    let data = data.unwrap_or_else(default_data);
    let r = match pos.first().map(String::as_str) {
        Some("run") if pos.len() == 2 => cmd_run(&pos[1], &data, result_file.as_deref()),
        Some("resolve") if pos.len() >= 2 => cmd_resolve(&pos[1..], &data),
        Some("diff") if pos.len() == 3 => cmd_diff(&pos[1], &pos[2], &data, limit),
        Some("promote") if pos.len() == 2 => cmd_promote(&pos[1], &data),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match r {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("tuning-probe: {e}");
            ExitCode::from(2)
        }
    }
}

fn cmd_run(job: &str, data: &Path, result_file: Option<&Path>) -> Result<bool, String> {
    let job: probe::Job = serde_json::from_str(&read_arg(job)?).map_err(|e| format!("job: {e}"))?;
    let res = probe::run_job(&job, data);
    let text = serde_json::to_string_pretty(&res).expect("serializes");
    match result_file {
        Some(p) => std::fs::write(p, text + "\n").map_err(|e| format!("{}: {e}", p.display()))?,
        None => println!("{text}"),
    }
    Ok(res.ok)
}

fn load_doc(file: &str, data: &Path) -> Result<(String, serde_json::Value), String> {
    // Data-root relative (or a shorthand) first, then a plain path on disk.
    let (label, disk) = path::expand_file(file)
        .ok()
        .map(|rel| (rel.clone(), data.join(rel)))
        .filter(|(_, p)| p.exists())
        .unwrap_or_else(|| (file.to_string(), PathBuf::from(file)));
    let text = std::fs::read_to_string(&disk).map_err(|e| format!("{}: {e}", disk.display()))?;
    let doc = overrides::parse_file(&label, &text)?;
    Ok((label, doc))
}

fn cmd_resolve(targets: &[String], data: &Path) -> Result<bool, String> {
    let mut out = Vec::new();
    for t in targets {
        let target = path::parse_target(t)?;
        if target.file == path::SCENARIO_FILE {
            out.push(serde_json::json!({"target": t, "file": "scenario", "values": []}));
            continue;
        }
        let (_, doc) = load_doc(&target.file, data)?;
        let locs = path::resolve(&doc, &target.segments).map_err(|e| format!("{t}: {e}"))?;
        let values: Vec<_> = locs
            .iter()
            .map(|s| serde_json::json!({"pointer": path::pointer(s), "value": path::get(&doc, s)}))
            .collect();
        out.push(serde_json::json!({"target": t, "file": target.file, "values": values}));
    }
    print_json(&out);
    Ok(true)
}

fn cmd_diff(a: &str, b: &str, data: &Path, limit: usize) -> Result<bool, String> {
    let (la, da) = load_doc(a, data)?;
    let (lb, db) = load_doc(b, data)?;
    let mut d = diff::diff(&da, &db, limit);
    d.a = la;
    d.b = lb;
    print_json(&d);
    Ok(true)
}

fn cmd_promote(req: &str, data: &Path) -> Result<bool, String> {
    let req: promote::Request =
        serde_json::from_str(&read_arg(req)?).map_err(|e| format!("request: {e}"))?;
    let o = promote::promote(&req, data);
    print_json(&o);
    Ok(o.ok)
}
