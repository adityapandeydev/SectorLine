pub mod checkpoint;
pub mod clock;
pub mod details;
pub mod navigation;
pub mod timeline;

use crate::{
    data::{cache::SessionCache, limits::Cancellation, openf1::Result},
    domain::{ElapsedMillis, PlaybackFrame},
};
use clock::{PlaybackClock, PlaybackStatus, Speed};
use std::time::Duration;
use timeline::{ReplayPolicy, SessionState, Timeline};

/// Recorded state sampled by timestamp independently from UI redraw cadence.
pub struct RecordedReplay {
    timeline: Timeline,
    state: SessionState,
    frame: PlaybackFrame,
    title: String,
    selected: u32,
    clock: PlaybackClock,
    now: Duration,
    checkpoints: checkpoint::Checkpoints,
    laps: navigation::LapIndex,
    offset_ms: i64,
    conditions: crate::domain::conditions::ConditionsFrame,
    track: crate::track::TrackArchive,
    details: details::DetailsIndex,
}
impl RecordedReplay {
    pub fn from_cache(
        cache: &SessionCache,
        title: String,
        cancel: &Cancellation,
        progress: &dyn Fn(usize, usize),
    ) -> Result<Self> {
        let laps = navigation::LapIndex::from_cache(cache, cancel)?;
        let mut replay = Self::with_navigation(
            Timeline::from_cache(cache, cancel, progress)?,
            title,
            laps,
            cancel,
        )?;
        replay.track = crate::track::TrackArchive::from_cache(cache, &replay.timeline, cancel)?;
        Ok(replay)
    }
    pub fn new(timeline: Timeline, title: String) -> Result<Self> {
        Self::with_navigation(
            timeline,
            title,
            navigation::LapIndex::default(),
            &Cancellation::default(),
        )
    }
    pub fn with_navigation(
        timeline: Timeline,
        title: String,
        laps: navigation::LapIndex,
        cancel: &Cancellation,
    ) -> Result<Self> {
        let checkpoints = checkpoint::Checkpoints::build(&timeline, cancel)?;
        let state = timeline.state_at(ElapsedMillis(0), ReplayPolicy::EventTimeEstimates)?;
        let frame = state.frame(&timeline)?;
        let selected = frame
            .samples()
            .first()
            .ok_or("Recorded roster is empty")?
            .driver
            .number
            .get();
        let conditions =
            timeline.conditions_at(ElapsedMillis(0), ReplayPolicy::EventTimeEstimates)?;
        let clock = PlaybackClock::new(timeline.end());
        let details = details::DetailsIndex::build(&timeline, cancel)?;
        Ok(Self {
            timeline,
            state,
            frame,
            title,
            selected,
            clock,
            now: Duration::ZERO,
            checkpoints,
            laps,
            offset_ms: 0,
            conditions,
            details,
            track: crate::track::TrackArchive::default(),
        })
    }
    pub fn visible_events(&self) -> &[details::ReplayEvent] {
        self.details.visible(
            self.timeline
                .origin()
                .0
                .saturating_add(self.frame.elapsed().0.min(i64::MAX as u64) as i64),
        )
    }
    pub fn telemetry_history(&self) -> Vec<details::HistoryPoint> {
        self.details.history(
            &self.timeline,
            self.selected,
            self.timeline
                .origin()
                .0
                .saturating_add(self.frame.elapsed().0.min(i64::MAX as u64) as i64),
        )
    }
    pub fn comparable_lap(&self) -> Option<crate::domain::timing::LapTiming> {
        self.state
            .comparable_lap(&self.timeline, std::num::NonZeroU32::new(self.selected)?)
    }
    pub fn eligible_reference(&self) -> Option<crate::domain::timing::LapTiming> {
        self.state
            .eligible_reference(&self.timeline, std::num::NonZeroU32::new(self.selected)?)
    }
    pub fn with_track_laps(mut self, laps: Vec<crate::track::LapWindow>) -> Result<Self> {
        self.track =
            crate::track::TrackArchive::build(&self.timeline, laps, &Cancellation::default())?;
        Ok(self)
    }
    pub fn track(&self) -> &crate::track::TrackArchive {
        &self.track
    }
    pub fn conditions(&self) -> &crate::domain::conditions::ConditionsFrame {
        &self.conditions
    }
    pub fn timeline(&self) -> &Timeline {
        &self.timeline
    }
    pub fn frame(&self) -> &PlaybackFrame {
        &self.frame
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn selected(&self) -> u32 {
        self.selected
    }
    pub fn advance_by(&mut self, milliseconds: u64) -> Result<()> {
        self.seek(ElapsedMillis(
            self.frame
                .elapsed()
                .0
                .saturating_add(milliseconds)
                .min(self.timeline.end().0),
        ))
    }
    pub fn seek(&mut self, target: ElapsedMillis) -> Result<()> {
        if target > self.timeline.end() {
            return Err("Time lies outside this recording's coverage".into());
        }
        let state = self.checkpoints.restore(&self.timeline, target)?;
        let frame = state.frame(&self.timeline)?;
        let conditions = self
            .timeline
            .conditions_at(target, ReplayPolicy::EventTimeEstimates)?;
        self.clock.seek(target, self.now)?;
        self.conditions = conditions;
        self.state = state;
        self.frame = frame;
        Ok(())
    }
    pub fn offset_ms(&self) -> i64 {
        self.offset_ms
    }
    pub fn navigate(&mut self, command: navigation::Navigation) -> Result<()> {
        use navigation::Navigation;
        let target = match command {
            Navigation::Time(ms) => ElapsedMillis(ms),
            Navigation::Lap(lap) => self.laps.target(
                self.selected,
                lap,
                self.timeline.origin(),
                self.timeline.end(),
            )?,
            Navigation::Offset(ms) => {
                self.offset_ms = ms;
                return Ok(());
            }
            Navigation::Recording(ms) => {
                let target = i128::from(ms) + i128::from(self.offset_ms);
                if target < 0 || target > i128::from(self.timeline.end().0) {
                    return Err("Recording time + offset lies outside coverage".into());
                }
                ElapsedMillis(target as u64)
            }
        };
        self.seek(target)
    }
    pub fn status(&self) -> PlaybackStatus {
        self.clock.status()
    }
    pub fn speed(&self) -> Speed {
        self.clock.speed()
    }
    pub fn update_at(&mut self, now: Duration) -> Result<bool> {
        self.clock.sync(now)?;
        self.now = now;
        self.project()
    }
    pub fn toggle(&mut self) -> Result<()> {
        self.clock.toggle(self.now)?;
        self.project()?;
        Ok(())
    }
    pub fn pause(&mut self) -> Result<()> {
        self.clock.pause(self.now)?;
        self.project()?;
        Ok(())
    }
    pub fn set_speed(&mut self, speed: Speed) -> Result<()> {
        self.clock.set_speed(speed, self.now)?;
        self.project()?;
        Ok(())
    }
    pub fn restart(&mut self) -> Result<()> {
        self.seek(ElapsedMillis(0))
    }
    fn project(&mut self) -> Result<bool> {
        let target = self.clock.position();
        if target == self.state.elapsed() {
            return Ok(false);
        }
        self.state.advance_to(&self.timeline, target)?;
        self.frame = self.state.frame(&self.timeline)?;
        self.conditions = self
            .timeline
            .conditions_at(target, ReplayPolicy::EventTimeEstimates)?;
        Ok(true)
    }
    pub fn select(&mut self, delta: i32) {
        let current = self
            .frame
            .samples()
            .iter()
            .position(|c| c.driver.number.get() == self.selected)
            .unwrap_or(0);
        let next = (current as i64 + delta as i64)
            .clamp(0, self.frame.samples().len().saturating_sub(1) as i64)
            as usize;
        self.selected = self.frame.samples()[next].driver.number.get();
    }
}
