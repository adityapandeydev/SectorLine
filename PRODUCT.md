# Sector Line

<!-- impeccable:product-schema 1 -->

## Platform

Terminal application: a native Rust TUI. Web/native-mobile platform presets do not apply.

## Users and purpose

Confirmed: Formula 1 fans using a personal race companion; a secondary future use is a watch-along display. The owner is learning Rust and wants a useful standalone project with a contemporary, restrained TUI, smooth interaction, and correct data.

## Release scope

Confirmed on 1 November 2026: **free historical replay first; live support later**. A paid account, Python installation, GPU, or hosted backend must not be required for the core v1 experience. Current implementation: Rust 2024, Ratatui 0.29, Crossterm 0.28, serde/serde_json, ureq; optional Python extraction bridge.

V1 is complete when a user can install a binary, discover a supported completed session, download and validate its available observations, replay and seek it, inspect drivers and race events, synchronize with their own recording, and replay the cached session offline. Races are the first acceptance path; qualifying and practice are included through capability-aware displays rather than pretending all sessions are races.

## Brand commitments

Name: Sector Line; Cargo package/executable: `sectorline`. Modern, readable, purposeful terminal design. Avoid excessive ornament and generic generated-dashboard styling. No implied official affiliation.

## Operating context

Keyboard-first; resizeable terminals; Windows is the owner's current environment. Linux/macOS binaries are intended release targets, contingent on actual CI and smoke tests. Color depth, Unicode widths, and terminal rendering vary. Source observations may be absent, approximate, delayed, inconsistent, or corrected.

## Evidence on hand

The source contains a session modal, cached playback, track drawing, timing table, and selected-car gauges. The current cache is a generated demonstration mixed with real metadata; it is **not a correctness oracle**. Current data/loading/UI limitations are recorded in tasks/plan.md. There are two existing unit tests; a passing cached demonstration does not establish provider fidelity.

## Product principles

1. A missing or stale observation remains visibly missing or stale.
2. Every displayed value has a defined source, timestamp, unit, and interpretation.
3. Smooth rendering must not invent measured precision or future knowledge.
4. Keyboard input and cancellation remain responsive during loading.
5. Contextual intelligence is explainable and reproducible.

## Decisions proposed by the implementation plan

OpenF1 is the primary account-free historical source. Native Rust acquisition is preferred for end-user portability; existing FastF1 integration may remain an optional developer import tool. Release layout dimensions and performance targets are proposed, subject to actual measurement and visual review. The interface has no requirement for an LLM, GPU, or Mojo runtime. Code license remains an owner decision before release; code and third-party data rights must be distinguished.
