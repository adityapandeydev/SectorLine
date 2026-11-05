use sectorline::{
    domain::ElapsedMillis,
    replay::clock::{PlaybackClock, PlaybackStatus, Speed},
};
use std::time::Duration;
fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

#[test]
fn replay_clock_paused_start_resume_and_pause_settle_exact_segments() {
    let mut clock = PlaybackClock::new(ElapsedMillis(10_000));
    clock.sync(ms(1000)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(0));
    clock.toggle(ms(1100)).unwrap();
    clock.sync(ms(1350)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(250));
    clock.toggle(ms(1400)).unwrap();
    clock.sync(ms(5000)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(300));
    clock.toggle(ms(5100)).unwrap();
    clock.sync(ms(5300)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(500));
}

#[test]
fn replay_clock_speed_changes_preserve_time_at_old_speed() {
    let mut clock = PlaybackClock::new(ElapsedMillis(50_000));
    clock.toggle(ms(0)).unwrap();
    clock.set_speed(Speed::Double, ms(100)).unwrap();
    clock.sync(ms(200)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(300));
    clock.set_speed(Speed::Half, ms(300)).unwrap();
    clock.sync(ms(500)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(600));
    clock.toggle(ms(600)).unwrap();
    clock.set_speed(Speed::Ten, ms(1000)).unwrap();
    clock.sync(ms(5000)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(650));
}

#[test]
fn replay_clock_fractional_ticks_do_not_drift_or_depend_on_frame_cadence() {
    let mut fragmented = PlaybackClock::new(ElapsedMillis(10_000));
    let mut whole = PlaybackClock::new(ElapsedMillis(10_000));
    for c in [&mut fragmented, &mut whole] {
        c.set_speed(Speed::Quarter, Duration::ZERO).unwrap();
        c.toggle(Duration::ZERO).unwrap();
    }
    for tick in 1..=10_000 {
        fragmented.sync(Duration::from_micros(tick * 17)).unwrap();
    }
    whole.sync(Duration::from_micros(170_000)).unwrap();
    assert_eq!(fragmented.position(), whole.position());
    assert_eq!(whole.position(), ElapsedMillis(42));
    fragmented.sync(ms(200)).unwrap();
    assert_eq!(fragmented.position(), ElapsedMillis(50));
}

#[test]
fn replay_clock_ends_without_wrapping_and_restart_is_deliberate() {
    let mut clock = PlaybackClock::new(ElapsedMillis(100));
    clock.toggle(ms(0)).unwrap();
    clock.sync(ms(1000)).unwrap();
    assert_eq!(clock.status(), PlaybackStatus::Ended);
    assert_eq!(clock.position(), ElapsedMillis(100));
    clock.toggle(ms(2000)).unwrap();
    clock.set_speed(Speed::Five, ms(3000)).unwrap();
    clock.sync(ms(9000)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(100));
    clock.restart(ms(10_000)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(0));
    assert_eq!(clock.status(), PlaybackStatus::Paused);
    clock.toggle(ms(10_000)).unwrap();
    clock.sync(ms(10_010)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(50));
}

#[test]
fn replay_clock_rejects_backwards_monotonic_input_without_mutation() {
    let mut clock = PlaybackClock::new(ElapsedMillis(1000));
    clock.toggle(ms(100)).unwrap();
    clock.sync(ms(200)).unwrap();
    assert!(clock.set_speed(Speed::Ten, ms(199)).is_err());
    assert_eq!(clock.position(), ElapsedMillis(100));
    assert_eq!(clock.speed(), Speed::Normal);
    assert!(clock.restart(ms(0)).is_err());
    assert_eq!(clock.position(), ElapsedMillis(100));
}

#[test]
fn replay_clock_zero_duration_and_manual_steps_have_explicit_states() {
    let mut empty = PlaybackClock::new(ElapsedMillis(0));
    empty.toggle(ms(100)).unwrap();
    assert_eq!(empty.status(), PlaybackStatus::Ended);
    let mut clock = PlaybackClock::new(ElapsedMillis(5000));
    clock.toggle(ms(0)).unwrap();
    clock.step(1000, ms(500)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(1500));
    assert_eq!(clock.status(), PlaybackStatus::Paused);
    clock.sync(ms(9000)).unwrap();
    assert_eq!(clock.position(), ElapsedMillis(1500));
}

fn app() -> sectorline::App {
    use sectorline::{
        data::openf1::{Channel, ChunkMetadata},
        domain::{
            Availability, Provider, SessionId, SpeedKmh, UtcMillis,
            normalize::{Event, Payload, RosterDriver, TelemetryValues},
        },
        replay::{RecordedReplay, timeline::Timeline},
    };
    let source = ChunkMetadata {
        channel: Channel::CarData,
        session_key: 9590,
        meeting_key: 1244,
        window: None,
        source_url: "https://api.openf1.org/v1/car_data?session_key=9590".into(),
        retrieved_at: UtcMillis(9000),
        status: 200,
        attempts: 1,
    };
    let events = [(1000, 0.0), (1257, 100.0), (2300, 200.0)]
        .into_iter()
        .map(|(at, speed)| {
            Event::new(
                4,
                UtcMillis(at),
                Availability::Known(UtcMillis(at)),
                0,
                Payload::Telemetry(TelemetryValues {
                    speed: Some(SpeedKmh::new(speed).unwrap()),
                    ..TelemetryValues::default()
                }),
            )
            .unwrap()
        })
        .collect();
    let timeline = Timeline::new(
        SessionId::new(Provider::OpenF1, "9590").unwrap(),
        UtcMillis(1000),
        vec![RosterDriver::new(4, "NOR", "Norris").unwrap()],
        vec![source],
        events,
    )
    .unwrap();
    let demo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache");
    let mut app = sectorline::App::new(
        sectorline::TelemetryStream::load_from_dir(demo).unwrap(),
        false,
    );
    app.prepared_recording = Some(RecordedReplay::new(timeline, "Monza".into()).unwrap());
    app
}

#[test]
fn replay_clock_app_controls_render_states_and_no_idle_redraw_work() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    let mut app = app();
    assert!(!app.update_at(ms(1000)));
    assert!(!app.needs_playback_ticks());
    app.handle_key(key(KeyCode::Char(' ')));
    assert!(app.needs_playback_ticks());
    assert!(app.update_at(ms(1256)));
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().frame().samples()[0]
            .speed_kmh
            .value()
            .unwrap()
            .get(),
        0.0
    );
    app.update_at(ms(1257));
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().frame().samples()[0]
            .speed_kmh
            .value()
            .unwrap()
            .get(),
        100.0
    );
    app.handle_key(key(KeyCode::Char('2')));
    app.update_at(ms(1357));
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().frame().elapsed(),
        ElapsedMillis(457)
    );
    app.handle_key(key(KeyCode::Char(' ')));
    assert!(!app.update_at(ms(5000)));
    for (code, expected) in [(KeyCode::Char(' '), "PLAYING"), (KeyCode::Home, "PAUSED")] {
        app.handle_key(key(code));
        for (width, height) in [(144, 44), (120, 36), (80, 24)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| app.render(f)).unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains(expected));
            assert!(text.contains("Space play/pause"));
        }
    }
    app.handle_key(key(KeyCode::Char(' ')));
    app.update_at(ms(10_000));
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().status(),
        PlaybackStatus::Ended
    );
    assert!(!app.needs_playback_ticks());
    assert!(!app.update_at(ms(20_000)));
    app.handle_key(key(KeyCode::Home));
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().frame().elapsed(),
        ElapsedMillis(0)
    );
    app.handle_key(key(KeyCode::Char(' ')));
    app.update_at(ms(20_100));
    app.handle_key(key(KeyCode::Char('s')));
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().status(),
        PlaybackStatus::Paused
    );
}

#[test]
fn replay_clock_demo_uses_irregular_timestamps_and_never_wraps() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache");
    let mut demo = sectorline::TelemetryStream::load_from_dir(dir).unwrap();
    demo.frames.truncate(3);
    for (frame, time) in demo.frames.iter_mut().zip([1000, 1257, 2300]) {
        frame.timestamp_ms = time;
    }
    demo.tick(256.0);
    assert_eq!(demo.current_frame_idx, 0);
    demo.tick(1.0);
    assert_eq!(demo.current_frame_idx, 1);
    demo.tick(10_000.0);
    assert_eq!(demo.current_frame_idx, 2);
    assert!(demo.has_ended && demo.is_paused);
    demo.toggle_pause();
    demo.tick(10_000.0);
    assert_eq!(demo.current_frame_idx, 2);
    demo.restart();
    assert_eq!(demo.current_frame_idx, 0);
    assert!(demo.is_paused && !demo.has_ended);
}
