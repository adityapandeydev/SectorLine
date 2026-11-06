use sectorline::domain::*;
use std::{num::NonZeroU32, path::PathBuf};

fn session() -> SessionId {
    SessionId::new(Provider::OpenF1, "9590").unwrap()
}

fn source() -> Source {
    Source {
        session: session(),
        channel: "car_data".into(),
        origin: Origin::Recorded,
    }
}

fn timing() -> Timing {
    Timing {
        observed_at: Some(UtcMillis(1_000)),
        available_at: Availability::Known(UtcMillis(1_100)),
        retrieved_at: Some(UtcMillis(2_000)),
        elapsed: Some(ElapsedMillis(100)),
    }
}

#[test]
fn observation_contract_distinguishes_missing_from_zero() {
    let zero = Observation::new(
        Some(Percent::new(0.0).unwrap()),
        source(),
        timing(),
        Quality::Observed,
        None,
    )
    .unwrap();
    let missing = Observation::<Percent>::new(
        None,
        source(),
        Timing::default(),
        Quality::Unavailable,
        Some("No observation".into()),
    )
    .unwrap();
    assert_eq!(zero.value().unwrap().get(), 0.0);
    assert!(missing.value().is_none());
    assert_eq!(missing.quality(), Quality::Unavailable);
    assert!(zero.available_by(UtcMillis(1_100)));
    assert!(!zero.available_by(UtcMillis(1_099)));
    assert!(!missing.available_by(UtcMillis(9_000)));
    assert!(
        Observation::<Percent>::new(None, source(), timing(), Quality::Observed, None).is_err()
    );
    assert!(
        Observation::new(
            Some(Percent::new(0.0).unwrap()),
            source(),
            timing(),
            Quality::Unavailable,
            None
        )
        .is_err()
    );
}

#[test]
fn observation_contract_rejects_bad_values_in_constructors_and_json() {
    for value in [-1.0, 104.0, f64::NAN, f64::INFINITY] {
        assert!(Percent::new(value).is_err());
    }
    for value in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(SpeedKmh::new(value).is_err());
        assert!(Seconds::new(value).is_err());
    }
    assert!(serde_json::from_str::<Percent>("104").is_err());
    assert!(serde_json::from_str::<SpeedKmh>("-1").is_err());
    assert!(serde_json::from_str::<Seconds>("null").is_err());
    assert!(serde_json::from_str::<Gear>("9").is_err());
    assert!(serde_json::from_str::<Rpm>("-1").is_err());
    assert!(serde_json::from_str::<SessionId>(r#"{"provider":"OpenF1","key":" "}"#).is_err());
    assert!(DriverId::new(session(), 0).is_err());
    assert!(SessionId::new(Provider::OpenF1, "").is_err());
}

#[test]
fn observation_contract_keeps_brake_discrete_and_unknown_drs_codes() {
    assert_eq!(BrakeState::from_raw(0).unwrap(), BrakeState::Released);
    assert_eq!(BrakeState::from_raw(100).unwrap(), BrakeState::Pressed);
    assert!(BrakeState::from_raw(72).is_err());
    assert!(BrakeState::from_raw(104).is_err());
    for (code, state) in [
        (0, DrsState::Off),
        (1, DrsState::Off),
        (8, DrsState::Eligible),
        (14, DrsState::On),
        (9, DrsState::Unknown),
        (255, DrsState::Unknown),
    ] {
        let drs = DrsCode(code);
        assert_eq!(drs.state(), state);
        assert_eq!(
            serde_json::from_str::<DrsCode>(&serde_json::to_string(&drs).unwrap())
                .unwrap()
                .0,
            code
        );
    }
}

#[test]
fn observation_contract_preserves_gap_units() {
    assert_eq!(parse_legacy_gap("LEADER").unwrap(), Gap::Leader);
    assert_eq!(
        parse_legacy_gap("+2.145").unwrap(),
        Gap::Seconds(Seconds::new(2.145).unwrap())
    );
    assert_eq!(
        parse_legacy_gap("+2 LAPS").unwrap(),
        Gap::LapsBehind(NonZeroU32::new(2).unwrap())
    );
    for invalid in ["INTERVAL", "", "+NaN", "-1", "++1", "+0 LAPS", "unknown"] {
        assert!(parse_legacy_gap(invalid).is_err(), "{invalid}");
    }
    assert_ne!(
        Gap::Seconds(Seconds::new(1.0).unwrap()),
        Gap::LapsBehind(NonZeroU32::new(1).unwrap())
    );
}

#[test]
fn observation_contract_enforces_time_and_quality_during_deserialization() {
    let observation = Observation::new(
        Some(SpeedKmh::new(0.0).unwrap()),
        source(),
        timing(),
        Quality::Observed,
        None,
    )
    .unwrap();
    let mut wire = serde_json::to_value(&observation).unwrap();
    wire["timing"]["available_at"] = serde_json::json!({"Known": 999});
    assert!(serde_json::from_value::<Observation<SpeedKmh>>(wire).is_err());
    let mut wire = serde_json::to_value(&observation).unwrap();
    wire["quality"] = serde_json::json!("Unavailable");
    assert!(serde_json::from_value::<Observation<SpeedKmh>>(wire).is_err());
    let mut unknown = timing();
    unknown.available_at = Availability::Unknown;
    let value = Observation::new(
        Some(SpeedKmh::new(1.0).unwrap()),
        source(),
        unknown,
        Quality::Observed,
        None,
    )
    .unwrap();
    assert!(!value.available_by(UtcMillis(i64::MAX)));
    let mut estimated = timing();
    estimated.available_at = Availability::Estimated(UtcMillis(1_100));
    let value = Observation::new(
        Some(SpeedKmh::new(1.0).unwrap()),
        source(),
        estimated,
        Quality::Observed,
        None,
    )
    .unwrap();
    assert!(
        !value.available_by(UtcMillis(i64::MAX)),
        "estimated availability requires an explicit replay policy"
    );
}

fn observed<T>(value: T) -> Observation<T> {
    Observation::new(Some(value), source(), timing(), Quality::Observed, None).unwrap()
}

fn recorded_sample() -> CarSample {
    CarSample {
        driver: DriverId::new(session(), 4).unwrap(),
        lap_eligibility: observed(timing::Eligibility::Eligible),
        lap_number: observed(NonZeroU32::new(1).unwrap()),
        lap_time: observed(Seconds::new(90.0).unwrap()),
        personal_best: observed(timing::BestLap {
            number: NonZeroU32::new(1).unwrap(),
            duration: Seconds::new(90.0).unwrap(),
            eligibility: timing::Eligibility::Eligible,
        }),
        status: observed(classification::DriverStatus::Retired),
        position: observed(NonZeroU32::new(1).unwrap()),
        speed_kmh: observed(SpeedKmh::new(200.0).unwrap()),
        rpm: observed(Rpm(11_000)),
        gear: observed(Gear::new(6).unwrap()),
        throttle: observed(Percent::new(99.0).unwrap()),
        brake: observed(BrakeState::Released),
        drs: observed(DrsCode(14)),
        gap_to_leader: observed(Gap::Leader),
        gap_to_ahead: Observation::new(
            None,
            source(),
            Timing::default(),
            Quality::Unavailable,
            Some("Leader has no car ahead".into()),
        )
        .unwrap(),
        sectors: [
            observed(Seconds::new(30.0).unwrap()),
            observed(Seconds::new(40.0).unwrap()),
            observed(Seconds::new(20.0).unwrap()),
        ],
    }
}

#[test]
fn observation_contract_recorded_roundtrip_and_session_isolation() {
    let sample = recorded_sample();
    let frame = PlaybackFrame::new(
        PlaybackMode::Recorded,
        session(),
        ElapsedMillis(100),
        vec![sample.clone()],
    )
    .unwrap();
    let wire = serde_json::to_string(&frame).unwrap();
    assert_eq!(serde_json::from_str::<PlaybackFrame>(&wire).unwrap(), frame);
    assert!(
        PlaybackFrame::new(
            PlaybackMode::Recorded,
            session(),
            ElapsedMillis(100),
            vec![sample.clone(), sample.clone()]
        )
        .is_err()
    );
    let mut wrong_driver = sample.clone();
    wrong_driver.driver.session = SessionId::new(Provider::FastF1, "9590").unwrap();
    assert!(
        PlaybackFrame::new(
            PlaybackMode::Recorded,
            session(),
            ElapsedMillis(100),
            vec![wrong_driver]
        )
        .is_err()
    );
    let mut wrong_channel = sample;
    let mut another_source = source();
    another_source.session = SessionId::new(Provider::OpenF1, "9636").unwrap();
    wrong_channel.speed_kmh = Observation::new(
        Some(SpeedKmh::new(200.0).unwrap()),
        another_source,
        timing(),
        Quality::Observed,
        None,
    )
    .unwrap();
    assert!(
        PlaybackFrame::new(
            PlaybackMode::Recorded,
            session(),
            ElapsedMillis(100),
            vec![wrong_channel]
        )
        .is_err()
    );
}

#[test]
fn observation_contract_generated_channel_rejected_even_with_real_session_id() {
    let mut sample = recorded_sample();
    let generated_source = Source {
        origin: Origin::Generated,
        ..source()
    };
    sample.throttle = Observation::new(
        Some(Percent::new(50.0).unwrap()),
        generated_source.clone(),
        Timing::default(),
        Quality::Generated,
        None,
    )
    .unwrap();
    assert!(
        PlaybackFrame::new(
            PlaybackMode::Recorded,
            session(),
            ElapsedMillis(100),
            vec![sample.clone()]
        )
        .is_err()
    );
    sample.throttle = Observation::new(
        None,
        generated_source,
        Timing::default(),
        Quality::Unavailable,
        Some("Generated placeholder".into()),
    )
    .unwrap();
    assert!(
        PlaybackFrame::new(
            PlaybackMode::Recorded,
            session(),
            ElapsedMillis(100),
            vec![sample]
        )
        .is_err()
    );
    let mut wire = serde_json::to_value(observed(Percent::new(50.0).unwrap())).unwrap();
    wire["source"]["origin"] = serde_json::json!("Generated");
    assert!(serde_json::from_value::<Observation<Percent>>(wire).is_err());
}

#[test]
fn observation_contract_requires_event_time_and_retains_large_unknown_codes() {
    assert!(
        Observation::new(
            Some(Percent::new(0.0).unwrap()),
            source(),
            Timing::default(),
            Quality::Observed,
            None
        )
        .is_err()
    );
    assert!(
        Observation::new(
            Some(Percent::new(0.0).unwrap()),
            source(),
            Timing::default(),
            Quality::Stale,
            None
        )
        .is_err()
    );
    assert!(
        Observation::<Percent>::new(
            None,
            source(),
            Timing::default(),
            Quality::Unavailable,
            None
        )
        .is_err()
    );
    let unknown: DrsCode = serde_json::from_str("1024").unwrap();
    assert_eq!(unknown.state(), DrsState::Unknown);
    assert_eq!(unknown.0, 1024);
    assert_eq!(
        serde_json::from_str::<UtcMillis>("-1").unwrap(),
        UtcMillis(-1)
    );
    assert!(serde_json::from_str::<ElapsedMillis>("-1").is_err());
}

#[test]
fn observation_contract_demo_adapter_cannot_enter_recorded_mode() {
    let cache = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache");
    let stream = sectorline::TelemetryStream::load_from_dir(cache).unwrap();
    let id = SessionId::new(Provider::LegacyDemo, "bundled-demo").unwrap();
    let frame = adapt_legacy_demo(&id, stream.current_frame()).unwrap();
    assert_eq!(frame.mode(), PlaybackMode::Demo);
    assert_eq!(frame.samples().len(), stream.current_frame().cars.len());
    let sample = &frame.samples()[0];
    assert_eq!(sample.speed_kmh.quality(), Quality::Generated);
    assert_eq!(
        sample.speed_kmh.value().unwrap().get(),
        stream.current_frame().cars[0].speed
    );
    assert_eq!(sample.speed_kmh.timing().elapsed, Some(ElapsedMillis(0)));
    assert!(sample.speed_kmh.timing().observed_at.is_none());
    assert!(
        sample.drs.value().is_none(),
        "legacy boolean cannot recover a raw source DRS code"
    );
    assert!(frame.clone().into_recorded().is_err());
    let mut wire = serde_json::to_value(&frame).unwrap();
    wire["mode"] = serde_json::json!("Recorded");
    assert!(serde_json::from_value::<PlaybackFrame>(wire).is_err());
    assert!(adapt_legacy_demo(&session(), stream.current_frame()).is_err());
}

#[test]
fn observation_contract_adapter_marks_invalid_generated_values_unavailable() {
    let cache = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache");
    let stream = sectorline::TelemetryStream::load_from_dir(cache).unwrap();
    let mut legacy = stream.current_frame().clone();
    legacy.cars[0].throttle = 104;
    legacy.cars[0].brake = 72;
    legacy.cars[0].s1 = "not a sector".into();
    let frame = adapt_legacy_demo(
        &SessionId::new(Provider::LegacyDemo, "test").unwrap(),
        &legacy,
    )
    .unwrap();
    let sample = &frame.samples()[0];
    assert!(sample.throttle.value().is_none());
    assert!(sample.brake.value().is_none());
    assert!(sample.sectors[0].value().is_none());
    assert!(sample.throttle.reason().is_some());
    assert_eq!(sample.throttle.source().origin, Origin::Generated);
}

#[test]
fn observation_contract_capabilities_default_to_no_evidence() {
    let capabilities = SessionCapabilities::default();
    assert!(
        !capabilities.telemetry
            && !capabilities.location
            && !capabilities.intervals
            && !capabilities.race_control
    );
    assert_eq!(
        serde_json::from_str::<SessionKind>(r#"{"Unknown":"Future format"}"#).unwrap(),
        SessionKind::Unknown("Future format".into())
    );
}

#[test]
fn observation_contract_adapts_entire_demo_cache() {
    let cache = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache");
    let stream = sectorline::TelemetryStream::load_from_dir(cache).unwrap();
    let session = SessionId::new(Provider::LegacyDemo, "cache").unwrap();
    assert!(!stream.frames.is_empty());
    for legacy in &stream.frames {
        let frame = adapt_legacy_demo(&session, legacy).unwrap();
        assert_eq!(frame.samples().len(), legacy.cars.len());
        assert_eq!(frame.elapsed(), ElapsedMillis(legacy.timestamp_ms));
        for car in frame.samples() {
            assert_eq!(car.speed_kmh.source().origin, Origin::Generated);
            assert!(!car.speed_kmh.available_by(UtcMillis(i64::MAX)));
        }
        assert!(frame.into_recorded().is_err());
    }
}
