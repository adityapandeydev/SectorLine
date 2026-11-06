pub mod checkpoint;
pub mod clock;
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
}
impl RecordedReplay {
    pub fn from_cache(
        cache: &SessionCache,
        title: String,
        cancel: &Cancellation,
        progress: &dyn Fn(usize, usize),
    ) -> Result<Self> {
        let laps = navigation::LapIndex::from_cache(cache, cancel)?;
        Self::with_navigation(
            Timeline::from_cache(cache, cancel, progress)?,
            title,
            laps,
            cancel,
        )
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
        let clock = PlaybackClock::new(timeline.end());
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
        })
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
        self.clock.seek(target, self.now)?;
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
