//! A metadata-only CLI entrypoint, kept separate from the generated-demo loader.
use super::catalogue::{Catalogue, Query, default_cache_path, now_ms};
use std::{error::Error, path::PathBuf};

pub fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let mut query = Query::default();
    let mut refresh = false;
    let mut path = None;
    let mut key = None;
    let mut seen = std::collections::BTreeSet::new();
    let mut i = 1;
    while i < args.len() {
        let flag = args[i].as_str();
        if !seen.insert(flag) {
            return Err(format!("Duplicate catalogue option: {flag}").into());
        }
        match flag {
            "--catalogue" => {}
            "--refresh" => refresh = true,
            "--year" | "--search" | "--session" | "--catalogue-cache" => {
                i += 1;
                let value = args
                    .get(i)
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| format!("{flag} needs a value"))?;
                match flag {
                    "--year" => query.year = Some(value.parse()?),
                    "--search" => query.text = value.clone(),
                    "--session" => key = Some(value.parse::<u32>()?),
                    "--catalogue-cache" => path = Some(PathBuf::from(value)),
                    _ => unreachable!(),
                }
            }
            _ => return Err(format!("Unknown catalogue option: {flag}; see --help").into()),
        }
        i += 1;
    }
    if refresh && query.year.is_none() && (!query.text.is_empty() || key.is_some()) {
        return Err("Session refresh needs an explicit --year; no default year is assumed".into());
    }
    let path = match path {
        Some(p) => p,
        None => default_cache_path()?,
    };
    let catalogue = if refresh {
        let previous = if path.exists() {
            Some(Catalogue::load(&path)?)
        } else {
            None
        };
        let updated = Catalogue::refresh(previous.as_ref(), query.year)?;
        updated.save(&path)?;
        updated
    } else {
        Catalogue::load(&path)?
    };
    println!(
        "OpenF1 session catalogue | metadata only | retrieved UTC Unix ms: {}",
        catalogue.retrieved_at_ms()
    );
    println!("Cache: {}", path.display());
    let now = now_ms()?;
    if let Some(key) = key {
        let selection = catalogue.select(key, now)?;
        if !catalogue
            .search(&query, now)?
            .iter()
            .any(|s| s.session.session_key == key)
        {
            return Err("Selected ID does not match the requested year/search".into());
        }
        print_selection(&selection);
        if let Some(reason) = selection.unavailable_reason {
            return Err(reason.into());
        }
        println!(
            "Selected OpenF1 session {}. Recorded replay loading is not implemented yet; no generated demo was loaded.",
            selection.session.id().key()
        );
    } else if query.year.is_some() || !query.text.trim().is_empty() {
        let matches = catalogue.search(&query, now)?;
        if matches.is_empty() {
            return Err(
                "No matching cached sessions; no alternative year or session was selected".into(),
            );
        }
        for selection in matches {
            print_selection(&selection);
        }
        println!(
            "Select an exact ID with --session ID. Completion metadata does not guarantee telemetry coverage."
        );
    } else {
        println!("Discovered years: {:?}", catalogue.years());
        for m in catalogue.meetings() {
            println!(
                "{} | meeting {} | {} | {}{}",
                m.year,
                m.meeting_key,
                m.meeting_name,
                m.circuit_short_name,
                if m.is_cancelled { " | cancelled" } else { "" }
            );
        }
        println!(
            "Fetch sessions with --refresh --year YYYY; offline search with --year YYYY --search \"Monza Q\"."
        );
    }
    Ok(())
}

fn print_selection(s: &super::catalogue::Selection<'_>) {
    println!(
        "{} | meeting {} | session {} | {} | {} | {}",
        s.session.year,
        s.meeting.meeting_key,
        s.session.session_key,
        s.meeting.meeting_name,
        s.session.session_name,
        s.unavailable_reason
            .as_deref()
            .unwrap_or("completed (metadata); coverage unverified")
    );
}
