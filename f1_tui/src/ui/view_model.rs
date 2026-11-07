//! Display formatting does not change classification, units or source knowledge.
use crate::{
    domain::{normalize::RosterDriver, *},
    replay::RecordedReplay,
    stream::TelemetryStream,
};
use ratatui::style::Color;
use std::{collections::BTreeMap, ops::Range};
#[derive(Debug, Clone)]
pub struct TimingRow {
    pub id: u32,
    pub position: String,
    pub driver: String,
    pub team: String,
    pub team_name: Option<String>,
    pub team_color: Option<Color>,
    pub gap: String,
    pub interval: String,
    pub tyre: String,
    pub tyre_age: String,
    pub lap: String,
    pub last: String,
    pub speed: String,
    pub sample_age: String,
    pub status: String,
}
pub fn viewport(total: usize, selected: usize, visible: usize, offset: usize) -> Range<usize> {
    if visible == 0 || total == 0 {
        return 0..0;
    }
    let selected = selected.min(total - 1);
    let mut start = offset.min(total.saturating_sub(visible));
    if selected < start {
        start = selected;
    } else if selected >= start.saturating_add(visible) {
        start = selected + 1 - visible;
    }
    start..start.saturating_add(visible).min(total)
}
pub fn format_gap(value: Option<&Gap>) -> String {
    match value {
        Some(Gap::Leader) => "leader".into(),
        Some(Gap::Seconds(v)) => format!("{:.2}", v.get()),
        Some(Gap::LapsBehind(v)) => format!("+{v}L"),
        None => "--".into(),
    }
}
pub fn team_code(name: &str) -> String {
    let words = name.split_whitespace().collect::<Vec<_>>();
    let code = if words.len() > 1 {
        words
            .iter()
            .filter_map(|w| w.chars().find(|c| c.is_ascii_alphanumeric()))
            .take(3)
            .collect::<String>()
    } else {
        words
            .first()
            .copied()
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(3)
            .collect()
    };
    if code.is_empty() {
        "--".into()
    } else {
        code.to_ascii_uppercase()
    }
}
fn team_codes(roster: &[RosterDriver]) -> BTreeMap<String, String> {
    let names = roster
        .iter()
        .filter_map(|d| d.team.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for name in names {
        groups.entry(team_code(&name)).or_default().push(name);
    }
    groups
        .into_iter()
        .flat_map(|(code, names)| {
            let duplicate = names.len() > 1;
            names.into_iter().enumerate().map(move |(i, name)| {
                let code = if duplicate {
                    format!("{}{:02}", code.chars().next().unwrap_or('T'), i + 1)
                } else {
                    code.clone()
                };
                (name, code)
            })
        })
        .collect()
}
/// Quantize source colour to the basic ANSI palette. Text identity is always present.
fn ansi_color(rgb: Option<[u8; 3]>) -> Option<Color> {
    let rgb = rgb?;
    let palette = [
        ([205, 49, 49], Color::Red),
        ([13, 188, 121], Color::Green),
        ([229, 229, 16], Color::Yellow),
        ([36, 114, 200], Color::Blue),
        ([188, 63, 188], Color::Magenta),
        ([17, 168, 205], Color::Cyan),
        ([229, 229, 229], Color::Gray),
    ];
    palette
        .into_iter()
        .min_by_key(|(color, _)| {
            rgb.iter()
                .zip(color)
                .map(|(a, b)| (i32::from(*a) - b).pow(2))
                .sum::<i32>()
        })
        .map(|(_, color)| color)
}
fn value<T>(observation: &Observation<T>, format: impl FnOnce(&T) -> String) -> String {
    observation
        .value()
        .map(|v| {
            let formatted = format(v);
            if formatted == "--" {
                return formatted;
            }
            format!(
                "{}{}",
                if observation.quality() == Quality::Stale {
                    "~"
                } else {
                    ""
                },
                formatted
            )
        })
        .unwrap_or_else(|| "--".into())
}
fn compound(value: Option<&str>) -> String {
    match value {
        Some("SOFT") => "S".into(),
        Some("MEDIUM") => "M".into(),
        Some("HARD") => "H".into(),
        Some("INTERMEDIATE") => "I".into(),
        Some("WET") => "W".into(),
        Some(_) => "?".into(),
        None => "--".into(),
    }
}
pub fn recorded(replay: &RecordedReplay) -> Vec<TimingRow> {
    let codes = team_codes(replay.timeline().drivers());
    replay
        .frame()
        .samples()
        .iter()
        .map(|car| {
            let driver = replay
                .timeline()
                .drivers()
                .iter()
                .find(|d| d.number == car.driver.number);
            let condition = replay.conditions().drivers.get(&car.driver.number.get());
            let tyre = condition
                .map(|d| value(&d.tyre, |t| compound(t.compound.as_deref())))
                .unwrap_or_else(|| "--".into());
            let tyre_age = condition
                .map(|d| {
                    value(&d.tyre, |t| {
                        t.completed_age
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "--".into())
                    })
                })
                .unwrap_or_else(|| "--".into());
            let sample_age = car
                .speed_kmh
                .timing()
                .observed_at
                .map(|t| {
                    format!(
                        "{:.1}",
                        replay
                            .timeline()
                            .origin()
                            .0
                            .saturating_add(replay.frame().elapsed().0 as i64)
                            .saturating_sub(t.0) as f64
                            / 1000.0
                    )
                })
                .unwrap_or_else(|| "--".into());
            TimingRow {
                id: car.driver.number.get(),
                position: value(&car.position, |v| v.to_string()),
                driver: format!(
                    "{} #{}",
                    driver.map(|d| d.code.as_str()).unwrap_or("--"),
                    car.driver.number
                ),
                team: driver
                    .and_then(|d| d.team.as_ref())
                    .and_then(|t| codes.get(t))
                    .cloned()
                    .unwrap_or_else(|| "--".into()),
                team_name: driver.and_then(|d| d.team.clone()),
                team_color: ansi_color(driver.and_then(|d| d.team_color)),
                gap: value(&car.gap_to_leader, |g| format_gap(Some(g))),
                interval: value(&car.gap_to_ahead, |g| format_gap(Some(g))),
                tyre,
                tyre_age,
                lap: value(&car.lap_number, |v| v.to_string()),
                last: value(&car.lap_time, |v| format!("{:.3}", v.get())),
                speed: value(&car.speed_kmh, |v| format!("{:.0}", v.get())),
                sample_age,
                status: value(&car.status, |v| {
                    match v {
                        classification::DriverStatus::Retired => "RET",
                        classification::DriverStatus::Dnf => "DNF",
                        classification::DriverStatus::Dns => "DNS",
                        classification::DriverStatus::Dsq => "DSQ",
                    }
                    .into()
                }),
            }
        })
        .collect()
}
pub fn demo(stream: &TelemetryStream) -> Vec<TimingRow> {
    let roster = stream
        .drivers
        .iter()
        .filter_map(|d| {
            RosterDriver::new(d.number, &d.code, &d.name)
                .ok()
                .and_then(|r| r.with_team(Some(&d.team), Some(&d.color)).ok())
        })
        .collect::<Vec<_>>();
    let codes = team_codes(&roster);
    stream
        .current_frame()
        .cars
        .iter()
        .map(|car| {
            let driver = roster.iter().find(|d| d.number.get() == car.number);
            let gap = |s: &str| {
                crate::domain::parse_legacy_gap(s)
                    .map(|g| format_gap(Some(&g)))
                    .unwrap_or_else(|_| "--".into())
            };
            TimingRow {
                id: car.number,
                position: car.position.to_string(),
                driver: format!("{} #{}", car.driver, car.number),
                team: driver
                    .and_then(|d| d.team.as_ref())
                    .and_then(|t| codes.get(t))
                    .cloned()
                    .unwrap_or_else(|| "--".into()),
                team_name: driver.and_then(|d| d.team.clone()),
                team_color: ansi_color(driver.and_then(|d| d.team_color)),
                gap: gap(&car.gap_to_leader),
                interval: gap(&car.gap_to_ahead),
                tyre: compound(Some(&car.compound)),
                tyre_age: car.tyre_age.to_string(),
                lap: "--".into(),
                last: "--".into(),
                speed: format!("{:.0}", car.speed),
                sample_age: "--".into(),
                status: if car.pit_status.is_empty() {
                    "--".into()
                } else {
                    car.pit_status.chars().take(5).collect()
                },
            }
        })
        .collect()
}
