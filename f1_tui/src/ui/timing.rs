//! Fixed cell widths and identity-based viewport; no inferred race order or status.
use super::*;
use ratatui::layout::Alignment;
use view_model::TimingRow;
#[derive(Clone, Copy)]
struct Column {
    title: &'static str,
    width: u16,
    right: bool,
    field: fn(&TimingRow) -> &str,
}
pub(super) fn render(
    frame: &mut Frame,
    app: &App,
    rows: &[TimingRow],
    selected: u32,
    area: Rect,
    generated: bool,
) {
    let visible = area.height.saturating_sub(3) as usize;
    let selected_index = rows.iter().position(|r| r.id == selected).unwrap_or(0);
    let range = view_model::viewport(rows.len(), selected_index, visible, app.timing_offset.get());
    app.timing_offset.set(range.start);
    let [heading, legend, table] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(format!(
            "Timing {}-{} / {}{}",
            if range.is_empty() { 0 } else { range.start + 1 },
            range.end,
            rows.len(),
            if generated { " / generated" } else { "" }
        ))
        .style(app.theme.title()),
        heading,
    );
    let race = generated
        || app
            .prepared_recording
            .as_ref()
            .is_some_and(|r| r.context().race_fields());
    let legend_text = if race {
        "Ty S/M/H/I/W | L=completed | ~stale".into()
    } else if let Some(r) = &app.prepared_recording {
        if r.context().qualifying() {
            r.phase()
                .part
                .map(|n| {
                    format!(
                        "{} best | * provisional | L# completed",
                        r.context().part_label(n)
                    )
                })
                .unwrap_or_else(|| "Partial: phase -- | best withheld".into())
        } else {
            "Best(s) | * provisional | L# completed".into()
        }
    } else {
        String::new()
    };
    frame.render_widget(Paragraph::new(legend_text).style(app.theme.text()), legend);
    let wide = area.width >= 80;
    let column = |title, width, right, field| Column {
        title,
        width,
        right,
        field,
    };
    let mut columns = vec![
        column("", 1, false, |_| ""),
        column(if race { "Pos" } else { "Rpt" }, 3, true, |r| &r.position),
        column(
            "Driver",
            if wide { if race { 12 } else { 10 } } else { 8 },
            false,
            |r| &r.driver,
        ),
        column("Tm", 3, false, |r| &r.team),
        if race {
            column("Gap(s)", 7, true, |r| &r.gap)
        } else {
            column("Best(s)", 9, true, |r| &r.best)
        },
    ];
    if wide && race {
        columns.push(column("Int(s)", 8, true, |r| &r.interval));
    }
    columns.extend([
        column("Ty", if !wide && !race { 2 } else { 3 }, false, |r| &r.tyre),
        column("Age", 3, true, |r| &r.tyre_age),
        column(if race { "Lap" } else { "L#" }, 3, true, |r| &r.lap),
    ]);
    if wide {
        columns.extend([
            column("Last(s)", 9, true, |r| &r.last),
            column("km/h", 6, true, |r| &r.speed),
            column("AgeS", 5, true, |r| &r.sample_age),
        ]);
    }
    columns.push(column(
        if race { "State" } else { "St" },
        if race { 5 } else { 4 },
        false,
        |r| &r.status,
    ));
    let header = Row::new(columns.iter().map(|c| {
        Cell::from(Line::raw(c.title).alignment(if c.right {
            Alignment::Right
        } else {
            Alignment::Left
        }))
    }))
    .style(app.theme.title());
    let items = rows[range].iter().map(|r| {
        let selected = r.id == selected;
        Row::new(columns.iter().enumerate().map(|(i, c)| {
            let text = if i == 0 {
                if selected { ">" } else { " " }.to_string()
            } else {
                let value = (c.field)(r);
                if Line::raw(value).width() > c.width as usize {
                    "...".into()
                } else {
                    value.to_owned()
                }
            };
            let mut style = app.theme.text();
            if c.title == "Tm"
                && app.theme.color
                && !selected
                && let Some(color) = r.team_color
            {
                style = style.fg(color);
            }
            Cell::from(Line::styled(text, style).alignment(if c.right {
                Alignment::Right
            } else {
                Alignment::Left
            }))
        }))
        .style(if selected {
            app.theme.selected()
        } else {
            app.theme.text()
        })
    });
    let widths = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i + 1 == columns.len() {
                Constraint::Min(c.width)
            } else {
                Constraint::Length(c.width)
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Table::new(items, widths).header(header).column_spacing(1),
        table,
    );
}
