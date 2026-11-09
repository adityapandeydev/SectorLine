//! Available-by-now event browser. No future-event count, automatic labels or inferred incidents.
use super::*;
use crate::{
    domain::{Availability, UtcMillis, conditions::Scope},
    replay::RecordedReplay,
};
pub(super) fn demo(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Paragraph::new("Events unavailable in generated demo\n\nOpen a recording to browse parsed control reports, completed laps, eligibility decisions and recorded pit visits.\nNo simulated event stream substitutes for provider evidence.").wrap(Wrap{trim:false}).style(app.theme.text()),area);
}
pub(super) fn scope(value: &Scope) -> String {
    match value {
        Scope::Track => "Track".into(),
        Scope::Sector(n) => format!("Sector {n}"),
        Scope::Driver(n) => format!("Driver #{n}"),
    }
}
fn utc(time: UtcMillis) -> String {
    chrono::DateTime::from_timestamp_millis(time.0)
        .map(|t| t.format("%H:%M:%S%.3f").to_string())
        .unwrap_or_else(|| "--".into())
}
pub(super) fn timestamp(time: UtcMillis) -> String {
    chrono::DateTime::from_timestamp_millis(time.0)
        .map(|t| t.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string())
        .unwrap_or_else(|| "--".into())
}
/// Context belongs to the current selected driver/cursor, independently of the report selection.
pub(super) fn context_text(replay: &RecordedReplay) -> String {
    let rows = replay.insights();
    let mut lines = vec![format!(
        "Context | driver #{} | replay now",
        replay.selected()
    )];
    if rows.is_empty() {
        lines.push("-- Insufficient fresh evidence for contextual insights.".into());
    }
    for i in rows {
        lines.push(i.summary);
        lines.push(format!(
            "Evidence window {} -> {}",
            timestamp(i.window_start),
            timestamp(i.observed)
        ));
        let available = match i.available {
            Availability::Known(t) => format!("known {}", timestamp(t)),
            Availability::Estimated(t) => format!("estimated {}", timestamp(t)),
            Availability::Unknown => "--".into(),
        };
        lines.push(format!("Available {available}"));
        lines.push(i.evidence);
        lines.push(String::new());
    }
    lines.join("\n")
}
pub(super) fn recorded(frame: &mut Frame, app: &App, replay: &RecordedReplay, area: Rect) {
    let rows = replay.visible_events();
    let [title, help, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(format!("Evidence timeline | {} available", rows.len()))
            .style(app.theme.title()),
        title,
    );
    frame.render_widget(
        Paragraph::new("Parsed reports | Enter seeks availability | [/] detail & context")
            .style(app.theme.text()),
        help,
    );
    if rows.is_empty() {
        app.event_selection.set(None);
        app.event_offset.set(0);
        frame.render_widget(
            scroll(
                Paragraph::new(format!(
                    "-- No parsed reports available at this replay time.\n\n{}",
                    context_text(replay)
                ))
                .wrap(Wrap { trim: false })
                .style(app.theme.text()),
                body,
                app.panel_scroll,
            ),
            body,
        );
        return;
    }
    let selected = app
        .event_selection
        .get()
        .filter(|i| *i < rows.len())
        .unwrap_or(rows.len() - 1);
    if app.event_selection.get().is_some_and(|i| i >= rows.len()) {
        app.event_selection.set(None);
    }
    let (list, detail) = if body.width >= 120 {
        let [a, _, b] = Layout::horizontal([
            Constraint::Length(52),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(body);
        (a, b)
    } else {
        let [a, _, b] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(6),
        ])
        .areas(body);
        (a, b)
    };
    let range = super::view_model::viewport(
        rows.len(),
        selected,
        list.height.saturating_sub(1) as usize,
        app.event_offset.get(),
    );
    app.event_offset.set(range.start);
    let items = range.clone().map(|i| {
        let e = &rows[i];
        Row::new(vec![
            Cell::from(if i == selected { ">" } else { " " }),
            Cell::from(utc(e.observed)),
            Cell::from(scope(&e.scope)),
            Cell::from(e.summary.clone()),
        ])
        .style(if i == selected {
            app.theme.selected()
        } else {
            app.theme.text()
        })
    });
    frame.render_widget(
        Table::new(
            items,
            [
                Constraint::Length(1),
                Constraint::Length(12),
                Constraint::Length(11),
                Constraint::Min(0),
            ],
        )
        .column_spacing(1)
        .header(Row::new(["", "Report UTC", "Scope", "Parsed evidence"]).style(app.theme.title())),
        list,
    );
    let e = &rows[selected];
    let available = match e.available {
        Availability::Known(t) => format!("known {}", timestamp(t)),
        Availability::Estimated(t) => format!("estimated {}", timestamp(t)),
        Availability::Unknown => "unknown".into(),
    };
    let target = e
        .seek_target(replay.timeline())
        .map(|t| format!("+{}", super::shell::clock(t.0)))
        .unwrap_or_else(|| "-- outside recording coverage".into());
    let text = format!(
        "{} | {}\nReport {}\nAvailable {available}\nJump {target} | Source {}\n{}\nRetrieved {}\n{} | {}/{} available",
        scope(&e.scope),
        e.summary,
        timestamp(e.observed),
        e.channel,
        e.evidence,
        timestamp(e.retrieved),
        if app.event_selection.get().is_none() {
            "Following latest"
        } else {
            "Browsing history"
        },
        selected + 1,
        rows.len()
    );
    let text = format!("{text}\n\n{}", context_text(replay));
    frame.render_widget(
        scroll(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .style(app.theme.text()),
            detail,
            app.panel_scroll,
        ),
        detail,
    );
}
