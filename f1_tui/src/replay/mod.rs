pub mod clock;
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
}
impl RecordedReplay {
    pub fn from_cache(
        cache: &SessionCache,
        title: String,
        cancel: &Cancellation,
        progress: &dyn Fn(usize, usize),
    ) -> Result<Self> {
        Self::new(Timeline::from_cache(cache, cancel, progress)?, title)
    }
    pub fn new(timeline: Timeline, title: String) -> Result<Self> {
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
        self.clock.step(milliseconds, self.now)?;
        self.project()?;
        Ok(())
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
        self.clock.restart(self.now)?;
        self.state = self
            .timeline
            .state_at(ElapsedMillis(0), ReplayPolicy::EventTimeEstimates)?;
        self.frame = self.state.frame(&self.timeline)?;
        Ok(())
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
