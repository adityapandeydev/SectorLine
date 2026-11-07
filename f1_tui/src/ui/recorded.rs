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
                let [summary, _, map] = Layout::vertical([
                    Constraint::Length(4),
                    Constraint::Length(1),
                    Constraint::Min(0),
                ])
                .areas(area);
                conditions(frame, app, replay, summary);
                super::track::recorded(frame, app, replay, map);
            }
            if let Some(area) = detail {
                super::driver::recorded(frame, app, replay, area);
            }
        }
        View::Driver => super::driver::recorded(frame, app, replay, r.body),
        View::Conditions => conditions(frame, app, replay, r.body),
        View::Track => super::track::recorded(frame, app, replay, r.body),
        View::Events => super::events::recorded(frame, app, replay, r.body),
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
        Line::raw(if replay.track().reference().is_some() {
            "Observed lap trace available in Track view"
        } else {
            "Track view unavailable"
        }),
        Line::raw(if replay.track().reference().is_some() {
            "Reference uses archived session coordinates, not a surveyed outline."
        } else {
            "No validated complete lap trace for this recording."
        }),
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
