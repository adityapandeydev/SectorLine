use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use sectorline::{App, TelemetryStream};
use std::{path::PathBuf, process::Command};

fn demo_app(modal: bool) -> App {
    let cache = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache");
    App::new(TelemetryStream::load_from_dir(cache).unwrap(), modal)
}

fn render(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(144, 44)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn mode_help_describes_generated_demo_and_unavailable_live() {
    let output = Command::new(env!("CARGO_BIN_EXE_sectorline"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("generated"), "{text}");
    assert!(text.contains("Live access is unavailable"), "{text}");
    assert!(!text.contains("Connect natively"));
}

#[test]
fn mode_header_stays_demo_when_paused() {
    let mut app = demo_app(false);
    for paused in [false, true] {
        app.stream.is_paused = paused;
        let text = render(&app);
        assert!(text.contains("DEMO"), "{text}");
        assert!(text.contains("Generated"), "{text}");
        assert!(!text.contains("LIVE"), "{text}");
        if paused {
            assert!(text.contains("PAUSED"));
        }
    }
}

#[test]
fn mode_session_menu_offers_demos_and_preserves_selection() {
    let mut app = demo_app(true);
    assert!(
        app.session_options
            .iter()
            .all(|s| !s.gp_key.eq_ignore_ascii_case("live"))
    );
    assert!(
        app.session_options
            .iter()
            .filter(|s| s.badge != "SEARCH")
            .all(|s| s.badge == "DEMO")
    );
    let text = render(&app);
    assert!(!text.contains("LIVE"));
    assert!(text.contains("Generated"));
    let expected = app.session_options[0].clone();
    assert_eq!(
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        sectorline::app::AppAction::SwitchSession(expected)
    );
}

#[test]
fn mode_live_cli_requests_fail_before_cache_loading() {
    for args in [
        vec!["--live"],
        vec!["--gp", "live"],
        vec!["--gp", "LIVE"],
        vec!["--gp=live"],
        vec!["--live", "--gp", "monza"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sectorline"))
            .args(args)
            .current_dir(std::env::temp_dir())
            .output()
            .unwrap();
        assert!(!output.status.success());
        let message = String::from_utf8(output.stderr).unwrap();
        assert!(message.contains("Live access is unavailable"), "{message}");
    }
}

#[test]
fn mode_public_live_routes_are_unavailable_without_cache_writes() {
    let missing =
        std::env::temp_dir().join(format!("sectorline-disabled-live-{}", std::process::id()));
    assert!(!missing.exists());
    assert!(!sectorline::LiveTelemetryClient::probe_live_status().is_live_active);
    let error = sectorline::LiveTelemetryClient::fetch_live_stream_natively(&missing)
        .err()
        .unwrap();
    assert!(error.to_string().contains("Live access is unavailable"));
    let error = sectorline::LiveTelemetryClient::ensure_circuit_ready(&missing, "LiVe", 2024, "R")
        .err()
        .unwrap();
    assert!(error.to_string().contains("Live access is unavailable"));
    assert!(!missing.exists());
}

#[test]
fn mode_current_historical_demo_still_loads_without_extraction() {
    let cache = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache");
    assert!(
        sectorline::LiveTelemetryClient::ensure_circuit_ready(&cache, "Baku", 2026, "R")
            .unwrap()
            .is_none()
    );
    let stream = TelemetryStream::load_from_dir(cache).unwrap();
    assert_eq!(stream.frames.len(), 600);
}
