# Completed-session catalogue (T04)

The catalogue reads OpenF1 meeting/session metadata and exposes it through
`--catalogue`. It is independent of generated-demo loading. No driver grid is
assumed, no latest-session alias is used, and no default year is selected here.
The existing TUI demo selector remains a legacy demo interface; catalogue-backed
TUI loading follows the acquisition/cache/background-loader tasks T05–T07.

## Discovery and selection

- `--catalogue --refresh` discovers meetings and years from the provider's
  `meetings` response. It does not claim that all discovered years' sessions have
  been downloaded or completed.
- Add `--year YYYY` to refresh the exact year's `sessions`. Other cached years
  survive a successful refresh. Refresh never runs from a render callback.
- Without `--refresh`, all operations are local. A year whose sessions have not
  been fetched produces an explicit error, distinct from a fetched empty year.
- `--search TEXT` matches normalized whole tokens across meeting name, circuit,
  location, country, year and session name. Common Latin accents and punctuation
  are normalized: `Sao Paulo` matches `São Paulo`, and `Spa` matches
  `Spa-Francorchamps` without matching `Spain`. This is deterministic matching,
  not fuzzy correction or a complete Unicode transliteration library.
- Type shortcuts are R, Q, FP1/FP2/FP3, S, and SQ/shootout. Original provider type
  and name are retained, including unknown values. Generic provider type `Race`
  does not make a Sprint match a race-name search.
- Searches list every match and its provider IDs. `--session ID` chooses exactly
  that session and must also satisfy any supplied year/search filter. The library
  `resolve` method rejects ambiguous searches rather than choosing the first.

Cancelled meetings/sessions, missing start/end times and future end times have
explicit unavailable reasons. Selection of these through the CLI exits with an
error. Known end time at or before the current UTC time establishes only metadata
completion; it does not establish actual telemetry coverage or original broadcast
availability. Selecting a completed ID reports that recorded loading is not yet
implemented and never invokes Python or supplies a generated substitute.

## Cache and validation

The versioned snapshot stores retrieval UTC milliseconds, meetings, sessions and
the exact years whose sessions were fetched. Cache loading validates schema version,
nonzero unique IDs, session/meeting year relationships, covered years, labels and
RFC3339 timestamps. Control characters in displayed provider labels are rejected.
Malformed or corrupt metadata produces an error, not a fallback year/provider.

Refresh uses HTTPS, 10-second connection and 30-second request timeouts, and an
8 MiB response/cache limit. It makes one meetings request and, when requested,
one sessions request. It does not retry automatically; provider errors ask the
user to retry explicitly later. T05 adds rate-limit/retry/cancellation policies
for larger telemetry acquisition. No paid credentials are used.

The user cache is separate from the source tree. Publishing writes a unique sibling
temporary file, flushes it, and renames it over the snapshot only after validation.
Failed acquisition/validation does not publish partial metadata. Corrupt existing
snapshots are reported even during refresh; move/remove the named file explicitly
or choose a different `--catalogue-cache` path to rebuild. The catalogue snapshot
is not the checksummed, resumable recording cache planned in T06.

Chrono 0.4.45 with only `std` support handles offset-aware RFC3339 parsing instead
of implementing calendar/offset arithmetic locally. Existing ureq performs HTTP;
no async runtime or UI dependency is added.

## Evidence, 3 November 2026

Eight offline catalogue tests cover discovery, aliases, ambiguity, missing years,
completion boundaries, cancellation, unknown kinds, malformed timestamps, duplicate
and mismatched IDs, control characters, corrupt/oversized caches, Windows snapshot
replacement and CLI selection/errors. Their provider-shaped metadata is synthetic
and clearly marked; its IDs are not evidence about real races.

A separate public-provider smoke check fetched meetings and 2024 sessions, then
refreshed 2023 while retaining 2024. `Monza R` resolved to 2024 meeting 1244/session
9590; `Monza Q` resolved to 2023 meeting 1218/session 9153. Offline selection of 9590
after that refresh verified retention. Raw smoke metadata stays in ignored build
output and is not committed.

All 28 Rust tests, three Python tests, release build and the existing headless demo
pass. New modules/tests pass rustfmt. Strict workspace Clippy remains blocked by
the previously recorded legacy collapsible-if warnings; no suppression or unrelated
formatting changes are included. This task makes no UI or performance-budget claim.

References: [OpenF1 endpoints](https://openf1.org/docs/#sessions),
[Chrono 0.4.45](https://docs.rs/chrono/0.4.45/chrono/struct.DateTime.html#method.parse_from_rfc3339),
[ureq 2.12.1](https://docs.rs/ureq/2.12.1/ureq/struct.AgentBuilder.html).
