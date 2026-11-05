use sectorline::{
    data::limits::Cancellation,
    loader::{Loader, LoaderEvent},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

fn wait<T: Send + 'static, R: Send + 'static>(loader: &Loader<R, T>) -> LoaderEvent<T> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(event) = loader.poll()
            && matches!(event, LoaderEvent::Finished { .. })
        {
            return event;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn loader_late_results_cannot_replace_new_selection() {
    let entered = Arc::new(AtomicBool::new(false));
    let flag = entered.clone();
    let loader = Loader::new(
        Duration::from_secs(1),
        move |request: u32, _cancel, progress| {
            flag.store(true, Ordering::SeqCst);
            progress("Working".into());
            if request == 1 {
                std::thread::sleep(Duration::from_millis(70));
            }
            Ok(request)
        },
    );
    let first = loader.start(1);
    while !entered.load(Ordering::SeqCst) {
        std::thread::yield_now();
    }
    let second = loader.start(2);
    assert_ne!(first, second);
    match wait(&loader) {
        LoaderEvent::Finished { id, result } => {
            assert_eq!(id, second);
            assert_eq!(result.unwrap(), 2);
        }
        _ => unreachable!(),
    }
}

#[test]
fn loader_has_one_active_job_and_only_latest_pending_request() {
    let started = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = started.clone();
    let loader = Loader::new(Duration::from_secs(1), move |n: u32, _cancel, progress| {
        log.lock().unwrap().push(n);
        if n == 0 {
            std::thread::sleep(Duration::from_millis(80));
        }
        for i in 0..10_000 {
            progress(format!("{i}"));
        }
        Ok(n)
    });
    loader.start(0);
    while started.lock().unwrap().is_empty() {
        std::thread::yield_now();
    }
    for n in 1..100 {
        loader.start(n);
    }
    match wait(&loader) {
        LoaderEvent::Finished { result, .. } => assert_eq!(result.unwrap(), 99),
        _ => unreachable!(),
    }
    assert_eq!(*started.lock().unwrap(), vec![0, 99]);
    assert!(loader.poll().is_none());
}

#[test]
fn loader_cancel_and_drop_never_wait_for_slow_io() {
    let entered = Arc::new(AtomicBool::new(false));
    let flag = entered.clone();
    let loader = Loader::new(Duration::from_secs(1), move |_: (), _cancel, _progress| {
        flag.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(200));
        Ok(())
    });
    loader.start(());
    while !entered.load(Ordering::SeqCst) {
        std::thread::yield_now();
    }
    let start = Instant::now();
    loader.cancel();
    drop(loader);
    assert!(start.elapsed() < Duration::from_millis(80));
}

#[test]
fn loader_job_deadline_and_errors_are_reported() {
    let loader = Loader::new(
        Duration::from_millis(25),
        |_: (), cancel: Cancellation, _progress| {
            loop {
                cancel.check()?;
                std::thread::sleep(Duration::from_millis(2));
            }
            #[allow(unreachable_code)]
            Ok(())
        },
    );
    loader.start(());
    match wait(&loader) {
        LoaderEvent::Finished { result, .. } => assert!(result.unwrap_err().contains("timed out")),
        _ => unreachable!(),
    }
}

fn demo_app() -> sectorline::App {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache");
    sectorline::App::new(
        sectorline::TelemetryStream::load_from_dir(dir).unwrap(),
        false,
    )
}
fn render(app: &sectorline::App, width: u16, height: u16) -> String {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
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
fn loader_app_retains_previous_session_and_ignores_stale_events() {
    use sectorline::loader::session::SessionResult;
    let mut app = demo_app();
    let name = app.stream.session.event_name.clone();
    app.begin_loading(2);
    app.accept_loader_event(LoaderEvent::Finished {
        id: 1,
        result: Err("stale error".into()),
    });
    assert_eq!(app.load_id, Some(2));
    assert!(!app.load_error);
    app.accept_loader_event(LoaderEvent::Finished::<SessionResult> {
        id: 2,
        result: Err("Provider unavailable; retry with r".into()),
    });
    assert_eq!(app.stream.session.event_name, name);
    assert!(app.load_error);
    for (w, h) in [(144, 44), (120, 36), (80, 24)] {
        let text = render(&app, w, h);
        assert!(text.contains("Error:"));
        assert!(text.contains("Provider unavailable"));
        assert!(text.contains("DEMO"));
        assert!(text.contains("r retry"));
    }
}

struct Temp(std::path::PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .join(format!("loader-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn catalogue(&self) -> std::path::PathBuf {
        let value = serde_json::json!({"version":1,"retrieved_at_ms":1000,"loaded_years":[2024],
            "meetings":[{"meeting_key":1244,"year":2024,"meeting_name":"Italian Grand Prix","circuit_short_name":"Monza","location":"Monza","country_name":"Italy"}],
            "sessions":[{"session_key":9590,"meeting_key":1244,"year":2024,"session_name":"Race","session_type":"Race","date_start":"2024-09-01T13:00:00Z","date_end":"2024-09-01T13:00:01Z"}]});
        let path = self.0.join("catalogue.json");
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn loader_slow_http_cancel_switch_resize_and_quit_trace() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use sectorline::{
        app::AppAction,
        data::limits::Limits,
        loader::session::{SessionRequest, SessionResult, WorkerConfig, session_loader},
    };
    use std::io::{Read, Write};
    let temp = Temp::new("slow");
    let catalogue = temp.catalogue();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let entered = Arc::new(AtomicBool::new(false));
    let flag = entered.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let server = std::thread::spawn(move || {
        while !stopping.load(Ordering::SeqCst) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 1024];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buffer).unwrap_or(0);
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..n]);
            }
            flag.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(300));
            let body = r#"[{"session_key":9590,"meeting_key":1244,"driver_number":4,"full_name":"Lando NORRIS","name_acronym":"NOR","team_name":"McLaren"}]"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            break;
        }
    });
    let demo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache");
    let loader = session_loader(WorkerConfig {
        demo_dir: demo,
        catalogue_path: catalogue,
        recording_root: temp.0.join("recordings"),
        limits: Limits::default(),
        endpoint,
        job_timeout: Duration::from_secs(5),
    });
    let mut app = demo_app();
    let id = loader.start(SessionRequest::Recording(9590));
    app.begin_loading(id);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !entered.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let start = Instant::now();
    let paused = app.stream.is_paused;
    app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    assert_ne!(app.stream.is_paused, paused);
    for (w, h) in [(144, 44), (80, 24)] {
        assert!(render(&app, w, h).contains("Loading:"));
    }
    assert_eq!(
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        AppAction::CancelLoad
    );
    loader.cancel();
    assert!(
        start.elapsed() < Duration::from_millis(150),
        "UI work waited for the 300ms HTTP response"
    );
    let next = loader.start(SessionRequest::Search("2024 Monza R".into()));
    app.begin_loading(next);
    let event = wait(&loader);
    assert!(
        matches!(&event,LoaderEvent::Finished{id,result:Ok(SessionResult::Search(_))} if *id==next)
    );
    app.accept_loader_event(event);
    assert_eq!(app.session_options[1].recording_key, Some(9590));
    app.show_session_modal = false;
    let quit = Instant::now();
    assert_eq!(
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        AppAction::Quit
    );
    loader.cancel();
    drop(loader);
    assert!(quit.elapsed() < Duration::from_millis(80));
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
}
