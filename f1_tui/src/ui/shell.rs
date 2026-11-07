use super::*;
use crate::replay::clock::PlaybackStatus;
pub(super) fn clock(ms: u64) -> String {
    format!(
        "{:02}:{:02}.{:03}",
        ms / 60_000,
        (ms / 1000) % 60,
        ms % 1000
    )
}
fn timing(app: &App) -> (u64, u64, String, String) {
    if let Some(r) = &app.prepared_recording {
        (
            r.frame().elapsed().0,
            r.timeline().end().0,
            match r.status() {
                PlaybackStatus::Paused => "PAUSED",
                PlaybackStatus::Playing => "PLAYING",
                PlaybackStatus::Ended => "ENDED",
            }
            .into(),
            r.speed().label().into(),
        )
    } else {
        let s = &app.stream;
        let start = s.frames.first().map(|f| f.timestamp_ms).unwrap_or(0);
        (
            s.current_frame().timestamp_ms.saturating_sub(start),
            s.frames
                .last()
                .map(|f| f.timestamp_ms.saturating_sub(start))
                .unwrap_or(0),
            if s.has_ended {
                "ENDED"
            } else if s.is_paused {
                "PAUSED"
            } else {
                "PLAYING"
            }
            .into(),
            format!("{}x", s.playback_speed),
        )
    }
}
pub(super) fn render(frame: &mut Frame, app: &App, r: &layout::Regions) {
    let (position, end, state, speed) = timing(app);
    let [first, identity] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(r.header);
    let [brand, title, state_area] = Layout::horizontal([
        Constraint::Length(14),
        Constraint::Min(0),
        Constraint::Length(22),
    ])
    .areas(first);
    frame.render_widget(
        Paragraph::new("Sector Line").style(app.theme.accent()),
        brand,
    );
    frame.render_widget(
        Paragraph::new(
            app.prepared_recording
                .as_ref()
                .map(|r| r.title().to_owned())
                .unwrap_or_else(|| app.stream.session.event_name.clone()),
        )
        .style(app.theme.title()),
        title,
    );
    frame.render_widget(
        Paragraph::new(format!("{state:>10}  {speed:>5}")).style(app.theme.title()),
        state_area,
    );
    let identity_text = if let Some(replay) = &app.prepared_recording {
        format!(
            "OpenF1 #{} | RECORDED SNAPSHOT | event-time estimates | -- missing / ~ stale",
            replay.timeline().session().key()
        )
    } else {
        format!(
            "DEMO | Generated telemetry | {} / {}",
            app.stream.session.circuit_name, app.stream.session.session_type
        )
    };
    frame.render_widget(
        Paragraph::new(identity_text).style(app.theme.text()),
        identity,
    );
    let titles = [
        (View::Timing, "Timing"),
        (View::Driver, "Driver"),
        (View::Conditions, "Conditions"),
        (View::Track, "Track"),
    ]
    .map(|(view, label)| {
        Span::styled(
            if view == app.view {
                format!(" [{label}] ")
            } else {
                format!("  {label}  ")
            },
            if view == app.view {
                app.theme.selected()
            } else {
                app.theme.text()
            },
        )
    });
    frame.render_widget(Paragraph::new(Line::from(titles.to_vec())), r.tabs);
    let [time, bar, meta] = Layout::vertical([Constraint::Length(1); 3]).areas(r.timeline);
    frame.render_widget(
        Paragraph::new(format!(
            "+{} / {} | {state} | {speed} | Home restart",
            clock(position),
            clock(end)
        ))
        .style(app.theme.title()),
        time,
    );
    let width = bar.width as usize;
    let filled = if end == 0 {
        0
    } else {
        ((position.min(end) as u128 * width as u128) / end as u128) as usize
    };
    let progress = format!(
        "{}{}",
        "=".repeat(filled),
        "-".repeat(width.saturating_sub(filled))
    );
    frame.render_widget(Paragraph::new(progress).style(app.theme.accent()), bar);
    let meta_text = if let Some(replay) = &app.prepared_recording {
        let utc = chrono::DateTime::from_timestamp_millis(
            replay.timeline().origin().0.saturating_add(position as i64),
        )
        .map(|v| v.format("%H:%M:%S%.3f UTC").to_string())
        .unwrap_or_else(|| "UTC --".into());
        format!("Observation timeline | {utc} | Left/Right 1s | PgUp/PgDn 60s")
    } else {
        "Generated timeline | Simulated data and movement".into()
    };
    frame.render_widget(Paragraph::new(meta_text).style(app.theme.text()), meta);
    let message = app
        .load_message
        .as_ref()
        .map(|message| {
            format!(
                "{}: {message}",
                if app.load_id.is_some() {
                    "Loading"
                } else if app.load_error {
                    "Error"
                } else {
                    "Status"
                }
            )
        })
        .unwrap_or_else(|| {
            if app.prepared_recording.is_some() {
                "OpenF1 estimates | original delivery times unknown | -- missing / ~ stale".into()
            } else {
                "Generated demo | Values are simulated, not observed race data".into()
            }
        });
    frame.render_widget(
        Paragraph::new(message).style(if app.load_error {
            app.theme.warning()
        } else {
            app.theme.text()
        }),
        r.status,
    );
    let help = if app.navigation_input.is_some() {
        "Enter apply | Esc cancel | Ctrl+C quit"
    } else if app.show_help {
        "Up/Down scroll | Esc close help | q quit"
    } else if app.show_session_modal {
        "Enter open/search | Esc close | ? help | q quit"
    } else if app.load_error {
        "Space play/pause | r retry | s sessions | ? full error | q quit"
    } else if app.load_id.is_some() {
        "Esc cancel | r retry | s sessions | ? full status | q quit"
    } else if app.view == View::Track {
        "Space play/pause | ,/. rotate | -/+ cell ratio | Tab views | ? help | q quit"
    } else if app.view != View::Timing {
        "Space play/pause | Tab views | [/] scroll | s sessions | ? help | q quit"
    } else {
        "Space play/pause | Tab views | g go to | s sessions | ? help | q quit"
    };
    frame.render_widget(Paragraph::new(help).style(app.theme.title()), r.help);
}
fn overlay(frame: &mut Frame, app: &App, area: Rect, title: &str) -> Rect {
    let area = Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(4),
        area.height,
    );
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title.to_owned())
        .style(app.theme.text());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}
pub(super) fn navigation(frame: &mut Frame, app: &App, area: Rect) {
    let inner = overlay(frame, app, area, " Go to ");
    let text = format!(
        "Go to > {}_\n\ntime MM:SS.mmm | lap N (selected driver)\noffset +/-MM:SS.mmm | recording MM:SS.mmm\n\nTimeline = recording + offset\nZero = first core observation; seeks pause playback\n\n{}",
        app.navigation_input.as_deref().unwrap_or(""),
        app.navigation_error
            .as_deref()
            .unwrap_or("Enter apply | Esc cancel")
    );
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .style(if app.navigation_error.is_some() {
                app.theme.warning()
            } else {
                app.theme.text()
            }),
        inner,
    );
}
pub(super) fn help(frame: &mut Frame, app: &App, area: Rect) {
    let inner = overlay(frame, app, area, " Help / full status ");
    let text = format!(
        "Playback\nSpace play/pause | Home restart paused\n1/2/5/0 speed: 1x / 2x / 5x / 10x\nLeft/Right seek 1s | PgUp/PgDn seek 60s (recorded)\n\nViews\nTab / Shift+Tab switch views | Up/Down or j/k select driver\n[ / ] scroll Driver or Conditions\ng go to time/lap/recording offset (recorded)\ns sessions | r retry last load | Esc cancel load/back\nq quit | Ctrl+C always quits\n\nSource and quality\n-- unavailable | ~ stale held value | ... column overflow | * provisional best\n\nTiming columns\nGap/Int in seconds; +nL means laps behind\nTy S/M/H/I/W soft/medium/hard/intermediate/wet; ? unrecognized\nAge tyre laps | Lap completed laps | Last seconds\nkm/h speed | AgeS telemetry observation age in seconds\nTm source team mnemonic; full name in Driver and Team key\nState RET/DNF/DNS/DSQ only when evidenced\nRecorded: event-time estimates; original delivery times unknown\nDemo: Generated simulation; no observed replay\nTrack: static observed lap trace from this recording; approximate XY\nCars use a 500ms display delay; interpolation endpoints already available\n~ held sample; missing or >2s old locations are hidden\nPit/unknown laps and gaps/jumps never interpolate\nTrack view: ,/. rotate 15deg; -/+ adjust cell height/width (default2)\n\nFull status\n{}\n\nEsc closes help; playback stays paused. Up/Down scroll.",
        app.load_message.as_deref().unwrap_or("Ready")
    );
    frame.render_widget(
        scroll(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .style(app.theme.text()),
            inner,
            app.help_scroll,
        ),
        inner,
    );
}
