# Data and interpretation

## Source and modes

Recorded mode reads OpenF1 historical observations for the exact selected session.
The catalogue, raw query URLs, retrieval times, coverage, checksums and normalized
field diagnostics are retained in the cache. A completed cache means the requested
queries were saved and validated; it does not prove the provider captured every
car, field or moment. Practice, qualifying and race coverage can differ.

The generated demo is a simulation, labelled DEMO. Historical metadata used by a
demo does not turn its movement, timing or conditions into observations. Recorded
loads never fall back to generated telemetry, gaps, flags or circuit geometry.

[OpenF1 documents](https://openf1.org/docs/) free unauthenticated historical access
from 2023 onward, approximate 3.7 Hz car/location samples, and endpoint units.
Sector Line has no paid live ingestion or F1 TV integration. A terminal redraw
rate of up to 30 Hz does not increase source sampling frequency.

## Time and availability

The replay cursor is observation time, beginning at the earliest retained core
observation. It can include pre-session coverage; zero does not mean lights-out.
Retrieval time records when a query was downloaded, not when viewers originally
received a report. The archive generally lacks original publication and revision
history. Recorded snapshots therefore use labelled event-time estimates and
cannot reproduce a historical live feed exactly.

A displayed value must be admitted at the cursor. Same-time conflicts suppress
ambiguous values; backward seeks rebuild state without retaining future samples.
Later corrections in a downloaded archive can still differ from what a live
viewer saw. The static reference lap used for the map is an explicit exception:
it uses whole-recording context, while car markers obey replay availability.

## Fields and quality

| Field | Meaning and limit |
| --- | --- |
| Position | Latest admitted provider-reported position; gaps or tyre data do not invent a ranking |
| Gap / interval | Provider timing in seconds or laps behind; missing and conflicting reports stay unavailable |
| Speed / RPM / gear | Reported km/h, engine revolutions per minute and gear; zero gear is neutral |
| Throttle / brake | Source throttle percentage and brake on/off; brake is not an analogue pressure measurement |
| DRS | Recognized source state or the unrecognized source code; no inferred activation |
| Lap / sectors | Completed-lap summaries admitted after a conservative completion estimate, not original sector delivery times |
| Best lap | `*` means provisional archive minimum with unverified eligibility; not an official final result |
| Tyre age | Initial reported age plus contiguous evidenced completed laps, not physical wear |
| Pit visit | An archived visit; its timestamp does not establish entry, service or exit phase |
| Flags / weather | Timestamped scoped reports; missing evidence does not mean green track or dry weather |

Lap completion uses the later of start plus duration/sector sum and the next lap
start where available. Conflicting/inconsistent summaries suppress affected
values. Recognized timed deletion/reinstatement messages update best eligibility;
unrecognized wording does not become an inferred steward decision. Historical
final-result status has no reliable publication boundary and is not injected into
earlier replay. Missing telemetry does not establish retirement.

Untimed stint summaries are anchored to evidenced lap starts as estimates. Pit
lane/stationary durations are held until a conservative completion estimate when
lane duration is known. These policies avoid presenting complete summaries as
instantaneous reports; they do not recover original delivery times.

`--` means unavailable. `~` means a held stale value, with its age measured against
the replay cursor. Pausing does not age observations. Freshness policy is two
seconds for telemetry/location, ten for intervals, 120 for weather/flag/session
reports and 180 for inferred stint evidence. These are application thresholds,
not provider guarantees. Sparse classification and completed history do not expire
like continuous samples. Invalid fields are quarantined rather than replaced with
zero; extra source fields do not automatically become supported display values.

## Track and context

The circuit background is a validated observed lap trace, labelled by driver and
lap. It is approximate source XY, not surveyed geometry, lane position, geographic
north or a start/finish detector. A reference needs a complete 30-300 second lap,
at least 50 contiguous finite location samples, plausible distances and closure,
and known non-pit context. Missing evidence leaves the map unavailable.

Car markers use a 500 ms display delay. Interpolation requires two already
available samples at most 750 ms apart, a short step and the same known non-pit
lap. Otherwise a usable held sample is marked stale; conflicting or more than
two-second-old positions are hidden. Map position never determines classification.

Context uses deterministic rules and states its evidence window and availability.
A reported position change is not automatically an on-track overtake. Gap trends
are descriptive; the app does not provide win probabilities, strategy predictions
or generated race commentary. Insights are withheld when evidence is inadequate.

## Attribution and rights

OpenF1 is an independent data provider. Sector Line is not affiliated with OpenF1,
Formula 1, the FIA, teams or drivers. Names and marks remain with their respective
owners. See [Formula 1's guidelines](https://www.formula1.com/en/information/guidelines.4EOKE9RRqevL4niTK9kWyt).

The [MIT license](LICENSE) covers Sector Line code and documentation. It does not
relicense provider data, third-party code, marks, video or audio, or grant broadcast
or commercial redistribution rights. API accessibility alone is not a data
redistribution license. Release binaries contain the generated demo; users acquire
recordings separately. Small provider-derived test excerpts retain source URLs,
retrieval dates and hashes in `f1_tui/tests/fixtures/manifest.json` and related
fixture records. Those attributions are provenance, not an assertion of ownership.

The optional developer Python bridge uses FastF1, NumPy and pandas under their own
licenses. They are not bundled with or required by the standalone executable.
Rust dependency notices are shipped in THIRD-PARTY-LICENSES.txt.
