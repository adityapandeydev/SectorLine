# Sector Line implementation tasks

Status: T01-T02 completed; T03-T24 remain planned. Scope confirmed: free historical replay first, live later. Follow [plan.md](plan.md) and [../DESIGN.md](../DESIGN.md). Each task should be one reviewable change; do not mix visual redesign with data rewrites. Proposed new file paths may be adjusted without changing boundaries or acceptance criteria.

## T01: Establish real-data and performance baselines

- [x] Complete T01.

**Dependencies:** None. **Scope:** S (at most five likely files).

**Likely files:** docs/data-capabilities.md; f1_tui/tests/fixtures/manifest.json; docs/performance-baseline.md.

**Acceptance criteria:**

- [x] Inspect one ordinary race, one incident-heavy race, one qualifying session and one practice session; record exact IDs, fields, cadence and timestamp limitations.
- [x] Choose the reference machine/terminals and datasets for the proposed budgets; measure current loading, frame cost and memory.
- [x] Record geometry availability and reject any field whose historical availability cannot be reconstructed honestly.

**Verification:** Provider contract inspection plus a recorded baseline report; no fabricated benchmark numbers.

**Evidence:** [Data capabilities](../docs/data-capabilities.md), [performance baseline](../docs/performance-baseline.md), and [inspection manifest](../f1_tui/tests/fixtures/manifest.json). Full-race release budgets remain unverified; see the baseline report.

## T02: Make current modes honest

- [x] Complete T02.

**Dependencies:** T01. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/main.rs; f1_tui/src/app.rs; f1_tui/src/stream.rs; README.md.

**Acceptance criteria:**

- [x] Generated cached playback is explicitly Demo in entrypoints, header and help.
- [x] Remove or disable the misleading live route until actual ingestion exists; no metadata-only live badge.
- [x] Existing demo and session selection still work; no broad visual redesign in this slice.

**Verification:** cargo test -p sectorline mode; cargo run --locked -p sectorline -- --demo; inspect help/header.

**Evidence:** Six offline Rust mode regressions, three Python bridge regressions, and the generated headless demo pass. Removed the misleading native generator; disabled live API/CLI/bridge entrypoints before network/cache activity. UI changes are labels only.

## T03: Introduce the typed observation contract

- [ ] Complete T03.

**Dependencies:** T01,T02. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/models.rs; f1_tui/src/domain/mod.rs; f1_tui/src/lib.rs; f1_tui/tests/observation_contract.rs.

**Acceptance criteria:**

- [ ] Model session/driver IDs, units, observation and availability times, quality, optional values and session capabilities.
- [ ] Represent brake on/off and unknown DRS codes correctly; preserve numeric gaps and lapped states rather than display strings.
- [ ] A legacy-demo adapter preserves compatibility without allowing generated fields into recorded mode.

**Verification:** cargo test -p sectorline --locked --test observation_contract; malformed, unknown-code and missing-versus-zero cases.

### Checkpoint after T03

- [ ] Focused tests and build pass; exercise the current end-to-end slice.
- [ ] Record correctness/performance/visual evidence appropriate to this slice.
- [ ] Review the milestone against its exit gate before expanding scope.

## T04: Build the completed-session catalogue

- [ ] Complete T04.

**Dependencies:** T03. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/data/catalogue.rs; f1_tui/src/data/mod.rs; f1_tui/src/app.rs; f1_tui/tests/catalogue.rs.

**Acceptance criteria:**

- [ ] Discover available years, meetings and session types using provider IDs.
- [ ] Search and selection resolve exactly; no hardcoded current grid or silent fallback year.
- [ ] Cached catalogue permits offline selection and unavailable sessions carry a clear reason.

**Verification:** cargo test -p sectorline --locked --test catalogue; fixture-based year/type/alias and offline selection checks.

## T05: Acquire one real session incrementally

- [ ] Complete T05.

**Dependencies:** T04. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/data/openf1.rs; f1_tui/src/data/limits.rs; f1_tui/src/lib.rs; f1_tui/tests/acquisition.rs.

**Acceptance criteria:**

- [ ] Fetch timestamped drivers, positions, car data and locations in bounded chunks; fetch other supported channels without requiring them to exist.
- [ ] Honor configured provider rate limits, Retry-After, bounded retries, timeouts and cancellation between chunks.
- [ ] Validate schema, session identity and numeric domains before accepting a chunk.

**Verification:** cargo test -p sectorline --locked --test acquisition; local stub HTTP server for limits, failures and interrupted chunks.

## T06: Publish a validated offline session cache

- [ ] Complete T06.

**Dependencies:** T05. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/data/cache.rs; f1_tui/src/data/paths.rs; f1_tui/tests/cache.rs; .gitignore.

**Acceptance criteria:**

- [ ] Versioned manifest includes exact identity, coverage, source/retrieval metadata and checksums; publish atomically.
- [ ] Interrupted downloads resume without duplicate events; corrupt and incompatible caches are reported or rebuilt explicitly.
- [ ] Use user cache directories rather than source-relative locations; bounded raw chunks never become bulk Git additions.

**Verification:** cargo test -p sectorline --locked --test cache; interrupted-write, wrong-year, wrong-type, checksum and read-only-directory tests.

### Checkpoint after T06

- [ ] Focused tests and build pass; exercise the current end-to-end slice.
- [ ] Record correctness/performance/visual evidence appropriate to this slice.
- [ ] Review the milestone against its exit gate before expanding scope.

## T07: Keep session loading off the UI thread

- [ ] Complete T07.

**Dependencies:** T04,T05,T06. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/app.rs; f1_tui/src/main.rs; f1_tui/src/loader.rs; f1_tui/tests/loader.rs.

**Acceptance criteria:**

- [ ] Background worker sends bounded progress/results and observes cancellation/timeouts.
- [ ] Each request has an ID; stale results cannot replace a newer selection; preserve the current usable session on error.
- [ ] Input, resize and quit remain responsive during slow acquisition; errors are visible and actionable.

**Verification:** cargo test -p sectorline --locked --test loader; manual slow-server cancel/switch/quit trace.

## T08: Normalize observations into deterministic session state

- [ ] Complete T08.

**Dependencies:** T03,T06. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/domain/normalize.rs; f1_tui/src/replay/timeline.rs; f1_tui/src/replay/mod.rs; f1_tui/tests/reducer.rs.

**Acceptance criteria:**

- [ ] Convert UTC samples into one elapsed-session timeline with explicit same-time ordering, deduplication and correction policy.
- [ ] State at T uses only fields available by T, with provider/event/retrieval times kept distinct.
- [ ] One complete race drives a minimal actual-data timing view; unknown fields remain unknown.

**Verification:** cargo test -p sectorline --locked --test reducer; compare selected timestamps to raw fixture observations, including future-leakage cases.

## T09: Drive replay using timestamps

- [ ] Complete T09.

**Dependencies:** T07,T08. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/replay/clock.rs; f1_tui/src/stream.rs; f1_tui/src/app.rs; f1_tui/tests/replay_clock.rs.

**Acceptance criteria:**

- [ ] Use monotonic elapsed time with pause/resume, speed changes and irregular source cadence.
- [ ] Playback ends explicitly instead of silently wrapping; restart is deliberate.
- [ ] UI updates and observation sampling are separate; no disk/network work occurs in a render callback.

**Verification:** cargo test -p sectorline --locked --test replay_clock; fake-clock speed/pause/end tests and full real-session playback.

### Checkpoint after T09

- [ ] Focused tests and build pass; exercise the current end-to-end slice.
- [ ] Record correctness/performance/visual evidence appropriate to this slice.
- [ ] Review the milestone against its exit gate before expanding scope.

## T10: Implement seek and recording synchronization

- [ ] Complete T10.

**Dependencies:** T09. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/replay/checkpoint.rs; f1_tui/src/replay/timeline.rs; f1_tui/src/app.rs; f1_tui/tests/seek.rs.

**Acceptance criteria:**

- [ ] Checkpointed backward/forward seek restores the same state as replay from the beginning.
- [ ] Go-to time/lap and signed recording offset use clear session-time semantics.
- [ ] Selection, event history, personal bests and quality state rebuild correctly after seeks.

**Verification:** cargo test -p sectorline --locked --test seek; 100 deterministic/random target comparisons plus a manual recording-sync workflow.

## T11: Correct classification and lap timing semantics

- [ ] Complete T11.

**Dependencies:** T08,T10. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/domain/classification.rs; f1_tui/src/domain/timing.rs; f1_tui/src/models.rs; f1_tui/tests/classification.rs.

**Acceptance criteria:**

- [ ] Source classification drives order; lapped cars, line crossings and DNF/DNS/DSQ do not corrupt it.
- [ ] Lap/sector values and personal/session bests respect completion, deletion and eligibility.
- [ ] No retrospective final result, retirement or sector appears before its availability boundary.

**Verification:** cargo test -p sectorline --locked --test classification; first lap, lapping, deleted lap, missing interval and final-classification leakage fixtures.

## T12: Add time-correct tyres, pit, weather and flags

- [ ] Complete T12.

**Dependencies:** T11. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/domain/conditions.rs; f1_tui/src/domain/stints.rs; f1_tui/src/domain/quality.rs; f1_tui/tests/conditions.rs.

**Acceptance criteria:**

- [ ] Stint/tyre age and pit states derive only from valid evidence, with missing states preserved.
- [ ] Local flags, whole-track flags, safety car, red flag and restart retain scope and chronology.
- [ ] Per-channel stale rules use replay time; missing/stale information never becomes green or zero.

**Verification:** cargo test -p sectorline --locked --test conditions; long gap, restart, pit and late-weather fixtures.

### Checkpoint after T12

- [ ] Focused tests and build pass; exercise the current end-to-end slice.
- [ ] Record correctness/performance/visual evidence appropriate to this slice.
- [ ] Review the milestone against its exit gate before expanding scope.

## T13: Implement the restrained UI shell

- [ ] Complete T13.

**Dependencies:** T09,T12. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/ui.rs; f1_tui/src/ui/theme.rs; f1_tui/src/ui/layout.rs; f1_tui/tests/ui_shell.rs.

**Acceptance criteria:**

- [ ] Implement DESIGN.md header/tabs/timeline/status/help hierarchy at wide and standard sizes.
- [ ] Semantic palette, stable labels and whitespace replace oversized banners and repeated frames.
- [ ] Mode, exact session identity, replay clock and data availability remain visible.

**Verification:** cargo test -p sectorline --locked --test ui_shell; TestBackend captures plus actual-terminal visual review.

## T14: Build the timing table

- [ ] Complete T14.

**Dependencies:** T11,T13. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/ui/timing.rs; f1_tui/src/ui/view_model.rs; f1_tui/src/app.rs; f1_tui/tests/ui_timing.rs.

**Acceptance criteria:**

- [ ] Stable numeric columns, units, lapped states and unavailable markers follow the data contract.
- [ ] Driver identity anchors selection through classification changes; grids larger than the viewport scroll.
- [ ] Selection and team identity remain readable in no-color and 16-color modes.

**Verification:** cargo test -p sectorline --locked --test ui_timing; 26-driver, missing-value and rapid-reorder captures.

## T15: Build a faithful track view

- [ ] Complete T15.

**Dependencies:** T08,T13. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/track.rs; f1_tui/src/domain/geometry.rs; f1_tui/src/ui/track.rs; f1_tui/tests/geometry.rs.

**Acceptance criteria:**

- [ ] Track and car positions use one validated session transform with aspect-ratio preservation and terminal-cell aspect correction.
- [ ] Interpolate only short contiguous sample intervals; do not cross gaps/pits/jumps or infer race order from coordinates.
- [ ] Prioritize selected-driver labels; unavailable geometry does not become a substitute circuit.

**Verification:** cargo test -p sectorline --locked --test geometry; rotation/scaling/line-crossing/gap fixtures and real terminal map captures.

### Checkpoint after T15

- [ ] Focused tests and build pass; exercise the current end-to-end slice.
- [ ] Record correctness/performance/visual evidence appropriate to this slice.
- [ ] Review the milestone against its exit gate before expanding scope.

## T16: Build driver detail and the event timeline

- [ ] Complete T16.

**Dependencies:** T10,T12,T13. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/ui/driver.rs; f1_tui/src/ui/events.rs; f1_tui/src/ui/timeline.rs; f1_tui/tests/ui_detail.rs.

**Acceptance criteria:**

- [ ] Driver panel shows typed telemetry, age and eligible lap/sector comparisons with meaningful units.
- [ ] Events display timestamp, scope and evidence and can seek to a replay moment.
- [ ] Display on/off brake, unknown gear/DRS and missing traces accurately; avoid decorative gauges for absent data.

**Verification:** cargo test -p sectorline --locked --test ui_detail; fixture-driven driver/event navigation and screenshot review.

## T17: Finish responsive behavior and terminal lifecycle

- [ ] Complete T17.

**Dependencies:** T13,T14,T15,T16. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/ui/layout.rs; f1_tui/src/main.rs; f1_tui/src/terminal.rs; f1_tui/tests/terminal_layout.rs.

**Acceptance criteria:**

- [ ] Wide/standard/compact layouts and below-minimum fallback work through repeated resizes without clipping or panics.
- [ ] No-color/ASCII alternatives and keyboard help work without emoji, Nerd Fonts or special image support.
- [ ] Normal exit, errors, panic and Ctrl-C restore raw mode, cursor and alternate screen; quit is prompt during loading.

**Verification:** cargo test -p sectorline --locked --test terminal_layout; actual Windows/Linux/macOS resize and cleanup smoke checklist.

## T18: Make qualifying and practice behavior explicit

- [ ] Complete T18.

**Dependencies:** T04,T11,T12,T17. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/domain/capabilities.rs; f1_tui/src/domain/classification.rs; f1_tui/src/ui/view_model.rs; f1_tui/tests/session_types.rs.

**Acceptance criteria:**

- [ ] Qualifying phases/eligible best laps and practice timing use provider-supported semantics.
- [ ] Hide inapplicable race gaps/lap totals; distinguish scheduled session duration from observed elapsed time.
- [ ] Missing phase or telemetry support produces a clear partial-capability state, not made-up defaults.

**Verification:** cargo test -p sectorline --locked --test session_types; one qualifying and one practice replay checked at fixed times.

### Checkpoint after T18

- [ ] Focused tests and build pass; exercise the current end-to-end slice.
- [ ] Record correctness/performance/visual evidence appropriate to this slice.
- [ ] Review the milestone against its exit gate before expanding scope.

## T19: Add explainable contextual insights

- [ ] Complete T19.

**Dependencies:** T10,T11,T12,T16. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/insights.rs; f1_tui/src/ui/events.rs; f1_tui/src/ui/driver.rs; f1_tui/tests/insights.rs.

**Acceptance criteria:**

- [ ] Gap trend, nearby reported interval, pit/tyre event and eligible-best insights include evidence window and timestamp.
- [ ] Suppress insufficient/stale evidence; label ambiguous classification changes as position changes, not overtakes.
- [ ] Deduplicate repeated notices and deterministically rebuild insights after seeking; no future knowledge or predictive claims.

**Verification:** cargo test -p sectorline --locked --test insights; sparse/corrected data, pit-vs-overtake and reverse-seek cases.

## T20: Meet the measured replay budgets

- [ ] Complete T20.

**Dependencies:** T09,T10,T17,T19. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/replay/timeline.rs; f1_tui/src/main.rs; f1_tui/benches/replay.rs; docs/performance-baseline.md.

**Acceptance criteria:**

- [ ] Measure frame phases, seek/start time, peak resident memory and paused CPU on the named reference setup.
- [ ] Use indexed lookups, bounded batches/coalescing and cached geometry only where profiles justify changes.
- [ ] Meet plan.md budgets or report a concrete blocking measurement; never weaken targets silently.

**Verification:** Optimized replay/seek benchmarks, real terminal input trace and a 3-hour memory/queue profile.

## T21: Harden failure and corruption handling

- [ ] Complete T21.

**Dependencies:** T06,T07,T18,T20. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/tests/provider_faults.rs; f1_tui/tests/replay_invariants.rs; f1_tui/tests/cache_faults.rs; docs/data-limitations.md.

**Acceptance criteria:**

- [ ] Offline, rate-limit, interrupted response, schema drift, nonfinite data and corrupt cache failures preserve usable state and visible reasons.
- [ ] Random seek/event-order tests verify invariants across all release fixtures; no network access in ordinary CI.
- [ ] Audit parsers, paths, channel bounds and logged diagnostics; no credentials or silently swallowed failures.

**Verification:** cargo test --workspace --locked; run deterministic fault suite and inspect diagnostics.

### Checkpoint after T21

- [ ] Focused tests and build pass; exercise the current end-to-end slice.
- [ ] Record correctness/performance/visual evidence appropriate to this slice.
- [ ] Review the milestone against its exit gate before expanding scope.

## T22: Package the standalone application

- [ ] Complete T22.

**Dependencies:** T17,T18,T21. **Scope:** M (at most five likely files).

**Likely files:** f1_tui/src/cli.rs; f1_tui/src/main.rs; f1_tui/src/data/paths.rs; README.md.

**Acceptance criteria:**

- [ ] Binary starts outside the repository with no source-relative paths or required Python installation.
- [ ] Clear catalogue/download/replay/demo commands, config defaults, cache location and offline behavior are documented.
- [ ] A clean-machine user can run a generated demo and download/replay a real session without a paid account.

**Verification:** Release build; unpack into a fresh directory, run with empty config/cache, then repeat offline.

## T23: Add multi-platform CI and release automation

- [ ] Complete T23.

**Dependencies:** T21,T22. **Scope:** M (at most five likely files).

**Likely files:** .github/workflows/ci.yml; .github/workflows/release.yml; docs/release-checklist.md.

**Acceptance criteria:**

- [ ] CI runs formatting, lint, unit/integration tests and release builds on supported Windows/Linux/macOS targets.
- [ ] Tagged releases produce named archives and checksums; source-data downloads are separate from CI fixtures.
- [ ] No unsupported platform promise; an untested target is withheld or clearly marked experimental.

**Verification:** Validate workflows and one non-public/draft release build; inspect archives and clean-machine smoke results before publication.

## T24: Complete documentation and release review

- [ ] Complete T24.

**Dependencies:** T22,T23. **Scope:** M (at most five likely files).

**Likely files:** README.md; CHANGELOG.md; LICENSE; docs/data-limitations.md; docs/release-checklist.md.

**Acceptance criteria:**

- [ ] Owner chooses a code license; document provider attribution, data precision/coverage, third-party rights and recorded/demo distinction.
- [ ] Document keys, terminal minimums, install/update, troubleshooting, cache cleanup and complete first-run instructions.
- [ ] All milestone gates pass and known issues are explicit; no paid live claims or fabricated telemetry remain in recorded mode.

**Verification:** Run the full release checklist with a fresh user profile and verify every README command; owner reviews release candidate.

### Checkpoint after T24

- [ ] Focused tests and build pass; exercise the current end-to-end slice.
- [ ] Record correctness/performance/visual evidence appropriate to this slice.
- [ ] Review the milestone against its exit gate before expanding scope.

## Completion record

- [ ] All 24 tasks meet their acceptance and verification criteria.
- [ ] V1 release gate in plan.md passes on the declared supported platforms.
- [ ] Paid live access remains explicitly future work; it is not required for v1 completion.
