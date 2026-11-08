use sectorline::domain::{
    capabilities::{PhaseBoundary, PhaseProjection, PhaseUpdate, SessionContext},
    conditions::Reading,
    *,
};
fn session() -> SessionId {
    SessionId::new(Provider::OpenF1, "9586").unwrap()
}
fn phase(
    at: i64,
    available: i64,
    part: Option<u8>,
    boundary: PhaseBoundary,
) -> Reading<PhaseUpdate> {
    Reading::new(
        UtcMillis(at),
        Availability::Known(UtcMillis(available)),
        UtcMillis(900000),
        Some(PhaseUpdate { part, boundary }),
    )
    .unwrap()
}
#[test]
fn session_types_metadata_never_defaults_unknown_or_conflicting_formats_to_race() {
    assert_eq!(
        SessionContext::from_names(session(), "Sprint", "Race").kind,
        SessionKind::Sprint
    );
    assert_eq!(
        SessionContext::from_names(session(), "Sprint Shootout", "Qualifying").kind,
        SessionKind::SprintQualifying
    );
    assert_eq!(
        SessionContext::from_names(session(), "Practice 2", "Practice").kind,
        SessionKind::Practice
    );
    assert!(!SessionContext::from_names(session(), "Qualifying", "Race").race_fields());
    assert!(!SessionContext::unknown(session()).race_fields());
}
#[test]
fn session_types_phase_windows_require_starts_and_allow_evidenced_flying_laps() {
    let rows = vec![
        phase(0, 100, Some(1), PhaseBoundary::Start),
        phase(1000, 1000, Some(1), PhaseBoundary::Close),
        phase(2000, 2200, Some(2), PhaseBoundary::Start),
    ];
    assert_eq!(PhaseProjection::at(&rows, 99).part, None);
    let q1 = PhaseProjection::at(&rows, 1500);
    assert_eq!(q1.part, Some(1));
    assert!(q1.closed);
    assert_eq!(q1.part_for_lap(500, 1400), Some(1));
    assert_eq!(q1.part_for_lap(1100, 1400), None);
    let q2 = PhaseProjection::at(&rows, 2200);
    assert_eq!(q2.part, Some(2));
    assert_eq!(q2.part_for_lap(500, 2100), None);
    assert_eq!(q2.part_for_lap(2100, 2300), Some(2));
    let report = vec![phase(0, 0, Some(2), PhaseBoundary::Report)];
    assert_eq!(PhaseProjection::at(&report, 100).part, Some(2));
    assert_eq!(PhaseProjection::at(&report, 100).part_for_lap(1, 90), None);
    assert!(!PhaseProjection::at(&report, 100).has_current_start());
    let repeated_close = vec![
        phase(0, 0, Some(1), PhaseBoundary::Start),
        phase(1000, 1000, Some(1), PhaseBoundary::Close),
        phase(1100, 1100, Some(1), PhaseBoundary::Close),
    ];
    let closed = PhaseProjection::at(&repeated_close, 1500);
    assert!(closed.has_current_start());
    assert_eq!(closed.part_for_lap(1050, 1400), None);
}
#[test]
fn session_types_conflicting_invalid_and_missing_phase_reports_remain_unknown() {
    let mut rows = vec![
        phase(0, 0, Some(1), PhaseBoundary::Start),
        phase(0, 0, Some(2), PhaseBoundary::Start),
    ];
    assert_eq!(PhaseProjection::at(&rows, 0).part, None);
    rows = vec![
        phase(0, 0, Some(1), PhaseBoundary::Start),
        phase(100, 100, Some(4), PhaseBoundary::Report),
    ];
    assert_eq!(PhaseProjection::at(&rows, 100).part, None);
    assert_eq!(PhaseProjection::at(&[], 100).part, None);
}

fn replay(name: &str, kind: &str, telemetry: bool) -> sectorline::replay::RecordedReplay {
    use sectorline::{
        data::openf1::{Channel, ChunkMetadata},
        domain::{
            conditions::ConditionsArchive,
            normalize::{Event, Payload, RosterDriver, TelemetryValues},
            timing::{Eligibility, LapTiming},
        },
        replay::{RecordedReplay, navigation::LapIndex, timeline::Timeline},
    };
    let source = |channel| ChunkMetadata {
        channel,
        session_key: 9586,
        meeting_key: 1244,
        window: None,
        source_url: format!(
            "https://api.openf1.org/v1/{}?session_key=9586",
            channel.endpoint()
        ),
        retrieved_at: UtcMillis(500000),
        status: 200,
        attempts: 1,
    };
    let mut events = vec![
        Event::new(
            4,
            UtcMillis(0),
            Availability::Known(UtcMillis(0)),
            0,
            Payload::Position(std::num::NonZeroU32::new(2)),
        )
        .unwrap(),
        Event::new(
            16,
            UtcMillis(0),
            Availability::Known(UtcMillis(0)),
            0,
            Payload::Position(std::num::NonZeroU32::new(1)),
        )
        .unwrap(),
    ];
    if telemetry {
        for at in [0, 350000] {
            events.push(
                Event::new(
                    4,
                    UtcMillis(at),
                    Availability::Known(UtcMillis(at)),
                    2,
                    Payload::Telemetry(TelemetryValues {
                        speed: Some(SpeedKmh::new(250.0).unwrap()),
                        ..TelemetryValues::default()
                    }),
                )
                .unwrap(),
            );
        }
    }
    let mut laps = LapIndex::default();
    for (driver, lap, start, duration) in [
        (4, 1, 0, 60.0),
        (16, 1, 0, 65.0),
        (4, 2, 120000, 62.0),
        (4, 3, 191000, 61.0),
        (4, 4, 270000, 61.0),
    ] {
        laps.insert(driver, lap, UtcMillis(start));
        let end = UtcMillis(start + (duration * 1000.0) as i64);
        events.push(
            Event::new(
                driver,
                end,
                Availability::Known(end),
                1,
                Payload::Lap(Box::new(LapTiming {
                    number: std::num::NonZeroU32::new(lap).unwrap(),
                    duration: Some(Seconds::new(duration).unwrap()),
                    sectors: [Some(Seconds::new(duration / 3.0).unwrap()); 3],
                    pit_out: Some(false),
                    eligibility: Eligibility::Eligible,
                })),
            )
            .unwrap(),
        );
    }
    for (at, available, eligibility) in [
        (225000, 230000, Eligibility::Ineligible),
        (235000, 236000, Eligibility::Eligible),
    ] {
        events.push(
            Event::new(
                4,
                UtcMillis(at),
                Availability::Known(UtcMillis(available)),
                3,
                Payload::LapDecision {
                    lap: std::num::NonZeroU32::new(2).unwrap(),
                    eligibility,
                },
            )
            .unwrap(),
        );
    }
    let mut conditions = ConditionsArchive::default();
    conditions.phases = vec![
        phase(0, 100, Some(1), PhaseBoundary::Start),
        phase(100000, 100000, Some(1), PhaseBoundary::Close),
        phase(120000, 125000, Some(2), PhaseBoundary::Start),
        phase(250000, 250000, Some(2), PhaseBoundary::Close),
        phase(270000, 270000, Some(3), PhaseBoundary::Start),
    ];
    let t = Timeline::new(
        session(),
        UtcMillis(0),
        vec![
            RosterDriver::new(4, "NOR", "Synthetic driver 4").unwrap(),
            RosterDriver::new(16, "LEC", "Synthetic driver 16").unwrap(),
        ],
        vec![
            source(Channel::Position),
            source(Channel::Laps),
            source(Channel::CarData),
            source(Channel::RaceControl),
        ],
        events,
    )
    .unwrap()
    .with_conditions(conditions);
    RecordedReplay::with_navigation(
        t,
        "SYNTHETIC SESSION TYPE FIXTURE".into(),
        laps,
        &sectorline::data::limits::Cancellation::default(),
    )
    .unwrap()
    .with_context(
        SessionContext::from_names(session(), name, kind)
            .with_schedule(UtcMillis(20000), UtcMillis(380000))
            .unwrap(),
    )
    .unwrap()
}
#[test]
fn session_types_qualifying_never_carries_a_q1_best_into_q2_or_q3() {
    let mut r = replay("Qualifying", "Qualifying", true);
    for (at, part, best) in [
        (99, None, None),
        (60000, Some(1), Some(60.0)),
        (125000, Some(2), None),
        (182000, Some(2), Some(62.0)),
        (230000, Some(2), None),
        (236000, Some(2), Some(62.0)),
        (252000, Some(2), Some(61.0)),
        (270000, Some(3), None),
        (331000, Some(3), Some(61.0)),
        (182000, Some(2), Some(62.0)),
    ] {
        r.seek(ElapsedMillis(at)).unwrap();
        assert_eq!(r.phase().part, part);
        assert_eq!(
            r.timing_bests().get(&4).map(|b| b.duration.get()),
            best,
            "{at}"
        );
        assert_eq!(
            r.frame().samples()[0].driver.number.get(),
            16,
            "Provider classification must not be sorted from phase bests"
        );
    }
    r.seek(ElapsedMillis(125000)).unwrap();
    assert!(r.comparable_lap().is_none());
    assert!(r.eligible_reference().is_none());
    assert!(!r.timing_bests().contains_key(&16));
}
#[test]
fn session_types_practice_uses_session_bests_without_a_race_countdown() {
    let mut r = replay("Practice 1", "Practice", false);
    r.seek(ElapsedMillis(182000)).unwrap();
    assert!(!r.context().race_fields());
    assert_eq!(r.timing_bests()[&4].duration.get(), 60.0);
    assert!(r.session_notice().contains("telemetry --"));
    let s = r.context().schedule.as_ref().unwrap();
    assert_eq!(s.end.0 - s.start.0, 360000);
    assert_eq!(r.frame().elapsed().0, 182000);
    let wrong = SessionContext::from_names(
        SessionId::new(Provider::OpenF1, "999").unwrap(),
        "Race",
        "Race",
    );
    assert!(r.with_context(wrong).is_err());
}
fn app(r: sectorline::replay::RecordedReplay) -> sectorline::App {
    let mut app = sectorline::App::new(
        sectorline::TelemetryStream::load_from_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache"),
        )
        .unwrap(),
        false,
    );
    app.prepared_recording = Some(r);
    app
}
fn capture(app: &sectorline::App, w: u16, h: u16) -> String {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    if let Some(root) = std::env::var_os("SECTORLINE_CAPTURE_DIR")
        && let Some(r) = &app.prepared_recording
        && r.title().contains("EXCERPT")
    {
        let size = match w {
            144 => "wide",
            120 => "standard",
            _ => "compact",
        };
        let name = if r.context().qualifying() {
            "qualifying"
        } else {
            "practice"
        };
        let buffer = t.backend().buffer();
        let cells = buffer.content().iter().map(|c| serde_json::json!({"s":c.symbol(),"fg":format!("{:?}",c.fg),"bg":format!("{:?}",c.bg),"m":format!("{:?}",c.modifier)})).collect::<Vec<_>>();
        std::fs::write(
            std::path::Path::new(&root).join(format!("t18-{name}-{size}.json")),
            serde_json::to_vec(&serde_json::json!({"width":w,"height":h,"cells":cells})).unwrap(),
        )
        .unwrap();
    }
    t.backend()
        .buffer()
        .content()
        .chunks(w as usize)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn session_types_non_race_tables_hide_gaps_and_expose_partial_capabilities() {
    for (name, kind) in [
        ("Qualifying", "Qualifying"),
        ("Practice 1", "Practice"),
        ("Sprint Shootout", "Qualifying"),
        ("Experimental", "Experimental"),
    ] {
        let mut r = replay(name, kind, false);
        r.seek(ElapsedMillis(182000)).unwrap();
        let app = app(r);
        for (w, h) in [(144, 44), (120, 36), (80, 24)] {
            let text = capture(&app, w, h);
            assert!(text.contains("Best(s)"));
            assert!(!text.contains("Gap(s)"));
            assert!(!text.contains("Int(s)"));
            assert!(text.contains("L#"));
            assert!(text.contains("sched 06:00.000"));
            assert!(text.contains("observed"));
            assert!(text.contains("telemetry --"));
        }
    }
}
