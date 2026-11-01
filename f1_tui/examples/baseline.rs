//! Reproducible headless baseline of the existing demo loader and renderer.
//! This does not measure a terminal emulator, a real race, or network ingestion.
use ratatui::{Terminal, backend::TestBackend};
use sectorline::{App, TelemetryStream};
use serde_json::{Value, json};
use std::{error::Error, hint::black_box, path::PathBuf, time::Instant};

fn summary(samples: &[f64]) -> Value {
    assert!(!samples.is_empty());
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile =
        |p: f64| sorted[((p * sorted.len() as f64).ceil() as usize - 1).min(sorted.len() - 1)];
    json!({"samples": sorted.len(), "min_ms": sorted[0], "median_ms": percentile(0.5),
        "p95_ms": percentile(0.95), "p99_ms": percentile(0.99), "max_ms": sorted[sorted.len()-1]})
}

#[cfg(windows)]
fn memory() -> Value {
    use std::{ffi::c_void, mem::size_of};
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(process: *mut c_void, counters: *mut Counters, size: u32) -> i32;
    }
    let mut counters = Counters {
        cb: size_of::<Counters>() as u32,
        ..Counters::default()
    };
    // SAFETY: the pseudo-handle is valid for this process and the writable C-layout
    // buffer has the exact size supplied to the Windows API. No handle is owned.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            size_of::<Counters>() as u32,
        )
    };
    if ok == 0 {
        return json!({"error": std::io::Error::last_os_error().to_string()});
    }
    json!({"working_set_bytes": counters.working_set, "peak_working_set_bytes": counters.peak_working_set,
        "private_commit_bytes": counters.pagefile, "method": "Windows GetProcessMemoryInfo"})
}

#[cfg(not(windows))]
fn memory() -> Value {
    json!({"unavailable": "Measure resident memory using the host OS profiler"})
}

fn main() -> Result<(), Box<dyn Error>> {
    let cache = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache")
        });
    let initial_memory = memory();
    let mut loads = Vec::with_capacity(31);
    for _ in 0..31 {
        let started = Instant::now();
        let stream = TelemetryStream::load_from_dir(&cache)?;
        loads.push(started.elapsed().as_secs_f64() * 1000.0);
        black_box(&stream);
        // Destruction is outside the load timing boundary.
    }
    let stream = TelemetryStream::load_from_dir(&cache)?;
    let dataset = json!({"frames": stream.frames.len(), "drivers": stream.drivers.len(),
        "first_timestamp_ms": stream.frames.first().map(|f| f.timestamp_ms),
        "last_timestamp_ms": stream.frames.last().map(|f| f.timestamp_ms),
        "event_name": stream.session.event_name, "data_kind": "existing generated demo, not observed race data"});
    let loaded_memory = memory();
    let mut app = App::new(stream, false);
    let mut render_runs = Vec::new();
    for (width, height) in [(144, 44), (120, 36), (80, 24)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        for _ in 0..300 {
            app.tick(1000.0 / 30.0);
            terminal.draw(|frame| app.render(frame))?;
        }
        let mut frames = Vec::with_capacity(1800);
        for _ in 0..1800 {
            let started = Instant::now();
            app.tick(1000.0 / 30.0);
            terminal.draw(|frame| app.render(frame))?;
            frames.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        black_box(terminal.backend());
        render_runs.push(
            json!({"width_columns": width, "height_rows": height, "warmup_frames": 300,
            "tick_ms": 1000.0/30.0, "frame_cost": summary(&frames), "raw_frame_ms": frames}),
        );
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"schema_version":1,
        "optimized": !cfg!(debug_assertions), "backend":"Ratatui TestBackend; no terminal output",
        "timing_boundary":"app.tick plus terminal.draw including in-memory buffer diff; no sleeps",
        "dataset":dataset, "initial_memory":initial_memory, "loaded_memory":loaded_memory,
        "final_memory":memory(), "first_load_ms":loads[0], "warm_load_cost":summary(&loads[1..]),
        "raw_load_ms":loads, "render_runs":render_runs}))?
    );
    Ok(())
}
