//! Pure target-time projection; all disk access stays in archive normalization.
use super::*;

fn permitted(available: Availability, now: UtcMillis, estimates: bool) -> bool {
    match available {
        Availability::Known(t) => t <= now,
        Availability::Estimated(t) => estimates && t <= now,
        Availability::Unknown => false,
    }
}
fn availability(available: Availability) -> i64 {
    match available {
        Availability::Known(t) | Availability::Estimated(t) => t.0,
        Availability::Unknown => i64::MAX,
    }
}
fn latest<T: PartialEq>(
    rows: &[Reading<T>],
    now: UtcMillis,
    estimates: bool,
) -> (Option<&Reading<T>>, bool) {
    let row = rows
        .iter()
        .filter(|r| permitted(r.available, now, estimates))
        .max_by_key(|r| (r.at, availability(r.available), r.retrieved));
    let conflict = row.is_some_and(|r| {
        rows.iter().any(|v| {
            permitted(v.available, now, estimates)
                && v.at == r.at
                && availability(v.available) == availability(r.available)
                && v.retrieved == r.retrieved
                && v.value != r.value
        })
    });
    (row, conflict)
}
fn observation<T: Clone, U>(
    context: (&SessionId, UtcMillis, UtcMillis),
    channel: &str,
    reading: Option<&Reading<U>>,
    value: Option<T>,
    conflict: bool,
    derived: bool,
) -> Result<Observation<T>> {
    let (session, origin, now) = context;
    let value = if conflict { None } else { value };
    let timing = reading
        .map(|r| Timing {
            observed_at: Some(r.at),
            available_at: r.available,
            retrieved_at: Some(r.retrieved),
            elapsed: Some(ElapsedMillis(r.at.0.saturating_sub(origin.0).max(0) as u64)),
        })
        .unwrap_or_default();
    let reason = if conflict {
        Some("Conflicting same-time condition evidence".into())
    } else if value.is_none() {
        Some("Condition evidence unavailable or invalid".into())
    } else if derived {
        Some("Derived from archived evidence; original publication time unknown".into())
    } else {
        None
    };
    let quality = if value.is_none() {
        Quality::Unavailable
    } else if derived {
        Quality::Derived
    } else {
        Quality::Observed
    };
    let value = Observation::new(
        value,
        Source {
            session: session.clone(),
            channel: channel.into(),
            origin: Origin::Recorded,
        },
        timing,
        quality,
        reason,
    )?;
    Ok(quality::at_replay_time(&value, now, channel)?)
}
impl ConditionsArchive {
    pub fn frame(
        &self,
        session: &SessionId,
        origin: UtcMillis,
        elapsed: ElapsedMillis,
        estimates: bool,
    ) -> Result<ConditionsFrame> {
        let now = UtcMillis(
            origin
                .0
                .checked_add(i64::try_from(elapsed.0)?)
                .ok_or("Condition time overflow")?,
        );
        let (weather, ambiguous) = latest(&self.weather, now, estimates);
        let w = weather.and_then(|r| r.value.as_ref());
        let weather_value = |value: Option<f64>| {
            observation(
                (session, origin, now),
                "weather",
                weather,
                value.filter(|v| v.is_finite()),
                ambiguous,
                false,
            )
        };
        let rain = observation(
            (session, origin, now),
            "weather",
            weather,
            w.and_then(|v| v.rain),
            ambiguous,
            false,
        )?;
        let mut scopes = BTreeSet::new();
        for row in &self.control {
            if permitted(row.available, now, estimates)
                && let Some(c) = &row.value
                && matches!(c.kind, ControlKind::Flag(_))
            {
                scopes.insert(c.scope.clone());
            }
        }
        let control_value = |scope: &Scope, kind: u8| {
            let rows = self
                .control
                .iter()
                .filter(|r| {
                    r.value.as_ref().is_some_and(|v| {
                        &v.scope == scope
                            && matches!(
                                (&v.kind, kind),
                                (ControlKind::Flag(_), 0)
                                    | (ControlKind::SafetyCar(_), 1)
                                    | (ControlKind::Session(_), 2)
                            )
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            let (row, conflict) = latest(&rows, now, estimates);
            (row.cloned(), conflict)
        };
        let flag = |scope: &Scope| -> Result<Observation<Flag>> {
            let (row, conflict) = control_value(scope, 0);
            let value = row.as_ref().and_then(|r| r.value.as_ref()).and_then(|v| {
                if let ControlKind::Flag(f) = &v.kind {
                    if *f == Flag::Unavailable {
                        None
                    } else {
                        Some(f.clone())
                    }
                } else {
                    None
                }
            });
            observation(
                (session, origin, now),
                "race_control",
                row.as_ref(),
                value,
                conflict,
                false,
            )
        };
        let track_flag = flag(&Scope::Track)?;
        let local_flags = scopes
            .into_iter()
            .filter(|s| *s != Scope::Track)
            .map(|scope| Ok((scope.clone(), flag(&scope)?)))
            .collect::<Result<Vec<_>>>()?;
        let (sc, conflict) = control_value(&Scope::Track, 1);
        let safety_car = observation(
            (session, origin, now),
            "race_control",
            sc.as_ref(),
            sc.as_ref().and_then(|r| r.value.as_ref()).and_then(|v| {
                if let ControlKind::SafetyCar(s) = &v.kind {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            conflict,
            false,
        )?;
        let (status, conflict) = control_value(&Scope::Track, 2);
        let session_status = observation(
            (session, origin, now),
            "race_control",
            status.as_ref(),
            status
                .as_ref()
                .and_then(|r| r.value.as_ref())
                .and_then(|v| {
                    if let ControlKind::Session(s) = &v.kind {
                        Some(s.clone())
                    } else {
                        None
                    }
                }),
            conflict,
            false,
        )?;
        let mut drivers = BTreeMap::new();
        for driver in self
            .stints
            .keys()
            .chain(self.pits.keys())
            .copied()
            .collect::<BTreeSet<_>>()
        {
            let (stint, conflict) = latest(
                self.stints.get(&driver).map(Vec::as_slice).unwrap_or(&[]),
                now,
                estimates,
            );
            let tyre = stint.and_then(|r| r.value.as_ref()).map(|s| {
                let completed = self
                    .completed_laps
                    .get(&driver)
                    .into_iter()
                    .flatten()
                    .filter(|(lap, time)| *lap >= s.lap_start && *time <= now)
                    .map(|(lap, _)| *lap)
                    .collect::<BTreeSet<_>>();
                let contiguous = completed
                    .iter()
                    .copied()
                    .eq(s.lap_start..s.lap_start.saturating_add(completed.len() as u32));
                Tyre {
                    stint: s.number,
                    compound: s.compound.clone(),
                    completed_age: if contiguous {
                        s.age_at_start
                            .and_then(|age| age.checked_add(completed.len() as u32))
                    } else {
                        None
                    },
                }
            });
            let mut evidence = stint.cloned();
            if let Some(evidence) = &mut evidence
                && let Some(stint) = &evidence.value
                && let Some(completed) = self
                    .completed_laps
                    .get(&driver)
                    .into_iter()
                    .flatten()
                    .filter(|(lap, time)| *lap >= stint.lap_start && *time <= now)
                    .map(|(_, time)| *time)
                    .max()
                && completed > evidence.at
            {
                evidence.at = completed;
                evidence.available = Availability::Estimated(UtcMillis(
                    availability(evidence.available).max(completed.0),
                ));
            }
            let tyre = observation(
                (session, origin, now),
                "stints",
                evidence.as_ref(),
                tyre,
                conflict,
                true,
            )?;
            let (pit, conflict) = latest(
                self.pits.get(&driver).map(Vec::as_slice).unwrap_or(&[]),
                now,
                estimates,
            );
            let pit_value = pit.and_then(|r| r.value.as_ref()).map(|p| Pit {
                phase: PitPhase::RecordedVisit,
                lap: p.lap,
                lane_seconds: p.lane_seconds,
                stop_seconds: p.stop_seconds,
            });
            // A visit is historical evidence, not a persistent claim of being in the pit.
            let pit = observation(
                (session, origin, now),
                "pit",
                pit,
                pit_value,
                conflict,
                false,
            )?;
            drivers.insert(driver, DriverConditions { tyre, pit });
        }
        Ok(ConditionsFrame {
            air_c: weather_value(w.and_then(|v| v.air_c))?,
            track_c: weather_value(w.and_then(|v| v.track_c))?,
            humidity: weather_value(w.and_then(|v| v.humidity))?,
            pressure_mbar: weather_value(w.and_then(|v| v.pressure_mbar))?,
            wind_mps: weather_value(w.and_then(|v| v.wind_mps))?,
            wind_degrees: weather_value(w.and_then(|v| v.wind_degrees))?,
            rain,
            track_flag,
            local_flags,
            safety_car,
            session_status,
            drivers,
        })
    }
}
