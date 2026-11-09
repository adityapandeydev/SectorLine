use sectorline::{
    data::{
        cache::{CacheSpec, CacheWriter, SessionCache},
        openf1::{Channel, ChunkMetadata, Window},
    },
    domain::UtcMillis,
};
use serde_json::json;
use std::{path::PathBuf, time::Duration};

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .join(format!("cache-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn spec() -> CacheSpec {
    CacheSpec::new(
        9590,
        1244,
        2024,
        ("Race", "Race"),
        Window::new(UtcMillis(1000), UtcMillis(3000)).unwrap(),
        Duration::from_secs(1),
        vec![Channel::Drivers, Channel::CarData],
    )
    .unwrap()
}
fn raw(index: usize) -> Vec<u8> {
    if index == 0 {
        serde_json::to_vec(&json!([{"session_key":9590,"meeting_key":1244,"driver_number":4,"full_name":"Lando NORRIS","name_acronym":"NOR","team_name":"McLaren","team_colour":"FF8000"}])).unwrap()
    } else {
        serde_json::to_vec(&json!([{"session_key":9590,"meeting_key":1244,"driver_number":4,"date":format!("1970-01-01T00:00:0{index}Z"),"speed":200,"rpm":10000,"n_gear":5,"throttle":104,"brake":0,"drs":9}])).unwrap()
    }
}
fn metadata(index: usize) -> ChunkMetadata {
    let (channel, window, source) = if index == 0 {
        (
            Channel::Drivers,
            None,
            "https://api.openf1.org/v1/drivers?session_key=9590".to_owned(),
        )
    } else {
        (
            Channel::CarData,
            Some(
                Window::new(
                    UtcMillis(index as i64 * 1000),
                    UtcMillis((index + 1) as i64 * 1000),
                )
                .unwrap(),
            ),
            format!(
                "https://api.openf1.org/v1/car_data?session_key=9590&date>=1970-01-01T00:00:0{index}.000&date<1970-01-01T00:00:0{}.000",
                index + 1
            ),
        )
    };
    ChunkMetadata {
        channel,
        session_key: 9590,
        meeting_key: 1244,
        window,
        source_url: source,
        retrieved_at: UtcMillis(1_800_000_000_000),
        status: 200,
        attempts: 1,
    }
}
fn append(writer: &mut CacheWriter, index: usize) {
    writer.import(metadata(index), &raw(index)).unwrap();
}

#[test]
fn cache_is_invisible_until_complete_and_resumes_without_duplicate_chunks() {
    let root = Temp::new("resume");
    let spec = spec();
    {
        let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
        append(&mut writer, 0);
        append(&mut writer, 1);
        assert!(writer.finish().is_err());
    }
    assert!(SessionCache::open(&root.0, &spec).is_err());
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    assert_eq!(writer.completed_chunks(), 2);
    append(&mut writer, 1); // Idempotent replay of an already committed chunk.
    assert_eq!(writer.completed_chunks(), 2);
    append(&mut writer, 2);
    writer.finish().unwrap();
    let cache = SessionCache::open(&root.0, &spec).unwrap();
    assert_eq!(cache.chunk_count(), 3);
    let car = cache.read_chunk(1).unwrap();
    assert!(car.records()[0]["throttle"].is_null());
    assert_eq!(car.issues().len(), 1);
    assert_eq!(car.raw_bytes(), raw(1));
    let timeline = sectorline::replay::timeline::Timeline::from_cache(
        &cache,
        &sectorline::data::limits::Cancellation::default(),
        &|_, _| {},
    )
    .unwrap();
    assert_eq!(timeline.drivers()[0].team.as_deref(), Some("McLaren"));
    assert_eq!(timeline.drivers()[0].team_color, Some([255, 128, 0]));
}

#[test]
fn cache_rejects_wrong_year_type_identity_and_coverage() {
    let root = Temp::new("identity");
    let expected = spec();
    let mut writer = CacheWriter::begin(&root.0, &expected).unwrap();
    for index in 0..3 {
        append(&mut writer, index);
    }
    writer.finish().unwrap();
    for field in [
        "year",
        "session_type",
        "session_name",
        "meeting_key",
        "window",
    ] {
        let mut changed = serde_json::to_value(&expected).unwrap();
        match field {
            "year" => changed[field] = json!(2023),
            "session_type" => changed[field] = json!("Qualifying"),
            "session_name" => changed[field] = json!("Sprint"),
            "meeting_key" => changed[field] = json!(999),
            "window" => changed[field]["end"] = json!(4000),
            _ => unreachable!(),
        }
        let changed: CacheSpec = serde_json::from_value(changed).unwrap();
        assert!(SessionCache::open(&root.0, &changed).is_err(), "{field}");
    }
}

#[test]
fn cache_corruption_is_reported_and_never_silently_rebuilt() {
    let root = Temp::new("checksum");
    let spec = spec();
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    for i in 0..3 {
        append(&mut writer, i);
    }
    writer.finish().unwrap();
    let cache = SessionCache::open(&root.0, &spec).unwrap();
    let raw_path = cache.directory().join("000001.json");
    std::fs::write(&raw_path, b"[]").unwrap();
    assert!(
        SessionCache::open(&root.0, &spec)
            .unwrap_err()
            .to_string()
            .contains("checksum")
    );
    assert_eq!(std::fs::read(raw_path).unwrap(), b"[]");
    assert!(CacheWriter::begin(&root.0, &spec).is_err());
}

#[test]
fn cache_ignores_uncommitted_chunk_and_rejects_incompatible_manifest() {
    let root = Temp::new("interrupted");
    let spec = spec();
    let directory;
    {
        let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
        append(&mut writer, 0);
        directory = writer.directory().to_owned();
    }
    std::fs::write(directory.join("000001.json"), b"partial").unwrap();
    std::fs::write(directory.join("manifest.tmp"), b"{partial").unwrap();
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    assert_eq!(writer.completed_chunks(), 1);
    append(&mut writer, 1);
    append(&mut writer, 2);
    writer.finish().unwrap();
    let cache = SessionCache::open(&root.0, &spec).unwrap();
    let path = cache.directory().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["version"] = json!(999);
    std::fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        SessionCache::open(&root.0, &spec)
            .unwrap_err()
            .to_string()
            .contains("version")
    );
}

#[test]
fn cache_fails_without_writable_destination_and_prevents_concurrent_writers() {
    let root = Temp::new("write-error");
    let spec = spec();
    let file = root.0.join("file");
    std::fs::write(&file, b"occupied").unwrap();
    assert!(CacheWriter::begin(&file, &spec).is_err());
    let writer = CacheWriter::begin(&root.0, &spec).unwrap();
    assert!(CacheWriter::begin(&root.0, &spec).is_err());
    drop(writer);
    assert!(CacheWriter::begin(&root.0, &spec).is_ok());
}

#[test]
fn cache_manifest_rejects_missing_duplicate_and_wrong_source_entries() {
    let root = Temp::new("manifest");
    let spec = spec();
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    for i in 0..3 {
        append(&mut writer, i);
    }
    writer.finish().unwrap();
    let cache = SessionCache::open(&root.0, &spec).unwrap();
    let path = cache.directory().join("manifest.json");
    let original = std::fs::read(&path).unwrap();
    for defect in [
        "missing",
        "duplicate",
        "source",
        "identity",
        "timestamp",
        "window",
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        match defect {
            "missing" => {
                value["entries"].as_array_mut().unwrap().pop();
            }
            "duplicate" => {
                value["entries"][2] = value["entries"][1].clone();
            }
            "source" => {
                value["entries"][1]["metadata"]["source_url"] =
                    json!("https://example.com/not-openf1")
            }
            "identity" => value["entries"][1]["metadata"]["session_key"] = json!(9636),
            "timestamp" => value["entries"][1]["metadata"]["retrieved_at"] = json!(-1),
            "window" => value["spec"]["window"]["end"] = json!(0),
            _ => unreachable!(),
        }
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(SessionCache::open(&root.0, &spec).is_err(), "{defect}");
    }
}

#[cfg(windows)]
#[test]
fn cache_readonly_manifest_failure_preserves_last_committed_prefix() {
    let root = Temp::new("readonly");
    let spec = spec();
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    append(&mut writer, 0);
    let path = writer.directory().join("manifest.json");
    let original_permissions = std::fs::metadata(&path).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    let result = writer.import(metadata(1), &raw(1));
    std::fs::set_permissions(&path, original_permissions).unwrap();
    assert!(result.is_err());
    assert_eq!(writer.completed_chunks(), 1);
    drop(writer);
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    assert_eq!(writer.completed_chunks(), 1);
    append(&mut writer, 1);
    append(&mut writer, 2);
    writer.finish().unwrap();
    assert!(SessionCache::open(&root.0, &spec).is_ok());
}

#[test]
fn cache_rejects_malformed_import_and_conflicting_duplicate_without_progress() {
    let root = Temp::new("import");
    let spec = spec();
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    assert!(writer.import(metadata(0), b"[").is_err());
    assert_eq!(writer.completed_chunks(), 0);
    assert!(writer.import(metadata(1), &raw(1)).is_err());
    append(&mut writer, 0);
    append(&mut writer, 1);
    let mut row: serde_json::Value = serde_json::from_slice(&raw(1)).unwrap();
    row[0]["speed"] = json!(201);
    assert!(
        writer
            .import(metadata(1), &serde_json::to_vec(&row).unwrap())
            .is_err()
    );
    assert_eq!(writer.completed_chunks(), 2);
}

#[test]
fn cache_crash_lock_child() {
    let Some(root) = std::env::var_os("SECTORLINE_TEST_CRASH_ROOT") else {
        return;
    };
    let mut writer = CacheWriter::begin(&PathBuf::from(root), &spec()).unwrap();
    append(&mut writer, 0);
    std::process::exit(0); // Deliberately bypass destructors, like process termination.
}

#[test]
fn cache_process_exit_releases_lock_and_keeps_committed_data() {
    let root = Temp::new("process-exit");
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cache_crash_lock_child"])
        .env("SECTORLINE_TEST_CRASH_ROOT", &root.0)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let mut writer = CacheWriter::begin(&root.0, &spec()).unwrap();
    assert_eq!(writer.completed_chunks(), 1);
    append(&mut writer, 1);
    append(&mut writer, 2);
    writer.finish().unwrap();
}

#[test]
fn cache_verified_visit_streams_validated_chunks_and_rejects_late_corruption() {
    use sectorline::data::limits::Cancellation;
    let root = Temp::new("visit");
    let spec = spec();
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    for i in 0..3 {
        append(&mut writer, i);
    }
    writer.finish().unwrap();
    let mut seen = Vec::new();
    let cache = SessionCache::open_visited(
        &root.0,
        &spec,
        &Cancellation::default(),
        &|_, _| {},
        &mut |cache, chunk| {
            assert_eq!(cache.chunk_count(), 3);
            seen.push(chunk.channel());
            if chunk.channel() == Channel::CarData {
                assert!(chunk.records()[0]["throttle"].is_null());
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        seen,
        vec![Channel::Drivers, Channel::CarData, Channel::CarData]
    );
    let old = sectorline::replay::RecordedReplay::from_cache(
        &cache,
        "REFERENCE".into(),
        &Cancellation::default(),
        &|_, _| {},
    )
    .unwrap();
    let mut streamed = sectorline::replay::RecordedReplay::open_cache(
        &root.0,
        &spec,
        "STREAMED".into(),
        &Cancellation::default(),
        &|_, _| {},
    )
    .unwrap();
    streamed.seek(old.timeline().end()).unwrap();
    let reference = old
        .timeline()
        .state_at(
            old.timeline().end(),
            sectorline::replay::timeline::ReplayPolicy::EventTimeEstimates,
        )
        .unwrap()
        .frame(old.timeline())
        .unwrap();
    assert_eq!(
        serde_json::to_vec(streamed.frame()).unwrap(),
        serde_json::to_vec(&reference).unwrap()
    );
    std::fs::write(cache.directory().join("000002.json"), b"[]").unwrap();
    assert!(
        sectorline::replay::RecordedReplay::open_cache(
            &root.0,
            &spec,
            "CORRUPT".into(),
            &Cancellation::default(),
            &|_, _| {}
        )
        .is_err()
    );
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        SessionCache::open_visited(&root.0, &spec, &cancel, &|_, _| {}, &mut |_, _| panic!(
            "Must cancel before visiting"
        ))
        .is_err()
    );
}

#[test]
fn cache_streamed_open_rejects_forged_counts_before_reserving_events() {
    let root = Temp::new("streamed-count");
    let spec = spec();
    let mut writer = CacheWriter::begin(&root.0, &spec).unwrap();
    for i in 0..3 {
        append(&mut writer, i);
    }
    let cache = writer.finish().unwrap();
    let path = cache.directory().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["entries"][1]["records"] = serde_json::json!(usize::MAX);
    std::fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        sectorline::replay::RecordedReplay::open_cache(
            &root.0,
            &spec,
            "BAD COUNTS".into(),
            &sectorline::data::limits::Cancellation::default(),
            &|_, _| {}
        )
        .is_err()
    );
}
