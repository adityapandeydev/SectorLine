use sectorline::terminal::{TerminalControl, TerminalSession};
use std::{cell::RefCell, io, rc::Rc};
#[derive(Default, Debug)]
struct State {
    raw: bool,
    alternate: bool,
    hidden: bool,
    fail: Option<&'static str>,
    calls: Vec<&'static str>,
}
#[derive(Clone)]
struct Control(Rc<RefCell<State>>);
impl Control {
    fn change(
        &mut self,
        operation: &'static str,
        raw: Option<bool>,
        alt: Option<bool>,
        cursor: Option<bool>,
    ) -> io::Result<()> {
        let mut s = self.0.borrow_mut();
        s.calls.push(operation);
        if let Some(v) = raw {
            s.raw = v;
        }
        if let Some(v) = alt {
            s.alternate = v;
        }
        if let Some(v) = cursor {
            s.hidden = !v;
        }
        if s.fail == Some(operation) {
            s.fail = None;
            return Err(io::Error::other(operation));
        }
        Ok(())
    }
}
impl TerminalControl for Control {
    fn raw_mode(&mut self, on: bool) -> io::Result<()> {
        self.change(if on { "raw-on" } else { "raw-off" }, Some(on), None, None)
    }
    fn alternate_screen(&mut self, on: bool) -> io::Result<()> {
        self.change(if on { "alt-on" } else { "alt-off" }, None, Some(on), None)
    }
    fn cursor_visible(&mut self, on: bool) -> io::Result<()> {
        self.change(if on { "show" } else { "hide" }, None, None, Some(on))
    }
}
fn clean(s: &State) {
    assert!(!s.raw, "Raw input must be restored");
    assert!(!s.alternate, "Original screen must be restored");
    assert!(!s.hidden, "Cursor must be visible");
}
#[test]
fn terminal_layout_normal_and_error_unwind_restore_all_modes() {
    for error in [false, true] {
        let state = Rc::new(RefCell::new(State::default()));
        let run = || -> io::Result<()> {
            let _session = TerminalSession::start(Control(state.clone()))?;
            assert!(state.borrow().raw && state.borrow().alternate && state.borrow().hidden);
            if error {
                return Err(io::Error::other("draw error"));
            }
            Ok(())
        };
        assert_eq!(run().is_err(), error);
        clean(&state.borrow());
    }
}
#[test]
fn terminal_layout_partial_setup_failure_restores_modes_and_preserves_error() {
    for step in ["raw-on", "alt-on", "hide"] {
        let state = Rc::new(RefCell::new(State {
            fail: Some(step),
            ..State::default()
        }));
        let error = match TerminalSession::start(Control(state.clone())) {
            Ok(_) => panic!("Expected setup failure"),
            Err(e) => e,
        };
        assert_eq!(error.to_string(), step);
        clean(&state.borrow());
    }
}
#[test]
fn terminal_layout_cleanup_failure_does_not_skip_other_steps_and_drop_retries() {
    for step in ["raw-off", "alt-off", "show"] {
        let state = Rc::new(RefCell::new(State::default()));
        let mut guard = TerminalSession::start(Control(state.clone())).unwrap();
        state.borrow_mut().fail = Some(step);
        assert_eq!(guard.restore().unwrap_err().to_string(), step);
        clean(&state.borrow());
        let before = state.borrow().calls.len();
        drop(guard);
        assert!(
            state.borrow().calls.len() > before,
            "Failed cleanup must be retried"
        );
        clean(&state.borrow());
    }
}
#[test]
fn terminal_layout_panic_unwind_restores_and_cleanup_is_idempotent() {
    let state = Rc::new(RefCell::new(State::default()));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = TerminalSession::start(Control(state.clone())).unwrap();
        panic!("UI draw panic");
    }));
    assert!(r.is_err());
    clean(&state.borrow());
    let mut guard = TerminalSession::start(Control(state.clone())).unwrap();
    guard.restore().unwrap();
    let calls = state.borrow().calls.len();
    guard.restore().unwrap();
    drop(guard);
    assert_eq!(calls, state.borrow().calls.len());
}

fn app(recorded: bool) -> sectorline::App {
    let mut app = sectorline::App::new(
        sectorline::TelemetryStream::load_from_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache"),
        )
        .unwrap(),
        false,
    );
    if recorded {
        use sectorline::{
            data::openf1::{Channel, ChunkMetadata},
            domain::{
                normalize::{Event, Payload, RosterDriver, TelemetryValues},
                timing::{Eligibility, LapTiming},
                *,
            },
            replay::{RecordedReplay, timeline::Timeline},
        };
        let source = |channel| ChunkMetadata {
            channel,
            session_key: 9590,
            meeting_key: 1244,
            window: None,
            source_url: format!(
                "https://api.openf1.org/v1/{}?session_key=9590",
                channel.endpoint()
            ),
            retrieved_at: UtcMillis(20000),
            status: 200,
            attempts: 1,
        };
        let mut events = (0..=48)
            .map(|n| {
                let at = UtcMillis(n * 250);
                Event::new(
                    4,
                    at,
                    Availability::Known(at),
                    0,
                    Payload::Telemetry(TelemetryValues {
                        speed: Some(SpeedKmh::new(200.0 + (n as f64 / 3.0).sin() * 30.0).unwrap()),
                        throttle: Some(Percent::new(n as f64 * 2.0).unwrap()),
                        gear: Some(Gear::new(0).unwrap()),
                        brake: Some(BrakeState::Released),
                        drs: Some(DrsCode(9)),
                        rpm: Some(Rpm(9000)),
                    }),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        events.push(
            Event::new(
                4,
                UtcMillis(10000),
                Availability::Known(UtcMillis(10000)),
                1,
                Payload::Lap(Box::new(LapTiming {
                    number: std::num::NonZeroU32::new(1).unwrap(),
                    duration: Some(Seconds::new(9.0).unwrap()),
                    sectors: [Some(Seconds::new(3.0).unwrap()); 3],
                    pit_out: Some(false),
                    eligibility: Eligibility::Eligible,
                })),
            )
            .unwrap(),
        );
        let t = Timeline::new(
            SessionId::new(Provider::OpenF1, "9590").unwrap(),
            UtcMillis(0),
            vec![RosterDriver::new(4, "NOR", "S\u{e3}o Paulo \u{2013} fixture").unwrap()],
            vec![source(Channel::CarData), source(Channel::Laps)],
            events,
        )
        .unwrap();
        let mut replay = RecordedReplay::new(t, "SYNTHETIC RESIZE FIXTURE".into()).unwrap();
        replay.seek(ElapsedMillis(11000)).unwrap();
        app.prepared_recording = Some(replay);
    }
    app
}
fn draw(
    t: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
    app: &sectorline::App,
    w: u16,
    h: u16,
) -> String {
    t.backend_mut().resize(w, h);
    t.resize(ratatui::layout::Rect::new(0, 0, w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    let b = t.backend().buffer();
    assert_eq!(b.content().len(), usize::from(w) * usize::from(h));
    if w == 0 {
        return String::new();
    }
    b.content()
        .chunks(w as usize)
        .map(|r| {
            r.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn terminal_layout_all_views_survive_repeated_real_buffer_resizes() {
    use sectorline::ui::View;
    for recorded in [false, true] {
        let mut app = app(recorded);
        let selected = app.stream.selected_driver_code.clone();
        let frame = app.prepared_recording.as_ref().map(|r| r.frame().clone());
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        for view in [
            View::Timing,
            View::Driver,
            View::Conditions,
            View::Track,
            View::Events,
        ] {
            app.view = view;
            for _ in 0..3 {
                for (w, h) in [
                    (0, 0),
                    (0, 10),
                    (1, 1),
                    (12, 3),
                    (40, 10),
                    (79, 24),
                    (80, 23),
                    (80, 24),
                    (119, 31),
                    (120, 32),
                    (144, 40),
                    (220, 60),
                    (80, 24),
                ] {
                    let text = draw(&mut t, &app, w, h);
                    if w >= 80 && h >= 24 {
                        assert!(text.contains("Sector Line"));
                        assert!(text.contains(if recorded {
                            "RECORDED SNAPSHOT"
                        } else {
                            "DEMO"
                        }));
                        assert!(text.lines().last().unwrap().contains("quit"));
                    }
                }
            }
        }
        assert_eq!(app.stream.selected_driver_code, selected);
        assert_eq!(
            app.prepared_recording.as_ref().map(|r| r.frame()),
            frame.as_ref()
        );
    }
}
#[test]
fn terminal_layout_ascii_no_color_handles_maps_traces_labels_and_overlays() {
    use sectorline::ui::View;
    for recorded in [false, true] {
        let mut app = app(recorded);
        app.theme.ascii = true;
        app.theme.color = false;
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        for view in [
            View::Timing,
            View::Driver,
            View::Conditions,
            View::Track,
            View::Events,
        ] {
            app.view = view;
            for overlay in [0, 1, 2, 3] {
                app.show_session_modal = overlay == 1;
                app.show_help = overlay == 2;
                app.navigation_input = (overlay == 3).then(|| "time 00:01".into());
                let text = draw(&mut t, &app, 120, 36);
                assert!(
                    text.is_ascii(),
                    "ASCII alternative must not require box or braille glyphs"
                );
                assert!(
                    t.backend()
                        .buffer()
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
fn terminal_layout_help_can_reach_the_end_and_reverse_after_resize() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = app(true);
    app.load_message = Some(format!(
        "{} END-OF-LONG-ERROR",
        "Long diagnostics with no fabricated data. ".repeat(150)
    ));
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    draw(&mut t, &app, 80, 24);
    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    for _ in 0..60 {
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    }
    assert!(draw(&mut t, &app, 80, 24).contains("END-OF-LONG-ERROR"));
    draw(&mut t, &app, 144, 44);
    let before = app.help_scroll;
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(app.help_scroll < before);
    for _ in 0..60 {
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    }
    assert!(draw(&mut t, &app, 80, 24).contains("Playback"));
}
#[test]
fn terminal_layout_ctrl_c_quits_even_in_every_overlay() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    for mode in 0..5 {
        let mut app = app(true);
        app.show_help = mode == 1;
        app.show_session_modal = mode == 2;
        app.navigation_input = (mode == 3).then(String::new);
        app.is_searching = mode == 4;
        app.begin_loading(1);
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            sectorline::app::AppAction::Quit
        );
        assert!(app.should_quit);
    }
}
#[test]
fn terminal_layout_redirected_output_gets_an_error_without_terminal_escapes() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_sectorline"))
        .args(["--ascii", "--no-color"])
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Interactive mode needs a terminal"));
    assert!(!out.stdout.windows(8).any(|s| s == b"\x1b[?1049h"));
}

#[test]
fn terminal_layout_quit_does_not_join_a_blocked_loader() {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let loader = sectorline::loader::Loader::new(
        Duration::from_secs(60),
        move |_: (), _, _: &dyn Fn(String)| {
            started_tx.send(()).unwrap();
            let _ = release_rx.recv();
            Ok(())
        },
    );
    let id = loader.start(());
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut app = app(true);
    app.begin_loading(id);
    let start = Instant::now();
    let action = app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('q'),
        crossterm::event::KeyModifiers::NONE,
    ));
    loader.cancel();
    drop(loader);
    assert_eq!(action, sectorline::app::AppAction::Quit);
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "UI shutdown must not wait for provider I/O"
    );
    release_tx.send(()).unwrap();
}

#[test]
fn terminal_layout_tiny_fallback_prioritizes_quit_over_hidden_text_entry() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = app(true);
    app.navigation_input = Some("time ".into());
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    assert_eq!(draw(&mut t, &app, 1, 1), "q");
    let text = draw(&mut t, &app, 16, 2);
    assert!(text.contains("80x24"));
    assert!(text.contains("q quit"));
    assert_eq!(
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        sectorline::app::AppAction::Quit
    );
}

#[test]
fn terminal_layout_capture_ascii_and_narrow_fallback_cells() {
    for (recorded, view, w, h, name) in [
        (true, sectorline::ui::View::Driver, 120, 36, "ascii-driver"),
        (false, sectorline::ui::View::Track, 144, 44, "ascii-track"),
        (true, sectorline::ui::View::Timing, 16, 2, "tiny"),
    ] {
        let mut app = app(recorded);
        app.theme.ascii = true;
        app.theme.color = false;
        app.view = view;
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        let text = draw(&mut t, &app, w, h);
        assert!(text.is_ascii());
        if let Some(root) = std::env::var_os("SECTORLINE_CAPTURE_DIR") {
            let root = std::path::PathBuf::from(root);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join(format!("t17-{name}.txt")), text).unwrap();
            let cells=t.backend().buffer().content().iter().map(|c|serde_json::json!({"s":c.symbol(),"fg":format!("{:?}",c.fg),"bg":format!("{:?}",c.bg),"m":format!("{:?}",c.modifier)})).collect::<Vec<_>>();
            std::fs::write(
                root.join(format!("t17-{name}.json")),
                serde_json::to_vec(&serde_json::json!({"width":w,"height":h,"cells":cells}))
                    .unwrap(),
            )
            .unwrap();
        }
    }
}
