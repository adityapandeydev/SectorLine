# Performance baseline (T01)

Measured 1 November 2026 against application source `65cb371` using the new [baseline example](../f1_tui/examples/baseline.rs). No application behavior was changed. Machine details, per-run summaries and raw-output SHA-256 values are in [the evidence manifest](../f1_tui/tests/fixtures/manifest.json).

## Reference environment

- Intel Core i7-12700H; 14 physical cores / 20 logical processors.
- Windows 11 Pro, 10.0.26300, build 26300; 16,474,168 KiB OS-visible memory (approximately 15.71 GiB).
- Rust 1.98.1, MSVC x86_64, LLVM 22.1.8; standard Cargo release profile, default target CPU, no extra RUSTFLAGS applied by this task.
- Windows Terminal 1.24.12741.0 selected for future visual/input checks. It was detected as installed, not used as a visible rendering benchmark here. Linux/macOS terminal tests remain future release checks.
- Three sequential fresh benchmark processes; normal system activity was not eliminated. Raw samples remain external. Provider acquisition ran independently during part of the measurement period, so these are local baseline observations, not isolated lab results.

## Workload and timing boundaries

Existing generated demo: 22 drivers, 600 frames, timestamps 0-119,800 ms. Four JSON files total 4,793,684 bytes. The dataset is not an observed full race. Event-name text is metadata, not evidence of authenticity.

Each process performs 31 loads: first invocation reported separately, followed by 30 warm-load observations. No OS cache flush is attempted; the first observation is **not** called disk-cold. Load timing includes reading/deserializing/constructing the stream, excludes destruction, process launch, Cargo, network and first visible terminal output. An additional load constructs the rendering app outside frame timing.

For each layout: 300 warmup frames then 1,800 measured frames. Each sample includes `app.tick(1000/30 ms)` and `Terminal::draw` against Ratatui TestBackend, including the in-memory buffer diff. The loop does not sleep and does not measure actual FPS. Backend buffer work is consumed; timing output includes raw sample arrays. Nearest-rank percentiles are computed per process, not pooled into an artificial independent sample set.

Memory is the process lifetime peak working set reported by Windows GetProcessMemoryInfo, after repeated load/drop and rendering. Current working set and commit are also recorded. It is not heap-only, incremental dataset size, terminal memory, or full-race capacity. The first load and repeated allocator reuse can affect the peak.

## Observations

All time columns below are milliseconds. Frame columns use 144x44 cells; peak memory is MiB.

| Process | First load | Warm load median | Warm load p95 | Frame median | Frame p95 | Frame p99 | Peak working set |
|---|---|---|---|---|---|---|---|
| 1 | 33.946 | 30.079 | 34.032 | 0.4178 | 0.6586 | 0.7292 | 19.56 |
| 2 | 29.023 | 29.610 | 33.568 | 0.4325 | 0.6599 | 0.7414 | 18.26 |
| 3 | 31.240 | 30.428 | 34.436 | 0.4055 | 0.6389 | 0.7167 | 18.40 |

| TestBackend dimensions | Per-process p95 range | Per-process p99 range |
|---|---|---|
| 144x44 | 0.6389-0.6599 ms | 0.7167-0.7414 ms |
| 120x36 | 0.5298-0.5652 ms | 0.5989-0.6527 ms |
| 80x24 | 0.4050-0.4208 ms | 0.4602-0.5003 ms |

The demo has ample headless rendering headroom on this machine; there is no evidence here that a GPU or Mojo is needed. Smaller buffers being faster does not establish a usable compact UI: the existing layout has a nominal 96x28 minimum, and an 80x24 measurement can include clipped/compressed content. Visual correctness must be assessed separately.

## Reproduce

From the repository root, using the same lockfile/toolchain and unchanged demo hashes:

```powershell
cargo build --release --locked -p sectorline --example baseline
.	argetelease\examplesaseline.exe > baseline-run-1.json
.	argetelease\examplesaseline.exe > baseline-run-2.json
.	argetelease\examplesaseline.exe > baseline-run-3.json
```

Redirect output outside the repository, or use a temporary directory. The example also accepts a cache directory as its first positional argument. Linux/macOS can run the example but must measure resident memory with host tooling; only the Windows memory implementation is supplied. The example emits optimized/debug status so debug results cannot silently masquerade as release results. Future runs should record their own machine, source revision and cache hashes.

| Input | Bytes | SHA-256 |
|---|---|---|
| drivers.json | 3246 | `289d8a05442def4e7c6b5337efe1dcba24fd6700ddc3b3494df7910b990cb686` |
| session_info.json | 787 | `7042d5206d9d495bed36c68acc3d3cfc8b70c1e273aaa09f48e6448bcdb29b56` |
| telemetry_stream.json | 4766166 | `e28983ad84226186ca2c74136826d3694b646413e95db0437c360e3a46bdc0d3` |
| track_layout.json | 23485 | `42a17adca9e650f20b07d0f89d41d0c640422c0cf6827dd1d4767ea3432e6eef` |

## What remains unmeasured

| Release budget | T01 status |
|---|---|
| p95 frame <=16 ms / p99 <=33 ms on a complete race | Demo/TestBackend measurements only; full-race budget not yet verified |
| p95 input-to-visible response <=50 ms | Not measured; requires a real terminal/input trace |
| p95 cached seek <=100 ms | Not implemented/measured |
| Warm cached startup <=2 seconds | Loader measured; process-to-first-usable-frame not measured |
| <=256 MiB for two-hour/22-car fixture | Small demo measured; capacity target not verified |
| Paused CPU <=1% of one logical CPU | Not measured; current loop redraws continuously even when paused |
| No unbounded growth over three hours | Not measured; current short demo/repeated benchmark is not a soak test |
| Network/download performance | Not measured as a performance promise; capability inspection records actual requests only |

The release budgets are unchanged. Use real session 9590 as the first full-race benchmark candidate, 9636 as the interruption/staleness case, and qualifying/practice fixtures 9586/9583 for capability coverage. Both 2024 race grids contain 20 drivers, so a separate explicitly labeled 22-car capacity fixture is still needed for the exact planned memory target. T05-T12 must produce validated full datasets; T20 performs release-budget verification.

## Verification and existing quality debt

`cargo test --workspace --all-targets --offline --locked` passes (two existing unit tests; the example compiles). The optimized example builds and all three measured processes finish successfully.

`cargo fmt --all --check` fails on existing source formatting. `cargo clippy --workspace --all-targets --offline --locked -- -D warnings` fails on eight existing library diagnostics, all reporting collapsible-if patterns. These commands were run as baseline observations; their failures are not hidden or waived as clean gates. Only the new example is formatted in this task. Production-source cleanup belongs in a focused later change before CI enforces these gates; T01 does not reformat unrelated code or lower lint strictness.

The evidence manifest stores summary statistics and hashes; raw benchmark JSON and raw provider acquisitions are kept outside the repository. This preserves an audit trail without treating generated timing outputs or bulk external data as source files.
