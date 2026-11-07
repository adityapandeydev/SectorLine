//! Generated demo uses the same shell while retaining explicit simulated provenance.
use super::*;
pub(super) fn render(frame: &mut Frame, app: &App, r: &layout::Regions) {
    match app.view {
        View::Timing => {
            let (table, context, detail) = r.timing_columns();
            timing(frame, app, table);
            if let Some(area) = context {
                conditions(frame, app, area);
            }
            if let Some(area) = detail {
                driver(frame, app, area);
            }
        }
        View::Driver => driver(frame, app, r.body),
        View::Conditions => conditions(frame, app, r.body),
    }
}
fn timing(frame: &mut Frame, app: &App, area: Rect) {
    let area = panel(frame, area, "Timing / generated", app.theme);
    let s = &app.stream;
    let cars = &s.current_frame().cars;
    let selected = cars
        .iter()
        .position(|c| c.driver == s.selected_driver_code)
        .unwrap_or(0);
    let start = selected.saturating_sub(area.height.saturating_sub(2) as usize);
    let rows = cars
        .iter()
        .skip(start)
        .take(area.height.saturating_sub(1) as usize)
        .map(|c| {
            Row::new(vec![
                Cell::from(if c.driver == s.selected_driver_code {
                    ">"
                } else {
                    " "
                }),
                Cell::from(c.position.to_string()),
                Cell::from(format!("{} #{}", c.driver, c.number)),
                Cell::from(c.gap_to_leader.clone()),
                Cell::from(format!("{:.0}", c.speed)),
            ])
            .style(if c.driver == s.selected_driver_code {
                app.theme.selected()
            } else {
                app.theme.text()
            })
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(10),
                Constraint::Length(11),
                Constraint::Min(5),
            ],
        )
        .header(Row::new(["", "Pos", "Driver", "Gap", "km/h"]).style(app.theme.title())),
        area,
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
        "Generated telemetry\n\nSpeed {:.0} km/h\nGear {} | RPM {}\nThrottle {}%\nBrake {}\nDRS {}\n\nTyre {} | Age {}\nPit {}\n\nS1 {}\nS2 {}\nS3 {}\n\nSimulation, not observed data",
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
        c.s3
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
