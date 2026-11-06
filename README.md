# Sector Line

An experimental Rust terminal race dashboard with a generated demo and an initial
recorded timing view backed by OpenF1 historical data. Recorded replay uses explicit
event-time estimates and starts paused. The demo's simulated movement, telemetry,
gaps and conditions remain clearly labelled DEMO.

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

Starting without options opens the cached demo and session catalogue selector. `--demo` (or
`--headless`) runs 20 generated frames without opening the TUI. Sessions generated
through `--gp` and `--year` remain demos. Catalogue recordings open the separate
RECORDED SNAPSHOT view.

**Live access is unavailable.** `--live` and `--gp live` exit with an explanation
before loading a cache or contacting a provider. The Python bridge also rejects
live requests, and failed historical extraction cannot fall back to the latest
OpenF1 session. Tyres, flags, track context and authenticated live ingestion remain future work.

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

This command browses **metadata**. Use the TUI selector or `--prepare-session ID`
to load recorded timing.
It reports cancellation, missing completion timestamps, and sessions that have not
ended. A completed session does not guarantee telemetry coverage. Selecting an ID
through `--catalogue --session` only selects metadata; it does not start a download.

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
including explicit empty queries; source gaps remain gaps. A completed cache
opens the manual recorded timing view through the TUI loader.

No public release or data redistribution license is configured.

## Background session preparation

Press `s`, then `c` or `/` to search a previously fetched catalogue (for example,
`2024 Monza R`). Select a provider session to verify its recording cache or resume
its download in the background. If the catalogue is missing, fetch it first with
`--catalogue --refresh --year 2024`. Search never assumes a year or substitutes
generated data for a recording.

During loading, `Esc` cancels, `r` retries the last request, `s` opens the selector,
and `q`/Ctrl+C quits. The previous session remains usable on cancellation or error.
One worker keeps only the newest pending selection; old results cannot overwrite
it. Progress and completion slots are bounded. Network calls retain their timeouts;
cancellation takes effect when an in-flight call returns, while input stays usable.

You can also start preparation directly:

```powershell
cargo run --release --locked -p sectorline -- --prepare-session 9590
```

Use `--catalogue-cache PATH` and `--recording-root PATH` to override user cache paths.
Preparation has a 15-minute job deadline; retry resumes saved chunks. A ready
recording opens **RECORDED SNAPSHOT / PAUSED**. Space starts/pauses timestamp-driven
playback; `1`/`2`/`5`/`0` select 1x/2x/5x/10x. Home restarts at the beginning,
paused. `Left`/`Right` seek one second and `Page Up`/`Page Down` seek 60 seconds, pausing
playback; Up/Down selects a driver. Opening the session selector pauses playback. The selector's generated option opens the bundled cached demo; legacy
`--gp` extraction remains a startup CLI path.

Recorded values use the latest eligible source observation, with its age shown.
Missing, quarantined and ambiguous values remain `--`; unknown DRS codes stay
visible. Retrieval time never stands in for original availability. The view says
when it uses event-time estimates because original delivery timing is unknown.
The timeline begins at the earliest retained core observation, including pre-session
records; it does not claim that instant is lights-out. Completed-lap sectors use
the conservative timing rules below. Tyres, flags, weather and geometry follow
in later tasks.

Same-time records resolve deterministically by channel/driver and retrieval time.
Tied conflicting revisions become unavailable. Exact historical revision timing
cannot be reconstructed from today's provider snapshot. Raw provenance remains
in the cache. Core event allocation is capped at 192 MiB; oversized coverage fails
explicitly instead of exhausting memory. No resampling or interpolation is added.

For an offline full-recording check against raw telemetry:

```powershell
cargo run --release --locked -p sectorline --example replay_check -- `
  "$env:LOCALAPPDATA\sectorline\catalogue.json" 9590
```

## Playback clock and completion

Playback uses monotonic elapsed time, independently of source sampling cadence and
terminal redraws. Controls settle time at the old speed before applying a change;
paused time is never added on resume. Source gaps retain their original spacing.
The last timestamp shows ENDED and stays fixed; Space cannot wrap or restart it.
Use Home deliberately, then Space to play again. The generated demo also follows
its own timestamps and stops at the end.

Rendering is capped at 30 Hz and runs on input/resize/loader updates or advancing
playback. Paused idle playback does not redraw continually. Rendering performs no
network/cache reads. Release performance budgets still require their later full
measurement tasks; current regression checks are not emulator latency guarantees.

## Seek and recording synchronization

Press `g` in a recorded session to open navigation. Playback pauses while editing;
Enter applies, Esc cancels, and errors stay beside the input. Every seek stays
paused; Space resumes at the chosen speed. Driver selection survives a seek.

- `time 12:34.500` goes to observation timeline time 12 minutes, 34.5 seconds.
- `lap 12` goes to the **selected driver's** recorded lap start. Missing,
  conflicting or out-of-coverage lap starts are rejected.
- `offset -00:12.500` sets a signed recording offset; `offset +00:00` resets it.
- `recording 15:00` aligns the dashboard with that external video/recording time.

The rule is **observation timeline time = external recording time + offset**.
Zero is the first core observation in the cached coverage, not lights out, the
scheduled start, or the video's start. For example, if timeline 10:00 aligns with
video 10:12.5, set offset -00:12.500, then enter the video's current `recording`
time. Pause the video while aligning and resume both at the same speed. Offset
changes do not jump playback; a `recording` command applies the mapping. This is
manual synchronization, without video control or automatic drift correction.
Offsets last only for the loaded recording. Invalid/out-of-range commands retain
the current state; relative arrow/page seeks stop at the coverage edges.

Seeks restore complete reducer checkpoints and replay only the remaining events.
Quality, sample ages, locations and source provenance follow the target time;
future samples do not survive a backward seek. Lap starts are navigation metadata,
not evidence that a lap's sectors or time were available then. Race-control history
follows in its event task. Completed-lap timing and provisional bests now rebuild
through the same checkpoints (see below).

## Recorded classification and lap timing

Order follows timestamped source positions. Lap counts, lapped gaps, missing
intervals, stationary telemetry and retirement labels never invent or reorder
positions. Ties remain visible until a later source observation resolves them.
Status is shown only from recognized, timestamped, explicit driver messages;
missing observations do not establish DNF. Final session-result DNF/DNS/DSQ flags
have no publication boundary in the historical schema and are not injected into
replay, including at the last telemetry timestamp.

Selected-driver detail shows the **last completed lap**, its sectors, personal
best and session best. Archived sectors are held until a conservative completion
estimate: the later of start plus duration/sector sum and the next lap start when
available. A next lap start can also establish completion when the previous
start/duration is missing; missing measured values remain `--`. These are
approximate event-time estimates, not original sector publication timestamps.

An asterisk marks a **provisional archive best** with unknown eligibility. This
is a minimum of admitted, positive, consistent, non-pit-out lap records after
recognized timed deletions; it is not certified official fastest-lap status.
Unknown pit-out eligibility is excluded. A timed deletion invalidates the named
lap for best-time calculations at that message's boundary; reinstatement makes
it a provisional candidate again. Completed values remain visible with an
`invalid` label, retaining the measured record. The message text's named lap is
used, not race-control's current-race-lap field. Unrecognized message formats do
not become inferred decisions. Conflicting lap summaries suppress values and
bests; inconsistent lap/sector totals suppress the lap duration and best.

The historical archive does not expose a general correction/publication history.
Later downloaded corrections can still differ from what a live viewer saw.
[OpenF1's documented lap fields and final-result semantics](https://openf1.org/docs/)
define this limitation. Checkpoints include completed laps and timed decisions,
so seeking backwards rebuilds bests, invalidation and quality at the target time.
