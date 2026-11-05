# Sector Line

An experimental Rust terminal race dashboard. Current playback is a **generated
demo**, using a mixture of historical metadata and simulated car movement,
telemetry, gaps, and conditions. It is not observed race replay.

## Run from this directory

Rust 1.89 or newer is required.

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

## Real-data acquisition (developer harness)

T05 adds incremental OpenF1 downloads, independent of demo playback. After fetching
the 2024 catalogue, this harness writes scratch chunks to a **new** user-cache directory:

```powershell
cargo run --release --locked -p sectorline --example acquire -- `
  "$env:LOCALAPPDATA\sectorline\catalogue.json" 9590 `
  "$env:LOCALAPPDATA\sectorline\scratch-9590"
```

It uses conservative free-tier pacing and may take several minutes. Optional final
arguments `START_RFC3339 END_RFC3339` narrow the high-frequency time range. Raw
responses, validated records and field diagnostics are separate. Missing or invalid
values are not fabricated. This is acquisition evidence, **not yet a playable,
resumable recording cache**. Use the cache harness below for a published recording.

## Offline recording cache

After fetching the catalogue, download or resume the exact session:

```powershell
cargo run --release --locked -p sectorline --example cache -- `
  "$env:LOCALAPPDATA\sectorline\catalogue.json" 9590
```

The default cache root is `%LOCALAPPDATA%\sectorline\sessions` on Windows, or
`$XDG_CACHE_HOME/sectorline/sessions` / `~/.cache/sectorline/sessions` on Unix.
Use `--root PATH` for an explicit alternate location. Repeating the command
resumes verified chunks or opens an already completed recording without networking.
Add `--verify` to require an existing completed recording and validate it offline.

The versioned manifest records exact session/meeting IDs, year, session name/type,
requested coverage, source URLs, retrieval dates and SHA-256 checksums. Interrupted
downloads remain in a `.partial` directory; only a fully validated recording is
published. Concurrent writers are prevented by an OS lock that releases on exit.
Checksums detect file corruption; they do not authenticate the data provider.
Malformed/incompatible recordings produce an error. Use a new `--root` to rebuild
explicitly while preserving the existing files.

Existing T05 scratch downloads can be imported with `--import SCRATCH_DIR`; raw
responses are revalidated and hashed, rather than trusting the saved normalized
JSON. Import resumes when repeated. The recording covers the requested time range,
including explicit empty queries; source gaps remain gaps. Cache completion does
not yet enable TUI replay—that integration follows T07–T09.

No public release or data redistribution license is configured.
