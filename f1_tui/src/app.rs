use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;

use crate::stream::TelemetryStream;
use crate::ui;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionOption {
    pub title: String,
    pub gp_key: String,
    pub year: u32,
    pub session_type: String,
    pub badge: String,
    pub subtitle: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppAction {
    None,
    Quit,
    SwitchSession(SessionOption),
}

pub struct App {
    pub stream: TelemetryStream,
    pub should_quit: bool,
    pub show_session_modal: bool,
    pub selected_session_idx: usize,
    pub session_options: Vec<SessionOption>,
    pub is_searching: bool,
    pub search_input: String,
}

impl App {
    pub fn new(stream: TelemetryStream, start_with_modal: bool) -> Self {
        let session_options = vec![
            SessionOption {
                title: "🇦🇿 2026 Azerbaijan Grand Prix (Baku)".to_string(),
                gp_key: "Baku".to_string(),
                year: 2026,
                session_type: "R".to_string(),
                badge: "DEMO".to_string(),
                subtitle: "Generated demo using historical session metadata; not recorded replay".to_string(),
            },
            SessionOption {
                title: "🇦🇿 2024 Azerbaijan Grand Prix (Baku)".to_string(),
                gp_key: "Baku".to_string(),
                year: 2024,
                session_type: "R".to_string(),
                badge: "DEMO".to_string(),
                subtitle: "Generated demo using historical session metadata; not recorded replay".to_string(),
            },
            SessionOption {
                title: "🇮🇹 2024 Italian Grand Prix (Monza)".to_string(),
                gp_key: "Monza".to_string(),
                year: 2024,
                session_type: "R".to_string(),
                badge: "DEMO".to_string(),
                subtitle: "Generated demo using historical session metadata; not recorded replay".to_string(),
            },
            SessionOption {
                title: "🇧🇪 2024 Belgian Grand Prix (Spa)".to_string(),
                gp_key: "Spa".to_string(),
                year: 2024,
                session_type: "R".to_string(),
                badge: "DEMO".to_string(),
                subtitle: "Generated demo using historical session metadata; not recorded replay".to_string(),
            },
            SessionOption {
                title: "🇬🇧 2024 British Grand Prix (Silverstone)".to_string(),
                gp_key: "Silverstone".to_string(),
                year: 2024,
                session_type: "R".to_string(),
                badge: "DEMO".to_string(),
                subtitle: "Generated demo using historical session metadata; not recorded replay".to_string(),
            },
            SessionOption {
                title: "🇦🇪 2023 Abu Dhabi Grand Prix (Yas Marina)".to_string(),
                gp_key: "Abu Dhabi".to_string(),
                year: 2023,
                session_type: "R".to_string(),
                badge: "DEMO".to_string(),
                subtitle: "Generated demo using historical session metadata; not recorded replay".to_string(),
            },
            SessionOption {
                title: "🇦🇪 2021 Abu Dhabi Grand Prix (Yas Marina)".to_string(),
                gp_key: "Abu Dhabi".to_string(),
                year: 2021,
                session_type: "R".to_string(),
                badge: "DEMO".to_string(),
                subtitle: "Generated demo using historical session metadata; not recorded replay".to_string(),
            },
            SessionOption {
                title: "🔍 SEARCH ANY GRAND PRIX / YEAR".to_string(),
                gp_key: "SEARCH".to_string(),
                year: 2026,
                session_type: "R".to_string(),
                badge: "SEARCH".to_string(),
                subtitle: "Choose historical metadata for a generated demo (Python required)".to_string(),
            },
        ];

        Self {
            stream,
            should_quit: false,
            show_session_modal: start_with_modal,
            selected_session_idx: 0, // Default to first demo
            session_options,
            is_searching: false,
            search_input: String::new(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> AppAction {
        if self.show_session_modal {
            if self.is_searching {
                match key.code {
                    KeyCode::Esc => {
                        self.is_searching = false;
                        self.search_input.clear();
                        AppAction::None
                    }
                    KeyCode::Backspace => {
                        self.search_input.pop();
                        AppAction::None
                    }
                    KeyCode::Char(c) => {
                        self.search_input.push(c);
                        AppAction::None
                    }
                    KeyCode::Enter => {
                        let query = self.search_input.trim();
                        if query.is_empty() {
                            self.is_searching = false;
                            return AppAction::None;
                        }

                        // Parse year from query (e.g. "Silverstone 2022" -> year 2022, gp "Silverstone")
                        let mut parsed_year = 2026;
                        let mut words = Vec::new();
                        for part in query.split_whitespace() {
                            if let Ok(y) = part.parse::<u32>() {
                                if (1950..=2030).contains(&y) {
                                    parsed_year = y;
                                    continue;
                                }
                            }
                            words.push(part);
                        }

                        let clean_gp = if words.is_empty() {
                            "Baku".to_string()
                        } else {
                            words.join(" ")
                        };

                        let custom_opt = SessionOption {
                            title: format!("🏁 {} {} (Custom Query)", parsed_year, clean_gp),
                            gp_key: clean_gp.clone(),
                            year: parsed_year,
                            session_type: "R".to_string(),
                            badge: "CUSTOM".to_string(),
                            subtitle: format!("Authentic FIA telemetry for {} {}", parsed_year, clean_gp),
                        };

                        self.is_searching = false;
                        self.show_session_modal = false;
                        AppAction::SwitchSession(custom_opt)
                    }
                    _ => AppAction::None,
                }
            } else {
                match key.code {
                    KeyCode::Esc => {
                        self.show_session_modal = false;
                        AppAction::None
                    }
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        self.show_session_modal = false;
                        AppAction::None
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Char('/') => {
                        self.is_searching = true;
                        self.search_input.clear();
                        AppAction::None
                    }
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        self.should_quit = true;
                        AppAction::Quit
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.selected_session_idx > 0 {
                            self.selected_session_idx -= 1;
                        }
                        AppAction::None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if self.selected_session_idx + 1 < self.session_options.len() {
                            self.selected_session_idx += 1;
                        }
                        AppAction::None
                    }
                    KeyCode::Enter => {
                        if self.session_options[self.selected_session_idx].badge == "SEARCH" {
                            self.is_searching = true;
                            self.search_input.clear();
                            AppAction::None
                        } else {
                            let opt = self.session_options[self.selected_session_idx].clone();
                            self.show_session_modal = false;
                            AppAction::SwitchSession(opt)
                        }
                    }
                    _ => AppAction::None,
                }
            }
        } else {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    self.should_quit = true;
                    AppAction::Quit
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    self.show_session_modal = true;
                    AppAction::None
                }
                KeyCode::Char(' ') => {
                    self.stream.toggle_pause();
                    AppAction::None
                }
                KeyCode::Char('1') => {
                    self.stream.set_speed(1.0);
                    AppAction::None
                }
                KeyCode::Char('2') => {
                    self.stream.set_speed(2.0);
                    AppAction::None
                }
                KeyCode::Char('5') => {
                    self.stream.set_speed(5.0);
                    AppAction::None
                }
                KeyCode::Char('0') => {
                    self.stream.set_speed(10.0);
                    AppAction::None
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.stream.select_prev_driver();
                    AppAction::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.stream.select_next_driver();
                    AppAction::None
                }
                _ => AppAction::None,
            }
        }
    }

    pub fn tick(&mut self, dt_ms: f64) {
        if !self.show_session_modal {
            self.stream.tick(dt_ms);
        }
    }

    pub fn render(&self, f: &mut Frame) {
        ui::render(f, self);
    }
}
