# Sector Line implementation plan

Planning baseline: commit `65cb371`; 1 November 2026. Status: T01-T07 completed (baseline, honest modes, typed observations, catalogue, acquisition, offline cache and background session loading); T08-T24 remain planned. Task list: [todo.md](todo.md). Product scope: [../PRODUCT.md](../PRODUCT.md). UI contract: [../DESIGN.md](../DESIGN.md).

## Outcome and release boundary

Build a free, trustworthy historical race companion in Rust: choose a completed session, download it once, replay the observations smoothly, inspect drivers and meaningful events, align the timeline with a personal recording, and use it offline afterward. User confirmed **free replay first; live support later**.

V1 includes race, qualifying, and practice selection, with available features expressed per session. Establish full race fidelity first, then verify session-specific ranking and labels. V1 excludes paid live ingestion, accounts, a cloud backend, predictions about race outcomes, broadcast video/audio redistribution, a web overlay, GPU processing, and Mojo. These are future options rather than hidden prerequisites for completion.

## Current implementation and required changes

| Before: source-grounded finding | After: planned behavior | Why |
|---|---|---|
| `live.rs:313` synthesizes 600 frames, even on the current live route | Explicit generated demo; recorded mode consumes timestamped observations | Avoid passing animation off as telemetry |
| `models.rs` requires values and stores formatted gaps/sectors as strings | Typed numeric values, optional observations, source/time/quality metadata | Missing data must not become plausible constants |
| `stream.rs:93` assumes 200 ms samples and loops | Monotonic replay clock, timestamp indexes, deliberate end state | Irregular sampling and seeking must be correct |
| `live.rs:555` checks cache identity through event-name aliases | Exact provider/session/year/type identity with a versioned manifest | Prevent wrong-year or wrong-session replay |
| `main.rs:171` performs blocking I/O from the UI action handler and swallows errors | Background acquisition with progress, cancellation, request IDs, and visible errors | Input and rendering must remain responsive |
| `live.rs:464` ranks fractional lap distance; gaps are generated constants | Use source classification/intervals; normalize without inventing order | A start-line crossing must not send the leader to the back |
| `ui.rs:69` always displays green flag | Time-aligned track and local flag scope, including unknown | Do not misrepresent session status |
| Fixed 38/min30/28 columns and 3/min16/8/1 rows | Responsive layouts with prioritized information | Current nominal minimum is 96 columns by 28 rows |
| Emoji banners, competing colors, repeated rounded boxes, permanent gauges | Quiet shell, aligned timing table, selected-driver focus, restrained separators | Improve hierarchy without imitating a graphical dashboard |

Existing tests passed at the baseline, but only two unit tests exist. The current generated JSON is not a real-session validation fixture. Source line references describe the planning baseline and will move during implementation.

## Architecture: small boundaries within the existing crate

Retain one Rust application crate initially. Keep `f1_tui/` as the source directory during behavior changes; any directory flattening should be a separate mechanical change. Avoid a plugin framework or crate-per-component architecture.

```text
catalogue + historical HTTP/import
            |
            v
validated raw cache -> normalization -> typed event timeline
                                           |
                                           v
                                deterministic session state
                                           |
                           clock + seek ----+---- contextual insights
                                           |
                                           v
                                read-only UI view model
                                           |
                                           v
                                 Ratatui terminal view
```

Suggested modules (split when their responsibility becomes real): `data/{catalogue,openf1,cache}`, `domain/{model,normalize}`, `replay/{clock,timeline}`, `insights`, `ui/{layout,theme,timing,track,driver,events}`, and existing `app`/entrypoint. File names are implementation guidance, not a requirement to create empty layers.

### Concurrency and storage decisions

- Keep the current synchronous HTTP client on a background worker initially. A full async stack is unnecessary for replay unless measured requirements justify it.
- One owner mutates application/replay state. Workers return bounded messages with request generation IDs; late results from an abandoned session cannot replace the active session.
- Timelines preserve event records; only replaceable display snapshots/progress may be coalesced. Limit event-processing batches so input is not starved. Network timeouts bound shutdown; cancellation is observed between requests/chunks.
- Store raw responses and a normalized, timestamp-indexed replay representation outside the repository in an OS-appropriate user cache. Use versioned manifests, checksums, atomic publication, resumable chunks, and explicit cache cleanup. Never publish a partial dataset as complete.
- Stream raw JSON into compact per-driver/time indexes; do not retain complete raw responses, full materialized frames, and derived copies simultaneously. Add disk-backed chunks if needed for the memory budget. Start with simple files and indexes; introduce a database only for a measured need.
- The existing tracked cache becomes a clearly identified generated demo or small test fixtures. Stop committing bulk downloaded sessions; preserve existing history rather than rewriting it.

## Data contract and truth rules

### Identity and time

Key a session by provider plus provider session ID; store year, event, type, source endpoints, import version, and retrieval time. Validate requested and actual identity before displaying data. Never silently fall back to another year/circuit.

Use UTC event times normalized to elapsed session time. Distinguish planned start, actual start, elapsed clock, race lap, and replay position. Store source event time separately from fetch/arrival time when available. Historical event replay cannot guarantee the exact original broadcast arrival timing if the source does not retain it; document that limit.

State at replay time T may consume only events available by T under the field's availability rule. A final lap record must not reveal a later sector, retirement, fastest lap, tyre change, or final classification early. When the source only supplies a completed-lap summary, reveal its values at the documented completion boundary; do not invent intermediate availability timestamps.

### Values and capabilities

Represent observations with typed numeric value, source, event time, unit, and quality (`observed`, `derived`, `interpolated`, `stale`, `unavailable`, or `generated`). Model actual zero separately from missing. Formatting belongs in the view model.

| Data | Display rule |
|---|---|
| Position and race gaps | Prefer provider records; distinguish seconds, lapped-car markers, and unavailable intervals. Never infer classification solely from track coordinates |
| Lap/sector timing | Publish after actual availability; mark deleted/invalid laps. Personal/session bests use eligible completed observations only |
| Speed/RPM/gear/throttle | Sample-and-hold by timestamp with an age indicator; do not smooth discrete gear/flags or invent values |
| Brake | Display pressed/released/unknown; source brake is not measured hydraulic pressure |
| DRS | Decode documented states; preserve unknown codes; do not force every value into a boolean |
| Position on circuit | Approximate coordinates; interpolation only across bounded, same-driver contiguous samples. Never interpolate across pits, gaps, jumps, or session resets |
| Tyres/pit state | Derive age from valid lap/stint evidence; distinguish pit entry, stop, and exit where the source supports them |
| Flags/race control | Preserve message scope: local yellow is not a whole-track yellow. Unknown/stale flag state must not become green |
| Weather | Latest available observation at T, with its own age; do not share the car-data freshness threshold |
| Track geometry | Session-matched coordinate transform shared by cars and track. If unknown, show unavailable rather than a Baku circle |
| Qualifying/practice | Capability-specific classification, best laps, elapsed/remaining time; no fabricated race gaps or race lap count |

Provisional stale thresholds live in configuration and require provider-cadence validation: car/location samples 2 seconds; intervals 10 seconds; position changes are event-driven and carry forward until the next eligible event, subject to source continuity rather than a fixed silence timeout; weather 120 seconds. In replay, age is T minus observation time, not wall-clock time. Flag state is event-driven and must follow cancellation/expiry semantics rather than a generic fixed timeout. Reject nonfinite/out-of-range data with diagnostics; do not silently coerce it into plausible values.

### Reproducibility and difficult cases

Stable ordering for equal timestamps, deduplication keys, explicit revision/conflict policy, and checkpointed seeking must yield the same state as replaying from the start. Preserve source corrections with an audit trail when available; avoid claiming historical revision accuracy that the provider cannot supply.

Verify starts, safety cars, red flags, restarts, lapped cars, DNF/DNS/DSQ, deleted laps, tyre changes, pit lane, equal timestamps, missing drivers, long sampling gaps, corrupted caches, interrupted downloads, non-race sessions, and schema changes. Obtain/inspect a small set of real public sessions at the data-feasibility checkpoint before expanding implementation. Do not use assumed TV agreement as an oracle.

## UI direction and dimensions

See DESIGN.md for the full contract. The main screen answers: **what is happening at this replay moment, and what is happening to the selected driver?**

- Neutral charcoal/default terminal background, bright primary text, readable muted labels; one selection accent. Team colors identify drivers sparingly; flag/tyre/sector colors retain meaning and text equivalents.
- Timing table is the anchor; map is supporting context. Avoid a box around every number, oversized banners, gradient approximations, emoji-dependent layout, decorative sparkline cards, and repeating “live” labels.
- Proposed wide target 144x44 cells: 2-row header, 1-row view bar, 36-row body, 3-row timeline, 1-row source/status line, 1-row help. Body columns 44 + 1 + 66 + 1 + 32. These are proposed new design values, not current measurements.
- At 120x36, use timing plus map with driver detail as a focused view; at 80x24, use a compact timing-first view with tabs. Below 80x24, retain help/quit and give a clear size requirement. Scroll rows for grids larger than the viewport; do not assume 20 cars.
- Stable column widths, right-aligned numeric values, fixed decimal precision, clear units, consistent `--` for unavailable, selection anchored to driver identity through reordering.
- Track positions interpolate linearly in event time over validated intervals, not decorative ease curves. Timeline scrub and keyboard navigation are instant. Default redraw cap 30 Hz; draw on state/input/resize changes and while playback motion needs it; paused idle does not redraw continually.

A monochrome dimensional reference of the **current** source layout is supplied outside the repository at `C:\Users\AdityaPandey\.codex\visualizations\2026\11\01\01a197f4-2e1f-78c1-8900-623be0678487\sectorline-layout-reference.html`. It records terminal-cell dimensions and marks proposed dimensions separately in DESIGN.md; no web pixel measurements or font assumptions are substituted for terminal geometry.

## Explainable intelligence

V1 insights are deterministic: selected driver near another car using reported intervals, gap trend over a configurable window, pit/tyre events, eligible personal/session bests, race-control context, and stale-data warnings. Each insight includes event time, evidence window, and a plain explanation. Suppress an insight when evidence is insufficient.

Position changes alone are not automatically on-track overtakes. Pit stops, retirements, and classification corrections must be considered; when ambiguous, say position change. Trends are descriptive rather than predictions. No invented win chances, strategy scores, confidence percentages, or AI-generated commentary. Suppress future knowledge after backward seeks and rebuild insight state deterministically.

## Performance targets and measurement

These are **proposed release budgets, not measured claims**. Task T01 records reference CPU/OS/terminal dimensions and fixtures. Changes to budgets must be explicit, evidence-backed decisions, never silent relaxation.

| Metric | Proposed target | Measurement |
|---|---|---|
| Frame preparation/reducer/render work | p95 <= 16 ms, p99 <= 33 ms at 144x44 during 1x replay | Instrument process phases on a complete race; exclude unrelated network time |
| Input to visible response | p95 <= 50 ms in reference terminal | Timed interaction trace; TestBackend alone cannot prove emulator latency |
| Cached seek | p95 <= 100 ms for 100 fixed/random seeks | Monotonic timer, cold/warm distinction; compare resulting state to reference |
| Warm local session start | <= 2 seconds to first usable cached frame | Process launch through first display; report hardware/cache state |
| Resident memory | <= 256 MiB for reference 2-hour/22-car dataset | Measure peak resident memory with raw files stored on disk |
| Idle paused use | <= 1% of one logical CPU on reference machine | 60-second profile; state terminal/process measurement method |
| Long run | No unbounded memory/queue growth over 3 hours | Stress replay plus repeated seeks/session switches; inspect slope and queue bounds |

Download speed depends on the provider and connection; promise visible progress, backoff, resumability, and cancellation rather than an artificial fixed download duration. Frame rate is not telemetry sample rate. Use absolute measurements and profiles; no GPU/extra language without a demonstrated computational bottleneck.

## Milestones, order, and exit gates

| Milestone | Tasks | Demonstrable exit gate |
|---|---|---|
| M0: evidence and truth boundary | T01-T03 | Real source capability report, explicit demo mode, typed contract |
| M1: one authentic replay | T04-T09 | Select/download/cache one real race; timing and telemetry agree with time-filtered raw observations |
| M2: complete replay interaction | T10-T12 | Seeking, end behavior, classification, pits and flags correct across fixtures |
| M3: modern terminal experience | T13-T17 | Responsive layouts, readable table/map/detail, no blocking acquisition, tested terminal states |
| M4: intelligent and dependable | T18-T21 | Evidence-based insights; reference budgets and fault scenarios pass |
| M5: shippable v1 | T22-T24 | Clean-machine install, multi-platform builds, documentation and release checks complete |

The first end-to-end slice is one real race from source to table, not a full provider abstraction or a purely cosmetic rewrite. Checkpoints after every 2-3 tasks keep each slice reviewable. Each milestone ends with an executable demonstration and recorded evidence.

## Verification and release definition of done

Current commands remain valid: `cargo test --workspace --locked`, `cargo build --release --locked -p sectorline`, `cargo run --locked -p sectorline -- --demo`. Introduce focused tests as specified in todo.md. Formatting/linting gates: `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings`; establish/fix the baseline explicitly before enforcing.

Place integration tests and benchmarks under `f1_tui/tests/` and `f1_tui/benches/` so Cargo discovers them in the package, rather than at the virtual workspace root. Use small, attributed, legally appropriate provider-derived fixtures plus clearly labeled synthetic fault cases. Record session IDs, retrieval dates, hashes, units, and expected availability semantics. Avoid network in ordinary CI tests; add a separately triggered provider contract smoke test. UI snapshots use Ratatui TestBackend, supplemented by actual Windows Terminal/Linux/macOS terminal captures and keyboard/resize checks. Do not treat buffer snapshots as proof of visual fidelity or display latency.

Release gate: no misleading live route, zero fabricated measured values in recorded mode, no source-identity mismatch, deterministic seek, offline cache replay, no crashes on malformed/empty data, terminal restored after errors/Ctrl-C, bounded memory/queues, valid no-color/ASCII modes, tested release binaries, user-chosen code license, source attribution, accurate README, changelog, and known-data-limit documentation. Publish code without bulk cached data; data licensing is a separate release consideration.

## Risks and decisions still open

- OpenF1 may lack sufficient circuit geometry or an accurate availability timestamp for a desired field. T01 tests this early; show unavailable or use a separately validated optional import, never invent values.
- Native Rust downloads reduce installation friction; removing Python import support is not required and should not be mixed with data correctness changes.
- Terminal screenshots do not exist yet. UI dimensions are a concrete proposal, with visual validation required at M3 rather than a claim that the redesign is already verified.
- Performance budgets need a named reference machine and complete dataset at T01. They remain targets until measured.
- Code license requires owner choice before public release. Live provider credentials, commercial redistribution, and any future browser overlay are not v1 requirements.

## Later live milestone (outside v1 completion)

Add authenticated streaming into the same typed-event pipeline, with token refresh, bounded buffering, reconnect/backfill, arrival-time handling, delay controls, provider limits, and source-staleness behavior. Validate during actual sessions and recorded disconnect/reconnect scenarios. Never display a live badge merely because the latest session endpoint returns metadata. No live credentials or paid access are needed to complete v1.

## Primary references checked

- [OpenF1 API](https://openf1.org/docs/): free historical data, event fields, approximate location, brake/DRS interpretations. Do not promise information beyond its documented precision.
- [FastF1 data reference](https://github.com/theOehrly/Fast-F1/blob/master/docs/data_reference/index.rst): optional historical/import capabilities.
- [Ratatui 0.29](https://github.com/ratatui/ratatui/tree/v0.29.0): existing version; use TestBackend and correct init/restore handling. Upgrading the framework is not a prerequisite.
