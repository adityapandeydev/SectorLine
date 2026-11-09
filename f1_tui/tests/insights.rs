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
