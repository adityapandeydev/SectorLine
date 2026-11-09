use sectorline::{
    data::{
        cache::{CacheSpec, CacheWriter},
        limits::Cancellation,
        openf1::{Channel, ChunkMetadata, Window},
    },
    domain::UtcMillis,
    replay::RecordedReplay,
};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
pub struct Fixture {
    pub root: PathBuf,
    pub spec: CacheSpec,
}
impl Fixture {
    pub fn empty() -> Self {
        static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .join(format!(
                "t21-fixture-{}-{}",
                std::process::id(),
                ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        std::fs::create_dir(&root).unwrap();
        Self {
            root,
            spec: CacheSpec::new(
                9590,
                1244,
                2024,
                ("Race", "Race"),
                Window::new(UtcMillis(0), UtcMillis(2000)).unwrap(),
                Duration::from_secs(2),
                vec![Channel::Drivers, Channel::CarData],
            )
            .unwrap(),
        }
    }
    pub fn complete() -> Self {
        let f = Self::empty();
        let mut writer = CacheWriter::begin(&f.root, &f.spec).unwrap();
        writer
            .import(
                meta(Channel::Drivers, None),
                &serde_json::to_vec(&drivers()).unwrap(),
            )
            .unwrap();
        writer
            .import(
                meta(
                    Channel::CarData,
                    Some(Window::new(UtcMillis(0), UtcMillis(2000)).unwrap()),
                ),
                &serde_json::to_vec(&car()).unwrap(),
            )
            .unwrap();
        writer.finish().unwrap();
        f
    }
    pub fn open(&self) -> sectorline::data::openf1::Result<RecordedReplay> {
        RecordedReplay::open_cache(
            &self.root,
            &self.spec,
            "SYNTHETIC FAULT FIXTURE".into(),
            &Cancellation::default(),
            &|_, _| {},
        )
    }
    pub fn directory(&self) -> PathBuf {
        self.root.join("openf1-9590")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let root = self.root.canonicalize().unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .canonicalize()
            .unwrap();
        assert!(root.starts_with(&base) && root != base);
        std::fs::remove_dir_all(root).unwrap();
    }
}
pub fn meta(channel: Channel, window: Option<Window>) -> ChunkMetadata {
    let mut url = format!(
        "https://api.openf1.org/v1/{}?session_key=9590",
        channel.endpoint()
    );
    if let Some(w) = window {
        url.push_str(&format!(
            "&date>={}&date<{}",
            stamp(w.start().0),
            stamp(w.end().0)
        ));
    }
    ChunkMetadata {
        channel,
        session_key: 9590,
        meeting_key: 1244,
        window,
        source_url: url,
        retrieved_at: UtcMillis(900000),
        status: 200,
        attempts: 1,
    }
}
pub fn stamp(t: i64) -> String {
    chrono::DateTime::from_timestamp_millis(t)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S%.3f")
        .to_string()
}
pub fn drivers() -> Value {
    json!([{"session_key":9590,"meeting_key":1244,"driver_number":4,"full_name":"SYNTHETIC DRIVER","name_acronym":"TST","team_name":"Synthetic"}])
}
pub fn car() -> Value {
    json!([{"session_key":9590,"meeting_key":1244,"driver_number":4,"date":"1970-01-01T00:00:01Z","speed":200,"rpm":10000,"n_gear":5,"throttle":50,"brake":0,"drs":8}])
}
