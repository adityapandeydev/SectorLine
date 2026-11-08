//! Normalize small cached channels. Source summaries never establish future state.
use super::*;
use crate::{
    data::{cache::SessionCache, limits::Cancellation, openf1::Channel},
    domain::normalize::utc_millis_ceiling,
};
use serde_json::Value;
fn u32_value(row: &Value, key: &str) -> Option<u32> {
    row[key].as_u64().and_then(|v| u32::try_from(v).ok())
}
fn seconds(row: &Value, key: &str) -> Option<f64> {
    row[key].as_f64().filter(|v| v.is_finite() && *v >= 0.0)
}
fn number(row: &Value, key: &str) -> Option<f64> {
    row[key].as_f64().filter(|v| v.is_finite())
}
fn time(row: &Value, key: &str) -> Option<UtcMillis> {
    row[key].as_str().and_then(|v| utc_millis_ceiling(v).ok())
}
impl ConditionsArchive {
    pub fn from_cache(cache: &SessionCache, cancel: &Cancellation) -> Result<Self> {
        let mut archive = Self::default();
        let qualifying = capabilities::SessionContext::from_names(
            SessionId::new(
                Provider::OpenF1,
                cache.spec().request()?.session_key().to_string(),
            )?,
            cache.spec().session_name(),
            cache.spec().session_type(),
        )
        .qualifying();
        let mut starts = BTreeMap::<(u32, u32), UtcMillis>::new();
        let mut laps = Vec::new();
        let mut stints = Vec::new();
        for index in 0..cache.chunk_count() {
            cancel.check()?;
            let meta = cache.chunk_metadata(index)?;
            if meta.status != 200
                || !matches!(
                    meta.channel,
                    Channel::Laps
                        | Channel::Stints
                        | Channel::Pit
                        | Channel::Weather
                        | Channel::RaceControl
                )
            {
                continue;
            }
            let chunk = cache.read_chunk(index)?;
            for row in chunk.records() {
                cancel.check()?;
                let retrieved = meta.retrieved_at;
                let driver = u32_value(row, "driver_number");
                match meta.channel {
                    Channel::Laps => {
                        if let (Some(driver), Some(lap)) = (driver, u32_value(row, "lap_number")) {
                            if let Some(start) = time(row, "date_start") {
                                starts
                                    .entry((driver, lap))
                                    .and_modify(|old| *old = (*old).max(start))
                                    .or_insert(start);
                            }
                            laps.push((driver, lap, row.clone()));
                        }
                    }
                    Channel::Stints => {
                        if let Some(driver) = driver {
                            stints.push((driver, row.clone(), retrieved));
                        }
                    }
                    Channel::Pit => {
                        if let (Some(driver), Some(at)) = (driver, time(row, "date")) {
                            let lane = seconds(row, "lane_duration")
                                .or_else(|| seconds(row, "pit_duration"));
                            // date has no documented entry/exit meaning. Delay archive durations
                            // by the lane duration; do not backfill an in-pit interval.
                            let delay = lane
                                .or_else(|| seconds(row, "stop_duration"))
                                .unwrap_or(0.0)
                                * 1000.0;
                            if delay >= i64::MAX as f64 {
                                continue;
                            }
                            let Some(boundary) =
                                at.0.checked_add(delay.ceil() as i64).map(UtcMillis)
                            else {
                                continue;
                            };
                            let mut reading = Reading::new(
                                at,
                                Availability::Estimated(boundary),
                                retrieved,
                                Some(PitRecord {
                                    lap: u32_value(row, "lap_number"),
                                    lane_seconds: lane,
                                    stop_seconds: seconds(row, "stop_duration")
                                        .filter(|stop| lane.is_none_or(|lane| *stop <= lane)),
                                }),
                            )?;
                            if row.get("lane_duration").is_some_and(|v| !v.is_null())
                                && row.get("pit_duration").is_some_and(|v| !v.is_null())
                                && seconds(row, "lane_duration") != seconds(row, "pit_duration")
                            {
                                reading.value = None;
                            }
                            archive.pits.entry(driver).or_default().push(reading);
                        }
                    }
                    Channel::Weather => {
                        if let Some(at) = time(row, "date") {
                            let weather = Weather {
                                air_c: number(row, "air_temperature"),
                                track_c: number(row, "track_temperature"),
                                humidity: seconds(row, "humidity").filter(|v| *v <= 100.0),
                                pressure_mbar: seconds(row, "pressure"),
                                wind_mps: seconds(row, "wind_speed"),
                                wind_degrees: seconds(row, "wind_direction").filter(|v| *v < 360.0),
                                rain: match row["rainfall"].as_u64() {
                                    Some(0) => Some(false),
                                    Some(1) => Some(true),
                                    _ => None,
                                },
                            };
                            archive.weather.push(Reading::new(
                                at,
                                Availability::Estimated(at),
                                retrieved,
                                Some(weather),
                            )?);
                        }
                    }
                    Channel::RaceControl => {
                        if qualifying
                            && let Some(at) = time(row, "date")
                            && let Some(value) = capabilities::PhaseUpdate::from_record(row)
                        {
                            archive.phases.push(Reading::new(
                                at,
                                Availability::Estimated(at),
                                retrieved,
                                Some(value),
                            )?);
                        }
                        if let (Some(at), Some(control)) = (time(row, "date"), parse_control(row)) {
                            archive.control.push(Reading::new(
                                at,
                                Availability::Estimated(at),
                                retrieved,
                                Some(control),
                            )?);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut completions = BTreeMap::<(u32, u32), Option<UtcMillis>>::new();
        for (driver, lap, row) in laps {
            let duration = seconds(&row, "lap_duration")
                .filter(|v| *v > 0.0)
                .and_then(|v| Seconds::new(v).ok());
            let sectors = [
                "duration_sector_1",
                "duration_sector_2",
                "duration_sector_3",
            ]
            .map(|key| {
                seconds(&row, key)
                    .filter(|v| *v > 0.0)
                    .and_then(|v| Seconds::new(v).ok())
            });
            let end = timing::completion(
                time(&row, "date_start"),
                duration,
                sectors,
                lap.checked_add(1)
                    .and_then(|lap| starts.get(&(driver, lap)).copied()),
            );
            completions
                .entry((driver, lap))
                .and_modify(|old| {
                    if *old != end {
                        *old = None;
                    }
                })
                .or_insert(end);
        }
        for ((driver, lap), end) in completions {
            if let Some(end) = end {
                archive
                    .completed_laps
                    .entry(driver)
                    .or_default()
                    .push((lap, end));
            }
        }
        for (driver, row, retrieved) in stints {
            let (Some(number), Some(lap_start)) = (
                u32_value(&row, "stint_number"),
                u32_value(&row, "lap_start"),
            ) else {
                continue;
            };
            let Some(at) = starts.get(&(driver, lap_start)).copied() else {
                continue;
            };
            let value = Stint {
                number,
                lap_start,
                compound: row["compound"]
                    .as_str()
                    .filter(|v| !v.is_empty() && !v.chars().any(char::is_control))
                    .map(str::to_owned),
                age_at_start: u32_value(&row, "tyre_age_at_start"),
            };
            archive.stints.entry(driver).or_default().push(Reading::new(
                at,
                Availability::Estimated(at),
                retrieved,
                Some(value),
            )?);
        }
        cancel.check()?;
        Ok(archive)
    }
}
/// Categories and scope are independent. Unknown flag strings remain explicit.
pub fn parse_control(row: &Value) -> Option<Control> {
    let category = row["category"].as_str()?;
    let message = row["message"].as_str().unwrap_or("");
    let scope = match row["scope"].as_str() {
        Some("Track") => Scope::Track,
        Some("Sector") => Scope::Sector(u32_value(row, "sector").filter(|v| *v > 0)?),
        Some("Driver") => Scope::Driver(u32_value(row, "driver_number").filter(|v| *v > 0)?),
        None if matches!(category, "SafetyCar" | "SessionStatus") => Scope::Track,
        _ => return None,
    };
    let kind = match category {
        "Flag" => ControlKind::Flag(match row["flag"].as_str().unwrap_or("") {
            "" => Flag::Unavailable,
            "GREEN" => Flag::Green,
            "YELLOW" => Flag::Yellow,
            "DOUBLE YELLOW" => Flag::DoubleYellow,
            "RED" => Flag::Red,
            "CHEQUERED" => Flag::Chequered,
            "BLUE" => Flag::Blue,
            "CLEAR" => Flag::Clear,
            other => Flag::Unknown(other.into()),
        }),
        "SafetyCar" => ControlKind::SafetyCar(match message {
            "SAFETY CAR DEPLOYED" => SafetyCar::Deployed,
            "SAFETY CAR IN THIS LAP" => SafetyCar::InThisLap,
            "SAFETY CAR RETURNED TO PITS" => SafetyCar::Returned,
            "VIRTUAL SAFETY CAR DEPLOYED" => SafetyCar::VirtualDeployed,
            "VIRTUAL SAFETY CAR ENDING" => SafetyCar::VirtualEnding,
            "SAFETY CAR ENDING" => SafetyCar::Ending,
            _ => return None,
        }),
        "SessionStatus" => ControlKind::Session(match message {
            "SESSION STARTED" => SessionStatus::Started,
            "SESSION RESUMED" => SessionStatus::Resumed,
            "SESSION SUSPENDED" | "SESSION STOPPED" | "SESSION ABORTED" => SessionStatus::Suspended,
            "SESSION FINISHED" => SessionStatus::Finished,
            _ => return None,
        }),
        _ => return None,
    };
    Some(Control { scope, kind })
}
