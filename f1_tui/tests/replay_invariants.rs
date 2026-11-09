//! Seeded permutation and reverse-seek checks on attributed sparse native excerpts.
use sectorline::{
    data::{
        cache::{CacheSpec, CacheWriter},
        limits::Cancellation,
        openf1::{Channel, ChunkMetadata, Window},
    },
    domain::{capabilities::SessionContext, normalize::utc_millis_ceiling, *},
    replay::{RecordedReplay, timeline::ReplayPolicy},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf, time::Duration};
fn shuffle<T>(rows: &mut [T], seed: &mut u64) {
    for i in (1..rows.len()).rev() {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        rows.swap(i, (*seed as usize) % (i + 1));
    }
}
struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let p = self.0.canonicalize().unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .canonicalize()
            .unwrap();
        assert!(p.starts_with(&base) && p != base);
        std::fs::remove_dir_all(p).unwrap();
    }
}
fn native(entry: &Value, seed: &mut u64) -> (Root, RecordedReplay) {
    let s = &entry["session"];
    let key = s["session_key"].as_u64().unwrap() as u32;
    let meeting = s["meeting_key"].as_u64().unwrap() as u32;
    let start = utc_millis_ceiling(s["date_start"].as_str().unwrap())
        .unwrap()
        .0
        - 3600000;
    let end = utc_millis_ceiling(s["date_end"].as_str().unwrap())
        .unwrap()
        .0
        + 3600000;
    let root = Root(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .join(format!("t21-native-{key}-{}-{}", std::process::id(), *seed)),
    );
    std::fs::create_dir(&root.0).unwrap();
    let mut identities = BTreeSet::new();
    let mut channels = std::collections::BTreeMap::new();
    for channel in Channel::ALL {
        if channel == Channel::Drivers {
            continue;
        }
        let c = &entry["channels"][channel.endpoint()];
        let mut rows = c
            .get("samples")
            .or_else(|| c.get("examples"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for r in &rows {
            if let Some(n) = r["driver_number"].as_u64() {
                identities.insert(n);
            }
        }
        // Duplicate a raw report and permute source order; canonical replay must not change.
        if let Some(r) = rows.first().cloned() {
            rows.push(r);
        }
        shuffle(&mut rows, seed);
        channels.insert(channel, rows);
    }
    // Driver identities are the observed numbers; labels remain unknown fixture placeholders.
    let drivers=identities.iter().map(|n|json!({"session_key":key,"meeting_key":meeting,"driver_number":n,"full_name":"--","name_acronym":"--","team_name":"--"})).collect::<Vec<_>>();
    let spec = CacheSpec::new(
        key,
        meeting,
        2024,
        (
            s["session_name"].as_str().unwrap(),
            s["session_type"].as_str().unwrap(),
        ),
        Window::new(UtcMillis(start), UtcMillis(end)).unwrap(),
        Duration::from_secs(3600),
        Channel::ALL.to_vec(),
    )
    .unwrap();
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    for channel in Channel::ALL {
        let rows = if channel == Channel::Drivers {
            &drivers
        } else {
            &channels[&channel]
        };
        let windows = if matches!(
            channel,
            Channel::CarData | Channel::Location | Channel::Intervals
        ) {
            (start..end)
                .step_by(3600000)
                .map(|t| {
                    Some(Window::new(UtcMillis(t), UtcMillis((t + 3600000).min(end))).unwrap())
                })
                .collect::<Vec<_>>()
        } else {
            vec![None]
        };
        for window in windows {
            let stamp = |t: UtcMillis| {
                chrono::DateTime::from_timestamp_millis(t.0)
                    .unwrap()
                    .format("%Y-%m-%dT%H:%M:%S%.3f")
                    .to_string()
            };
            let mut url = format!(
                "https://api.openf1.org/v1/{}?session_key={key}",
                channel.endpoint()
            );
            if let Some(w) = window {
                url.push_str(&format!(
                    "&date>={}&date<{}",
                    stamp(w.start()),
                    stamp(w.end())
                ));
            }
            let selected = rows
                .iter()
                .filter(|r| {
                    window.is_none_or(|w| {
                        let t = utc_millis_ceiling(r["date"].as_str().unwrap()).unwrap();
                        t >= w.start() && t < w.end()
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            writer
                .import(
                    ChunkMetadata {
                        channel,
                        session_key: key,
                        meeting_key: meeting,
                        window,
                        source_url: url,
                        retrieved_at: UtcMillis(1795000000000),
                        status: 200,
                        attempts: 1,
                    },
                    &serde_json::to_vec(&selected).unwrap(),
                )
                .unwrap();
        }
    }
    let cache = writer.finish().unwrap();
    let context = SessionContext::from_names(
        SessionId::new(Provider::OpenF1, key.to_string()).unwrap(),
        s["session_name"].as_str().unwrap(),
        s["session_type"].as_str().unwrap(),
    );
    let replay = RecordedReplay::from_cache(
        &cache,
        format!("ATTRIBUTED PARTIAL EXCERPT {key}"),
        &Cancellation::default(),
        &|_, _| {},
    )
    .unwrap()
    .with_context(context)
    .unwrap();
    (root, replay)
}
fn snapshot(r: &RecordedReplay) -> Value {
    json!({"frame":r.frame(),"conditions":r.conditions(),"bests":r.timing_bests(),"eligible_bests":r.eligible_bests(),"phase":[r.phase().part.map(u32::from),Some(u32::from(r.phase().closed))],"insights":format!("{:?}",r.insights()),"events":r.visible_events().iter().map(|e|(&e.summary,e.observed,e.available)).collect::<Vec<_>>()})
}
#[test]
fn replay_invariants_native_release_excerpts_match_after_permutation_and_random_seeks() {
    let raw: Value = serde_json::from_str(include_str!("fixtures/manifest.json")).unwrap();
    for entry in raw["sessions"].as_array().unwrap() {
        let (_a, mut a) = native(entry, &mut 0x2101);
        let (_b, mut b) = native(entry, &mut 0x2102);
        assert_eq!(a.timeline().end(), b.timeline().end());
        let end = a.timeline().end().0;
        let mut seed = 0x21_u64;
        let mut targets = vec![0, end];
        for _ in 0..100 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            targets.push(seed % (end + 1));
        }
        for at in targets {
            a.seek(ElapsedMillis(at)).unwrap();
            b.seek(ElapsedMillis(at)).unwrap();
            assert_eq!(
                snapshot(&a),
                snapshot(&b),
                "session{} at{at}",
                entry["session"]["session_key"]
            );
            let oracle = a
                .timeline()
                .state_at(ElapsedMillis(at), ReplayPolicy::EventTimeEstimates)
                .unwrap()
                .frame(a.timeline())
                .unwrap();
            assert_eq!(a.frame(), &oracle);
            let now = UtcMillis(a.timeline().origin().0 + at as i64);
            for car in a.frame().samples() {
                assert_eq!(car.driver.session, *a.timeline().session());
                // Independent raw-excerpt oracle: native snapshot telemetry is event-time
                // estimated, field quarantine excludes out-of-domain values, never zero-fill.
                let data = &entry["channels"]["car_data"];
                let source_rows = data
                    .get("samples")
                    .or_else(|| data.get("examples"))
                    .and_then(Value::as_array)
                    .unwrap();
                let expected = source_rows
                    .iter()
                    .filter(|r| {
                        r["driver_number"].as_u64() == Some(u64::from(car.driver.number.get()))
                    })
                    .filter_map(|r| utc_millis_ceiling(r["date"].as_str()?).ok().map(|t| (t, r)))
                    .filter(|(t, _)| *t <= now)
                    .max_by_key(|(t, _)| *t)
                    .and_then(|(_, r)| r["speed"].as_f64())
                    .and_then(|n| SpeedKmh::new(n).ok())
                    .map(|n| n.get());
                assert_eq!(
                    car.speed_kmh.value().map(|n| n.get()),
                    expected,
                    "Raw speed oracle mismatch at{now:?}"
                );
                for timing in [
                    car.speed_kmh.timing(),
                    car.gap_to_leader.timing(),
                    car.lap_time.timing(),
                ] {
                    if let Availability::Known(t) | Availability::Estimated(t) = timing.available_at
                    {
                        assert!(t <= now);
                    }
                }
            }
            let positions = a
                .frame()
                .samples()
                .iter()
                .filter_map(|c| c.position.value().map(|p| p.get()))
                .collect::<Vec<_>>();
            assert!(positions.windows(2).all(|p| p[0] <= p[1]));
            for i in a.insights() {
                assert!(i.window_start <= i.observed && i.observed <= now);
                assert!(
                    matches!(i.available,Availability::Known(t)|Availability::Estimated(t) if t<=now)
                );
            }
        }
    }
}

#[test]
fn replay_invariants_delayed_conflicting_and_unknown_events_survive_seeded_ordering() {
    use sectorline::domain::normalize::{Event, Payload, RosterDriver, TelemetryValues};
    use sectorline::replay::timeline::Timeline;
    let session = SessionId::new(Provider::OpenF1, "9590").unwrap();
    let sources = [Channel::CarData, Channel::Position]
        .map(|channel| ChunkMetadata {
            channel,
            session_key: 9590,
            meeting_key: 1244,
            window: None,
            source_url: format!("synthetic://{}", channel.endpoint()),
            retrieved_at: UtcMillis(900000),
            status: 200,
            attempts: 1,
        })
        .to_vec();
    let mut events = Vec::new();
    for driver in 1..=4 {
        for n in 0..100_i64 {
            let at = n * 997;
            let availability = if n % 11 == 0 {
                Availability::Unknown
            } else if n % 3 == 0 {
                Availability::Known(UtcMillis(at + 5000))
            } else {
                Availability::Estimated(UtcMillis(at))
            };
            let payload = Payload::Telemetry(TelemetryValues {
                speed: if n % 13 == 0 {
                    None
                } else {
                    Some(SpeedKmh::new((n * 3) as f64).unwrap())
                },
                ..Default::default()
            });
            let e = Event::new(driver, UtcMillis(at), availability, 0, payload).unwrap();
            events.push(e.clone());
            if n % 7 == 0 {
                events.push(e);
            }
            if n % 17 == 0 {
                events.push(
                    Event::new(
                        driver,
                        UtcMillis(at),
                        availability,
                        0,
                        Payload::Telemetry(TelemetryValues {
                            speed: Some(SpeedKmh::new(333.0).unwrap()),
                            ..Default::default()
                        }),
                    )
                    .unwrap(),
                );
            }
            events.push(
                Event::new(
                    driver,
                    UtcMillis(at),
                    Availability::Known(UtcMillis(at)),
                    1,
                    Payload::Position(std::num::NonZeroU32::new(driver)),
                )
                .unwrap(),
            );
        }
    }
    let build = |events| {
        Timeline::new(
            session.clone(),
            UtcMillis(0),
            (1..=4)
                .map(|n| RosterDriver::new(n, "TST", "SYNTHETIC").unwrap())
                .collect(),
            sources.clone(),
            events,
        )
        .unwrap()
    };
    let reference = build(events.clone());
    let end = reference.end().0;
    let mut seed = 0x21c0de;
    for _ in 0..12 {
        shuffle(&mut events, &mut seed);
        let t = build(events.clone());
        assert!(t.report().ambiguous_groups > 0);
        let mut r = RecordedReplay::new(t, "SYNTHETIC ORDERING".into()).unwrap();
        for _ in 0..64 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let at = ElapsedMillis(seed % (end + 1));
            r.seek(at).unwrap();
            let expected = reference
                .state_at(at, ReplayPolicy::EventTimeEstimates)
                .unwrap()
                .frame(&reference)
                .unwrap();
            assert_eq!(r.frame(), &expected);
        }
    }
}
