//! Shared measurement helpers for the replay benchmark (not a standalone example).
use sectorline::{
    App, TelemetryStream,
    domain::ElapsedMillis,
    replay::{RecordedReplay, timeline::ReplayPolicy},
    ui::View,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
pub fn hash(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}
pub fn summary(samples: &[f64]) -> Value {
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    let p = |q: f64| {
        v[((q * v.len() as f64).ceil() as usize)
            .saturating_sub(1)
            .min(v.len() - 1)]
    };
    json!({"samples":v.len(),"min_ms":v[0],"median_ms":p(0.5),"p95_ms":p(0.95),"p99_ms":p(0.99),"max_ms":v[v.len()-1]})
}
pub fn measure(
    mut r: RecordedReplay,
    args: &[String],
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let count = args
        .windows(2)
        .find(|a| a[0] == "--frames")
        .map(|a| a[1].parse::<usize>())
        .transpose()?
        .unwrap_or(1800);
    if !(100..=100000).contains(&count) {
        return Err("Frames must be100-100000".into());
    }
    let end = r.timeline().end().0;
    let mut seek = Vec::new();
    let mut state = 0x51ec7_u64;
    for i in 0..100 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let target = if i == 0 {
            0
        } else if i == 1 {
            end
        } else {
            state % (end + 1)
        };
        let t = Instant::now();
        r.seek(ElapsedMillis(target))?;
        seek.push(t.elapsed().as_secs_f64() * 1000.0);
        let reference = r
            .timeline()
            .state_at(ElapsedMillis(target), ReplayPolicy::EventTimeEstimates)?
            .frame(r.timeline())?;
        assert_eq!(
            serde_json::to_vec(r.frame())?,
            serde_json::to_vec(&reference)?,
            "Seek/reference mismatch at {target}"
        );
        let conditions = r
            .timeline()
            .conditions_at(ElapsedMillis(target), ReplayPolicy::EventTimeEstimates)?;
        assert_eq!(
            serde_json::to_vec(r.conditions())?,
            serde_json::to_vec(&conditions)?,
            "Seek/conditions mismatch at {target}"
        );
    }
    let mut app = App::new(
        TelemetryStream::load_from_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache"),
        )
        .map_err(|e| e.to_string())?,
        false,
    );
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(144, 44))?;
    let mut views = serde_json::Map::new();
    let mut now = Duration::ZERO;
    for view in [
        View::Timing,
        View::Driver,
        View::Conditions,
        View::Track,
        View::Events,
    ] {
        app.view = view;
        let mut update = Vec::new();
        let mut draw = Vec::new();
        let mut total = Vec::new();
        for i in 0..count {
            // Six deterministic windows distributed across coverage; 33 ms steps are1x frame work.
            let per = (count / 6).max(1);
            if i % per == 0 {
                let base = (end.saturating_sub(per as u64 * 33)) * (i / per).min(5) as u64 / 5;
                r.pause()?;
                r.seek(ElapsedMillis(base))?;
                r.toggle()?;
            }
            now += Duration::from_millis(33);
            app.prepared_recording = Some(r);
            let t = Instant::now();
            app.update_at(now);
            assert!(
                !app.load_error,
                "Replay update failed: {:?}",
                app.load_message
            );
            let prep = t.elapsed().as_secs_f64() * 1000.0;
            let d = Instant::now();
            terminal.draw(|f| app.render(f))?;
            let render = d.elapsed().as_secs_f64() * 1000.0;
            r = app.prepared_recording.take().unwrap();
            black_box(terminal.backend().buffer());
            update.push(prep);
            draw.push(render);
            total.push(prep + render);
        }
        views.insert(format!("{view:?}"),json!({"update":summary(&update),"render_and_buffer_diff":summary(&draw),"total":summary(&total)}));
    }
    Ok(
        json!({"coverage_ms":end,"drivers":r.timeline().drivers().len(),"seeks":summary(&seek),"reference_checks":100,"views":views,"memory":memory(),"sampling":"six stratified1x windows; seeks into windows excluded from frame samples; no sleep/emulator"}),
    )
}
#[cfg(windows)]
pub fn memory() -> Value {
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
pub fn memory() -> Value {
    json!({"unavailable": "Measure resident memory using the host OS profiler"})
}
