//! Explicit observation-time navigation; external recording time + offset = timeline time.
use crate::{
    data::{
        cache::SessionCache,
        limits::Cancellation,
        openf1::{Channel, Result},
    },
    domain::{ElapsedMillis, UtcMillis, normalize::utc_millis_ceiling},
};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct LapIndex(BTreeMap<(u32, u32), Option<UtcMillis>>);
impl LapIndex {
    pub fn insert(&mut self, driver: u32, lap: u32, start: UtcMillis) {
        if driver == 0 || lap == 0 {
            return;
        }
        self.0
            .entry((driver, lap))
            .and_modify(|old| {
                if *old != Some(start) {
                    *old = None;
                }
            })
            .or_insert(Some(start));
    }
    pub fn from_cache(cache: &SessionCache, cancel: &Cancellation) -> Result<Self> {
        let mut index = Self::default();
        for i in 0..cache.chunk_count() {
            cancel.check()?;
            if cache.chunk_metadata(i)?.channel != Channel::Laps {
                continue;
            }
            let chunk = cache.read_chunk(i)?;
            for row in chunk.records() {
                let fields = (
                    row["driver_number"].as_u64(),
                    row["lap_number"].as_u64(),
                    row["date_start"].as_str(),
                );
                if let (Some(driver), Some(lap), Some(date)) = fields
                    && let (Ok(driver), Ok(lap), Ok(start)) = (
                        u32::try_from(driver),
                        u32::try_from(lap),
                        utc_millis_ceiling(date),
                    )
                {
                    index.insert(driver, lap, start);
                }
            }
        }
        Ok(index)
    }
    pub fn target(
        &self,
        driver: u32,
        lap: u32,
        origin: UtcMillis,
        end: ElapsedMillis,
    ) -> Result<ElapsedMillis> {
        let start = self
            .0
            .get(&(driver, lap))
            .copied()
            .flatten()
            .ok_or("Lap start unavailable or conflicting for selected driver")?;
        let delta = start.0.checked_sub(origin.0).ok_or("Lap time overflow")?;
        let target =
            u64::try_from(delta).map_err(|_| "Lap starts before this recording's coverage")?;
        if target > end.0 {
            return Err("Lap starts after this recording's coverage".into());
        }
        Ok(ElapsedMillis(target))
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Navigation {
    Time(u64),
    Lap(u32),
    Offset(i64),
    Recording(u64),
}
impl Navigation {
    pub fn parse(text: &str) -> Result<Self> {
        let mut words = text.split_whitespace();
        let kind = words.next().ok_or("Enter time, lap, offset or recording")?;
        let value = words.next().ok_or("Missing navigation value")?;
        if words.next().is_some() {
            return Err("Too many navigation values".into());
        }
        Ok(match kind {
            "time" => Self::Time(parse_time(value)?),
            "recording" => Self::Recording(parse_time(value)?),
            "lap" => {
                let lap = value.parse::<u32>()?;
                if lap == 0 {
                    return Err("Lap must be positive".into());
                }
                Self::Lap(lap)
            }
            "offset" => {
                let (sign, rest) = match value.as_bytes().first() {
                    Some(b'+') => (1, value.get(1..).unwrap()),
                    Some(b'-') => (-1, value.get(1..).unwrap()),
                    _ => return Err("Offset needs an explicit + or - sign".into()),
                };
                Self::Offset(
                    i64::try_from(parse_time(rest)?)?
                        .checked_mul(sign)
                        .ok_or("Offset overflow")?,
                )
            }
            _ => {
                return Err(
                    "Use time MM:SS.mmm, lap N, offset +/-MM:SS.mmm, or recording MM:SS.mmm".into(),
                );
            }
        })
    }
}
fn parse_time(text: &str) -> Result<u64> {
    let (minutes, seconds) = text.split_once(':').ok_or("Use MM:SS or MM:SS.mmm")?;
    if minutes.is_empty() || !minutes.bytes().all(|c| c.is_ascii_digit()) {
        return Err("Invalid minutes".into());
    }
    let (seconds, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    if seconds.len() != 2
        || !seconds.bytes().all(|c| c.is_ascii_digit())
        || fraction.len() > 3
        || !fraction.bytes().all(|c| c.is_ascii_digit())
        || text.ends_with('.')
    {
        return Err("Use MM:SS or up to three millisecond digits".into());
    }
    let seconds = seconds.parse::<u64>()?;
    if seconds >= 60 {
        return Err("Seconds must be below 60".into());
    }
    let millis = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>()? * 10_u64.pow(3 - fraction.len() as u32)
    };
    minutes
        .parse::<u64>()?
        .checked_mul(60_000)
        .and_then(|v| v.checked_add(seconds * 1000 + millis))
        .ok_or_else(|| "Time overflow".into())
}
