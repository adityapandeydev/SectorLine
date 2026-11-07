//! Recorded body panes; shell owns chrome, overlays and source identity.
use super::*;
use crate::{
    domain::{DrsState, Gap, Quality, conditions::Flag},
    replay::RecordedReplay,
};
pub(super) fn render(frame: &mut Frame, app: &App, replay: &RecordedReplay, r: &layout::Regions) {
    match app.view {
        View::Timing => {
            let (table, context, detail) = r.timing_columns();
            timing(frame, app, replay, table);
            if let Some(area) = context {
                conditions(frame, app, replay, area);
            }
            if let Some(area) = detail {
                driver(frame, app, replay, area);
            }
        }
        View::Driver => driver(frame, app, replay, r.body),
        View::Conditions => conditions(frame, app, replay, r.body),
    }
}
fn timing(frame: &mut Frame, app: &App, replay: &RecordedReplay, area: Rect) {
    let area = panel(frame, area, "Timing", app.theme);
    let cars = replay.frame().samples();
    let selected = cars
        .iter()
        .position(|c| c.driver.number.get() == replay.selected())
        .unwrap_or(0);
    let count = area.height.saturating_sub(1) as usize;
    let start = selected.saturating_sub(count.saturating_sub(1));
    let narrow = area.width < 72;
    let rows = cars.iter().skip(start).take(count).map(|car| {
        let code = replay
            .timeline()
            .drivers()
            .iter()
            .find(|d| d.number == car.driver.number)
            .map(|d| d.code.as_str())
            .unwrap_or("--");
        let age = car
            .speed_kmh
            .timing()
            .observed_at
            .map(|t| {
                format!(
                    "{:.1}s",
                    replay
                        .timeline()
                        .origin()
                        .0
                        .saturating_add(replay.frame().elapsed().0 as i64)
                        .saturating_sub(t.0) as f64
                        / 1000.0
                )
            })
            .unwrap_or_else(|| "--".into());
        let mut values = vec![
            if car.driver.number.get() == replay.selected() {
                ">".into()
            } else {
                " ".into()
            },
            car.position
                .value()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "--".into()),
            format!("{code} #{}", car.driver.number),
            condition(&car.gap_to_leader, |g| gap(Some(g))),
            condition(&car.speed_kmh, |v| format!("{:.0}", v.get())),
        ];
        if !narrow {
            values.extend([
                condition(&car.throttle, |v| format!("{:.0}", v.get())),
                condition(&car.brake, |v| format!("{v:?}").to_lowercase()),
                condition(&car.drs, |d| match d.state() {
                    DrsState::Off => "off".into(),
                    DrsState::On => "on".into(),
                    DrsState::Eligible => "eligible".into(),
                    DrsState::Unknown => format!("?{}", d.0),
                }),
            ]);
        }
        values.push(age);
        Row::new(values.into_iter().map(Cell::from)).style(
            if car.driver.number.get() == replay.selected() {
                app.theme.selected()
            } else {
                app.theme.text()
            },
        )
    });
    let (headers, widths) = if narrow {
        (
            vec!["", "Pos", "Driver", "Gap(s/lap)", "km/h", "Age"],
            vec![
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(6),
                Constraint::Min(4),
            ],
        )
    } else {
        (
            vec![
                "",
                "Pos",
                "Driver",
                "Gap(s/lap)",
                "km/h",
                "Thr%",
                "Brake",
                "DRS",
                "Car age",
            ],
            vec![
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(6),
                Constraint::Length(5),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Min(5),
            ],
        )
    };
    frame.render_widget(
        Table::new(rows, widths)
            .column_spacing(1)
            .header(Row::new(headers).style(app.theme.title())),
        area,
    );
}
fn driver(frame: &mut Frame, app: &App, replay: &RecordedReplay, area: Rect) {
    let Some(car) = replay
        .frame()
        .samples()
        .iter()
        .find(|c| c.driver.number.get() == replay.selected())
    else {
        return;
    };
    let area = panel(
        frame,
        area,
        &format!("Selected #{}", car.driver.number),
        app.theme,
    );
    let lap = car
        .lap_number
        .value()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "--".into());
    let eligibility = match car.lap_eligibility.value() {
        Some(crate::domain::timing::Eligibility::Eligible) => "eligible",
        Some(crate::domain::timing::Eligibility::Ineligible) => "invalid",
        _ => "unverified",
    };
    let best = |b: Option<&crate::domain::timing::BestLap>| {
        b.map(|b| {
            format!(
                "{}{} L{}",
                lap_time(Some(&b.duration)),
                if b.eligibility == crate::domain::timing::Eligibility::Unknown {
                    "*"
                } else {
                    ""
                },
                b.number
            )
        })
        .unwrap_or_else(|| "--".into())
    };
    let session = replay
        .frame()
        .session_best()
        .map(|(d, b)| format!("#{d} {}", best(Some(b))))
        .unwrap_or_else(|| "--".into());
    let conditions = replay.conditions().drivers.get(&car.driver.number.get());
    let tyre = conditions
        .map(|d| {
            condition(&d.tyre, |t| {
                format!(
                    "{} age {}L",
                    t.compound.as_deref().unwrap_or("--"),
                    t.completed_age
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "--".into())
                )
            })
        })
        .unwrap_or_else(|| "--".into());
    let pit = conditions.and_then(|d| d.pit.value());
    let text = format!(
        "Completed L{lap} [{eligibility}]\nLap {} | Status {}\nS1 {}\nS2 {}\nS3 {}\nBest {}\nSession {session}\n* Provisional archive best\nTyre {tyre}\nPit {}\nLane {}s | Stop {}s\nphase unknown\nSpeed {} km/h | Gear {}\nThrottle {}% | Brake {}",
        lap_time(car.lap_time.value()),
        car.status.value().map(|s| s.label()).unwrap_or("--"),
        lap_time(car.sectors[0].value()),
        lap_time(car.sectors[1].value()),
        lap_time(car.sectors[2].value()),
        best(car.personal_best.value()),
        pit.map(|p| format!(
            "recorded L{}",
            p.lap.map(|v| v.to_string()).unwrap_or_else(|| "--".into())
        ))
        .unwrap_or_else(|| "--".into()),
        pit.and_then(|p| p.lane_seconds)
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "--".into()),
        pit.and_then(|p| p.stop_seconds)
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "--".into()),
        condition(&car.speed_kmh, |v| format!("{:.0}", v.get())),
        condition(&car.gear, |v| v.get().to_string()),
        condition(&car.throttle, |v| format!("{:.0}", v.get())),
        condition(&car.brake, |v| format!("{v:?}").to_lowercase())
    );
    frame.render_widget(
        scroll(
            Paragraph::new(text).style(app.theme.text()),
            area,
            app.panel_scroll,
        ),
        area,
    );
}
fn conditions(frame: &mut Frame, app: &App, replay: &RecordedReplay, area: Rect) {
    let area = panel(frame, area, "Session conditions", app.theme);
    let c = replay.conditions();
    let flag = condition(&c.track_flag, |v| format!("{v:?}"));
    let mut lines = vec![
        Line::from(vec![
            Span::raw("Track "),
            Span::styled(
                flag,
                if c.track_flag.quality() == Quality::Stale {
                    app.theme.warning()
                } else {
                    match c.track_flag.value() {
                        Some(Flag::Red) => app.theme.danger(),
                        Some(Flag::Yellow | Flag::DoubleYellow) => app.theme.warning(),
                        _ => app.theme.text(),
                    }
                },
            ),
        ]),
        Line::raw(format!(
            "SC {} | Session {}",
            condition(&c.safety_car, |v| format!("{v:?}")),
            condition(&c.session_status, |v| format!("{v:?}"))
        )),
        Line::raw(""),
        Line::raw(format!(
            "Air {} C | Track {} C",
            condition(&c.air_c, |v| format!("{v:.1}")),
            condition(&c.track_c, |v| format!("{v:.1}"))
        )),
        Line::raw(format!(
            "Rain {} | Humidity {}%",
            condition(&c.rain, |v| if *v { "yes".into() } else { "no".into() }),
            condition(&c.humidity, |v| format!("{v:.0}"))
        )),
        Line::raw(format!(
            "Wind {} m/s | Direction {} deg",
            condition(&c.wind_mps, |v| format!("{v:.1}")),
            condition(&c.wind_degrees, |v| format!("{v:.0}"))
        )),
        Line::raw(""),
        Line::styled("Local reports", app.theme.title()),
    ];
    if c.local_flags.is_empty() {
        lines.push(Line::raw("-- No eligible local report"));
    }
    for (scope, flag) in &c.local_flags {
        lines.push(Line::raw(format!(
            "{scope:?}: {}",
            condition(flag, |v| format!("{v:?}"))
        )));
    }
    lines.extend([
        Line::raw(""),
        Line::raw("Track view unavailable"),
        Line::raw("Geometry is not validated for this recording."),
        Line::raw("-- missing | ~ stale held report"),
        Line::raw("Stale reports do not confirm current conditions."),
    ]);
    frame.render_widget(
        scroll(
            Paragraph::new(lines)
                .style(app.theme.text())
                .wrap(Wrap { trim: false }),
            area,
            app.panel_scroll,
        ),
        area,
    );
}
fn gap(value: Option<&Gap>) -> String {
    match value {
        Some(Gap::Leader) => "leader".into(),
        Some(Gap::Seconds(v)) => format!("{:.3}", v.get()),
        Some(Gap::LapsBehind(n)) => format!("+{n}L"),
        None => "--".into(),
    }
}

fn lap_time(value: Option<&crate::domain::Seconds>) -> String {
    value
        .map(|v| super::shell::clock((v.get() * 1000.0).round() as u64))
        .unwrap_or_else(|| "--".into())
}

fn condition<T>(
    value: &crate::domain::Observation<T>,
    format: impl FnOnce(&T) -> String,
) -> String {
    value
        .value()
        .map(|v| {
            format!(
                "{}{}",
                if value.quality() == crate::domain::Quality::Stale {
                    "~"
                } else {
                    ""
                },
                format(v)
            )
        })
        .unwrap_or_else(|| "--".into())
}
