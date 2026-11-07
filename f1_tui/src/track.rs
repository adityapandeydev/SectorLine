//! Indexed, session-local observed lap reference. No external outline or demo fallback.
use crate::{
    data::{
        cache::SessionCache,
        limits::Cancellation,
        openf1::{Channel, Result},
    },
    domain::{
        geometry::*,
        normalize::{Event, Payload, utc_millis_ceiling},
        *,
    },
    replay::timeline::Timeline,
};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, PartialEq)]
pub struct LapWindow {
    pub driver: u32,
    pub lap: u32,
    pub start: i64,
    pub end: i64,
    pub pit_free: bool,
}
#[derive(Default)]
pub struct TrackArchive {
    session: Option<SessionId>,
    token: Option<std::sync::Arc<()>>,
    locations: BTreeMap<u32, Vec<usize>>,
    laps: Vec<LapWindow>,
    reference: Option<(u32, u32, Trace)>,
}
impl TrackArchive {
    pub fn build(timeline: &Timeline, laps: Vec<LapWindow>, cancel: &Cancellation) -> Result<Self> {
        cancel.check()?;
        let mut map = Self {
            session: Some(timeline.session().clone()),
            token: Some(timeline.token().clone()),
            laps,
            ..Self::default()
        };
        let count = timeline
            .events()
            .iter()
            .filter(|e| matches!(e.payload, Payload::Location(_)))
            .count();
        if count.saturating_mul(std::mem::size_of::<usize>()) > 24 * 1024 * 1024 {
            return Ok(map);
        }
        for (i, e) in timeline.events().iter().enumerate() {
            if i % 4096 == 0 {
                cancel.check()?;
            }
            if matches!(e.payload, Payload::Location(_)) {
                map.locations.entry(e.driver.get()).or_default().push(i);
            }
        }
        for indices in map.locations.values_mut() {
            cancel.check()?;
            indices.sort_unstable_by_key(|i| {
                let e = &timeline.events()[*i];
                (
                    e.observed,
                    match e.available {
                        Availability::Known(t) | Availability::Estimated(t) => t.0,
                        Availability::Unknown => i64::MAX,
                    },
                )
            });
        }
        // First deterministic complete, pit-free lap with contiguous coordinates wins.
        map.laps.sort_by_key(|l| (l.start, l.driver, l.lap));
        for lap in map.laps.iter().filter(|l| {
            l.pit_free && l.end > l.start && (30_000..=300_000).contains(&(l.end - l.start))
        }) {
            cancel.check()?;
            let Some(indices) = map.locations.get(&lap.driver) else {
                continue;
            };
            let start = indices.partition_point(|i| timeline.events()[*i].observed.0 < lap.start);
            let end = indices.partition_point(|i| timeline.events()[*i].observed.0 <= lap.end);
            let fixes = indices[start..end]
                .iter()
                .map(|i| fix(&timeline.events()[*i]))
                .collect::<Vec<_>>();
            if fixes.len() < 50
                || fixes.len() > 10_000
                || fixes
                    .iter()
                    .any(|f| f.ambiguous || f.point.is_none() || !f.admitted(i64::MAX))
            {
                continue;
            }
            if fixes[0].at - lap.start > MAX_GAP_MS
                || lap.end - fixes.last().unwrap().at > MAX_GAP_MS
                || fixes
                    .windows(2)
                    .any(|f| f[1].at <= f[0].at || f[1].at - f[0].at > MAX_GAP_MS)
            {
                continue;
            }
            if let Ok(trace) = Trace::new(fixes.iter().filter_map(|f| f.point).collect()) {
                map.reference = Some((lap.driver, lap.lap, trace));
                break;
            }
        }
        Ok(map)
    }
    pub fn from_cache(
        cache: &SessionCache,
        timeline: &Timeline,
        cancel: &Cancellation,
    ) -> Result<Self> {
        let mut laps = BTreeMap::<(u32, u32), Option<LapWindow>>::new();
        let mut out = BTreeSet::new();
        let mut pits = BTreeSet::new();
        let mut unknown_pits = BTreeSet::new();
        let pit_coverage = cache.spec().channels().contains(&Channel::Pit);
        for i in 0..cache.chunk_count() {
            cancel.check()?;
            let channel = cache.chunk_metadata(i)?.channel;
            if !matches!(channel, Channel::Laps | Channel::Pit) {
                continue;
            }
            let chunk = cache.read_chunk(i)?;
            for row in chunk.records() {
                let Some(driver) = row["driver_number"]
                    .as_u64()
                    .and_then(|v| u32::try_from(v).ok())
                else {
                    continue;
                };
                let lap = row["lap_number"]
                    .as_u64()
                    .and_then(|v| u32::try_from(v).ok());
                if channel == Channel::Pit {
                    if let Some(lap) = lap {
                        pits.insert((driver, lap));
                    } else {
                        unknown_pits.insert(driver);
                    }
                    continue;
                }
                let Some(lap) = lap else { continue };
                if row["is_pit_out_lap"].as_bool() != Some(false) {
                    out.insert((driver, lap));
                }
                let window = row["date_start"]
                    .as_str()
                    .and_then(|s| utc_millis_ceiling(s).ok())
                    .zip(row["lap_duration"].as_f64())
                    .and_then(|(start, d)| {
                        if !d.is_finite() || !(30.0..=300.0).contains(&d) {
                            return None;
                        }
                        Some(LapWindow {
                            driver,
                            lap,
                            start: start.0,
                            end: start.0.checked_add((d * 1000.0).ceil() as i64)?,
                            pit_free: pit_coverage
                                && row["is_pit_out_lap"].as_bool() == Some(false),
                        })
                    });
                laps.entry((driver, lap))
                    .and_modify(|old| {
                        if *old != window {
                            *old = None;
                        }
                    })
                    .or_insert(window);
            }
        }
        let next_laps = laps.clone();
        let windows = laps
            .into_values()
            .flatten()
            .map(|mut l| {
                // Exclude pit-in, pit-out, unknown and unbounded last laps. No pit-phase inference.
                l.pit_free &= !unknown_pits.contains(&l.driver)
                    && !pits.contains(&(l.driver, l.lap))
                    && !out.contains(&(l.driver, l.lap))
                    && l.lap.checked_add(1).is_some_and(|next| {
                        if let Some(Some(next_lap)) = next_laps.get(&(l.driver, next)) {
                            l.end = l.end.min(next_lap.start);
                            next_lap.start > l.start && !out.contains(&(l.driver, next))
                        } else {
                            false
                        }
                    });
                l
            })
            .collect();
        Self::build(timeline, windows, cancel)
    }
    pub fn reference(&self) -> Option<(u32, u32, &Trace)> {
        self.reference.as_ref().map(|(d, l, t)| (*d, *l, t))
    }
    pub fn position(&self, timeline: &Timeline, driver: u32, now: i64) -> Option<MapPosition> {
        if self.session.as_ref() != Some(timeline.session())
            || !self
                .token
                .as_ref()
                .is_some_and(|token| std::sync::Arc::ptr_eq(token, timeline.token()))
        {
            return None;
        }
        let (_, _, trace) = self.reference()?;
        let indices = self.locations.get(&driver)?;
        let target = now.saturating_sub(MAP_DELAY_MS).max(timeline.origin().0);
        let end = indices.partition_point(|i| timeline.events()[*i].observed.0 <= target);
        // Bound pathological delayed-revision scans; lack of a usable fix stays unavailable.
        let n = (0..end)
            .rev()
            .take(64)
            .find(|n| fix(&timeline.events()[indices[*n]]).admitted(now))?;
        let a = fix(&timeline.events()[indices[n]]);
        let b = indices.get(n + 1).map(|i| fix(&timeline.events()[*i]));
        let pit_free = b.is_some_and(|b| {
            self.laps
                .iter()
                .any(|l| l.driver == driver && l.pit_free && l.start <= a.at && b.at <= l.end)
        });
        sample(a, b, target, now, pit_free, trace.max_step())
    }
}
fn fix(event: &Event) -> Fix {
    let point = match &event.payload {
        Payload::Location(c) => {
            let (x, y, _) = c.xyz();
            x.zip(y).map(|(x, y)| Point { x, y }).filter(|p| p.valid())
        }
        _ => None,
    };
    Fix {
        at: event.observed.0,
        available: event.available,
        point,
        ambiguous: event.ambiguous,
    }
}
