//! Offline full-recording check against independently time-filtered raw telemetry.
use sectorline::{
    data::{
        cache::{CacheSpec, SessionCache},
        catalogue::{Catalogue, now_ms},
        limits::{Cancellation, Limits},
        openf1::{AcquisitionRequest, Channel, Result},
        paths::session_cache_root,
    },
    domain::{ElapsedMillis, UtcMillis},
    replay::timeline::{ReplayPolicy, Timeline},
};
use std::{collections::BTreeMap, path::PathBuf};

fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 && args.len() != 4 {
        return Err("Usage: cargo run --release -p sectorline --example replay_check -- CATALOGUE_JSON SESSION_ID [CACHE_ROOT]".into());
    }
    let catalogue = Catalogue::load(&PathBuf::from(&args[1])).map_err(|e| e.to_string())?;
    let selection = catalogue
        .select(args[2].parse()?, now_ms().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let request = AcquisitionRequest::from_selection(&selection)?;
    let spec = CacheSpec::new(
        request.session_key(),
        request.meeting_key(),
        selection.session.year,
        (
            &selection.session.session_name,
            &selection.session.session_type,
        ),
        request.window(),
        Limits::default().chunk_span,
        Channel::ALL.to_vec(),
    )?;
    let root = if let Some(root) = args.get(3) {
        PathBuf::from(root)
    } else {
        session_cache_root()?
    };
    let cache = SessionCache::open(&root, &spec)?;
    let timeline = Timeline::from_cache(&cache, &Cancellation::default(), &|_, _| {})?;
    let end = timeline.end().0;
    let targets = [end / 4, end / 2, end * 3 / 4, end];
    // Four small oracle maps, fed by raw source JSON rather than reducer events.
    let mut expected = targets.map(|_| BTreeMap::<u32, (i64, serde_json::Value)>::new());
    for index in 0..cache.chunk_count() {
        if cache.chunk_metadata(index)?.channel != Channel::CarData
            || cache.chunk_metadata(index)?.status != 200
        {
            continue;
        }
        let chunk = cache.read_chunk(index)?;
        let rows: Vec<serde_json::Value> = serde_json::from_slice(chunk.raw_bytes())?;
        for row in rows {
            let date = chrono::DateTime::parse_from_rfc3339(
                row["date"].as_str().ok_or("Missing source date")?,
            )?;
            let driver = row["driver_number"].as_u64().ok_or("Missing driver")? as u32;
            for (target, map) in targets.iter().zip(expected.iter_mut()) {
                let utc =
                    chrono::DateTime::from_timestamp_millis(timeline.origin().0 + *target as i64)
                        .ok_or("Invalid target")?;
                if date <= utc
                    && map
                        .get(&driver)
                        .is_none_or(|(old, _)| date.timestamp_micros() > *old)
                {
                    map.insert(driver, (date.timestamp_micros(), row.clone()));
                }
            }
        }
    }
    println!(
        "Session {} | normalized events {} | {:?}",
        timeline.session().key(),
        timeline.event_count(),
        timeline.report()
    );
    for (target, expected) in targets.into_iter().zip(expected) {
        let frame = timeline
            .state_at(ElapsedMillis(target), ReplayPolicy::EventTimeEstimates)?
            .frame(&timeline)?;
        for sample in frame.samples() {
            let row = expected
                .get(&sample.driver.number.get())
                .map(|(_, row)| row);
            let number = |name: &str| row.and_then(|r| r[name].as_f64());
            assert_eq!(
                sample.speed_kmh.value().map(|v| v.get()),
                number("speed").filter(|v| v.is_finite() && *v >= 0.0)
            );
            assert_eq!(
                sample.rpm.value().map(|v| v.0 as f64),
                number("rpm").filter(|v| *v >= 0.0 && v.fract() == 0.0)
            );
            assert_eq!(
                sample.gear.value().map(|v| v.get() as f64),
                number("n_gear").filter(|v| (0.0..=8.0).contains(v) && v.fract() == 0.0)
            );
            assert_eq!(
                sample.throttle.value().map(|v| v.get()),
                number("throttle").filter(|v| (0.0..=100.0).contains(v))
            );
            let brake = sample.brake.value().map(|v| match v {
                sectorline::domain::BrakeState::Pressed => 100.0,
                sectorline::domain::BrakeState::Released => 0.0,
            });
            assert_eq!(brake, number("brake").filter(|v| *v == 0.0 || *v == 100.0));
            assert_eq!(
                sample.drs.value().map(|v| v.0 as f64),
                number("drs").filter(|v| *v >= 0.0 && v.fract() == 0.0)
            );
            assert!(sample.sectors.iter().all(|s| s.value().is_none()));
        }
        println!(
            "Verified UTC {}: {} drivers, all core telemetry fields match eligible raw records",
            timeline.origin().0 + target as i64,
            frame.samples().len()
        );
    }
    println!(
        "Recorded origin {:?}; end {:?}; raw source gaps retained. No generated data or network used.",
        UtcMillis(timeline.origin().0),
        timeline.end()
    );
    let mut replay = sectorline::replay::RecordedReplay::new(timeline, "Offline traversal".into())?;
    replay.set_speed(sectorline::replay::clock::Speed::Twenty)?;
    replay.toggle()?;
    let mut ticks = 0;
    let mut now = std::time::Duration::ZERO;
    while replay.status() != sectorline::replay::clock::PlaybackStatus::Ended {
        now += std::time::Duration::from_millis([17, 83, 5, 125][ticks % 4]);
        replay.update_at(now)?;
        ticks += 1;
    }
    let reference = replay
        .timeline()
        .state_at(replay.timeline().end(), ReplayPolicy::EventTimeEstimates)?
        .frame(replay.timeline())?;
    assert_eq!(replay.frame(), &reference);
    replay.update_at(now + std::time::Duration::from_secs(60))?;
    assert_eq!(replay.frame(), &reference);
    replay.restart()?;
    assert_eq!(replay.frame().elapsed(), ElapsedMillis(0));
    assert_eq!(
        replay.status(),
        sectorline::replay::clock::PlaybackStatus::Paused
    );
    println!(
        "Full recorded traversal passed: {ticks} irregular fake-clock ticks at 20x; end stayed fixed, deliberate restart reset state."
    );
    Ok(())
}
