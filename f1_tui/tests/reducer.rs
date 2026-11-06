use sectorline::{
    data::openf1::{Channel, ChunkMetadata},
    domain::{
        Availability, ElapsedMillis, Percent, Provider, SessionId, SpeedKmh, UtcMillis,
        normalize::{Event, Payload, RosterDriver, TelemetryValues},
    },
    replay::timeline::{ReplayPolicy, Timeline},
};
use std::num::NonZeroU32;

fn source(retrieved: i64) -> ChunkMetadata {
    ChunkMetadata {
        channel: Channel::CarData,
        session_key: 9590,
        meeting_key: 1244,
        window: None,
        source_url: "https://api.openf1.org/v1/car_data?session_key=9590".into(),
        retrieved_at: UtcMillis(retrieved),
        status: 200,
        attempts: 1,
    }
}
fn event(observed: i64, available: Availability, source: u32, speed: Option<f64>) -> Event {
    Event::new(
        4,
        UtcMillis(observed),
        available,
        source,
        Payload::Telemetry(TelemetryValues {
            speed: speed.map(|s| SpeedKmh::new(s).unwrap()),
            ..TelemetryValues::default()
        }),
    )
    .unwrap()
}
fn timeline(events: Vec<Event>, sources: Vec<ChunkMetadata>) -> Timeline {
    Timeline::new(
        SessionId::new(Provider::OpenF1, "9590").unwrap(),
        UtcMillis(1000),
        vec![RosterDriver::new(4, "NOR", "Lando Norris").unwrap()],
        sources,
        events,
    )
    .unwrap()
}

#[test]
fn reducer_admits_known_values_only_at_availability_boundary() {
    let t = timeline(
        vec![event(
            1000,
            Availability::Known(UtcMillis(2000)),
            0,
            Some(200.0),
        )],
        vec![source(9000)],
    );
    let early = t
        .state_at(ElapsedMillis(999), ReplayPolicy::KnownOnly)
        .unwrap()
        .frame(&t)
        .unwrap();
    assert!(early.samples()[0].speed_kmh.value().is_none());
    let frame = t
        .state_at(ElapsedMillis(1000), ReplayPolicy::KnownOnly)
        .unwrap()
        .frame(&t)
        .unwrap();
    let speed = &frame.samples()[0].speed_kmh;
    assert_eq!(speed.value().unwrap().get(), 200.0);
    assert_eq!(speed.timing().observed_at, Some(UtcMillis(1000)));
    assert_eq!(speed.timing().retrieved_at, Some(UtcMillis(9000)));
}

#[test]
fn reducer_estimated_policy_is_explicit_and_unknown_is_never_admitted() {
    let t = timeline(
        vec![
            event(1000, Availability::Estimated(UtcMillis(1000)), 0, Some(0.0)),
            event(2000, Availability::Unknown, 0, Some(999.0)),
        ],
        vec![source(9000)],
    );
    assert!(
        t.state_at(ElapsedMillis(5000), ReplayPolicy::KnownOnly)
            .unwrap()
            .frame(&t)
            .unwrap()
            .samples()[0]
            .speed_kmh
            .value()
            .is_none()
    );
    let f = t
        .state_at(ElapsedMillis(5000), ReplayPolicy::EventTimeEstimates)
        .unwrap()
        .frame(&t)
        .unwrap();
    assert_eq!(f.samples()[0].speed_kmh.value().unwrap().get(), 0.0);
    assert!(
        !f.samples()[0].speed_kmh.available_by(UtcMillis(6000)),
        "estimate must not become known provenance"
    );
}

#[test]
fn reducer_corrections_and_duplicate_resolution_are_deterministic() {
    let a = event(1000, Availability::Known(UtcMillis(1000)), 0, Some(100.0));
    let b = event(1000, Availability::Known(UtcMillis(1000)), 1, Some(110.0));
    let c = event(1000, Availability::Known(UtcMillis(2000)), 2, Some(120.0));
    let sources = vec![source(8000), source(9000), source(10000)];
    let t = timeline(
        vec![c.clone(), a.clone(), b.clone(), b.clone()],
        sources.clone(),
    );
    let shuffled = timeline(vec![b, c, a], sources);
    assert_eq!(t.event_count(), 2);
    for (at, speed) in [(0, 110.0), (999, 110.0), (1000, 120.0)] {
        let frame = t
            .state_at(ElapsedMillis(at), ReplayPolicy::KnownOnly)
            .unwrap()
            .frame(&t)
            .unwrap();
        assert_eq!(
            frame,
            shuffled
                .state_at(ElapsedMillis(at), ReplayPolicy::KnownOnly)
                .unwrap()
                .frame(&shuffled)
                .unwrap()
        );
        assert_eq!(frame.samples()[0].speed_kmh.value().unwrap().get(), speed);
    }
}

#[test]
fn reducer_delayed_older_observation_cannot_overwrite_newer_state() {
    let t = timeline(
        vec![
            event(2000, Availability::Known(UtcMillis(2000)), 0, Some(200.0)),
            event(1000, Availability::Known(UtcMillis(3000)), 1, Some(100.0)),
        ],
        vec![source(8000), source(9000)],
    );
    assert_eq!(
        t.state_at(ElapsedMillis(2000), ReplayPolicy::KnownOnly)
            .unwrap()
            .frame(&t)
            .unwrap()
            .samples()[0]
            .speed_kmh
            .value()
            .unwrap()
            .get(),
        200.0
    );
}

#[test]
fn reducer_missing_replaces_old_value_without_turning_into_zero() {
    let zero = TelemetryValues {
        throttle: Some(Percent::new(0.0).unwrap()),
        ..TelemetryValues::default()
    };
    let first = Event::new(
        4,
        UtcMillis(1000),
        Availability::Known(UtcMillis(1000)),
        0,
        Payload::Telemetry(zero),
    )
    .unwrap();
    let t = timeline(
        vec![
            first,
            event(2000, Availability::Known(UtcMillis(2000)), 0, None),
        ],
        vec![source(9000)],
    );
    let f = t
        .state_at(ElapsedMillis(0), ReplayPolicy::KnownOnly)
        .unwrap()
        .frame(&t)
        .unwrap();
    assert_eq!(f.samples()[0].throttle.value().unwrap().get(), 0.0);
    let f = t
        .state_at(ElapsedMillis(1000), ReplayPolicy::KnownOnly)
        .unwrap()
        .frame(&t)
        .unwrap();
    assert!(f.samples()[0].throttle.value().is_none());
    assert!(f.samples()[0].sectors.iter().all(|s| s.value().is_none()));
}

#[test]
fn reducer_rejects_wrong_identity_and_time_travel() {
    let bad = Event::new(
        0,
        UtcMillis(1000),
        Availability::Unknown,
        0,
        Payload::Position(NonZeroU32::new(1)),
    );
    assert!(bad.is_err());
    let t = timeline(
        vec![event(
            1000,
            Availability::Known(UtcMillis(1000)),
            0,
            Some(1.0),
        )],
        vec![source(9000)],
    );
    let mut state = t
        .state_at(ElapsedMillis(10), ReplayPolicy::KnownOnly)
        .unwrap();
    assert!(state.advance_to(&t, ElapsedMillis(9)).is_err());
    let mut wrong = source(9000);
    wrong.session_key = 9636;
    assert!(
        Timeline::new(
            SessionId::new(Provider::OpenF1, "9590").unwrap(),
            UtcMillis(1000),
            vec![RosterDriver::new(4, "NOR", "Norris").unwrap()],
            vec![wrong],
            vec![]
        )
        .is_err()
    );
}

#[test]
fn reducer_tied_conflicts_are_unavailable_and_fractional_samples_never_appear_early() {
    let t = timeline(
        vec![
            event(1000, Availability::Known(UtcMillis(1000)), 0, Some(10.0)),
            event(1000, Availability::Known(UtcMillis(1000)), 1, Some(20.0)),
        ],
        vec![source(9000), source(9000)],
    );
    let frame = t
        .state_at(ElapsedMillis(0), ReplayPolicy::KnownOnly)
        .unwrap()
        .frame(&t)
        .unwrap();
    assert!(frame.samples()[0].speed_kmh.value().is_none());
    assert!(
        frame.samples()[0]
            .speed_kmh
            .reason()
            .unwrap()
            .contains("Conflicting")
    );
    assert_eq!(t.report().ambiguous_groups, 1);
    use sectorline::domain::normalize::utc_millis_ceiling;
    assert_eq!(
        utc_millis_ceiling("1970-01-01T00:00:01.000001Z").unwrap(),
        UtcMillis(1001)
    );
    assert_eq!(
        utc_millis_ceiling("1970-01-01T00:00:01.000000Z").unwrap(),
        UtcMillis(1000)
    );
    assert_eq!(
        utc_millis_ceiling("1970-01-01T01:00:01.000001+01:00").unwrap(),
        UtcMillis(1001)
    );
    let other = timeline(vec![], vec![]);
    let state = t
        .state_at(ElapsedMillis(0), ReplayPolicy::KnownOnly)
        .unwrap();
    assert!(state.frame(&other).is_err());
}

struct Fixture(std::path::PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn fixture_cache() -> (
    Fixture,
    sectorline::data::cache::SessionCache,
    serde_json::Value,
) {
    fixture_cache_with(|_| {})
}
fn fixture_cache_with(
    change: impl FnOnce(&mut serde_json::Value),
) -> (
    Fixture,
    sectorline::data::cache::SessionCache,
    serde_json::Value,
) {
    use sectorline::data::{
        cache::{CacheSpec, CacheWriter},
        openf1::{Channel, ChunkMetadata, Window},
    };
    let mut raw: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/reducer-monza.json")).unwrap();
    change(&mut raw);
    static FIXTURE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = Fixture(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .join(format!(
                "reducer-fixture-{}-{}",
                std::process::id(),
                FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            )),
    );
    let window: Window = serde_json::from_value(raw["window"].clone()).unwrap();
    let channels = vec![
        Channel::Drivers,
        Channel::Position,
        Channel::CarData,
        Channel::Location,
        Channel::Intervals,
        Channel::Laps,
    ];
    let spec = CacheSpec::new(
        9590,
        1244,
        2024,
        ("Race", "Race"),
        window,
        std::time::Duration::from_secs(300),
        channels.clone(),
    )
    .unwrap();
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    for (chunk, channel) in raw["channels"].as_array().unwrap().iter().zip(channels) {
        let mut metadata = chunk["metadata"].clone();
        metadata["channel"] = serde_json::to_value(channel).unwrap();
        let metadata: ChunkMetadata = serde_json::from_value(metadata).unwrap();
        writer
            .import(metadata, &serde_json::to_vec(&chunk["rows"]).unwrap())
            .unwrap();
    }
    let cache = writer.finish().unwrap();
    (root, cache, raw)
}

#[test]
fn reducer_real_monza_excerpts_match_independent_time_filtered_oracle() {
    use sectorline::{data::limits::Cancellation, domain::normalize::utc_millis_ceiling};
    let (_root, cache, raw) = fixture_cache();
    let t = Timeline::from_cache(&cache, &Cancellation::default(), &|_, _| {}).unwrap();
    let cars = &raw["channels"][2]["rows"].as_array().unwrap();
    let mut targets = Vec::new();
    for row in *cars {
        let at = utc_millis_ceiling(row["date"].as_str().unwrap()).unwrap().0;
        targets.extend([at - 1, at, at + 1]);
    }
    for at in targets {
        let frame = t
            .state_at(
                ElapsedMillis((at - t.origin().0) as u64),
                ReplayPolicy::EventTimeEstimates,
            )
            .unwrap()
            .frame(&t)
            .unwrap();
        for sample in frame.samples() {
            // This oracle independently filters raw JSON, not normalized events.
            let expected = cars
                .iter()
                .filter(|row| {
                    row["driver_number"].as_u64() == Some(sample.driver.number.get() as u64)
                })
                .filter(|row| utc_millis_ceiling(row["date"].as_str().unwrap()).unwrap().0 <= at)
                .max_by_key(|row| utc_millis_ceiling(row["date"].as_str().unwrap()).unwrap().0);
            assert_eq!(
                sample.speed_kmh.value().map(|v| v.get()),
                expected.and_then(|row| row["speed"].as_f64())
            );
            if let Some(row) = expected {
                assert_eq!(sample.drs.value().map(|d| d.0 as u64), row["drs"].as_u64());
            }
            let laps = raw["channels"][5]["rows"].as_array().unwrap();
            let expected = expected_completed_lap(laps, sample.driver.number.get() as u64, at);
            assert_eq!(
                sample.lap_number.value().map(|n| n.get() as u64),
                expected.and_then(|r| r["lap_number"].as_u64())
            );
            for (index, sector) in sample.sectors.iter().enumerate() {
                assert_eq!(
                    sector.value().map(|v| v.get()),
                    expected.and_then(|r| r[format!("duration_sector_{}", index + 1)].as_f64())
                );
                if sector.value().is_some() {
                    assert!(sector.timing().observed_at.unwrap().0 <= at);
                }
            }
        }
    }
}

#[test]
fn reducer_recorded_view_uses_real_frame_and_manual_time_steps_only() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use sectorline::{
        data::limits::Cancellation, domain::normalize::utc_millis_ceiling, replay::RecordedReplay,
    };
    let (_root, cache, raw) = fixture_cache();
    let mut replay = RecordedReplay::from_cache(
        &cache,
        "2024 Italian Grand Prix | Race | #9590".into(),
        &Cancellation::default(),
        &|_, _| {},
    )
    .unwrap();
    let at = utc_millis_ceiling(raw["channels"][2]["rows"][2]["date"].as_str().unwrap()).unwrap();
    replay
        .advance_by((at.0 - replay.timeline().origin().0) as u64)
        .unwrap();
    let demo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache");
    let mut app = sectorline::App::new(
        sectorline::TelemetryStream::load_from_dir(demo).unwrap(),
        false,
    );
    app.prepared_recording = Some(replay);
    for (width, height) in [(144, 44), (120, 36), (80, 24)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("RECORDED SNAPSHOT") && text.contains("PAUSED"));
        assert!(text.contains("original delivery times unknown"));
        assert!(text.contains("NOR #4"));
        assert!(!text.contains("DEMO") && !text.contains("BAKU") && !text.contains("AIR:"));
        assert!(text.contains("Home restart"));
    }
    let before = app.prepared_recording.as_ref().unwrap().frame().elapsed();
    app.tick(10_000.0);
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().frame().elapsed(),
        before
    );
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().frame().elapsed().0,
        before.0 + 1000
    );
}

#[test]
fn reducer_forward_steps_equal_reconstruction_and_location_respects_availability() {
    use sectorline::domain::normalize::Coordinates;
    let location = Event::new(
        4,
        UtcMillis(1000),
        Availability::Known(UtcMillis(1500)),
        1,
        Payload::Location(Coordinates::new(Some(-10.0), None, Some(0.0)).unwrap()),
    )
    .unwrap();
    let mut loc_source = source(9000);
    loc_source.channel = Channel::Location;
    let t = timeline(
        vec![
            event(1000, Availability::Known(UtcMillis(1000)), 0, Some(0.0)),
            location,
            event(3000, Availability::Known(UtcMillis(3000)), 0, Some(300.0)),
        ],
        vec![source(9000), loc_source],
    );
    let mut state = t
        .state_at(ElapsedMillis(0), ReplayPolicy::KnownOnly)
        .unwrap();
    assert!(state.location(&t, NonZeroU32::new(4).unwrap()).is_none());
    for at in [499, 500, 1000, 1999, 2000] {
        state.advance_to(&t, ElapsedMillis(at)).unwrap();
        assert_eq!(
            state.frame(&t).unwrap(),
            t.state_at(ElapsedMillis(at), ReplayPolicy::KnownOnly)
                .unwrap()
                .frame(&t)
                .unwrap()
        );
    }
    assert_eq!(
        state
            .location(&t, NonZeroU32::new(4).unwrap())
            .unwrap()
            .xyz(),
        (Some(-10.0), None, Some(0.0))
    );
}

#[test]
fn reducer_raw_fractional_time_integral_floats_and_quarantine_survive_cache_normalization() {
    use sectorline::{data::limits::Cancellation, domain::normalize::utc_millis_ceiling};
    let (_root, cache, raw) = fixture_cache_with(|raw| {
        let row = &mut raw["channels"][2]["rows"][0];
        let time = chrono::DateTime::parse_from_rfc3339(row["date"].as_str().unwrap()).unwrap()
            + chrono::Duration::microseconds(1);
        row["date"] = serde_json::json!(time.to_rfc3339_opts(chrono::SecondsFormat::Micros, true));
        row["speed"] = serde_json::json!(0.0);
        row["rpm"] = serde_json::json!(10000.0);
        row["n_gear"] = serde_json::json!(5.0);
        row["drs"] = serde_json::json!(9.0);
        row["throttle"] = serde_json::json!(104);
        row["brake"] = serde_json::json!(104);
    });
    let t = Timeline::from_cache(&cache, &Cancellation::default(), &|_, _| {}).unwrap();
    let at = utc_millis_ceiling(raw["channels"][2]["rows"][0]["date"].as_str().unwrap())
        .unwrap()
        .0;
    let early = t
        .state_at(
            ElapsedMillis((at - 1 - t.origin().0) as u64),
            ReplayPolicy::EventTimeEstimates,
        )
        .unwrap()
        .frame(&t)
        .unwrap();
    assert!(
        early
            .samples()
            .iter()
            .find(|c| c.driver.number.get() == 4)
            .unwrap()
            .speed_kmh
            .value()
            .is_none()
    );
    let frame = t
        .state_at(
            ElapsedMillis((at - t.origin().0) as u64),
            ReplayPolicy::EventTimeEstimates,
        )
        .unwrap()
        .frame(&t)
        .unwrap();
    let car = frame
        .samples()
        .iter()
        .find(|c| c.driver.number.get() == 4)
        .unwrap();
    assert_eq!(car.speed_kmh.value().unwrap().get(), 0.0);
    assert_eq!(car.rpm.value().unwrap().0, 10000);
    assert_eq!(car.gear.value().unwrap().get(), 5);
    assert_eq!(car.drs.value().unwrap().0, 9);
    assert!(car.throttle.value().is_none() && car.brake.value().is_none());
}

// Independent raw-row completion oracle; no normalized events or reducer helpers.
fn expected_completed_lap(
    rows: &[serde_json::Value],
    driver: u64,
    at: i64,
) -> Option<&serde_json::Value> {
    rows.iter()
        .filter(|r| r["driver_number"].as_u64() == Some(driver))
        .filter(|r| {
            let start = r["date_start"]
                .as_str()
                .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
                .map(|v| v.timestamp_millis());
            let sectors = (1..=3)
                .map(|i| r[format!("duration_sector_{i}")].as_f64())
                .collect::<Option<Vec<_>>>()
                .map(|v| v.iter().sum::<f64>());
            let duration = r["lap_duration"]
                .as_f64()
                .into_iter()
                .chain(sectors)
                .reduce(f64::max);
            let end = start
                .zip(duration)
                .map(|(s, d)| s + (d * 1000.0).ceil() as i64);
            let next = rows
                .iter()
                .find(|n| {
                    n["driver_number"] == r["driver_number"]
                        && n["lap_number"].as_u64() == r["lap_number"].as_u64().map(|n| n + 1)
                })
                .and_then(|n| n["date_start"].as_str())
                .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
                .map(|v| v.timestamp_millis());
            end.into_iter()
                .chain(next)
                .max()
                .is_some_and(|end| end <= at)
        })
        .max_by_key(|r| r["lap_number"].as_u64())
}
