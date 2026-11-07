//! Completed-lap state and best-time projection, rebuilt by the replay reducer.
use super::*;

pub(super) struct LapFields {
    pub(super) number: Observation<NonZeroU32>,
    pub(super) duration: Observation<Seconds>,
    pub(super) eligibility: Observation<timing::Eligibility>,
    pub(super) best: Observation<timing::BestLap>,
    pub(super) sectors: [Observation<Seconds>; 3],
}
impl SessionState {
    pub(crate) fn comparable_lap(
        &self,
        timeline: &Timeline,
        driver: NonZeroU32,
    ) -> Option<timing::LapTiming> {
        if !Arc::ptr_eq(&self.token, &timeline.token) {
            return None;
        }
        let (key, index) = self
            .laps
            .range((driver, 0)..=(driver, u32::MAX))
            .next_back()?;
        let e = &timeline.events[*index];
        let Payload::Lap(lap) = &e.payload else {
            return None;
        };
        (!e.ambiguous
            && lap.pit_out == Some(false)
            && lap.duration.is_some()
            && eligibility(lap, self.decisions.get(key).map(|i| &timeline.events[*i]))
                == timing::Eligibility::Eligible)
            .then(|| {
                let mut value = lap.as_ref().clone();
                value.eligibility = timing::Eligibility::Eligible;
                value
            })
    }
    pub(crate) fn eligible_reference(
        &self,
        timeline: &Timeline,
        driver: NonZeroU32,
    ) -> Option<timing::LapTiming> {
        if !Arc::ptr_eq(&self.token, &timeline.token) {
            return None;
        }
        self.laps
            .range((driver, 0)..=(driver, u32::MAX))
            .filter_map(|(key, index)| {
                let event = &timeline.events[*index];
                let Payload::Lap(lap) = &event.payload else {
                    return None;
                };
                if event.ambiguous
                    || eligibility(lap, self.decisions.get(key).map(|i| &timeline.events[*i]))
                        != timing::Eligibility::Eligible
                    || lap.pit_out != Some(false)
                    || lap.duration.is_none()
                {
                    return None;
                }
                let mut lap = lap.as_ref().clone();
                lap.eligibility = timing::Eligibility::Eligible;
                Some(lap)
            })
            .min_by(|a, b| {
                a.duration
                    .unwrap()
                    .get()
                    .total_cmp(&b.duration.unwrap().get())
                    .then_with(|| a.number.cmp(&b.number))
            })
    }

    pub(super) fn completed_laps(
        &self,
        timeline: &Timeline,
        driver: NonZeroU32,
    ) -> Result<LapFields> {
        let latest = self
            .laps
            .range((driver, 0)..=(driver, u32::MAX))
            .next_back()
            .map(|(_, index)| &timeline.events[*index]);
        let lap = latest.and_then(|e| match &e.payload {
            Payload::Lap(lap) => Some(lap.as_ref()),
            _ => None,
        });
        let best = self
            .laps
            .range((driver, 0)..=(driver, u32::MAX))
            .filter_map(|(key, index)| {
                let event = &timeline.events[*index];
                if event.ambiguous {
                    return None;
                }
                let Payload::Lap(lap) = &event.payload else {
                    return None;
                };
                let decision = self.decisions.get(key).map(|i| &timeline.events[*i]);
                let eligibility = eligibility(lap, decision);
                lap.best(eligibility).map(|value| (event, decision, value))
            })
            .min_by(|a, b| {
                a.2.duration
                    .get()
                    .total_cmp(&b.2.duration.get())
                    .then_with(|| a.2.number.cmp(&b.2.number))
            });
        // A deletion can change the minimum by removing a different lap.
        // Its availability still belongs to the derived best's knowledge boundary.
        let frontier = self
            .decisions
            .range((driver, 0)..=(driver, u32::MAX))
            .map(|(_, i)| &timeline.events[*i])
            .max_by_key(|e| availability_key(e.available));
        let best = best.map(|(event, decision, value)| {
            let knowledge = decision
                .into_iter()
                .chain(frontier)
                .max_by_key(|e| availability_key(e.available));
            (event, knowledge, value)
        });
        let personal_best = best_observation(timeline, best)?;
        let decision = lap
            .and_then(|lap| self.decisions.get(&(driver, lap.number.get())))
            .map(|i| &timeline.events[*i]);
        let lap_eligibility = if let Some(decision) = decision {
            observation(
                timeline,
                Some(decision),
                match decision.payload {
                    Payload::LapDecision { eligibility, .. } => Some(eligibility),
                    _ => None,
                },
                "race_control",
            )?
        } else {
            observation(timeline, latest, lap.map(|l| l.eligibility), "laps")?
        };
        Ok(LapFields {
            eligibility: lap_eligibility,
            number: observation(timeline, latest, lap.map(|l| l.number), "laps")?,
            duration: observation(timeline, latest, lap.and_then(|l| l.duration), "laps")?,
            best: personal_best,
            sectors: [
                observation(timeline, latest, lap.and_then(|l| l.sectors[0]), "laps")?,
                observation(timeline, latest, lap.and_then(|l| l.sectors[1]), "laps")?,
                observation(timeline, latest, lap.and_then(|l| l.sectors[2]), "laps")?,
            ],
        })
    }
}

fn best_observation(
    timeline: &Timeline,
    best: Option<(&Event, Option<&Event>, timing::BestLap)>,
) -> Result<Observation<timing::BestLap>> {
    let Some((event, decision, value)) = best else {
        return observation(timeline, None, None, "laps");
    };
    let mut timing = Timing {
        observed_at: Some(event.observed),
        available_at: event.available,
        retrieved_at: Some(timeline.sources[event.source as usize].retrieved_at),
        elapsed: Some(ElapsedMillis(
            event.observed.0.saturating_sub(timeline.origin.0).max(0) as u64,
        )),
    };
    if let Some(decision) = decision {
        let latest = availability_key(event.available)
            .0
            .max(availability_key(decision.available).0);
        timing.available_at = if matches!(event.available, Availability::Estimated(_))
            || matches!(decision.available, Availability::Estimated(_))
        {
            Availability::Estimated(UtcMillis(latest))
        } else {
            Availability::Known(UtcMillis(latest))
        };
        timing.retrieved_at = Some(
            timing
                .retrieved_at
                .unwrap()
                .max(timeline.sources[decision.source as usize].retrieved_at),
        );
    }
    let reason = (value.eligibility == timing::Eligibility::Unknown)
        .then(|| "Provisional recorded best; deletion/eligibility history is incomplete".into());
    Ok(Observation::new(
        Some(value),
        Source {
            session: timeline.session.clone(),
            channel: if decision.is_some() {
                "laps+race_control"
            } else {
                "laps"
            }
            .into(),
            origin: Origin::Recorded,
        },
        timing,
        Quality::Derived,
        reason,
    )?)
}

fn eligibility(lap: &timing::LapTiming, decision: Option<&Event>) -> timing::Eligibility {
    match decision {
        Some(e) if e.ambiguous => timing::Eligibility::Ineligible,
        Some(Event {
            payload: Payload::LapDecision { eligibility, .. },
            ..
        }) => *eligibility,
        Some(_) => timing::Eligibility::Ineligible,
        None => lap.eligibility,
    }
}
