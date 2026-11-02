//! Legacy generated-demo display format. Recorded input must use the validated
//! contracts in `crate::domain`, not these string-oriented presentation models.
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrsZone {
    pub name: String,
    pub start_pct: f64,
    pub end_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedTrap {
    pub name: String,
    pub distance_pct: f64,
    pub record_kmh: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub event_name: String,
    pub circuit_name: String,
    pub session_type: String,
    pub total_laps: u32,
    pub track_length_meters: f64,
    pub air_temp_c: f64,
    pub track_temp_c: f64,
    pub humidity_pct: f64,
    pub status: String,
    pub drs_zones: Vec<DrsZone>,
    pub speed_traps: Vec<SpeedTrap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackPoint {
    pub x: f64,
    pub y: f64,
    pub dist_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Corner {
    pub number: u32,
    pub name: String,
    pub distance_pct: f64,
    pub speed_apex: f64,
    pub gear: u32,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackLayout {
    pub circuit: String,
    pub points: Vec<TrackPoint>,
    pub corners: Vec<Corner>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Driver {
    pub number: u32,
    pub code: String,
    pub name: String,
    pub team: String,
    pub color: String,
    pub base_pace: f64,
}

impl Driver {
    pub fn parsed_color(&self) -> Color {
        parse_hex_color(&self.color)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarTelemetry {
    pub driver: String,
    pub number: u32,
    pub position: u32,
    pub x: f64,
    pub y: f64,
    pub dist_pct: f64,
    pub speed: f64,
    pub rpm: u32,
    pub gear: u32,
    pub throttle: u32,
    pub brake: u32,
    pub drs: bool,
    pub compound: String,
    pub tyre_age: u32,
    pub pit_status: String,
    pub gap_to_leader: String,
    pub gap_to_ahead: String,
    pub s1: String,
    pub s2: String,
    pub s3: String,
    pub s1_status: String,
    pub s2_status: String,
    pub s3_status: String,
}

impl CarTelemetry {
    pub fn compound_badge(&self) -> (&'static str, Color) {
        match self.compound.to_uppercase().as_str() {
            "SOFT" => ("[S]", Color::Rgb(240, 50, 50)),
            "MEDIUM" => ("[M]", Color::Rgb(250, 200, 30)),
            "HARD" => ("[H]", Color::Rgb(240, 240, 240)),
            "INTERMEDIATE" => ("[I]", Color::Rgb(50, 200, 80)),
            "WET" => ("[W]", Color::Rgb(40, 100, 250)),
            _ => ("[?]", Color::Gray),
        }
    }

    pub fn sector_color(status: &str) -> Color {
        match status {
            "PURPLE" => Color::Rgb(180, 70, 255),
            "GREEN" => Color::Rgb(60, 220, 100),
            "YELLOW" => Color::Rgb(240, 200, 40),
            _ => Color::DarkGray,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryFrame {
    pub timestamp_ms: u64,
    pub lap: u32,
    pub cars: Vec<CarTelemetry>,
}

pub fn parse_hex_color(hex: &str) -> Color {
    let clean = hex.trim_start_matches('#');
    if clean.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&clean[0..2], 16),
            u8::from_str_radix(&clean[2..4], 16),
            u8::from_str_radix(&clean[4..6], 16),
        ) {
            return Color::Rgb(r, g, b);
        }
    }
    Color::Cyan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_color_parser() {
        assert_eq!(parse_hex_color("#FF8000"), Color::Rgb(255, 128, 0)); // McLaren Papaya
        assert_eq!(parse_hex_color("#E8002D"), Color::Rgb(232, 0, 45));   // Ferrari Red
        assert_eq!(parse_hex_color("#27F4D2"), Color::Rgb(39, 244, 210)); // Mercedes Cyan
    }
}
