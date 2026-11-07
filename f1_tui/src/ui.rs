//! Stable eight-row chrome shared by generated and recorded playback.
use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};
mod demo;
pub mod layout;
mod recorded;
mod shell;
pub mod theme;
mod timing;
mod track;
pub mod view_model;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Timing,
    Driver,
    Conditions,
    Track,
}
impl View {
    pub fn next(self, back: bool) -> Self {
        let index = match self {
            Self::Timing => 0,
            Self::Driver => 1,
            Self::Conditions => 2,
            Self::Track => 3,
        };
        [Self::Timing, Self::Driver, Self::Conditions, Self::Track]
            [(index + if back { 3 } else { 1 }) % 4]
    }
}
pub fn render(frame: &mut Frame, app: &App) {
    let regions = layout::Regions::new(frame.area());
    frame.render_widget(Block::default().style(app.theme.text()), frame.area());
    if regions.class == layout::SizeClass::Small {
        frame.render_widget(
            Paragraph::new(
                "Sector Line\nResize to at least 80x24 cells.\nq quit | Ctrl+C always quits",
            )
            .style(app.theme.text()),
            frame.area(),
        );
        return;
    }
    shell::render(frame, app, &regions);
    if let Some(replay) = &app.prepared_recording {
        recorded::render(frame, app, replay, &regions);
    } else {
        demo::render(frame, app, &regions);
    }
    if app.show_session_modal {
        session_modal(frame, app, regions.body);
    }
    if app.navigation_input.is_some() {
        shell::navigation(frame, app, regions.body);
    }
    if app.show_help {
        shell::help(frame, app, regions.body);
    }
}
fn session_modal(frame: &mut Frame, app: &App, area: Rect) {
    let area = Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(4),
        area.height,
    );
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Sessions ")
        .style(app.theme.text());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [intro, list, selection, help] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new("Cached demo or recorded timing.\nSearch by year, event and session type.")
            .style(app.theme.text()),
        intro,
    );
    if app.is_searching {
        let max = list.width.saturating_sub(10) as usize;
        let text = app
            .search_input
            .chars()
            .rev()
            .take(max)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        frame.render_widget(
            Paragraph::new(format!(
                "Search > {text}_\n\nExample: 2024 Monza Race\nEnter search | Esc back"
            ))
            .style(app.theme.accent()),
            list,
        );
    } else {
        let start = app
            .selected_session_idx
            .saturating_sub(list.height.saturating_sub(1) as usize);
        let rows = app
            .session_options
            .iter()
            .enumerate()
            .skip(start)
            .take(list.height as usize)
            .map(|(i, opt)| {
                Row::new(vec![
                    Cell::from(if i == app.selected_session_idx {
                        ">"
                    } else {
                        " "
                    }),
                    Cell::from(opt.badge.clone()),
                    Cell::from(opt.title.clone()),
                ])
                .style(if i == app.selected_session_idx {
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
                    Constraint::Length(9),
                    Constraint::Min(0),
                ],
            )
            .column_spacing(1),
            list,
        );
        if let Some(opt) = app.session_options.get(app.selected_session_idx) {
            frame.render_widget(
                Paragraph::new(format!(
                    "{}\n{}",
                    opt.subtitle,
                    opt.unavailable_reason
                        .as_deref()
                        .unwrap_or("Enter opens selected session")
                ))
                .wrap(Wrap { trim: true })
                .style(app.theme.text()),
                selection,
            );
        }
    }
    frame.render_widget(
        Paragraph::new(if app.is_searching {
            "Enter search | Esc back | Ctrl+C quit"
        } else {
            "Up/Down select | Enter open | c search | Esc close | q quit"
        })
        .style(app.theme.title()),
        help,
    );
}
pub(super) fn panel(frame: &mut Frame, area: Rect, title: &str, theme: theme::Theme) -> Rect {
    let [heading, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    frame.render_widget(
        Paragraph::new(title.to_owned()).style(theme.title()),
        heading,
    );
    body
}

/// Ratatui's measured wrapped line count keeps overlays from scrolling into blank space.
pub(super) fn scroll<'a>(paragraph: Paragraph<'a>, area: Rect, offset: u16) -> Paragraph<'a> {
    let max = paragraph
        .line_count(area.width)
        .saturating_sub(area.height as usize)
        .min(u16::MAX as usize) as u16;
    paragraph.scroll((offset.min(max), 0))
}
