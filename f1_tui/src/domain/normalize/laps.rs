//! Archived lap completion and explicit driver decisions.
use super::*;

pub(super) fn append_laps(
    chunk: &ValidatedChunk,
    source: u32,
    events: &mut Vec<Event>,
) -> Result<()> {
    let mut starts = std::collections::BTreeMap::<(u64, u64), UtcMillis>::new();
    let mut summaries = std::collections::BTreeMap::new();
    let mut conflicting = std::collections::BTreeSet::new();
    for row in chunk.records() {
        let key = (
            row["driver_number"].as_u64().unwrap_or(0),
            row["lap_number"].as_u64().unwrap_or(0),
        );
        if let Some(start) = row["date_start"]
            .as_str()
            .and_then(|v| utc_millis_ceiling(v).ok())
        {
            starts
                .entry(key)
                .and_modify(|old| *old = (*old).max(start))
                .or_insert(start);
        }
        if let Some(old) = summaries.insert(key, row)
            && [
                "date_start",
                "lap_duration",
                "duration_sector_1",
                "duration_sector_2",
                "duration_sector_3",
                "is_pit_out_lap",
            ]
            .iter()
            .any(|field| old[*field] != row[*field])
        {
            conflicting.insert(key);
        }
    }
    for row in chunk.records() {
        let driver = number(row, "driver_number")?;
        let Some(number) = row["lap_number"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .and_then(NonZeroU32::new)
        else {
            continue;
        };
        let seconds = |key: &str| {
            row[key]
                .as_f64()
                .filter(|v| v.is_finite() && *v > 0.0)
                .and_then(|v| Seconds::new(v).ok())
        };
        let mut duration = seconds("lap_duration");
        let sectors = [
            seconds("duration_sector_1"),
            seconds("duration_sector_2"),
            seconds("duration_sector_3"),
        ];
        let start = row["date_start"]
            .as_str()
            .and_then(|s| utc_millis_ceiling(s).ok());
        let next = starts
            .get(&(u64::from(driver), u64::from(number.get()) + 1))
            .copied();
        let Some(end) = timing::completion(start, duration, sectors, next) else {
            continue;
        };
        if start.is_some_and(|s| end <= s) {
            continue;
        }
        let inconsistent = duration
            .zip(sectors.into_iter().collect::<Option<Vec<_>>>())
            .is_some_and(|(d, s)| (d.get() - s.iter().map(|s| s.get()).sum::<f64>()).abs() > 0.01);
        if inconsistent {
            duration = None;
        }
        let mut event = Event::new(
            driver,
            end,
            Availability::Estimated(end),
            source,
            Payload::Lap(Box::new(timing::LapTiming {
                number,
                duration,
                sectors,
                pit_out: row["is_pit_out_lap"].as_bool(),
                eligibility: if inconsistent {
                    timing::Eligibility::Ineligible
                } else {
                    timing::Eligibility::Unknown
                },
            })),
        )?;
        event.ambiguous = conflicting.contains(&(u64::from(driver), u64::from(number.get())));
        events.push(event);
    }
    Ok(())
}

/// Only explicit, driver-scoped decisions. Never use race_control.lap_number:
/// it is the lap at message time, not necessarily the deleted lap.
pub fn control_decision(message: &str) -> Option<(u32, Payload)> {
    let rest = message.strip_prefix("CAR ")?;
    let (number, tail) = rest.split_once(" (")?;
    let driver = number.parse::<u32>().ok()?;
    let (code, action) = tail.split_once(") ")?;
    if driver == 0 || code.is_empty() || !code.bytes().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let status = match action {
        "- DISQUALIFIED" => Some(classification::DriverStatus::Dsq),
        "- DID NOT START" => Some(classification::DriverStatus::Dns),
        "- RETIRED" => Some(classification::DriverStatus::Retired),
        _ => None,
    };
    if let Some(status) = status {
        return Some((driver, Payload::Status(status)));
    }
    if !(action.starts_with("TIME ")
        || action.starts_with("LAP TIME ")
        || action.starts_with("LAP DELETED - "))
    {
        return None;
    }
    let deleted = message.contains(" DELETED - ");
    let reinstated = message.contains(" REINSTATED");
    if deleted == reinstated
        || message.contains(" NOT DELETED")
        || !(message.contains(" TIME ") || message.contains(" LAP DELETED"))
    {
        return None;
    }
    let lap = message
        .rsplit_once(" LAP ")?
        .1
        .split_whitespace()
        .next()?
        .parse::<u32>()
        .ok()
        .and_then(NonZeroU32::new)?;
    Some((
        driver,
        Payload::LapDecision {
            lap,
            eligibility: if deleted {
                timing::Eligibility::Ineligible
            } else {
                timing::Eligibility::Unknown
            },
        },
    ))
}
pub(crate) fn append_control(
    chunk: &ValidatedChunk,
    source: u32,
    drivers: &[RosterDriver],
    events: &mut Vec<Event>,
) -> Result<()> {
    for row in chunk.records() {
        let Some((driver, payload)) = row["message"].as_str().and_then(control_decision) else {
            continue;
        };
        if !drivers.iter().any(|d| d.number.get() == driver)
            || row["driver_number"]
                .as_u64()
                .is_some_and(|v| v != u64::from(driver))
        {
            continue;
        }
        let observed = utc_millis_ceiling(row["date"].as_str().ok_or("Missing control date")?)?;
        events.push(Event::new(
            driver,
            observed,
            Availability::Estimated(observed),
            source,
            payload,
        )?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflicting_lap_start_summaries_never_choose_an_arbitrary_best() {
        use crate::data::openf1::{AcquisitionRequest, ChunkMetadata, Window};
        let row = serde_json::json!({"driver_number":4,"meeting_key":1244,"session_key":9590,"lap_number":1,"date_start":"1970-01-01T00:00:00Z","lap_duration":60,"duration_sector_1":20,"duration_sector_2":20,"duration_sector_3":20,"is_pit_out_lap":false});
        let mut changed = row.clone();
        changed["date_start"] = serde_json::json!("1970-01-01T00:00:01Z");
        let request = AcquisitionRequest::new(
            9590,
            1244,
            Window::new(UtcMillis(0), UtcMillis(300_000)).unwrap(),
        )
        .unwrap();
        let chunk = ValidatedChunk::from_archive(
            ChunkMetadata {
                channel: Channel::Laps,
                session_key: 9590,
                meeting_key: 1244,
                window: None,
                source_url: "https://api.openf1.org/v1/laps?session_key=9590".into(),
                retrieved_at: UtcMillis(900_000),
                status: 200,
                attempts: 1,
            },
            serde_json::to_vec(&vec![row, changed]).unwrap(),
            &request,
            &std::collections::BTreeSet::from([4]),
        )
        .unwrap();
        let mut events = Vec::new();
        append_laps(&chunk, 0, &mut events).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.ambiguous));
    }
}
