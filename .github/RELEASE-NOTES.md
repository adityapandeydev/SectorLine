Draft build for owner review. This workflow never publishes a release automatically.

Windows x86_64 has local interactive smoke evidence. Linux x86_64 and macOS arm64
archives remain EXPERIMENTAL until native interactive resize/cleanup checks pass;
CI headless tests alone do not establish terminal support. macOS binaries are not
signed/notarized and Linux binaries use the runner's glibc baseline.

Archives bundle a generated DEMO, not race recordings. Historical source data must
be downloaded separately from OpenF1; no provider credentials are bundled.

Do not publish before the owner chooses a code license and completes the release
review. Outstanding gates include the two-second warm-start target and visible
terminal latency. Review CI results, checksums, platform limits and attribution.
