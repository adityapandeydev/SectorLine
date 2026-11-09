//! Explicit release-only replay measurements. Inputs and exclusions are emitted with results.
use sectorline::{
    data::{
        cache::{CacheSpec, SessionCache},
        limits::Cancellation,
    },
    replay::RecordedReplay,
};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Instant};
#[path = "support/capacity.rs"]
mod capacity;
#[path = "support/mod.rs"]
mod support;
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.iter().any(|a| a == "--synthetic-capacity") {
        if cfg!(debug_assertions) {
            return Err("Synthetic capacity requires an optimized bench build".into());
        }
        let started = Instant::now();
        let input = capacity::generate()?;
        let generation_ms = started.elapsed().as_secs_f64() * 1000.0;
        // Fresh child excludes generation allocations from resident-memory measurements.
        let frames = args
            .windows(2)
            .find(|a| a[0] == "--frames")
            .map(|a| a[1].as_str())
            .unwrap_or("1800");
        let output = std::process::Command::new(std::env::current_exe()?)
            .arg("--manifest")
            .arg(&input.manifest)
            .args(["--frames", frames])
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "Capacity child failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let measured: Value = serde_json::from_slice(&output.stdout)?;
        let report = json!({"optimized":true,"source":"SYNTHETIC raw-on-disk capacity:2h/22 drivers, telemetry+location4Hz, intervals+position0.2Hz,80 laps/driver","generation_ms":generation_ms,"raw_input_on_disk":true,"fresh_measurement_process":true,"benchmark_process":measured});
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if !args.iter().any(|a| a == "--manifest") {
        println!(
            "Replay benchmark skipped: provide --manifest PATH in an optimized cargo bench run"
        );
        return Ok(());
    }
    if cfg!(debug_assertions) {
        return Err("Use cargo bench --bench replay (optimized builds only)".into());
    }
    let path = args
        .windows(2)
        .find(|a| a[0] == "--manifest")
        .map(|a| PathBuf::from(&a[1]))
        .ok_or("Provide --manifest PATH; no network or implicit demo fallback")?;
    let raw = std::fs::read(&path)?;
    let meta: Value = serde_json::from_slice(&raw)?;
    let spec: CacheSpec = serde_json::from_value(meta["spec"].clone())?;
    let root = path
        .parent()
        .and_then(|p| p.parent())
        .ok_or("Manifest must be under the recording root/session directory")?;
    let started = Instant::now();
    let legacy = args.iter().any(|a| a == "--legacy");
    let mut verified = None;
    let loaded = if legacy {
        let cache = SessionCache::open(root, &spec)?;
        verified = Some(started.elapsed().as_secs_f64() * 1000.0);
        RecordedReplay::from_cache(
            &cache,
            "RECORDED PERFORMANCE INPUT".into(),
            &Cancellation::default(),
            &|_, _| {},
        )
    } else {
        RecordedReplay::open_cache(
            root,
            &spec,
            "RECORDED PERFORMANCE INPUT".into(),
            &Cancellation::default(),
            &|_, _| {},
        )
    };
    let core = [
        "Position",
        "CarData",
        "Location",
        "Intervals",
        "Laps",
        "RaceControl",
    ];
    let rows: u64 = meta["entries"]
        .as_array()
        .ok_or("Missing entries")?
        .iter()
        .filter(|e| core.contains(&e["metadata"]["channel"].as_str().unwrap_or("")))
        .filter_map(|e| e["records"].as_u64())
        .sum();
    let mut report = json!({"optimized":true,"manifest":path,"manifest_sha256":support::hash(&raw),"input_spec":meta["spec"],"core_rows":rows,"event_size":std::mem::size_of::<sectorline::domain::normalize::Event>(),"cache_verify_ms":verified,"memory_after_load_attempt":support::memory(),"network":false,"terminal":"headless TestBackend; no emulator latency claim"});
    report["load_mode"] = json!(if legacy {
        "verify then normalize"
    } else {
        "normalize while verifying"
    });
    match loaded {
        Ok(replay) => {
            report["load_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
            report["memory_after_load"] = support::memory();
            if !args.iter().any(|a| a == "--load-only") {
                report["benchmark"] = support::measure(replay, &args)?;
            }
        }
        Err(error) => {
            report["load_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
            report["load_error"] = json!(error.to_string());
            report["memory_on_error"] = support::memory();
        }
    }
    let failed = report.get("load_error").is_some();
    println!("{}", serde_json::to_string_pretty(&report)?);
    if failed {
        return Err("Replay load failed; see the measurement report".into());
    }
    Ok(())
}
