//! Download/resume a recording cache, verify offline, or import T05 scratch data.
use sectorline::data::{
    cache::{CacheSpec, CacheWriter, SessionCache},
    catalogue::{Catalogue, now_ms},
    limits::{Cancellation, Limits},
    openf1::{AcquisitionRequest, Channel, ChunkMetadata, OpenF1Client, Result},
    paths::session_cache_root,
};
use std::{fs, io::Read, path::PathBuf};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err("Usage: cargo run --release -p sectorline --example cache -- CATALOGUE_JSON SESSION_ID [--verify] [--import SCRATCH_DIR] [--root CACHE_ROOT]".into());
    }
    let mut root = None;
    let mut import = None;
    let mut verify = false;
    let mut seen = std::collections::BTreeSet::new();
    let mut i = 3;
    while i < args.len() {
        let flag = args[i].as_str();
        if !seen.insert(flag) {
            return Err("Duplicate cache option".into());
        }
        match flag {
            "--verify" => verify = true,
            "--root" | "--import" => {
                i += 1;
                let value = args
                    .get(i)
                    .filter(|s| !s.starts_with("--"))
                    .ok_or("Cache option needs a path")?;
                if flag == "--root" {
                    root = Some(PathBuf::from(value));
                } else {
                    import = Some(PathBuf::from(value));
                }
            }
            _ => return Err(format!("Unknown cache option: {flag}").into()),
        }
        i += 1;
    }
    if verify && import.is_some() {
        return Err("Choose either offline verification or scratch import".into());
    }
    let catalogue = Catalogue::load(&PathBuf::from(&args[1])).map_err(|e| e.to_string())?;
    let selection = catalogue
        .select(args[2].parse()?, now_ms().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let request = AcquisitionRequest::from_selection(&selection)?;
    let limits = Limits::default();
    let spec = CacheSpec::new(
        request.session_key(),
        request.meeting_key(),
        selection.session.year,
        (
            &selection.session.session_name,
            &selection.session.session_type,
        ),
        request.window(),
        limits.chunk_span,
        Channel::ALL.to_vec(),
    )?;
    let root = match root {
        Some(root) => root,
        None => session_cache_root()?,
    };
    if verify || SessionCache::exists(&root, &spec) {
        let cache = SessionCache::open(&root, &spec)?;
        println!(
            "Verified offline recording: {} chunks | {}",
            cache.chunk_count(),
            cache.directory().display()
        );
        return Ok(());
    }
    let mut writer = CacheWriter::begin(&root, &spec)?;
    println!(
        "Resuming recording: {} committed chunks | {}",
        writer.completed_chunks(),
        writer.directory().display()
    );
    if let Some(input) = import {
        let mut paths = fs::read_dir(&input)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        for path in paths.into_iter().filter(|p| {
            p.file_name()
                .is_some_and(|s| s.to_string_lossy().ends_with(".metadata.json"))
        }) {
            let file = path
                .file_name()
                .unwrap()
                .to_str()
                .ok_or("Non-UTF8 scratch filename")?;
            let stem = file
                .strip_suffix(".metadata.json")
                .ok_or("Invalid scratch filename")?;
            let endpoint = stem.split_once('-').ok_or("Invalid scratch sequence")?.1;
            let channel = Channel::ALL
                .into_iter()
                .find(|c| c.endpoint() == endpoint)
                .ok_or("Unknown scratch channel")?;
            let mut metadata: serde_json::Value =
                serde_json::from_slice(&read(&path, 8 * 1024 * 1024)?)?;
            metadata["channel"] = serde_json::to_value(channel)?;
            let metadata: ChunkMetadata = serde_json::from_value(metadata)?;
            writer.import(
                metadata,
                &read(&input.join(format!("{stem}.raw.json")), 64 * 1024 * 1024)?,
            )?;
        }
    } else {
        let summary = OpenF1Client::new(limits)?.acquire_with_store(
            &request,
            spec.channels(),
            &Cancellation::default(),
            &mut writer,
        )?;
        println!("Acquisition: {summary:?}");
    }
    let cache = writer.finish()?;
    println!(
        "Published offline recording: {} chunks | {}. Playback integration follows T07–T09.",
        cache.chunk_count(),
        cache.directory().display()
    );
    Ok(())
}

fn read(path: &std::path::Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "Scratch import file {} exceeds {limit}-byte limit",
            path.display()
        )
        .into());
    }
    Ok(bytes)
}
