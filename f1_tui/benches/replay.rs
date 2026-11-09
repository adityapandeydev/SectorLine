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
#[path = "support/mod.rs"]
mod support;
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = std::env::args().collect::<Vec<_>>();
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
    let cache = SessionCache::open(root, &spec)?;
    let verified = started.elapsed().as_secs_f64() * 1000.0;
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
    let mut report = json!({"optimized":true,"manifest":path,"manifest_sha256":support::hash(&raw),"input_spec":meta["spec"],"core_rows":rows,"event_size":std::mem::size_of::<sectorline::domain::normalize::Event>(),"cache_verify_ms":verified,"memory_after_verify":support::memory(),"network":false,"terminal":"headless TestBackend; no emulator latency claim"});
    match RecordedReplay::from_cache(
        &cache,
        "RECORDED PERFORMANCE INPUT".into(),
        &Cancellation::default(),
        &|_, _| {},
    ) {
        Ok(replay) => {
            report["load_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
            report["memory_after_load"] = support::memory();
            report["benchmark"] = support::measure(replay, &args)?;
        }
        Err(error) => {
            report["load_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
            report["load_error"] = json!(error.to_string());
            report["memory_on_error"] = support::memory();
        }
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
