use sectorline::{
    data::{
        limits::{Cancellation, Limits},
        openf1::{AcquisitionRequest, Channel, OpenF1Client, Window},
    },
    domain::UtcMillis,
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

// Synthetic HTTP responses exercise the real request/reader/validation path.
struct Reply {
    status: u16,
    headers: String,
    body: String,
    truncate: bool,
    delay: Duration,
}
impl Reply {
    fn json(value: Value) -> Self {
        Self {
            status: 200,
            headers: String::new(),
            body: value.to_string(),
            truncate: false,
            delay: Duration::ZERO,
        }
    }
    fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::json(json!([]))
        }
    }
}
struct Server {
    url: String,
    requests: Arc<Mutex<Vec<(Instant, String)>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let handle = thread::spawn(move || {
            let mut replies = replies.into_iter();
            while !stopping.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(_) => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                };
                // Windows accepted sockets can inherit the listener's nonblocking mode.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 1024];
                while !bytes.windows(4).any(|w| w == b"\r\n\r\n") && bytes.len() < 8192 {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => bytes.extend_from_slice(&buffer[..n]),
                    }
                }
                // Consume complete headers before closing, avoiding a TCP reset
                // from unread request bytes when the OS splits the header writes.
                if !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    continue;
                }
                log.lock()
                    .unwrap()
                    .push((Instant::now(), String::from_utf8_lossy(&bytes).to_string()));
                let Some(reply) = replies.next() else {
                    break;
                };
                thread::sleep(reply.delay);
                let length = reply.body.len() + if reply.truncate { 50 } else { 0 };
                let response = format!(
                    "HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
                    reply.status, length, reply.headers, reply.body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            url,
            requests,
            stop,
            handle: Some(handle),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.handle.take().unwrap().join().unwrap();
    }
}
fn limits() -> Limits {
    Limits {
        request_spacing: Duration::from_millis(5),
        retry_backoff: Duration::from_millis(5),
        max_retries: 2,
        ..Limits::default()
    }
}
fn request() -> AcquisitionRequest {
    AcquisitionRequest::new(
        9590,
        1244,
        Window::new(UtcMillis(1_000), UtcMillis(3_000)).unwrap(),
    )
    .unwrap()
}
fn driver() -> Value {
    json!({"session_key":9590,"meeting_key":1244,"driver_number":4,"full_name":"Lando NORRIS","name_acronym":"NOR","team_name":"McLaren"})
}
fn car(time: &str) -> Value {
    json!({"session_key":9590,"meeting_key":1244,"driver_number":4,"date":time,"speed":200,"rpm":10000,"n_gear":5,"throttle":99,"brake":0,"drs":9})
}
fn client(server: &Server, limits: Limits) -> OpenF1Client {
    OpenF1Client::with_endpoint(&server.url, limits).unwrap()
}

#[test]
fn acquisition_streams_bounded_half_open_chunks_with_provenance() {
    let server = Server::new(vec![
        Reply::json(json!([driver()])),
        Reply::json(json!([car("1970-01-01T00:00:01Z")])),
        Reply::json(json!([car("1970-01-01T00:00:02Z")])),
    ]);
    let mut limits = limits();
    limits.chunk_span = Duration::from_secs(1);
    let mut client = client(&server, limits);
    let mut chunks = Vec::new();
    client
        .acquire_channels(
            &request(),
            &[Channel::Drivers, Channel::CarData],
            &Cancellation::default(),
            |c| {
                chunks.push(c);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(chunks.len(), 3);
    assert_eq!(
        chunks[1].window(),
        Some(Window::new(UtcMillis(1000), UtcMillis(2000)).unwrap())
    );
    assert_eq!(
        chunks[2].window(),
        Some(Window::new(UtcMillis(2000), UtcMillis(3000)).unwrap())
    );
    assert_eq!(chunks[1].records()[0]["drs"], 9); // Unknown code retained.
    assert_eq!(chunks[1].session_key(), 9590);
    assert!(chunks[1].retrieved_at().0 > 0);
    assert!(!chunks[1].raw_bytes().is_empty());
    let log = server.requests.lock().unwrap();
    assert!(log[1].1.contains("session_key=9590"));
    assert!(log[1].1.contains("date"));
}

#[test]
fn acquisition_retries_transient_errors_and_paces_every_attempt() {
    let server = Server::new(vec![Reply::status(503), Reply::json(json!([driver()]))]);
    let mut limits = limits();
    limits.request_spacing = Duration::from_millis(40);
    let mut client = client(&server, limits);
    client
        .acquire_channels(
            &request(),
            &[Channel::Drivers],
            &Cancellation::default(),
            |_| Ok(()),
        )
        .unwrap();
    let log = server.requests.lock().unwrap();
    assert_eq!(log.len(), 2);
    assert!(log[1].0.duration_since(log[0].0) >= Duration::from_millis(35));
}

#[test]
fn acquisition_honors_retry_after_and_stops_at_retry_budget() {
    let mut limited = Reply::status(429);
    limited.headers = "Retry-After: 1\r\n".into();
    let server = Server::new(vec![limited, Reply::json(json!([driver()]))]);
    let mut client = client(&server, limits());
    client
        .acquire_channels(
            &request(),
            &[Channel::Drivers],
            &Cancellation::default(),
            |_| Ok(()),
        )
        .unwrap();
    let log = server.requests.lock().unwrap();
    assert!(log[1].0.duration_since(log[0].0) >= Duration::from_millis(980));
    drop(log);
    let server = Server::new(vec![
        Reply::status(500),
        Reply::status(500),
        Reply::status(500),
    ]);
    let error = self::client(&server, limits())
        .acquire_channels(
            &request(),
            &[Channel::Drivers],
            &Cancellation::default(),
            |_| Ok(()),
        )
        .unwrap_err();
    assert!(error.to_string().contains("retry"));
    assert_eq!(server.requests.lock().unwrap().len(), 3);
}

#[test]
fn acquisition_cancelled_between_chunks_does_not_request_or_accept_more() {
    let server = Server::new(vec![Reply::json(json!([driver()]))]);
    let cancel = Cancellation::default();
    let mut accepted = 0;
    let error = client(&server, limits())
        .acquire_channels(
            &request(),
            &[Channel::Drivers, Channel::CarData],
            &cancel,
            |_| {
                accepted += 1;
                cancel.cancel();
                Ok(())
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert_eq!(accepted, 1);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn acquisition_empty_window_is_not_a_missing_endpoint_or_session() {
    let server = Server::new(vec![
        Reply::json(json!([driver()])),
        Reply::status(404),
        Reply::json(json!([car("1970-01-01T00:00:02Z")])),
    ]);
    let mut limits = limits();
    limits.chunk_span = Duration::from_secs(1);
    let mut statuses = Vec::new();
    client(&server, limits)
        .acquire_channels(
            &request(),
            &[Channel::Drivers, Channel::CarData],
            &Cancellation::default(),
            |c| {
                statuses.push((c.records().len(), c.empty_reason().map(str::to_owned)));
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(statuses[1].0, 0);
    assert!(statuses[1].1.as_ref().unwrap().contains("404"));
    assert_eq!(statuses[2].0, 1);
}

#[test]
fn acquisition_rejects_identity_schema_and_range_before_acceptance() {
    for defect in ["session", "meeting", "driver", "date", "schema", "window"] {
        let mut row = car("1970-01-01T00:00:01Z");
        match defect {
            "session" => row["session_key"] = json!(9636),
            "meeting" => row["meeting_key"] = json!(999),
            "driver" => row["driver_number"] = json!(99),
            "date" => row["date"] = json!("not a timestamp"),
            "schema" => {
                row.as_object_mut().unwrap().remove("speed");
            }
            "window" => row["date"] = json!("1970-01-01T00:00:03Z"),
            _ => unreachable!(),
        }
        let server = Server::new(vec![
            Reply::json(json!([driver()])),
            Reply::json(json!([car("1970-01-01T00:00:01Z"), row])),
        ]);
        let mut accepted = 0;
        assert!(
            client(&server, limits())
                .acquire_channels(
                    &request(),
                    &[Channel::Drivers, Channel::CarData],
                    &Cancellation::default(),
                    |_| {
                        accepted += 1;
                        Ok(())
                    }
                )
                .is_err(),
            "{defect}"
        );
        assert_eq!(accepted, 1, "{defect}");
    }
}

#[test]
fn acquisition_quarantines_bad_fields_and_preserves_raw_evidence() {
    let mut row = car("1970-01-01T00:00:01Z");
    row["throttle"] = json!(104);
    row["brake"] = json!(104);
    let server = Server::new(vec![
        Reply::json(json!([driver()])),
        Reply::json(json!([row])),
    ]);
    let mut chunks = Vec::new();
    client(&server, limits())
        .acquire_channels(
            &request(),
            &[Channel::Drivers, Channel::CarData],
            &Cancellation::default(),
            |c| {
                chunks.push(c);
                Ok(())
            },
        )
        .unwrap();
    assert!(chunks[1].records()[0]["throttle"].is_null());
    assert!(chunks[1].records()[0]["brake"].is_null());
    assert_eq!(chunks[1].issues().len(), 2);
    assert_eq!(
        serde_json::from_slice::<Value>(chunks[1].raw_bytes()).unwrap()[0]["throttle"],
        104
    );
}

#[test]
fn acquisition_never_accepts_truncated_oversized_or_error_bodies() {
    let mut truncated = Reply::json(json!([driver()]));
    truncated.truncate = true;
    for reply in [
        truncated,
        Reply {
            body: "{".into(),
            ..Reply::json(json!([]))
        },
        Reply::status(401),
    ] {
        let server = Server::new(vec![reply]);
        let mut limits = limits();
        limits.max_retries = 0;
        let mut accepted = 0;
        assert!(
            client(&server, limits)
                .acquire_channels(
                    &request(),
                    &[Channel::Drivers],
                    &Cancellation::default(),
                    |_| {
                        accepted += 1;
                        Ok(())
                    }
                )
                .is_err()
        );
        assert_eq!(accepted, 0);
    }
    let server = Server::new(vec![Reply::json(json!([driver()]))]);
    let mut limits = limits();
    limits.max_response_bytes = 10;
    assert!(
        client(&server, limits)
            .acquire_channels(
                &request(),
                &[Channel::Drivers],
                &Cancellation::default(),
                |_| Ok(())
            )
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
}

#[test]
fn acquisition_request_and_limits_reject_invalid_configuration() {
    assert!(Window::new(UtcMillis(1), UtcMillis(1)).is_err());
    assert!(AcquisitionRequest::new(0, 1244, request().window()).is_err());
    let mut limits = limits();
    limits.chunk_span = Duration::ZERO;
    assert!(OpenF1Client::new(limits).is_err());
    assert!(OpenF1Client::with_endpoint("http://example.com", self::limits()).is_err());
}

#[test]
fn acquisition_preserves_optional_missing_and_retrospective_channels() {
    let identity = json!({"session_key":9590,"meeting_key":1244,"driver_number":4});
    let mut position = identity.clone();
    position["date"] = json!("1970-01-01T00:00:00Z");
    position["position"] = json!(1);
    let mut location = identity.clone();
    location["date"] = json!("1970-01-01T00:00:01Z");
    location["x"] = json!(-100);
    location["y"] = Value::Null;
    location["z"] = json!(0);
    let mut interval = identity.clone();
    interval["date"] = json!("1970-01-01T00:00:01Z");
    interval["interval"] = json!("+1 LAP");
    interval["gap_to_leader"] = Value::Null;
    let mut stint = identity.clone();
    stint["stint_number"] = json!(1);
    stint["lap_start"] = json!(1);
    stint["lap_end"] = Value::Null;
    stint["tyre_age_at_start"] = json!(0);
    stint["compound"] = json!("FUTURE_COMPOUND");
    let server = Server::new(vec![
        Reply::json(json!([driver()])),
        Reply::json(json!([position])),
        Reply::json(json!([location])),
        Reply::json(json!([interval])),
        Reply::json(json!([stint])),
        Reply::status(404),
    ]);
    let mut chunks = Vec::new();
    client(&server, limits())
        .acquire_channels(
            &request(),
            &[
                Channel::Drivers,
                Channel::Position,
                Channel::Location,
                Channel::Intervals,
                Channel::Stints,
                Channel::Weather,
            ],
            &Cancellation::default(),
            |c| {
                chunks.push(c);
                Ok(())
            },
        )
        .unwrap();
    assert!(chunks[1].window().is_none()); // Position events may predate scheduled start.
    assert_eq!(chunks[2].records()[0]["x"], -100);
    assert!(chunks[2].records()[0]["y"].is_null());
    assert_eq!(chunks[3].records()[0]["interval"], "+1 LAP");
    assert!(chunks[3].records()[0]["gap_to_leader"].is_null());
    assert!(chunks[4].records()[0].get("date").is_none());
    assert!(chunks[5].empty_reason().is_some());
}

#[test]
fn acquisition_can_cancel_during_provider_backoff() {
    let mut response = Reply::status(429);
    response.headers = "Retry-After: 60\r\n".into();
    let server = Server::new(vec![response]);
    let cancel = Cancellation::default();
    let other = cancel.clone();
    let log = server.requests.clone();
    let trigger = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while log.lock().unwrap().is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(30));
        other.cancel();
    });
    let start = Instant::now();
    let error = client(&server, limits())
        .acquire_channels(&request(), &[Channel::Drivers], &cancel, |_| Ok(()))
        .unwrap_err();
    trigger.join().unwrap();
    assert!(error.to_string().contains("cancelled"));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn acquisition_recovers_from_truncated_response_but_not_from_invalid_json() {
    let mut bad = Reply::json(json!([driver()]));
    bad.truncate = true;
    let server = Server::new(vec![bad, Reply::json(json!([driver()]))]);
    let mut chunks = Vec::new();
    client(&server, limits())
        .acquire_channels(
            &request(),
            &[Channel::Drivers],
            &Cancellation::default(),
            |c| {
                chunks.push(c);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(chunks.len(), 1);
    // Windows may abort a loopback connection before sending complete headers.
    // Require recovery within the configured budget, without assuming no other
    // transport failure occurred before the intentional truncated response.
    assert!((2..=3).contains(&chunks[0].attempts()));
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    assert_eq!(chunks[0].records().len(), 1);
    let server = Server::new(vec![Reply {
        body: "[broken".into(),
        ..Reply::json(json!([]))
    }]);
    assert!(
        client(&server, limits())
            .acquire_channels(
                &request(),
                &[Channel::Drivers],
                &Cancellation::default(),
                |_| Ok(())
            )
            .is_err()
    );
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn acquisition_timeout_row_limit_and_sink_error_stop_delivery() {
    let server = Server::new(vec![Reply {
        delay: Duration::from_millis(150),
        ..Reply::json(json!([driver()]))
    }]);
    let mut policy = limits();
    policy.request_timeout = Duration::from_millis(40);
    policy.connect_timeout = Duration::from_millis(40);
    policy.max_retries = 0;
    let error = client(&server, policy)
        .acquire_channels(
            &request(),
            &[Channel::Drivers],
            &Cancellation::default(),
            |_| Ok(()),
        )
        .unwrap_err();
    assert!(error.to_string().contains("transport"), "{error}");
    let mut second = driver();
    second["driver_number"] = json!(16);
    let server = Server::new(vec![Reply::json(json!([driver(), second]))]);
    let mut policy = limits();
    policy.max_rows = 1;
    assert!(
        client(&server, policy)
            .acquire_channels(
                &request(),
                &[Channel::Drivers],
                &Cancellation::default(),
                |_| Ok(())
            )
            .unwrap_err()
            .to_string()
            .contains("row limit")
    );
    let server = Server::new(vec![Reply::json(json!([driver()]))]);
    let error = client(&server, limits())
        .acquire_channels(
            &request(),
            &[Channel::Drivers, Channel::Position],
            &Cancellation::default(),
            |_| Err("output disk full".into()),
        )
        .unwrap_err();
    assert!(error.to_string().contains("disk full"));
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn acquisition_retry_after_http_date_and_excessive_delay_are_respected() {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        + 2000;
    let date = chrono::DateTime::from_timestamp_millis(millis)
        .unwrap()
        .format("%a, %d %b %Y %H:%M:%S GMT");
    let mut response = Reply::status(503);
    response.headers = format!("Retry-After: {date}\r\n");
    let server = Server::new(vec![response, Reply::json(json!([driver()]))]);
    client(&server, limits())
        .acquire_channels(
            &request(),
            &[Channel::Drivers],
            &Cancellation::default(),
            |_| Ok(()),
        )
        .unwrap();
    let log = server.requests.lock().unwrap();
    assert!(log[1].0.duration_since(log[0].0) >= Duration::from_millis(900));
    drop(log);
    let mut response = Reply::status(429);
    response.headers = "Retry-After: 99999\r\n".into();
    let server = Server::new(vec![response]);
    assert!(
        client(&server, limits())
            .acquire_channels(
                &request(),
                &[Channel::Drivers],
                &Cancellation::default(),
                |_| Ok(())
            )
            .unwrap_err()
            .to_string()
            .contains("Retry-After")
    );
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn acquisition_resumes_cache_with_only_missing_http_chunks() {
    use sectorline::data::{
        cache::{CacheSpec, CacheWriter, SessionCache},
        openf1::{ChunkStore, Result, ValidatedChunk},
    };
    struct StopAfterTwo<'a> {
        writer: &'a mut CacheWriter,
        cancel: &'a Cancellation,
    }
    impl ChunkStore for StopAfterTwo<'_> {
        fn prepare(&self, r: &AcquisitionRequest, c: &[Channel], span: Duration) -> Result<()> {
            self.writer.prepare(r, c, span)
        }
        fn restore(&mut self, c: Channel, w: Option<Window>) -> Result<Option<ValidatedChunk>> {
            self.writer.restore(c, w)
        }
        fn accept(&mut self, chunk: ValidatedChunk) -> Result<()> {
            self.writer.accept(chunk)?;
            if self.writer.completed_chunks() == 2 {
                self.cancel.cancel();
            }
            Ok(())
        }
    }
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target")
        .join(format!("http-cache-resume-{}", std::process::id()));
    let channels = vec![Channel::Drivers, Channel::CarData];
    let spec = CacheSpec::new(
        9590,
        1244,
        2024,
        ("Race", "Race"),
        request().window(),
        Duration::from_secs(1),
        channels.clone(),
    )
    .unwrap();
    let server = Server::new(vec![
        Reply::json(json!([driver()])),
        Reply::json(json!([car("1970-01-01T00:00:01Z")])),
    ]);
    let mut policy = limits();
    policy.chunk_span = Duration::from_secs(1);
    {
        let mut writer = CacheWriter::begin(&root, &spec).unwrap();
        let cancel = Cancellation::default();
        let error = client(&server, policy.clone())
            .acquire_with_store(
                &request(),
                &channels,
                &cancel,
                &mut StopAfterTwo {
                    writer: &mut writer,
                    cancel: &cancel,
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
    }
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    let server = Server::new(vec![Reply::json(json!([car("1970-01-01T00:00:02Z")]))]);
    let mut writer = CacheWriter::begin(&root, &spec).unwrap();
    let mut wrong_policy = policy.clone();
    wrong_policy.chunk_span = Duration::from_secs(2);
    assert!(
        client(&server, wrong_policy)
            .acquire_with_store(&request(), &channels, &Cancellation::default(), &mut writer)
            .unwrap_err()
            .to_string()
            .contains("settings")
    );
    assert!(server.requests.lock().unwrap().is_empty());
    client(&server, policy)
        .acquire_with_store(&request(), &channels, &Cancellation::default(), &mut writer)
        .unwrap();
    writer.finish().unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    assert_eq!(SessionCache::open(&root, &spec).unwrap().chunk_count(), 3);
    std::fs::remove_dir_all(root).unwrap();
}
