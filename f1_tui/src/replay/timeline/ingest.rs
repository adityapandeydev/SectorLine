//! One-pass normalization of independently validated chunks. No partial replay publication.
use super::*;
use crate::data::openf1::ValidatedChunk;
pub(crate) const CHANNELS: [Channel; 6] = [
    Channel::Position,
    Channel::CarData,
    Channel::Location,
    Channel::Intervals,
    Channel::Laps,
    Channel::RaceControl,
];
pub(crate) struct CoreBuilder {
    events: Vec<Event>,
    sources: Vec<ChunkMetadata>,
    drivers: Vec<RosterDriver>,
}
impl CoreBuilder {
    pub(crate) fn new(cache: &SessionCache) -> Result<Self> {
        let count = cache.record_count(&CHANNELS);
        let bytes = count
            .checked_mul(std::mem::size_of::<Event>())
            .ok_or("Timeline size overflow")?;
        if bytes > 192 * 1024 * 1024 {
            return Err("Recorded core timeline exceeds the 192 MiB event budget; request a shorter coverage window".into());
        }
        let mut events = Vec::new();
        events
            .try_reserve_exact(count)
            .map_err(|_| "Cannot allocate the recorded timeline within available memory")?;
        Ok(Self {
            events,
            sources: Vec::new(),
            drivers: Vec::new(),
        })
    }
    pub(crate) fn visit(&mut self, chunk: &ValidatedChunk) -> Result<()> {
        if chunk.channel() == Channel::Drivers {
            self.drivers = normalize::roster(chunk)?;
        } else if CHANNELS.contains(&chunk.channel()) && chunk.metadata().status == 200 {
            let source = self.sources.len().try_into()?;
            self.sources.push(chunk.metadata());
            if chunk.channel() == Channel::RaceControl {
                normalize::append_control(chunk, source, &self.drivers, &mut self.events)?;
            } else {
                normalize::append(chunk, source, &mut self.events)?;
            }
        }
        Ok(())
    }
    pub(crate) fn finish(self, cache: &SessionCache, cancel: &Cancellation) -> Result<Timeline> {
        cancel.check()?;
        let origin = self
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e.payload,
                    Payload::Position(_)
                        | Payload::Telemetry(_)
                        | Payload::Location(_)
                        | Payload::Intervals { .. }
                )
            })
            .map(|e| e.observed)
            .min()
            .ok_or("Recording has no timestamped core observations")?;
        let mut t = Timeline::new(
            SessionId::new(
                Provider::OpenF1,
                cache.spec().request()?.session_key().to_string(),
            )?,
            origin,
            self.drivers,
            self.sources,
            self.events,
        )?;
        t.conditions = conditions::ConditionsArchive::from_cache(cache, cancel)?;
        cancel.check()?;
        Ok(t)
    }
}
