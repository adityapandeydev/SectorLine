# Sector Line

An experimental Rust terminal race dashboard. Current playback is a **generated
demo**, using a mixture of historical metadata and simulated car movement,
telemetry, gaps, and conditions. It is not observed race replay.

## Run from this directory

```powershell
cargo run --release -p sectorline
cargo run --release -p sectorline -- --demo
cargo test --workspace --locked
python -m unittest discover -s python_bridge -p test_modes.py
```

The cached demo runs without Python. Historical extraction through
`python_bridge/extract.py` requires Python with FastF1, NumPy, and pandas.

## Current status

Starting without options opens the demo session selector. `--demo` (or
`--headless`) runs 20 generated frames without opening the TUI. All current
sessions, including those generated through `--gp` and `--year`, remain demos.

**Live access is unavailable.** `--live` and `--gp live` exit with an explanation
before loading a cache or contacting a provider. The Python bridge also rejects
live requests, and failed historical extraction cannot fall back to the latest
OpenF1 session. Faithful historical replay and authenticated live ingestion
remain future work.

## Completed-session catalogue

Discover OpenF1 meetings and fetch one explicit year's session metadata:

```powershell
cargo run --release --locked -p sectorline -- --catalogue --refresh
cargo run --release --locked -p sectorline -- --catalogue --refresh --year 2024
cargo run --release --locked -p sectorline -- --catalogue --year 2024 --search "Monza Q"
cargo run --release --locked -p sectorline -- --catalogue --year 2024 --session 9590
```

Only `--refresh` contacts the provider. Later searches and exact-ID selections use
the cached catalogue offline. The default path is
`%LOCALAPPDATA%\sectorline\catalogue.json` on Windows, or
`$XDG_CACHE_HOME/sectorline/catalogue.json` / `~/.cache/sectorline/catalogue.json`
on Unix. Override it with `--catalogue-cache PATH`.

This command browses **metadata**, independently of the existing demo selector.
It reports cancellation, missing completion timestamps, and sessions that have not
ended. A completed session does not guarantee telemetry coverage. Selecting an ID
does not download or play a recording yet; those tasks follow next. See
[catalogue behavior and verification](docs/catalogue.md).

No public release or data redistribution license is configured.
