use sectorline::domain::{conditions::*, quality::*, stints::*, *};
fn id() -> SessionId {
    SessionId::new(Provider::OpenF1, "9590").unwrap()
}
fn reading<T>(at: i64, value: T) -> Reading<T> {
    Reading::new(
        UtcMillis(at),
        Availability::Estimated(UtcMillis(at)),
        UtcMillis(900_000),
        Some(value),
    )
    .unwrap()
}
#[test]
fn conditions_weather_obeys_availability_and_stales_in_replay_time() {
    let mut archive = ConditionsArchive::default();
    archive.weather.push(
        Reading::new(
            UtcMillis(1000),
            Availability::Estimated(UtcMillis(5000)),
            UtcMillis(900_000),
            Some(Weather {
                air_c: Some(30.0),
                track_c: None,
                rain: Some(false),
                ..Default::default()
            }),
        )
        .unwrap(),
    );
    assert!(
        archive
            .frame(&id(), UtcMillis(0), ElapsedMillis(4999), true)
            .unwrap()
            .air_c
            .value()
            .is_none()
    );
    let frame = archive
        .frame(&id(), UtcMillis(0), ElapsedMillis(5000), true)
        .unwrap();
    assert_eq!(frame.air_c.value(), Some(&30.0));
    assert!(frame.track_c.value().is_none());
    assert_eq!(frame.rain.value(), Some(&false));
    let stale = archive
        .frame(&id(), UtcMillis(0), ElapsedMillis(121_001), true)
        .unwrap();
    assert_eq!(stale.air_c.quality(), Quality::Stale);
    assert_eq!(stale.air_c.value(), Some(&30.0));
    assert!(
        archive
            .frame(&id(), UtcMillis(0), ElapsedMillis(5000), false)
            .unwrap()
            .air_c
            .value()
            .is_none()
    );
}
#[test]
fn conditions_scopes_red_restart_and_safety_car_are_independent() {
    let mut archive = ConditionsArchive::default();
    archive.control.push(reading(
        0,
        Control {
            scope: Scope::Track,
            kind: ControlKind::Flag(Flag::Red),
        },
    ));
    archive.control.push(reading(
        1000,
        Control {
            scope: Scope::Sector(2),
            kind: ControlKind::Flag(Flag::Yellow),
        },
    ));
    archive.control.push(reading(
        2000,
        Control {
            scope: Scope::Track,
            kind: ControlKind::SafetyCar(SafetyCar::Deployed),
        },
    ));
    archive.control.push(reading(
        3000,
        Control {
            scope: Scope::Track,
            kind: ControlKind::Session(SessionStatus::Started),
        },
    ));
    archive.control.push(reading(
        4000,
        Control {
            scope: Scope::Track,
            kind: ControlKind::Flag(Flag::Green),
        },
    ));
    let f = archive
        .frame(&id(), UtcMillis(0), ElapsedMillis(3500), true)
        .unwrap();
    assert_eq!(f.track_flag.value(), Some(&Flag::Red));
    assert_eq!(f.safety_car.value(), Some(&SafetyCar::Deployed));
    assert_eq!(f.local_flags.len(), 1);
    let f = archive
        .frame(&id(), UtcMillis(0), ElapsedMillis(4000), true)
        .unwrap();
    assert_eq!(f.track_flag.value(), Some(&Flag::Green));
    assert_eq!(f.local_flags[0].1.value(), Some(&Flag::Yellow));
    assert_eq!(
        archive
            .frame(&id(), UtcMillis(0), ElapsedMillis(131_001), true)
            .unwrap()
            .track_flag
            .quality(),
        Quality::Stale
    );
    assert_eq!(
        archive
            .frame(&id(), UtcMillis(0), ElapsedMillis(500), true)
            .unwrap()
            .track_flag
            .value(),
        Some(&Flag::Red)
    );
}
#[test]
fn conditions_pit_record_does_not_fabricate_entry_stationary_or_exit() {
    let mut archive = ConditionsArchive::default();
    archive.pits.entry(4).or_default().push(reading(
        20_000,
        PitRecord {
            lap: Some(2),
            lane_seconds: Some(22.0),
            stop_seconds: None,
        },
    ));
    assert!(
        archive
            .frame(&id(), UtcMillis(0), ElapsedMillis(19_999), true)
            .unwrap()
            .drivers
            .get(&4)
            .unwrap()
            .pit
            .value()
            .is_none()
    );
    let f = archive
        .frame(&id(), UtcMillis(0), ElapsedMillis(20_000), true)
        .unwrap();
    let pit = f.drivers[&4].pit.value().unwrap();
    assert_eq!(pit.lane_seconds, Some(22.0));
    assert_eq!(pit.phase, PitPhase::RecordedVisit);
    assert!(pit.stop_seconds.is_none());
}
#[test]
fn conditions_tyre_age_counts_evidenced_completed_laps_and_hides_future_stint_end() {
    let mut archive = ConditionsArchive::default();
    archive.stints.entry(4).or_default().push(reading(
        1000,
        Stint {
            number: 1,
            lap_start: 1,
            compound: Some("MEDIUM".into()),
            age_at_start: Some(3),
        },
    ));
    archive
        .completed_laps
        .entry(4)
        .or_default()
        .extend([(1, UtcMillis(60_000)), (2, UtcMillis(120_000))]);
    let f = archive
        .frame(&id(), UtcMillis(0), ElapsedMillis(1000), true)
        .unwrap();
    assert_eq!(f.drivers[&4].tyre.value().unwrap().completed_age, Some(3));
    let f = archive
        .frame(&id(), UtcMillis(0), ElapsedMillis(60_000), true)
        .unwrap();
    assert_eq!(f.drivers[&4].tyre.value().unwrap().completed_age, Some(4));
    archive.stints.get_mut(&4).unwrap().push(reading(
        80_000,
        Stint {
            number: 2,
            lap_start: 2,
            compound: None,
            age_at_start: None,
        },
    ));
    let f = archive
        .frame(&id(), UtcMillis(0), ElapsedMillis(90_000), true)
        .unwrap();
    assert!(f.drivers[&4].tyre.value().unwrap().compound.is_none());
    assert!(f.drivers[&4].tyre.value().unwrap().completed_age.is_none());
}
#[test]
fn conditions_quality_thresholds_and_missing_values_never_default_to_zero() {
    assert_eq!(stale_after("car_data"), Some(2000));
    assert_eq!(stale_after("weather"), Some(120_000));
    assert_eq!(stale_after("laps"), None);
    let f = ConditionsArchive::default()
        .frame(&id(), UtcMillis(0), ElapsedMillis(999_999), true)
        .unwrap();
    assert!(f.rain.value().is_none());
    assert!(f.track_flag.value().is_none());
    assert!(f.safety_car.value().is_none());
}

#[test]
fn conditions_conflicts_null_updates_and_clear_scope_do_not_turn_green() {
    let mut archive = ConditionsArchive::default();
    archive.weather.push(reading(
        0,
        Weather {
            air_c: Some(30.0),
            ..Default::default()
        },
    ));
    archive.weather.push(reading(
        0,
        Weather {
            air_c: Some(31.0),
            ..Default::default()
        },
    ));
    let f = archive
        .frame(&id(), UtcMillis(0), ElapsedMillis(0), true)
        .unwrap();
    assert!(f.air_c.value().is_none());
    assert!(f.air_c.reason().unwrap().contains("Conflicting"));
    archive.weather.push(reading(1000, Weather::default()));
    assert!(
        archive
            .frame(&id(), UtcMillis(0), ElapsedMillis(1000), true)
            .unwrap()
            .air_c
            .value()
            .is_none()
    );
    archive.control.push(reading(
        0,
        Control {
            scope: Scope::Track,
            kind: ControlKind::Flag(Flag::Green),
        },
    ));
    archive.control.push(reading(
        1000,
        parse_control(&serde_json::json!({"category":"Flag","scope":"Track","flag":null})).unwrap(),
    ));
    assert!(
        archive
            .frame(&id(), UtcMillis(0), ElapsedMillis(1000), true)
            .unwrap()
            .track_flag
            .value()
            .is_none(),
        "Null flag must invalidate a previously held green"
    );
    let invalid =
        serde_json::json!({"category":"Flag","scope":"Sector","flag":"YELLOW","sector":null});
    assert!(parse_control(&invalid).is_none());
    let local = serde_json::json!({"category":"Flag","scope":"Sector","flag":"CLEAR","sector":2});
    assert_eq!(parse_control(&local).unwrap().scope, Scope::Sector(2));
    let sc = serde_json::json!({"category":"SafetyCar","scope":null,"message":"VIRTUAL SAFETY CAR ENDING"});
    assert_eq!(
        parse_control(&sc).unwrap().kind,
        ControlKind::SafetyCar(SafetyCar::VirtualEnding)
    );
    let status =
        serde_json::json!({"category":"SessionStatus","scope":null,"message":"SESSION ABORTED"});
    assert_eq!(
        parse_control(&status).unwrap().kind,
        ControlKind::Session(SessionStatus::Suspended)
    );
    let unknown = serde_json::json!({"category":"Flag","scope":"Track","flag":"NEW_CODE"});
    assert!(matches!(
        parse_control(&unknown).unwrap().kind,
        ControlKind::Flag(Flag::Unknown(_))
    ));
}
#[test]
fn conditions_cached_channels_seek_staleness_and_ui_match_independent_boundaries() {
    use sectorline::{
        data::{
            cache::{CacheSpec, CacheWriter},
            limits::Cancellation,
            openf1::{Channel, ChunkMetadata, Window},
        },
        replay::{RecordedReplay, timeline::ReplayPolicy},
    };
    use serde_json::{Value, json};
    let base = 1_725_195_600_000_i64; // 2024-09-01 13:00 UTC; synthetic session, not actual race data.
    let utc = |ms: i64| {
        chrono::DateTime::from_timestamp_millis(base + ms)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    let row = |value: Value| {
        let mut value = value;
        value["session_key"] = json!(9590);
        value["meeting_key"] = json!(1244);
        value
    };
    let data = std::collections::BTreeMap::from([
        (
            Channel::Drivers,
            vec![row(
                json!({"driver_number":4,"full_name":"Synthetic fixture driver","name_acronym":"TST","team_name":"Fixture"}),
            )],
        ),
        (
            Channel::Position,
            vec![
                row(json!({"driver_number":4,"date":utc(0),"position":1})),
                row(json!({"driver_number":4,"date":utc(480_000),"position":1})),
            ],
        ),
        (
            Channel::Laps,
            vec![
                row(
                    json!({"driver_number":4,"lap_number":1,"date_start":utc(0),"lap_duration":60.0,"duration_sector_1":20.0,"duration_sector_2":20.0,"duration_sector_3":20.0,"is_pit_out_lap":false}),
                ),
                row(
                    json!({"driver_number":4,"lap_number":2,"date_start":utc(60_000),"lap_duration":60.0,"duration_sector_1":20.0,"duration_sector_2":20.0,"duration_sector_3":20.0,"is_pit_out_lap":false}),
                ),
                row(
                    json!({"driver_number":4,"lap_number":3,"date_start":utc(120_000),"lap_duration":60.0,"duration_sector_1":20.0,"duration_sector_2":20.0,"duration_sector_3":20.0,"is_pit_out_lap":true}),
                ),
            ],
        ),
        (
            Channel::Stints,
            vec![
                row(
                    json!({"driver_number":4,"stint_number":1,"lap_start":1,"lap_end":2,"compound":"MEDIUM","tyre_age_at_start":3}),
                ),
                row(
                    json!({"driver_number":4,"stint_number":2,"lap_start":3,"lap_end":null,"compound":"HARD","tyre_age_at_start":0}),
                ),
            ],
        ),
        (
            Channel::Pit,
            vec![row(
                json!({"driver_number":4,"lap_number":2,"date":utc(100_000),"lane_duration":20.0,"pit_duration":20.0,"stop_duration":2.5}),
            )],
        ),
        (
            Channel::Weather,
            vec![
                row(
                    json!({"date":utc(10_000),"air_temperature":30.0,"track_temperature":42.0,"humidity":70.0,"pressure":1000.0,"wind_speed":2.0,"wind_direction":90,"rainfall":0}),
                ),
                row(
                    json!({"date":utc(140_000),"air_temperature":null,"track_temperature":38.0,"humidity":90.0,"pressure":1000.0,"wind_speed":3.0,"wind_direction":null,"rainfall":1}),
                ),
            ],
        ),
        (
            Channel::RaceControl,
            vec![
                row(
                    json!({"date":utc(0),"driver_number":null,"category":"Flag","scope":"Track","flag":"GREEN","message":"GREEN LIGHT - PIT EXIT OPEN"}),
                ),
                row(
                    json!({"date":utc(50_000),"driver_number":null,"category":"Flag","scope":"Sector","sector":2,"flag":"YELLOW","message":"YELLOW IN TRACK SECTOR 2"}),
                ),
                row(
                    json!({"date":utc(60_000),"driver_number":null,"category":"SafetyCar","scope":null,"flag":null,"message":"SAFETY CAR DEPLOYED"}),
                ),
                row(
                    json!({"date":utc(80_000),"driver_number":null,"category":"Flag","scope":"Track","flag":"RED","message":"RED FLAG"}),
                ),
                row(
                    json!({"date":utc(81_000),"driver_number":null,"category":"SessionStatus","scope":null,"flag":null,"message":"SESSION ABORTED"}),
                ),
                row(
                    json!({"date":utc(180_000),"driver_number":null,"category":"SessionStatus","scope":null,"flag":null,"message":"SESSION STARTED"}),
                ),
                row(
                    json!({"date":utc(181_000),"driver_number":null,"category":"Flag","scope":"Track","flag":"GREEN","message":"GREEN FLAG"}),
                ),
                row(
                    json!({"date":utc(182_000),"driver_number":null,"category":"Flag","scope":"Sector","sector":2,"flag":"CLEAR","message":"CLEAR IN TRACK SECTOR 2"}),
                ),
            ],
        ),
    ]);
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target")
        .join(format!("t12-cache-{}", std::process::id()));
    let window = Window::new(UtcMillis(base - 3_600_000), UtcMillis(base + 10_800_000)).unwrap();
    let spec = CacheSpec::new(
        9590,
        1244,
        2024,
        ("Race", "Race"),
        window,
        std::time::Duration::from_secs(300),
        Channel::ALL.to_vec(),
    )
    .unwrap();
    let mut writer = CacheWriter::begin(&root, &spec).unwrap();
    for channel in Channel::ALL {
        let windows = if matches!(
            channel,
            Channel::CarData | Channel::Location | Channel::Intervals
        ) {
            (0..48)
                .map(|i| {
                    Some(
                        Window::new(
                            UtcMillis(window.start().0 + i * 300_000),
                            UtcMillis(window.start().0 + (i + 1) * 300_000),
                        )
                        .unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        } else {
            vec![None]
        };
        for window in windows {
            let mut url = format!(
                "https://api.openf1.org/v1/{}?session_key=9590",
                channel.endpoint()
            );
            if let Some(w) = window {
                let date = |t: UtcMillis| {
                    chrono::DateTime::from_timestamp_millis(t.0)
                        .unwrap()
                        .format("%Y-%m-%dT%H:%M:%S%.3f")
                        .to_string()
                };
                url.push_str(&format!(
                    "&date>={}&date<{}",
                    date(w.start()),
                    date(w.end())
                ));
            }
            let empty = Vec::new();
            let rows = data.get(&channel).unwrap_or(&empty);
            writer
                .import(
                    ChunkMetadata {
                        channel,
                        session_key: 9590,
                        meeting_key: 1244,
                        window,
                        source_url: url,
                        retrieved_at: UtcMillis(base + 10_800_000),
                        status: 200,
                        attempts: 1,
                    },
                    &serde_json::to_vec(rows).unwrap(),
                )
                .unwrap();
        }
    }
    let cache = writer.finish().unwrap();
    let mut replay = RecordedReplay::from_cache(
        &cache,
        "SYNTHETIC T12 FIXTURE".into(),
        &Cancellation::default(),
        &|_, _| {},
    )
    .unwrap();
    assert_eq!(replay.timeline().origin(), UtcMillis(base));
    assert!(replay.conditions().air_c.value().is_none());
    for (ms, expected_flag, expected_rain, expected_age) in [
        (59_999, Flag::Green, Some(false), Some(3)),
        (60_000, Flag::Green, Some(false), Some(4)),
        (119_999, Flag::Red, Some(false), Some(4)),
        (120_000, Flag::Red, Some(false), Some(0)),
        (140_000, Flag::Red, Some(true), Some(0)),
        (181_000, Flag::Green, Some(true), Some(1)),
        (80_000, Flag::Red, Some(false), Some(4)),
    ] {
        replay.seek(ElapsedMillis(ms)).unwrap();
        let f = replay.conditions();
        assert_eq!(f.track_flag.value(), Some(&expected_flag));
        assert_eq!(f.rain.value(), expected_rain.as_ref());
        assert_eq!(
            f.drivers[&4].tyre.value().unwrap().completed_age,
            expected_age
        );
        assert_eq!(f.drivers[&4].pit.value().is_some(), ms >= 120_000);
        assert_eq!(
            f,
            &replay
                .timeline()
                .conditions_at(ElapsedMillis(ms), ReplayPolicy::EventTimeEstimates)
                .unwrap()
        );
    }
    let mut seed = 42_u64;
    for _ in 0..100 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let target = ElapsedMillis(seed % 480_001);
        replay.seek(target).unwrap();
        assert_eq!(
            replay.conditions(),
            &replay
                .timeline()
                .conditions_at(target, ReplayPolicy::EventTimeEstimates)
                .unwrap()
        );
        assert_eq!(
            replay.frame(),
            &replay
                .timeline()
                .state_at(target, ReplayPolicy::EventTimeEstimates)
                .unwrap()
                .frame(replay.timeline())
                .unwrap()
        );
    }
    replay.seek(ElapsedMillis(400_000)).unwrap();
    assert_eq!(replay.conditions().rain.quality(), Quality::Stale);
    assert_eq!(replay.conditions().track_flag.quality(), Quality::Stale);
    assert!(replay.conditions().air_c.value().is_none());
    let paused = replay.conditions().clone();
    replay
        .update_at(std::time::Duration::from_secs(600))
        .unwrap();
    assert_eq!(
        replay.conditions(),
        &paused,
        "Paused wall time must not age recorded conditions"
    );
    for (width, height) in [(144, 44), (120, 36), (80, 24)] {
        let stream = sectorline::stream::TelemetryStream::load_from_dir(std::path::Path::new(
            "../python_bridge/cache",
        ))
        .or_else(|_| {
            sectorline::stream::TelemetryStream::load_from_dir(std::path::Path::new(
                "python_bridge/cache",
            ))
        })
        .unwrap();
        let mut app = sectorline::app::App::new(stream, false);
        app.prepared_recording = Some(replay);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("~Green"));
        assert!(text.contains("Rain ~yes"));
        assert!(text.contains("Tyre ~HARD"));
        assert!(text.contains("phase unknown"));
        replay = app.prepared_recording.take().unwrap();
    }
    // Retain this explicitly synthetic cache for manual terminal verification; target is ignored.
    std::fs::write(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/t12-cache-root.txt"),
        root.to_string_lossy().as_bytes(),
    )
    .unwrap();
}

#[test]
fn conditions_brazil_source_excerpts_preserve_real_red_restart_and_sc_chronology() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/conditions-brazil.json")).unwrap();
    let rows = fixture["channels"][0]["rows"].as_array().unwrap();
    let time = |r: &serde_json::Value| {
        sectorline::domain::normalize::utc_millis_ceiling(r["date"].as_str().unwrap()).unwrap()
    };
    let mut archive = ConditionsArchive::default();
    for row in rows {
        if let Some(control) = parse_control(row) {
            archive.control.push(reading(time(row).0, control));
        }
    }
    let origin = rows.iter().map(&time).min().unwrap();
    for row in rows {
        let at = time(row).0;
        let mut targets = vec![at];
        if at > origin.0 {
            targets.push(at - 1);
        }
        for at in targets {
            let f = archive
                .frame(&id(), origin, ElapsedMillis((at - origin.0) as u64), true)
                .unwrap();
            let track = rows
                .iter()
                .filter(|r| time(r).0 <= at && r["category"] == "Flag" && r["scope"] == "Track")
                .max_by_key(|r| time(r));
            let expected = track.and_then(|r| r["flag"].as_str()).map(|v| match v {
                "RED" => Flag::Red,
                "GREEN" => Flag::Green,
                _ => panic!("unexpected fixture flag"),
            });
            assert_eq!(f.track_flag.value(), expected.as_ref());
            let sc = rows
                .iter()
                .filter(|r| time(r).0 <= at && r["category"] == "SafetyCar")
                .max_by_key(|r| time(r));
            let expected = sc.and_then(|r| r["message"].as_str()).map(|v| match v {
                "SAFETY CAR DEPLOYED" => SafetyCar::Deployed,
                "SAFETY CAR IN THIS LAP" => SafetyCar::InThisLap,
                "VIRTUAL SAFETY CAR DEPLOYED" => SafetyCar::VirtualDeployed,
                "VIRTUAL SAFETY CAR ENDING" => SafetyCar::VirtualEnding,
                _ => panic!("unexpected fixture safety car"),
            });
            assert_eq!(f.safety_car.value(), expected.as_ref());
        }
    }
    assert!(archive.control.iter().any(|r| {
        r.value
            .as_ref()
            .is_some_and(|c| c.kind == ControlKind::Flag(Flag::Red))
    }));
    assert!(archive.control.iter().any(|r| {
        r.value
            .as_ref()
            .is_some_and(|c| c.kind == ControlKind::Session(SessionStatus::Suspended))
    }));
}
#[test]
fn conditions_telemetry_interval_gaps_are_stale_at_replay_time_and_seek_restores_freshness() {
    use sectorline::{
        data::openf1::{Channel, ChunkMetadata},
        domain::normalize::{Event, Payload, RosterDriver, TelemetryValues},
        replay::{RecordedReplay, timeline::Timeline},
    };
    let sources = [Channel::CarData, Channel::Intervals, Channel::Position]
        .map(|channel| ChunkMetadata {
            channel,
            session_key: 9590,
            meeting_key: 1244,
            window: None,
            source_url: "https://api.openf1.org/v1/test".into(),
            retrieved_at: UtcMillis(900_000),
            status: 200,
            attempts: 1,
        })
        .to_vec();
    let events = vec![
        Event::new(
            4,
            UtcMillis(0),
            Availability::Estimated(UtcMillis(0)),
            0,
            Payload::Telemetry(TelemetryValues {
                speed: Some(SpeedKmh::new(0.0).unwrap()),
                ..Default::default()
            }),
        )
        .unwrap(),
        Event::new(
            4,
            UtcMillis(0),
            Availability::Estimated(UtcMillis(0)),
            1,
            Payload::Intervals {
                leader: Some(Gap::Leader),
                ahead: None,
            },
        )
        .unwrap(),
        Event::new(
            4,
            UtcMillis(500_000),
            Availability::Estimated(UtcMillis(500_000)),
            2,
            Payload::Position(std::num::NonZeroU32::new(1)),
        )
        .unwrap(),
    ];
    let timeline = Timeline::new(
        id(),
        UtcMillis(0),
        vec![RosterDriver::new(4, "TST", "Fixture").unwrap()],
        sources,
        events,
    )
    .unwrap();
    let mut replay = RecordedReplay::new(timeline, "Gap fixture".into()).unwrap();
    replay.seek(ElapsedMillis(2000)).unwrap();
    assert_eq!(
        replay.frame().samples()[0].speed_kmh.quality(),
        Quality::Observed
    );
    replay.seek(ElapsedMillis(2001)).unwrap();
    assert_eq!(
        replay.frame().samples()[0].speed_kmh.quality(),
        Quality::Stale
    );
    assert_eq!(
        replay.frame().samples()[0].speed_kmh.value().unwrap().get(),
        0.0
    );
    replay.seek(ElapsedMillis(10_001)).unwrap();
    assert_eq!(
        replay.frame().samples()[0].gap_to_leader.quality(),
        Quality::Stale
    );
    assert!(replay.frame().samples()[0].gap_to_ahead.value().is_none());
    replay.seek(ElapsedMillis(0)).unwrap();
    assert_eq!(
        replay.frame().samples()[0].speed_kmh.quality(),
        Quality::Observed
    );
}
