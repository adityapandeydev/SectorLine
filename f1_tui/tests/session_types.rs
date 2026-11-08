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

struct CacheRoot(std::path::PathBuf);
impl Drop for CacheRoot {
    fn drop(&mut self) {
        if std::env::var_os("SECTORLINE_CAPTURE_DIR").is_none() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn native_excerpt(
    label: &str,
) -> (
    CacheRoot,
    sectorline::replay::RecordedReplay,
    serde_json::Value,
) {
    use sectorline::{
        data::{
            cache::{CacheSpec, CacheWriter},
            limits::Cancellation,
            openf1::{Channel, ChunkMetadata, Window},
        },
        domain::normalize::utc_millis_ceiling,
        replay::RecordedReplay,
    };
    use serde_json::json;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/session-types.json")).unwrap();
    let e = fixture["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["label"] == label)
        .unwrap()
        .clone();
    let session = &e["session"];
    let key = session["session_key"].as_u64().unwrap() as u32;
    let meeting = session["meeting_key"].as_u64().unwrap() as u32;
    let start = utc_millis_ceiling(session["date_start"].as_str().unwrap()).unwrap();
    let end = utc_millis_ceiling(session["date_end"].as_str().unwrap()).unwrap();
    let window = Window::new(UtcMillis(start.0 - 3600000), UtcMillis(end.0 + 3600000)).unwrap();
    let channels = Channel::ALL.to_vec();
    let spec = CacheSpec::new(
        key,
        meeting,
        2024,
        (
            session["session_name"].as_str().unwrap(),
            session["session_type"].as_str().unwrap(),
        ),
        window,
        std::time::Duration::from_secs(300),
        channels.clone(),
    )
    .unwrap();
    let root = CacheRoot(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../target/t18-excerpt-{label}-{}",
            std::process::id()
        )),
    );
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    for channel in channels {
        let windows = if matches!(
            channel,
            Channel::CarData | Channel::Location | Channel::Intervals
        ) {
            (0..36)
                .map(|n| {
                    Some(
                        Window::new(
                            UtcMillis(window.start().0 + n * 300000),
                            UtcMillis(window.start().0 + (n + 1) * 300000),
                        )
                        .unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        } else {
            vec![None]
        };
        let rows = if channel == Channel::Drivers {
            vec![
                json!({"session_key":key,"meeting_key":meeting,"driver_number":4,"name_acronym":"--","full_name":"--","team_name":"--"}),
            ]
        } else {
            e["channels"][channel.endpoint()]["rows"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        };
        let retrieved = e["channels"][channel.endpoint()]["retrieved_at_utc"]
            .as_str()
            .map(|s| utc_millis_ceiling(s).unwrap())
            .unwrap_or(UtcMillis(1793548800000));
        for w in windows {
            let mut url = format!(
                "https://api.openf1.org/v1/{}?session_key={key}",
                channel.endpoint()
            );
            if let Some(w) = w {
                let fmt = |t: UtcMillis| {
                    chrono::DateTime::from_timestamp_millis(t.0)
                        .unwrap()
                        .format("%Y-%m-%dT%H:%M:%S%.3f")
                        .to_string()
                };
                url.push_str(&format!("&date>={}&date<{}", fmt(w.start()), fmt(w.end())));
            }
            let selected = rows
                .iter()
                .filter(|r| {
                    w.is_none_or(|w| {
                        let t = utc_millis_ceiling(r["date"].as_str().unwrap()).unwrap();
                        t >= w.start() && t < w.end()
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            writer
                .import(
                    ChunkMetadata {
                        channel,
                        session_key: key,
                        meeting_key: meeting,
                        window: w,
                        source_url: url,
                        retrieved_at: retrieved,
                        status: 200,
                        attempts: 1,
                    },
                    &serde_json::to_vec(&selected).unwrap(),
                )
                .unwrap();
        }
    }
    let cache = writer.finish().unwrap();
    let replay = RecordedReplay::from_cache(
        &cache,
        format!("{label} EXCERPT - partial test fixture"),
        &Cancellation::default(),
        &|_, _| {},
    )
    .unwrap();
    let context = SessionContext::from_names(
        SessionId::new(Provider::OpenF1, key.to_string()).unwrap(),
        session["session_name"].as_str().unwrap(),
        session["session_type"].as_str().unwrap(),
    )
    .with_schedule(start, end)
    .unwrap();
    let replay = replay.with_context(context).unwrap();
    if std::env::var_os("SECTORLINE_CAPTURE_DIR").is_some() {
        std::fs::write(
            root.0
                .parent()
                .unwrap()
                .join(format!("t18-{label}-root.txt")),
            root.0.to_str().unwrap(),
        )
        .unwrap();
        let catalogue = json!({"version":1,"retrieved_at_ms":1793548800000_i64,"loaded_years":[2024],"meetings":[{"meeting_key":meeting,"year":2024,"meeting_name":"PARTIAL REAL-SOURCE EXCERPT FIXTURE","circuit_short_name":"Monza","location":"Monza","country_name":"Italy"}],"sessions":[session]});
        std::fs::write(
            root.0
                .parent()
                .unwrap()
                .join(format!("t18-{label}-catalogue.json")),
            serde_json::to_vec(&catalogue).unwrap(),
        )
        .unwrap();
    }
    (root, replay, e)
}
#[test]
fn session_types_native_qualifying_and_practice_excerpts_match_fixed_boundaries() {
    use sectorline::domain::normalize::utc_millis_ceiling;
    for (label, boundary, duration, position) in [
        ("qualifying", "2024-08-31T14:06:29.821Z", 79.911, 6),
        // Conservative millisecond ceiling of the source sector sum delays by 1 ms.
        ("practice", "2024-08-30T11:34:18.699Z", 84.371, 4),
    ] {
        let (_root, mut r, raw) = native_excerpt(label);
        let at = utc_millis_ceiling(boundary).unwrap().0 - r.timeline().origin().0;
        r.seek(ElapsedMillis(at as u64 - 1)).unwrap();
        assert!(
            !r.timing_bests().contains_key(&4),
            "No best before completed-lap evidence"
        );
        r.seek(ElapsedMillis(at as u64)).unwrap();
        let best = r.timing_bests()[&4].clone();
        assert_eq!(best.duration.get(), duration);
        assert_eq!(
            best.eligibility,
            sectorline::domain::timing::Eligibility::Unknown
        );
        assert!(r.eligible_reference().is_none());
        assert_eq!(
            r.frame().samples()[0].position.value().unwrap().get(),
            position
        );
        assert_eq!(
            r.context().schedule.as_ref().unwrap().end.0
                - r.context().schedule.as_ref().unwrap().start.0,
            3600000
        );
        assert_ne!(r.frame().elapsed().0, 3600000);
        if label == "qualifying" {
            assert_eq!(r.phase().part, Some(1));
        }
        let mut app = app(r);
        for (w, h) in [(144, 44), (120, 36), (80, 24)] {
            let text = capture(&app, w, h);
            assert!(text.contains("Best(s)"));
            assert!(text.contains("Rpt"));
            assert!(
                !text
                    .lines()
                    .find(|line| line.contains("Rpt") && line.contains("Best(s)"))
                    .unwrap()
                    .contains("Stat"),
                "Nonrace status header must fit its four-cell slot"
            );
            assert!(!text.contains("Gap(s)"));
            assert!(!text.contains("Int(s)"));
            assert!(text.contains(&format!("{duration:.3}*")));
            assert!(text.contains("sched 60:00.000"));
        }
        let car = &raw["channels"]["car_data"]["rows"][0];
        let at = utc_millis_ceiling(car["date"].as_str().unwrap()).unwrap().0
            - app
                .prepared_recording
                .as_ref()
                .unwrap()
                .timeline()
                .origin()
                .0;
        let r = app.prepared_recording.as_mut().unwrap();
        r.seek(ElapsedMillis(at as u64)).unwrap();
        assert_eq!(
            r.frame().samples()[0].speed_kmh.value().unwrap().get(),
            car["speed"].as_f64().unwrap()
        );
        if label == "qualifying" {
            assert!(r.frame().samples()[0].brake.value().is_none());
            assert!(r.frame().samples()[0].throttle.value().is_none());
            assert_eq!(r.frame().samples()[0].gear.value().unwrap().get(), 0);
        }
    }
}
