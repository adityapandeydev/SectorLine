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
    frame.render_widget(
        Paragraph::new("Ty S/M/H/I/W | L=completed | ~stale").style(app.theme.text()),
        legend,
    );
    let wide = area.width >= 80;
    let column = |title, width, right, field| Column {
        title,
        width,
        right,
        field,
    };
    let mut columns = vec![
        column("", 1, false, |_| ""),
        column("Pos", 3, true, |r| &r.position),
        column("Driver", if wide { 12 } else { 8 }, false, |r| &r.driver),
        column("Tm", 3, false, |r| &r.team),
        column("Gap(s)", 7, true, |r| &r.gap),
    ];
    if wide {
        columns.push(column("Int(s)", 8, true, |r| &r.interval));
    }
    columns.extend([
        column("Ty", 3, false, |r| &r.tyre),
        column("Age", 3, true, |r| &r.tyre_age),
        column("Lap", 3, true, |r| &r.lap),
    ]);
    if wide {
        columns.extend([
            column("Last(s)", 9, true, |r| &r.last),
            column("km/h", 6, true, |r| &r.speed),
            column("AgeS", 5, true, |r| &r.sample_age),
        ]);
    }
    columns.push(column("State", 5, false, |r| &r.status));
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
