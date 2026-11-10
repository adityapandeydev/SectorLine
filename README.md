# Sector Line

A Rust terminal dashboard for replaying historical Formula 1 timing and telemetry.
Download a completed session, replay it offline, inspect drivers and race-control
events, and align the timeline with a recording you are watching.

Recorded mode uses OpenF1 observations and labels missing, stale and estimated
data. The bundled demo is generated. Live timing and video playback are not
implemented; no account, subscription or Python installation is needed to run
the binary.

## Install

No public release is available yet. Review builds are available from successful
[Draft release workflow runs](https://github.com/adityapandeydev/SectorLine/actions/workflows/release.yml).
Sign in to GitHub, open a completed run, download the artifact for your platform,
and extract both the Actions artifact and the archive inside it.

| Platform | Archive | Status |
| --- | --- | --- |
| Windows x86-64 | `.zip` | Native automated and terminal lifecycle checks passed |
| Linux x86-64 | `-experimental.tar.gz` | Built on Ubuntu 24.04; interactive terminal checks pending |
| macOS Apple Silicon | `-experimental.tar.gz` | Native automated checks passed; interactive checks pending |

There are no Intel macOS or Windows ARM builds. Binaries are unsigned; macOS
builds are not notarized. Linux compatibility depends on the system libraries
available on the host; the build does not target every distribution.

Keep the executable, license and notices together. Each archive has a `.sha256`
sidecar and `BUILD-INFO.json` containing its target, source commit and binary hash.
On Windows, compare the sidecar with the result of `Get-FileHash` using SHA256.
On Linux, use `sha256sum --check` with the sidecar; on macOS use `shasum -a 256 -c`.
Checksums detect corruption; they are not signed authenticity proofs.

To build from the repository, install Rust 1.89 or newer and your platform's
native linker/build tools:

```powershell
cargo build --release --locked -p sectorline
```

The executable is `target/release/sectorline.exe` on Windows and
`target/release/sectorline` on Linux/macOS. Copy it out of the checkout if desired.

## First run

The examples below use PowerShell from the executable's directory. On Linux/macOS,
replace `.\sectorline.exe` with `./sectorline`.

```powershell
.\sectorline.exe --version
.\sectorline.exe --help
.\sectorline.exe --demo
.\sectorline.exe
```

With no command, Sector Line opens the interactive generated demo and session
selector. `--demo` checks 20 generated frames without opening the TUI or writing
a cache. Interactive mode requires terminal input and output.

Use a terminal of at least **80 columns by 24 rows**. At 120x32, timing and context
appear together; at 144x40, selected-driver details appear alongside them. Smaller
windows show a resize notice. `--ascii` avoids braille map/history glyphs;
`--no-color` or the `NO_COLOR` environment variable selects monochrome rendering.

## Replay a historical session

Fetch the year's catalogue, find a completed session and download its exact ID:

```powershell
.\sectorline.exe --catalogue --refresh --year 2024
.\sectorline.exe --catalogue --year 2024 --search "Monza Race"
.\sectorline.exe --download 9590
.\sectorline.exe --replay 9590
```

Session 9590 is the 2024 Monza race. Check the catalogue result before choosing
another ID. Downloads can take several minutes and depend on provider coverage,
rate limits and connectivity. Repeating `--download ID` resumes saved chunks or
validates an existing completed recording. Progress, retries and failures are
reported; a catalogue entry alone does not establish data completeness.

`--replay ID` requires the cached catalogue and a completed recording. It runs
offline, opens paused, and prevents downloads throughout that TUI session.
For background acquisition instead, use `--prepare-session ID`. In the TUI,
press `s`, then `c` or `/` to search the cached catalogue and select a session.
The previous recording stays available if a new load fails or is cancelled.

| Command | Behavior |
| --- | --- |
| No command | Open the generated demo and session selector |
| `--demo` / `--headless` | Check 20 generated frames without a terminal |
| `--catalogue --refresh --year YYYY` | Fetch a year's historical session metadata |
| `--catalogue --year YYYY --search TEXT` | Search cached metadata without networking |
| `--download ID` | Download/resume and validate an exact session |
| `--replay ID` | Open a completed recording; acquisition stays disabled |
| `--prepare-session ID` | Open cached timing or acquire it in the background |
| `--offline` | Disable acquisition throughout an interactive session |
| `--paths` | Print resolved cache paths without creating them |

`--live` is rejected. There is no live-data login or F1 TV integration.
No configuration file is required.

## Controls

| Key | Action |
| --- | --- |
| `Tab` / `Shift+Tab` | Cycle Timing, Driver, Conditions, Track and Events |
| `Space` | Play/pause |
| `1`, `2`, `5`, `0` | Set 1x, 2x, 5x or 10x playback speed |
| `Home` | Restart at the beginning, paused |
| `Left` / `Right` | Seek one second backward/forward, paused |
| `Page Up` / `Page Down` | Seek 60 seconds; in Events, move ten reports |
| `Up` / `Down`, `k` / `j` | Select a driver; in Events, select a report |
| `Enter` in Events | Seek to the selected report's availability time |
| `g` | Open recorded-session navigation |
| `s` | Open the session selector and pause playback |
| `[` / `]` | Scroll detail panes outside Timing |
| `,` / `.` in Track | Rotate the map by 15 degrees |
| `-` / `+` in Track | Adjust terminal-cell aspect ratio |
| `?` | Open help and the full load/error message; pauses playback |
| `r` | Retry the last load request |
| `Esc` | Close an overlay or cancel loading; quit from the main view |
| `q` / `Ctrl+C` | Quit; use Ctrl+C while editing text |

Help scrolls with arrows or page keys. At the last timestamp playback stays
ENDED; use Home to restart. Opening an overlay does not resume playback when
it closes. Normal exit and handled errors restore terminal modes; forced process
termination may prevent cleanup.

### Align with a video recording

Press `g` in recorded mode. Enter accepts a command; Esc cancels. A seek leaves
playback paused.

- `time 12:34.500`: seek to 12 minutes, 34.5 seconds on the observation timeline.
- `lap 12`: seek to the selected driver's evidenced lap start.
- `offset -00:12.500`: set the recording-time offset.
- `recording 15:00`: seek using the external recording's elapsed time.

The mapping is **observation time = recording time + offset**. Observation zero
is the first retained core observation, which can precede the race start. If
observation 10:00 matches video 10:12.5, use `offset -00:12.500`. Offset changes
do not seek until a `recording` command is applied. Alignment is manual; Sector
Line does not control the video or correct drift automatically.

## Read the display

Timing follows provider-reported positions. `--` means unavailable, `~` marks a
stale held value, and `*` marks a provisional archive best with unverified
eligibility. Gaps/intervals are seconds; `+nL` is laps behind. Speed is km/h,
tyre age is completed laps, and observation age is seconds of replay time.

Driver shows telemetry, source evidence and descriptive context. Conditions
shows reported flags, weather, stints and pit visits. Events lists normalized
reports already available at the cursor. Track uses a validated observed lap
trace and bounded interpolation of available location samples. It can be absent
when the recording lacks a suitable reference.

Reported position changes are not automatically overtakes. Context describes
evidence windows and rules; it does not predict race results or strategy.
Qualifying bests are scoped to an evidenced part; practice uses session bests.
These are replay observations, not certified final classifications.

See [Data and interpretation](DATA.md) for timing boundaries, precision, freshness,
map behavior and provider attribution.

## Storage and updates

| System | Default cache directory |
| --- | --- |
| Windows | `%LOCALAPPDATA%\sectorline` |
| Linux/macOS | `$XDG_CACHE_HOME/sectorline`, or `~/.cache/sectorline` |

`catalogue.json` holds metadata; `sessions` holds recordings and partial downloads.
`--paths` shows the resolved locations. Override them with `--catalogue-cache PATH`
and `--recording-root PATH`. For portable offline use, copy the catalogue together
with the recordings and set both overrides on the destination machine.

To update, quit the app, keep a copy of the previous executable, then replace it
with the new build. Keep its bundled notices. The user cache stays in place.
Incompatible or corrupt completed caches are rejected rather than silently
rewritten. To download a separate copy, use a different recording root.

To remove recordings, quit all Sector Line processes, inspect the locations from
`--paths`, and delete only the unwanted session's completed and partial directories
under your chosen recording root. This removes offline access until downloaded
again. Removing `catalogue.json` requires another explicit catalogue refresh.
Do not delete an active download or an unrelated cache directory.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Interactive mode refuses to start | Run in a terminal with both input and output attached; use `--demo` for a headless check |
| Resize notice | Enlarge the terminal to at least 80x24 cells |
| Map or text renders incorrectly | Use a monospaced terminal font; try `--ascii --no-color` |
| Catalogue/session missing | Refresh the correct year and verify the exact ID and cache paths |
| Replay cannot open a recording | Complete `--download ID` first; replay does not acquire missing chunks |
| Rate limit, timeout or interrupted download | Retain the partial files, wait, then repeat `--download ID` |
| Cache validation fails | Preserve the error and damaged files; explicitly download to a different root |
| Blank values, missing map or stale markers | Inspect Driver evidence and the limitations in DATA.md; absence is not replaced with simulation |
| Timing differs from a broadcast | Check timeline alignment and source coverage; archive timing does not reconstruct original delivery |

Report defects through [GitHub Issues](https://github.com/adityapandeydev/SectorLine/issues).
Include the binary version, OS/terminal, dimensions, session ID, command and full
error text from help. Exclude credentials and bulk/private recordings.

## Development

The Rust binary contains acquisition, cache validation, normalization, replay and
Ratatui views. The optional Python bridge generates demo/developer data; it is not
a runtime dependency. Standard tests use attributed excerpts and synthetic or
localhost fault fixtures, with no external provider calls.

After the initial build has fetched dependencies:

```powershell
cargo fmt --all -- --check
cargo clippy --release --workspace --all-targets --locked --offline -- -D warnings
cargo test --release --workspace --all-targets --locked --offline -- --test-threads=1
python -m unittest discover -s python_bridge -p "test_*.py"
python -m unittest discover -s scripts -p "test_*.py"
```

CI runs native checks on all three build targets. Tag builds produce unpublished
drafts only. Performance remains bounded by source volume: the reference full-race
validated load measured 2.28 seconds against a two-second target. A three-hour
Windows workload peaked at 169 MiB. Input-to-visible terminal latency and native
Linux/macOS interactive behavior still require validation before a stable release.

## License and attribution

Sector Line code is [MIT licensed](LICENSE). Dependency notices are included in
[THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt). Historical data comes from
[OpenF1](https://openf1.org/docs/); its documented historical access from 2023
onward is free and unauthenticated. This does not establish rights to redistribute
data or broadcast content under the code license.

Sector Line is an independent project, unaffiliated with Formula 1, the FIA, teams
or drivers. Formula 1 names and marks belong to their respective owners. See
[Formula 1's rights guidelines](https://www.formula1.com/en/information/guidelines.4EOKE9RRqevL4niTK9kWyt).
