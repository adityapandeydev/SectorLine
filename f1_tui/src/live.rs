//! Legacy historical-demo bridge. Actual live ingestion is not implemented.
use crate::stream::TelemetryStream;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

pub const LIVE_UNAVAILABLE: &str = "Live access is unavailable. Sector Line currently provides generated demos only; authentic replay and live ingestion are not implemented.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveSessionStatus {
    pub is_live_active: bool,
    pub session_name: String,
    pub circuit_name: String,
    pub country_name: String,
    pub detail_message: String,
}

pub struct LiveTelemetryClient;

impl LiveTelemetryClient {
    /// Reports implementation availability, without probing a remote session.
    pub fn probe_live_status() -> LiveSessionStatus {
        LiveSessionStatus {
            is_live_active: false,
            session_name: "Unavailable".into(),
            circuit_name: String::new(),
            country_name: String::new(),
            detail_message: LIVE_UNAVAILABLE.into(),
        }
    }

    /// Reject before any network request or cache write.
    pub fn fetch_live_stream_natively(
        _cache_dir: &Path,
    ) -> Result<TelemetryStream, Box<dyn std::error::Error>> {
        Err(LIVE_UNAVAILABLE.into())
    }

    /// Prepare a generated demo using historical metadata, never a live stream.
    pub fn ensure_circuit_ready(
        cache_dir: &Path,
        gp: &str,
        year: u32,
        session_type: &str,
    ) -> Result<Option<TelemetryStream>, Box<dyn std::error::Error>> {
        let is_live_request = gp.eq_ignore_ascii_case("live");

        if is_live_request {
            let stream = Self::fetch_live_stream_natively(cache_dir)?;
            return Ok(Some(stream));
        }

        // For historical sessions: check cache first, then invoke Python FastF1 if missing
        let session_info_path = cache_dir.join("session_info.json");
        let mut needs_extract = true;

        if session_info_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&session_info_path) {
                if let Ok(info) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(event) = info.get("event_name").and_then(|v| v.as_str()) {
                        let ev_lower = event.to_lowercase();
                        let gp_lower = gp.to_lowercase();
                        let is_match = ev_lower.contains(&gp_lower)
                            || (gp_lower == "silverstone" && ev_lower.contains("british"))
                            || (gp_lower == "british" && ev_lower.contains("silverstone"))
                            || (gp_lower == "baku" && ev_lower.contains("azerbaijan"))
                            || (gp_lower == "azerbaijan" && ev_lower.contains("baku"))
                            || (gp_lower == "spa" && ev_lower.contains("belgian"))
                            || (gp_lower == "monza" && ev_lower.contains("italian"));
                        if is_match {
                            needs_extract = false;
                        }
                    }
                }
            }
        }

        if needs_extract {
            println!(
                "-> [RUST BRIDGE] Spawning FastF1 Python extraction bridge for '{}' ({})...",
                gp, year
            );

            let script_candidates = [
                "f1_suite/python_bridge/extract.py",
                "python_bridge/extract.py",
                "../python_bridge/extract.py",
            ];

            let script_path = script_candidates
                .iter()
                .find(|p| Path::new(p).exists())
                .ok_or_else(|| "Could not locate python_bridge/extract.py script!".to_string())?;

            let mut cmd = Command::new("python");
            cmd.arg(script_path)
                .arg("--gp")
                .arg(gp)
                .arg("--year")
                .arg(year.to_string())
                .arg("--session")
                .arg(session_type);

            let status = cmd.status()?;

            if !status.success() {
                return Err(format!(
                    "Python extraction bridge exited with error code: {:?}",
                    status.code()
                )
                .into());
            }

            println!("-> [RUST BRIDGE] Historical session extraction complete!");
        }

        Ok(None)
    }
}
