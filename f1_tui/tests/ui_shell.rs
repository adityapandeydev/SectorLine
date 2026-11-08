use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use sectorline::{
    app::{App, AppAction},
    stream::TelemetryStream,
    ui::{
        View,
        layout::{Regions, SizeClass},
    },
};
fn app() -> App {
    App::new(
        TelemetryStream::load_from_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache"),
        )
        .unwrap(),
        false,
    )
}
fn capture(app: &App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    t.backend()
        .buffer()
        .content()
        .chunks(w as usize)
        .map(|row| {
            row.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn ui_shell_geometry_matches_the_eight_row_contract() {
    for (w, h, class) in [
        (144, 44, SizeClass::Wide),
        (120, 36, SizeClass::Standard),
        (80, 24, SizeClass::Compact),
    ] {
        let r = Regions::new(Rect::new(2, 3, w, h));
        assert_eq!(r.class, class);
        assert_eq!(r.header.height, 2);
        assert_eq!(r.tabs.height, 1);
        assert_eq!(r.body.height, h - 8);
        assert_eq!(r.timeline.height, 3);
        assert_eq!(r.status.height, 1);
        assert_eq!(r.help.height, 1);
        assert_eq!(r.help.bottom(), 3 + h);
        assert_eq!(r.body.width, w);
        let (table, context, detail) = r.timing_columns();
        assert_eq!(table.width, if w >= 120 { 44 } else { w });
        if class == SizeClass::Wide {
            assert_eq!(detail.unwrap().width, 32);
            assert_eq!(context.unwrap().width, 66);
        } else if class == SizeClass::Standard {
            assert_eq!(context.unwrap().width, 75);
            assert!(detail.is_none());
        }
    }
}
#[test]
fn ui_shell_tabs_work_without_changing_playback_or_driver() {
    let mut app = app();
    let selected = app.stream.selected_driver_code.clone();
    let frame = app.stream.current_frame_idx;
    for expected in [
        View::Driver,
        View::Conditions,
        View::Track,
        View::Events,
        View::Timing,
    ] {
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            AppAction::None
        );
        assert_eq!(app.view, expected);
    }
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(app.view, View::Events);
    assert_eq!(app.stream.selected_driver_code, selected);
    assert_eq!(app.stream.current_frame_idx, frame);
}
#[test]
fn ui_shell_demo_is_honest_and_has_stable_header_timeline_status_help() {
    let app = app();
    for (w, h) in [(144, 44), (120, 36), (80, 24)] {
        let text = capture(&app, w, h);
        assert!(text.contains("Sector Line"));
        assert!(text.contains("DEMO"));
        assert!(text.contains("Generated"));
        assert!(text.contains("[Timing]"));
        assert!(text.contains("Home restart"));
        assert!(text.contains("Space play/pause"));
        assert!(!text.contains("SPEEDOMETER"));
        assert!(!text.contains("LIVE"));
        assert!(text.lines().last().unwrap().contains("q quit"));
    }
}
#[test]
fn ui_shell_tiny_sizes_do_not_panic_and_explain_the_minimum() {
    let app = app();
    for w in [0, 1, 40, 79] {
        for h in [0, 1, 10, 23] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| app.render(f)).unwrap();
        }
    }
    assert!(capture(&app, 70, 15).contains("80x24"));
}

fn recorded_app() -> App {
    use sectorline::{
        data::openf1::{Channel, ChunkMetadata},
        domain::{
            conditions::*,
            normalize::{Event, Payload, RosterDriver, TelemetryValues},
            stints::*,
            *,
        },
        replay::{RecordedReplay, timeline::Timeline},
    };
    let mut app = app();
    let sources = [Channel::Position, Channel::CarData]
        .map(|channel| ChunkMetadata {
            channel,
            session_key: 9590,
            meeting_key: 1244,
            window: None,
            source_url: "https://api.openf1.org/v1/test".into(),
            retrieved_at: UtcMillis(900_000),
            status: 200,
            attempts: 1,
        })
        .to_vec();
    let mut roster = vec![
        RosterDriver::new(4, "NOR", "Fixture Norris").unwrap(),
        RosterDriver::new(16, "LEC", "Fixture Leclerc").unwrap(),
    ];
    for i in 0..20 {
        roster.push(
            RosterDriver::new(100 + i, &format!("R{i:02}"), &format!("Fixture {i}")).unwrap(),
        );
    }
    let mut events = roster
        .iter()
        .enumerate()
        .map(|(i, d)| {
            Event::new(
                d.number.get(),
                UtcMillis(0),
                Availability::Estimated(UtcMillis(0)),
                0,
                Payload::Position(std::num::NonZeroU32::new(i as u32 + 1)),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    events.push(
        Event::new(
            4,
            UtcMillis(90_000),
            Availability::Estimated(UtcMillis(90_000)),
            1,
            Payload::Telemetry(TelemetryValues {
                speed: Some(SpeedKmh::new(285.0).unwrap()),
                rpm: Some(Rpm(11_000)),
                gear: Some(Gear::new(8).unwrap()),
                throttle: Some(Percent::new(95.0).unwrap()),
                brake: Some(BrakeState::Released),
                drs: Some(DrsCode(12)),
            }),
        )
        .unwrap(),
    );
    events.push(
        Event::new(
            4,
            UtcMillis(600_000),
            Availability::Estimated(UtcMillis(600_000)),
            0,
            Payload::Position(std::num::NonZeroU32::new(1)),
        )
        .unwrap(),
    );
    let read = |at, value| {
        Reading::new(
            UtcMillis(at),
            Availability::Estimated(UtcMillis(at)),
            UtcMillis(900_000),
            Some(value),
        )
        .unwrap()
    };
    let mut conditions = ConditionsArchive::default();
    conditions.weather.push(read(
        80_000,
        Weather {
            air_c: Some(24.0),
            track_c: Some(31.5),
            rain: Some(true),
            humidity: Some(85.0),
            wind_mps: Some(3.0),
            wind_degrees: Some(90.0),
            ..Default::default()
        },
    ));
    conditions.control.push(
        Reading::new(
            UtcMillis(80_000),
            Availability::Estimated(UtcMillis(80_000)),
            UtcMillis(900_000),
            Some(Control {
                scope: Scope::Track,
                kind: ControlKind::Flag(Flag::Red),
            }),
        )
        .unwrap(),
    );
    conditions.control.push(
        Reading::new(
            UtcMillis(85_000),
            Availability::Estimated(UtcMillis(85_000)),
            UtcMillis(900_000),
            Some(Control {
                scope: Scope::Sector(2),
                kind: ControlKind::Flag(Flag::Yellow),
            }),
        )
        .unwrap(),
    );
    conditions.stints.entry(4).or_default().push(
        Reading::new(
            UtcMillis(0),
            Availability::Estimated(UtcMillis(0)),
            UtcMillis(900_000),
            Some(Stint {
                number: 1,
                lap_start: 1,
                compound: Some("INTERMEDIATE".into()),
                age_at_start: Some(3),
            }),
        )
        .unwrap(),
    );
    conditions.pits.entry(4).or_default().push(
        Reading::new(
            UtcMillis(60_000),
            Availability::Estimated(UtcMillis(60_000)),
            UtcMillis(900_000),
            Some(PitRecord {
                lap: Some(1),
                lane_seconds: Some(22.1),
                stop_seconds: Some(2.4),
            }),
        )
        .unwrap(),
    );
    let timeline = Timeline::new(
        SessionId::new(Provider::OpenF1, "9590").unwrap(),
        UtcMillis(0),
        roster,
        sources,
        events,
    )
    .unwrap()
    .with_conditions(conditions);
    let mut replay = RecordedReplay::new(timeline, "SHELL FIXTURE - synthetic".into()).unwrap();
    let context = sectorline::domain::capabilities::SessionContext::from_names(
        replay.timeline().session().clone(),
        "Race",
        "Race",
    );
    replay = replay.with_context(context).unwrap();
    replay.seek(ElapsedMillis(90_000)).unwrap();
    app.prepared_recording = Some(replay);
    app.theme.color = true;
    app
}
#[test]
fn ui_shell_recorded_snapshots_keep_exact_identity_and_source_boundaries() {
    let mut app = recorded_app();
    for (w, h, name) in [
        (144, 44, "wide"),
        (120, 36, "standard"),
        (80, 24, "compact"),
    ] {
        let text = capture(&app, w, h);
        assert!(text.contains("OpenF1 #9590 | RECORDED SNAPSHOT"));
        assert!(text.contains("SHELL FIXTURE - synthetic"));
        assert!(text.contains("PAUSED"));
        assert!(text.contains("+01:30.000 / 10:00.000"));
        assert!(text.contains("original delivery times unknown"));
        assert!(text.contains("NOR #4"));
        assert!(!text.contains("DEMO"));
        if w >= 120 {
            assert!(text.contains("Track Red"));
        } else {
            assert!(!text.contains("Track Red"));
        }
        if let Some(directory) = std::env::var_os("SECTORLINE_CAPTURE_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(format!("t13-{name}.txt")), &text).unwrap();
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| app.render(f)).unwrap();
            let cells=terminal.backend().buffer().content().iter().map(|c|serde_json::json!({"s":c.symbol(),"fg":format!("{:?}",c.fg),"bg":format!("{:?}",c.bg),"m":format!("{:?}",c.modifier)})).collect::<Vec<_>>();
            std::fs::write(
                directory.join(format!("t13-{name}.json")),
                serde_json::to_vec(&serde_json::json!({"width":w,"height":h,"cells":cells}))
                    .unwrap(),
            )
            .unwrap();
        }
    }
    let mut terminal = Terminal::new(TestBackend::new(144, 44)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|c| c.fg == ratatui::style::Color::Red && c.symbol() == "R"),
        "Fresh red flag gets semantic red as well as its label"
    );
    app.view = View::Driver;
    let text = capture(&app, 80, 24);
    assert!(text.contains("phase unknown"));
    assert!(text.contains("Throttle 95%"));
    app.view = View::Conditions;
    assert!(capture(&app, 80, 24).contains("Track Red"));
}
#[test]
fn ui_shell_loading_error_status_does_not_cover_source_timeline_or_quit() {
    let mut app = recorded_app();
    app.begin_loading(7);
    app.load_message = Some("Verifying source chunks".into());
    for error in [false, true] {
        app.load_error = error;
        if error {
            app.load_id = None;
            app.load_message = Some(format!(
                "{} END-OF-ERROR",
                "A detailed failure message. ".repeat(20)
            ));
        }
        let text = capture(&app, 80, 24);
        assert!(text.contains(if error { "Error:" } else { "Loading:" }));
        assert!(text.contains("OpenF1 #9590"));
        assert!(text.contains("Home restart"));
        assert!(text.lines().last().unwrap().contains("q quit"));
    }
    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert!(app.show_help);
    // The help now includes driver-history and event-navigation controls.
    for _ in 0..128 {
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    let text = capture(&app, 80, 24);
    assert!(text.contains("END-OF-ERROR"));
    assert_eq!(
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        AppAction::None
    );
    assert!(!app.show_help);
    assert_eq!(
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        AppAction::RetryLoad
    );
}
#[test]
fn ui_shell_no_color_preserves_selection_and_keyboard_focus() {
    use ratatui::style::{Color, Modifier};
    let mut app = recorded_app();
    app.theme.color = false;
    let mut terminal = Terminal::new(TestBackend::new(144, 44)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
    );
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|c| c.modifier.contains(Modifier::REVERSED))
    );
    assert!(
        capture(&app, 80, 24)
            .lines()
            .any(|line| line.starts_with('>') && line.contains("NOR #4")),
        "No-color marker stays on selected driver after numeric right alignment"
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    let view = app.view;
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.view, view);
    assert!(capture(&app, 80, 24).contains("Sessions"));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.navigation_input.as_deref(), Some("t"));
    assert_eq!(app.view, view);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
    assert_eq!(app.panel_scroll, 1);
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.panel_scroll, 0);
}
#[test]
fn ui_shell_resizing_and_help_keep_playback_state_and_selection_intact() {
    let mut app = recorded_app();
    let before = app.prepared_recording.as_ref().unwrap().frame().clone();
    let selected = app.prepared_recording.as_ref().unwrap().selected();
    for (w, h) in [(144, 44), (80, 24), (120, 36), (200, 50), (80, 24)] {
        let _ = capture(&app, w, h);
    }
    assert_eq!(app.prepared_recording.as_ref().unwrap().frame(), &before);
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().selected(),
        selected
    );
    app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(
        app.prepared_recording.as_ref().unwrap().status(),
        sectorline::replay::clock::PlaybackStatus::Paused
    );
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.show_help);
    assert!(!app.should_quit);
}
