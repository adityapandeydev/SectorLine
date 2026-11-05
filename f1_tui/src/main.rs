use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::env;
use std::io::stdout;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sectorline::app::App;
use sectorline::stream::TelemetryStream;
use sectorline::loader::session::{SessionRequest, WorkerConfig, session_loader};

fn find_cache_dir() -> PathBuf {
    let candidate_paths = [
        "f1_suite/python_bridge/cache",
        "../python_bridge/cache",
        "python_bridge/cache",
        "cache",
    ];

    for candidate in &candidate_paths {
        let p = Path::new(candidate);
        if p.exists() && p.join("session_info.json").exists() {
            return p.to_path_buf();
        }
    }

    PathBuf::from("f1_suite/python_bridge/cache")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cache_dir = find_cache_dir();
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Sector Line - recorded timing and generated demo");
        println!("Generated demo playback is simulated. Recorded timing uses explicit event-time estimates.");
        println!("Usage: sectorline [--demo] [--gp NAME] [--year YYYY]");
        println!("  --demo, --headless  Run 20 generated frames without opening the TUI");
        println!("  --gp NAME           Generate a demo using historical session metadata (Python required)");
        println!("  --year YYYY         Historical metadata year (default: 2026)");
        println!("  --help, -h          Show help");
        println!("  --catalogue         Browse cached OpenF1 metadata (separate from demo playback)");
        println!("    --refresh [--year YYYY]  Fetch meetings and optionally that exact year's sessions");
        println!("    --year YYYY --search TEXT --session ID  Filter metadata/select an exact ID offline");
        println!("    --catalogue-cache PATH  Override the user catalogue cache path");
        println!("Live access is unavailable; --live and --gp live are disabled.");
        println!("Without options, open cached demo and catalogue search. Recordings open paused; Space plays, 1/2/5/0 set speed, Home restarts.");
        println!("  --recording-root PATH  Override the recording cache for the interactive loader");
        println!("  --prepare-session ID   Prepare an exact catalogue recording in the background");
        return Ok(());
    }

    if args.iter().any(|a| a == "--live" || a.eq_ignore_ascii_case("--gp=live")) {
        return Err(sectorline::live::LIVE_UNAVAILABLE.into());
    }
    if args.iter().any(|a| a == "--catalogue") {
        return sectorline::data::catalogue_cli::run(&args);
    }
    let is_demo_mode = args.iter().any(|a| a == "--demo" || a == "--headless");
    let prepare_key = if let Some(index) = args.iter().position(|a| a == "--prepare-session") {
        Some(
            args.get(index + 1)
                .ok_or("--prepare-session needs a numeric session ID")?
                .parse::<u32>()?,
        )
    } else {
        None
    };
    if is_demo_mode && prepare_key.is_some() {
        return Err("--prepare-session opens the interactive loader; omit --demo".into());
    }

    // Parse --gp and --year arguments
    let mut target_gp: Option<String> = None;
    let mut target_year: u32 = 2026;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--gp" && i + 1 < args.len() {
            target_gp = Some(args[i + 1].clone());
            i += 1;
        } else if args[i] == "--year" && i + 1 < args.len() {
            if let Ok(y) = args[i + 1].parse::<u32>() {
                target_year = y;
            }
            i += 1;
        }
        i += 1;
    }

    if target_gp.as_deref().is_some_and(|gp| gp.eq_ignore_ascii_case("live")) {
        return Err(sectorline::live::LIVE_UNAVAILABLE.into());
    }

    println!("Sector Line - DEMO | Generated car movement and telemetry; not observed replay.");
    let stream = if let Some(ref gp) = target_gp {
        if let Some(s) = sectorline::live::LiveTelemetryClient::ensure_circuit_ready(&cache_dir, gp, target_year, "R")? {
            s
        } else {
            TelemetryStream::load_from_dir(&cache_dir)?
        }
    } else {
        println!("Loading generated demo cache from: {:?}", cache_dir);
        TelemetryStream::load_from_dir(&cache_dir)?
    };

    println!(
        "Loaded: {} | Circuit: {} | Drivers: {} | Telemetry Frames: {}",
        stream.session.event_name,
        stream.session.circuit_name,
        stream.drivers.len(),
        stream.frames.len()
    );

    let start_with_modal = !is_demo_mode && target_gp.is_none();
    let mut app = App::new(stream, start_with_modal);

    // Headless / Demo Mode for non-interactive environments and CI/CD validation
    if is_demo_mode {
        println!("\n[DEMO MODE] Running automated 20-frame headless playback verification...");
        for step in 1..=20 {
            app.tick(200.0);
            let frame = app.stream.current_frame();
            let p1 = &frame.cars[0];
            println!(
                " Frame {:2}/20 | Sim Time: {:5.1}s | P1: {} (Speed: {:3.0} km/h, Gear: {}, RPM: {})",
                step,
                frame.timestamp_ms as f64 / 1000.0,
                p1.driver,
                p1.speed,
                p1.gear,
                p1.rpm
            );
        }
        println!("\n[OK] Headless playback simulation passed successfully!");
        return Ok(());
    }

    let option_path = |flag: &str| {
        args.windows(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| PathBuf::from(&pair[1]))
    };
    let catalogue_path = match option_path("--catalogue-cache") {
        Some(path) => path,
        None => sectorline::data::catalogue::default_cache_path()?,
    };
    let recording_root = match option_path("--recording-root") {
        Some(path) => path,
        None => sectorline::data::paths::session_cache_root().map_err(|e| e.to_string())?,
    };
    let loader = session_loader(WorkerConfig {
        demo_dir: cache_dir.clone(),
        catalogue_path,
        recording_root,
        limits: sectorline::data::limits::Limits::default(),
        endpoint: "https://api.openf1.org/v1".into(),
        job_timeout: Duration::from_secs(15 * 60),
    });
    let mut last_request: Option<SessionRequest> = None;
    if let Some(key) = prepare_key {
        let request = SessionRequest::Recording(key);
        app.begin_loading(loader.start(request.clone()));
        last_request = Some(request);
        app.show_session_modal = false;
    }

    // Interactive Terminal Setup
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let started = Instant::now();
    let mut last_draw = Instant::now();
    let mut dirty = true;
    let target_fps = 30;
    let tick_rate = Duration::from_millis(1000 / target_fps);

    // Main Interactive Render Loop
    let run_res = (|| -> Result<(), Box<dyn std::error::Error>> {
        loop {
            dirty |= app.update_at(started.elapsed());
            if let Some(event) = loader.poll() {
                app.accept_loader_event(event);
                dirty = true;
            }
            if dirty && last_draw.elapsed() >= tick_rate {
                terminal.draw(|f| app.render(f))?;
                dirty = false;
                last_draw = Instant::now();
            }
            let timeout = if dirty {
                tick_rate.saturating_sub(last_draw.elapsed())
            } else if app.needs_playback_ticks() {
                tick_rate
            } else {
                Duration::from_millis(100)
            };
            if event::poll(timeout)? {
                let event = event::read()?;
                if matches!(event, Event::Resize(..)) {
                    dirty = true;
                    continue;
                }
                let Event::Key(key) = event else {
                    continue;
                };
                if key.kind == crossterm::event::KeyEventKind::Release {
                    continue;
                }
                app.update_at(started.elapsed());
                let action = app.handle_key(key);
                dirty = true;
                match action {
                    sectorline::app::AppAction::Quit => break,
                    sectorline::app::AppAction::SwitchSession(opt) => {
                        let request = opt
                            .recording_key
                            .map(SessionRequest::Recording)
                            .unwrap_or(SessionRequest::Demo);
                        let id = loader.start(request.clone());
                        last_request = Some(request);
                        app.begin_loading(id);
                    }
                    sectorline::app::AppAction::SearchCatalogue(query) => {
                        let request = SessionRequest::Search(query);
                        let id = loader.start(request.clone());
                        last_request = Some(request);
                        app.begin_loading(id);
                    }
                    sectorline::app::AppAction::CancelLoad => loader.cancel(),
                    sectorline::app::AppAction::RetryLoad => {
                        if let Some(request) = &last_request {
                            app.begin_loading(loader.start(request.clone()));
                        }
                    }
                    sectorline::app::AppAction::None => {}
                }
            }

            if app.should_quit {
                break;
            }
        }
        Ok(())
    })();

    loader.cancel();
    // Terminal Teardown
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    run_res
}
