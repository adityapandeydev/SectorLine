//! Deterministic descriptions of admitted evidence. No prediction or inferred overtake.
use crate::{
    data::{
        limits::Cancellation,
        openf1::{Channel, Result},
    },
    domain::{
        conditions::{self, Reading},
        normalize::{Event, Payload},
        *,
    },
    replay::{RecordedReplay, timeline::Timeline},
};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InsightKind {
    GapTrend,
    NearbyInterval,
    PositionChange,
    PitVisit,
    TyreReport,
    EligibleBest,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Insight {
    pub kind: InsightKind,
    pub driver: u32,
    pub window_start: UtcMillis,
    pub observed: UtcMillis,
    pub available: Availability,
    pub summary: String,
    pub evidence: String,
}
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    trend_window_ms: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            trend_window_ms: 30000,
        }
    }
}
impl Policy {
    pub fn new(trend_window_ms: u64) -> Result<Self> {
        if !(10000..=120000).contains(&trend_window_ms) {
            return Err("Insight trend window must be 10-120 seconds".into());
        }
        Ok(Self { trend_window_ms })
    }
}
fn available(a: Availability) -> Option<i64> {
    match a {
        Availability::Known(t) | Availability::Estimated(t) => Some(t.0),
        Availability::Unknown => None,
    }
}
fn merge(a: Availability, b: Availability) -> Availability {
    let t = UtcMillis(available(a).unwrap().max(available(b).unwrap()));
    if matches!(a, Availability::Estimated(_)) || matches!(b, Availability::Estimated(_)) {
        Availability::Estimated(t)
    } else {
        Availability::Known(t)
    }
}
fn recent(at: UtcMillis, now: i64, budget: i64) -> bool {
    at.0 <= now && now.saturating_sub(at.0) <= budget
}
fn seconds(g: &Option<Gap>) -> Option<f64> {
    if let Some(Gap::Seconds(s)) = g {
        Some(s.get())
    } else {
        None
    }
}
#[derive(Default)]
pub(crate) struct Index {
    rows: BTreeMap<(u32, Channel), Vec<usize>>,
}
impl Index {
    pub(crate) fn build(timeline: &Timeline, cancel: &Cancellation) -> Result<Self> {
        let mut index = Self::default();
        for (i, e) in timeline.events().iter().enumerate() {
            if i % 4096 == 0 {
                cancel.check()?;
            }
            if matches!(
                e.payload,
                Payload::Position(_)
                    | Payload::Intervals { .. }
                    | Payload::Lap(_)
                    | Payload::LapDecision { .. }
            ) {
                index
                    .rows
                    .entry((e.driver.get(), e.payload.channel()))
                    .or_default()
                    .push(i);
            }
        }
        for rows in index.rows.values_mut() {
            rows.sort_by_key(|i| timeline.events()[*i].observed);
        }
        Ok(index)
    }
    // The same observation can be corrected at a later availability boundary.
    // Resolve only admitted versions; a conflict/missing value stays in the window.
    fn window<'a>(
        &self,
        t: &'a Timeline,
        driver: u32,
        channel: Channel,
        start: i64,
        now: i64,
    ) -> Vec<&'a Event> {
        let Some(rows) = self.rows.get(&(driver, channel)) else {
            return Vec::new();
        };
        let lo = rows.partition_point(|i| t.events()[*i].observed.0 < start);
        let hi = rows.partition_point(|i| t.events()[*i].observed.0 <= now);
        let mut values = BTreeMap::<(UtcMillis, u32), &Event>::new();
        for i in &rows[lo..hi] {
            let e = &t.events()[*i];
            if !available(e.available).is_some_and(|a| a <= now) {
                continue;
            }
            let slot = match &e.payload {
                Payload::Lap(l) => l.number.get(),
                Payload::LapDecision { lap, .. } => lap.get(),
                _ => 0,
            };
            let key = (e.observed, slot);
            let rank = |v: &Event| {
                (
                    t.sources()[v.source as usize].retrieved_at,
                    available(v.available),
                )
            };
            if values.get(&key).is_none_or(|old| rank(e) >= rank(old)) {
                values.insert(key, e);
            }
        }
        values.into_values().collect()
    }
    pub(crate) fn at(&self, r: &RecordedReplay, policy: Policy) -> Vec<Insight> {
        let t = r.timeline();
        let driver = r.selected();
        let now = t
            .origin()
            .0
            .saturating_add(r.frame().elapsed().0.min(i64::MAX as u64) as i64);
        let mut result = Vec::new();
        let mut push = |kind, start, observed, availability, summary, evidence| {
            result.push(Insight {
                kind,
                driver,
                window_start: start,
                observed,
                available: availability,
                summary,
                evidence,
            })
        };
        let intervals = self.window(
            t,
            driver,
            Channel::Intervals,
            now.saturating_sub(policy.trend_window_ms as i64),
            now,
        );
        let fresh = crate::domain::quality::stale_after("intervals").unwrap() as i64;
        if r.context().race_fields()
            && let Some(last) = intervals
                .last()
                .filter(|e| !e.ambiguous && recent(e.observed, now, fresh))
        {
            if let Payload::Intervals { ahead, .. } = &last.payload
                && let Some(n) = seconds(ahead).filter(|s| *s <= 1.0)
            {
                push(
                    InsightKind::NearbyInterval,
                    last.observed,
                    last.observed,
                    last.available,
                    format!("Reported interval ahead {n:.3}s"),
                    "intervals | reported interval <=1.000s; latest report <=10s old".into(),
                );
            }
            let valid=intervals.iter().all(|e|!e.ambiguous && matches!(&e.payload,Payload::Intervals{leader,..} if seconds(leader).is_some()));
            if valid && intervals.len() >= 3 {
                let first = intervals[0];
                let span = last.observed.0.saturating_sub(first.observed.0);
                let contiguous = intervals
                    .windows(2)
                    .all(|p| p[1].observed.0.saturating_sub(p[0].observed.0) <= fresh);
                if span >= 5000 && contiguous {
                    let value = |e: &Event| {
                        if let Payload::Intervals { leader, .. } = &e.payload {
                            seconds(leader).unwrap()
                        } else {
                            unreachable!()
                        }
                    };
                    let a = value(first);
                    let b = value(last);
                    let availability = intervals
                        .iter()
                        .fold(first.available, |a, e| merge(a, e.available));
                    push(
                        InsightKind::GapTrend,
                        first.observed,
                        last.observed,
                        availability,
                        format!("Reported leader gap {a:.3} -> {b:.3}s ({:+.3}s)", b - a),
                        format!(
                            "intervals | {} reports / {:.3}s; endpoint change; sample gaps <=10s; latest report <=10s old",
                            intervals.len(),
                            span as f64 / 1000.0
                        ),
                    );
                }
            }
        }
        let positions = self.window(t, driver, Channel::Position, now.saturating_sub(30000), now);
        if let Some(last) = positions.last().filter(|e| !e.ambiguous)
            && let Payload::Position(Some(current)) = last.payload
        {
            // Repeated equal reports refresh evidence, not the transition timestamp.
            let mut run = positions.len() - 1;
            while run > 0
                && !positions[run - 1].ambiguous
                && matches!(positions[run-1].payload,Payload::Position(Some(n)) if n==current)
            {
                run -= 1;
            }
            if run > 0 {
                let prior = positions[run - 1];
                let changed = positions[run];
                if !prior.ambiguous
                    && let Payload::Position(Some(before)) = prior.payload
                {
                    push(InsightKind::PositionChange,prior.observed,changed.observed,merge(prior.available,changed.available),format!("Reported position {before} -> {current}"),"position | cause unestablished; pits, retirements and corrections can change classification".into());
                }
            }
        }
        let c = t.conditions_archive();
        if let Some((row, pit)) =
            latest(c.pits.get(&driver), now).filter(|(r, _)| recent(r.at, now, 60000))
        {
            push(
                InsightKind::PitVisit,
                row.at,
                row.at,
                row.available,
                format!(
                    "Recorded pit visit L{}",
                    pit.lap
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "--".into())
                ),
                "pit | report age <=60s; entry/exit phase unknown".into(),
            );
        }
        if let Some((row, stint)) =
            latest(c.stints.get(&driver), now).filter(|(r, _)| recent(r.at, now, 60000))
            && let Some(compound) = stint
                .compound
                .as_deref()
                .filter(|v| matches!(*v, "SOFT" | "MEDIUM" | "HARD" | "INTERMEDIATE" | "WET"))
        {
            push(
                InsightKind::TyreReport,
                row.at,
                row.at,
                row.available,
                format!("Stint {} reported: {compound}", stint.number),
                format!(
                    "stints | observed lap{} anchor; report age <=60s; tyre-change instant unknown",
                    stint.lap_start
                ),
            );
        }
        let bests = r.eligible_bests();
        if let Some(best) = bests.get(&driver) {
            let laps = self.window(t, driver, Channel::Laps, i64::MIN, now);
            let chosen = laps
                .iter()
                .filter(|e| matches!(&e.payload,Payload::Lap(l) if l.number==best.number))
                .max_by_key(|e| available(e.available));
            if let Some(chosen) = chosen {
                let mut boundary = chosen.available;
                let mut start = chosen.observed;
                let mut end = chosen.observed;
                for d in t.drivers() {
                    for ch in [Channel::Laps, Channel::RaceControl] {
                        for e in self.window(t, d.number.get(), ch, i64::MIN, now) {
                            boundary = merge(boundary, e.available);
                            start = start.min(e.observed);
                            end = end.max(e.observed);
                        }
                    }
                }
                if r.context().qualifying() {
                    for p in &c.phases {
                        if available(p.available).is_some_and(|a| a <= now) && p.at.0 <= now {
                            boundary = merge(boundary, p.available);
                            start = start.min(p.at);
                            end = end.max(p.at);
                        }
                    }
                }
                let lowest = bests
                    .iter()
                    .min_by(|(da, a), (db, b)| {
                        a.duration
                            .get()
                            .total_cmp(&b.duration.get())
                            .then_with(|| da.cmp(db))
                    })
                    .unwrap();
                let scope = if r.context().qualifying() {
                    r.phase()
                        .part
                        .map(|p| r.context().part_label(p))
                        .unwrap_or_else(|| "phase --".into())
                } else {
                    "recording".into()
                };
                push(
                    InsightKind::EligibleBest,
                    start,
                    end,
                    boundary,
                    format!(
                        "Eligible personal best {scope}: {:.3}s L{}",
                        best.duration.get(),
                        best.number
                    ),
                    format!(
                        "laps + explicit eligibility decisions | lowest eligible best in {scope}: #{} {:.3}s; admitted recording only, provisional/invalid/pit-out excluded",
                        lowest.0,
                        lowest.1.duration.get()
                    ),
                );
            }
        }
        result.sort_by_key(|i| i.kind);
        result
    }
}
fn latest<T: PartialEq>(rows: Option<&Vec<Reading<T>>>, now: i64) -> Option<(&Reading<T>, &T)> {
    let (row, conflict) =
        conditions::latest(rows.map(Vec::as_slice).unwrap_or(&[]), UtcMillis(now), true);
    let row = row.filter(|r| !conflict && r.at.0 <= now)?;
    Some((row, row.value.as_ref()?))
}
