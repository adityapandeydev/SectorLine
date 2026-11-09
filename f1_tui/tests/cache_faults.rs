#[path = "support/faults.rs"]
mod support;
use sectorline::data::cache::SessionCache;
use serde_json::Value;
use support::Fixture;
#[test]
fn cache_faults_never_rewrite_damaged_input_or_publish_a_partial_replay() {
    for field in [
        "version",
        "records",
        "sha256",
        "bytes",
        "source_url",
        "channel",
        "session_key",
        "entries",
        "window",
    ] {
        let f = Fixture::complete();
        let p = f.directory().join("manifest.json");
        let mut m: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        match field {
            "version" => m["version"] = serde_json::json!(999),
            "records" => m["entries"][1][field] = serde_json::json!(usize::MAX),
            "sha256" => m["entries"][1][field] = serde_json::json!("0".repeat(64)),
            "bytes" => m["entries"][1][field] = serde_json::json!(0),
            "source_url" => {
                m["entries"][1]["metadata"][field] =
                    serde_json::json!("https://evil.invalid/?token=never-display-this")
            }
            "channel" => m["entries"][1]["metadata"][field] = serde_json::json!("Location"),
            "session_key" => m["entries"][1]["metadata"][field] = serde_json::json!(9636),
            "entries" => {
                m["entries"].as_array_mut().unwrap().pop();
            }
            _ => m["entries"][1]["metadata"][field]["end"] = serde_json::json!(1000),
        }
        let raw = serde_json::to_vec(&m).unwrap();
        std::fs::write(&p, &raw).unwrap();
        let err = f.open().err().expect(field).to_string();
        assert!(!err.contains("never-display-this"));
        assert!(!err.is_empty());
        assert_eq!(std::fs::read(p).unwrap(), raw);
    }
    for body in [b"[".as_slice(), b"[]".as_slice()] {
        let f = Fixture::complete();
        let p = f.directory().join("000001.json");
        std::fs::write(&p, body).unwrap();
        assert!(f.open().is_err());
        assert_eq!(std::fs::read(p).unwrap(), body);
    }
    let f = Fixture::complete();
    std::fs::remove_file(f.directory().join("000001.json")).unwrap();
    assert!(f.open().is_err());
}
#[cfg(windows)]
#[test]
fn cache_faults_reject_junction_session_directories_before_reading_external_files() {
    let source = Fixture::complete();
    let destination = Fixture::empty();
    let junction = destination.directory();
    let status=std::process::Command::new("powershell").args(["-NoProfile","-NonInteractive","-Command","New-Item -ItemType Junction -Path $env:SECTORLINE_T21_LINK -Target $env:SECTORLINE_T21_TARGET -ErrorAction Stop | Out-Null"]).env("SECTORLINE_T21_LINK",&junction).env("SECTORLINE_T21_TARGET",source.directory()).status().unwrap();
    assert!(status.success());
    let result = SessionCache::open(&destination.root, &destination.spec);
    // Remove only the junction entry; never recursively remove through it.
    std::fs::remove_dir(&junction).unwrap();
    assert!(source.open().is_ok());
    assert!(
        result.is_err(),
        "A cache junction must be rejected before following its target"
    );
}

#[cfg(unix)]
#[test]
fn cache_faults_reject_symbolic_session_directories() {
    let source = Fixture::complete();
    let destination = Fixture::empty();
    let link = destination.directory();
    std::os::unix::fs::symlink(source.directory(), &link).unwrap();
    let result = SessionCache::open(&destination.root, &destination.spec);
    std::fs::remove_file(link).unwrap();
    assert!(result.is_err());
    assert!(source.open().is_ok());
}
