//! Retrospective stint summaries anchored conservatively to evidence, never future lap_end.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stint {
    pub number: u32,
    pub lap_start: u32,
    pub compound: Option<String>,
    pub age_at_start: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tyre {
    pub stint: u32,
    pub compound: Option<String>,
    pub completed_age: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PitPhase {
    RecordedVisit,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitRecord {
    pub lap: Option<u32>,
    pub lane_seconds: Option<f64>,
    pub stop_seconds: Option<f64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pit {
    pub phase: PitPhase,
    pub lap: Option<u32>,
    pub lane_seconds: Option<f64>,
    pub stop_seconds: Option<f64>,
}
