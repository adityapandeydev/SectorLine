//! A completed-lap summary, not an instantaneous stream of sector crossings.
use super::{ContractError, Seconds};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Eligibility {
    Eligible,
    Ineligible,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LapTiming {
    pub number: NonZeroU32,
    pub duration: Option<Seconds>,
    pub sectors: [Option<Seconds>; 3],
    pub pit_out: Option<bool>,
    pub eligibility: Eligibility,
}
impl LapTiming {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self
            .duration
            .iter()
            .chain(self.sectors.iter().flatten())
            .any(|v| v.get() <= 0.0)
        {
            return Err(ContractError("Timing durations must be positive"));
        }
        if let Some(duration) = self.duration
            && let Some(sectors) = self.sectors.into_iter().collect::<Option<Vec<_>>>()
            && (duration.get() - sectors.iter().map(|s| s.get()).sum::<f64>()).abs() > 0.01
        {
            return Err(ContractError("Lap and sector totals disagree"));
        }
        Ok(())
    }
    pub fn best(&self, eligibility: Eligibility) -> Option<BestLap> {
        if self.pit_out != Some(false) || eligibility == Eligibility::Ineligible {
            return None;
        }
        self.duration.map(|duration| BestLap {
            number: self.number,
            duration,
            eligibility,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BestLap {
    pub number: NonZeroU32,
    pub duration: Seconds,
    pub eligibility: Eligibility,
}

/// Conservative completion boundary for a retrospective lap summary.
/// The next lap start can delay, never accelerate, an end inferred from durations.
pub fn completion(
    start: Option<super::UtcMillis>,
    duration: Option<Seconds>,
    sectors: [Option<Seconds>; 3],
    next: Option<super::UtcMillis>,
) -> Option<super::UtcMillis> {
    let sector_sum = sectors
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .map(|s| s.iter().map(|s| s.get()).sum::<f64>());
    let seconds = duration
        .map(|s| s.get())
        .into_iter()
        .chain(sector_sum)
        .reduce(f64::max);
    let inferred = start.zip(seconds).and_then(|(s, d)| {
        let ms = (d * 1000.0).ceil();
        if !ms.is_finite() || ms <= 0.0 || ms >= i64::MAX as f64 {
            return None;
        }
        s.0.checked_add(ms as i64).map(super::UtcMillis)
    });
    match (inferred, next) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}
