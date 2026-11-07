use super::*;
fn app() -> sectorline::app::App {
    let mut app = sectorline::app::App::new(
        sectorline::stream::TelemetryStream::load_from_dir(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache"),
        )
        .unwrap(),
        false,
    );
    let mut replay = replay(26, true);
    replay
        .seek(sectorline::domain::ElapsedMillis(30125))
        .unwrap();
    app.prepared_recording = Some(replay);
    app.show_session_modal = false;
    app.view = sectorline::ui::View::Track;
    app
}
fn capture(app: &sectorline::app::App, w: u16, h: u16) -> (String, ratatui::buffer::Buffer) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let text = buffer
        .content()
        .chunks(w as usize)
        .map(|row| {
            row.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (text, buffer)
}
#[test]
fn geometry_map_prioritizes_selected_labels_and_is_readable_without_color() {
    let mut app = app();
    for color in [true, false] {
        app.theme.color = color;
        for (w, h) in [(144, 44), (120, 36), (80, 24)] {
            let (text, buffer) = capture(&app, w, h);
            assert!(text.contains("Observed lap trace"));
            assert!(text.contains("Cars -0.5s"));
            assert!(
                text.contains("[D00]"),
                "Selected identity wins coincident cars"
            );
            assert!(buffer.content().iter().all(|c| !matches!(
                c.fg,
                ratatui::style::Color::Rgb(..) | ratatui::style::Color::Indexed(_)
            )));
            if !color {
                assert!(
                    buffer
                        .content()
                        .iter()
                        .all(|c| c.fg == ratatui::style::Color::Reset
                            && c.bg == ratatui::style::Color::Reset)
                );
            }
        }
    }
}
#[test]
fn geometry_absent_reference_keeps_timing_and_does_not_draw_a_demo_circuit() {
    let mut app = app();
    app.prepared_recording = Some(replay(1, false));
    let (text, buffer) = capture(&app, 80, 24);
    assert!(text.contains("Track view unavailable"));
    assert!(!buffer.content().iter().any(|c| {
        c.symbol()
            .chars()
            .any(|s| ('\u{2801}'..='\u{28ff}').contains(&s))
    }));
    app.view = sectorline::ui::View::Timing;
    assert!(capture(&app, 80, 24).0.contains("Timing 1-1 / 1"));
}
#[test]
fn geometry_controls_only_change_map_transform() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = app();
    let before = app.prepared_recording.as_ref().unwrap().frame().clone();
    app.handle_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE));
    assert_eq!(app.map_rotation, 15.0);
    assert!((app.cell_aspect - 2.1).abs() < 0.001);
    assert_eq!(app.prepared_recording.as_ref().unwrap().frame(), &before);
}
#[test]
fn geometry_capture_actual_map_cells() {
    let app = app();
    for (w, h, name) in [
        (144, 44, "wide"),
        (120, 36, "standard"),
        (80, 24, "compact"),
    ] {
        let (text, buffer) = capture(&app, w, h);
        if let Some(root) = std::env::var_os("SECTORLINE_CAPTURE_DIR") {
            let root = std::path::PathBuf::from(root);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join(format!("t15-{name}.txt")), text).unwrap();
            let cells=buffer.content().iter().map(|c|serde_json::json!({"s":c.symbol(),"fg":format!("{:?}",c.fg),"bg":format!("{:?}",c.bg),"m":format!("{:?}",c.modifier)})).collect::<Vec<_>>();
            std::fs::write(
                root.join(format!("t15-{name}.json")),
                serde_json::to_vec(&serde_json::json!({"width":w,"height":h,"cells":cells}))
                    .unwrap(),
            )
            .unwrap();
        }
    }
}
