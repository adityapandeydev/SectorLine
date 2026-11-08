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
