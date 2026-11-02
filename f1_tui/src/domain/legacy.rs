//! One-way adapter: preserves the legacy UI, never promotes its data to recorded.
use super::*;
use crate::models::TelemetryFrame;

pub fn parse_legacy_gap(text: &str) -> Result<Gap, ContractError> {
    let text = text.trim().to_ascii_uppercase();
    if text == "LEADER" {
        return Ok(Gap::Leader);
    }
    let number = text.strip_prefix('+').unwrap_or(&text);
    if number.starts_with(['+', '-']) {
        return Err(ContractError("gap must have at most one leading plus sign"));
    }
    for suffix in ["LAPS", "LAP"] {
        if let Some(laps) = number.strip_suffix(suffix) {
            let laps = laps
                .trim()
                .parse::<u32>()
                .ok()
                .and_then(NonZeroU32::new)
                .ok_or(ContractError("invalid lapped gap"))?;
            return Ok(Gap::LapsBehind(laps));
        }
    }
    let seconds = number
        .parse::<f64>()
        .map_err(|_| ContractError("gap has no numeric value"))?;
    Ok(Gap::Seconds(Seconds::new(seconds)?))
}

fn generated<T>(
    value: Result<T, ContractError>,
    source: &Source,
    timing: &Timing,
) -> Result<Observation<T>, ContractError> {
    match value {
        Ok(value) => Observation::new(
            Some(value),
            source.clone(),
            timing.clone(),
            Quality::Generated,
            None,
        ),
        Err(error) => Observation::new(
            None,
            source.clone(),
            timing.clone(),
            Quality::Unavailable,
            Some(error.to_string()),
        ),
    }
}

pub fn adapt_legacy_demo(
    session: &SessionId,
    frame: &TelemetryFrame,
) -> Result<PlaybackFrame, ContractError> {
    if session.provider() != Provider::LegacyDemo {
        return Err(ContractError(
            "legacy adapter requires an explicit demo identity",
        ));
    }
    let source = Source {
        session: session.clone(),
        channel: "legacy_demo".into(),
        origin: Origin::Generated,
    };
    let timing = Timing {
        elapsed: Some(ElapsedMillis(frame.timestamp_ms)),
        ..Timing::default()
    };
    let mut samples = Vec::with_capacity(frame.cars.len());
    for car in &frame.cars {
        let sectors = [&car.s1, &car.s2, &car.s3].map(|text| {
            let value = text
                .parse::<f64>()
                .map_err(|_| ContractError("invalid legacy sector"))
                .and_then(Seconds::new);
            generated(value, &source, &timing)
        });
        let [s1, s2, s3] = sectors;
        samples.push(CarSample {
            driver: DriverId::new(session.clone(), car.number)?,
            position: generated(
                NonZeroU32::new(car.position).ok_or(ContractError("missing classification")),
                &source,
                &timing,
            )?,
            speed_kmh: generated(SpeedKmh::new(car.speed), &source, &timing)?,
            rpm: generated(Ok(Rpm(car.rpm)), &source, &timing)?,
            gear: generated(
                u8::try_from(car.gear)
                    .map_err(|_| ContractError("invalid gear"))
                    .and_then(Gear::new),
                &source,
                &timing,
            )?,
            throttle: generated(Percent::new(f64::from(car.throttle)), &source, &timing)?,
            brake: generated(BrakeState::from_raw(car.brake), &source, &timing)?,
            // A boolean cannot recover whether the source code was 10, 12 or 14.
            drs: generated(
                Err(ContractError("legacy DRS boolean has no source code")),
                &source,
                &timing,
            )?,
            gap_to_leader: generated(parse_legacy_gap(&car.gap_to_leader), &source, &timing)?,
            gap_to_ahead: generated(parse_legacy_gap(&car.gap_to_ahead), &source, &timing)?,
            sectors: [s1?, s2?, s3?],
        });
    }
    PlaybackFrame::new(
        PlaybackMode::Demo,
        session.clone(),
        ElapsedMillis(frame.timestamp_ms),
        samples,
    )
}
