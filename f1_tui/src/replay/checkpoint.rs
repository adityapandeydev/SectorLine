//! Bounded snapshots of the complete reducer state, never of rendered rows.
use super::timeline::{ReplayPolicy, SessionState, Timeline};
use crate::{
    data::{limits::Cancellation, openf1::Result},
    domain::ElapsedMillis,
};

pub struct Checkpoints {
    states: Vec<SessionState>,
}
impl Checkpoints {
    pub fn build(timeline: &Timeline, cancel: &Cancellation) -> Result<Self> {
        cancel.check()?;
        // At most 512 snapshots; long recordings widen the interval.
        let interval = 30_000_u64.max(timeline.end().0 / 511 + 1);
        let mut state = timeline.state_at(ElapsedMillis(0), ReplayPolicy::EventTimeEstimates)?;
        let mut states = vec![state.clone()];
        let mut time = interval;
        while time < timeline.end().0 {
            cancel.check()?;
            state.advance_to(timeline, ElapsedMillis(time))?;
            states.push(state.clone());
            time = time.saturating_add(interval);
        }
        cancel.check()?;
        Ok(Self { states })
    }
    pub fn restore(&self, timeline: &Timeline, target: ElapsedMillis) -> Result<SessionState> {
        if target > timeline.end() {
            return Err("Seek exceeds recorded coverage".into());
        }
        let index = self
            .states
            .partition_point(|s| s.elapsed() <= target)
            .saturating_sub(1);
        let mut state = self.states[index].clone();
        state.advance_to(timeline, target)?;
        Ok(state)
    }
    pub fn len(&self) -> usize {
        self.states.len()
    }
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}
