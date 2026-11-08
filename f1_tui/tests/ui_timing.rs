use sectorline::{
    app::App,
    domain::*,
    stream::TelemetryStream,
    ui::view_model::{format_gap, team_code, viewport},
};
use std::num::NonZeroU32;
#[test]
fn ui_timing_gap_units_and_missing_are_not_conflated() {
    assert_eq!(format_gap(None), "--");
    assert_eq!(format_gap(Some(&Gap::Leader)), "leader");
    assert_eq!(
        format_gap(Some(&Gap::Seconds(Seconds::new(0.0).unwrap()))),
        "0.00"
    );
    assert_eq!(
        format_gap(Some(&Gap::LapsBehind(NonZeroU32::new(2).unwrap()))),
        "+2L"
    );
}
#[test]
fn ui_timing_viewport_keeps_position_until_selected_driver_leaves_it() {
    assert_eq!(viewport(26, 0, 13, 0), 0..13);
    assert_eq!(viewport(26, 12, 13, 0), 0..13);
    assert_eq!(viewport(26, 13, 13, 0), 1..14);
    assert_eq!(viewport(26, 25, 13, 1), 13..26);
    assert_eq!(viewport(26, 20, 13, 13), 13..26);
    assert_eq!(viewport(26, 2, 13, 13), 2..15);
    assert_eq!(viewport(26, 25, 33, 13), 0..26);
    assert_eq!(viewport(0, 0, 0, 5), 0..0);
}
#[test]
fn ui_timing_team_labels_have_text_without_color() {
    assert_eq!(team_code("McLaren"), "MCL");
    assert_eq!(team_code("Red Bull Racing"), "RBR");
    assert_eq!(team_code(""), "--");
}

fn app() -> App {
    use sectorline::{
        data::openf1::{Channel, ChunkMetadata},
        domain::{
            classification::DriverStatus,
            conditions::*,
            normalize::{Event, Payload, RosterDriver, TelemetryValues},
            stints::*,
            timing::{Eligibility, LapTiming},
        },
        replay::{RecordedReplay, timeline::Timeline},
    };
    let source = |channel| ChunkMetadata {
        channel,
        session_key: 9590,
        meeting_key: 1244,
        window: None,
        source_url: "https://api.openf1.org/v1/test".into(),
        retrieved_at: UtcMillis(900_000),
        status: 200,
        attempts: 1,
    };
    let roster = (0..26)
        .map(|i| {
            RosterDriver::new(4 + i, &format!("D{i:02}"), &format!("Fixture driver {i}"))
                .unwrap()
                .with_team(
                    Some(match i % 4 {
                        0 => "McLaren",
                        1 => "Red Bull Racing",
                        2 => "Real Best Racing",
                        _ => "Ferrari",
                    }),
                    Some(match i % 4 {
                        0 => "FF8000",
                        1 => "3671C6",
                        2 => "FFFFFF",
                        _ => "FF0000",
                    }),
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    let mut events = Vec::new();
    for i in 0..26_u32 {
        let id = 4 + i;
        for (at, pos) in [(0, i + 1), (100_000, 26 - i), (200_000, (i + 7) % 26 + 1)] {
            events.push(
                Event::new(
                    id,
                    UtcMillis(at),
                    Availability::Known(UtcMillis(at)),
                    0,
                    Payload::Position(NonZeroU32::new(pos)),
                )
                .unwrap(),
            );
        }
        let leader = match i {
            0 => Some(Gap::Leader),
            1 => Some(Gap::Seconds(Seconds::new(0.0).unwrap())),
            2 => Some(Gap::LapsBehind(NonZeroU32::new(2).unwrap())),
            3 => None,
            4 => Some(Gap::Seconds(Seconds::new(1_000_000.125).unwrap())),
            _ => Some(Gap::Seconds(Seconds::new(i as f64 + 0.25).unwrap())),
        };
        events.push(
            Event::new(
                id,
                UtcMillis(90_000),
                Availability::Known(UtcMillis(90_000)),
                1,
                Payload::Intervals {
                    leader,
                    ahead: if i < 2 {
                        None
                    } else {
                        Some(Gap::Seconds(Seconds::new(1.234).unwrap()))
                    },
                },
            )
            .unwrap(),
        );
        events.push(
            Event::new(
                id,
                UtcMillis(90_000),
                Availability::Known(UtcMillis(90_000)),
                2,
                Payload::Telemetry(TelemetryValues {
                    speed: if i == 3 {
                        None
                    } else {
                        Some(SpeedKmh::new(if i == 0 { 0.0 } else { 280.0 + i as f64 }).unwrap())
                    },
                    ..Default::default()
                }),
            )
            .unwrap(),
        );
        if i != 3 {
            events.push(
                Event::new(
                    id,
                    UtcMillis(60_000),
                    Availability::Known(UtcMillis(60_000)),
                    3,
                    Payload::Lap(Box::new(LapTiming {
                        number: NonZeroU32::new(1).unwrap(),
                        duration: Some(Seconds::new(60.0).unwrap()),
                        sectors: [Some(Seconds::new(20.0).unwrap()); 3],
                        pit_out: Some(false),
                        eligibility: Eligibility::Unknown,
                    })),
                )
                .unwrap(),
            );
        }
    }
    events.push(
        Event::new(
            29,
            UtcMillis(100_000),
            Availability::Known(UtcMillis(150_000)),
            4,
            Payload::Status(DriverStatus::Dns),
        )
        .unwrap(),
    );
    let mut conditions = ConditionsArchive::default();
    for i in 0..26_u32 {
        if i == 3 {
            continue;
        }
        conditions.stints.entry(i + 4).or_default().push(
            Reading::new(
                UtcMillis(0),
                Availability::Known(UtcMillis(0)),
                UtcMillis(900_000),
                Some(Stint {
                    number: 1,
                    lap_start: 1,
                    compound: if i == 5 {
                        None
                    } else {
                        Some(
                            match i % 4 {
                                0 => "MEDIUM",
                                1 => "HARD",
                                2 => "INTERMEDIATE",
                                _ => "UNRECOGNIZED",
                            }
                            .into(),
                        )
                    },
                    age_at_start: if i == 5 { None } else { Some(i) },
                }),
            )
            .unwrap(),
        );
        conditions
            .completed_laps
            .entry(i + 4)
            .or_default()
            .push((1, UtcMillis(60_000)));
    }
    let timeline = Timeline::new(
        SessionId::new(Provider::OpenF1, "9590").unwrap(),
        UtcMillis(0),
        roster,
        vec![
            source(Channel::Position),
            source(Channel::Intervals),
            source(Channel::CarData),
            source(Channel::Laps),
            source(Channel::RaceControl),
        ],
        events,
    )
    .unwrap()
    .with_conditions(conditions);
    let mut replay =
        RecordedReplay::new(timeline, "TIMING FIXTURE - 26 synthetic drivers".into()).unwrap();
    let context = sectorline::domain::capabilities::SessionContext::from_names(
        replay.timeline().session().clone(),
        "Race",
        "Race",
    );
    replay = replay.with_context(context).unwrap();
    replay.seek(ElapsedMillis(90_000)).unwrap();
    let mut app = App::new(
        TelemetryStream::load_from_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache"),
        )
        .unwrap(),
        false,
    );
    app.prepared_recording = Some(replay);
    app.theme.color = true;
    app
}
fn capture(app: &App, w: u16, h: u16) -> (String, ratatui::buffer::Buffer) {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    let b = t.backend().buffer().clone();
    let text = b
        .content()
        .chunks(w as usize)
        .map(|row| {
            row.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (text, b)
}
#[test]
fn ui_timing_recorded_rows_keep_optional_units_status_and_team_metadata() {
    use sectorline::ui::view_model::recorded;
    let mut app = app();
    let replay = app.prepared_recording.as_mut().unwrap();
    let rows = recorded(replay);
    assert_eq!(rows.len(), 26);
    assert_eq!(rows[0].gap, "leader");
    assert_eq!(rows[1].gap, "0.00");
    assert_eq!(rows[2].gap, "+2L");
    assert_eq!(rows[3].gap, "--");
    assert_eq!(rows[0].interval, "--");
    assert_eq!(rows[0].speed, "0");
    assert_eq!(rows[3].speed, "--");
    assert_eq!(rows[0].tyre, "M");
    assert_eq!(rows[0].tyre_age, "1");
    assert_eq!(rows[3].tyre, "--");
    assert_eq!(rows[5].tyre_age, "--");
    assert_eq!(rows[3].lap, "--");
    assert_eq!(rows[0].last, "60.000");
    assert_eq!(rows[25].status, "--");
    assert_eq!(rows[0].team, "MCL");
    assert_ne!(
        rows[1].team, rows[2].team,
        "Colliding team initials need distinct stable text"
    );
    assert_eq!(rows[1].team_name.as_deref(), Some("Red Bull Racing"));
    replay.seek(ElapsedMillis(200_000)).unwrap();
    let rows = recorded(replay);
    let driver = rows.iter().find(|r| r.id == 29).unwrap();
    assert_eq!(driver.status, "DNS");
    let zero = rows.iter().find(|r| r.id == 4).unwrap();
    assert_eq!(zero.speed, "~0");
    assert_eq!(zero.gap, "~leader");
}
#[test]
fn ui_timing_numeric_alignment_and_overflow_are_stable_across_value_changes() {
    let mut app = app();
    let (before, _) = capture(&app, 80, 24);
    let lines = before.lines().collect::<Vec<_>>();
    assert!(lines[5].contains("Gap(s)"));
    assert!(lines[5].contains("Last(s)"));
    assert!(lines[5].contains("AgeS"));
    assert!(lines[6].contains("leader"));
    assert!(lines[8].contains("+2L"));
    assert!(
        lines[10].contains("..."),
        "Oversized gap must not be truncated into another number"
    );
    assert_eq!(lines[6].find("D00 #4"), lines[7].find("D01 #5"));
    app.prepared_recording
        .as_mut()
        .unwrap()
        .seek(ElapsedMillis(100_000))
        .unwrap();
    let (after, _) = capture(&app, 80, 24);
    let heading = |s: &str| {
        s.lines()
            .find(|line| line.contains("Gap(s)"))
            .unwrap()
            .to_string()
    };
    assert_eq!(heading(&before), heading(&after));
    for (w, h) in [(144, 44), (120, 36), (80, 24)] {
        let (text, _) = capture(&app, w, h);
        assert!(text.contains("Gap(s)"));
        assert!(text.contains("Ty"));
        assert!(text.contains("Lap"));
        assert!(text.contains("State"));
    }
}
#[test]
fn ui_timing_26_driver_scrolling_and_rapid_reorders_keep_driver_identity() {
    let mut app = app();
    app.prepared_recording.as_mut().unwrap().select(25);
    let selected = app.prepared_recording.as_ref().unwrap().selected();
    assert_eq!(selected, 29);
    let (text, _) = capture(&app, 80, 24);
    assert!(text.contains("Timing 14-26 / 26"));
    assert!(
        text.lines()
            .any(|line| line.starts_with('>') && line.contains("D25 #29"))
    );
    assert_eq!(app.timing_offset.get(), 13);
    app.prepared_recording.as_mut().unwrap().select(-1);
    let _ = capture(&app, 80, 24);
    assert_eq!(
        app.timing_offset.get(),
        13,
        "Do not jump the viewport while selection remains visible"
    );
    let selected = app.prepared_recording.as_ref().unwrap().selected();
    for n in 0..100 {
        let target = ElapsedMillis([90_000, 100_000, 200_000][n % 3]);
        app.prepared_recording
            .as_mut()
            .unwrap()
            .seek(target)
            .unwrap();
        let (text, _) = capture(&app, 80, 24);
        assert_eq!(
            app.prepared_recording.as_ref().unwrap().selected(),
            selected
        );
        assert!(
            text.lines()
                .any(|line| line.starts_with('>') && line.contains(&format!("#{}", selected))),
            "Selected identity must remain visible after source reorder"
        );
    }
    let (text, _) = capture(&app, 144, 44);
    assert!(text.contains("Timing 1-26 / 26"));
    assert_eq!(app.timing_offset.get(), 0);
}
#[test]
fn ui_timing_team_identity_and_selection_use_text_in_monochrome_and_ansi() {
    use ratatui::style::{Color, Modifier};
    let mut app = app();
    for color in [true, false] {
        app.theme.color = color;
        let (text, buffer) = capture(&app, 80, 24);
        assert!(text.contains("MCL"));
        assert!(
            !text.contains("FERR"),
            "Team column remains fixed at three cells"
        );
        assert!(
            text.lines()
                .any(|line| line.starts_with('>') && line.contains("D00 #4"))
        );
        assert!(
            buffer
                .content()
                .iter()
                .any(|c| c.modifier.contains(Modifier::REVERSED))
        );
        if !color {
            assert!(
                buffer
                    .content()
                    .iter()
                    .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
            );
        } else {
            assert!(
                buffer
                    .content()
                    .iter()
                    .all(|c| !matches!(c.fg, Color::Rgb(..) | Color::Indexed(_)))
            );
            assert!(buffer.content().iter().any(|c| c.fg != Color::Reset));
        }
    }
    app.view = sectorline::ui::View::Driver;
    assert!(capture(&app, 80, 24).0.contains("Team McLaren"));
}
#[test]
fn ui_timing_missing_team_or_invalid_color_does_not_infer_team_from_driver() {
    use sectorline::domain::normalize::RosterDriver;
    let d = RosterDriver::new(1, "VER", "Fixture").unwrap();
    assert!(d.team.is_none());
    assert!(d.team_color.is_none());
    let d = d.with_team(Some("Source Team"), Some("nothex")).unwrap();
    assert_eq!(d.team.as_deref(), Some("Source Team"));
    assert!(d.team_color.is_none());
    assert!(
        RosterDriver::new(1, "TST", "Fixture")
            .unwrap()
            .with_team(Some("Bad\nTeam"), None)
            .is_err()
    );
}
#[test]
fn ui_timing_wide_standard_compact_capture_actual_cells() {
    let app = app();
    for (width, height, name) in [
        (144, 44, "wide"),
        (120, 36, "standard"),
        (80, 24, "compact"),
    ] {
        let (text, buffer) = capture(&app, width, height);
        if let Some(root) = std::env::var_os("SECTORLINE_CAPTURE_DIR") {
            let root = std::path::PathBuf::from(root);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join(format!("t14-{name}.txt")), &text).unwrap();
            let cells=buffer.content().iter().map(|c|serde_json::json!({"s":c.symbol(),"fg":format!("{:?}",c.fg),"bg":format!("{:?}",c.bg),"m":format!("{:?}",c.modifier)})).collect::<Vec<_>>();
            std::fs::write(
                root.join(format!("t14-{name}.json")),
                serde_json::to_vec(
                    &serde_json::json!({"width":width,"height":height,"cells":cells}),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
}

#[test]
fn ui_timing_overflow_value_is_readable_in_driver_details() {
    let mut app = app();
    app.prepared_recording.as_mut().unwrap().select(4);
    app.view = sectorline::ui::View::Driver;
    assert!(capture(&app, 80, 24).0.contains("1000000.12"));
}
