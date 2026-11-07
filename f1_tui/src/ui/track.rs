//! Quiet, aspect-correct source XY map; labels are laid out in terminal cells.
use super::*;
use crate::{
    domain::geometry::{Motion, Point, Transform},
    replay::RecordedReplay,
};
use ratatui::{
    style::Color,
    symbols::Marker,
    widgets::canvas::{Canvas, Line as CanvasLine, Points},
};
struct Car {
    id: u32,
    label: String,
    point: Point,
    held: bool,
    color: Option<Color>,
}
pub(super) fn recorded(frame: &mut Frame, app: &App, replay: &RecordedReplay, area: Rect) {
    let Some((driver, lap, trace)) = replay.track().reference() else {
        unavailable(
            frame,
            app,
            area,
            "No complete, contiguous pit-free lap trace in this recording.",
        );
        return;
    };
    let now = replay
        .timeline()
        .origin()
        .0
        .saturating_add(replay.frame().elapsed().0.min(i64::MAX as u64) as i64);
    let rows = super::view_model::recorded(replay);
    let cars = rows
        .iter()
        .filter_map(|r| {
            let position = replay.track().position(replay.timeline(), r.id, now)?;
            Some(Car {
                id: r.id,
                label: replay
                    .timeline()
                    .drivers()
                    .iter()
                    .find(|d| d.number.get() == r.id)
                    .map(|d| d.code.clone())
                    .unwrap_or_else(|| r.id.to_string()),
                point: position.point,
                held: position.motion == Motion::Held,
                color: r.team_color,
            })
        })
        .collect::<Vec<_>>();
    draw(
        frame,
        app,
        area,
        Scene {
            points: trace.points(),
            cars: &cars,
            selected: replay.selected(),
            total: rows.len(),
            source: &format!("Observed lap trace #{driver} L{lap} | approximate XY"),
            legend: "Cars -0.5s | [] selected | ~ held",
        },
    );
}
pub(super) fn demo(frame: &mut Frame, app: &App, area: Rect) {
    let points = app
        .stream
        .track
        .points
        .iter()
        .map(|p| Point { x: p.x, y: p.y })
        .collect::<Vec<_>>();
    let rows = super::view_model::demo(&app.stream);
    let cars = app
        .stream
        .current_frame()
        .cars
        .iter()
        .map(|c| Car {
            id: c.number,
            label: c.driver.clone(),
            point: Point { x: c.x, y: c.y },
            held: false,
            color: rows
                .iter()
                .find(|r| r.id == c.number)
                .and_then(|r| r.team_color),
        })
        .collect::<Vec<_>>();
    draw(
        frame,
        app,
        area,
        Scene {
            points: &points,
            cars: &cars,
            selected: app.stream.selected_car().map(|c| c.number).unwrap_or(0),
            total: cars.len(),
            source: "Generated track | simulation",
            legend: "Generated positions | [] selected",
        },
    );
}
fn unavailable(frame: &mut Frame, app: &App, area: Rect, reason: &str) {
    frame.render_widget(Paragraph::new(format!("Track view unavailable\n\n{reason}\nTiming and driver data remain usable.\nNo substitute circuit is drawn.")).wrap(Wrap{trim:false}).style(app.theme.text()),area);
}
struct Scene<'a> {
    points: &'a [Point],
    cars: &'a [Car],
    selected: u32,
    total: usize,
    source: &'a str,
    legend: &'a str,
}
fn draw(frame: &mut Frame, app: &App, area: Rect, scene: Scene<'_>) {
    let Scene {
        points,
        cars,
        selected,
        total,
        source,
        legend,
    } = scene;
    let [heading, quality, plot, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(source).style(app.theme.title()), heading);
    frame.render_widget(Paragraph::new(legend).style(app.theme.text()), quality);
    let Some(transform) = Transform::fit(
        points,
        plot.width,
        plot.height,
        app.map_rotation,
        app.cell_aspect,
    ) else {
        unavailable(frame, app, plot, "Insufficient map extent or space.");
        return;
    };
    let projected = points
        .iter()
        .filter_map(|p| transform.project(*p))
        .collect::<Vec<_>>();
    let mut visible = cars
        .iter()
        .filter_map(|c| transform.project(c.point).map(|p| (c, p)))
        .collect::<Vec<_>>();
    visible.sort_by_key(|(c, _)| (c.id != selected, c.id));
    let dots = visible.iter().map(|(_, p)| (p.x, p.y)).collect::<Vec<_>>();
    frame.render_widget(
        Canvas::default()
            .background_color(Color::Reset)
            .marker(Marker::Braille)
            .x_bounds([0.0, f64::from(plot.width - 1)])
            .y_bounds([0.0, f64::from(plot.height - 1)])
            .paint(|ctx| {
                for pair in projected.windows(2) {
                    ctx.draw(&CanvasLine {
                        x1: pair[0].x,
                        y1: pair[0].y,
                        x2: pair[1].x,
                        y2: pair[1].y,
                        color: Color::Reset,
                    });
                }
                ctx.draw(&Points {
                    coords: &dots,
                    color: if app.theme.color {
                        Color::Cyan
                    } else {
                        Color::Reset
                    },
                });
            }),
        plot,
    );
    let mut occupied = vec![false; usize::from(plot.width) * usize::from(plot.height)];
    let mut hidden = 0;
    for (car, p) in &visible {
        let selected = car.id == selected;
        let name = if car.label.chars().all(|c| c.is_ascii_alphanumeric()) && car.label.len() <= 5 {
            car.label.clone()
        } else {
            format!("#{}", car.id)
        };
        let label = format!(
            "{}{}{}{}",
            if selected { "[" } else { "" },
            if car.held { "~" } else { "" },
            name,
            if selected { "]" } else { "" }
        );
        let width = label.len().min(plot.width as usize) as i32;
        let x = p.x.round() as i32;
        let y = i32::from(plot.height - 1) - p.y.round() as i32;
        let candidates = [
            (x + 1, y),
            (x - width, y),
            (x - width / 2, y - 1),
            (x - width / 2, y + 1),
        ];
        let place = candidates
            .into_iter()
            .map(|(x, y)| {
                (
                    x.clamp(0, i32::from(plot.width) - width),
                    y.clamp(0, i32::from(plot.height) - 1),
                )
            })
            .find(|(x, y)| {
                (0..width)
                    .all(|n| !occupied[(*y as usize) * plot.width as usize + (*x + n) as usize])
            });
        if let Some((x, y)) = place {
            for n in 0..width {
                occupied[y as usize * plot.width as usize + (x + n) as usize] = true;
            }
            let mut style = if selected {
                app.theme.selected()
            } else {
                app.theme.text()
            };
            if !selected
                && app.theme.color
                && let Some(color) = car.color
            {
                style = style.fg(color);
            }
            frame.render_widget(
                Paragraph::new(label).style(style),
                Rect::new(plot.x + x as u16, plot.y + y as u16, width as u16, 1),
            );
        } else {
            hidden += 1;
        }
    }
    let selected_missing = !visible.iter().any(|(c, _)| c.id == selected);
    frame.render_widget(
        Paragraph::new(format!(
            "Shown {}/{} | labels hidden {} | cell h/w {:.1}{}",
            visible.len(),
            total,
            hidden,
            app.cell_aspect,
            if selected_missing {
                " | selected unavailable"
            } else {
                ""
            }
        ))
        .style(app.theme.text()),
        status,
    );
}
