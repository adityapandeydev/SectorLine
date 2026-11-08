//! Session applicability and source-evidenced qualifying parts; no rule-duration countdowns.
use super::{conditions::Reading, *};
use std::collections::BTreeMap;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    pub start: UtcMillis,
    pub end: UtcMillis,
}
#[derive(Debug, Clone)]
pub struct SessionContext {
    pub id: SessionId,
    pub kind: SessionKind,
    pub schedule: Option<Schedule>,
}
impl SessionContext {
    pub fn unknown(id: SessionId) -> Self {
        Self {
            id,
            kind: SessionKind::Unknown("Metadata unavailable".into()),
            schedule: None,
        }
    }
    pub fn from_names(id: SessionId, name: &str, kind: &str) -> Self {
        fn parse(s: &str) -> Option<SessionKind> {
            match s.trim().to_ascii_lowercase().as_str() {
                "race" => Some(SessionKind::Race),
                "sprint" | "sprint race" => Some(SessionKind::Sprint),
                "qualifying" => Some(SessionKind::Qualifying),
                "sprint qualifying" | "sprint shootout" => Some(SessionKind::SprintQualifying),
                "practice" | "practice 1" | "practice 2" | "practice 3" => {
                    Some(SessionKind::Practice)
                }
                _ => None,
            }
        }
        let kind = match (parse(name), parse(kind)) {
            (Some(SessionKind::Sprint), Some(SessionKind::Race | SessionKind::Sprint)) => {
                SessionKind::Sprint
            }
            (
                Some(SessionKind::SprintQualifying),
                Some(SessionKind::Qualifying | SessionKind::SprintQualifying),
            ) => SessionKind::SprintQualifying,
            (Some(a), Some(b)) if a == b => a,
            (None, Some(b)) => b,
            (Some(a), None) if kind.trim().is_empty() => a,
            _ => SessionKind::Unknown("Missing/conflicting session metadata".into()),
        };
        Self {
            id,
            kind,
            schedule: None,
        }
    }
    pub fn with_schedule(
        mut self,
        start: UtcMillis,
        end: UtcMillis,
    ) -> Result<Self, ContractError> {
        if !end.0.checked_sub(start.0).is_some_and(|n| n > 0) {
            return Err(ContractError("Invalid session schedule"));
        }
        self.schedule = Some(Schedule { start, end });
        Ok(self)
    }
    pub fn race_fields(&self) -> bool {
        matches!(self.kind, SessionKind::Race | SessionKind::Sprint)
    }
    pub fn qualifying(&self) -> bool {
        matches!(
            self.kind,
            SessionKind::Qualifying | SessionKind::SprintQualifying
        )
    }
    pub fn label(&self) -> &'static str {
        match self.kind {
            SessionKind::Race => "Race",
            SessionKind::Sprint => "Sprint",
            SessionKind::Qualifying => "Qualifying",
            SessionKind::SprintQualifying => "Sprint qualifying",
            SessionKind::Practice => "Practice",
            SessionKind::Unknown(_) => "Unknown session",
        }
    }
    pub fn part_label(&self, n: u8) -> String {
        format!(
            "{}{n}",
            if self.kind == SessionKind::SprintQualifying {
                "SQ"
            } else {
                "Q"
            }
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseBoundary {
    Start,
    Report,
    Close,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseUpdate {
    pub part: Option<u8>,
    pub boundary: PhaseBoundary,
}
impl PhaseUpdate {
    pub fn from_record(row: &serde_json::Value) -> Option<Self> {
        let message = row["message"].as_str().unwrap_or("");
        let category = row["category"].as_str().unwrap_or("");
        let start = (category == "SessionStatus" && message == "SESSION STARTED")
            || (category == "Flag"
                && row["scope"] == "Track"
                && row["flag"] == "GREEN"
                && message == "GREEN LIGHT - PIT EXIT OPEN");
        let close = (category == "SessionStatus" && message == "SESSION FINISHED")
            || (category == "Flag" && row["scope"] == "Track" && row["flag"] == "CHEQUERED");
        // Null on ordinary messages means no phase update, not phase end.
        if row["qualifying_phase"].is_null() && !start && !close {
            return None;
        }
        let part = row["qualifying_phase"]
            .as_u64()
            .filter(|n| (1..=3).contains(n))
            .map(|n| n as u8);
        Some(Self {
            part,
            boundary: if start {
                PhaseBoundary::Start
            } else if close {
                PhaseBoundary::Close
            } else {
                PhaseBoundary::Report
            },
        })
    }
}
#[derive(Debug, Clone)]
struct PartWindow {
    part: u8,
    start: i64,
    cutoff: Option<i64>,
    end: Option<i64>,
}
#[derive(Debug, Clone, Default)]
pub struct PhaseProjection {
    pub part: Option<u8>,
    pub closed: bool,
    windows: Vec<PartWindow>,
}
impl PhaseProjection {
    pub fn at(rows: &[Reading<PhaseUpdate>], now: i64) -> Self {
        let mut canonical = BTreeMap::<(i64, i64), (UtcMillis, Option<PhaseUpdate>)>::new();
        for r in rows {
            let available = match r.available {
                Availability::Known(t) | Availability::Estimated(t) if t.0 <= now => t.0,
                _ => continue,
            };
            if r.at.0 > now {
                continue;
            }
            let entry = canonical
                .entry((r.at.0, available))
                .or_insert((r.retrieved, r.value));
            if r.retrieved > entry.0 {
                *entry = (r.retrieved, r.value);
            } else if r.retrieved == entry.0 && entry.1 != r.value {
                entry.1 = None;
            }
        }
        let mut result = Self::default();
        let mut open: Option<usize> = None;
        for ((at, _), (_, value)) in canonical {
            let part = value.and_then(|v| v.part).filter(|n| (1..=3).contains(n));
            let boundary = value.map(|v| v.boundary);
            if boundary == Some(PhaseBoundary::Close) && part.is_none() && value.is_some() {
                if let Some(i) = open {
                    let window = &mut result.windows[i];
                    window.cutoff = Some(window.cutoff.map_or(at, |old| old.min(at)));
                }
                result.closed = result.part.is_some();
                continue;
            }
            if part.is_none() {
                if let Some(i) = open.take() {
                    result.windows[i].end = Some(at);
                }
                result.part = None;
                result.closed = false;
                continue;
            }
            let part = part.unwrap();
            if boundary == Some(PhaseBoundary::Start) {
                if result.part != Some(part) || result.closed || open.is_none() {
                    if let Some(i) = open {
                        result.windows[i].end = Some(at);
                    }
                    result.windows.push(PartWindow {
                        part,
                        start: at,
                        cutoff: None,
                        end: None,
                    });
                    open = Some(result.windows.len() - 1);
                }
                result.closed = false;
            } else if result.part != Some(part) {
                if let Some(i) = open.take() {
                    result.windows[i].end = Some(at);
                }
                result.closed = false;
            }
            result.part = Some(part);
            if boundary == Some(PhaseBoundary::Close) {
                if let Some(i) = open {
                    let window = &mut result.windows[i];
                    window.cutoff = Some(window.cutoff.map_or(at, |old| old.min(at)));
                }
                result.closed = true;
            }
        }
        result
    }
    pub fn has_current_start(&self) -> bool {
        self.part.is_some_and(|part| {
            self.windows
                .iter()
                .any(|w| w.part == part && w.end.is_none())
        })
    }
    pub fn part_for_lap(&self, start: i64, end: i64) -> Option<u8> {
        if end < start {
            return None;
        }
        self.windows
            .iter()
            .find(|w| {
                w.start <= start
                    && w.cutoff.is_none_or(|t| start < t)
                    && w.end.is_none_or(|t| end <= t)
            })
            .map(|w| w.part)
    }
}
