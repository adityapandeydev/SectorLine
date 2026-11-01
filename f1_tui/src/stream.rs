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
    pub is_live: bool,
    pub selected_driver_code: String,
    accumulated_ms: f64,
}

impl TelemetryStream {
    /// Loads session telemetry datasets from the specified cache folder.
    pub fn load_from_dir<P: AsRef<Path>>(cache_dir: P) -> Result<Self, Box<dyn std::error::Error>> {
        let dir = cache_dir.as_ref();

        let session_raw = fs::read_to_string(dir.join("session_info.json"))?;
        let session: SessionInfo = serde_json::from_str(&session_raw)?;

        let track_raw = fs::read_to_string(dir.join("track_layout.json"))?;
        let track: TrackLayout = serde_json::from_str(&track_raw)?;

        let drivers_raw = fs::read_to_string(dir.join("drivers.json"))?;
        let drivers: Vec<Driver> = serde_json::from_str(&drivers_raw)?;

        let stream_raw = fs::read_to_string(dir.join("telemetry_stream.json"))?;
        let frames: Vec<TelemetryFrame> = serde_json::from_str(&stream_raw)?;

        if frames.is_empty() {
            return Err("Telemetry stream contains 0 frames".into());
        }

        let first_code = drivers.first().map(|d| d.code.clone()).unwrap_or_else(|| "VER".into());

        Ok(Self {
            session,
            track,
            drivers,
            frames,
            current_frame_idx: 0,
            playback_speed: 1.0,
            is_paused: false,
            is_live: false,
            selected_driver_code: first_code,
            accumulated_ms: 0.0,
        })
    }

    /// Creates a TelemetryStream directly from in-memory datasets (used by native OpenF1 client).
    pub fn new_in_memory(
        session: SessionInfo,
        track: TrackLayout,
        drivers: Vec<Driver>,
        frames: Vec<TelemetryFrame>,
        is_live: bool,
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
            is_live,
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
        if self.is_paused || self.frames.is_empty() {
            return;
        }

        self.accumulated_ms += dt_ms * self.playback_speed;

        // Frames are recorded at ~5 Hz (200ms per frame)
        let frame_duration_ms = 200.0;
        while self.accumulated_ms >= frame_duration_ms {
            self.accumulated_ms -= frame_duration_ms;
            self.current_frame_idx = (self.current_frame_idx + 1) % self.frames.len();
        }
    }

    pub fn toggle_pause(&mut self) {
        self.is_paused = !self.is_paused;
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.playback_speed = speed.clamp(0.25, 20.0);
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
