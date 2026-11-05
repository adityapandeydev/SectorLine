use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::Canvas;
use ratatui::widgets::{Block, BorderType, Borders, Cell, Clear, Gauge, Paragraph, Row, Table};
use ratatui::Frame;

use crate::app::App;
use crate::models::CarTelemetry;
use crate::stream::TelemetryStream;
use crate::track::CircuitRenderer;
mod recorded;

pub fn render(f: &mut Frame, app: &App) {
    let size = f.area();
    if let Some(replay) = &app.prepared_recording {
        recorded::render(f, app, replay);
    } else {
        let stream = &app.stream;

        // Vertical layout: Header (3) -> Main Body (Min 16) -> Cockpit Gauges (8) -> Footer (1)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(16),
                Constraint::Length(8),
                Constraint::Length(1),
            ])
            .split(size);

        render_header(f, chunks[0], stream);
        render_main_body(f, chunks[1], stream);
        render_cockpit_teleprinter(f, chunks[2], stream);
        render_footer(f, chunks[3], stream);
    }

    if app.show_session_modal {
        render_session_modal(f, app);
    }
    if let Some(message) = &app.load_message {
        let h = 3.min(size.height);
        let area = Rect::new(
            size.x,
            size.y + size.height.saturating_sub(h),
            size.width,
            h,
        );
        f.render_widget(Clear, area);
        let prefix = if app.load_id.is_some() {
            "Loading"
        } else if app.load_error {
            "Error"
        } else {
            "Status"
        };
        let message_area = Rect::new(area.x, area.y, area.width, h.saturating_sub(1));
        let help_area = Rect::new(area.x, area.y + h.saturating_sub(1), area.width, 1.min(h));
        f.render_widget(
            Paragraph::new(format!("{prefix}: {message}"))
                .wrap(ratatui::widgets::Wrap { trim: true })
                .style(Style::default().fg(if app.load_error {
                    Color::Yellow
                } else {
                    Color::White
                })),
            message_area,
        );
        f.render_widget(
            Paragraph::new("Esc cancel | r retry | s sessions | q quit")
                .style(Style::default().fg(Color::White)),
            help_area,
        );
    }
}

fn render_header(f: &mut Frame, area: Rect, stream: &TelemetryStream) {
    let current_frame = stream.current_frame();
    let sim_secs = current_frame.timestamp_ms as f64 / 1000.0;
    let mins = (sim_secs / 60.0) as u32;
    let secs = sim_secs % 60.0;

    let speed_badge = if stream.has_ended {
        Span::styled(" [DEMO / ENDED] Home restarts ",Style::default().fg(Color::White))
    }else if stream.is_paused {
        Span::styled(" [DEMO / PAUSED] ", Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(format!(" [DEMO / {}x] ", stream.playback_speed), Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD))
    };
    let title_text = " Sector Line ";

    let title_line = Line::from(vec![
        Span::styled(title_text, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(format!("- {} ", stream.session.circuit_name.to_uppercase()), Style::default().fg(Color::Rgb(180, 195, 215))),
        speed_badge,
        Span::raw(" "),
        Span::styled(" Generated telemetry ", Style::default().fg(Color::Gray)),
    ]);

    let telemetry_info = Line::from(vec![
        Span::styled(format!("LAP {} / {} ", current_frame.lap, stream.session.total_laps), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::styled(format!("TIME: {:02}:{:04.1}s ", mins, secs), Style::default().fg(Color::White)),
        Span::raw(" | "),
        Span::styled(format!("AIR: {:.1}°C ", stream.session.air_temp_c), Style::default().fg(Color::Cyan)),
        Span::styled(format!("TRACK: {:.1}°C ", stream.session.track_temp_c), Style::default().fg(Color::Red)),
    ]);

    let header_widget = Paragraph::new(vec![title_line, telemetry_info])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(70, 85, 110))),
        )
        .alignment(Alignment::Left);

    f.render_widget(header_widget, area);
}

fn render_main_body(f: &mut Frame, area: Rect, stream: &TelemetryStream) {
    // Horizontal layout: Left Timing Tower (38) -> Center Radar (Flex) -> Right Sectors (28)
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(38),
            Constraint::Min(30),
            Constraint::Length(28),
        ])
        .split(area);

    render_timing_tower(f, body_chunks[0], stream);
    render_circuit_radar(f, body_chunks[1], stream);
    render_sectors_and_traps(f, body_chunks[2], stream);
}

fn render_timing_tower(f: &mut Frame, area: Rect, stream: &TelemetryStream) {
    let frame = stream.current_frame();

    let header_cells = ["P", "Driver", "Gap Ldr", "Interval", "Tyre", "Age"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
    let header_row = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = frame
        .cars
        .iter()
        .map(|c| {
            let is_selected = c.driver == stream.selected_driver_code;
            let (tyre_badge, tyre_color) = c.compound_badge();
            let team_color = stream
                .driver_info(&c.driver)
                .map(|d| d.parsed_color())
                .unwrap_or(Color::White);

            let row_style = if is_selected {
                Style::default().bg(Color::Rgb(40, 50, 65)).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(format!("{:2}", c.position)).style(Style::default().fg(Color::White)),
                Cell::from(format!("{:3}", c.driver)).style(Style::default().fg(team_color).add_modifier(Modifier::BOLD)),
                Cell::from(c.gap_to_leader.clone()).style(Style::default().fg(Color::Rgb(200, 210, 225))),
                Cell::from(c.gap_to_ahead.clone()).style(Style::default().fg(Color::Rgb(160, 175, 195))),
                Cell::from(tyre_badge).style(Style::default().fg(tyre_color).add_modifier(Modifier::BOLD)),
                Cell::from(format!("L{:2}", c.tyre_age)).style(Style::default().fg(Color::Gray)),
            ])
            .style(row_style)
        })
        .collect();

    let tower_table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Length(9),
            Constraint::Length(4),
            Constraint::Length(4),
        ],
    )
    .header(header_row)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(70, 85, 110)))
            .title(Span::styled(" DEMO TIMING TOWER ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
    );

    f.render_widget(tower_table, area);
}

fn render_circuit_radar(f: &mut Frame, area: Rect, stream: &TelemetryStream) {
    let frame = stream.current_frame();

    let canvas = Canvas::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(70, 85, 110)))
                .title(Span::styled(" 2D CIRCUIT RADAR ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        )
        .x_bounds([0.0, 100.0])
        .y_bounds([0.0, 100.0])
        .paint(move |ctx| {
            CircuitRenderer::draw(
                ctx,
                &stream.track,
                &frame.cars,
                &stream.drivers,
                &stream.selected_driver_code,
            );
        });

    f.render_widget(canvas, area);
}

fn render_sectors_and_traps(f: &mut Frame, area: Rect, stream: &TelemetryStream) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let frame = stream.current_frame();

    // 1. Mini-Sectors Table for Top 6 Drivers
    let sector_header = Row::new(vec!["Drv", "S1", "S2", "S3"])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    let sector_rows: Vec<Row> = frame
        .cars
        .iter()
        .take(8)
        .map(|c| {
            Row::new(vec![
                Cell::from(c.driver.as_str()).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Cell::from(c.s1.as_str()).style(Style::default().fg(CarTelemetry::sector_color(&c.s1_status))),
                Cell::from(c.s2.as_str()).style(Style::default().fg(CarTelemetry::sector_color(&c.s2_status))),
                Cell::from(c.s3.as_str()).style(Style::default().fg(CarTelemetry::sector_color(&c.s3_status))),
            ])
        })
        .collect();

    let sectors_table = Table::new(
        sector_rows,
        [
            Constraint::Length(4),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(7),
        ],
    )
    .header(sector_header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(70, 85, 110)))
            .title(Span::styled(" SECTOR SPLITS ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
    );

    f.render_widget(sectors_table, chunks[0]);

    // 2. Speed Trap Radar
    let speed_lines: Vec<Line> = if !stream.session.speed_traps.is_empty() {
        stream
            .session
            .speed_traps
            .iter()
            .map(|st| {
                Line::from(vec![
                    Span::styled(format!("{}: ", st.name), Style::default().fg(Color::Gray)),
                    Span::styled(
                        format!("{:.1} km/h", st.record_kmh),
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    ),
                ])
            })
            .collect()
    } else {
        vec![
            Line::from(vec![
                Span::styled("Speed Trap: ", Style::default().fg(Color::Gray)),
                Span::styled("335.0 km/h", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("Finish Line: ", Style::default().fg(Color::Gray)),
                Span::styled("310.0 km/h", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ]),
        ]
    };

    let speed_widget = Paragraph::new(speed_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(70, 85, 110)))
            .title(Span::styled(" SPEED TRAPS ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
    );

    f.render_widget(speed_widget, chunks[1]);
}

fn render_cockpit_teleprinter(f: &mut Frame, area: Rect, stream: &TelemetryStream) {
    let car = match stream.selected_car() {
        Some(c) => c,
        None => return,
    };
    let driver_meta = stream.driver_info(&car.driver);

    let driver_name = driver_meta.map(|d| d.name.as_str()).unwrap_or("Unknown");
    let team_name = driver_meta.map(|d| d.team.as_str()).unwrap_or("F1 Team");
    let team_color = driver_meta.map(|d| d.parsed_color()).unwrap_or(Color::Cyan);

    // Horizontal split for Cockpit metrics:
    // [Driver Badge (24)] -> [Speed Gauge (22)] -> [RPM / Gear (22)] -> [Pedals (24)] -> [DRS & Telemetry (Flex)]
    let cockpit_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(25),
            Constraint::Length(24),
            Constraint::Length(24),
            Constraint::Length(26),
            Constraint::Min(20),
        ])
        .split(area);

    // 1. Driver Badge
    let (tyre_badge, tyre_color) = car.compound_badge();
    let badge_lines = vec![
        Line::from(vec![
            Span::styled(format!("#{} ", car.number), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(driver_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![Span::styled(team_name, Style::default().fg(team_color))]),
        Line::from(vec![
            Span::styled(format!("POS: P{:2} | ", car.position), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(tyre_badge, Style::default().fg(tyre_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" (L{})", car.tyre_age), Style::default().fg(Color::Gray)),
        ]),
    ];
    let badge_widget = Paragraph::new(badge_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(team_color))
            .title(Span::styled(format!(" DRIVER: {} ", car.driver), Style::default().fg(team_color).add_modifier(Modifier::BOLD))),
    );
    f.render_widget(badge_widget, cockpit_chunks[0]);

    // 2. Speedometer Gauge
    let speed_pct = (car.speed / 350.0).clamp(0.0, 1.0);
    let speed_gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(70, 85, 110)))
                .title(" SPEEDOMETER "),
        )
        .gauge_style(Style::default().fg(Color::Cyan).bg(Color::Rgb(30, 35, 45)))
        .percent((speed_pct * 100.0) as u16)
        .label(format!("{:.0} KM/H", car.speed));
    f.render_widget(speed_gauge, cockpit_chunks[1]);

    // 3. Gear & RPM
    let rpm_pct = (car.rpm as f64 / 13000.0).clamp(0.0, 1.0);
    let rpm_gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(70, 85, 110)))
                .title(format!(" GEAR: [ {} ] ", car.gear)),
        )
        .gauge_style(Style::default().fg(Color::Yellow).bg(Color::Rgb(30, 35, 45)))
        .percent((rpm_pct * 100.0) as u16)
        .label(format!("{} RPM", car.rpm));
    f.render_widget(rpm_gauge, cockpit_chunks[2]);

    // 4. Throttle & Brake Dual Bars
    let throttle_gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Green).bg(Color::Rgb(25, 30, 40)))
        .percent(car.throttle as u16)
        .label(format!("THR: {:3}%", car.throttle));

    let brake_gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Red).bg(Color::Rgb(25, 30, 40)))
        .percent(car.brake as u16)
        .label(format!("BRK: {:3}%", car.brake));

    let pedal_sub = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(cockpit_chunks[3]);

    f.render_widget(throttle_gauge, pedal_sub[0]);
    f.render_widget(brake_gauge, pedal_sub[1]);

    // 5. DRS & Status Teleprinter
    let drs_badge = if car.drs {
        Span::styled(" [DRS ACTIVE] ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" [DRS CLOSED] ", Style::default().fg(Color::Rgb(150, 160, 175)).bg(Color::Rgb(30, 35, 45)))
    };

    let status_lines = vec![
        Line::from(vec![drs_badge]),
        Line::from(vec![
            Span::styled("STATUS: ", Style::default().fg(Color::Gray)),
            Span::styled(&car.pit_status, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
    ];

    let drs_widget = Paragraph::new(status_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(70, 85, 110)))
            .title(" TELEMETRY FLAGS "),
    );
    f.render_widget(drs_widget, cockpit_chunks[4]);
}

fn render_footer(f: &mut Frame, area: Rect, _stream: &TelemetryStream) {
    let shortcuts = Line::from(vec![
        Span::styled(" [Space] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw("Pause/Play | "),
        Span::styled(" [1/2/5/0] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Speed | "),
        Span::styled(" [↑ / ↓] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw("Driver | "),
        Span::styled(" [s] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("Select Session | "),
        Span::styled(" [q] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw("Quit"),
    ]);

    let footer = Paragraph::new(shortcuts).alignment(Alignment::Center);
    f.render_widget(footer, area);
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn render_session_modal(f: &mut Frame, app: &App) {
    let area = centered_rect(75, 75, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" DEMO / SESSION CATALOGUE ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(Color::Rgb(15, 20, 30)));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let modal_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Subtitle
            Constraint::Min(8),    // List of sessions
            Constraint::Length(2), // Hotkey instructions
        ])
        .split(inner_area);

    let intro = Paragraph::new("Cached demo or recorded timing. Recorded timing uses explicit event-time estimates.")
        .style(Style::default().fg(Color::Rgb(170, 185, 205)))
        .alignment(Alignment::Center);
    f.render_widget(intro, modal_chunks[0]);

    // Build session rows
    if app.is_searching {
        let search_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .title(" SEARCH CACHED CATALOGUE ")
            .style(Style::default().bg(Color::Rgb(20, 30, 45)));

        let search_text = Line::from(vec![
            Span::styled(" Query: ", Style::default().fg(Color::Gray)),
            Span::styled(
                if app.search_input.is_empty() { "e.g. 2024 Monza Q, 2023 Spa R" } else { &app.search_input },
                if app.search_input.is_empty() { Style::default().fg(Color::DarkGray) } else { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) }
            ),
            Span::styled(" █", Style::default().fg(Color::Yellow)),
        ]);

        let help_text = Line::from(vec![
            Span::styled(" [Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Search catalogue  |  "),
            Span::styled(" [Esc] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw("Cancel Search"),
        ]);

        let p = Paragraph::new(vec![Line::raw(""), search_text, Line::raw(""), help_text])
            .block(search_block)
            .alignment(Alignment::Center);

        f.render_widget(p, modal_chunks[1]);
    } else {
        let mut rows = Vec::new();
        let visible = modal_chunks[1].height as usize;
        let start = app.selected_session_idx.saturating_sub(visible.saturating_sub(1));
        for (i, opt) in app.session_options.iter().enumerate().skip(start).take(visible) {
            let is_selected = i == app.selected_session_idx;
            let cursor = if is_selected { " ▶ " } else { "   " };

            let badge_style = match opt.badge.as_str() {
                "RACE" => Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD),
                "QUALI" => Style::default().fg(Color::White).bg(Color::Magenta).add_modifier(Modifier::BOLD),
                "SEARCH" | "CUSTOM" => Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
                _ => Style::default().fg(Color::Black).bg(Color::Cyan),
            };

            let row_style = if is_selected {
                Style::default().bg(Color::Rgb(35, 50, 75)).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(200, 210, 225))
            };

            rows.push(
                Row::new(vec![
                    Cell::from(Span::styled(cursor, if is_selected { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default() })),
                    Cell::from(Span::styled(format!(" {} ", opt.badge), badge_style)),
                    Cell::from(Span::styled(&opt.title, Style::default().add_modifier(Modifier::BOLD))),
                    Cell::from(Span::styled(&opt.subtitle, Style::default().fg(Color::Rgb(140, 155, 175)))),
                ])
                .style(row_style)
                .height(1),
            );
        }

        let session_table = Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Length(10),
                Constraint::Length(44),
                Constraint::Min(25),
            ],
        );
        f.render_widget(session_table, modal_chunks[1]);
    }

    let footer_text = if app.is_searching {
        Line::from(vec![
            Span::styled(" [Typing Query] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Type GP name & Season Year  |  "),
            Span::styled(" [Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Search catalogue  |  "),
            Span::styled(" [Esc] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw("Cancel"),
        ])
    } else {
        Line::from(vec![
            Span::styled(" [↑ / ↓ / j / k] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Navigate  |  "),
            Span::styled(" [Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Load Session  |  "),
            Span::styled(" [c] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Custom Search  |  "),
            Span::styled(" [Esc / s] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw("Dismiss"),
        ])
    };
    let footer_bar = Paragraph::new(footer_text).alignment(Alignment::Center);
    f.render_widget(footer_bar, modal_chunks[2]);
}
