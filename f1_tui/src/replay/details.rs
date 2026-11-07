//! Read-only evidence/history index. Visibility and navigation use availability, never retrieval.
use super::timeline::Timeline;
use crate::{
    data::{limits::Cancellation, openf1::Result},
    domain::{
        conditions::{ControlKind, Flag, SafetyCar, Scope, SessionStatus},
        normalize::Payload,
        *,
    },
};
use std::collections::BTreeMap;
#[derive(Debug, Clone)]
pub struct ReplayEvent {
    session: SessionId,
    pub observed: UtcMillis,
    pub available: Availability,
    pub retrieved: UtcMillis,
    pub scope: Scope,
    pub channel: &'static str,
    pub summary: String,
    pub evidence: String,
}
impl ReplayEvent {
    pub fn available_ms(&self) -> i64 {
        available_ms(self.available)
    }
    pub fn seek_target(&self, timeline: &Timeline) -> Option<ElapsedMillis> {
        if &self.session != timeline.session() {
            return None;
        }
        let offset = self.available_ms().checked_sub(timeline.origin().0)?;
        let offset = u64::try_from(offset).ok()?;
        (offset <= timeline.end().0).then_some(ElapsedMillis(offset))
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryPoint {
    pub at: UtcMillis,
    pub speed_kmh: Option<f64>,
    pub throttle_percent: Option<f64>,
}
#[derive(Default)]
pub(super) struct DetailsIndex {
    events: Vec<ReplayEvent>,
    telemetry: BTreeMap<u32, Vec<usize>>,
}
fn available_ms(value: Availability) -> i64 {
    match value {
        Availability::Known(t) | Availability::Estimated(t) => t.0,
        Availability::Unknown => i64::MAX,
    }
}
fn admitted(value: Availability, now: i64) -> bool {
    matches!(value,Availability::Known(t)|Availability::Estimated(t) if t.0<=now)
}
type EventKey = (i64, i64, bool, Scope, String);
fn insert(rows: &mut BTreeMap<EventKey, ReplayEvent>, row: ReplayEvent, slot: String) {
    if row.available == Availability::Unknown {
        return;
    }
    let key = (
        row.observed.0,
        row.available_ms(),
        matches!(row.available, Availability::Estimated(_)),
        row.scope.clone(),
        slot,
    );
    match rows.entry(key) {
        std::collections::btree_map::Entry::Vacant(e) => {
            e.insert(row);
        }
        std::collections::btree_map::Entry::Occupied(mut e) => {
            if row.retrieved > e.get().retrieved {
                e.insert(row);
            } else if row.retrieved == e.get().retrieved
                && (row.summary != e.get().summary || row.evidence != e.get().evidence)
            {
                e.get_mut().summary = "Conflicting provider reports".into();
                e.get_mut().evidence =
                    "Same-time, equal-retrieval evidence disagrees; no resolved report".into();
            }
        }
    }
}
impl DetailsIndex {
    pub(super) fn build(timeline: &Timeline, cancel: &Cancellation) -> Result<Self> {
        cancel.check()?;
        let mut index = Self::default();
        let mut rows = BTreeMap::new();
        for (i, event) in timeline.events().iter().enumerate() {
            if i % 4096 == 0 {
                cancel.check()?;
            }
            if matches!(event.payload, Payload::Telemetry(_)) {
                index
                    .telemetry
                    .entry(event.driver.get())
                    .or_default()
                    .push(i);
                continue;
            }
            let (summary, slot) = match &event.payload {
                Payload::Lap(lap) => (
                    format!(
                        "Completed L{}: {}s ({:?})",
                        lap.number,
                        lap.duration
                            .map(|v| format!("{:.3}", v.get()))
                            .unwrap_or_else(|| "--".into()),
                        lap.eligibility
                    ),
                    format!("lap:{}", lap.number),
                ),
                Payload::LapDecision { lap, eligibility } => (
                    format!("L{lap} eligibility {eligibility:?}"),
                    format!("decision:{lap}"),
                ),
                Payload::Status(status) => (format!("Status {}", status.label()), "status".into()),
                _ => continue,
            };
            let source = &timeline.sources()[event.source as usize];
            insert(
                &mut rows,
                ReplayEvent {
                    session: timeline.session().clone(),
                    observed: event.observed,
                    available: event.available,
                    retrieved: source.retrieved_at,
                    scope: Scope::Driver(event.driver.get()),
                    channel: source.channel.endpoint(),
                    summary: if event.ambiguous {
                        "Conflicting provider reports".into()
                    } else {
                        summary
                    },
                    evidence: format!(
                        "Parsed {} | {}{}",
                        source.channel.endpoint(),
                        source.source_url,
                        if event.ambiguous {
                            " | unresolved same-time conflict"
                        } else {
                            ""
                        }
                    ),
                },
                slot,
            );
        }
        let conditions = timeline.conditions_archive();
        for r in &conditions.control {
            cancel.check()?;
            let Some(value) = &r.value else {
                continue;
            };
            let slot = match value.kind {
                ControlKind::Flag(_) => "flag",
                ControlKind::SafetyCar(_) => "safety-car",
                ControlKind::Session(_) => "session",
            };
            insert(
                &mut rows,
                ReplayEvent {
                    session: timeline.session().clone(),
                    observed: r.at,
                    available: r.available,
                    retrieved: r.retrieved,
                    scope: value.scope.clone(),
                    channel: "race_control",
                    summary: control_summary(&value.kind),
                    evidence: "Parsed provider race_control kind and scope; not a verbatim message"
                        .into(),
                },
                slot.into(),
            );
        }
        for (driver, visits) in &conditions.pits {
            for r in visits {
                cancel.check()?;
                let Some(value) = &r.value else {
                    continue;
                };
                insert(
                    &mut rows,
                    ReplayEvent {
                        session: timeline.session().clone(),
                        observed: r.at,
                        available: r.available,
                        retrieved: r.retrieved,
                        scope: Scope::Driver(*driver),
                        channel: "pit",
                        summary: format!(
                            "Pit visit L{}",
                            value
                                .lap
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| "--".into())
                        ),
                        evidence: format!(
                            "Recorded visit; lane {}s; stop {}s; phase unknown",
                            value
                                .lane_seconds
                                .map(|v| format!("{v:.3}"))
                                .unwrap_or_else(|| "--".into()),
                            value
                                .stop_seconds
                                .map(|v| format!("{v:.3}"))
                                .unwrap_or_else(|| "--".into())
                        ),
                    },
                    "pit".into(),
                );
            }
        }
        index.events = rows.into_values().collect();
        index.events.sort_by(|a, b| {
            (
                a.available_ms(),
                a.observed,
                &a.scope,
                a.channel,
                &a.summary,
            )
                .cmp(&(
                    b.available_ms(),
                    b.observed,
                    &b.scope,
                    b.channel,
                    &b.summary,
                ))
        });
        for indices in index.telemetry.values_mut() {
            cancel.check()?;
            indices.sort_unstable_by_key(|i| {
                let e = &timeline.events()[*i];
                (
                    e.observed,
                    available_ms(e.available),
                    timeline.sources()[e.source as usize].retrieved_at,
                )
            });
        }
        Ok(index)
    }
    pub(super) fn visible(&self, now: i64) -> &[ReplayEvent] {
        &self.events[..self.events.partition_point(|e| e.available_ms() <= now)]
    }
    pub(super) fn history(&self, timeline: &Timeline, driver: u32, now: i64) -> Vec<HistoryPoint> {
        let Some(indices) = self.telemetry.get(&driver) else {
            return Vec::new();
        };
        let end = indices.partition_point(|i| timeline.events()[*i].observed.0 <= now);
        let mut rows = BTreeMap::new();
        for i in indices[..end].iter().rev().take(512) {
            let e = &timeline.events()[*i];
            if e.observed.0 < now.saturating_sub(10_000) {
                break;
            }
            if !admitted(e.available, now) {
                continue;
            }
            if let Payload::Telemetry(t) = &e.payload {
                rows.entry(e.observed).or_insert_with(|| HistoryPoint {
                    at: e.observed,
                    speed_kmh: (!e.ambiguous).then_some(t.speed).flatten().map(|v| v.get()),
                    throttle_percent: (!e.ambiguous)
                        .then_some(t.throttle)
                        .flatten()
                        .map(|v| v.get()),
                });
            }
        }
        rows.into_values().collect()
    }
}

fn control_summary(kind: &ControlKind) -> String {
    match kind {
        ControlKind::Flag(flag) => match flag {
            Flag::Unknown(code) => format!("Unknown flag ({code})"),
            other => format!("{other:?} flag"),
        },
        ControlKind::SafetyCar(car) => match car {
            SafetyCar::Deployed => "Safety car deployed",
            SafetyCar::Ending => "Safety car ending",
            SafetyCar::InThisLap => "Safety car in this lap",
            SafetyCar::VirtualDeployed => "VSC deployed",
            SafetyCar::VirtualEnding => "VSC ending",
            SafetyCar::Returned => "Safety car returned",
        }
        .into(),
        ControlKind::Session(status) => match status {
            SessionStatus::Started => "Session started",
            SessionStatus::Resumed => "Session resumed",
            SessionStatus::Suspended => "Session suspended",
            SessionStatus::Finished => "Session finished",
        }
        .into(),
    }
}
