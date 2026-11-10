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
    verify_completed_laps(&cache, &timeline)?;
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
            for sector in &sample.sectors {
                if sector.value().is_some() {
                    assert!(
                        sector.timing().observed_at.unwrap().0
                            <= timeline.origin().0 + target as i64
                    );
                }
            }
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
    let laps =
        sectorline::replay::navigation::LapIndex::from_cache(&cache, &Cancellation::default())?;
    let mut replay = sectorline::replay::RecordedReplay::with_navigation(
        timeline,
        "Offline traversal".into(),
        laps,
        &Cancellation::default(),
    )?;
    let mut seed = 42_u64;
    let mut slowest = std::time::Duration::ZERO;
    for _ in 0..100 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let target = ElapsedMillis(seed % (replay.timeline().end().0 + 1));
        let reference = replay
            .timeline()
            .state_at(target, ReplayPolicy::EventTimeEstimates)?
            .frame(replay.timeline())?;
        let start = std::time::Instant::now();
        replay.seek(target)?;
        slowest = slowest.max(start.elapsed());
        assert_eq!(replay.frame(), &reference);
    }
    // Verify cache-backed lap navigation independently against the raw archive.
    let selected = replay.selected();
    let mut lap_checked = false;
    for i in 0..cache.chunk_count() {
        if cache.chunk_metadata(i)?.channel != Channel::Laps {
            continue;
        }
        let chunk = cache.read_chunk(i)?;
        for row in chunk.records() {
            if row["driver_number"].as_u64() != Some(selected as u64) {
                continue;
            }
            if let (Some(lap), Some(date)) =
                (row["lap_number"].as_u64(), row["date_start"].as_str())
            {
                let start = sectorline::domain::normalize::utc_millis_ceiling(date)?;
                if start.0 < replay.timeline().origin().0 {
                    continue;
                }
                replay.navigate(sectorline::replay::navigation::Navigation::Lap(
                    lap.try_into()?,
                ))?;
                assert_eq!(
                    replay.frame().elapsed().0,
                    (start.0 - replay.timeline().origin().0) as u64
                );
                lap_checked = true;
                break;
            }
        }
        if lap_checked {
            break;
        }
    }
    assert!(lap_checked, "Expected a cache-backed lap-start check");
    println!(
        "100 full-recording seeks match fresh reconstruction; slowest measured seek {:?}. Cache-backed selected-driver lap start matches raw data.",
        slowest
    );
    replay.restart()?;
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

/// Independent raw archive oracle for completion, last lap, sectors and provisional best.
fn verify_completed_laps(cache: &SessionCache, timeline: &Timeline) -> Result<()> {
    use serde_json::Value;
    let mut laps = Vec::<Value>::new();
    let mut controls = Vec::<Value>::new();
    for i in 0..cache.chunk_count() {
        let channel = cache.chunk_metadata(i)?.channel;
        if matches!(channel, Channel::Laps | Channel::RaceControl) {
            let chunk = cache.read_chunk(i)?;
            let rows: Vec<Value> = serde_json::from_slice(chunk.raw_bytes())?;
            if channel == Channel::Laps {
                laps.extend(rows);
            } else {
                controls.extend(rows);
            }
        }
    }
    let stamp = |text: &str| -> i64 {
        let d = chrono::DateTime::parse_from_rfc3339(text).unwrap();
        d.timestamp_millis() + i64::from(!d.timestamp_subsec_nanos().is_multiple_of(1_000_000))
    };
    let completion = |r: &Value| -> Option<i64> {
        let start = r["date_start"].as_str().map(stamp);
        let sectors = (1..=3)
            .map(|i| {
                r[format!("duration_sector_{i}")]
                    .as_f64()
                    .filter(|v| *v > 0.0)
            })
            .collect::<Option<Vec<_>>>()
            .map(|v| v.iter().sum::<f64>());
        let duration = r["lap_duration"]
            .as_f64()
            .filter(|v| *v > 0.0)
            .into_iter()
            .chain(sectors)
            .reduce(f64::max);
        let inferred = start
            .zip(duration)
            .map(|(s, d)| s + (d * 1000.0).ceil() as i64);
        let next = laps
            .iter()
            .find(|n| {
                n["driver_number"] == r["driver_number"]
                    && n["lap_number"].as_u64() == r["lap_number"].as_u64().map(|n| n + 1)
            })
            .and_then(|r| r["date_start"].as_str())
            .map(stamp);
        inferred.into_iter().chain(next).max()
    };
    let origin = timeline.origin().0;
    let end = timeline.end().0;
    let mut targets = vec![end / 4, end / 2, end * 3 / 4, end];
    // Check just before and at each named deletion, not merely the final result.
    for r in &controls {
        if r["message"]
            .as_str()
            .is_some_and(|s| s.contains(" DELETED - "))
        {
            let t = stamp(r["date"].as_str().unwrap()) - origin;
            if t > 0 {
                targets.extend([t as u64 - 1, t as u64]);
            }
        }
    }
    for target in &targets {
        let utc = origin + *target as i64;
        let frame = timeline
            .state_at(ElapsedMillis(*target), ReplayPolicy::EventTimeEstimates)?
            .frame(timeline)?;
        let mut deleted = std::collections::BTreeSet::new();
        for control in &controls {
            if stamp(control["date"].as_str().unwrap()) > utc {
                continue;
            }
            let message = control["message"].as_str().unwrap_or("");
            let words = message.split_whitespace().collect::<Vec<_>>();
            if words.first() != Some(&"CAR") {
                continue;
            }
            let driver = words.get(1).and_then(|n| n.parse::<u32>().ok());
            let lap = words
                .iter()
                .rposition(|w| *w == "LAP")
                .and_then(|i| words.get(i + 1))
                .and_then(|n| n.parse::<u32>().ok());
            if let (Some(driver), Some(lap)) = (driver, lap) {
                if message.contains(" DELETED - ") {
                    deleted.insert((driver, lap));
                } else if message.contains(" REINSTATED") {
                    deleted.remove(&(driver, lap));
                }
            }
        }
        for car in frame.samples() {
            let driver = car.driver.number.get();
            let completed = laps
                .iter()
                .filter(|r| {
                    r["driver_number"].as_u64() == Some(driver as u64)
                        && completion(r).is_some_and(|t| t <= utc)
                })
                .collect::<Vec<_>>();
            let latest = completed.iter().max_by_key(|r| r["lap_number"].as_u64());
            assert_eq!(
                car.lap_number.value().map(|n| n.get() as u64),
                latest.and_then(|r| r["lap_number"].as_u64())
            );
            let consistent = |r: &Value| {
                let sum = (1..=3)
                    .map(|i| r[format!("duration_sector_{i}")].as_f64())
                    .collect::<Option<Vec<_>>>()
                    .map(|s| s.iter().sum::<f64>());
                r["lap_duration"]
                    .as_f64()
                    .zip(sum)
                    .is_none_or(|(d, s)| (d - s).abs() <= 0.01)
            };
            assert_eq!(
                car.lap_time.value().map(|v| v.get()),
                latest
                    .filter(|r| consistent(r))
                    .and_then(|r| r["lap_duration"].as_f64().filter(|v| *v > 0.0))
            );
            for (i, sector) in car.sectors.iter().enumerate() {
                assert_eq!(
                    sector.value().map(|v| v.get()),
                    latest.and_then(|r| r[format!("duration_sector_{}", i + 1)]
                        .as_f64()
                        .filter(|v| *v > 0.0))
                );
            }
            let best = completed
                .into_iter()
                .filter(|r| {
                    consistent(r)
                        && r["is_pit_out_lap"].as_bool() == Some(false)
                        && r["lap_duration"].as_f64().is_some_and(|v| v > 0.0)
                        && !deleted.contains(&(driver, r["lap_number"].as_u64().unwrap() as u32))
                })
                .min_by(|a, b| {
                    a["lap_duration"]
                        .as_f64()
                        .unwrap()
                        .total_cmp(&b["lap_duration"].as_f64().unwrap())
                        .then_with(|| a["lap_number"].as_u64().cmp(&b["lap_number"].as_u64()))
                });
            assert_eq!(
                car.personal_best
                    .value()
                    .map(|b| (b.number.get() as u64, b.duration.get())),
                best.map(|b| (
                    b["lap_number"].as_u64().unwrap(),
                    b["lap_duration"].as_f64().unwrap()
                ))
            );
            assert!(
                car.status.value().is_none(),
                "No timestamped explicit retirement result expected in this Monza fixture"
            );
        }
    }
    println!(
        "Verified {} full-race timing checkpoints against raw laps and named deletion messages: completed lap, sectors, provisional personal best and withheld final status.",
        targets.len()
    );
    Ok(())
}
