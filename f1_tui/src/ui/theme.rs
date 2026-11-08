//! Terminal palette only. Selection has a text marker and reverse video in any mode.
use ratatui::style::{Color, Modifier, Style};
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub color: bool,
    pub ascii: bool,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            color: std::env::var_os("NO_COLOR").is_none(),
            ascii: std::env::var_os("SECTORLINE_ASCII").is_some(),
        }
    }
}
impl Theme {
    pub fn text(self) -> Style {
        Style::default().fg(Color::Reset).bg(Color::Reset)
    }
    pub fn title(self) -> Style {
        self.text().add_modifier(Modifier::BOLD)
    }
    pub fn accent(self) -> Style {
        if self.color {
            self.title().fg(Color::Cyan)
        } else {
            self.title()
        }
    }
    pub fn selected(self) -> Style {
        self.text()
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
    }
    pub fn danger(self) -> Style {
        if self.color {
            self.title().fg(Color::Red)
        } else {
            self.title()
        }
    }
    pub fn positive(self) -> Style {
        if self.color {
            self.title().fg(Color::Green)
        } else {
            self.title()
        }
    }
    pub fn warning(self) -> Style {
        if self.color {
            self.title().fg(Color::Yellow)
        } else {
            self.title()
        }
    }
}
