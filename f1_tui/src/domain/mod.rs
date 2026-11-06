//! Validated data contracts. Legacy display models are deliberately separate.
//! Numeric units are carried by Rust types and explicit field names in JSON.
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    num::{NonZeroU16, NonZeroU32},
};

pub mod classification;
mod legacy;
pub mod normalize;
pub mod timing;
pub use legacy::{adapt_legacy_demo, parse_legacy_gap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError(pub &'static str);
impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ContractError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    OpenF1,
    FastF1,
    LegacyDemo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SessionIdWire")]
pub struct SessionId {
    provider: Provider,
    key: String,
}
#[derive(Deserialize)]
struct SessionIdWire {
    provider: Provider,
    key: String,
}
impl SessionId {
    pub fn new(provider: Provider, key: impl Into<String>) -> Result<Self, ContractError> {
        let key = key.into();
        if key.trim().is_empty() || key.trim() != key {
            return Err(ContractError(
                "session key must be nonempty and have no surrounding whitespace",
            ));
        }
        Ok(Self { provider, key })
    }
    pub fn provider(&self) -> Provider {
        self.provider
    }
    pub fn key(&self) -> &str {
        &self.key
    }
}
impl TryFrom<SessionIdWire> for SessionId {
    type Error = ContractError;
    fn try_from(w: SessionIdWire) -> Result<Self, Self::Error> {
        Self::new(w.provider, w.key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverId {
    pub session: SessionId,
    pub number: NonZeroU32,
}
impl DriverId {
    pub fn new(session: SessionId, number: u32) -> Result<Self, ContractError> {
        Ok(Self {
            session,
            number: NonZeroU32::new(number)
                .ok_or(ContractError("driver number must be nonzero"))?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionKind {
    Race,
    Sprint,
    Qualifying,
    SprintQualifying,
    Practice,
    Unknown(String),
}

/// Channel availability is established from evidence, not inferred from session type.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCapabilities {
    pub timing: bool,
    pub classification: bool,
    pub intervals: bool,
    pub telemetry: bool,
    pub location: bool,
    pub stints: bool,
    pub race_control: bool,
    pub weather: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDescriptor {
    pub id: SessionId,
    pub year: NonZeroU16,
    pub kind: SessionKind,
    pub capabilities: SessionCapabilities,
}

macro_rules! scalar_unit {
    ($name:ident, $max:expr, $unit:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
        #[serde(try_from = "f64", into = "f64")]
        pub struct $name(f64);
        impl $name {
            pub const UNIT: &'static str = $unit;
            pub fn new(value: f64) -> Result<Self, ContractError> {
                if !value.is_finite() || value < 0.0 || value > $max {
                    return Err(ContractError(concat!(
                        stringify!($name),
                        " is outside its finite nonnegative domain"
                    )));
                }
                Ok(Self(value))
            }
            pub fn get(self) -> f64 {
                self.0
            }
        }
        impl TryFrom<f64> for $name {
            type Error = ContractError;
            fn try_from(v: f64) -> Result<Self, Self::Error> {
                Self::new(v)
            }
        }
        impl From<$name> for f64 {
            fn from(v: $name) -> Self {
                v.0
            }
        }
    };
}
scalar_unit!(Percent, 100.0, "%");
scalar_unit!(SpeedKmh, f64::MAX, "km/h");
scalar_unit!(Seconds, f64::MAX, "s");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rpm(pub u32);
impl Rpm {
    pub const UNIT: &'static str = "rpm";
}

/// 0 is neutral; forward gears 1 through 8 are supported by the current sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Gear(u8);
impl Gear {
    pub fn new(value: u8) -> Result<Self, ContractError> {
        if value > 8 {
            return Err(ContractError("gear must be neutral or 1 through 8"));
        }
        Ok(Self(value))
    }
    pub fn get(self) -> u8 {
        self.0
    }
}
impl TryFrom<u8> for Gear {
    type Error = ContractError;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}
impl From<Gear> for u8 {
    fn from(v: Gear) -> Self {
        v.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrakeState {
    Released,
    Pressed,
}
impl BrakeState {
    pub fn from_raw(value: u32) -> Result<Self, ContractError> {
        match value {
            0 => Ok(Self::Released),
            100 => Ok(Self::Pressed),
            _ => Err(ContractError(
                "brake must be 0 (released) or 100 (pressed), not pressure",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrsState {
    Off,
    Eligible,
    On,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DrsCode(pub u32);
impl DrsCode {
    pub fn state(self) -> DrsState {
        match self.0 {
            0 | 1 => DrsState::Off,
            8 => DrsState::Eligible,
            10 | 12 | 14 => DrsState::On,
            _ => DrsState::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Gap {
    Leader,
    Seconds(Seconds),
    LapsBehind(NonZeroU32),
}

/// Absolute UTC Unix milliseconds and elapsed playback milliseconds cannot mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UtcMillis(pub i64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ElapsedMillis(pub u64);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Availability {
    Known(UtcMillis),
    Estimated(UtcMillis),
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timing {
    pub observed_at: Option<UtcMillis>,
    pub available_at: Availability,
    /// Retrieval is not original delivery/availability time.
    pub retrieved_at: Option<UtcMillis>,
    pub elapsed: Option<ElapsedMillis>,
}
impl Timing {
    fn validate(&self) -> Result<(), ContractError> {
        let availability = match self.available_at {
            Availability::Known(t) | Availability::Estimated(t) => Some(t),
            Availability::Unknown => None,
        };
        if let (Some(observed), Some(available)) = (self.observed_at, availability)
            && available < observed
        {
            return Err(ContractError(
                "availability cannot precede observation time",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Observed,
    Derived,
    Interpolated,
    Stale,
    Unavailable,
    Generated,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    Recorded,
    Generated,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub session: SessionId,
    pub channel: String,
    pub origin: Origin,
}

/// Fields are private so construction and JSON decoding share the same checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "ObservationWire<T>",
    bound(deserialize = "T: Deserialize<'de>")
)]
pub struct Observation<T> {
    value: Option<T>,
    source: Source,
    timing: Timing,
    quality: Quality,
    reason: Option<String>,
}
#[derive(Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct ObservationWire<T> {
    value: Option<T>,
    source: Source,
    timing: Timing,
    quality: Quality,
    reason: Option<String>,
}
impl<T> Observation<T> {
    pub fn new(
        value: Option<T>,
        source: Source,
        timing: Timing,
        quality: Quality,
        reason: Option<String>,
    ) -> Result<Self, ContractError> {
        timing.validate()?;
        if source.channel.trim().is_empty() {
            return Err(ContractError("source channel must be nonempty"));
        }
        if value.is_none() != (quality == Quality::Unavailable) {
            return Err(ContractError(
                "only unavailable observations may have no value",
            ));
        }
        if quality == Quality::Unavailable && reason.as_deref().is_none_or(|r| r.trim().is_empty())
        {
            return Err(ContractError("unavailable observations need a reason"));
        }
        if source.session.provider == Provider::LegacyDemo && source.origin != Origin::Generated {
            return Err(ContractError("legacy demo source cannot be recorded"));
        }
        if value.is_some()
            && (source.origin == Origin::Generated) != (quality == Quality::Generated)
        {
            return Err(ContractError("generated provenance and quality must agree"));
        }
        if matches!(quality, Quality::Observed | Quality::Stale) && timing.observed_at.is_none() {
            return Err(ContractError(
                "observed or stale values need an observation timestamp",
            ));
        }
        Ok(Self {
            value,
            source,
            timing,
            quality,
            reason,
        })
    }
    pub fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }
    pub fn source(&self) -> &Source {
        &self.source
    }
    pub fn timing(&self) -> &Timing {
        &self.timing
    }
    pub fn quality(&self) -> Quality {
        self.quality
    }
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
    /// Conservative admission: estimated availability requires a separate policy.
    pub fn available_by(&self, time: UtcMillis) -> bool {
        self.value.is_some()
            && self.source.origin == Origin::Recorded
            && matches!(self.timing.available_at, Availability::Known(t) if t <= time)
    }
    fn validate_for(&self, mode: PlaybackMode, session: &SessionId) -> Result<(), ContractError> {
        if &self.source.session != session {
            return Err(ContractError("observation belongs to another session"));
        }
        if mode == PlaybackMode::Recorded && self.source.origin != Origin::Recorded {
            return Err(ContractError(
                "generated observation cannot enter recorded playback",
            ));
        }
        Ok(())
    }
}
impl<T> TryFrom<ObservationWire<T>> for Observation<T> {
    type Error = ContractError;
    fn try_from(w: ObservationWire<T>) -> Result<Self, Self::Error> {
        Self::new(w.value, w.source, w.timing, w.quality, w.reason)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CarSample {
    pub driver: DriverId,
    pub position: Observation<NonZeroU32>,
    pub speed_kmh: Observation<SpeedKmh>,
    pub rpm: Observation<Rpm>,
    pub gear: Observation<Gear>,
    pub throttle: Observation<Percent>,
    pub brake: Observation<BrakeState>,
    pub drs: Observation<DrsCode>,
    pub gap_to_leader: Observation<Gap>,
    pub gap_to_ahead: Observation<Gap>,
    pub sectors: [Observation<Seconds>; 3],
    pub lap_number: Observation<NonZeroU32>,
    pub lap_time: Observation<Seconds>,
    pub lap_eligibility: Observation<timing::Eligibility>,
    pub personal_best: Observation<timing::BestLap>,
    pub status: Observation<classification::DriverStatus>,
}
impl CarSample {
    fn validate_for(&self, mode: PlaybackMode, session: &SessionId) -> Result<(), ContractError> {
        if &self.driver.session != session {
            return Err(ContractError("driver belongs to another session"));
        }
        self.lap_number.validate_for(mode, session)?;
        self.lap_time.validate_for(mode, session)?;
        self.lap_eligibility.validate_for(mode, session)?;
        self.personal_best.validate_for(mode, session)?;
        self.status.validate_for(mode, session)?;
        if self.lap_time.value().is_some_and(|v| v.get() <= 0.0)
            || self.personal_best.value().is_some_and(|v| {
                v.duration.get() <= 0.0 || v.eligibility == timing::Eligibility::Ineligible
            })
        {
            return Err(ContractError(
                "Invalid completed/best lap duration or eligibility",
            ));
        }
        self.position.validate_for(mode, session)?;
        self.speed_kmh.validate_for(mode, session)?;
        self.rpm.validate_for(mode, session)?;
        self.gear.validate_for(mode, session)?;
        self.throttle.validate_for(mode, session)?;
        self.brake.validate_for(mode, session)?;
        self.drs.validate_for(mode, session)?;
        self.gap_to_leader.validate_for(mode, session)?;
        self.gap_to_ahead.validate_for(mode, session)?;
        for sector in &self.sectors {
            sector.validate_for(mode, session)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackMode {
    Demo,
    Recorded,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PlaybackFrameWire")]
pub struct PlaybackFrame {
    mode: PlaybackMode,
    session: SessionId,
    elapsed: ElapsedMillis,
    samples: Vec<CarSample>,
}
#[derive(Deserialize)]
struct PlaybackFrameWire {
    mode: PlaybackMode,
    session: SessionId,
    elapsed: ElapsedMillis,
    samples: Vec<CarSample>,
}
impl PlaybackFrame {
    pub fn new(
        mode: PlaybackMode,
        session: SessionId,
        elapsed: ElapsedMillis,
        samples: Vec<CarSample>,
    ) -> Result<Self, ContractError> {
        if mode == PlaybackMode::Recorded && session.provider == Provider::LegacyDemo {
            return Err(ContractError(
                "legacy session cannot enter recorded playback",
            ));
        }
        let mut drivers = std::collections::HashSet::new();
        for sample in &samples {
            sample.validate_for(mode, &session)?;
            if !drivers.insert(sample.driver.number) {
                return Err(ContractError("duplicate driver in frame"));
            }
        }
        Ok(Self {
            mode,
            session,
            elapsed,
            samples,
        })
    }
    pub fn mode(&self) -> PlaybackMode {
        self.mode
    }
    pub fn session(&self) -> &SessionId {
        &self.session
    }
    pub fn elapsed(&self) -> ElapsedMillis {
        self.elapsed
    }
    pub fn samples(&self) -> &[CarSample] {
        &self.samples
    }
    /// Smallest admitted personal best; Unknown eligibility remains provisional.
    pub fn session_best(&self) -> Option<(NonZeroU32, &timing::BestLap)> {
        self.samples
            .iter()
            .filter_map(|s| s.personal_best.value().map(|b| (s.driver.number, b)))
            .min_by(|a, b| {
                a.1.duration
                    .get()
                    .total_cmp(&b.1.duration.get())
                    .then_with(|| a.0.cmp(&b.0))
            })
    }
    pub fn into_recorded(self) -> Result<Self, ContractError> {
        Self::new(
            PlaybackMode::Recorded,
            self.session,
            self.elapsed,
            self.samples,
        )
    }
}
impl TryFrom<PlaybackFrameWire> for PlaybackFrame {
    type Error = ContractError;
    fn try_from(w: PlaybackFrameWire) -> Result<Self, Self::Error> {
        Self::new(w.mode, w.session, w.elapsed, w.samples)
    }
}
