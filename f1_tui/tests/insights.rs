use sectorline::{
    data::{
        limits::Cancellation,
        openf1::{Channel, ChunkMetadata},
    },
    domain::{
        capabilities::SessionContext,
        normalize::{Event, Payload, RosterDriver},
        *,
    },
    insights::{InsightKind, Policy},
    replay::{RecordedReplay, timeline::Timeline},
};
fn session() -> SessionId {
    SessionId::new(Provider::OpenF1, "9590").unwrap()
}
fn source(channel: Channel, retrieved: i64) -> ChunkMetadata {
    ChunkMetadata {
        channel,
        session_key: 9590,
        meeting_key: 1244,
        window: None,
        source_url: format!(
            "https://api.openf1.org/v1/{}?session_key=9590",
            channel.endpoint()
        ),
        retrieved_at: UtcMillis(retrieved),
        status: 200,
        attempts: 1,
    }
}
fn event(at: i64, available: i64, source: u32, payload: Payload) -> Event {
    Event::new(
        4,
        UtcMillis(at),
        Availability::Known(UtcMillis(available)),
        source,
        payload,
    )
    .unwrap()
}
fn interval(at: i64, available: i64, gap: Option<f64>, ahead: Option<Gap>) -> Event {
    event(
        at,
        available,
        0,
        Payload::Intervals {
            leader: gap.map(|s| Gap::Seconds(Seconds::new(s).unwrap())),
            ahead,
        },
    )
}
fn position(at: i64, number: u32) -> Event {
    event(
        at,
        at,
        1,
        Payload::Position(std::num::NonZeroU32::new(number)),
    )
}
fn recording(
    events: Vec<Event>,
    conditions: sectorline::domain::conditions::ConditionsArchive,
) -> RecordedReplay {
    let t = Timeline::new(
        session(),
        UtcMillis(0),
        vec![RosterDriver::new(4, "TST", "SYNTHETIC DRIVER").unwrap()],
        vec![
            source(Channel::Intervals, 900000),
            source(Channel::Position, 900000),
            source(Channel::Laps, 900000),
            source(Channel::RaceControl, 900000),
            source(Channel::Intervals, 900001),
        ],
        events,
    )
    .unwrap()
    .with_conditions(conditions);
    RecordedReplay::with_navigation(
        t,
        "SYNTHETIC INSIGHT FIXTURE".into(),
        Default::default(),
        &Cancellation::default(),
    )
    .unwrap()
    .with_context(SessionContext::from_names(session(), "Race", "Race"))
    .unwrap()
}
fn race(events: Vec<Event>) -> RecordedReplay {
    recording(events, Default::default())
}
fn gap(s: f64) -> Option<Gap> {
    Some(Gap::Seconds(Seconds::new(s).unwrap()))
}
#[test]
fn insights_intervals_are_descriptive_fresh_and_timestamped() {
    let mut r = race(vec![
        interval(0, 0, Some(5.0), gap(1.2)),
        interval(5000, 5000, Some(4.5), gap(1.1)),
        interval(10000, 12000, Some(4.0), gap(0.7)),
        position(80000, 4),
    ]);
    r.seek(ElapsedMillis(11999)).unwrap();
    assert!(
        !r.insights()
            .iter()
            .any(|i| i.kind == InsightKind::NearbyInterval)
    );
    r.seek(ElapsedMillis(12000)).unwrap();
    let rows = r.insights();
    let trend = rows
        .iter()
        .find(|i| i.kind == InsightKind::GapTrend)
        .unwrap();
    assert!(trend.summary.contains("-1.000s"));
    assert_eq!(trend.window_start, UtcMillis(0));
    assert_eq!(trend.observed, UtcMillis(10000));
    assert_eq!(trend.available, Availability::Known(UtcMillis(12000)));
    assert!(trend.evidence.contains("3 reports"));
    let nearby = rows
        .iter()
        .find(|i| i.kind == InsightKind::NearbyInterval)
        .unwrap();
    assert!(nearby.summary.contains("0.700s"));
    assert!(!nearby.summary.contains("attack"));
    assert_eq!(
        rows,
        r.insights(),
        "Repeated redraw must not append notices"
    );
    assert!(Policy::new(9999).is_err());
    assert!(Policy::new(120001).is_err());
    r.seek(ElapsedMillis(20001)).unwrap();
    assert!(
        !r.insights()
            .iter()
            .any(|i| matches!(i.kind, InsightKind::GapTrend | InsightKind::NearbyInterval))
    );
    r.seek(ElapsedMillis(12000)).unwrap();
    assert_eq!(rows, r.insights());
}
#[test]
fn insights_do_not_bridge_missing_lapped_conflicting_or_sparse_reports() {
    for middle in [
        None,
        Some(Gap::LapsBehind(std::num::NonZeroU32::new(1).unwrap())),
    ] {
        let e = event(
            5000,
            5000,
            0,
            Payload::Intervals {
                leader: middle,
                ahead: None,
            },
        );
        let mut r = race(vec![
            interval(0, 0, Some(5.0), None),
            e,
            interval(10000, 10000, Some(4.0), None),
        ]);
        r.seek(ElapsedMillis(10000)).unwrap();
        assert!(!r.insights().iter().any(|i| i.kind == InsightKind::GapTrend));
    }
    let mut r = race(vec![
        interval(0, 0, Some(5.0), None),
        interval(10000, 10000, Some(4.0), None),
    ]);
    r.seek(ElapsedMillis(10000)).unwrap();
    assert!(!r.insights().iter().any(|i| i.kind == InsightKind::GapTrend));
}

#[test]
fn insights_resolve_only_available_corrections_and_drop_conflicts() {
    let base = vec![
        interval(0, 0, Some(5.0), None),
        interval(5000, 5000, Some(4.5), None),
        interval(10000, 10000, Some(4.0), gap(0.8)),
    ];
    let mut conflict = base.clone();
    conflict.push(interval(10000, 10000, Some(3.0), gap(0.6)));
    let mut r = race(conflict);
    r.seek(ElapsedMillis(10000)).unwrap();
    assert!(r.insights().is_empty());
    let mut corrected = base;
    corrected.push(event(
        10000,
        15000,
        4,
        Payload::Intervals {
            leader: gap(6.0),
            ahead: gap(1.5),
        },
    ));
    corrected.push(position(80000, 4));
    let mut r = race(corrected);
    r.seek(ElapsedMillis(14999)).unwrap();
    let old = r.insights();
    assert!(old.iter().any(|i| i.summary.contains("-1.000s")));
    r.seek(ElapsedMillis(15000)).unwrap();
    let new = r.insights();
    assert!(new.iter().any(|i| i.summary.contains("+1.000s")));
    assert!(!new.iter().any(|i| i.kind == InsightKind::NearbyInterval));
    assert!(
        !r.insights_with_policy(Policy::new(10000).unwrap())
            .iter()
            .any(|i| i.kind == InsightKind::GapTrend)
    );
    r.seek(ElapsedMillis(14999)).unwrap();
    assert_eq!(r.insights(), old);
    r.seek(ElapsedMillis(15000)).unwrap();
    assert_eq!(r.insights(), new);
}
#[test]
fn insights_position_changes_never_claim_an_overtake_or_causal_pit_stop() {
    use sectorline::domain::{
        conditions::Reading,
        stints::{PitRecord, Stint},
    };
    let mut c = sectorline::domain::conditions::ConditionsArchive::default();
    let pit = Reading::new(
        UtcMillis(9000),
        Availability::Estimated(UtcMillis(12000)),
        UtcMillis(900000),
        Some(PitRecord {
            lap: Some(2),
            lane_seconds: Some(25.0),
            stop_seconds: Some(2.4),
        }),
    )
    .unwrap();
    c.pits.insert(4, vec![pit.clone(), pit]);
    let tyre = Reading::new(
        UtcMillis(10000),
        Availability::Estimated(UtcMillis(13000)),
        UtcMillis(900000),
        Some(Stint {
            number: 2,
            lap_start: 3,
            compound: Some("HARD".into()),
            age_at_start: Some(0),
        }),
    )
    .unwrap();
    c.stints.insert(4, vec![tyre.clone(), tyre]);
    let mut r = recording(
        vec![
            position(0, 5),
            position(10000, 4),
            position(15000, 4),
            position(80000, 4),
        ],
        c,
    );
    r.seek(ElapsedMillis(11000)).unwrap();
    assert!(!r.insights().iter().any(|i| i.kind == InsightKind::PitVisit));
    r.seek(ElapsedMillis(13000)).unwrap();
    let rows = r.insights();
    assert_eq!(
        rows.iter()
            .filter(|i| i.kind == InsightKind::PitVisit)
            .count(),
        1
    );
    assert_eq!(
        rows.iter()
            .filter(|i| i.kind == InsightKind::TyreReport)
            .count(),
        1
    );
    let changed = rows
        .iter()
        .find(|i| i.kind == InsightKind::PositionChange)
        .unwrap();
    assert_eq!(changed.summary, "Reported position 5 -> 4");
    assert!(changed.evidence.contains("cause unestablished"));
    r.seek(ElapsedMillis(15000)).unwrap();
    assert_eq!(
        changed,
        r.insights()
            .iter()
            .find(|i| i.kind == InsightKind::PositionChange)
            .unwrap()
    );
    r.seek(ElapsedMillis(80000)).unwrap();
    assert!(r.insights().is_empty());
    r.seek(ElapsedMillis(13000)).unwrap();
    assert_eq!(r.insights(), rows);
}
fn lap(
    at: i64,
    n: u32,
    seconds: f64,
    eligibility: sectorline::domain::timing::Eligibility,
    pit_out: bool,
) -> Event {
    use sectorline::domain::timing::LapTiming;
    event(
        at,
        at,
        2,
        Payload::Lap(Box::new(LapTiming {
            number: std::num::NonZeroU32::new(n).unwrap(),
            duration: Some(Seconds::new(seconds).unwrap()),
            sectors: [None; 3],
            pit_out: Some(pit_out),
            eligibility,
        })),
    )
}
fn decision(
    at: i64,
    available: i64,
    n: u32,
    eligibility: sectorline::domain::timing::Eligibility,
) -> Event {
    event(
        at,
        available,
        3,
        Payload::LapDecision {
            lap: std::num::NonZeroU32::new(n).unwrap(),
            eligibility,
        },
    )
}
#[test]
fn insights_best_requires_explicit_eligibility_and_rebuilds_after_decisions() {
    use sectorline::domain::timing::Eligibility::*;
    let mut r = race(vec![
        lap(60000, 1, 60.0, Eligible, false),
        lap(120000, 2, 59.0, Unknown, false),
        decision(125000, 130000, 2, Eligible),
        decision(140000, 142000, 2, Ineligible),
        decision(145000, 146000, 2, Eligible),
        lap(200000, 3, 58.0, Eligible, true),
    ]);
    for (at, best) in [
        (59999, None),
        (60000, Some("60.000s")),
        (120000, Some("60.000s")),
        (129999, Some("60.000s")),
        (130000, Some("59.000s")),
        (142000, Some("60.000s")),
        (146000, Some("59.000s")),
        (200000, Some("59.000s")),
        (60000, Some("60.000s")),
    ] {
        r.seek(ElapsedMillis(at)).unwrap();
        let rows = r.insights();
        let eligible = rows.iter().find(|i| i.kind == InsightKind::EligibleBest);
        assert_eq!(eligible.is_some(), best.is_some());
        if let (Some(i), Some(s)) = (eligible, best) {
            assert!(i.summary.contains(s), "{at} {i:?}");
            assert!(i.window_start <= i.observed);
            assert!(match i.available {
                Availability::Known(t) | Availability::Estimated(t) =>
                    t.0 <= at as i64 && t >= i.observed,
                Availability::Unknown => false,
            });
        }
    }
    let mut unknown = race(vec![lap(60000, 1, 60.0, Unknown, false)]);
    unknown.seek(ElapsedMillis(60000)).unwrap();
    assert!(unknown.insights().is_empty());
    let qualifying = unknown
        .with_context(SessionContext::from_names(
            session(),
            "Qualifying",
            "Qualifying",
        ))
        .unwrap();
    assert!(qualifying.insights().is_empty());
}
#[test]
fn insights_hide_unknown_availability_and_nonrace_gap_rules() {
    let unknown = Event::new(
        4,
        UtcMillis(10000),
        Availability::Unknown,
        0,
        Payload::Intervals {
            leader: gap(3.0),
            ahead: gap(0.2),
        },
    )
    .unwrap();
    let mut r = race(vec![unknown, position(10000, 4)]);
    r.seek(ElapsedMillis(10000)).unwrap();
    assert!(r.insights().is_empty());
    let r = race(vec![interval(0, 0, Some(1.0), gap(0.2))])
        .with_context(SessionContext::from_names(
            session(),
            "Practice 1",
            "Practice",
        ))
        .unwrap();
    assert!(r.insights().is_empty());
}

#[test]
fn insights_never_bridge_long_gaps_or_claim_a_trend_from_a_burst() {
    for times in [[0, 20000, 25000], [0, 1000, 2000]] {
        let mut r = race(
            times
                .into_iter()
                .enumerate()
                .map(|(n, t)| interval(t, t, Some(5.0 - n as f64), None))
                .collect(),
        );
        r.seek(ElapsedMillis(times[2] as u64)).unwrap();
        assert!(!r.insights().iter().any(|i| i.kind == InsightKind::GapTrend));
    }
}

fn showcase() -> RecordedReplay {
    use sectorline::domain::{
        conditions::{ConditionsArchive, Reading},
        stints::{PitRecord, Stint},
        timing::Eligibility,
    };
    let c = ConditionsArchive {
        pits: std::collections::BTreeMap::from([(
            4,
            vec![
                Reading::new(
                    UtcMillis(59000),
                    Availability::Estimated(UtcMillis(61000)),
                    UtcMillis(900000),
                    Some(PitRecord {
                        lap: Some(1),
                        lane_seconds: Some(25.0),
                        stop_seconds: None,
                    }),
                )
                .unwrap(),
            ],
        )]),
        stints: std::collections::BTreeMap::from([(
            4,
            vec![
                Reading::new(
                    UtcMillis(60000),
                    Availability::Estimated(UtcMillis(61000)),
                    UtcMillis(900000),
                    Some(Stint {
                        number: 2,
                        lap_start: 2,
                        compound: Some("HARD".into()),
                        age_at_start: Some(0),
                    }),
                )
                .unwrap(),
            ],
        )]),
        ..Default::default()
    };
    let mut r = recording(
        vec![
            interval(50000, 50000, Some(5.0), gap(1.2)),
            interval(55000, 55000, Some(4.5), gap(1.1)),
            interval(60000, 60000, Some(4.0), gap(0.7)),
            position(50000, 5),
            position(60000, 4),
            position(80000, 4),
            lap(60000, 1, 60.0, Eligibility::Eligible, false),
        ],
        c,
    );
    r.seek(ElapsedMillis(61000)).unwrap();
    r
}
fn app(r: RecordedReplay) -> sectorline::App {
    let mut a = sectorline::App::new(
        sectorline::TelemetryStream::load_from_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache"),
        )
        .unwrap(),
        false,
    );
    a.prepared_recording = Some(r);
    a
}
fn capture(a: &sectorline::App, w: u16, h: u16) -> (String, ratatui::buffer::Buffer) {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(|f| a.render(f)).unwrap();
    let b = t.backend().buffer().clone();
    let s = b
        .content()
        .chunks(w as usize)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    (s, b)
}
#[test]
fn insights_ui_exposes_windows_availability_and_rules_at_all_sizes() {
    for view in [sectorline::ui::View::Driver, sectorline::ui::View::Events] {
        let mut a = app(showcase());
        a.view = view;
        for (w, h) in [(144, 44), (120, 36), (80, 24)] {
            let mut all = String::new();
            let mut saved = false;
            for offset in (0..160).step_by(1) {
                a.panel_scroll = offset;
                let (text, buffer) = capture(&a, w, h);
                if !saved
                    && text.contains("Context | driver #4")
                    && text.contains("Reported leader gap")
                    && text.contains("Evidence window")
                    && text.contains("Available known")
                {
                    if let Some(root) = std::env::var_os("SECTORLINE_CAPTURE_DIR") {
                        let view = if view == sectorline::ui::View::Driver {
                            "driver"
                        } else {
                            "events"
                        };
                        let size = match w {
                            144 => "wide",
                            120 => "standard",
                            _ => "compact",
                        };
                        let cells=buffer.content().iter().map(|c|serde_json::json!({"s":c.symbol(),"fg":format!("{:?}",c.fg),"bg":format!("{:?}",c.bg),"m":format!("{:?}",c.modifier)})).collect::<Vec<_>>();
                        std::fs::write(std::path::Path::new(&root).join(format!("t19-{view}-{size}.json")),serde_json::to_vec(&serde_json::json!({"width":w,"height":h,"cells":cells,"fixture":"synthetic","scroll_rows":offset})).unwrap()).unwrap();
                    }
                    saved = true;
                }
                all.push_str(&text);
            }
            assert!(
                saved,
                "Context heading and first rule must be reachable together"
            );
            assert!(all.contains("Context | driver #4"), "{view:?} {w}x{h}");
            assert!(all.contains("Reported interval ahead"));
            assert!(all.contains("Evidence window"));
            assert!(all.contains("Available estimated"));
            assert!(all.contains("cause unestablished"));
            assert!(!all.contains("win chance"));
        }
    }
}

#[test]
fn insights_conflicting_or_invalid_condition_reports_withhold_a_notice() {
    use sectorline::domain::{
        conditions::{ConditionsArchive, Reading},
        stints::Stint,
    };
    let a = Reading::new(
        UtcMillis(0),
        Availability::Known(UtcMillis(0)),
        UtcMillis(900000),
        Some(Stint {
            number: 1,
            lap_start: 1,
            compound: Some("SOFT".into()),
            age_at_start: Some(0),
        }),
    )
    .unwrap();
    let mut b = a.clone();
    b.value.as_mut().unwrap().compound = Some("HARD".into());
    let c = ConditionsArchive {
        stints: std::collections::BTreeMap::from([(4, vec![a.clone(), b])]),
        ..Default::default()
    };
    assert!(recording(vec![position(0, 4)], c).insights().is_empty());
    let invalid = Reading::new(
        UtcMillis(1000),
        Availability::Known(UtcMillis(1000)),
        UtcMillis(900000),
        None,
    )
    .unwrap();
    let c = ConditionsArchive {
        stints: std::collections::BTreeMap::from([(4, vec![a, invalid])]),
        ..Default::default()
    };
    let mut r = recording(vec![position(0, 4), position(1000, 4)], c);
    r.seek(ElapsedMillis(1000)).unwrap();
    assert!(r.insights().is_empty());
}
#[test]
fn insights_events_can_show_context_when_there_are_no_parsed_reports() {
    let mut r = race(vec![
        interval(0, 0, Some(5.0), None),
        interval(5000, 5000, Some(4.5), None),
        interval(10000, 10000, Some(4.0), gap(0.5)),
    ]);
    r.seek(ElapsedMillis(10000)).unwrap();
    assert!(r.visible_events().is_empty());
    let mut a = app(r);
    a.view = sectorline::ui::View::Events;
    let text = capture(&a, 80, 24).0;
    assert!(text.contains("Evidence timeline | 0 available"));
    assert!(text.contains("Context | driver #4"));
    assert!(text.contains("Reported leader gap"));
}

#[test]
fn insights_recording_minimum_respects_availability_and_driver_selection() {
    use sectorline::domain::timing::{Eligibility, LapTiming};
    let other = Event::new(
        16,
        UtcMillis(60000),
        Availability::Estimated(UtcMillis(65000)),
        2,
        Payload::Lap(Box::new(LapTiming {
            number: std::num::NonZeroU32::new(1).unwrap(),
            duration: Some(Seconds::new(59.0).unwrap()),
            sectors: [None; 3],
            pit_out: Some(false),
            eligibility: Eligibility::Eligible,
        })),
    )
    .unwrap();
    let t = Timeline::new(
        session(),
        UtcMillis(0),
        vec![
            RosterDriver::new(4, "TST", "SYNTHETIC4").unwrap(),
            RosterDriver::new(16, "OTH", "SYNTHETIC16").unwrap(),
        ],
        vec![
            source(Channel::Intervals, 900000),
            source(Channel::Position, 900000),
            source(Channel::Laps, 900000),
        ],
        vec![lap(60000, 1, 60.0, Eligibility::Eligible, false), other],
    )
    .unwrap();
    let mut r = RecordedReplay::new(t, "SYNTHETIC MULTI-DRIVER FIXTURE".into()).unwrap();
    r.seek(ElapsedMillis(64999)).unwrap();
    let before = r.insights();
    assert!(before.iter().any(|i| i.evidence.contains("#4 60.000s")));
    r.seek(ElapsedMillis(65000)).unwrap();
    assert!(
        r.insights()
            .iter()
            .any(|i| i.evidence.contains("#16 59.000s"))
    );
    r.select(1);
    assert_eq!(r.selected(), 16);
    assert!(
        r.insights()
            .iter()
            .all(|i| i.driver == 16 && i.summary.contains("59.000s"))
    );
    r.select(-1);
    r.seek(ElapsedMillis(64999)).unwrap();
    assert_eq!(r.insights(), before);
}
#[test]
fn insights_qualifying_best_does_not_survive_a_part_change() {
    use sectorline::domain::{
        capabilities::{PhaseBoundary, PhaseUpdate},
        conditions::{ConditionsArchive, Reading},
        timing::Eligibility,
    };
    let phases = [(0, 1), (70000, 2)]
        .map(|(at, part)| {
            Reading::new(
                UtcMillis(at),
                Availability::Estimated(UtcMillis(at)),
                UtcMillis(900000),
                Some(PhaseUpdate {
                    part: Some(part),
                    boundary: PhaseBoundary::Start,
                }),
            )
            .unwrap()
        })
        .to_vec();
    let t = Timeline::new(
        session(),
        UtcMillis(0),
        vec![RosterDriver::new(4, "TST", "SYNTHETIC4").unwrap()],
        vec![
            source(Channel::Intervals, 900000),
            source(Channel::Position, 900000),
            source(Channel::Laps, 900000),
        ],
        vec![
            lap(60000, 1, 60.0, Eligibility::Eligible, false),
            lap(140000, 2, 69.0, Eligibility::Eligible, false),
        ],
    )
    .unwrap()
    .with_conditions(ConditionsArchive {
        phases,
        ..Default::default()
    });
    let mut starts = sectorline::replay::navigation::LapIndex::default();
    starts.insert(4, 1, UtcMillis(0));
    starts.insert(4, 2, UtcMillis(71000));
    let mut r = RecordedReplay::with_navigation(
        t,
        "SYNTHETIC QUALIFYING INSIGHT FIXTURE".into(),
        starts,
        &Cancellation::default(),
    )
    .unwrap()
    .with_context(SessionContext::from_names(
        session(),
        "Qualifying",
        "Qualifying",
    ))
    .unwrap();
    for (at, expected) in [
        (60000, Some("Q1: 60.000s")),
        (70000, None),
        (140000, Some("Q2: 69.000s")),
        (60000, Some("Q1: 60.000s")),
    ] {
        r.seek(ElapsedMillis(at)).unwrap();
        let rows = r.insights();
        let best = rows.iter().find(|i| i.kind == InsightKind::EligibleBest);
        assert_eq!(best.is_some(), expected.is_some());
        if let Some(s) = expected {
            assert!(best.unwrap().summary.contains(s));
            assert!(matches!(
                best.unwrap().available,
                Availability::Estimated(_)
            ));
        }
    }
}
