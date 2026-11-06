use sectorline::{
    data::openf1::{Channel, ChunkMetadata},
    domain::{
        classification::DriverStatus,
        normalize::{Event, Payload, RosterDriver},
        timing::{Eligibility, LapTiming},
        *,
    },
    replay::timeline::{ReplayPolicy, Timeline},
};
use std::num::NonZeroU32;
fn lap(number: u32, seconds: Option<f64>, eligibility: Eligibility) -> Payload {
    Payload::Lap(Box::new(LapTiming {
        number: NonZeroU32::new(number).unwrap(),
        duration: seconds.map(|v| Seconds::new(v).unwrap()),
        sectors: [Some(Seconds::new(seconds.unwrap_or(60.0) / 3.0).unwrap()); 3],
        pit_out: Some(false),
        eligibility,
    }))
}
fn ev(driver: u32, at: i64, available: Availability, payload: Payload) -> Event {
    let source = match payload.channel() {
        Channel::Position => 0,
        Channel::Laps => 1,
        Channel::RaceControl => 2,
        Channel::Intervals => 3,
        _ => unreachable!(),
    };
    Event::new(driver, UtcMillis(at), available, source, payload).unwrap()
}
fn known(driver: u32, at: i64, payload: Payload) -> Event {
    ev(driver, at, Availability::Known(UtcMillis(at)), payload)
}
fn timeline(events: Vec<Event>) -> Timeline {
    let sources = [
        Channel::Position,
        Channel::Laps,
        Channel::RaceControl,
        Channel::Intervals,
    ]
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
    Timeline::new(
        SessionId::new(Provider::OpenF1, "9590").unwrap(),
        UtcMillis(0),
        vec![
            RosterDriver::new(4, "NOR", "Lando Norris").unwrap(),
            RosterDriver::new(16, "LEC", "Charles Leclerc").unwrap(),
        ],
        sources,
        events,
    )
    .unwrap()
}
fn frame(t: &Timeline, ms: u64) -> PlaybackFrame {
    t.state_at(ElapsedMillis(ms), ReplayPolicy::EventTimeEstimates)
        .unwrap()
        .frame(t)
        .unwrap()
}
#[test]
fn classification_order_uses_positions_not_laps_gaps_or_retirement() {
    let t = timeline(vec![
        known(4, 0, Payload::Position(NonZeroU32::new(2))),
        known(16, 0, Payload::Position(NonZeroU32::new(1))),
        known(4, 60_000, lap(2, Some(60.0), Eligibility::Eligible)),
        known(16, 60_000, lap(1, Some(60.0), Eligibility::Eligible)),
        known(16, 61_000, Payload::Status(DriverStatus::Retired)),
        known(
            4,
            61_000,
            Payload::Intervals {
                leader: Some(Gap::LapsBehind(NonZeroU32::new(1).unwrap())),
                ahead: None,
            },
        ),
    ]);
    let f = frame(&t, 70_000);
    assert_eq!(f.samples()[0].driver.number.get(), 16);
    assert_eq!(f.samples()[0].status.value(), Some(&DriverStatus::Retired));
    assert!(f.samples()[1].gap_to_ahead.value().is_none());
}
#[test]
fn classification_no_lap_sector_or_final_status_before_availability() {
    let t = timeline(vec![
        ev(
            4,
            60_000,
            Availability::Known(UtcMillis(62_000)),
            lap(1, Some(60.0), Eligibility::Eligible),
        ),
        ev(
            4,
            63_000,
            Availability::Unknown,
            Payload::Status(DriverStatus::Dsq),
        ),
        ev(
            16,
            63_000,
            Availability::Known(UtcMillis(65_000)),
            Payload::Status(DriverStatus::Dns),
        ),
    ]);
    let early = frame(&t, 61_999);
    assert!(early.samples()[0].lap_time.value().is_none());
    assert!(
        early.samples()[0]
            .sectors
            .iter()
            .all(|v| v.value().is_none())
    );
    assert!(early.samples()[0].status.value().is_none());
    let completed = frame(&t, 62_000);
    assert_eq!(completed.samples()[0].lap_time.value().unwrap().get(), 60.0);
    assert!(
        completed.samples()[0]
            .sectors
            .iter()
            .all(|v| v.value().is_some())
    );
    assert!(frame(&t, 90_000).samples()[0].status.value().is_none());
    assert!(frame(&t, 64_999).samples()[1].status.value().is_none());
    assert_eq!(
        frame(&t, 65_000).samples()[1].status.value(),
        Some(&DriverStatus::Dns)
    );
}
#[test]
fn classification_deletion_reinstatement_and_backward_seek_rebuild_bests() {
    let t = timeline(vec![
        known(4, 60_000, lap(1, Some(60.0), Eligibility::Eligible)),
        known(4, 120_000, lap(2, Some(59.0), Eligibility::Eligible)),
        known(
            4,
            125_000,
            Payload::LapDecision {
                lap: NonZeroU32::new(2).unwrap(),
                eligibility: Eligibility::Ineligible,
            },
        ),
        known(
            4,
            130_000,
            Payload::LapDecision {
                lap: NonZeroU32::new(2).unwrap(),
                eligibility: Eligibility::Eligible,
            },
        ),
        known(16, 60_000, lap(1, Some(61.0), Eligibility::Eligible)),
    ]);
    assert_eq!(
        frame(&t, 120_000).samples()[0]
            .personal_best
            .value()
            .unwrap()
            .duration
            .get(),
        59.0
    );
    assert_eq!(
        frame(&t, 125_000).samples()[0]
            .personal_best
            .value()
            .unwrap()
            .duration
            .get(),
        60.0
    );
    assert_eq!(
        frame(&t, 125_000).samples()[0]
            .personal_best
            .timing()
            .available_at,
        Availability::Known(UtcMillis(125_000))
    );
    assert_eq!(
        frame(&t, 130_000).session_best().unwrap().1.duration.get(),
        59.0
    );
    let mut r = sectorline::replay::RecordedReplay::new(t, "Deletion fixture".into()).unwrap();
    r.seek(ElapsedMillis(130_000)).unwrap();
    r.seek(ElapsedMillis(125_000)).unwrap();
    assert_eq!(
        r.frame().samples()[0]
            .personal_best
            .value()
            .unwrap()
            .duration
            .get(),
        60.0
    );
    r.seek(ElapsedMillis(119_999)).unwrap();
    assert_eq!(r.frame().samples()[0].lap_number.value().unwrap().get(), 1);
}
#[test]
fn classification_first_lap_pitout_missing_and_unknown_eligibility() {
    let mut out = match lap(2, Some(50.0), Eligibility::Eligible) {
        Payload::Lap(v) => v,
        _ => unreachable!(),
    };
    out.pit_out = Some(true);
    let t = timeline(vec![
        known(4, 60_000, lap(1, None, Eligibility::Unknown)),
        known(4, 120_000, Payload::Lap(out)),
        known(4, 180_000, lap(3, Some(60.0), Eligibility::Unknown)),
    ]);
    assert!(
        frame(&t, 60_000).samples()[0]
            .personal_best
            .value()
            .is_none()
    );
    assert!(
        frame(&t, 120_000).samples()[0]
            .personal_best
            .value()
            .is_none()
    );
    let f = frame(&t, 180_000);
    assert_eq!(
        f.samples()[0].personal_best.value().unwrap().eligibility,
        Eligibility::Unknown
    );
}

#[test]
fn classification_cached_summaries_wait_for_completion_and_named_lap_decisions() {
    use sectorline::data::{
        cache::{CacheSpec, CacheWriter},
        limits::Cancellation,
        openf1::Window,
    };
    use serde_json::json;
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target")
        .join(format!("classification-fixture-{}", std::process::id()));
    let channels = vec![
        Channel::Drivers,
        Channel::Position,
        Channel::Laps,
        Channel::RaceControl,
    ];
    let spec = CacheSpec::new(
        9590,
        1244,
        2024,
        ("Race", "Race"),
        Window::new(UtcMillis(0), UtcMillis(300_000)).unwrap(),
        std::time::Duration::from_secs(300),
        channels.clone(),
    )
    .unwrap();
    let rows = [
        json!([{"session_key":9590,"meeting_key":1244,"driver_number":4,"full_name":"Lando Norris","name_acronym":"NOR","team_name":"McLaren"}]),
        json!([{"session_key":9590,"meeting_key":1244,"driver_number":4,"date":"1970-01-01T00:00:00Z","position":1}]),
        json!([
            {"session_key":9590,"meeting_key":1244,"driver_number":4,"lap_number":1,"date_start":"1970-01-01T00:00:00Z","lap_duration":60,"duration_sector_1":20,"duration_sector_2":20,"duration_sector_3":20,"is_pit_out_lap":false},
            {"session_key":9590,"meeting_key":1244,"driver_number":4,"lap_number":2,"date_start":"1970-01-01T00:01:05Z","lap_duration":50,"duration_sector_1":15,"duration_sector_2":15,"duration_sector_3":20,"is_pit_out_lap":false}
        ]),
        json!([
            {"session_key":9590,"meeting_key":1244,"date":"1970-01-01T00:02:00Z","driver_number":null,"lap_number":9,"category":"Other","message":"CAR 4 (NOR) TIME 0:50.000 DELETED - TRACK LIMITS AT TURN 2 LAP 2 00:01:55"},
            {"session_key":9590,"meeting_key":1244,"date":"1970-01-01T00:02:05Z","driver_number":null,"lap_number":10,"category":"Other","message":"CAR 4 (NOR) TIME 0:50.000 REINSTATED - TRACK LIMITS AT TURN 2 LAP 2"},
            {"session_key":9590,"meeting_key":1244,"date":"1970-01-01T00:02:10Z","driver_number":4,"lap_number":10,"category":"Other","message":"CAR 4 (NOR) STOPPED ON TRACK"}
        ]),
    ];
    let mut writer = CacheWriter::begin(&root, &spec).unwrap();
    for (channel, rows) in channels.into_iter().zip(rows) {
        let endpoint = match channel {
            Channel::Drivers => "drivers",
            Channel::Position => "position",
            Channel::Laps => "laps",
            Channel::RaceControl => "race_control",
            _ => unreachable!(),
        };
        writer
            .import(
                ChunkMetadata {
                    channel,
                    session_key: 9590,
                    meeting_key: 1244,
                    window: None,
                    source_url: format!("https://api.openf1.org/v1/{endpoint}?session_key=9590"),
                    retrieved_at: UtcMillis(900_000),
                    status: 200,
                    attempts: 1,
                },
                &serde_json::to_vec(&rows).unwrap(),
            )
            .unwrap();
    }
    let cache = writer.finish().unwrap();
    let timeline = Timeline::from_cache(&cache, &Cancellation::default(), &|_, _| {}).unwrap();
    assert_eq!(timeline.origin(), UtcMillis(0));
    assert!(
        frame(&timeline, 64_999).samples()[0]
            .sectors
            .iter()
            .all(|s| s.value().is_none())
    );
    assert_eq!(
        frame(&timeline, 65_000).samples()[0]
            .lap_time
            .value()
            .unwrap()
            .get(),
        60.0
    );
    assert_eq!(
        frame(&timeline, 119_999).samples()[0]
            .personal_best
            .value()
            .unwrap()
            .duration
            .get(),
        50.0
    );
    assert_eq!(
        frame(&timeline, 120_000).samples()[0]
            .personal_best
            .value()
            .unwrap()
            .duration
            .get(),
        60.0
    );
    assert_eq!(
        frame(&timeline, 120_000).samples()[0]
            .lap_eligibility
            .value(),
        Some(&Eligibility::Ineligible)
    );
    let reinstated = frame(&timeline, 125_000);
    let pb = &reinstated.samples()[0].personal_best;
    assert_eq!(pb.value().unwrap().duration.get(), 50.0);
    assert_eq!(pb.value().unwrap().eligibility, Eligibility::Unknown);
    assert_eq!(
        pb.timing().available_at,
        Availability::Estimated(UtcMillis(125_000))
    );
    assert!(
        frame(&timeline, 130_000).samples()[0]
            .status
            .value()
            .is_none()
    );
    assert!(
        timeline
            .state_at(ElapsedMillis(130_000), ReplayPolicy::KnownOnly)
            .unwrap()
            .frame(&timeline)
            .unwrap()
            .samples()[0]
            .lap_time
            .value()
            .is_none()
    );
    let mut replay =
        sectorline::replay::RecordedReplay::new(timeline, "Cached timing fixture".into()).unwrap();
    replay.seek(ElapsedMillis(120_000)).unwrap();
    for (width, height) in [(144, 44), (120, 36), (80, 24)] {
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
        app.prepared_recording = Some(replay);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("Completed L2 [invalid]"));
        assert!(text.contains("Provisional archive best"));
        assert!(text.contains("S1 00:15.000"));
        replay = app.prepared_recording.take().unwrap();
    }
    drop(cache);
    let target = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target")
        .canonicalize()
        .unwrap();
    let cleanup = root.canonicalize().unwrap();
    assert!(cleanup.starts_with(&target));
    std::fs::remove_dir_all(cleanup).unwrap();
}
#[test]
fn classification_completion_and_control_parser_do_not_invent_timing() {
    use sectorline::domain::{normalize::control_decision, timing::completion};
    let seconds = |v| Some(Seconds::new(v).unwrap());
    assert_eq!(
        completion(
            Some(UtcMillis(1)),
            seconds(60.0),
            [seconds(20.0); 3],
            Some(UtcMillis(65_000))
        ),
        Some(UtcMillis(65_000))
    );
    assert!(completion(None, None, [seconds(20.0), None, None], None).is_none());
    assert_eq!(
        completion(None, None, [None; 3], Some(UtcMillis(65_000))),
        Some(UtcMillis(65_000))
    );
    assert_eq!(
        completion(Some(UtcMillis(i64::MAX)), seconds(60.0), [None; 3], None),
        None
    );
    for message in [
        "CAR 4 (NOR) STOPPED ON TRACK",
        "CAR 4 (NOR) REPORTS CAR 16 - RETIRED",
        "CAR 4 (NOR) LAP TIME NOT DELETED - LAP 2",
        "CAR 4 (NOR) LAP TIME UNDER INVESTIGATION",
        "CAR 4 (NOR) DELETED - LAP 0",
    ] {
        assert!(control_decision(message).is_none());
    }
    let (driver, payload) =
        control_decision("CAR 22 (TSU) LAP DELETED - TRACK LIMITS AT TURN 2 LAP 7 15:12:44 (PIT)")
            .unwrap();
    assert_eq!(driver, 22);
    assert!(matches!(payload,Payload::LapDecision{lap,..} if lap.get()==7));
}

#[test]
fn classification_late_correction_replaces_earlier_completion_and_conflicts_withhold_values() {
    let t = timeline(vec![
        known(4, 60_000, lap(1, Some(60.0), Eligibility::Eligible)),
        ev(
            4,
            59_000,
            Availability::Known(UtcMillis(65_000)),
            lap(1, Some(59.0), Eligibility::Eligible),
        ),
    ]);
    assert_eq!(
        frame(&t, 64_999).samples()[0]
            .lap_time
            .value()
            .unwrap()
            .get(),
        60.0
    );
    let corrected = frame(&t, 65_000);
    assert_eq!(corrected.samples()[0].lap_time.value().unwrap().get(), 59.0);
    assert_eq!(
        corrected.samples()[0].personal_best.timing().available_at,
        Availability::Known(UtcMillis(65_000))
    );
    let conflict = timeline(vec![
        known(4, 60_000, lap(1, Some(60.0), Eligibility::Eligible)),
        known(4, 60_000, lap(1, Some(61.0), Eligibility::Eligible)),
    ]);
    let f = frame(&conflict, 60_000);
    assert!(f.samples()[0].lap_time.value().is_none());
    assert!(f.samples()[0].personal_best.value().is_none());
    assert!(f.samples()[0].sectors.iter().all(|s| s.value().is_none()));
}
#[test]
fn classification_zero_and_inconsistent_laps_cannot_enter_the_timing_contract() {
    let payload = lap(1, Some(0.0), Eligibility::Eligible);
    assert!(
        Event::new(
            4,
            UtcMillis(1000),
            Availability::Known(UtcMillis(1000)),
            1,
            payload
        )
        .is_err()
    );
    let mut value = match lap(1, Some(60.0), Eligibility::Eligible) {
        Payload::Lap(v) => v,
        _ => unreachable!(),
    };
    value.sectors[0] = Some(Seconds::new(30.0).unwrap());
    assert!(
        Event::new(
            4,
            UtcMillis(1000),
            Availability::Known(UtcMillis(1000)),
            1,
            Payload::Lap(value)
        )
        .is_err()
    );
}
