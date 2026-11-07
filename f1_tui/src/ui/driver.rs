//! Typed driver readouts and point-only source history; no fabricated gauges or best sectors.
use super::*;
use crate::{
    domain::{timing::Eligibility, *},
    replay::{RecordedReplay, details::HistoryPoint},
};
fn value<T>(o: &Observation<T>, format: impl FnOnce(&T) -> String) -> String {
    o.value()
        .map(|v| {
            format!(
                "{}{}",
                if o.quality() == Quality::Stale {
                    "~"
                } else {
                    ""
                },
                format(v)
            )
        })
        .unwrap_or_else(|| "--".into())
}
fn duration(s: Option<&Seconds>) -> String {
    s.map(|v| super::shell::clock((v.get() * 1000.0).round() as u64))
        .unwrap_or_else(|| "--".into())
}
fn age<T>(o: &Observation<T>, now: i64) -> String {
    o.timing()
        .observed_at
        .map(|t| format!("{:.3}s", now.saturating_sub(t.0) as f64 / 1000.0))
        .unwrap_or_else(|| "--".into())
}
fn best(b: Option<&crate::domain::timing::BestLap>) -> String {
    b.map(|b| {
        format!(
            "{}{} L{}",
            duration(Some(&b.duration)),
            if b.eligibility == Eligibility::Unknown {
                "*"
            } else {
                ""
            },
            b.number
        )
    })
    .unwrap_or_else(|| "--".into())
}
pub(super) fn recorded(frame: &mut Frame, app: &App, replay: &RecordedReplay, area: Rect) {
    let Some(car) = replay
        .frame()
        .samples()
        .iter()
        .find(|c| c.driver.number.get() == replay.selected())
    else {
        return;
    };
    let body = panel(
        frame,
        area,
        &format!("Selected #{}", car.driver.number),
        app.theme,
    );
    if body.width >= 120 {
        let [summary, _, history] = Layout::horizontal([
            Constraint::Length(56),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(body);
        render_lines(
            frame,
            app,
            summary,
            summary_lines(replay, car, summary.width),
        );
        render_lines(
            frame,
            app,
            history,
            history_lines(replay, car, history.width),
        );
    } else {
        let mut lines = summary_lines(replay, car, body.width);
        if body.width >= 72 {
            lines.push(Line::raw(""));
            lines.extend(history_lines(replay, car, body.width));
        }
        render_lines(frame, app, body, lines);
    }
}
fn render_lines(frame: &mut Frame, app: &App, area: Rect, lines: Vec<Line<'static>>) {
    frame.render_widget(
        scroll(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .style(app.theme.text()),
            area,
            app.panel_scroll,
        ),
        area,
    );
}
fn summary_lines(replay: &RecordedReplay, car: &CarSample, width: u16) -> Vec<Line<'static>> {
    let wide = width >= 52;
    let driver = replay
        .timeline()
        .drivers()
        .iter()
        .find(|d| d.number == car.driver.number);
    let now = replay
        .timeline()
        .origin()
        .0
        .saturating_add(replay.frame().elapsed().0.min(i64::MAX as u64) as i64);
    let identity = format!(
        "Driver {} #{}{}",
        driver.map(|d| d.code.as_str()).unwrap_or("--"),
        car.driver.number,
        if wide {
            driver.map(|d| format!(" | {}", d.name)).unwrap_or_default()
        } else {
            String::new()
        }
    );
    let gap = value(&car.gap_to_leader, |v| {
        super::view_model::format_gap(Some(v))
    });
    let interval = value(&car.gap_to_ahead, |v| {
        super::view_model::format_gap(Some(v))
    });
    let mut lines = vec![
        identity,
        format!(
            "Team {}",
            driver.and_then(|d| d.team.as_deref()).unwrap_or("--")
        ),
        format!("Gap {gap} | Int {interval} (s or +nL)"),
    ];
    let speed = format!(
        "Speed {} km/h",
        value(&car.speed_kmh, |v| format!("{:.0}", v.get()))
    );
    let gears = format!(
        "Gear {} | RPM {}",
        value(&car.gear, |v| if v.get() == 0 {
            "N".into()
        } else {
            v.get().to_string()
        }),
        value(&car.rpm, |v| v.0.to_string())
    );
    if wide {
        lines.push(format!("{speed} | {gears}"));
    } else {
        lines.extend([speed, gears]);
    }
    let brake = value(&car.brake, |v| {
        match v {
            BrakeState::Pressed => "on",
            BrakeState::Released => "off",
        }
        .into()
    });
    lines.push(format!(
        "Throttle {}% | Brake {brake}",
        value(&car.throttle, |v| format!("{:.0}", v.get()))
    ));
    let drs = value(&car.drs, |v| match v.state() {
        DrsState::On => "on".into(),
        DrsState::Off => "off".into(),
        DrsState::Eligible => "eligible".into(),
        DrsState::Unknown => format!("unknown ({})", v.0),
    });
    let sample_age = format!("Sample age {}", age(&car.speed_kmh, now));
    if wide {
        lines.push(format!("DRS {drs} | {sample_age}"));
    } else {
        lines.extend([format!("DRS {drs}"), sample_age]);
    }
    let eligibility = match car.lap_eligibility.value() {
        Some(Eligibility::Eligible) => "eligible",
        Some(Eligibility::Ineligible) => "invalid",
        _ => "unverified",
    };
    lines.push(format!(
        "Completed L{} [{eligibility}]",
        value(&car.lap_number, |v| v.to_string())
    ));
    lines.push(format!(
        "Lap {} | Status {}",
        duration(car.lap_time.value()),
        car.status.value().map(|s| s.label()).unwrap_or("--")
    ));
    let sectors = (0..3)
        .map(|i| format!("S{} {}", i + 1, duration(car.sectors[i].value())))
        .collect::<Vec<_>>();
    if wide {
        lines.push(sectors.join(" | "));
    } else {
        lines.extend(sectors);
    }
    let provisional = car
        .personal_best
        .value()
        .is_some_and(|b| b.eligibility == Eligibility::Unknown);
    lines.push(format!(
        "Best {}{}",
        best(car.personal_best.value()),
        if provisional {
            " | Provisional archive best"
        } else {
            ""
        }
    ));
    let session = replay
        .frame()
        .session_best()
        .map(|(d, b)| format!("#{d} {}", best(Some(b))))
        .unwrap_or_else(|| "--".into());
    lines.push(format!("Session {session}"));
    let conditions = replay.conditions().drivers.get(&replay.selected());
    let tyre = conditions
        .map(|c| {
            value(&c.tyre, |t| {
                format!(
                    "{} age {}L",
                    t.compound.as_deref().unwrap_or("--"),
                    t.completed_age
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "--".into())
                )
            })
        })
        .unwrap_or_else(|| "--".into());
    lines.push(format!("Tyre {tyre}"));
    let pit = conditions.and_then(|c| c.pit.value());
    let pit_lap = pit
        .map(|p| {
            format!(
                "recorded L{}",
                p.lap.map(|n| n.to_string()).unwrap_or_else(|| "--".into())
            )
        })
        .unwrap_or_else(|| "--".into());
    let lane = pit
        .and_then(|p| p.lane_seconds)
        .map(|v| format!("{v:.3}"))
        .unwrap_or_else(|| "--".into());
    let stop = pit
        .and_then(|p| p.stop_seconds)
        .map(|v| format!("{v:.3}"))
        .unwrap_or_else(|| "--".into());
    lines.extend([
        format!("Pit {pit_lap} | phase unknown"),
        format!("Lane {lane}s | Stop {stop}s"),
        String::new(),
        "Eligible comparisons".into(),
    ]);
    if let Some(current) = replay.comparable_lap()
        && let Some(reference) = replay.eligible_reference()
    {
        lines.push(format!("vs eligible PB L{}", reference.number));
        lines.push(format!(
            "Lap delta {:+.3}s",
            current.duration.unwrap().get() - reference.duration.unwrap().get()
        ));
        for i in 0..3 {
            lines.push(format!(
                "S{} delta {}s",
                i + 1,
                current.sectors[i]
                    .zip(reference.sectors[i])
                    .map(|(a, b)| format!("{:+.3}", a.get() - b.get()))
                    .unwrap_or_else(|| "--".into())
            ));
        }
        lines.push("Sectors compare the same reference lap.".into());
    } else {
        lines.push("-- eligible comparable lap/reference unavailable".into());
    }
    lines.push("* Provisional archive best; excluded from comparisons.".into());
    lines.into_iter().map(Line::raw).collect()
}
fn history_lines(replay: &RecordedReplay, car: &CarSample, width: u16) -> Vec<Line<'static>> {
    let rows = replay.telemetry_history();
    let now = replay
        .timeline()
        .origin()
        .0
        .saturating_add(replay.frame().elapsed().0.min(i64::MAX as u64) as i64);
    let mut lines = vec![Line::raw(format!(
        "Telemetry history | {} admitted samples / 10s",
        rows.len()
    ))];
    lines.extend(trace(&rows, now, width, "Speed km/h", |r| r.speed_kmh));
    lines.push(Line::raw(""));
    lines.extend(trace(&rows, now, width, "Throttle %", |r| {
        r.throttle_percent
    }));
    lines.push(Line::raw(
        "Points only; blanks have no eligible value. No connecting lines.",
    ));
    lines.push(Line::raw(""));
    lines.push(Line::raw("Telemetry evidence"));
    lines.push(Line::raw(format!(
        "Source {} | age {}",
        car.speed_kmh.source().channel,
        age(&car.speed_kmh, now)
    )));
    let availability = match car.speed_kmh.timing().available_at {
        Availability::Known(t) => format!("known {}", super::events::timestamp(t)),
        Availability::Estimated(t) => format!("estimated {}", super::events::timestamp(t)),
        Availability::Unknown => "unknown".into(),
    };
    lines.push(Line::raw(format!(
        "Quality {}",
        format!("{:?}", car.speed_kmh.quality()).to_ascii_lowercase()
    )));
    lines.push(Line::raw(format!("Available {availability}")));
    if let Some(reason) = car.speed_kmh.reason() {
        lines.push(Line::raw(reason.to_owned()));
    }
    lines.push(Line::raw(
        "Gear N=neutral; -- unknown. Brake is on/off, not pressure.",
    ));
    lines
}
fn trace(
    rows: &[HistoryPoint],
    now: i64,
    width: u16,
    label: &str,
    field: fn(&HistoryPoint) -> Option<f64>,
) -> Vec<Line<'static>> {
    let values = rows.iter().filter_map(field).collect::<Vec<_>>();
    if values.is_empty() {
        return vec![Line::raw(format!("{label} trace -- (no eligible values)"))];
    }
    let min = values.iter().copied().reduce(f64::min).unwrap();
    let max = values.iter().copied().reduce(f64::max).unwrap();
    let cols = usize::from(width.saturating_sub(2).clamp(1, 80));
    let mut dots = vec![0u8; cols * 3];
    for r in rows {
        let Some(v) = field(r) else {
            continue;
        };
        let x = (((r.at.0 - now.saturating_sub(10000)) as f64 / 10000.0) * (cols * 2 - 1) as f64)
            .clamp(0.0, (cols * 2 - 1) as f64) as usize;
        let y = if max == min {
            5
        } else {
            ((max - v) / (max - min) * 11.0).round() as usize
        }
        .min(11);
        let bit = match (x % 2, y % 4) {
            (0, 0) => 0,
            (0, 1) => 1,
            (0, 2) => 2,
            (0, _) => 6,
            (1, 0) => 3,
            (1, 1) => 4,
            (1, 2) => 5,
            _ => 7,
        };
        dots[(y / 4) * cols + x / 2] |= 1 << bit;
    }
    let mut lines = vec![Line::raw(format!("{label} trace [{min:.1} .. {max:.1}]"))];
    lines.extend(dots.chunks(cols).map(|row| {
        Line::raw(
            row.iter()
                .map(|b| {
                    if *b == 0 {
                        ' '
                    } else {
                        char::from_u32(0x2800 + u32::from(*b)).unwrap()
                    }
                })
                .collect::<String>(),
        )
    }));
    lines
}
