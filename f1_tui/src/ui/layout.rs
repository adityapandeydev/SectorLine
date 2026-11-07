//! Layout dimensions are terminal cells. Chrome is always eight rows.
use ratatui::layout::{Constraint, Layout, Rect};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeClass {
    Wide,
    Standard,
    Compact,
    Small,
}
pub struct Regions {
    pub class: SizeClass,
    pub header: Rect,
    pub tabs: Rect,
    pub body: Rect,
    pub timeline: Rect,
    pub status: Rect,
    pub help: Rect,
}
impl Regions {
    pub fn new(area: Rect) -> Self {
        let class = if area.width < 80 || area.height < 24 {
            SizeClass::Small
        } else if area.width >= 144 && area.height >= 40 {
            SizeClass::Wide
        } else if area.width >= 120 && area.height >= 32 {
            SizeClass::Standard
        } else {
            SizeClass::Compact
        };
        let [header, tabs, body, timeline, status, help] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        Self {
            class,
            header,
            tabs,
            body,
            timeline,
            status,
            help,
        }
    }
    pub fn timing_columns(&self) -> (Rect, Option<Rect>, Option<Rect>) {
        match self.class {
            SizeClass::Wide => {
                let [timing, _, context, _, detail] = Layout::horizontal([
                    Constraint::Length(44),
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(1),
                    Constraint::Length(32),
                ])
                .areas(self.body);
                (timing, Some(context), Some(detail))
            }
            SizeClass::Standard => {
                let [timing, _, context] = Layout::horizontal([
                    Constraint::Length(44),
                    Constraint::Length(1),
                    Constraint::Min(0),
                ])
                .areas(self.body);
                (timing, Some(context), None)
            }
            _ => (self.body, None, None),
        }
    }
}
