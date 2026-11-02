# Observation contract (T03)

The validated contract lives in `f1_tui/src/domain/`. The existing TUI still
consumes the legacy display models; this task introduces the boundary for later
recorded ingestion and replay, without changing the screen or claiming real replay.

## Identity and values

Session identity combines a provider and its nonempty session key. Driver identity
includes that session and a nonzero driver number. Frames reject duplicate drivers,
drivers from other sessions, and observations whose session differs from the frame.
Session capabilities default to false and must be established from source evidence.
Unknown session kinds retain their original label.

Values carry their units through types: speed in km/h, time/gaps in seconds,
throttle in percent, RPM, and gear (0 neutral, 1–8 forward). Numeric constructors
reject nonfinite or negative values, and percent rejects values above 100.
Serialization uses numeric values; deserialization goes through the same validators.

Brake is released/pressed, decoded from source codes 0/100; values such as 72 or
104 are rejected, never interpreted as measured pressure. DRS retains its raw
unsigned code: 0/1 off, 8 eligible, 10/12/14 on, all other codes unknown.
Gaps distinguish leader, numeric seconds and a positive count of laps behind.
Formatting belongs in a later view model.

## Time, quality and provenance

`UtcMillis` is signed Unix UTC milliseconds; `ElapsedMillis` is a separate,
nonnegative playback duration. Observation time, availability and retrieval time
are separate fields. Retrieval never substitutes for original delivery time.
Availability is known, estimated or unknown, and cannot precede a known observation
time. `available_by` admits only present recorded values with known availability
at or before the requested UTC time. Estimated timing needs an explicit replay
policy in later tasks; unknown availability does not silently become event time.

Each observation carries its value, source channel, session, timing, quality and
optional explanation. Missing values require unavailable quality and a nonempty
reason; observed zero remains a present value. Observed/stale values require an
observation timestamp. Other qualities are derived, interpolated and generated.
Freshness policy and derivation evidence are responsibilities of later reducers.

Origin is retained separately from quality, so an unavailable generated value
still has generated provenance. Present generated values must have generated
quality. Recorded frames reject all generated origins, including unavailable ones.
Frame construction and JSON decoding both enforce this rule; changing a mode label
does not promote a demo frame into recorded data.

These checks enforce internal consistency, not source authentication. A caller
can invent a source declaration; acquisition/cache tasks must establish the actual
provenance from provider responses and manifests.

## Legacy demo compatibility

`adapt_legacy_demo` accepts only an explicit legacy-demo session. It preserves
elapsed timestamps and valid core telemetry as generated observations, without
inventing UTC dates or availability. Invalid fields become unavailable with a
reason rather than being clamped. A legacy DRS boolean cannot recover the source
code and therefore becomes unavailable. The old UI/cache remains unchanged.

The current contract covers core car telemetry, position, gaps and sectors.
Weather, flags, geometry, stints and classification eligibility follow in their
planned tasks. No provider loader, new runtime dependency or UI migration is added.

## Verification checkpoint

- Twelve offline contract tests cover malformed JSON, numeric limits, zero versus
  missing, unknown codes, timing boundaries, session isolation and generated origin.
- Every frame in the bundled demo cache adapts and remains excluded from recorded mode.
- All 20 Rust tests and three Python mode tests pass; release build and the existing
  20-frame headless demo pass.
- New domain/test files pass rustfmt. Strict workspace Clippy remains blocked by
  pre-existing collapsible-if warnings in legacy files; no lint suppression is added.
- No performance improvement is claimed: this boundary is not yet on the render path.

M0's exit gate is met: source capabilities are documented (T01), demo mode is
explicit (T02), and the validated observation contract is available (T03).
T04 begins the completed-session catalogue; authentic replay remains the M1 goal.
