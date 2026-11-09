#[path = "support/faults.rs"]
mod support;
use sectorline::{
    data::{
        limits::{Cancellation, Limits},
        openf1::{AcquisitionRequest, Channel, OpenF1Client, Window},
    },
    domain::{
        capabilities::SessionContext,
        normalize::{Event, Payload, RosterDriver, TelemetryValues},
        *,
    },
    loader::{LoaderEvent, session::SessionResult},
    replay::{RecordedReplay, timeline::Timeline},
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    time::Duration,
};
use support::meta;
pub fn recording() -> RecordedReplay {
    let id = SessionId::new(Provider::OpenF1, "9590").unwrap();
    let t = Timeline::new(
        id.clone(),
        UtcMillis(0),
        vec![RosterDriver::new(4, "TST", "SYNTHETIC DRIVER").unwrap()],
        vec![meta(Channel::CarData, None)],
        [0, 10000]
            .into_iter()
            .map(|at| {
                Event::new(
                    4,
                    UtcMillis(at),
                    Availability::Known(UtcMillis(at)),
                    0,
                    Payload::Telemetry(TelemetryValues {
                        speed: Some(SpeedKmh::new(200.0).unwrap()),
                        ..Default::default()
                    }),
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap();
    RecordedReplay::new(t, "SYNTHETIC PREVIOUS RECORDING".into())
        .unwrap()
        .with_context(SessionContext::from_names(id, "Race", "Race"))
        .unwrap()
}
pub fn app() -> sectorline::App {
    let mut a = sectorline::App::new(
        sectorline::TelemetryStream::load_from_dir(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../python_bridge/cache"),
        )
        .unwrap(),
        false,
    );
    a.prepared_recording = Some(recording());
    a
}

fn preserved(error: String) {
    let mut a = app();
    let before = serde_json::to_vec(a.prepared_recording.as_ref().unwrap().frame()).unwrap();
    a.begin_loading(77);
    a.accept_loader_event(LoaderEvent::Finished::<SessionResult> {
        id: 77,
        result: Err(error),
    });
    assert!(a.load_error);
    assert!(
        a.load_message
            .as_ref()
            .unwrap()
            .contains("Previous session retained")
    );
    assert_eq!(
        serde_json::to_vec(a.prepared_recording.as_ref().unwrap().frame()).unwrap(),
        before
    );
    for (w, h) in [(144, 44), (120, 36), (80, 24)] {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        t.draw(|f| a.render(f)).unwrap();
        let text = t
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("Error:"));
        assert!(text.contains("SYNTHETIC PREVIOUS RECORDING"));
    }
    a.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char(' '),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(
        a.prepared_recording.as_ref().unwrap().status(),
        sectorline::replay::clock::PlaybackStatus::Playing
    );
    a.update_at(Duration::from_millis(100));
    assert_eq!(
        a.prepared_recording.as_ref().unwrap().frame().elapsed(),
        ElapsedMillis(100)
    );
}
#[test]
fn provider_faults_keep_previous_recording_and_expose_reasons() {
    for (status, body, extra, why) in [
        (429, "[]", "Retry-After: 0\r\n", "HTTP 429"),
        (503, "[]", "Retry-After: 0\r\n", "HTTP 503"),
        (
            302,
            "[]",
            "Location: https://evil.invalid/private\r\n",
            "302",
        ),
        (200, "[{", "", "read"),
        (200, "[{\"session_key\":9590}]", "", "field"),
        (
            200,
            "[{\"session_key\":9590,\"meeting_key\":1244,\"driver_number\":4,\"full_name\":1,\"name_acronym\":\"TST\",\"team_name\":\"Synthetic\"}]",
            "",
            "text",
        ),
        (200, "[{\"speed\":NaN}]", "", "JSON"),
        (200, "[{\"speed\":1e999}]", "", "range"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.windows(4).any(|b| b == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                assert!(request.len() < 8192);
            }
            let size = body.len() + if body == "[{" { 50 } else { 0 };
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Length: {size}\r\nConnection: close\r\n{extra}\r\n{body}"
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        let limits = Limits {
            max_retries: 0,
            request_spacing: Duration::from_millis(1),
            connect_timeout: Duration::from_millis(100),
            request_timeout: Duration::from_secs(2),
            ..Default::default()
        };
        let mut client = OpenF1Client::with_endpoint(&endpoint, limits).unwrap();
        let req = AcquisitionRequest::new(
            9590,
            1244,
            Window::new(UtcMillis(0), UtcMillis(2000)).unwrap(),
        )
        .unwrap();
        let error = client
            .acquire_channels(&req, &[Channel::Drivers], &Cancellation::default(), |_| {
                panic!("Faulty response must not be published")
            })
            .unwrap_err()
            .to_string();
        server.join().unwrap();
        assert!(
            error.to_lowercase().contains(&why.to_lowercase()),
            "{status}: {error}"
        );
        assert!(!error.contains("evil.invalid"));
        preserved(error);
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let mut client = OpenF1Client::with_endpoint(
        &endpoint,
        Limits {
            max_retries: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let req = AcquisitionRequest::new(
        9590,
        1244,
        Window::new(UtcMillis(0), UtcMillis(2000)).unwrap(),
    )
    .unwrap();
    preserved(
        client
            .acquire_channels(&req, &[Channel::Drivers], &Cancellation::default(), |_| {
                panic!("Offline data must not publish")
            })
            .unwrap_err()
            .to_string(),
    );
    let f = support::Fixture::complete();
    std::fs::write(f.directory().join("000001.json"), b"[]").unwrap();
    preserved(f.open().err().unwrap().to_string());
}
#[test]
fn provider_faults_quarantine_bad_numeric_fields_and_accept_additive_schema() {
    let f = support::Fixture::empty();
    let mut writer = sectorline::data::cache::CacheWriter::begin(&f.root, &f.spec).unwrap();
    writer
        .import(
            meta(Channel::Drivers, None),
            &serde_json::to_vec(&support::drivers()).unwrap(),
        )
        .unwrap();
    let mut raw = support::car();
    raw[0]["speed"] = serde_json::json!(-1);
    raw[0]["throttle"] = serde_json::json!(104);
    raw[0]["future_field"] = serde_json::json!({"anything":true});
    writer
        .import(
            meta(
                Channel::CarData,
                Some(Window::new(UtcMillis(0), UtcMillis(2000)).unwrap()),
            ),
            &serde_json::to_vec(&raw).unwrap(),
        )
        .unwrap();
    let cache = writer.finish().unwrap();
    let validated = cache.read_chunk(1).unwrap();
    assert_eq!(validated.issues().len(), 2);
    assert_eq!(
        validated.records()[0]["future_field"],
        raw[0]["future_field"]
    );
    let mut r = f.open().unwrap();
    r.seek(ElapsedMillis(0)).unwrap();
    let c = &r.frame().samples()[0];
    assert!(c.speed_kmh.value().is_none());
    assert!(c.throttle.value().is_none());
    assert_eq!(c.gear.value().unwrap().get(), 5);
}

#[test]
fn provider_faults_reject_untrusted_endpoints_and_bound_visible_diagnostics() {
    for endpoint in [
        "http://api.openf1.org/v1",
        "https://user:secret@example.com",
        "http://127.0.0.1:80/path",
        "http://127.0.0.1:80?token=secret",
        "http://127.0.0.1:0",
        "https://api.openf1.org/v1/../private",
    ] {
        let error = OpenF1Client::with_endpoint(endpoint, Limits::default())
            .err()
            .expect(endpoint)
            .to_string();
        assert!(!error.contains("secret"));
    }
    let text = format!("\u{1b}[2J\r\n{}\u{7}", "x".repeat(2000));
    let message = sectorline::loader::display_message(&text);
    assert!(message.chars().count() <= 400);
    assert!(!message.chars().any(char::is_control));
}
