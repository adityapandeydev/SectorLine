use sectorline::{
    data::{
        limits::Cancellation,
        openf1::{Channel, ChunkMetadata},
    },
    domain::{
        Availability, ElapsedMillis, Provider, SessionId, SpeedKmh, UtcMillis,
        normalize::{Coordinates, Event, Payload, RosterDriver, TelemetryValues},
    },
    replay::{
        RecordedReplay,
        checkpoint::Checkpoints,
        clock::{PlaybackStatus, Speed},
        navigation::{LapIndex, Navigation},
        timeline::{ReplayPolicy, Timeline},
    },
};
use std::{num::NonZeroU32, time::Duration};
fn timeline() -> Timeline {
    let source = |channel| ChunkMetadata {
        channel,
        session_key: 9590,
        meeting_key: 1244,
        window: None,
        source_url: "https://api.openf1.org/v1/test".into(),
        retrieved_at: UtcMillis(900_000),
        status: 200,
        attempts: 1,
    };
    let mut events = Vec::new();
    for i in 0..1500_i64 {
        let t = 1000 + i * 137;
        events.push(
            Event::new(
                4,
                UtcMillis(t),
                if i % 17 == 0 {
                    Availability::Unknown
                } else if i % 3 == 0 {
                    Availability::Known(UtcMillis(t + 59))
                } else {
                    Availability::Estimated(UtcMillis(t))
                },
                0,
                Payload::Telemetry(TelemetryValues {
                    speed: if i % 19 == 0 {
                        None
                    } else {
                        Some(SpeedKmh::new((i % 300) as f64).unwrap())
                    },
                    ..Default::default()
                }),
            )
            .unwrap(),
        );
        if i % 23 == 0 {
            events.push(
                Event::new(
                    4,
                    UtcMillis(t),
                    Availability::Estimated(UtcMillis(t)),
                    1,
                    Payload::Location(Coordinates::new(Some(i as f64), None, Some(0.0)).unwrap()),
                )
                .unwrap(),
            );
        }
    }
    // Same-time conflict, followed by a later-available correction to an older observation.
    for value in [10.0, 20.0] {
        events.push(
            Event::new(
                4,
                UtcMillis(60_999),
                Availability::Known(UtcMillis(61_000)),
                0,
                Payload::Telemetry(TelemetryValues {
                    speed: Some(SpeedKmh::new(value).unwrap()),
                    ..Default::default()
                }),
            )
            .unwrap(),
        );
    }
    events.push(
        Event::new(
            4,
            UtcMillis(200_000),
            Availability::Known(UtcMillis(207_000)),
            0,
            Payload::Telemetry(TelemetryValues {
                speed: None,
                ..Default::default()
            }),
        )
        .unwrap(),
    );
    Timeline::new(
        SessionId::new(Provider::OpenF1, "9590").unwrap(),
        UtcMillis(1000),
        vec![
            RosterDriver::new(4, "NOR", "Lando Norris").unwrap(),
            RosterDriver::new(16, "LEC", "Charles Leclerc").unwrap(),
        ],
        vec![source(Channel::CarData), source(Channel::Location)],
        events,
    )
    .unwrap()
}
fn replay() -> RecordedReplay {
    let mut laps = LapIndex::default();
    laps.insert(4, 2, UtcMillis(61_000));
    laps.insert(16, 2, UtcMillis(62_000));
    RecordedReplay::with_navigation(
        timeline(),
        "Seek fixture".into(),
        laps,
        &Cancellation::default(),
    )
    .unwrap()
}
fn encoded(frame: &sectorline::domain::PlaybackFrame) -> serde_json::Value {
    serde_json::to_value(frame).unwrap()
}
#[test]
fn seek_checkpoints_equal_fresh_replay_at_100_random_targets_and_boundaries() {
    let t = timeline();
    let checkpoints = Checkpoints::build(&t, &Cancellation::default()).unwrap();
    assert!(checkpoints.len() > 1 && checkpoints.len() <= 512);
    let mut seed = 42_u64;
    let mut targets = vec![
        0, 1, 29_999, 30_000, 30_001, 59_999, 60_000, 60_001, 206_000,
    ];
    for _ in 0..100 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        targets.push(seed % (t.end().0 + 1));
    }
    let mut r = replay();
    r.select(1);
    let selected = r.selected();
    for target in targets {
        let target = ElapsedMillis(target);
        let expected = t
            .state_at(target, ReplayPolicy::EventTimeEstimates)
            .unwrap();
        let restored = checkpoints.restore(&t, target).unwrap();
        assert_eq!(
            encoded(&expected.frame(&t).unwrap()),
            encoded(&restored.frame(&t).unwrap())
        );
        assert_eq!(
            expected.location(&t, NonZeroU32::new(4).unwrap()),
            restored.location(&t, NonZeroU32::new(4).unwrap())
        );
        r.seek(target).unwrap();
        assert_eq!(encoded(r.frame()), encoded(&expected.frame(&t).unwrap()));
        assert_eq!(r.selected(), selected);
        assert!(
            r.frame()
                .samples()
                .iter()
                .all(|s| s.sectors.iter().all(|v| v.value().is_none()))
        );
    }
    assert!(checkpoints.restore(&timeline(), ElapsedMillis(1)).is_err());
}
#[test]
fn seek_pauses_reanchors_and_retains_speed_without_future_samples() {
    let mut r = replay();
    r.set_speed(Speed::Five).unwrap();
    r.toggle().unwrap();
    r.update_at(Duration::from_secs(10)).unwrap();
    r.seek(ElapsedMillis(100)).unwrap();
    assert_eq!(r.status(), PlaybackStatus::Paused);
    assert_eq!(r.speed(), Speed::Five);
    r.update_at(Duration::from_secs(30)).unwrap();
    assert_eq!(r.frame().elapsed().0, 100);
    r.toggle().unwrap();
    r.update_at(Duration::from_millis(30_100)).unwrap();
    assert_eq!(r.frame().elapsed().0, 600);
    r.seek(r.timeline().end()).unwrap();
    assert_eq!(r.status(), PlaybackStatus::Ended);
    r.seek(ElapsedMillis(0)).unwrap();
    assert_eq!(r.status(), PlaybackStatus::Paused);
    let before = encoded(r.frame());
    assert!(r.seek(ElapsedMillis(u64::MAX)).is_err());
    assert_eq!(encoded(r.frame()), before);
}
#[test]
fn seek_navigation_has_strict_units_signed_offsets_and_selected_driver_laps() {
    let mut r = replay();
    r.navigate(Navigation::Lap(2)).unwrap();
    assert_eq!(r.frame().elapsed().0, 60_000);
    r.select(1);
    r.navigate(Navigation::Lap(2)).unwrap();
    assert_eq!(r.frame().elapsed().0, 61_000);
    assert!(r.navigate(Navigation::Lap(999)).is_err());
    r.navigate(Navigation::parse("offset -00:12.5").unwrap())
        .unwrap();
    r.navigate(Navigation::parse("recording 01:12.500").unwrap())
        .unwrap();
    assert_eq!(r.frame().elapsed().0, 60_000);
    r.navigate(Navigation::parse("offset +00:02").unwrap())
        .unwrap();
    r.navigate(Navigation::parse("recording 00:03").unwrap())
        .unwrap();
    assert_eq!(r.frame().elapsed().0, 5000);
    r.navigate(Navigation::Offset(-10_000)).unwrap();
    assert!(r.navigate(Navigation::Recording(1)).is_err());
    assert_eq!(
        Navigation::parse("time 120:00.001").unwrap(),
        Navigation::Time(7_200_001)
    );
    for input in [
        "time -00:01",
        "time 00:60",
        "time 1",
        "time 00:00.",
        "time 00:01.1234",
        "time 00:01 extra",
        "lap 0",
        "offset 00:01",
        "offset --00:01",
        "time 9999999999999999999:00",
        "recording NaN",
    ] {
        assert!(Navigation::parse(input).is_err(), "{input}");
    }
}
#[test]
fn seek_lap_index_rejects_conflicts_missing_and_outside_coverage() {
    let mut laps = LapIndex::default();
    laps.insert(4, 1, UtcMillis(999));
    laps.insert(4, 2, UtcMillis(2000));
    laps.insert(4, 2, UtcMillis(2000));
    assert_eq!(
        laps.target(4, 2, UtcMillis(1000), ElapsedMillis(3000))
            .unwrap(),
        ElapsedMillis(1000)
    );
    laps.insert(4, 2, UtcMillis(2001));
    laps.insert(4, 2, UtcMillis(2000));
    for lap in [1, 2, 3] {
        assert!(
            laps.target(4, lap, UtcMillis(1000), ElapsedMillis(3000))
                .is_err()
        );
    }
    laps.insert(4, 4, UtcMillis(5000));
    assert!(
        laps.target(4, 4, UtcMillis(1000), ElapsedMillis(3000))
            .is_err()
    );
}
#[test]
fn seek_cancelled_checkpoint_build_stops() {
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(Checkpoints::build(&timeline(), &cancel).is_err());
}
#[test]
fn seek_command_editor_is_bounded_pauses_and_retains_errors() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = sectorline::app::App::new(
        sectorline::stream::TelemetryStream::load_from_dir(std::path::Path::new(
            "../python_bridge/cache",
        ))
        .or_else(|_| {
            sectorline::stream::TelemetryStream::load_from_dir(std::path::Path::new(
                "python_bridge/cache",
            ))
        })
        .unwrap(),
        false,
    );
    app.prepared_recording = Some(replay());
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Char('g')));
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().status(),
        PlaybackStatus::Paused
    );
    for c in "time 00:60".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.navigation_error.is_some());
    assert!(app.navigation_input.is_some());
    app.handle_key(key(KeyCode::Esc));
    assert!(app.navigation_input.is_none());
    app.handle_key(key(KeyCode::Char('g')));
    for c in "time 01:00".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.navigation_input.is_none());
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().frame().elapsed().0,
        60_000
    );
    app.handle_key(key(KeyCode::Left));
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().frame().elapsed().0,
        59_000
    );
    app.handle_key(key(KeyCode::Char('g')));
    for _ in 0..200 {
        app.handle_key(key(KeyCode::Char('x')));
    }
    assert_eq!(app.navigation_input.as_ref().unwrap().len(), 64);
    app.handle_key(key(KeyCode::Esc));
    for (width, height) in [(144, 44), (120, 36), (80, 24)] {
        app.handle_key(key(KeyCode::Char('g')));
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| app.render(f)).unwrap();
        let text = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Go to >"));
        assert!(text.contains("Timeline = recording + offset"));
        app.handle_key(key(KeyCode::Esc));
    }
}
