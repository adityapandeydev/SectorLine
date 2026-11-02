use sectorline::data::catalogue::{Catalogue, CatalogueCache, Query};
use serde_json::{Value, json};

// Synthetic provider-shaped metadata. IDs are test identities, not real sessions.
fn fixture() -> CatalogueCache {
    serde_json::from_value(json!({
        "version":1,"retrieved_at_ms":1000,"loaded_years":[2024],
        "meetings":[
            {"meeting_key":1,"year":2024,"meeting_name":"Italian Grand Prix","circuit_short_name":"Monza","location":"Monza","country_name":"Italy"},
            {"meeting_key":2,"year":2024,"meeting_name":"São Paulo Grand Prix","circuit_short_name":"Interlagos","location":"São Paulo","country_name":"Brazil"},
            {"meeting_key":3,"year":2023,"meeting_name":"Italian Grand Prix","circuit_short_name":"Monza","location":"Monza","country_name":"Italy"}
        ],
        "sessions":[
            {"session_key":101,"meeting_key":1,"year":2024,"session_name":"Race","session_type":"Race","date_start":"2024-09-01T13:00:00+00:00","date_end":"2024-09-01T15:00:00+00:00"},
            {"session_key":102,"meeting_key":1,"year":2024,"session_name":"Qualifying","session_type":"Qualifying","date_start":"2024-08-31T16:00:00+02:00","date_end":"2024-08-31T17:00:00+02:00"},
            {"session_key":103,"meeting_key":1,"year":2024,"session_name":"Practice 1","session_type":"Practice","date_start":null,"date_end":null},
            {"session_key":104,"meeting_key":2,"year":2024,"session_name":"Sprint","session_type":"Race","date_start":"2024-11-02T14:00:00Z","date_end":"2024-11-02T15:00:00Z"},
            {"session_key":105,"meeting_key":2,"year":2024,"session_name":"Race","session_type":"Race","date_start":"2024-11-03T14:00:00Z","date_end":"2024-11-03T16:00:00Z","is_cancelled":true}
        ]
    })).unwrap()
}

fn now() -> i64 {
    1_735_689_600_000
} // 2025-01-01 UTC
fn catalogue() -> Catalogue {
    Catalogue::new(fixture()).unwrap()
}
fn query(year: u16, text: &str) -> Query {
    Query {
        year: Some(year),
        text: text.into(),
    }
}

#[test]
fn catalogue_discovers_years_meetings_and_session_types() {
    let c = catalogue();
    assert_eq!(c.years(), vec![2023, 2024]);
    assert_eq!(c.search(&query(2024, "Monza"), now()).unwrap().len(), 3);
    assert_eq!(
        c.resolve(&query(2024, "Monza Q"), now())
            .unwrap()
            .session
            .session_key,
        102
    );
    assert_eq!(
        c.resolve(&query(2024, "Sao Paulo sprint"), now())
            .unwrap()
            .session
            .session_key,
        104
    );
    assert!(c.search(&query(2024, "Spain"), now()).unwrap().is_empty());
}

#[test]
fn catalogue_never_guesses_year_or_resolves_ambiguous_search() {
    let c = catalogue();
    assert!(c.resolve(&query(2024, "Monza"), now()).is_err());
    assert!(c.resolve(&query(2024, "Monza 2023 R"), now()).is_err());
    assert!(
        c.search(&query(2023, "Monza"), now())
            .unwrap_err()
            .to_string()
            .contains("not cached")
    );
    assert!(
        c.search(&query(2022, "Monza"), now())
            .unwrap_err()
            .to_string()
            .contains("not cached")
    );
    assert_eq!(c.select(101, now()).unwrap().session.session_key, 101);
    assert!(c.select(999, now()).is_err());
}

#[test]
fn catalogue_unavailability_is_explicit_and_time_aware() {
    let c = catalogue();
    assert!(c.select(101, now()).unwrap().unavailable_reason.is_none());
    assert!(
        c.select(101, 0)
            .unwrap()
            .unavailable_reason
            .unwrap()
            .contains("not completed")
    );
    assert!(
        c.select(103, now())
            .unwrap()
            .unavailable_reason
            .unwrap()
            .contains("timestamps")
    );
    assert!(
        c.select(105, now())
            .unwrap()
            .unavailable_reason
            .unwrap()
            .contains("cancelled")
    );
    let end = chrono::DateTime::parse_from_rfc3339("2024-09-01T15:00:00Z")
        .unwrap()
        .timestamp_millis();
    assert!(c.select(101, end - 1).unwrap().unavailable_reason.is_some());
    assert!(c.select(101, end).unwrap().unavailable_reason.is_none());
}

#[test]
fn catalogue_rejects_corruption_and_identity_mismatch() {
    for change in [
        "version",
        "duplicate",
        "orphan",
        "year",
        "time",
        "zero",
        "coverage",
        "control",
    ] {
        let mut value = serde_json::to_value(fixture()).unwrap();
        match change {
            "version" => value["version"] = json!(99),
            "duplicate" => {
                let row = value["sessions"][0].clone();
                value["sessions"].as_array_mut().unwrap().push(row);
            }
            "orphan" => value["sessions"][0]["meeting_key"] = json!(999),
            "year" => value["sessions"][0]["year"] = json!(2023),
            "time" => value["sessions"][0]["date_end"] = json!("yesterday"),
            "zero" => value["sessions"][0]["session_key"] = json!(0),
            "coverage" => value["loaded_years"] = json!([]),
            "control" => value["meetings"][0]["meeting_name"] = json!("Bad\u{001b}[2J"),
            _ => unreachable!(),
        }
        let parsed: CatalogueCache = serde_json::from_value(value).unwrap();
        assert!(Catalogue::new(parsed).is_err(), "{change}");
    }
}

#[test]
fn catalogue_roundtrip_permits_offline_selection() {
    // Windows GetTempPath2 can resolve to a protected system directory in the
    // desktop sandbox; use Cargo's ignored build directory for this fixture.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target")
        .join(format!("sectorline-catalogue-{}.json", std::process::id()));
    let c = catalogue();
    c.save(&path).unwrap();
    c.save(&path).unwrap(); // Replacing an existing snapshot must work on Windows too.
    let offline = Catalogue::load(&path).unwrap();
    assert_eq!(
        offline
            .resolve(&query(2024, "Monza R"), now())
            .unwrap()
            .session
            .session_key,
        101
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sectorline"))
        .args([
            "--catalogue",
            "--catalogue-cache",
            path.to_str().unwrap(),
            "--year",
            "2024",
            "--search",
            "Monza Q",
            "--session",
            "102",
        ])
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("102") && text.contains("Qualifying") && text.contains("not implemented"),
        "{text}"
    );
    assert!(!text.contains("Loading generated demo"));
}

#[test]
fn catalogue_unknown_kind_and_missing_end_are_preserved() {
    let mut value: Value = serde_json::to_value(fixture()).unwrap();
    value["sessions"][0]["session_name"] = json!("Experimental session");
    value["sessions"][0]["session_type"] = json!("Experimental");
    value["sessions"][0]["date_end"] = Value::Null;
    let c = Catalogue::new(serde_json::from_value(value).unwrap()).unwrap();
    assert!(c.select(101, now()).unwrap().unavailable_reason.is_some());
    assert_eq!(
        c.select(101, now()).unwrap().session.session_type,
        "Experimental"
    );
}

#[test]
fn catalogue_cli_rejects_invalid_options_before_loading_or_network() {
    for args in [
        vec!["--catalogue", "--year"],
        vec!["--catalogue", "--year", "2024", "--year", "2023"],
        vec!["--catalogue", "--refresh", "--search", "Monza"],
        vec!["--catalogue", "--gp", "Baku"],
        vec!["--catalogue", "--session", "latest"],
        vec!["--catalogue", "--year", "not-a-year"],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_sectorline"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("Cannot open catalogue"));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn catalogue_corrupt_or_missing_snapshot_is_not_replaced_by_fallback() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target")
        .join(format!("catalogue-corrupt-{}.json", std::process::id()));
    std::fs::write(&path, b"not json").unwrap();
    assert!(Catalogue::load(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"not json");
    std::fs::write(&path, vec![b' '; 8 * 1024 * 1024 + 1]).unwrap();
    assert!(
        Catalogue::load(&path)
            .unwrap_err()
            .to_string()
            .contains("size limit")
    );
    std::fs::remove_file(&path).unwrap();
    assert!(Catalogue::load(&path).is_err());
    assert!(!path.exists());
}
