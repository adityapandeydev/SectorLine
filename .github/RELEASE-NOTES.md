Draft build for owner review. This workflow never publishes a release automatically.

Windows x86_64 has local interactive smoke evidence. Linux x86_64 and macOS arm64
archives remain EXPERIMENTAL until native interactive resize/cleanup checks pass;
CI headless tests alone do not establish terminal support. macOS binaries are not
signed/notarized and Linux binaries use the runner's glibc baseline.

Archives bundle a generated DEMO, not race recordings. Historical source data must
be downloaded separately from OpenF1; no provider credentials are bundled.

Sector Line code is MIT licensed. LICENSE and THIRD-PARTY-LICENSES.txt accompany
the executable; DATA.md documents data semantics and rights boundaries. Provider
data and Formula 1 marks are not relicensed under MIT.

Stable publication remains pending release review. Outstanding gates include the
two-second warm-start target and visible terminal latency. Review CI results,
checksums, platform limits and attribution before publishing.
