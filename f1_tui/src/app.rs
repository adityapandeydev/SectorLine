use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;

use crate::data::cache::SessionCache;
use crate::loader::{LoaderEvent, session::SessionResult};
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
    pub recording_key: Option<u32>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppAction {
    None,
    Quit,
    SwitchSession(SessionOption),
    SearchCatalogue(String),
    CancelLoad,
    RetryLoad,
}

pub struct App {
    pub stream: TelemetryStream,
    pub should_quit: bool,
    pub show_session_modal: bool,
    pub selected_session_idx: usize,
    pub session_options: Vec<SessionOption>,
    pub is_searching: bool,
    pub search_input: String,
    pub load_id: Option<u64>,
    pub load_message: Option<String>,
    pub load_error: bool,
    pub prepared_recording: Option<SessionCache>,
}

impl App {
    pub fn new(stream: TelemetryStream, start_with_modal: bool) -> Self {
        let session_options = vec![cached_demo_option(&stream), search_option()];

        Self {
            stream,
            should_quit: false,
            show_session_modal: start_with_modal,
            selected_session_idx: 0, // Default to first demo
            session_options,
            is_searching: false,
            search_input: String::new(),
            load_id: None,
            load_message: None,
            load_error: false,
            prepared_recording: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> AppAction {
        if key.code == KeyCode::Char('c')
            && key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL)
        {
            self.should_quit = true;
            return AppAction::Quit;
        }
        if key.code == KeyCode::Esc && self.load_id.is_some() {
            self.load_id = None;
            self.load_error = false;
            self.load_message = Some("Loading cancelled; previous session retained".into());
            return AppAction::CancelLoad;
        }
        if key.code == KeyCode::Char('r') && !self.is_searching {
            return AppAction::RetryLoad;
        }
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
                        if !c.is_control() && self.search_input.chars().count() < 128 {
                            self.search_input.push(c);
                        }
                        AppAction::None
                    }
                    KeyCode::Enter => {
                        let query = self.search_input.trim();
                        if query.is_empty() {
                            self.is_searching = false;
                            return AppAction::None;
                        }

                        let query = query.to_owned();
                        self.is_searching = false;
                        AppAction::SearchCatalogue(query)
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
                        let Some(option) = self.session_options.get(self.selected_session_idx)
                        else {
                            return AppAction::None;
                        };
                        if let Some(reason) = &option.unavailable_reason {
                            self.load_message = Some(reason.clone());
                            self.load_error = true;
                            return AppAction::None;
                        }
                        if option.badge == "SEARCH" {
                            self.is_searching = true;
                            self.search_input.clear();
                            AppAction::None
                        } else {
                            let opt = option.clone();
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

    pub fn begin_loading(&mut self, id: u64) {
        self.load_id = Some(id);
        self.load_error = false;
        self.load_message = Some("Loading session; Esc cancels, s opens sessions, q quits".into());
    }
    pub fn accept_loader_event(&mut self, event: LoaderEvent<SessionResult>) {
        let id = match &event {
            LoaderEvent::Progress { id, .. } | LoaderEvent::Finished { id, .. } => *id,
        };
        if self.load_id != Some(id) {
            return;
        }
        match event {
            LoaderEvent::Progress { message, .. } => self.load_message = Some(message),
            LoaderEvent::Finished { result, .. } => {
                self.load_id = None;
                match result {
                    Ok(SessionResult::Demo(stream)) => {
                        self.stream = *stream;
                        self.load_message = Some("Generated demo ready".into());
                        self.load_error = false;
                    }
                    Ok(SessionResult::Search(mut options)) => {
                        let count = options.len();
                        self.session_options = vec![cached_demo_option(&self.stream)];
                        self.session_options.append(&mut options);
                        self.session_options.push(search_option());
                        self.selected_session_idx = 1;
                        self.show_session_modal = true;
                        self.load_message = Some(format!(
                            "{count} catalogue matches. Enter prepares a recording; current display remains DEMO"
                        ));
                    }
                    Ok(SessionResult::Recording(cache)) => {
                        self.prepared_recording = Some(cache);
                        self.load_message = Some("Recording cached and verified; recorded playback unavailable. Current display remains DEMO".into());
                        self.load_error = false;
                    }
                    Err(error) => {
                        self.load_error = true;
                        self.load_message = Some(format!("{error}. Previous session retained"));
                    }
                }
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

fn cached_demo_option(stream: &TelemetryStream) -> SessionOption {
    SessionOption {
        title: stream.session.event_name.clone(),
        gp_key: "CACHED_DEMO".into(),
        year: 0,
        session_type: stream.session.session_type.clone(),
        badge: "DEMO".into(),
        subtitle: "Generated cached demo; not recorded replay".into(),
        recording_key: None,
        unavailable_reason: None,
    }
}
fn search_option() -> SessionOption {
    SessionOption {
        title: "Search cached session catalogue".into(),
        gp_key: "SEARCH".into(),
        year: 0,
        session_type: String::new(),
        badge: "SEARCH".into(),
        subtitle: "Use a year, meeting and type; e.g. 2024 Monza Q".into(),
        recording_key: None,
        unavailable_reason: None,
    }
}
