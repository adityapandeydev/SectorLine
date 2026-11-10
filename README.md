# Sector Line

An experimental Rust terminal race dashboard with a bundled generated demo and
recorded timing backed by OpenF1 historical data. Recorded snapshots use explicit
event-time estimates and start paused. Simulated movement, telemetry, gaps and
conditions remain labelled DEMO.

## Standalone use

The binary runs from any working directory. It needs neither the source checkout
nor Python, a separate demo cache, or a Sector Line account. Windows standalone
checks pass; Linux/macOS native verification and release automation remain pending.
No public release archive is published yet.

Build with Rust 1.89 or newer, then copy the binary wherever you want:

```powershell
cargo build --release --locked -p sectorline
# Copy target\release\sectorline.exe to a directory of your choice.
# From that directory:
.\sectorline.exe
.\sectorline.exe --demo
.\sectorline.exe --help
.\sectorline.exe --paths
```

With no command, the TUI opens the bundled demo and session selector (minimum
80x24 cells). `--demo` / `--headless` runs 20 generated frames without a terminal
and does not create cache files. `--version` prints the binary version.

For a first historical recording, fetch metadata, select an exact completed ID,
then download and replay it:

```powershell
.\sectorline.exe --catalogue --refresh --year 2024
.\sectorline.exe --catalogue --year 2024 --search "Monza Race"
.\sectorline.exe --download 9590
.\sectorline.exe --replay 9590
```

OpenF1 [documents historical access from 2023 onward as free and unauthenticated](https://openf1.org/docs/).
Availability, pacing and provider failures still apply; a metadata entry does not
guarantee telemetry coverage. Downloads can take several minutes. Repeating
`--download ID` resumes partial chunks or validates an existing completed cache.
`--replay ID` never downloads: it needs both the cached catalogue and completed
recording. Missing or corrupt caches show an error and are never silently rebuilt.

`--prepare-session ID` opens a cache or downloads in the background. Add `--offline`
to prohibit acquisition for the entire TUI session, including later selections and
retries. Bare `--offline` opens the bundled demo/selector with the same restriction.
Only explicit catalogue `--refresh` and recording download/preparation contact the
provider. No configuration file is required; flags control behavior.

`--catalogue-cache PATH` and `--recording-root PATH` override storage. Default
locations are `%LOCALAPPDATA%\sectorline\catalogue.json` and
`%LOCALAPPDATA%\sectorline\sessions` on Windows; Unix uses
`$XDG_CACHE_HOME/sectorline` or `~/.cache/sectorline`. `--paths` prints the resolved
locations without creating them. Recordings stay in that user cache when the
binary moves; back up the catalogue alongside recordings for portable offline use.

## Developer checks and optional Python bridge

```powershell
cargo test --release --workspace --all-targets --locked --offline -- --test-threads=1
python -m unittest discover -s python_bridge -p test_modes.py
```

The Python command tests the optional developer bridge; it is not needed by the
binary. Historical demo generation remains available through
`python_bridge/extract.py` with Python/FastF1/NumPy/pandas. The binary's former
`--gp` generation route now directs users to the bridge or native recordings;
`--year` belongs to `--catalogue` in the standalone CLI.

**Live access is unavailable.** `--live` and `--gp live` fail before loading data or
contacting a provider. Historical demo generation never produces an observed
recording. Recorded maps use validated session-local lap traces.

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
opens recorded playback through the background TUI loader.

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
playback; Up/Down selects a driver. Opening the session selector pauses playback. The selector's generated option opens the bundled demo without Python or disk files.

Recorded values use the latest eligible source observation, with its age shown.
Missing, quarantined and ambiguous values remain `--`; unknown DRS codes stay
visible. Retrieval time never stands in for original availability. The view says
when it uses event-time estimates because original delivery timing is unknown.
The timeline begins at the earliest retained core observation, including pre-session
records; it does not claim that instant is lights-out. Completed-lap sectors use
the conservative timing rules below. Tyres, flags and weather follow source-timed
condition rules; recorded maps use the observed-lap reference described below.

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

## Replay conditions and freshness

The recorded view shows source-scoped track/local flags, safety-car and session
messages, weather, selected-driver tyres and recorded pit visits. Missing
conditions remain `--`; rainfall zero means an actual dry report, not missing data.
A `~` prefix marks a held **stale** report. Neither stale nor missing flags imply
a clear track. A local sector clear does not clear other sectors or the track;
restart and safety-car-ending messages do not automatically invent green flags
or a returned safety car. Recognized messages retain their own chronology.

Stint summaries have no observation timestamp. Compound/initial tyre age are
anchored to the matching lap's approximate start as **event-time estimates**;
without that evidence they stay unavailable. Future `lap_end` is never displayed
or used to promise the stint is still active. Tyre age means completed laps:
initial source age plus contiguous, evidenced completed laps since the stint
start. It is not fractional tyre wear or a prediction. Unknown age/compound and
gaps stay unknown. Old stint evidence becomes stale when lap evidence stops.

Pit rows do not document whether `date` denotes entry, service or exit. The view
therefore shows a **recorded visit**, with active phase unknown. Archived lane
and stationary durations are delayed until date plus lane duration, when present,
as a conservative estimate; they are not backfilled at pit entry. Lane time is
not stationary service time. Conflicting lane aliases suppress the visit values.

Freshness uses replay time, so pausing does not age data and seeking recomputes
it. Current conservative policies are: telemetry/location 2 seconds, intervals
10 seconds, weather/flag/session reports 120 seconds, and inferred stint evidence
180 seconds. Sparse positions and completed lap/pit history do not expire as
continuous samples. These thresholds are application policy, not guarantees of
provider delivery; stale values retain their timestamps and never become zero.

Conditions are normalized in the existing background loader, then projected from
small in-memory source channels at the target time. Seeking does not reuse a
future conditions snapshot. Render callbacks perform no file or network reads.
The historical provider still lacks original delivery/revision history, so this
is source-based event-time replay, not an exact reconstruction of a live feed.

## Terminal workspace

Sector Line uses the same compact shell for generated demo and recorded replay:
two header rows, one view-tab row, a flexible body, three timeline rows, one
source/load-status row and one contextual-help row. Exact OpenF1 session ID,
recorded/demo provenance and playback state stay visible around overlays.

- `Tab` / `Shift+Tab`: switch Timing, Driver and Conditions.
- `[` / `]`: scroll Driver or Conditions; arrows still select the driver.
- `?`: open scrollable help and the full load/error message; Up/Down scroll,
  Esc closes. Opening help pauses playback; Space resumes after closing.
- `s`: sessions; `g`: recorded go-to; `r`: retry the last load. Error/loading
  footers expose their retry/cancel controls. Existing playback keys remain.

At **144x40 or larger**, timing, context and selected-driver panes appear side by
side. At **120x32 or larger**, timing and context share the body; driver detail
has its own tab. At **80x24**, each tab uses the full body width. Smaller windows
show the minimum-size notice; Ctrl+C always quits, including inside text input.
The geometry is measured in terminal cells; font appearance is controlled by your
terminal. The shell uses terminal colors, reverse-video selection and text
markers. Set `NO_COLOR` before launch for a monochrome workspace.

The timeline is playback progress, not predicted race distance. `~` stale and `--`
unavailable retain their meanings. Track geometry stays explicitly unavailable
until its validation/view task; no demo track is substituted into recorded mode.
Detailed timing columns, calibrated track rendering and richer driver/event views
continue in the next tasks. The previous framed gauges/demo radar are retired
from the shared workspace.


## Timing table

The timing table uses fixed, right-aligned numeric columns. `Gap(s)` and `Int(s)`
are seconds; `+nL` means laps behind and `leader` is explicit. `--` is unavailable,
`~` marks a stale held value, and `...` means the value exceeds its column width.
The Driver view retains full timing detail.

`Ty` uses S/M/H/I/W for soft/medium/hard/intermediate/wet, `?` for an unrecognized
source compound. `Age` is tyre age in completed laps; `Lap` counts completed laps.
The expanded table adds interval, last lap in seconds, speed in km/h and `AgeS`
(telemetry observation age in seconds). Narrow panes prioritize classification,
gap, tyres and evidenced RET/DNF/DNS/DSQ status. A historical pit summary does not
imply that a car is currently in the pits.

`Tm` is a mnemonic of the source team name, with deterministic numbered labels
where mnemonics collide. Full names appear in Driver and the Team key. Optional
source colours use the basic ANSI palette; team text and `>` selection also work
with `NO_COLOR`. Missing team metadata stays unknown. Up/Down or j/k follows the
selected driver's identity through reorders and scrolls larger grids into view;
the heading shows the visible range and total.


## Recorded track view

Tab to **Track** for the full map; larger Timing views also show a map beside the
classification and flag summary. The background is a **static observed lap trace**
from this recording, labelled with its driver and lap. It is approximate source XY,
not a surveyed circuit outline, lane position, or start/finish detector. It uses
archived lap context even before that lap completes in playback; car markers obey
replay availability independently. No separate circuit service, account or download
is added.

A reference needs a complete 30–300 second lap with at least 50 finite, contiguous
location samples, plausible step distances and closure, and known non-pit context.
Recorded pit laps, pit-out laps and their preceding laps are excluded, as are
unknown/conflicting lap or pit context and missing pit-channel coverage. The trace
and all cars share one source coordinate frame and one rotation/scale transform.
Without a validated reference the map explains that it is unavailable; timing
continues without borrowing the generated demo circuit.

Car markers use a **500 ms display delay**. Linear XY interpolation requires two
already available samples no more than 750 ms apart, a short spatial step and the
same known non-pit lap. Otherwise `~` marks a held sample. Missing/conflicting or
more than two-second-old locations are hidden; out-of-frame locations are counted
as unshown. Classification and gaps always come from timing records. Selected
labels win collisions; the footer reports omitted labels and shown-car count.

In Track view, `,` / `.` rotate 15 degrees and `-` / `+` adjust physical cell
height/width from 1.0 to 3.0 (default 2.0). This compensates for terminal font
geometry without independently stretching the map axes. Rotation has no implied
geographic north. Unicode braille needs terminal font support; labels, selection
and the basic ANSI/no-colour palette remain text based. Demo maps remain explicitly
generated.


## Driver details and evidence timeline

Driver shows typed speed, RPM, gear (`N` is neutral), throttle, brake on/off and
DRS on/off/eligible or its unknown source code. Missing values stay `--`; stale
held values carry `~`, with sample age measured in replay time. Wider views put
history beside the readout; use `[` / `]` to scroll compact views.

Speed and throttle history plot admitted source points from the last ten seconds
(up to 512 indexed observations), with no connecting lines or invented zeros.
A blank means no eligible value at that plotted location. The displayed range
comes from those source values; it is not a gauge or physical performance scale.
The evidence section identifies the channel, quality and availability time.

Lap/sector deltas compare the latest comparable completed lap against the best
admitted, explicitly eligible personal lap, including the current lap. Sectors
come from that same reference lap, not an invented combination of best sectors.
Pit-out, deleted, conflicting or unverified laps do not produce comparisons;
missing sectors stay missing. Provisional archive bests remain labelled `*`.

The **Events** tab lists parsed control reports, completed laps, eligibility/status
reports and recorded pit visits already available at the replay time. It is a
normalized evidence timeline, not a verbatim feed of every provider message.
Scope, report timestamp, source evidence, retrieval and known/estimated availability
are explicit. Conflicting same-time reports stay unresolved. Retrieval is never
used as the report's arrival time.

Up/Down or j/k select a report; PgUp/PgDn move ten rows. Enter pauses and seeks to
its **availability time**, so delayed reports are visible at the destination.
Reports outside recording coverage cannot be jumped to. The latest row follows
new reports; selecting an older row holds the browsing position. Backward seeks
hide future reports and their counts. `[` / `]` scroll the selected evidence.
Generated demos show no substitute provider event stream.


## Terminal compatibility and cleanup

Run the regular interface with plain ASCII and no semantic colours:

```powershell
cargo run --release --locked -p sectorline -- --ascii --no-color
```

`SECTORLINE_ASCII=1` and `NO_COLOR=1` provide the same options. ASCII mode uses
regular dots/stars for maps and history, simple overlay borders, common Latin
accent transliteration, and `?` for unsupported label glyphs. IDs and numeric
values keep their slots. It needs no emoji, Nerd Font, image protocol or braille
font. No-color mode keeps text markers, bold and reverse selection.

Below 80×24, a resize/quit fallback replaces the workspace. Very small windows
prioritize `q`; hidden text-entry controls cannot consume it. Restoring the size
preserves the current session and driver. Help measures its wrapped content at
the current size: PgUp/PgDn move a page and Up/Down a line.

Interactive mode requires terminal input and output; redirected invocations get
a clear error. `--demo`/`--headless`, `--help` and `--catalogue` remain usable
without a TUI. A terminal guard restores raw input, alternate screen and cursor
on normal/error returns and unwinding UI panics, including partial setup. Cleanup
attempts every step even if another fails; failed steps are retried on drop.
Cleanup failures are reported rather than masking an earlier runtime error.

Ctrl-C input and OS interrupt signals request graceful shutdown; Unix termination
signals are also handled. A caught loader-worker panic does not tear down the
active UI. Quit cancels loading without joining an in-flight provider call.
Forced process kills, panic-abort builds and a disconnected terminal cannot be
promised graceful cleanup.

For a disposable terminal/PTY lifecycle check, build and run the test example:

```powershell
cargo run --release --locked -p sectorline --example terminal_smoke -- normal
```

Other modes are `error`, `panic`, `startup-error`, `worker-panic` and `signal`.
The failure modes are deliberate: the final `RESTORED` marker must say
`raw_off=true`. `signal` waits up to 15 seconds for an OS interrupt (not a raw
Ctrl-C input byte). These probes change terminal modes; run them in a separate
test terminal. Windows ConPTY resize, quit, Ctrl-C, Ctrl-Break and restoration
checks passed locally. Native Linux/macOS smoke verification remains pending.

Qualifying and sprint qualifying show the reported Q1/Q2/Q3 or SQ1/SQ2/SQ3 part
and a best completed non-pit-out lap scoped to that part. A part needs an explicit
start message and a usable lap-start link; missing evidence withholds the best.
A lap started before a reported chequered close may finish afterward. `*` marks
unknown eligibility: it is provisional, not an official classified result.
Practice shows a session best. `Rpt` preserves the last provider-reported position;
it is not a new ranking calculated from these bests. Non-race tables omit race
gaps and show `L#` as the completed lap number, without a race lap total.

`sched` is catalogue session-duration metadata. `observed` is the replay UTC cursor;
the elapsed/span above it belongs to the recording, including any pre-session
coverage. There is no inferred countdown, elimination, or final classification.
Missing session/phase/telemetry evidence is labelled partial; unknown session kinds
never silently select the race layout.


### Contextual insights

In a recording, open Driver or Events and use `[` / `]` to scroll to Context.
Context describes the selected driver at the current replay cursor, independently
of the report selected in Events. Every insight shows its evidence window,
availability timestamp and rule explanation.

Race/sprint leader-gap changes need at least three usable reports spanning five
seconds, with no sample gap over ten seconds and a latest report at most ten
seconds old. The default observation window is 30s; the Rust `insights::Policy`
API accepts 10-120s. Reported intervals ahead at most 1s get a separate description.
Missing, conflicting and lapped values suppress gap calculations.

Position changes retain their reported meaning; their cause stays unestablished.
Pit/stint notices use reports at most 60s old and preserve unknown entry/exit or
change times. Eligible bests use admitted completed laps and explicit eligibility;
provisional archive bests remain excluded. Their recording minimum covers the
admitted recording, rather than asserting an official session result. Qualifying
bests stay within the evidenced part. Historical best facts remain valid as the
cursor advances; current interval freshness is a separate rule.

At most one description of each kind is shown for the selected driver. Repeated
redraws do not queue notices, and seeking rebuilds context from available evidence.

### Replay performance checks

Run `cargo bench --locked -p sectorline --bench replay -- --manifest PATH --frames 1800`
with a completed recording manifest. It measures validated load, 100 checked seeks,
and update/render work at 144x44 across five views. `--legacy` measures the former
verify-then-normalize path. `--synthetic-capacity` instead generates a temporary
raw-on-disk two-hour, 22-driver workload and measures it in a fresh child process.
Explicit recording failures exit nonzero; benchmarks never substitute demo results.

On the i7-12700H Windows reference machine, three paired Monza cache runs reduced
median load from 4.84s to 2.28s. The two-second warm-start target remains unmet.
Frame work, seeks, the declared synthetic capacity case and paused CPU were within
their budgets. TestBackend timings and ConPTY first-output traces do not establish
visible terminal latency. A three-hour native run completed 180 session-switch
cycles with a 169 MiB peak working set and near-flat final-hour memory usage; no
sustained unbounded growth was observed in that workload.


### Fault verification

Provider, archive and replay regressions are available in `provider_faults`,
`cache_faults` and `replay_invariants`. They use localhost fault responses and
attributed sparse historical excerpts, plus labelled synthetic ordering cases.
Failed loads retain the previous playable recording and show a diagnostic;
corrupt completed caches are rejected without automatic repair. Interrupted
responses and invalid JSON name the affected channel and failure cause.

Run `cargo test --release --workspace --all-targets --locked --offline -- --test-threads=1`
for the optimized suite. Tests use no external provider network access; Windows
fixtures need localhost sockets and temporary junction creation. The suite does
not certify provider completeness or untested platform behavior.


### CI and draft archives

GitHub Actions checks Windows x86-64, Linux x86-64 (Ubuntu 24.04) and macOS
Apple Silicon: formatting, warning-free Clippy, offline Rust fixtures, Python
tests, optimized builds and an unpacked-binary smoke check. No race-data download
or provider credentials are used. Linux/macOS archives are explicitly experimental
until interactive terminal, resize and restoration checks pass on native machines.

Run the **Draft release** workflow manually to get Actions artifacts (14-day
retention); this does not create a tag or publish a release. Each native archive
contains the executable, README, release notes and build provenance, with a SHA-256
sidecar. The unpack test starts the embedded demo outside the checkout with an
empty PATH and fresh profile. Python is build tooling, not a runtime requirement.

A pushed `v<package-version>` tag runs all checks before creating an **unpublished
draft**. Tags must match `f1_tui/Cargo.toml`; public publication requires owner
review. The code licence, remaining performance gates and platform validation
remain open. See the bundled release notes before sharing builds.
