pub mod timeline;

use crate::{
    data::{cache::SessionCache, limits::Cancellation, openf1::Result},
    domain::{ElapsedMillis, PlaybackFrame},
};
use timeline::{ReplayPolicy, SessionState, Timeline};

/// T08 manual inspection; the monotonic playback clock follows in T09.
pub struct RecordedReplay {
    timeline: Timeline,
    state: SessionState,
    frame: PlaybackFrame,
    title: String,
    selected: u32,
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
        Ok(Self {
            timeline,
            state,
            frame,
            title,
            selected,
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
        let target = ElapsedMillis(
            self.state
                .elapsed()
                .0
                .saturating_add(milliseconds)
                .min(self.timeline.end().0),
        );
        self.state.advance_to(&self.timeline, target)?;
        self.frame = self.state.frame(&self.timeline)?;
        Ok(())
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
