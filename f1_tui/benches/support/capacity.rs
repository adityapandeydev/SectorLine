//! Temporary raw-on-disk synthetic capacity input. No external data or network.
use sectorline::{
    data::{
        cache::{CacheSpec, CacheWriter},
        openf1::{Channel, ChunkMetadata, Result, Window},
    },
    domain::UtcMillis,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub struct Input {
    pub root: PathBuf,
    pub manifest: PathBuf,
}
impl Drop for Input {
    fn drop(&mut self) {
        // Only our uniquely created child of the workspace target directory is removable.
        let safe = self
            .root
            .canonicalize()
            .ok()
            .zip(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../target")
                    .canonicalize()
                    .ok(),
            )
            .is_some_and(|(path, base)| {
                path.starts_with(&base)
                    && path != base
                    && path
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("sectorline-capacity-"))
            });
        if safe && let Err(e) = std::fs::remove_dir_all(&self.root) {
            eprintln!(
                "Cannot remove temporary capacity fixture {}: {e}",
                self.root.display()
            );
        }
    }
}
fn stamp(t: i64) -> String {
    chrono::DateTime::from_timestamp_millis(t)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S%.3f")
        .to_string()
}
fn rows(channel: Channel, start: i64, end: i64) -> Vec<Value> {
    let mut rows = Vec::new();
    let mut push = |driver: u32, at: i64, mut data: Value| {
        data["session_key"] = json!(999999);
        data["meeting_key"] = json!(999999);
        if driver > 0 {
            data["driver_number"] = json!(driver);
        }
        if !matches!(channel, Channel::Drivers | Channel::Stints | Channel::Laps) {
            data["date"] = json!(stamp(at) + "Z");
        }
        rows.push(data);
    };
    match channel {
        Channel::Drivers => {
            for n in 1..=22 {
                push(
                    n,
                    0,
                    json!({"name_acronym":format!("S{n:02}"),"full_name":format!("SYNTHETIC DRIVER{n}"),"team_name":"Synthetic Benchmark","team_colour":"888888"}),
                );
            }
        }
        Channel::CarData | Channel::Location | Channel::Intervals | Channel::Position => {
            let step = if matches!(channel, Channel::CarData | Channel::Location) {
                250
            } else {
                5000
            };
            for at in (start..end).step_by(step) {
                for n in 1..=22 {
                    let angle =
                        std::f64::consts::TAU * (at as f64 / 90000.0 + f64::from(n - 1) / 22.0);
                    let value = match channel {
                        Channel::CarData => {
                            json!({"speed":250.0+angle.sin()*20.0,"rpm":10000,"n_gear":7,"throttle":90,"brake":0,"drs":8})
                        }
                        Channel::Location => {
                            json!({"x":angle.cos()*1000.0,"y":angle.sin()*500.0,"z":0})
                        }
                        Channel::Intervals => {
                            json!({"gap_to_leader":f64::from(n-1)*1.3,"interval":1.3})
                        }
                        _ => json!({"position":n}),
                    };
                    push(n, at, value);
                }
            }
        }
        Channel::Laps => {
            for n in 1..=22 {
                for lap in 1..=80 {
                    push(
                        n,
                        0,
                        json!({"lap_number":lap,"date_start":stamp((lap-1)*90000)+"Z","lap_duration":90,"duration_sector_1":30,"duration_sector_2":30,"duration_sector_3":30,"is_pit_out_lap":false}),
                    );
                }
            }
        }
        Channel::Stints => {
            for n in 1..=22 {
                push(
                    n,
                    0,
                    json!({"stint_number":1,"lap_start":1,"lap_end":80,"compound":"SOFT","tyre_age_at_start":0}),
                );
            }
        }
        Channel::Weather => {
            for at in (0..7200000).step_by(60000) {
                push(
                    0,
                    at,
                    json!({"air_temperature":20,"track_temperature":30,"humidity":40,"pressure":1020,"wind_speed":3,"wind_direction":90,"rainfall":0}),
                );
            }
        }
        Channel::Pit | Channel::RaceControl => {}
    }
    rows
}
pub fn generate() -> Result<Input> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target")
        .join(format!(
            "sectorline-capacity-{}-{nonce}",
            std::process::id()
        ));
    std::fs::create_dir(&root)
        .map_err(|e| format!("Create temporary capacity root {}: {e}", root.display()))?;
    let input = Input {
        manifest: root.join("openf1-999999/manifest.json"),
        root,
    };
    let spec = CacheSpec::new(
        999999,
        999999,
        2024,
        ("SYNTHETIC CAPACITY - NOT OBSERVED", "Race"),
        Window::new(UtcMillis(0), UtcMillis(7200000))?,
        Duration::from_secs(300),
        Channel::ALL.to_vec(),
    )?;
    let mut writer = CacheWriter::begin(&input.root, &spec)?;
    for channel in Channel::ALL {
        let windows = if matches!(
            channel,
            Channel::CarData | Channel::Location | Channel::Intervals
        ) {
            (0..24)
                .map(|n| {
                    Some(Window::new(UtcMillis(n * 300000), UtcMillis((n + 1) * 300000)).unwrap())
                })
                .collect::<Vec<_>>()
        } else {
            vec![None]
        };
        for window in windows {
            let mut url = format!(
                "http://127.0.0.1:1/{}?session_key=999999",
                channel.endpoint()
            );
            let (start, end) = window
                .map(|w| (w.start().0, w.end().0))
                .unwrap_or((0, 7200000));
            if window.is_some() {
                url.push_str(&format!("&date>={}&date<{}", stamp(start), stamp(end)));
            }
            writer.import(
                ChunkMetadata {
                    channel,
                    session_key: 999999,
                    meeting_key: 999999,
                    window,
                    source_url: url,
                    retrieved_at: UtcMillis(8000000),
                    status: 200,
                    attempts: 1,
                },
                &serde_json::to_vec(&rows(channel, start, end))?,
            )?;
        }
    }
    writer.finish()?;
    Ok(input)
}
