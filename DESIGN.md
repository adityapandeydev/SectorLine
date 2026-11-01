# Sector Line terminal design contract

Status: proposed for implementation, not an implemented or visually verified redesign. Surface mode: Operate. Scope: session library, loading states, historical replay workspace, driver detail, event view, timeline, errors/help. Unit: terminal cells, never CSS pixels.

## Visual intent

A compact timing desk: data first, restrained framing, consistent alignment, immediate controls. A precise track-progress marker and session timeline give Sector Line its identity. Modernity comes from hierarchy, spacing, legibility, and behavior, rather than ornamental terminal graphics.

## Current source dimensional reference

At `f1_tui/src/ui.rs:20`, the vertical constraints are 3, minimum 16, 8, and 1 rows; nominal minimum total is 28 rows. At line 98, body constraints are 38, minimum 30, and 28 columns; nominal minimum total is 96 columns. These are layout constraints, not a guarantee of legibility at that size. Border cells further reduce usable space. Terminal aspect ratio and font metrics remain environment-dependent.

The external dimensional sheet documents this incumbent layout using source-backed variable codes. It is intentionally monochrome, uses content slots, and does not imply measured pixel dimensions. Proposed dimensions below are design decisions, not inferred existing values.

## Proposed layout variables

| Code | Variable | Proposed value |
|---|---|---|
| H1 | session header | 2 rows |
| H2 | view tabs | 1 row |
| H3 | replay timeline | 3 rows |
| H4 | source/status | 1 row |
| H5 | contextual keys | 1 row |
| HB | main body | terminal rows - H1-H2-H3-H4-H5 = rows - 8 |
| W1 | wide timing table | 44 columns |
| W2 | wide driver detail | 32 columns |
| S1 | inter-region gutter | 1 column |
| WM | wide map | terminal columns - W1-W2-2*S1 |
| WR | wide target | >=144 columns, >=40 rows |
| WC | standard target | >=120 columns, >=32 rows |
| WN | compact minimum | >=80 columns, >=24 rows |

At 144x44: HB=36 and WM=66, so 44+1+66+1+32=144 and 2+1+36+3+1+1=44. At 120x36, timing is 44, gutter 1, map 75, and driver detail is a separate view; HB=28. At 80x24, HB=16; show timing or selected view, with scrolling. Below minimum, keep quit/help and explain the size requirement. Modal geometry is bounded by actual available dimensions; no assumptions about a fixed 20-car grid.

```text
SESSION HEADER SLOT                                            MODE / TIME SLOT
VIEW TABS SLOT
TIMING TABLE SLOT | TRACK CONTEXT SLOT                 | SELECTED DRIVER SLOT
                 |                                    | (wide layout only)
                 |                                    |
REPLAY TIMELINE / EVENT MARKERS / PLAYBACK CONTROLS SLOT
SOURCE / QUALITY / LOAD STATUS SLOT
CONTEXTUAL KEY HELP SLOT
```

This is a structural schematic, not an accurate cell-width screenshot. All measurements are specified in the variable table rather than inferred from this text.

## Information hierarchy and interaction

Timing columns prioritize position, driver, gap/interval, tyre/stint, lap, and status. Right-align numbers and units; reserve stable widths so updates do not move columns. Selection is a restrained row background plus driver label/marker; team identity uses a small swatch, not a fully colored row. Scrolling keeps the selected driver visible and stable when classification changes.

Track view preserves aspect ratio using source coordinates and terminal cell geometry. Clearly distinguish selected driver, pit lane where supported, start/finish, and unavailable locations. Avoid marker labels that obscure one another; prioritize selected/nearby drivers, with a legend and optional full labels. A map may be unavailable while timing continues to work.

Driver view shows actual speed, gear, throttle, brake on/off, and eligible sectors/lap traces, with units and sample age. No fixed cockpit panel that crowds out the timing table. Race-control entries have scope, event time, readable severity, and a way to seek to the event.

Session library opens immediately, supports year/event/type search, identifies cached/downloadable/unavailable sessions, and records exact selection. Loading has phases, progress, retry/cancel, and a retained previous session. Errors state what failed and what remains usable. Demo mode is explicitly generated; recorded mode never silently substitutes demo values.

Proposed keyboard map: arrows/j/k select driver; Tab cycles views; Space pauses; Left/Right seeks 5 seconds; Shift+Left/Right seeks 30 seconds; +/- changes speed; `g` opens go-to time/lap; `s` opens sessions; `e` events; `?` help; Esc backs out/cancels; q quits with safe cleanup. Avoid ambiguous Ctrl bindings and verify key support on each terminal. Document and keep keys visible in context.

## Palette, text, and motion

Use the terminal's readable neutral background by default; optional controlled charcoal theme. Primary text is near-white in dark mode, labels a sufficiently bright neutral, secondary regions separated by whitespace or one subdued rule. Selection has one accent. Red/yellow/green, sector purple, tyre colors, and team colors are semantic and paired with words or symbols. Add 16-color/no-color modes and ASCII alternatives. Do not depend on emoji width, truecolor, Nerd Fonts, image protocols, or a bundled typeface. User controls font and font size.

Use sentence-case labels and racing vocabulary. Avoid loud all-caps banners, gradients, rainbows, faux glass, animated text, or a bordered tile for every metric. Limit border nesting. No decorative animation on keyboard actions; interpolation only communicates observed movement. Discrete information changes immediately at its event boundary. Pause stops motion and unnecessary redraws.

## Visual acceptance

Capture standard, wide, and compact layouts with populated, missing/stale, loading, error, red-flag, and selected-driver states. Use TestBackend snapshots to catch geometry regressions and actual terminal screenshots to assess readability. Check Unicode/no-color fallback, 22+ drivers with scrolling, small-window behavior, and map aspect ratio. Perform one consolidated review/fix pass per milestone, not endless screenshot churn. These checks happen during implementation, not claimed complete by this plan.
