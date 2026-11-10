use std::{
    path::PathBuf,
    process::{Command, Output},
};
struct Fresh(PathBuf);
impl Fresh {
    fn new() -> Self {
        static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .join(format!(
                "t22-fresh-{}-{}",
                std::process::id(),
                ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        std::fs::create_dir(&path).unwrap();
        std::fs::copy(
            env!("CARGO_BIN_EXE_sectorline"),
            path.join(
                std::path::Path::new(env!("CARGO_BIN_EXE_sectorline"))
                    .file_name()
                    .unwrap(),
            ),
        )
        .unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(
            self.0.join(
                std::path::Path::new(env!("CARGO_BIN_EXE_sectorline"))
                    .file_name()
                    .unwrap(),
            ),
        )
        .args(args)
        .current_dir(&self.0)
        .env("PATH", "")
        .env("LOCALAPPDATA", self.0.join("profile"))
        .env("XDG_CACHE_HOME", self.0.join("profile"))
        .env("HOME", self.0.join("home"))
        .output()
        .unwrap()
    }
}
impl Drop for Fresh {
    fn drop(&mut self) {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .canonicalize()
            .unwrap();
        let path = self.0.canonicalize().unwrap();
        assert!(path.starts_with(&base) && path != base);
        std::fs::remove_dir_all(path).unwrap();
    }
}
#[test]
fn packaging_copied_binary_runs_builtin_demo_without_python_or_cache() {
    let f = Fresh::new();
    for _ in 0..2 {
        let o = f.run(&["--demo"]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let text = String::from_utf8_lossy(&o.stdout);
        assert!(text.contains("DEMO"));
        assert!(text.contains("20/20"));
        assert!(!text.contains("Loading generated demo cache"));
    }
    assert!(!f.0.join("profile").exists());
    // An unrelated, malformed source-shaped cache must not hijack the bundled demo.
    std::fs::create_dir(f.0.join("cache")).unwrap();
    std::fs::write(f.0.join("cache/session_info.json"), b"bad json").unwrap();
    assert!(f.run(&["--demo"]).status.success());
}
#[test]
fn packaging_help_version_and_paths_work_in_empty_directory() {
    let f = Fresh::new();
    let h = f.run(&["--help"]);
    assert!(h.status.success());
    let h = String::from_utf8_lossy(&h.stdout);
    for flag in ["--download", "--replay", "--paths", "--demo"] {
        assert!(h.contains(flag), "{h}");
    }
    let v = f.run(&["--version"]);
    assert!(v.status.success());
    assert!(String::from_utf8_lossy(&v.stdout).contains(env!("CARGO_PKG_VERSION")));
    let p = f.run(&["--paths"]);
    assert!(p.status.success());
    assert!(
        String::from_utf8_lossy(&p.stdout)
            .replace('\\', "/")
            .contains(
                &f.0.join("profile/sectorline")
                    .to_string_lossy()
                    .replace('\\', "/")
            )
    );
    assert!(!f.0.join("profile").exists());
}
#[test]
fn packaging_invalid_options_fail_before_loading_writing_or_network() {
    let f = Fresh::new();
    for args in [
        vec!["--replay"],
        vec!["--download", "0"],
        vec!["--demo", "--replay", "9590"],
        vec!["--demo", "--unknown"],
        vec!["--recording-root"],
        vec!["--demo", "--demo"],
        vec!["--download", "9590", "--offline"],
        vec!["--year", "oops"],
        vec!["--prepare-session", "9590", "--replay", "9590"],
    ] {
        let o = f.run(&args);
        assert!(!o.status.success(), "{args:?}");
        assert!(o.stdout.is_empty(), "{args:?}");
        assert!(!f.0.join("profile").exists());
    }
    let o = f.run(&[]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("needs a terminal"));
}

#[test]
fn packaging_native_download_can_be_replayed_without_provider_or_python() {
    use sectorline::{
        data::limits::{Cancellation, Limits},
        loader::{
            LoaderEvent,
            session::{
                SessionRequest, SessionResult, WorkerConfig, prepare_recording,
                session_loader_with_policy,
            },
        },
    };
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    let f = Fresh::new();
    let catalogue = f.0.join("catalogue.json");
    let data = serde_json::json!({"version":1,"retrieved_at_ms":1000,"loaded_years":[2024],"meetings":[{"meeting_key":1244,"year":2024,"meeting_name":"SYNTHETIC PACKAGING FIXTURE","circuit_short_name":"Test","location":"Test","country_name":"Test"}],"sessions":[{"session_key":9590,"meeting_key":1244,"year":2024,"session_name":"Race","session_type":"Race","date_start":"2024-09-01T13:00:00Z","date_end":"2024-09-01T13:00:01Z"}]});
    std::fs::write(&catalogue, serde_json::to_vec(&data).unwrap()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = stopped.clone();
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let server = std::thread::spawn(move || {
        let mut car_chunks = 0;
        let deadline = Instant::now() + Duration::from_secs(15);
        while !stop.load(Ordering::Relaxed) {
            assert!(Instant::now() < deadline, "Stub deadline expired");
            let mut stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut raw = Vec::new();
            let mut buf = [0; 2048];
            while !raw.windows(4).any(|b| b == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                assert!(raw.len() < 8192);
            }
            let text = String::from_utf8(raw).unwrap();
            assert!(!text.to_ascii_lowercase().contains("authorization:"));
            count.fetch_add(1, Ordering::Relaxed);
            let body = if text.starts_with("GET /drivers?") {
                serde_json::json!([{"meeting_key":1244,"session_key":9590,"driver_number":4,"name_acronym":"TST","full_name":"SYNTHETIC DRIVER","team_name":"Synthetic"}])
            } else if text.starts_with("GET /position?") {
                serde_json::json!([{"meeting_key":1244,"session_key":9590,"driver_number":4,"date":"2024-09-01T13:00:00Z","position":1}])
            } else if text.starts_with("GET /car_data?") {
                let n = car_chunks;
                car_chunks += 1;
                if n == 12 {
                    serde_json::json!([{"meeting_key":1244,"session_key":9590,"driver_number":4,"date":"2024-09-01T13:00:00Z","speed":200,"rpm":10000,"n_gear":5,"throttle":50,"brake":0,"drs":8}])
                } else {
                    serde_json::json!([])
                }
            } else {
                serde_json::json!([])
            };
            let body = body.to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    let mut config = WorkerConfig {
        demo_dir: PathBuf::new(),
        catalogue_path: catalogue,
        recording_root: f.0.join("recordings"),
        limits: Limits {
            request_spacing: Duration::from_millis(1),
            ..Default::default()
        },
        endpoint,
        job_timeout: Duration::from_secs(15),
    };
    let r = prepare_recording(&config, 9590, false, &Cancellation::default(), &|_| {});
    stopped.store(true, Ordering::Relaxed);
    server.join().unwrap();
    let r = r.unwrap();
    assert!(requests.load(Ordering::Relaxed) > 70);
    assert_eq!(
        r.frame().samples()[0].speed_kmh.value().unwrap().get(),
        200.0
    );
    // All subsequent access is forbidden even if the endpoint would be invalid.
    config.endpoint = "https://must-not-be-used.invalid".into();
    let cached = prepare_recording(&config, 9590, true, &Cancellation::default(), &|_| {}).unwrap();
    assert_eq!(r.frame(), cached.frame());
    let original = config.recording_root.join("openf1-9590");
    let held = config.recording_root.join("held-fixture");
    std::fs::rename(&original, &held).unwrap();
    let loader = session_loader_with_policy(config, true);
    loader.start(SessionRequest::Recording(9590));
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(LoaderEvent::Finished { result, .. }) = loader.poll() {
            let error = result.err().unwrap();
            assert!(
                error.contains("Offline: completed recording 9590 is not cached"),
                "{error}"
            );
            assert!(!original.exists());
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    std::fs::rename(held, &original).unwrap();
    loader.start(SessionRequest::Demo);
    loop {
        if let Some(LoaderEvent::Finished { result, .. }) = loader.poll() {
            assert!(matches!(result, Ok(SessionResult::Demo(_))));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn packaging_explicit_paths_do_not_require_default_profile_directories() {
    let f = Fresh::new();
    let binary = f.0.join(
        std::path::Path::new(env!("CARGO_BIN_EXE_sectorline"))
            .file_name()
            .unwrap(),
    );
    let output = Command::new(binary)
        .args([
            "--paths",
            "--catalogue-cache",
            "custom catalogue.json",
            "--recording-root",
            "custom recordings",
        ])
        .current_dir(&f.0)
        .env_remove("LOCALAPPDATA")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("HOME")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("custom catalogue.json"));
    assert!(text.contains("custom recordings"));
}
