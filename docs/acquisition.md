# Incremental historical acquisition (T05)

`OpenF1Client` delivers one validated chunk at a time to a caller-supplied sink.
It never accumulates a full race in memory, invokes the generated demo bridge,
or writes a published replay cache. The developer harness is
`f1_tui/examples/acquire.rs`; normal TUI playback is unchanged.

## Exact identity and coverage

Requests use numeric session/meeting IDs from the completed-session catalogue.
The catalogue constructor rejects cancelled/incomplete selections and waits until
30 minutes after the scheduled end for free historical access. Every provider row
must match the requested session and meeting. A validated, nonempty, unique driver
roster is required first; per-driver records must belong to that roster.

Acquisition covers ten channels: drivers, position, car data, location, intervals,
laps, stints, pit, race control and weather. Car data, location and intervals use
adjacent five-minute half-open UTC windows `[start, end)`. Queries use documented
comparison syntax and UTC timestamps without offsets; returned timestamps must
actually fall inside the requested window. This avoids gaps or duplicated boundary
times caused by using two strict bounds. No resampling or interpolation occurs here.

Sparse channels use one response each, subject to the same byte/row limits. Their
records can precede the scheduled start. Drivers and stints lack event timestamps;
none are invented. Lap start dates can be null, and completed lap/stint summaries
remain retrospective. Their publication/availability policy belongs to later replay
tasks, not retrieval time.

The default high-frequency coverage is the scheduled session interval plus one hour
on each side; callers may supply an explicit UTC window. These are requested coverage
bounds, not proof of all data in the archive or the actual racing start/end. Records
outside that window are not claimed as acquired. The source URLs and exact windows
travel with each chunk. The overall request window is limited to 24 hours.

## Failures and resource bounds

- One client issues sequential attempts with at least 2.1 seconds between starts,
  including retries. This stays below the published free 3/second and 30/minute
  limits. It is a per-client limiter; callers must reuse a client or coordinate
  other processes using the same provider quota.
- Defaults: 10-second connect timeout, 30-second request/read/write timeouts,
  8 MiB decompressed response limit, 100,000 rows per chunk, three retries after
  the initial attempt. No queue or full-session response list is retained.
- Transport failures, interrupted body reads, HTTP 429 and 500/502/503/504 retry
  whole chunks with bounded exponential backoff. Invalid JSON/schema/identity,
  authorization failures and byte/row limits fail explicitly, without accepting
  any rows from that chunk or silently switching sessions.
- `Retry-After` supports delta seconds and HTTP dates. A delay above the configured
  automatic wait budget (60 seconds by default) stops with an explanation; it is
  never shortened to make an early retry. There is no automatic fallback provider.
- Cancellation is checked before attempts, during pacing/backoff, between body
  reads, before validation/publication and between chunks. An active blocking I/O
  call can take until its timeout to return; this is cooperative cancellation,
  not instantaneous socket interruption. DNS resolution has the blocking-client
  limitations of ureq 2.x. Background/UI integration remains T07.
- Empty arrays and HTTP 404 produce explicit query-level empty outcomes. They
  do not mark a whole session or endpoint unsupported. Missing optional channels
  do not prevent other channels from being acquired. A missing roster fails.
- A sink failure stops acquisition. Previously delivered chunks remain partial
  output, never a completed recording. Resume/deduplication/checksums/atomic session
  publication are T06, not promises of this harness.

Production acquisition permits only the OpenF1 HTTPS endpoint, with redirects
disabled. An explicit IPv4 loopback HTTP endpoint exists for local protocol tests.
No credentials, live subscriptions or runtime dependencies are added.

## Validation and provenance

Each accepted chunk carries raw response bytes, source URL, HTTP status, attempt
count, retrieval UTC milliseconds, identity, requested window, validated records
and field issues. Raw bytes are evidence and must not be rendered as measured
values without validation. The validated records have invalid numeric fields set
to null with a row/field/reason diagnostic; valid zero and source null stay distinct.

Schema/type/identity errors reject the entire chunk, even if earlier rows were valid.
Supported numeric fields validate finite values, unsigned identities, positive
classification/lap numbers, percentage ranges, neutral/eight gears, discrete 0/100
brake, durations, signed coordinates and weather domains. Unknown unsigned DRS
codes, text flags/compounds and source extensions remain available for later decoding.
Gap values preserve numeric seconds, lapped strings and null; unknown strings are
unavailable. Unsafe control-bearing display labels are quarantined as fields.
Mini-sector arrays and additional extension fields are raw source metadata, not
validated timing observations; later domain reducers must decode their own contracts.

The harness writes raw, validated and metadata files into a new explicit scratch
directory. It refuses to reuse an existing directory. A hard process interruption
can leave partial scratch files; nothing treats that directory as a usable cache.
Bulk responses remain outside Git and outside the project tree in the verification run.

## Verification

Fourteen local HTTP-stub integration tests exercise pacing, transient failures,
both forms of Retry-After, retry exhaustion, cancellation during backoff/between
chunks, timeouts, partial/truncated bodies, byte/row limits, malformed/schema-invalid
data, identity/window mismatch, missing channels, field quarantine and sink failure.
Normal tests never contact OpenF1. Desktop sandbox runs require loopback network
permission; the tests fail rather than being skipped when it is unavailable.

The real-source verification uses Monza 2024 race session 9590/meeting 1244.
A 13:10–13:11 UTC high-frequency check plus sparse channels returned 11,350 rows;
one brake field was quarantined. The full 12:00–16:00 UTC requested high-frequency
interval, plus session-scoped sparse channels, returned **1,219,329 records in 151
chunks**, including 70 empty queries. Each request succeeded on its first attempt.
The 178,452,858 raw response bytes remain in the user cache outside Git; the largest
chunk was 3,826,512 bytes, below the 8 MiB cap. The run quarantined 31,952 throttle
and 32,009 brake fields; source values such as 104 remain raw evidence, not plausible
measured percentages. No other fields were quarantined in this run.

Per-channel counts, retrieval timestamps, exact query URLs and raw-response SHA-256
hashes are in the small [acquisition evidence manifest](../f1_tui/tests/fixtures/acquisition-monza.json).
That manifest records acquisition, not temporal replay correctness. All 42 Rust
tests and three Python tests pass, along with release builds and the generated demo.
New files pass rustfmt; strict workspace Clippy retains the recorded legacy warnings.
No frame-rate/memory improvement or original live arrival timing is claimed.

References: [OpenF1 schema/filtering](https://openf1.org/docs/),
[free limits and historical access window](https://openf1.org/),
[ureq 2.12.1](https://docs.rs/ureq/2.12.1/ureq/struct.AgentBuilder.html).
