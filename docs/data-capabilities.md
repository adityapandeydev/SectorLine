# Historical data capability baseline (T01)

Inspected 1 November 2026, using unauthenticated historical OpenF1 queries. Application source baseline: `65cb371`. Exact query URLs, successful response-byte SHA-256 values, UTC retrieval times, record counts, fields, null counts and small examples are in [the evidence manifest](../f1_tui/tests/fixtures/manifest.json). This is a provider inspection, not implementation of replay or a claim of complete dataset fidelity.

## Representative sessions

Monza is the relatively ordinary race comparison; Sao Paulo supplies explicit VSC, safety-car, red-flag and restart context. Practice is not assumed incident-free (this sample also includes a red flag).

| Scenario | Session ID | Meeting ID | Provider session start (UTC) | Driver 4 lap records | All race-control messages |
|---|---|---|---|---|---|
| ordinary_race | 9590 | 1244 | 2024-09-01T13:00:00+00:00 | 53 | 61 |
| incident_race | 9636 | 1249 | 2024-11-03T15:30:00+00:00 | 69 | 111 |
| qualifying | 9586 | 1244 | 2024-08-31T14:00:00+00:00 | 12 | 36 |
| practice | 9583 | 1244 | 2024-08-30T11:30:00+00:00 | 26 | 23 |

All four session driver queries returned 20 drivers. Main telemetry/location/interval inspection windows are the 60 seconds starting 10 minutes after the advertised session start; query endpoints are strict `date>` / `date<`. Other channels have the scopes documented in the manifest. These windows are probes, not complete races. Driver 4 was selected consistently, with later qualifying checks for drivers 4 and 16.

## Observed cadence, not promised fixed frequency

Counts cover the bounded 60-second driver windows for car/location and full-session weather. Median gaps are computed over positive sorted timestamp differences; irregular cadence is retained rather than resampled. Cross-channel timing and arrival latency were not measured.

| Scenario | Car rows / median gap | Location rows / median gap | Weather rows / median gap |
|---|---|---|---|
| ordinary_race | 230 / 0.240s | 234 / 0.240s | 133 / 60.004s |
| incident_race | 213 / 0.280s | 229 / 0.240s | 201 / 60.002s |
| qualifying | 222 / 0.279s | 224 / 0.260s | 79 / 60.002s |
| practice | 233 / 0.240s | 227 / 0.260s | 81 / 60.004s |

Monza race intervals returned 15 records in the minute, with a 3.864-second median positive gap. Position records are event-like: the full driver query returned only 25 entries, with a 40.328-second median gap. Absence of a new classification event is not evidence that the current classification is stale. This is different from a periodic telemetry channel.

Sao Paulo intervals returned HTTP 404 for the 15:40-15:41 UTC window. A full-session query for the same driver succeeded with 1,354 records, median gap 3.931 seconds, and maximum gap 4,321.520 seconds. Its first record predates the advertised session start. Do not infer actual racing start from `sessions.date_start`, fabricate intervals in a gap, or mark the entire endpoint unsupported after one empty window.

The sampled qualifying/practice interval queries, including full-session driver queries, returned HTTP 404. The product must support unavailable race intervals for these sessions. Do not extrapolate this observation into a claim about every provider/version.

## Field capability and replay policy

| Channel | Observed shape/evidence | Consequence for implementation |
|---|---|---|
| Sessions/drivers | Stable provider IDs, scheduled dates, year/type, circuit key, driver roster | Key cache by exact identity, not a display-name alias; catalogue dates are not necessarily actual racing start |
| Car data | `date`, speed, RPM, gear, throttle, brake, DRS | Time-index individually; zero differs from missing; validate values before display |
| Location | Timestamped x/y/z, no lateral precision or measured lap fraction | Approximate map context; never derive race classification from wrapped coordinate progress |
| Position | Timestamp plus classification changes | Carry forward last eligible change within that session; do not apply a telemetry-style fixed expiry blindly |
| Intervals | Timestamp, interval and gap-to-leader, nullable; interrupted periods possible | Preserve units/types and missing gaps; do not reconstruct exact gaps from geometry |
| Laps | Approximate `date_start`, sector durations, lap duration, pit-out state, speed traps; no per-sector publication/arrival timestamp | Completed-lap records are retrospective; never show all sectors at lap start; exact original broadcast availability cannot be proven |
| Stints | `lap_start`, `lap_end`, compound and starting tyre age, without event timestamps | Do not reveal future stint end; join cautiously to completed laps and pit evidence; mark derived timing |
| Pit | Timestamp and lane/pit durations; `stop_duration` is null in an inspected Monza record | Lane transit is not stationary service time; no invented stop duration |
| Race control | Timestamp, nullable flag/scope/sector/driver, message, lap and qualifying phase | Preserve local-vs-track scope; category SafetyCar may have no flag value; derive state from documented semantics |
| Weather | Timestamped measurements approximately every minute | Display its own observation age; do not apply a two-second car-data freshness rule |

The incident fixture includes VSC deployed at 16:28:21 UTC, safety car deployed at 16:32:53, whole-track red flag at 16:37:22, and subsequent safety-car events. Its messages contain Driver, Sector and Track scopes. Qualifying messages include phases 1, 2, 3 and null. A phase-aware UI is possible, but event ordering/availability still needs replay tests.

### Anomaly discovered: brake and throttle outside the documented range

The qualifying driver-4 sample at 14:10-14:11 UTC returned 222 records with brake=104 and throttle=104, while speed/RPM/gear were zero. A 14:15-14:16 follow-up returned the same values in 224 records. A 14:55-14:56 check returned normal moving-car ranges for both drivers 4 and 16 (219 records each; throttle/brake 0-100).

Therefore the anomalous early values cannot be rendered as valid percentages, silently clamped to 100, or used as a universal description of qualifying data. Retain raw evidence and display those fields as unavailable with a diagnostic. The cause of 104 was not established. DRS codes 1, 8, 9 and 14 also occur in the inspected windows; preserve raw codes and unknown meanings instead of forcing every nonzero value into true.

### Historical knowledge boundary

None of the inspected schemas contains a general original delivery/arrival timestamp or revision history. Data downloaded today may include retrospective corrections. We can build event-time replay; we cannot claim an exact reconstruction of what a live viewer knew at each millisecond.

Reject or defer these displays until an honest availability rule exists: exact mid-lap sector publication, future stint end, retrospective race result at an earlier replay moment, exact brake pressure, unobserved pit service duration, millisecond-accurate gaps inferred from map coordinates, and true live classification. Unknown never becomes green, zero, or a plausible placeholder.

## Circuit geometry feasibility

Both meeting responses include `circuit_info_url` linking the separate MultiViewer service:

- Monza, circuit 39/year 2024: 752 x/y points, 11 corners, rotation 95; HTTP 200.
- Interlagos, circuit 14/year 2024: 283 x/y points, 15 corners, rotation 258; HTTP 200.

URLs and raw-response hashes are recorded in the manifest. This demonstrates available geometry, not proven calibration against the source location samples. T15 must validate year/circuit identity, coordinate basis, rotation and terminal-cell aspect ratio together. The linked service is an additional dependency with its own availability and usage terms; do not bundle its outline automatically. No separate native OpenF1 coordinate-outline endpoint was established by this inspection. If geometry cannot be validated, show the map as unavailable while timing remains usable.

## Reference datasets and acquisition limits

Use session 9590 for the first authentic full-race path; 9636 for incident/time-gap recovery; 9586 for qualifying phases, missing sectors and invalid early telemetry; 9583 for practice capability and pit-out/missing-lap behavior. The inspection manifest deliberately does not claim full multi-driver replay coverage. Full datasets and correctness checkpoints are acquired in T05-T12, with the same IDs and explicit coverage metadata.

The current generated 22-driver/600-frame cache is only the performance/demo reference. It is not a substitute for these observations or the planned two-hour capacity fixture. Full-race memory, long-run stability, seeking and provider-correction behavior remain unmeasured at T01.

Requests were sequential with at least 2.2 seconds between regular OpenF1 calls and bounded backoff on 5xx/429. Initial encoded inclusive/offset timestamp queries returned HTTP 500; strict UTC filters without offsets succeeded. Both syntax and timestamp formatting changed, so this does not establish which caused the failures. Keep endpoint failures observable; test the exact query format before treating all HTTP errors as missing data.

Raw responses and acquisition scripts remain outside the repository. The committed manifest contains small excerpts and descriptive statistics, not bulk session downloads. Successful hashes refer to HTTP response bytes, so future provider revisions can legitimately produce a different hash. Regular CI must not call these remote endpoints.

## References

- [OpenF1 API reference](https://openf1.org/docs/): event schemas, approximate location, lap-time caveats, brake and DRS semantics.
- [OpenF1 access limits](https://openf1.org/): free historical access and published request limits, checked on the inspection date.
- Exact OpenF1 and linked MultiViewer requests, timestamps and hashes: the evidence manifest.
