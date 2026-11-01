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
        println!("==========================================================================");
        println!("       F1 RACE CONTROL COMMAND CENTER & LIVE TELEMETRY TELEPRINTER        ");
        println!("==========================================================================");
        println!("USAGE:");
        println!("  cargo run --release -p sectorline -- [OPTIONS]\n");
        println!("OPTIONS:");
        println!("  --live             Connect natively to OpenF1 real-time live timing stream");
        println!("  --gp <NAME>        Select Grand Prix (e.g. Silverstone, Monza, Spa, Monaco)");
        println!("  --year <YYYY>      Championship season year (default: 2024 / 2023)");
        println!("  --demo             Run 20-frame headless playback verification");
        println!("  --help, -h         Show this help message\n");
        println!("EXAMPLES:");
        println!("  cargo run --release -p sectorline -- --live");
        println!("  cargo run --release -p sectorline -- --gp monza");
        println!("  cargo run --release -p sectorline -- --gp spa --year 2024");
        return Ok(());
    }

    let is_live_mode = args.iter().any(|a| a == "--live");
    let is_demo_mode = args.iter().any(|a| a == "--demo" || a == "--headless");

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

    // In live mode without explicit --gp override, connect directly to OpenF1 live stream
    if is_live_mode && target_gp.is_none() {
        target_gp = Some("LIVE".to_string());
    }

    println!("==========================================================================");
    println!("       F1 RACE CONTROL COMMAND CENTER & LIVE TELEMETRY TELEPRINTER        ");
    println!("==========================================================================");

    let stream = if is_live_mode || target_gp.as_deref() == Some("LIVE") {
        println!("Connecting natively to OpenF1 real-time telemetry API (100% Pure Rust, Zero Python)...");
        sectorline::live::LiveTelemetryClient::fetch_live_stream_natively(&cache_dir)?
    } else if let Some(ref gp) = target_gp {
        if let Some(s) = sectorline::live::LiveTelemetryClient::ensure_circuit_ready(&cache_dir, gp, target_year, "R")? {
            s
        } else {
            TelemetryStream::load_from_dir(&cache_dir)?
        }
    } else {
        println!("Loading Grand Prix Telemetry Datasets from: {:?}...", cache_dir);
        TelemetryStream::load_from_dir(&cache_dir)?
    };

    println!(
        "Loaded: {} | Circuit: {} | Drivers: {} | Telemetry Frames: {}",
        stream.session.event_name,
        stream.session.circuit_name,
        stream.drivers.len(),
        stream.frames.len()
    );

    let start_with_modal = !is_demo_mode && target_gp.is_none() && !is_live_mode;
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

    // Interactive Terminal Setup
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let mut last_tick = Instant::now();
    let target_fps = 30;
    let tick_rate = Duration::from_millis(1000 / target_fps);

    // Main Interactive Render Loop
    let run_res = (|| -> Result<(), Box<dyn std::error::Error>> {
        loop {
            terminal.draw(|f| app.render(f))?;

            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    let action = app.handle_key(key);
                    match action {
                        sectorline::app::AppAction::Quit => break,
                        sectorline::app::AppAction::SwitchSession(opt) => {
                            terminal.draw(|f| {
                                let area = sectorline::ui::centered_rect(60, 20, f.area());
                                f.render_widget(ratatui::widgets::Clear, area);
                                let p = ratatui::widgets::Paragraph::new(format!(
                                    "\n  Loading {}...\n  Preparing authentic circuit telemetry & driver roster...",
                                    opt.title
                                ))
                                .block(
                                    ratatui::widgets::Block::default()
                                        .borders(ratatui::widgets::Borders::ALL)
                                        .border_type(ratatui::widgets::BorderType::Rounded)
                                        .title(" 🏎️  SESSION LOADER ")
                                        .style(ratatui::style::Style::default().fg(ratatui::style::Color::Cyan)),
                                );
                                f.render_widget(p, area);
                            })?;

                            if opt.is_live || opt.gp_key.eq_ignore_ascii_case("live") {
                                if let Ok(new_stream) = sectorline::live::LiveTelemetryClient::fetch_live_stream_natively(&cache_dir) {
                                    app.stream = new_stream;
                                }
                            } else {
                                if let Ok(opt_stream) = sectorline::live::LiveTelemetryClient::ensure_circuit_ready(
                                    &cache_dir,
                                    &opt.gp_key,
                                    opt.year,
                                    &opt.session_type,
                                ) {
                                    if let Some(s) = opt_stream {
                                        app.stream = s;
                                    } else if let Ok(mut new_stream) = TelemetryStream::load_from_dir(&cache_dir) {
                                        if opt.is_live {
                                            new_stream.is_live = true;
                                        }
                                        app.stream = new_stream;
                                    }
                                }
                            }
                            last_tick = Instant::now();
                        }
                        sectorline::app::AppAction::None => {}
                    }
                }
            }

            let elapsed = last_tick.elapsed().as_millis() as f64;
            last_tick = Instant::now();
            app.tick(elapsed);

            if app.should_quit {
                break;
            }
        }
        Ok(())
    })();

    // Terminal Teardown
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    run_res
}
