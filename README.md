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

No public release or data redistribution license is configured.
