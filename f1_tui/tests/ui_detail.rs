use sectorline::{
    app::App,
    data::openf1::{Channel, ChunkMetadata},
    domain::{
        conditions::*,
        normalize::{Event, Payload, RosterDriver, TelemetryValues},
        stints::PitRecord,
        timing::{Eligibility, LapTiming},
        *,
    },
    replay::{RecordedReplay, timeline::Timeline},
};
use std::num::NonZeroU32;
fn replay() -> RecordedReplay {
    replay_variant(false, false)
}
fn replay_variant(missing_sector: bool, pit_out: bool) -> RecordedReplay {
    let source = |channel| ChunkMetadata {
        channel,
        session_key: 9590,
        meeting_key: 1244,
        window: None,
        source_url: format!(
            "https://api.openf1.org/v1/{}?session_key=9590",
            channel.endpoint()
        ),
        retrieved_at: UtcMillis(200000),
        status: 200,
        attempts: 1,
    };
    let mut events = Vec::new();
    for (at, available, speed, gear, brake, drs) in [
        (
            1000,
            2000,
            Some(200.0),
            Some(0),
            Some(BrakeState::Pressed),
            Some(9),
        ),
        (3000, 3000, None, None, None, None),
        (
            4000,
            4000,
            Some(210.0),
            Some(8),
            Some(BrakeState::Released),
            Some(12),
        ),
        (
            120500,
            120500,
            Some(280.0),
            Some(8),
            Some(BrakeState::Pressed),
            Some(8),
        ),
    ] {
        events.push(
            Event::new(
                4,
                UtcMillis(at),
                Availability::Known(UtcMillis(available)),
                0,
                Payload::Telemetry(TelemetryValues {
                    speed: speed.map(|s| SpeedKmh::new(s).unwrap()),
                    gear: gear.map(|v| Gear::new(v).unwrap()),
                    brake,
                    drs: drs.map(DrsCode),
                    throttle: Some(Percent::new(0.0).unwrap()),
                    rpm: Some(Rpm(10000)),
                }),
            )
            .unwrap(),
        );
    }
    for n in 0..37 {
        let at = UtcMillis(111000 + n * 250);
        events.push(
            Event::new(
                4,
                at,
                Availability::Known(at),
                0,
                Payload::Telemetry(TelemetryValues {
                    speed: Some(SpeedKmh::new(240.0 + 30.0 * (n as f64 / 5.0).sin()).unwrap()),
                    throttle: Some(Percent::new(50.0 + 30.0 * (n as f64 / 4.0).cos()).unwrap()),
                    gear: Some(Gear::new(7).unwrap()),
                    brake: Some(BrakeState::Released),
                    drs: Some(DrsCode(12)),
                    rpm: Some(Rpm(10000)),
                }),
            )
            .unwrap(),
        );
    }
    for (n, t, d, eligibility) in [
        (1, 60000, 60.0, Eligibility::Eligible),
        (2, 120000, 63.0, Eligibility::Eligible),
        (3, 180000, 57.0, Eligibility::Unknown),
    ] {
        events.push(
            Event::new(
                4,
                UtcMillis(t),
                Availability::Known(UtcMillis(t)),
                1,
                Payload::Lap(Box::new(LapTiming {
                    number: NonZeroU32::new(n).unwrap(),
                    duration: Some(Seconds::new(d).unwrap()),
                    sectors: if missing_sector && n == 2 {
                        [Some(Seconds::new(d / 3.0).unwrap()), None, None]
                    } else {
                        [Some(Seconds::new(d / 3.0).unwrap()); 3]
                    },
                    pit_out: Some(pit_out && n == 2),
                    eligibility,
                })),
            )
            .unwrap(),
        );
    }
    for (at, available, eligibility) in [
        (130000, 135000, Eligibility::Ineligible),
        (160000, 165000, Eligibility::Eligible),
    ] {
        events.push(
            Event::new(
                4,
                UtcMillis(at),
                Availability::Known(UtcMillis(available)),
                2,
                Payload::LapDecision {
                    lap: NonZeroU32::new(1).unwrap(),
                    eligibility,
                },
            )
            .unwrap(),
        );
    }
    let mut conditions = ConditionsArchive::default();
    conditions.control.push(
        Reading::new(
            UtcMillis(100000),
            Availability::Known(UtcMillis(140000)),
            UtcMillis(200000),
            Some(Control {
                scope: Scope::Track,
                kind: ControlKind::Flag(Flag::Red),
            }),
        )
        .unwrap(),
    );
    conditions.control.push(
        Reading::new(
            UtcMillis(110000),
            Availability::Known(UtcMillis(110000)),
            UtcMillis(200000),
            Some(Control {
                scope: Scope::Sector(2),
                kind: ControlKind::Flag(Flag::Yellow),
            }),
        )
        .unwrap(),
    );
    conditions.control.push(
        Reading::new(
            UtcMillis(0),
            Availability::Unknown,
            UtcMillis(200000),
            Some(Control {
                scope: Scope::Track,
                kind: ControlKind::Flag(Flag::Green),
            }),
        )
        .unwrap(),
    );
    conditions.pits.insert(
        4,
        vec![
            Reading::new(
                UtcMillis(100000),
                Availability::Known(UtcMillis(125000)),
                UtcMillis(200000),
                Some(PitRecord {
                    lap: Some(2),
                    lane_seconds: Some(25.0),
                    stop_seconds: None,
                }),
            )
            .unwrap(),
        ],
    );
    RecordedReplay::new(
        Timeline::new(
            SessionId::new(Provider::OpenF1, "9590").unwrap(),
            UtcMillis(0),
            vec![
                RosterDriver::new(4, "NOR", "Synthetic Norris")
                    .unwrap()
                    .with_team(Some("Fixture team"), None)
                    .unwrap(),
            ],
            vec![
                source(Channel::CarData),
                source(Channel::Laps),
                source(Channel::RaceControl),
            ],
            events,
        )
        .unwrap()
        .with_conditions(conditions),
        "SYNTHETIC DETAIL FIXTURE".into(),
    )
    .unwrap()
}
fn app(at: u64) -> App {
    let mut app = App::new(
        sectorline::stream::TelemetryStream::load_from_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache"),
        )
        .unwrap(),
        false,
    );
    let mut r = replay();
    r.seek(ElapsedMillis(at)).unwrap();
    app.prepared_recording = Some(r);
    app.view = sectorline::ui::View::Driver;
    app
}
fn capture(app: &App, w: u16, h: u16) -> (String, ratatui::buffer::Buffer) {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    let b = t.backend().buffer().clone();
    let text = b
        .content()
        .chunks(w as usize)
        .map(|r| {
            r.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (text, b)
}
#[test]
fn ui_detail_eligible_reference_respects_deletion_reinstatement_and_seek() {
    let mut r = replay();
    for (at, reference) in [(121000, 1), (135000, 2), (165000, 1), (121000, 1)] {
        r.seek(ElapsedMillis(at)).unwrap();
        assert_eq!(r.eligible_reference().unwrap().number.get(), reference);
    }
}
#[test]
fn ui_detail_event_availability_scope_and_seek_target_are_explicit() {
    let mut r = replay();
    r.seek(ElapsedMillis(124999)).unwrap();
    assert!(!r.visible_events().iter().any(|e| e.channel == "pit"));
    r.seek(ElapsedMillis(125000)).unwrap();
    let pit = r
        .visible_events()
        .iter()
        .find(|e| e.channel == "pit")
        .unwrap();
    assert_eq!(pit.observed, UtcMillis(100000));
    assert_eq!(pit.seek_target(r.timeline()), Some(ElapsedMillis(125000)));
    assert_eq!(pit.scope, Scope::Driver(4));
    assert!(!r.visible_events().iter().any(|e| e.summary.contains("Red")));
    r.seek(ElapsedMillis(140000)).unwrap();
    assert!(
        r.visible_events()
            .iter()
            .any(|e| e.scope == Scope::Track && e.summary.contains("Red"))
    );
    r.seek(ElapsedMillis(121000)).unwrap();
    assert!(
        !r.visible_events()
            .iter()
            .any(|e| e.channel == "pit" || e.summary.contains("Red"))
    );
}
#[test]
fn ui_detail_history_does_not_fill_null_or_future_samples_with_zero() {
    let mut r = replay();
    r.seek(ElapsedMillis(1500)).unwrap();
    assert!(r.telemetry_history().is_empty());
    r.seek(ElapsedMillis(4000)).unwrap();
    let rows = r.telemetry_history();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].speed_kmh, Some(200.0));
    assert!(rows[1].speed_kmh.is_none());
    assert_eq!(rows[2].speed_kmh, Some(210.0));
}
#[test]
fn ui_detail_readout_distinguishes_neutral_brake_drs_unknown_and_missing() {
    let mut app = app(2000);
    let text = capture(&app, 80, 24).0;
    assert!(text.contains("Gear N"));
    assert!(text.contains("Brake on"));
    assert!(text.contains("DRS unknown (9)"));
    assert!(text.contains("age 1.000s"));
    app.prepared_recording
        .as_mut()
        .unwrap()
        .seek(ElapsedMillis(3000))
        .unwrap();
    let text = capture(&app, 80, 24).0;
    assert!(text.contains("Gear --"));
    assert!(text.contains("Brake --"));
    assert!(text.contains("DRS --"));
    app.prepared_recording
        .as_mut()
        .unwrap()
        .seek(ElapsedMillis(4000))
        .unwrap();
    let text = capture(&app, 80, 24).0;
    assert!(text.contains("Brake off"));
    assert!(text.contains("DRS on"));
}
#[test]
fn ui_detail_event_keyboard_seeks_availability_without_changing_driver() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = app(125000);
    app.view = sectorline::ui::View::Events;
    let driver = app.prepared_recording.as_ref().unwrap().selected();
    capture(&app, 80, 24);
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let r = app.prepared_recording.as_ref().unwrap();
    assert_eq!(r.frame().elapsed(), ElapsedMillis(120000));
    assert_eq!(r.selected(), driver);
    assert_eq!(
        r.status(),
        sectorline::replay::clock::PlaybackStatus::Paused
    );
    assert!(!capture(&app, 80, 24).0.contains("Pit visit"));
}

#[test]
fn ui_detail_comparison_deltas_are_from_one_eligible_reference_lap() {
    let mut app = app(121000);
    let text = capture(&app, 144, 70).0;
    assert!(text.contains("vs eligible PB L1"));
    assert!(text.contains("Lap delta +3.000s"));
    assert!(text.contains("S1 delta +1.000s"));
    app.prepared_recording
        .as_mut()
        .unwrap()
        .seek(ElapsedMillis(135000))
        .unwrap();
    assert!(capture(&app, 144, 70).0.contains("vs eligible PB L2"));
    app.prepared_recording
        .as_mut()
        .unwrap()
        .seek(ElapsedMillis(180000))
        .unwrap();
    let text = capture(&app, 144, 70).0;
    assert!(text.contains("unverified"));
    assert!(!text.contains("Lap delta"));
}
#[test]
fn ui_detail_all_sizes_no_color_and_actual_cell_captures() {
    let mut app = app(121000);
    for color in [false, true] {
        app.theme.color = color;
        for (view, view_name) in [
            (sectorline::ui::View::Driver, "driver"),
            (sectorline::ui::View::Events, "events"),
        ] {
            app.view = view;
            for (w, h, size) in [
                (144, 44, "wide"),
                (120, 36, "standard"),
                (80, 24, "compact"),
            ] {
                let (text, buffer) = capture(&app, w, h);
                assert!(text.contains("RECORDED SNAPSHOT"));
                assert!(text.contains("SYNTHETIC DETAIL FIXTURE"));
                assert!(text.lines().last().unwrap().contains("q quit"));
                if !color {
                    assert!(
                        buffer
                            .content()
                            .iter()
                            .all(|c| c.fg == ratatui::style::Color::Reset
                                && c.bg == ratatui::style::Color::Reset)
                    );
                }
                if color && let Some(root) = std::env::var_os("SECTORLINE_CAPTURE_DIR") {
                    let root = std::path::PathBuf::from(root);
                    std::fs::create_dir_all(&root).unwrap();
                    std::fs::write(root.join(format!("t16-{view_name}-{size}.txt")), text).unwrap();
                    let cells=buffer.content().iter().map(|c|serde_json::json!({"s":c.symbol(),"fg":format!("{:?}",c.fg),"bg":format!("{:?}",c.bg),"m":format!("{:?}",c.modifier)})).collect::<Vec<_>>();
                    std::fs::write(
                        root.join(format!("t16-{view_name}-{size}.json")),
                        serde_json::to_vec(
                            &serde_json::json!({"width":w,"height":h,"cells":cells}),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                }
            }
        }
    }
}
#[test]
fn ui_detail_empty_history_and_events_do_not_show_future_or_a_fake_trace() {
    let mut app = app(0);
    let text = capture(&app, 144, 70).0;
    assert!(text.contains("trace -- (no eligible values)"));
    app.view = sectorline::ui::View::Events;
    let text = capture(&app, 80, 24).0;
    assert!(text.contains("0 available"));
    assert!(!text.contains("Red"));
    assert!(!text.contains("Completed L"));
}

#[test]
fn ui_detail_missing_sectors_and_pit_out_laps_do_not_get_fake_deltas() {
    let mut app = app(121000);
    let mut r = replay_variant(true, false);
    r.seek(ElapsedMillis(121000)).unwrap();
    app.prepared_recording = Some(r);
    let text = capture(&app, 144, 70).0;
    assert!(text.contains("S1 delta +1.000s"));
    assert!(text.contains("S2 delta --s"));
    let mut r = replay_variant(false, true);
    r.seek(ElapsedMillis(121000)).unwrap();
    app.prepared_recording = Some(r);
    assert!(!capture(&app, 144, 70).0.contains("Lap delta"));
}
#[test]
fn ui_detail_conflicting_reports_are_not_rendered_as_a_resolved_flag() {
    let source = ChunkMetadata {
        channel: Channel::CarData,
        session_key: 9590,
        meeting_key: 1244,
        window: None,
        source_url: "https://api.openf1.org/v1/car_data?session_key=9590".into(),
        retrieved_at: UtcMillis(200000),
        status: 200,
        attempts: 1,
    };
    let event = Event::new(
        4,
        UtcMillis(60000),
        Availability::Known(UtcMillis(60000)),
        0,
        Payload::Telemetry(TelemetryValues::default()),
    )
    .unwrap();
    let mut conditions = ConditionsArchive::default();
    for flag in [Flag::Red, Flag::Green] {
        conditions.control.push(
            Reading::new(
                UtcMillis(50000),
                Availability::Known(UtcMillis(50000)),
                UtcMillis(100000),
                Some(Control {
                    scope: Scope::Track,
                    kind: ControlKind::Flag(flag),
                }),
            )
            .unwrap(),
        );
    }
    let t = Timeline::new(
        SessionId::new(Provider::OpenF1, "9590").unwrap(),
        UtcMillis(0),
        vec![RosterDriver::new(4, "NOR", "Fixture").unwrap()],
        vec![source],
        vec![event],
    )
    .unwrap()
    .with_conditions(conditions);
    let mut r = RecordedReplay::new(t, "Conflict fixture".into()).unwrap();
    r.seek(ElapsedMillis(50000)).unwrap();
    assert_eq!(r.visible_events().len(), 1);
    assert_eq!(
        r.visible_events()[0].summary,
        "Conflicting provider reports"
    );
    assert!(r.conditions().track_flag.value().is_none());
}
