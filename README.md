# Sector Line

An experimental Rust terminal race dashboard, extracted from the random workspace.

## Run from this directory

```powershell
cargo run --release -p sectorline
cargo run --release -p sectorline -- --demo
cargo test --workspace --locked
```

The cached demo runs without Python. Historical extraction through
`python_bridge/extract.py` requires Python with FastF1, NumPy, and pandas.

## Current status

This version combines session metadata with generated animation frames.
The current live labels do not indicate a continuous observed telemetry feed.
Faithful historical replay and authenticated live ingestion remain future work.

No public release or data redistribution license is configured.
