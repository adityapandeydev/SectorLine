//! Developer acquisition harness. Outputs scratch chunks, not a replay cache.
use sectorline::data::{
    catalogue::{Catalogue, now_ms},
    limits::{Cancellation, Limits},
    openf1::{AcquisitionRequest, OpenF1Client, Result, Window},
};
use sectorline::domain::UtcMillis;
use std::{fs, path::PathBuf};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 && args.len() != 6 {
        return Err("Usage: cargo run --release -p sectorline --example acquire -- CATALOGUE_JSON SESSION_ID NEW_OUTPUT_DIR [START_RFC3339 END_RFC3339]".into());
    }
    let catalogue = Catalogue::load(&PathBuf::from(&args[1])).map_err(|e| e.to_string())?;
    let selection = catalogue
        .select(args[2].parse()?, now_ms().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut request = AcquisitionRequest::from_selection(&selection)?;
    if args.len() == 6 {
        request = request.with_window(Window::new(
            UtcMillis(chrono::DateTime::parse_from_rfc3339(&args[4])?.timestamp_millis()),
            UtcMillis(chrono::DateTime::parse_from_rfc3339(&args[5])?.timestamp_millis()),
        )?)?;
    }
    let output = PathBuf::from(&args[3]);
    // Refuse overwrite; partial scratch output remains explicit on cancellation/error.
    fs::create_dir(&output)?;
    println!(
        "OpenF1 session {} | requested high-frequency UTC window {:?} | raw scratch output {}",
        request.session_key(),
        request.window(),
        output.display()
    );
    let mut index = 0;
    let summary = OpenF1Client::new(Limits::default())?.acquire(
        &request,
        &Cancellation::default(),
        |chunk| {
            let stem = format!("{index:04}-{}", chunk.channel().endpoint());
            fs::write(output.join(format!("{stem}.raw.json")), chunk.raw_bytes())?;
            // Raw numeric anomalies are evidence, not validated measured values.
            fs::write(
                output.join(format!("{stem}.validated.json")),
                serde_json::to_vec(chunk.records())?,
            )?;
            fs::write(
                output.join(format!("{stem}.metadata.json")),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "session_key":chunk.session_key(), "meeting_key":chunk.meeting_key(),
                    "source_url":chunk.source_url(), "retrieved_at_utc_ms":chunk.retrieved_at(),
                    "window":chunk.window(), "status":chunk.status(), "attempts":chunk.attempts(),
                    "empty_reason":chunk.empty_reason(), "issues":chunk.issues()
                }))?,
            )?;
            println!(
                "{stem}: {} rows, {} field issues{}",
                chunk.records().len(),
                chunk.issues().len(),
                chunk
                    .empty_reason()
                    .map(|r| format!(" | {r}"))
                    .unwrap_or_default()
            );
            index += 1;
            Ok(())
        },
    )?;
    println!(
        "Acquisition complete: {summary:?}. Scratch output is not a validated/published replay cache (T06). No demo was loaded."
    );
    Ok(())
}
