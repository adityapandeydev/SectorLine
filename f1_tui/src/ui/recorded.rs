//! Recorded body panes; shell owns chrome, overlays and source identity.
use super::*;
use crate::{
    domain::{Quality, conditions::Flag},
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
    super::timing::render(
        frame,
        app,
        &super::view_model::recorded(replay),
        replay.selected(),
        area,
        false,
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
    let team = replay
        .timeline()
        .drivers()
        .iter()
        .find(|d| d.number == car.driver.number)
        .and_then(|d| d.team.as_deref())
        .unwrap_or("--");
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
    let gap = condition(&car.gap_to_leader, |g| {
        super::view_model::format_gap(Some(g))
    });
    let interval = condition(&car.gap_to_ahead, |g| {
        super::view_model::format_gap(Some(g))
    });
    let pit = conditions.and_then(|d| d.pit.value());
    let sectors = if area.width >= 72 {
        format!(
            "S1 {} | S2 {} | S3 {}",
            lap_time(car.sectors[0].value()),
            lap_time(car.sectors[1].value()),
            lap_time(car.sectors[2].value())
        )
    } else {
        format!(
            "S1 {}\nS2 {}\nS3 {}",
            lap_time(car.sectors[0].value()),
            lap_time(car.sectors[1].value()),
            lap_time(car.sectors[2].value())
        )
    };
    let text = format!(
        "Team {team}\nGap {gap} | Int {interval} (s or +nL)\nCompleted L{lap} [{eligibility}]\nLap {} | Status {}\n{sectors}\nBest {}\nSession {session}\n* Provisional archive best\nTyre {tyre}\nPit {}\nLane {}s | Stop {}s\nphase unknown\nSpeed {} km/h | Gear {}\nThrottle {}% | Brake {}",
        lap_time(car.lap_time.value()),
        car.status.value().map(|s| s.label()).unwrap_or("--"),
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
    lines.push(Line::raw(""));
    lines.push(Line::styled("Team key", app.theme.title()));
    let teams = super::view_model::recorded(replay)
        .into_iter()
        .filter_map(|r| r.team_name.map(|name| (r.team, name)))
        .collect::<std::collections::BTreeSet<_>>();
    for (code, name) in teams {
        lines.push(Line::raw(format!("{code} {name}")));
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
