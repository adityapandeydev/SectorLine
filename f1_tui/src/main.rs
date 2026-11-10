use crossterm::event::{self, Event};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::env;
use std::io::{IsTerminal, stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sectorline::app::App;
use sectorline::loader::session::{SessionRequest, WorkerConfig, session_loader_with_policy};
use sectorline::stream::TelemetryStream;
use sectorline::terminal::{PanicCleanup, Shutdown, SystemControl, TerminalSession};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use sectorline::cli::{Mode, Options};
    let args: Vec<String> = env::args().collect();
    let options = Options::parse(&args)?;
    match options.mode {
        Mode::Help => {
            print!("{}", sectorline::cli::HELP);
            return Ok(());
        }
        Mode::Version => {
            println!("Sector Line {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Mode::Catalogue => return sectorline::data::catalogue_cli::run(&args),
        _ => {}
    }
    if options.mode == Mode::Demo {
        let mut app = App::new(TelemetryStream::bundled_demo()?, false);
        println!("Sector Line - DEMO | Generated car movement and telemetry; not observed replay.");
        for step in 1..=20 {
            app.tick(200.0);
            let frame = app.stream.current_frame();
            let p1 = &frame.cars[0];
            println!(
                " Frame {step:2}/20 | Sim Time: {:.1}s | P1: {} (Speed: {:.0} km/h)",
                frame.timestamp_ms as f64 / 1000.0,
                p1.driver,
                p1.speed
            );
        }
        println!("[OK] Headless playback simulation passed successfully!");
        return Ok(());
    }
    let catalogue_path = match options.catalogue_path {
        Some(p) => p,
        None => sectorline::data::catalogue::default_cache_path()?,
    };
    let recording_root = match options.recording_root {
        Some(p) => p,
        None => sectorline::data::paths::session_cache_root().map_err(|e| e.to_string())?,
    };
    if options.mode == Mode::Paths {
        println!(
            "Catalogue: {}\nRecordings: {}\nDemo: bundled generated simulation (no disk cache)\nConfig: command flags; no config file required",
            catalogue_path.display(),
            recording_root.display()
        );
        return Ok(());
    }
    let config = WorkerConfig {
        demo_dir: PathBuf::new(),
        catalogue_path,
        recording_root,
        limits: sectorline::data::limits::Limits::default(),
        endpoint: "https://api.openf1.org/v1".into(),
        job_timeout: Duration::from_secs(15 * 60),
    };
    if let Mode::Download(key) = options.mode {
        let cancel = sectorline::data::limits::Cancellation::with_timeout(config.job_timeout);
        let replay = sectorline::loader::session::prepare_recording(
            &config,
            key,
            false,
            &cancel,
            &|message| eprintln!("{message}"),
        )
        .map_err(|e| e.to_string())?;
        println!(
            "Validated recorded snapshot #{} | {} drivers | {:.3}s observed coverage\nCache: {}\nReplay offline: sectorline --replay {key}",
            key,
            replay.timeline().drivers().len(),
            replay.timeline().end().0 as f64 / 1000.0,
            config.recording_root.display()
        );
        return Ok(());
    }
    let Mode::Interactive {
        session: prepare_key,
        offline,
    } = options.mode
    else {
        unreachable!()
    };
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err("Interactive mode needs a terminal. Use --demo for headless verification, --download ID for acquisition, or --catalogue for metadata.".into());
    }
    let mut app = App::new(TelemetryStream::bundled_demo()?, prepare_key.is_none());
    app.theme.ascii |= options.ascii;
    app.theme.color &= !options.no_color;
    let shutdown = Shutdown::install()?;
    let loader = session_loader_with_policy(config, offline);
    let mut last_request: Option<SessionRequest> = None;
    if let Some(key) = prepare_key {
        let request = if offline {
            SessionRequest::CachedRecording(key)
        } else {
            SessionRequest::Recording(key)
        };
        app.begin_loading(loader.start(request.clone()));
        last_request = Some(request);
        app.show_session_modal = false;
    }

    let _panic_cleanup = PanicCleanup::install();
    let mut terminal_session = TerminalSession::start(SystemControl)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let started = Instant::now();
    let mut last_draw = Instant::now();
    let mut dirty = true;
    let target_fps = 30;
    let tick_rate = Duration::from_millis(1000 / target_fps);

    // Main Interactive Render Loop
    let run_res = (|| -> Result<(), Box<dyn std::error::Error>> {
        loop {
            if shutdown.requested() {
                break;
            }
            dirty |= app.update_at(started.elapsed());
            if let Some(event) = loader.poll() {
                let failed = matches!(
                    &event,
                    sectorline::loader::LoaderEvent::Finished { result: Err(_), .. }
                );
                app.accept_loader_event(event);
                if failed {
                    terminal.clear()?;
                }
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
    let cleanup = terminal_session.restore();
    match (run_res, cleanup) {
        (Err(error), Err(cleanup)) => {
            Err(format!("{error}; terminal cleanup also failed: {cleanup}").into())
        }
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Ok(()), Ok(())) => Ok(()),
    }
}
