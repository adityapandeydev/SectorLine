use std::fs;
use std::path::Path;

use crate::models::{CarTelemetry, Driver, SessionInfo, TelemetryFrame, TrackLayout};

pub struct TelemetryStream {
    pub session: SessionInfo,
    pub track: TrackLayout,
    pub drivers: Vec<Driver>,
    pub frames: Vec<TelemetryFrame>,
    pub current_frame_idx: usize,
    pub playback_speed: f64,
    pub is_paused: bool,
    pub has_ended: bool,
    pub selected_driver_code: String,
    accumulated_ms: f64,
}

impl TelemetryStream {
    /// Bundled generated simulation. No runtime filesystem, Python or provider access.
    pub fn bundled_demo() -> Result<Self, Box<dyn std::error::Error>> {
        Self::from_json(
            include_str!("../../python_bridge/cache/session_info.json"),
            include_str!("../../python_bridge/cache/track_layout.json"),
            include_str!("../../python_bridge/cache/drivers.json"),
            include_str!("../../python_bridge/cache/telemetry_stream.json"),
        )
    }
    /// Loads generated demo datasets from the specified cache folder.
    pub fn load_from_dir<P: AsRef<Path>>(cache_dir: P) -> Result<Self, Box<dyn std::error::Error>> {
        let dir = cache_dir.as_ref();

        let session_raw = fs::read_to_string(dir.join("session_info.json"))?;

        let track_raw = fs::read_to_string(dir.join("track_layout.json"))?;

        let drivers_raw = fs::read_to_string(dir.join("drivers.json"))?;

        let stream_raw = fs::read_to_string(dir.join("telemetry_stream.json"))?;
        Self::from_json(&session_raw, &track_raw, &drivers_raw, &stream_raw)
    }
    fn from_json(
        session: &str,
        track: &str,
        drivers: &str,
        frames: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let session: SessionInfo = serde_json::from_str(session)?;
        let track: TrackLayout = serde_json::from_str(track)?;
        let drivers: Vec<Driver> = serde_json::from_str(drivers)?;
        let frames: Vec<TelemetryFrame> = serde_json::from_str(frames)?;

        if frames.is_empty() {
            return Err("Telemetry stream contains 0 frames".into());
        }
        if frames
            .windows(2)
            .any(|pair| pair[0].timestamp_ms >= pair[1].timestamp_ms)
        {
            return Err("Demo timestamps must be strictly increasing".into());
        }

        let first_code = drivers
            .first()
            .map(|d| d.code.clone())
            .unwrap_or_else(|| "VER".into());

        Ok(Self {
            session,
            track,
            drivers,
            frames,
            current_frame_idx: 0,
            playback_speed: 1.0,
            is_paused: false,
            has_ended: false,
            selected_driver_code: first_code,
            accumulated_ms: 0.0,
        })
    }

    /// Creates generated demo playback from in-memory datasets.
    pub fn new_in_memory(
        session: SessionInfo,
        track: TrackLayout,
        drivers: Vec<Driver>,
        frames: Vec<TelemetryFrame>,
    ) -> Self {
        let first_code = drivers.first().map(|d| d.code.clone()).unwrap_or_else(|| "RUS".into());
        Self {
            session,
            track,
            drivers,
            frames,
            current_frame_idx: 0,
            playback_speed: 1.0,
            is_paused: false,
            has_ended: false,
            selected_driver_code: first_code,
            accumulated_ms: 0.0,
        }
    }

    /// Returns the current active telemetry frame.
    pub fn current_frame(&self) -> &TelemetryFrame {
        &self.frames[self.current_frame_idx]
    }

    /// Advance simulation playback by `dt_ms` real-world milliseconds scaled by `playback_speed`.
    pub fn tick(&mut self, dt_ms: f64) {
        if self.is_paused
            || self.has_ended
            || self.frames.is_empty()
            || !dt_ms.is_finite()
            || dt_ms < 0.0
        {
            return;
        }

        self.accumulated_ms += dt_ms * self.playback_speed;

        let target = self.frames[0].timestamp_ms as f64 + self.accumulated_ms;
        self.current_frame_idx = self
            .frames
            .partition_point(|frame| frame.timestamp_ms as f64 <= target)
            .saturating_sub(1);
        if self.current_frame_idx == self.frames.len() - 1 {
            self.has_ended = true;
            self.is_paused = true;
        }
    }

    pub fn toggle_pause(&mut self) {
        if !self.has_ended {
            self.is_paused = !self.is_paused;
        }
    }

    pub fn set_speed(&mut self, speed: f64) {
        if speed.is_finite() {
            self.playback_speed = speed.clamp(0.25, 20.0);
        }
    }
    pub fn restart(&mut self) {
        self.current_frame_idx = 0;
        self.accumulated_ms = 0.0;
        self.has_ended = false;
        self.is_paused = true;
    }

    /// Returns the active telemetry data for the currently selected driver.
    pub fn selected_car(&self) -> Option<&CarTelemetry> {
        let frame = self.current_frame();
        frame.cars.iter().find(|c| c.driver == self.selected_driver_code)
    }

    pub fn driver_info(&self, code: &str) -> Option<&Driver> {
        self.drivers.iter().find(|d| d.code == code)
    }

    /// Move driver selection down the leaderboard.
    pub fn select_next_driver(&mut self) {
        let frame = self.current_frame();
        if let Some(pos) = frame.cars.iter().position(|c| c.driver == self.selected_driver_code) {
            let next_pos = (pos + 1) % frame.cars.len();
            self.selected_driver_code = frame.cars[next_pos].driver.clone();
        }
    }

    /// Move driver selection up the leaderboard.
    pub fn select_prev_driver(&mut self) {
        let frame = self.current_frame();
        if let Some(pos) = frame.cars.iter().position(|c| c.driver == self.selected_driver_code) {
            let prev_pos = if pos == 0 { frame.cars.len() - 1 } else { pos - 1 };
            self.selected_driver_code = frame.cars[prev_pos].driver.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_playback_controls() {
        let cache_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../python_bridge/cache");

        let mut stream = TelemetryStream::load_from_dir(cache_path).expect("Failed to load cache");
        assert_eq!(stream.current_frame_idx, 0);
        assert!(!stream.is_paused);
        assert_eq!(stream.playback_speed, 1.0);

        // Tick to advance
        stream.tick(250.0);
        assert_eq!(stream.current_frame_idx, 1);

        // Pause
        stream.toggle_pause();
        assert!(stream.is_paused);
        stream.tick(500.0);
        assert_eq!(stream.current_frame_idx, 1); // Should not advance while paused

        // Unpause and change speed
        stream.toggle_pause();
        stream.set_speed(5.0);
        assert_eq!(stream.playback_speed, 5.0);

        // Driver selection
        let initial_driver = stream.selected_driver_code.clone();
        stream.select_next_driver();
        assert_ne!(stream.selected_driver_code, initial_driver);
        assert!(stream.selected_car().is_some());
    }
}
