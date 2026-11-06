//! Timestamped core samples and conservative completed-lap summaries.
use super::*;
use crate::data::openf1::{Channel, Result, ValidatedChunk};
use chrono::DateTime;
use serde_json::Value;
mod laps;
pub(crate) use laps::append_control;
pub use laps::control_decision;

#[derive(Debug, Clone, PartialEq)]
pub struct RosterDriver {
    pub number: NonZeroU32,
    pub code: String,
    pub name: String,
}
impl RosterDriver {
    pub fn new(number: u32, code: &str, name: &str) -> Result<Self> {
        if [code, name]
            .iter()
            .any(|s| s.trim().is_empty() || s.chars().any(char::is_control))
        {
            return Err("Invalid roster label".into());
        }
        Ok(Self {
            number: NonZeroU32::new(number).ok_or("Invalid driver number")?,
            code: code.into(),
            name: name.into(),
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TelemetryValues {
    pub speed: Option<SpeedKmh>,
    pub rpm: Option<Rpm>,
    pub gear: Option<Gear>,
    pub throttle: Option<Percent>,
    pub brake: Option<BrakeState>,
    pub drs: Option<DrsCode>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Coordinates {
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
}
impl Coordinates {
    pub fn new(x: Option<f64>, y: Option<f64>, z: Option<f64>) -> Result<Self> {
        if [x, y, z].into_iter().flatten().any(|v| !v.is_finite()) {
            return Err("Nonfinite location".into());
        }
        Ok(Self { x, y, z })
    }
    pub fn xyz(&self) -> (Option<f64>, Option<f64>, Option<f64>) {
        (self.x, self.y, self.z)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Payload {
    Position(Option<NonZeroU32>),
    Lap(Box<timing::LapTiming>),
    LapDecision {
        lap: NonZeroU32,
        eligibility: timing::Eligibility,
    },
    Status(classification::DriverStatus),
    Telemetry(TelemetryValues),
    Location(Coordinates),
    Intervals {
        leader: Option<Gap>,
        ahead: Option<Gap>,
    },
}
impl Payload {
    pub fn channel(&self) -> Channel {
        match self {
            Self::Position(_) => Channel::Position,
            Self::Lap(_) => Channel::Laps,
            Self::LapDecision { .. } | Self::Status(_) => Channel::RaceControl,
            Self::Telemetry(_) => Channel::CarData,
            Self::Location(_) => Channel::Location,
            Self::Intervals { .. } => Channel::Intervals,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Event {
    pub(crate) ambiguous: bool,
    pub(crate) driver: NonZeroU32,
    pub(crate) observed: UtcMillis,
    pub(crate) available: Availability,
    pub(crate) source: u32,
    pub(crate) payload: Payload,
}
impl Event {
    pub fn new(
        driver: u32,
        observed: UtcMillis,
        available: Availability,
        source: u32,
        payload: Payload,
    ) -> Result<Self> {
        if let Payload::Lap(lap) = &payload {
            lap.validate()?;
        }
        if matches!(available,Availability::Known(t)|Availability::Estimated(t) if t<observed) {
            return Err("Availability precedes observation".into());
        }
        Ok(Self {
            ambiguous: false,
            driver: NonZeroU32::new(driver).ok_or("Invalid driver number")?,
            observed,
            available,
            source,
            payload,
        })
    }
}

/// Round fractional milliseconds forward, so a sample is never admitted early.
/// Raw archive bytes retain the original RFC3339 precision.
pub fn utc_millis_ceiling(text: &str) -> Result<UtcMillis> {
    let time = DateTime::parse_from_rfc3339(text)?;
    Ok(UtcMillis(
        time.timestamp_millis()
            .checked_add(i64::from(time.timestamp_subsec_nanos() % 1_000_000 != 0))
            .ok_or("Timestamp overflow")?,
    ))
}

pub(crate) fn roster(chunk: &ValidatedChunk) -> Result<Vec<RosterDriver>> {
    chunk
        .records()
        .iter()
        .map(|row| {
            RosterDriver::new(
                number(row, "driver_number")?,
                row["name_acronym"].as_str().unwrap_or("--"),
                row["full_name"].as_str().unwrap_or("--"),
            )
        })
        .collect()
}
pub(crate) fn append(chunk: &ValidatedChunk, source: u32, events: &mut Vec<Event>) -> Result<()> {
    if chunk.channel() == Channel::Laps {
        return laps::append_laps(chunk, source, events);
    }
    for row in chunk.records() {
        let observed = utc_millis_ceiling(row["date"].as_str().ok_or("Missing event timestamp")?)?;
        let payload = match chunk.channel() {
            Channel::Position => Payload::Position(
                row["position"]
                    .as_f64()
                    .map(|v| v as u64)
                    .map(|v| NonZeroU32::new(v as u32).ok_or("Invalid position"))
                    .transpose()?,
            ),
            Channel::CarData => Payload::Telemetry(TelemetryValues {
                speed: row["speed"].as_f64().map(SpeedKmh::new).transpose()?,
                rpm: row["rpm"].as_f64().map(|v| Rpm(v as u32)),
                gear: row["n_gear"]
                    .as_f64()
                    .map(|v| Gear::new(v as u8))
                    .transpose()?,
                throttle: row["throttle"].as_f64().map(Percent::new).transpose()?,
                brake: row["brake"]
                    .as_f64()
                    .map(|v| BrakeState::from_raw(v as u32))
                    .transpose()?,
                drs: row["drs"].as_f64().map(|v| DrsCode(v as u32)),
            }),
            Channel::Location => Payload::Location(Coordinates::new(
                row["x"].as_f64(),
                row["y"].as_f64(),
                row["z"].as_f64(),
            )?),
            Channel::Intervals => Payload::Intervals {
                leader: gap(&row["gap_to_leader"])?,
                ahead: gap(&row["interval"])?,
            },
            _ => return Err("Unsupported core event channel".into()),
        };
        events.push(Event::new(
            number(row, "driver_number")?,
            observed,
            Availability::Estimated(observed),
            source,
            payload,
        )?);
    }
    Ok(())
}
fn number(row: &Value, field: &str) -> Result<u32> {
    Ok(row[field]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v > 0)
        .ok_or("Invalid source identity")?)
}
fn gap(value: &Value) -> Result<Option<Gap>> {
    match value {
        Value::Null => Ok(None),
        Value::Number(_) => Ok(Some(Gap::Seconds(Seconds::new(
            value.as_f64().ok_or("Invalid gap")?,
        )?))),
        Value::String(s) => Ok(Some(parse_legacy_gap(s)?)),
        _ => Err("Invalid gap type".into()),
    }
}
