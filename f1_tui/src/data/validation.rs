//! Validate identities/schema before publication; quarantine numeric anomalies by field.
use super::openf1::{AcquisitionRequest, Channel, FieldIssue, Result, Window};
use crate::domain::{BrakeState, Gear, Percent, Seconds, SpeedKmh, parse_legacy_gap};
use chrono::DateTime;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub(super) fn validate(
    channel: Channel,
    rows: &mut [Value],
    request: &AcquisitionRequest,
    window: Option<Window>,
    drivers: &BTreeSet<u32>,
) -> Result<Vec<FieldIssue>> {
    let mut issues = Vec::new();
    for (index, value) in rows.iter_mut().enumerate() {
        let row = value
            .as_object_mut()
            .ok_or("Provider row must be an object")?;
        if id(row, "session_key")? != request.session_key()
            || id(row, "meeting_key")? != request.meeting_key()
        {
            return Err("Provider session/meeting identity mismatch".into());
        }
        if !matches!(channel, Channel::Weather | Channel::RaceControl) {
            let number = id(row, "driver_number")?;
            if channel != Channel::Drivers && !drivers.contains(&number) {
                return Err(
                    "Provider record references a driver outside the session roster".into(),
                );
            }
        } else if channel == Channel::RaceControl && !required(row, "driver_number")?.is_null() {
            let number = id(row, "driver_number")?;
            if !drivers.contains(&number) {
                return Err("Race control references a driver outside the session roster".into());
            }
        }
        if !matches!(channel, Channel::Drivers | Channel::Stints) {
            let field = if channel == Channel::Laps {
                "date_start"
            } else {
                "date"
            };
            let value = required(row, field)?;
            if !(channel == Channel::Laps && value.is_null()) {
                let time = DateTime::parse_from_rfc3339(
                    value
                        .as_str()
                        .ok_or("Observation timestamp must be RFC3339 text")?,
                )?
                .timestamp_millis();
                if let Some(w) = window
                    && (time < w.start().0 || time >= w.end().0)
                {
                    return Err("Provider ignored requested time window".into());
                }
            }
        }
        let mut fields = Fields {
            row,
            index,
            issues: &mut issues,
        };
        match channel {
            Channel::Drivers => {
                for name in ["full_name", "name_acronym", "team_name"] {
                    fields.text(name)?;
                }
            }
            Channel::Position => fields.integer("position", 1, u32::MAX)?,
            Channel::CarData => {
                fields.numeric("speed", |v| SpeedKmh::new(v).is_ok())?;
                fields.integer("rpm", 0, u32::MAX)?;
                fields.numeric("n_gear", |v| {
                    v.fract() == 0.0
                        && v >= 0.0
                        && v <= u8::MAX as f64
                        && Gear::new(v as u8).is_ok()
                })?;
                fields.numeric("throttle", |v| Percent::new(v).is_ok())?;
                fields.numeric("brake", |v| {
                    v.fract() == 0.0
                        && v >= 0.0
                        && v <= u32::MAX as f64
                        && BrakeState::from_raw(v as u32).is_ok()
                })?;
                fields.integer("drs", 0, u32::MAX)?; // Unknown unsigned codes remain intact.
            }
            Channel::Location => {
                for name in ["x", "y", "z"] {
                    fields.numeric(name, f64::is_finite)?;
                }
            }
            Channel::Intervals => {
                for name in ["gap_to_leader", "interval"] {
                    fields.gap(name)?;
                }
            }
            Channel::Laps => {
                fields.integer("lap_number", 1, u32::MAX)?;
                for name in [
                    "lap_duration",
                    "duration_sector_1",
                    "duration_sector_2",
                    "duration_sector_3",
                ] {
                    fields.numeric(name, |v| Seconds::new(v).is_ok())?;
                }
                for name in ["i1_speed", "i2_speed", "st_speed"] {
                    if fields.row.contains_key(name) {
                        fields.numeric(name, |v| SpeedKmh::new(v).is_ok())?;
                    }
                }
                let flag = required(fields.row, "is_pit_out_lap")?;
                if !flag.is_null() && !flag.is_boolean() {
                    return Err("Lap pit-out field must be boolean or null".into());
                }
                // Mini-sector/status arrays are retained raw; not decoded as measured timing.
            }
            Channel::Stints => {
                fields.integer("stint_number", 1, u32::MAX)?;
                fields.integer("lap_start", 1, u32::MAX)?;
                fields.integer("lap_end", 1, u32::MAX)?;
                fields.integer("tyre_age_at_start", 0, u32::MAX)?;
                fields.text("compound")?;
                if let (Some(start), Some(end)) = (
                    fields.row["lap_start"].as_u64(),
                    fields.row["lap_end"].as_u64(),
                ) && end < start
                {
                    fields.reject("lap_end", "Stint end precedes its start");
                }
            }
            Channel::Pit => {
                fields.integer("lap_number", 1, u32::MAX)?;
                for name in ["pit_duration", "lane_duration", "stop_duration"] {
                    if fields.row.contains_key(name) {
                        fields.numeric(name, |v| Seconds::new(v).is_ok())?;
                    }
                }
            }
            Channel::RaceControl => {
                fields.text("category")?;
                fields.text("message")?;
                for name in ["flag", "scope"] {
                    if fields.row.contains_key(name) {
                        fields.text(name)?;
                    }
                }
                for name in ["lap_number", "sector", "qualifying_phase"] {
                    if fields.row.contains_key(name) {
                        fields.integer(name, 1, u32::MAX)?;
                    }
                }
            }
            Channel::Weather => {
                for name in ["air_temperature", "track_temperature"] {
                    fields.numeric(name, f64::is_finite)?;
                }
                fields.numeric("humidity", |v| Percent::new(v).is_ok())?;
                for name in ["pressure", "wind_speed"] {
                    fields.numeric(name, |v| v.is_finite() && v >= 0.0)?;
                }
                fields.integer("rainfall", 0, 1)?;
                fields.numeric("wind_direction", |v| (0.0..=360.0).contains(&v))?;
            }
        }
    }
    Ok(issues)
}

fn required<'a>(row: &'a Map<String, Value>, field: &str) -> Result<&'a Value> {
    row.get(field)
        .ok_or_else(|| format!("Missing provider field: {field}").into())
}
fn id(row: &Map<String, Value>, field: &str) -> Result<u32> {
    let value = required(row, field)?
        .as_u64()
        .filter(|v| *v > 0 && *v <= u32::MAX as u64)
        .ok_or_else(|| format!("Invalid identity field: {field}"))?;
    Ok(value as u32)
}
struct Fields<'a> {
    row: &'a mut Map<String, Value>,
    index: usize,
    issues: &'a mut Vec<FieldIssue>,
}
impl Fields<'_> {
    fn reject(&mut self, field: &str, reason: &str) {
        self.row.insert(field.into(), Value::Null);
        self.issues.push(FieldIssue {
            row: self.index,
            field: field.into(),
            reason: reason.into(),
        });
    }
    fn numeric(&mut self, field: &str, valid: impl FnOnce(f64) -> bool) -> Result<()> {
        let value = required(self.row, field)?;
        if value.is_null() {
            return Ok(());
        }
        let number = value
            .as_f64()
            .ok_or_else(|| format!("Numeric provider field has wrong type: {field}"))?;
        if !valid(number) {
            self.reject(
                field,
                "Outside the supported numeric domain; raw value retained",
            );
        }
        Ok(())
    }
    fn integer(&mut self, field: &str, min: u32, max: u32) -> Result<()> {
        self.numeric(field, |v| {
            v.fract() == 0.0 && v >= min as f64 && v <= max as f64
        })
    }
    fn text(&mut self, field: &str) -> Result<()> {
        let value = required(self.row, field)?;
        if value.is_null() {
            return Ok(());
        }
        let text = value
            .as_str()
            .ok_or_else(|| format!("Provider field must be text: {field}"))?;
        if text.trim().is_empty() || text.chars().any(char::is_control) {
            self.reject(field, "Empty or control-bearing text; raw value retained");
        }
        Ok(())
    }
    fn gap(&mut self, field: &str) -> Result<()> {
        let value = required(self.row, field)?;
        let valid = match value {
            Value::Null => return Ok(()),
            Value::Number(_) => value.as_f64().is_some_and(|v| Seconds::new(v).is_ok()),
            Value::String(s) => parse_legacy_gap(s).is_ok(),
            _ => return Err("Interval must be numeric seconds, lapped text or null".into()),
        };
        if !valid {
            self.reject(field, "Invalid interval; raw value retained");
        }
        Ok(())
    }
}
