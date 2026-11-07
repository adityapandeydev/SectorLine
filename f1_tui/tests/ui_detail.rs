use sectorline::{
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
