//! Deterministic available-by-T projection of a historical provider snapshot.
use crate::{
    data::{
        cache::SessionCache,
        limits::Cancellation,
        openf1::{Channel, ChunkMetadata, Result},
    },
    domain::{
        normalize::{self, Event, Payload, RosterDriver},
        *,
    },
};
pub(crate) mod ingest;
mod lap_state;
use std::sync::Arc;
use std::{cmp::Ordering, collections::BTreeMap, num::NonZeroU32};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayPolicy {
    KnownOnly,
    EventTimeEstimates,
}
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NormalizationReport {
    pub duplicate_rows: usize,
    pub conflicting_rows: usize,
    pub ambiguous_groups: usize,
}
impl ReplayPolicy {
    fn permits(self, available: Availability, time: UtcMillis) -> bool {
        match available {
            Availability::Known(t) => t <= time,
            Availability::Estimated(t) => self == Self::EventTimeEstimates && t <= time,
            Availability::Unknown => false,
        }
    }
}
pub struct Timeline {
    token: Arc<()>,
    session: SessionId,
    origin: UtcMillis,
    drivers: Vec<RosterDriver>,
    sources: Vec<ChunkMetadata>,
    events: Vec<Event>,
    end: ElapsedMillis,
    report: NormalizationReport,
    conditions: conditions::ConditionsArchive,
}
impl Timeline {
    pub fn new(
        session: SessionId,
        origin: UtcMillis,
        mut drivers: Vec<RosterDriver>,
        sources: Vec<ChunkMetadata>,
        mut events: Vec<Event>,
    ) -> Result<Self> {
        if session.provider() != Provider::OpenF1 {
            return Err("This timeline requires OpenF1 recorded sources".into());
        }
        drivers.sort_by_key(|d| d.number);
        if drivers.is_empty() || drivers.windows(2).any(|p| p[0].number == p[1].number) {
            return Err("Invalid recorded roster".into());
        }
        let key = session.key().parse::<u32>()?;
        if sources
            .iter()
            .any(|s| s.session_key != key || s.status != 200)
        {
            return Err("Timeline source identity/status mismatch".into());
        }
        for event in &events {
            let source = sources
                .get(event.source as usize)
                .ok_or("Missing source provenance")?;
            if source.channel != event.payload.channel()
                || drivers
                    .binary_search_by_key(&event.driver, |d| d.number)
                    .is_err()
            {
                return Err("Event channel/driver does not match source roster".into());
            }
        }
        // Same eligibility time -> event time -> channel -> driver. Repeated keys
        // choose latest retrieval; exact ties use canonical typed payload order.
        events.sort_unstable_by(|a, b| {
            event_key(a)
                .cmp(&event_key(b))
                .then_with(|| {
                    sources[a.source as usize]
                        .retrieved_at
                        .cmp(&sources[b.source as usize].retrieved_at)
                })
                .then_with(|| payload_cmp(&a.payload, &b.payload))
                .then_with(|| {
                    sources[a.source as usize]
                        .source_url
                        .cmp(&sources[b.source as usize].source_url)
                })
        });
        // Reverse first: retain the winner, then restore playback order. Corrections
        // with different availability boundaries remain separate events.
        events.reverse();
        let mut report = NormalizationReport::default();
        let mut retained = 0;
        for index in 0..events.len() {
            if retained > 0 && event_key(&events[retained - 1]) == event_key(&events[index]) {
                if events[retained - 1].payload == events[index].payload {
                    report.duplicate_rows += 1;
                } else {
                    report.conflicting_rows += 1;
                    if sources[events[retained - 1].source as usize].retrieved_at
                        == sources[events[index].source as usize].retrieved_at
                        && !events[retained - 1].ambiguous
                    {
                        events[retained - 1].ambiguous = true;
                        report.ambiguous_groups += 1;
                    }
                }
            } else {
                events.swap(retained, index);
                retained += 1;
            }
        }
        events.truncate(retained);
        events.reverse();
        let end = ElapsedMillis(
            events
                .iter()
                .filter_map(|e| match e.available {
                    Availability::Known(t) | Availability::Estimated(t) => {
                        Some(t.0.saturating_sub(origin.0).max(0) as u64)
                    }
                    Availability::Unknown => None,
                })
                .max()
                .unwrap_or(0),
        );
        Ok(Self {
            token: Arc::new(()),
            session,
            origin,
            drivers,
            sources,
            events,
            end,
            report,
            conditions: conditions::ConditionsArchive::default(),
        })
    }
    pub fn from_cache(
        cache: &SessionCache,
        cancel: &Cancellation,
        progress: &dyn Fn(usize, usize),
    ) -> Result<Self> {
        let mut builder = ingest::CoreBuilder::new(cache)?;
        for i in 0..cache.chunk_count() {
            cancel.check()?;
            let meta = cache.chunk_metadata(i)?;
            if meta.channel == Channel::Drivers
                || (ingest::CHANNELS.contains(&meta.channel) && meta.status == 200)
            {
                builder.visit(&cache.read_chunk(i)?)?;
            }
            progress(i + 1, cache.chunk_count());
        }
        builder.finish(cache, cancel)
    }
    pub fn with_conditions(mut self, archive: conditions::ConditionsArchive) -> Self {
        self.conditions = archive;
        self
    }
    pub fn conditions_at(
        &self,
        time: ElapsedMillis,
        policy: ReplayPolicy,
    ) -> Result<conditions::ConditionsFrame> {
        self.conditions.frame(
            &self.session,
            self.origin,
            time,
            policy == ReplayPolicy::EventTimeEstimates,
        )
    }
    pub fn origin(&self) -> UtcMillis {
        self.origin
    }
    pub fn session(&self) -> &SessionId {
        &self.session
    }
    pub fn drivers(&self) -> &[RosterDriver] {
        &self.drivers
    }
    pub(crate) fn conditions_archive(&self) -> &conditions::ConditionsArchive {
        &self.conditions
    }
    pub(crate) fn sources(&self) -> &[ChunkMetadata] {
        &self.sources
    }
    pub(crate) fn token(&self) -> &Arc<()> {
        &self.token
    }
    pub(crate) fn events(&self) -> &[Event] {
        &self.events
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn report(&self) -> &NormalizationReport {
        &self.report
    }
    pub fn end(&self) -> ElapsedMillis {
        self.end
    }
    pub fn state_at(&self, time: ElapsedMillis, policy: ReplayPolicy) -> Result<SessionState> {
        let mut state = SessionState {
            token: self.token.clone(),
            elapsed: ElapsedMillis(0),
            policy,
            next: 0,
            laps: BTreeMap::new(),
            decisions: BTreeMap::new(),
            drivers: self.drivers.iter().map(|d| (d.number, [None; 6])).collect(),
        };
        state.advance_to(self, time)?;
        Ok(state)
    }
}
fn payload_cmp(a: &Payload, b: &Payload) -> Ordering {
    if a == b {
        Ordering::Equal
    } else {
        serde_json::to_vec(a)
            .expect("finite typed payload")
            .cmp(&serde_json::to_vec(b).expect("finite typed payload"))
    }
}
fn availability_key(available: Availability) -> (i64, u8) {
    match available {
        Availability::Known(t) => (t.0, 0),
        Availability::Estimated(t) => (t.0, 1),
        Availability::Unknown => (i64::MAX, 2),
    }
}
fn channel_index(channel: Channel) -> usize {
    match channel {
        Channel::Position => 0,
        Channel::CarData => 1,
        Channel::Location => 2,
        Channel::Intervals => 3,
        Channel::Laps => 4,
        Channel::RaceControl => 5,
        _ => unreachable!("core channel"),
    }
}
fn event_key(event: &Event) -> ((i64, u8), i64, usize, NonZeroU32, u32) {
    (
        availability_key(event.available),
        event.observed.0,
        channel_index(event.payload.channel()),
        event.driver,
        match &event.payload {
            Payload::Lap(v) => v.number.get(),
            Payload::LapDecision { lap, .. } => lap.get(),
            _ => 0,
        },
    )
}

#[derive(Clone)]
pub struct SessionState {
    token: Arc<()>,
    elapsed: ElapsedMillis,
    policy: ReplayPolicy,
    next: usize,
    drivers: BTreeMap<NonZeroU32, [Option<usize>; 6]>,
    laps: BTreeMap<(NonZeroU32, u32), usize>,
    decisions: BTreeMap<(NonZeroU32, u32), usize>,
}
impl SessionState {
    pub fn elapsed(&self) -> ElapsedMillis {
        self.elapsed
    }
    pub fn policy(&self) -> ReplayPolicy {
        self.policy
    }
    pub fn advance_to(&mut self, timeline: &Timeline, time: ElapsedMillis) -> Result<()> {
        if !Arc::ptr_eq(&self.token, &timeline.token) {
            return Err("State belongs to another timeline".into());
        }
        if time < self.elapsed {
            return Err(
                "Backward replay needs reconstruction; forward reducer cannot travel back".into(),
            );
        }
        let utc = UtcMillis(
            timeline
                .origin
                .0
                .checked_add(i64::try_from(time.0)?)
                .ok_or("Replay time overflow")?,
        );
        while let Some(event) = timeline.events.get(self.next) {
            if availability_key(event.available).0 > utc.0 {
                break;
            }
            if self.policy.permits(event.available, utc) {
                let map = match &event.payload {
                    Payload::Lap(lap) => Some((&mut self.laps, lap.number.get())),
                    Payload::LapDecision { lap, .. } => Some((&mut self.decisions, lap.get())),
                    _ => None,
                };
                if let Some((map, lap)) = map {
                    let key = (event.driver, lap);
                    let old = map.get(&key).copied();
                    if old.is_none_or(|i| {
                        availability_key(event.available)
                            >= availability_key(timeline.events[i].available)
                    }) {
                        map.insert(key, self.next);
                    }
                } else {
                    let slot = &mut self
                        .drivers
                        .get_mut(&event.driver)
                        .ok_or("State roster mismatch")?[channel_index(event.payload.channel())];
                    let old = *slot;
                    if newer_event(timeline, old, event) {
                        *slot = Some(self.next);
                    }
                }
            }
            self.next += 1;
        }
        self.elapsed = time;
        Ok(())
    }
    pub fn frame(&self, timeline: &Timeline) -> Result<PlaybackFrame> {
        if !Arc::ptr_eq(&self.token, &timeline.token) {
            return Err("State belongs to another timeline".into());
        }
        let mut cars = Vec::with_capacity(self.drivers.len());
        for (&driver, slots) in &self.drivers {
            let position = slots[0].map(|i| &timeline.events[i]);
            let telemetry = slots[1].map(|i| &timeline.events[i]);
            let intervals = slots[3].map(|i| &timeline.events[i]);
            let values = match telemetry.map(|e| &e.payload) {
                Some(Payload::Telemetry(v)) => Some(v),
                _ => None,
            };
            let (leader, ahead) = match intervals.map(|e| &e.payload) {
                Some(Payload::Intervals { leader, ahead }) => (leader.clone(), ahead.clone()),
                _ => (None, None),
            };
            let pos = match position.map(|e| &e.payload) {
                Some(Payload::Position(v)) => *v,
                _ => None,
            };
            let completed = self.completed_laps(timeline, driver)?;
            let status = slots[5].map(|i| &timeline.events[i]);
            cars.push(CarSample {
                driver: DriverId::new(timeline.session.clone(), driver.get())?,
                position: observation(timeline, position, pos, "position")?,
                speed_kmh: observation(
                    timeline,
                    telemetry,
                    values.and_then(|v| v.speed),
                    "car_data",
                )?,
                rpm: observation(timeline, telemetry, values.and_then(|v| v.rpm), "car_data")?,
                gear: observation(timeline, telemetry, values.and_then(|v| v.gear), "car_data")?,
                throttle: observation(
                    timeline,
                    telemetry,
                    values.and_then(|v| v.throttle),
                    "car_data",
                )?,
                brake: observation(
                    timeline,
                    telemetry,
                    values.and_then(|v| v.brake),
                    "car_data",
                )?,
                drs: observation(timeline, telemetry, values.and_then(|v| v.drs), "car_data")?,
                gap_to_leader: observation(timeline, intervals, leader, "intervals")?,
                gap_to_ahead: observation(timeline, intervals, ahead, "intervals")?,
                lap_eligibility: completed.eligibility,
                lap_number: completed.number,
                lap_time: completed.duration,
                personal_best: completed.best,
                status: observation(
                    timeline,
                    status,
                    status.and_then(|e| match e.payload {
                        Payload::Status(v) => Some(v),
                        _ => None,
                    }),
                    "race_control",
                )?,
                sectors: completed.sectors,
            });
        }
        cars.sort_by_key(|c| {
            (
                c.position.value().map(|p| p.get()).unwrap_or(u32::MAX),
                c.driver.number,
            )
        });
        let now = UtcMillis(
            timeline
                .origin
                .0
                .checked_add(i64::try_from(self.elapsed.0)?)
                .ok_or("Replay time overflow")?,
        );
        for car in &mut cars {
            car.speed_kmh = quality::at_replay_time(&car.speed_kmh, now, "car_data")?;
            car.rpm = quality::at_replay_time(&car.rpm, now, "car_data")?;
            car.gear = quality::at_replay_time(&car.gear, now, "car_data")?;
            car.throttle = quality::at_replay_time(&car.throttle, now, "car_data")?;
            car.brake = quality::at_replay_time(&car.brake, now, "car_data")?;
            car.drs = quality::at_replay_time(&car.drs, now, "car_data")?;
            car.gap_to_leader = quality::at_replay_time(&car.gap_to_leader, now, "intervals")?;
            car.gap_to_ahead = quality::at_replay_time(&car.gap_to_ahead, now, "intervals")?;
        }
        Ok(PlaybackFrame::new(
            PlaybackMode::Recorded,
            timeline.session.clone(),
            self.elapsed,
            cars,
        )?)
    }
    pub fn location<'a>(
        &self,
        timeline: &'a Timeline,
        driver: NonZeroU32,
    ) -> Option<&'a normalize::Coordinates> {
        if !Arc::ptr_eq(&self.token, &timeline.token) {
            return None;
        }
        self.drivers
            .get(&driver)
            .and_then(|slots| slots[2])
            .and_then(|i| match &timeline.events[i].payload {
                Payload::Location(c) if !timeline.events[i].ambiguous => Some(c),
                _ => None,
            })
    }
}
fn newer_event(timeline: &Timeline, old: Option<usize>, event: &Event) -> bool {
    old.is_none_or(|old| {
        let old = &timeline.events[old];
        event.observed > old.observed
            || (event.observed == old.observed
                && timeline.sources[event.source as usize].retrieved_at
                    >= timeline.sources[old.source as usize].retrieved_at)
    })
}
fn observation<T>(
    timeline: &Timeline,
    event: Option<&Event>,
    mut value: Option<T>,
    channel: &str,
) -> Result<Observation<T>> {
    let timing = event
        .map(|e| Timing {
            observed_at: Some(e.observed),
            available_at: e.available,
            retrieved_at: Some(timeline.sources[e.source as usize].retrieved_at),
            elapsed: Some(ElapsedMillis(
                e.observed.0.saturating_sub(timeline.origin.0).max(0) as u64,
            )),
        })
        .unwrap_or_default();
    let ambiguous = event.is_some_and(|e| e.ambiguous);
    if ambiguous {
        value = None;
    }
    let reason = if ambiguous {
        Some("Conflicting same-time source values; no defensible winner".into())
    } else if value.is_some() {
        None
    } else if event.is_some() {
        Some("Source value missing or invalid; raw chunk retained".into())
    } else {
        Some("No eligible observation at this time".into())
    };
    let quality = if value.is_some() {
        Quality::Observed
    } else {
        Quality::Unavailable
    };
    Ok(Observation::new(
        value,
        Source {
            session: timeline.session.clone(),
            channel: channel.into(),
            origin: Origin::Recorded,
        },
        timing,
        quality,
        reason,
    )?)
}
