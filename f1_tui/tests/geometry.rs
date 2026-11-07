use sectorline::domain::Availability;
use sectorline::domain::UtcMillis;
use sectorline::domain::geometry::{Fix, Motion, Point, Trace, Transform, sample};
fn ring() -> Vec<Point> {
    (0..=240)
        .map(|n| {
            let a = n as f64 * std::f64::consts::TAU / 240.0;
            Point {
                x: 1000.0 * a.cos(),
                y: 600.0 * a.sin(),
            }
        })
        .collect()
}
#[test]
fn geometry_preserves_rotation_scale_and_terminal_cell_aspect() {
    let points = ring();
    for rotation in [0.0, 37.0, 90.0, 258.0] {
        let fit = Transform::fit(&points, 80, 20, rotation, 2.0).unwrap();
        let center = fit.project(Point { x: 0.0, y: 0.0 }).unwrap();
        assert!((center.x - 39.5).abs() < 0.01 && (center.y - 9.5).abs() < 0.01);
        assert!(points.iter().all(|p| fit.project(*p).is_some()));
    }
    let fit = Transform::fit(&points, 80, 30, 0.0, 2.0).unwrap();
    let p = fit.project(Point { x: 100.0, y: 100.0 }).unwrap();
    let zero = fit.project(Point { x: 0.0, y: 0.0 }).unwrap();
    assert!(((p.x - zero.x) / (p.y - zero.y) - 2.0).abs() < 1e-9);
    let rotated = Transform::fit(&points, 80, 30, 90.0, 2.0).unwrap();
    let x = rotated.project(Point { x: 100.0, y: 0.0 }).unwrap();
    let zero = rotated.project(Point { x: 0.0, y: 0.0 }).unwrap();
    assert!((x.x - zero.x).abs() < 1e-9 && x.y > zero.y);
    assert!(Transform::fit(&points, 0, 20, 0.0, 2.0).is_none());
    assert!(Transform::fit(&points, 80, 20, 0.0, 0.0).is_none());
}
#[test]
fn geometry_rejects_degenerate_and_discontinuous_lap_traces() {
    assert!(Trace::new(ring()).is_ok());
    assert!(Trace::new(vec![Point { x: 1.0, y: 1.0 }; 100]).is_err());
    let mut points = ring();
    points[100].x = 1e8;
    assert!(Trace::new(points).is_err());
    let mut points = ring();
    points.truncate(140);
    assert!(Trace::new(points).is_err());
    assert!(
        Trace::new(vec![
            Point {
                x: f64::NAN,
                y: 0.0
            };
            100
        ])
        .is_err()
    );
}
fn fix(at: i64, x: f64) -> Fix {
    Fix {
        at,
        available: Availability::Known(UtcMillis(at)),
        point: Some(Point { x, y: 0.0 }),
        ambiguous: false,
    }
}
#[test]
fn geometry_interpolates_only_admitted_short_pit_free_segments() {
    let a = fix(1000, -10.0);
    let b = fix(1250, 10.0);
    let middle = sample(a, Some(b), 1125, 1500, true, 100.0).unwrap();
    assert_eq!(middle.motion, Motion::Interpolated);
    assert_eq!(
        middle.point.x, 0.0,
        "Crossing an axis must interpolate raw XY, never wrap distance"
    );
    for (next, pit_free) in [
        (fix(2000, 10.0), true),
        (fix(1250, 1000.0), true),
        (b, false),
    ] {
        assert_eq!(
            sample(a, Some(next), 1125, 1500, pit_free, 100.0)
                .unwrap()
                .point
                .x,
            -10.0
        );
    }
    let mut future = b;
    future.available = Availability::Known(UtcMillis(1600));
    assert_eq!(
        sample(a, Some(future), 1125, 1500, true, 100.0)
            .unwrap()
            .motion,
        Motion::Held
    );
    future.available = Availability::Unknown;
    assert_eq!(
        sample(a, Some(future), 1125, 1500, true, 100.0)
            .unwrap()
            .point
            .x,
        -10.0
    );
    let mut missing = a;
    missing.point = None;
    assert!(sample(missing, Some(b), 1125, 1500, true, 100.0).is_none());
    missing = a;
    missing.ambiguous = true;
    assert!(sample(missing, Some(b), 1125, 1500, true, 100.0).is_none());
    assert!(
        sample(a, None, 3500, 4000, true, 100.0).is_none(),
        "Stale locations must leave the map"
    );
}

fn replay(drivers: u32, pit_free: bool) -> sectorline::replay::RecordedReplay {
    use sectorline::{
        data::openf1::{Channel, ChunkMetadata},
        domain::{
            Provider, SessionId,
            normalize::{Coordinates, Event, Payload, RosterDriver},
        },
        replay::{RecordedReplay, timeline::Timeline},
        track::LapWindow,
    };
    let source = ChunkMetadata {
        channel: Channel::Location,
        session_key: 9590,
        meeting_key: 1244,
        window: None,
        source_url: "https://api.openf1.org/v1/location?session_key=9590".into(),
        retrieved_at: UtcMillis(100000),
        status: 200,
        attempts: 1,
    };
    let roster = (0..drivers)
        .map(|n| RosterDriver::new(n + 4, &format!("D{n:02}"), "Synthetic driver").unwrap())
        .collect();
    let mut events = Vec::new();
    for d in 0..drivers {
        for (n, p) in ring().into_iter().enumerate() {
            let at = UtcMillis(n as i64 * 250);
            events.push(
                Event::new(
                    d + 4,
                    at,
                    Availability::Known(at),
                    0,
                    Payload::Location(Coordinates::new(Some(p.x), Some(p.y), None).unwrap()),
                )
                .unwrap(),
            );
        }
    }
    RecordedReplay::new(
        Timeline::new(
            SessionId::new(Provider::OpenF1, "9590").unwrap(),
            UtcMillis(0),
            roster,
            vec![source],
            events,
        )
        .unwrap(),
        "SYNTHETIC GEOMETRY FIXTURE".into(),
    )
    .unwrap()
    .with_track_laps(
        (0..drivers)
            .map(|d| LapWindow {
                driver: d + 4,
                lap: 1,
                start: 0,
                end: 60000,
                pit_free,
            })
            .collect(),
    )
    .unwrap()
}
#[test]
fn geometry_archive_is_session_bound_and_seek_deterministic() {
    use sectorline::domain::ElapsedMillis;
    let mut r = replay(2, true);
    assert!(r.track().reference().is_some());
    let first = r.track().position(r.timeline(), 4, 30125).unwrap();
    assert_eq!(first.motion, Motion::Interpolated);
    r.seek(ElapsedMillis(55000)).unwrap();
    r.seek(ElapsedMillis(30125)).unwrap();
    assert_eq!(
        r.track().position(r.timeline(), 4, 30125).unwrap().point,
        first.point
    );
    assert!(r.track().position(r.timeline(), 4, -1).is_none());
    let cancel = sectorline::data::limits::Cancellation::default();
    cancel.cancel();
    assert!(sectorline::track::TrackArchive::build(r.timeline(), vec![], &cancel).is_err());
    let foreign = replay(1, true);
    assert!(r.track().position(foreign.timeline(), 4, 30125).is_none());
    assert!(
        replay(1, false).track().reference().is_none(),
        "Pit/unknown laps cannot create a reference circuit"
    );
}
#[test]
fn geometry_real_monza_excerpt_uses_raw_xy_and_rejects_a_partial_reference() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/reducer-monza.json")).unwrap();
    let rows = fixture["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["channel"] == "location")
        .unwrap()["rows"]
        .as_array()
        .unwrap();
    let fix_from = |r: &serde_json::Value| {
        let at =
            sectorline::domain::normalize::utc_millis_ceiling(r["date"].as_str().unwrap()).unwrap();
        Fix {
            at: at.0,
            available: Availability::Estimated(at),
            point: Some(Point {
                x: r["x"].as_f64().unwrap(),
                y: r["y"].as_f64().unwrap(),
            }),
            ambiguous: false,
        }
    };
    let a = fix_from(&rows[0]);
    let b = fix_from(&rows[1]);
    let target = (a.at + b.at) / 2;
    let p = sample(a, Some(b), target, target + 500, true, 1000.0).unwrap();
    assert_eq!(p.motion, Motion::Interpolated);
    let fraction = (target - a.at) as f64 / (b.at - a.at) as f64;
    assert!((p.point.x - (10880.0 + 27.0 * fraction)).abs() < 1e-6);
    assert!((p.point.y - (15152.0 - 145.0 * fraction)).abs() < 1e-6);
    assert!(
        Trace::new(rows.iter().map(|r| fix_from(r).point.unwrap()).collect()).is_err(),
        "Six excerpt points cannot be claimed as a circuit"
    );
}
#[test]
fn geometry_cache_pipeline_rejects_pits_gaps_jumps_and_unknown_context() {
    use sectorline::{
        data::{
            cache::{CacheSpec, CacheWriter},
            limits::Cancellation,
            openf1::{Channel, ChunkMetadata, Window},
        },
        replay::RecordedReplay,
    };
    use serde_json::{Value, json};
    use std::{collections::BTreeMap, time::Duration};
    let base = 1725195600000_i64;
    let utc = |ms: i64| {
        chrono::DateTime::from_timestamp_millis(base + ms)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    for case in [
        "valid",
        "pit",
        "out",
        "gap",
        "jump",
        "missing",
        "conflict",
        "unknown-pit",
        "no-pit-channel",
        "next-boundary",
    ] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../target/t15-cache-{case}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut data = BTreeMap::<Channel, Vec<Value>>::new();
        let row = |mut r: Value| {
            r["session_key"] = json!(9590);
            r["meeting_key"] = json!(1244);
            r
        };
        data.insert(Channel::Drivers,vec![row(json!({"driver_number":4,"name_acronym":"TST","full_name":"Synthetic map driver","team_name":"Fixture","team_colour":"FF8000"}))]);
        data.insert(Channel::Laps,(1..=3).map(|n|row(json!({"driver_number":4,"lap_number":n,"date_start":utc((n-1)*60000),"lap_duration":if case=="next-boundary" && n==1 {61.0}else{60.0},"duration_sector_1":null,"duration_sector_2":null,"duration_sector_3":null,"is_pit_out_lap":case=="out" && n==2}))).collect());
        data.insert(
            Channel::Location,
            ring()
                .iter()
                .enumerate()
                .map(|(n, p)| {
                    row(json!({"driver_number":4,"date":utc(n as i64*250),"x":p.x,"y":p.y,"z":0}))
                })
                .collect(),
        );
        match case {
            "pit" | "unknown-pit" => {
                data.insert(Channel::Pit,vec![row(json!({"driver_number":4,"lap_number":if case=="pit"{json!(1)}else{Value::Null},"date":utc(30000),"lane_duration":20.0,"pit_duration":20.0,"stop_duration":2.5}))]);
            }
            "gap" => {
                data.get_mut(&Channel::Location).unwrap().drain(100..104);
            }
            "jump" => data.get_mut(&Channel::Location).unwrap()[100]["x"] = json!(1e8),
            "missing" => data.get_mut(&Channel::Location).unwrap()[100]["x"] = Value::Null,
            "conflict" => {
                let mut r = data[&Channel::Location][100].clone();
                r["x"] = json!(1.0);
                data.get_mut(&Channel::Location).unwrap().push(r);
            }
            _ => {}
        }
        let channels = Channel::ALL
            .into_iter()
            .filter(|c| case != "no-pit-channel" || *c != Channel::Pit)
            .collect::<Vec<_>>();
        let window = Window::new(UtcMillis(base - 3600000), UtcMillis(base + 10800000)).unwrap();
        let spec = CacheSpec::new(
            9590,
            1244,
            2024,
            ("Race", "Race"),
            window,
            Duration::from_secs(300),
            channels.clone(),
        )
        .unwrap();
        let mut writer = CacheWriter::begin(&root, &spec).unwrap();
        for channel in channels {
            let windows = if matches!(
                channel,
                Channel::CarData | Channel::Location | Channel::Intervals
            ) {
                (0..48)
                    .map(|n| {
                        Some(
                            Window::new(
                                UtcMillis(window.start().0 + n * 300000),
                                UtcMillis(window.start().0 + (n + 1) * 300000),
                            )
                            .unwrap(),
                        )
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![None]
            };
            for win in windows {
                let mut url = format!(
                    "https://api.openf1.org/v1/{}?session_key=9590",
                    channel.endpoint()
                );
                if let Some(w) = win {
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
                let rows = data
                    .get(&channel)
                    .into_iter()
                    .flatten()
                    .filter(|r| {
                        win.is_none_or(|w| {
                            let t = sectorline::domain::normalize::utc_millis_ceiling(
                                r["date"].as_str().unwrap(),
                            )
                            .unwrap();
                            t >= w.start() && t < w.end()
                        })
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                writer
                    .import(
                        ChunkMetadata {
                            channel,
                            session_key: 9590,
                            meeting_key: 1244,
                            window: win,
                            source_url: url,
                            retrieved_at: UtcMillis(base + 10800000),
                            status: 200,
                            attempts: 1,
                        },
                        &serde_json::to_vec(&rows).unwrap(),
                    )
                    .unwrap();
            }
        }
        let cache = writer.finish().unwrap();
        let mut replay = RecordedReplay::from_cache(
            &cache,
            "SYNTHETIC T15 MAP FIXTURE".into(),
            &Cancellation::default(),
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(
            replay.track().reference().is_some(),
            matches!(case, "valid" | "next-boundary"),
            "{case}"
        );
        replay
            .seek(sectorline::domain::ElapsedMillis(30125))
            .unwrap();
        if case == "valid" {
            assert_eq!(
                replay
                    .track()
                    .position(replay.timeline(), 4, base + 30125)
                    .unwrap()
                    .motion,
                Motion::Interpolated
            );
        }
        if case == "valid" && std::env::var_os("SECTORLINE_CAPTURE_DIR").is_some() {
            std::fs::write(
                root.parent().unwrap().join("t15-cache-root.txt"),
                root.to_str().unwrap(),
            )
            .unwrap();
        } else {
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
