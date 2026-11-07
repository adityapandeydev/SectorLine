//! Generated demo uses the same shell while retaining explicit simulated provenance.
use super::*;
pub(super) fn render(frame: &mut Frame, app: &App, r: &layout::Regions) {
    match app.view {
        View::Timing => {
            let (table, context, detail) = r.timing_columns();
            timing(frame, app, table);
            if let Some(area) = context {
                let [summary, _, map] = Layout::vertical([
                    Constraint::Length(4),
                    Constraint::Length(1),
                    Constraint::Min(0),
                ])
                .areas(area);
                conditions(frame, app, summary);
                super::track::demo(frame, app, map);
            }
            if let Some(area) = detail {
                driver(frame, app, area);
            }
        }
        View::Driver => driver(frame, app, r.body),
        View::Conditions => conditions(frame, app, r.body),
        View::Track => super::track::demo(frame, app, r.body),
        View::Events => super::events::demo(frame, app, r.body),
    }
}
fn timing(frame: &mut Frame, app: &App, area: Rect) {
    let selected = app.stream.selected_car().map(|c| c.number).unwrap_or(0);
    super::timing::render(
        frame,
        app,
        &super::view_model::demo(&app.stream),
        selected,
        area,
        true,
    );
}
fn driver(frame: &mut Frame, app: &App, area: Rect) {
    let s = &app.stream;
    let Some(c) = s.selected_car() else {
        return;
    };
    let area = panel(
        frame,
        area,
        &format!("Selected {} #{} / demo", c.driver, c.number),
        app.theme,
    );
    let text = format!(
        "Generated telemetry\n\nSpeed {:.0} km/h\nGear {} | RPM {}\nThrottle {}%\nBrake {}\nDRS {}\n\nTyre {} | Age {}\nPit {}\n\nS1 {}\nS2 {}\nS3 {}\n\nSimulation, not observed data\nTeam {}",
        c.speed,
        c.gear,
        c.rpm,
        c.throttle,
        if c.brake == 0 { "released" } else { "pressed" },
        if c.drs { "on" } else { "off" },
        c.compound,
        c.tyre_age,
        c.pit_status,
        c.s1,
        c.s2,
        c.s3,
        s.driver_info(&c.driver)
            .map(|d| d.team.as_str())
            .unwrap_or("--")
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
fn conditions(frame: &mut Frame, app: &App, area: Rect) {
    let s = &app.stream;
    let area = panel(frame, area, "Session context / demo", app.theme);
    frame.render_widget(Paragraph::new(format!("Generated session conditions\n\nAir {:.1} C\nTrack {:.1} C\nHumidity {:.0}%\nStatus {}\n\nThese values are simulated.\n\nRecorded track view requires\nvalidated source geometry (T15).",s.session.air_temp_c,s.session.track_temp_c,s.session.humidity_pct,s.session.status)).style(app.theme.text()),area);
}
