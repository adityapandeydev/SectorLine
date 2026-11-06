//! Source-backed timing view with explicit completed-lap and provisional-best labels.
use crate::{
    app::App,
    domain::{DrsState, Gap},
    replay::RecordedReplay,
    replay::clock::PlaybackStatus,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    widgets::{Cell, Paragraph, Row, Table, Wrap},
};

pub(super) fn render(frame: &mut Frame, app: &App, replay: &RecordedReplay) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(5),
        Constraint::Length(if app.navigation_input.is_some() { 5 } else { 2 }),
        Constraint::Length(if app.load_message.is_some() { 3 } else { 0 }),
    ])
    .split(frame.area());
    let elapsed = replay.frame().elapsed().0;
    let utc = replay
        .timeline()
        .origin()
        .0
        .saturating_add(elapsed.min(i64::MAX as u64) as i64);
    let utc = chrono::DateTime::from_timestamp_millis(utc)
        .map(|t| t.format("%H:%M:%S%.3f UTC").to_string())
        .unwrap_or_else(|| "UTC unavailable".into());
    frame.render_widget(
        Paragraph::new(format!(
            "Sector Line | RECORDED SNAPSHOT | {} | {}\n{}\nObservation timeline +{} | {utc}",
            match replay.status() {
                PlaybackStatus::Paused => "PAUSED",
                PlaybackStatus::Playing => "PLAYING",
                PlaybackStatus::Ended => "ENDED",
            },
            replay.speed().label(),
            replay.title(),
            clock(elapsed)
        ))
        .style(Style::default().fg(Color::White)),
        areas[0],
    );
    frame.render_widget(Paragraph::new("Event-time estimates; original delivery times unknown. -- = unavailable. Reported positions may temporarily tie.").wrap(Wrap{trim:true}).style(Style::default().fg(Color::White)),areas[1]);
    let cars = replay.frame().samples();
    let selected = cars
        .iter()
        .position(|c| c.driver.number.get() == replay.selected())
        .unwrap_or(0);
    let visible = areas[2].height.saturating_sub(1) as usize;
    let start = selected.saturating_sub(visible.saturating_sub(1));
    let rows = cars.iter().skip(start).take(visible).map(|car| {
        let driver = replay
            .timeline()
            .drivers()
            .iter()
            .find(|d| d.number == car.driver.number)
            .expect("recorded roster");
        let drs = car
            .drs
            .value()
            .map(|d| match d.state() {
                DrsState::Off => "off".into(),
                DrsState::On => "on".into(),
                DrsState::Eligible => "eligible".into(),
                DrsState::Unknown => format!("?{}", d.0),
            })
            .unwrap_or_else(|| "--".into());
        let age = car
            .speed_kmh
            .timing()
            .observed_at
            .map(|t| {
                format!(
                    "{:.1}s",
                    (replay
                        .timeline()
                        .origin()
                        .0
                        .saturating_add(elapsed as i64)
                        .saturating_sub(t.0)) as f64
                        / 1000.0
                )
            })
            .unwrap_or_else(|| "--".into());
        let values = vec![
            car.position
                .value()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "--".into()),
            format!("{} #{}", driver.code, driver.number),
            gap(car.gap_to_leader.value()),
            car.speed_kmh
                .value()
                .map(|v| format!("{:.0}", v.get()))
                .unwrap_or_else(|| "--".into()),
            car.throttle
                .value()
                .map(|v| format!("{:.0}", v.get()))
                .unwrap_or_else(|| "--".into()),
            car.brake
                .value()
                .map(|b| format!("{b:?}").to_lowercase())
                .unwrap_or_else(|| "--".into()),
            drs,
            age,
        ];
        let style = if car.driver.number.get() == replay.selected() {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        Row::new(values.into_iter().map(Cell::from)).style(style)
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(3),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(7),
                Constraint::Length(6),
                Constraint::Length(9),
                Constraint::Length(9),
                Constraint::Min(6),
            ],
        )
        .header(
            Row::new([
                "Pos",
                "Driver",
                "Gap(s/lap)",
                "km/h",
                "Thr%",
                "Brake",
                "DRS",
                "Car age",
            ])
            .style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ),
        areas[2],
    );
    if let Some(car) = cars
        .iter()
        .find(|c| c.driver.number.get() == replay.selected())
    {
        let lap = car
            .lap_number
            .value()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "--".into());
        let state = car.status.value().map(|v| v.label()).unwrap_or("--");
        let eligibility = match car.lap_eligibility.value() {
            Some(crate::domain::timing::Eligibility::Eligible) => "eligible",
            Some(crate::domain::timing::Eligibility::Ineligible) => "invalid",
            _ => "unverified",
        };
        let best = |b: Option<&crate::domain::timing::BestLap>| {
            b.map(|b| {
                format!(
                    "{}{} (L{})",
                    lap_time(Some(&b.duration)),
                    if b.eligibility == crate::domain::timing::Eligibility::Unknown {
                        "*"
                    } else {
                        ""
                    },
                    b.number
                )
            })
            .unwrap_or_else(|| "--".into())
        };
        let session = replay
            .frame()
            .session_best()
            .map(|(driver, b)| format!("#{} {}", driver, best(Some(b))))
            .unwrap_or_else(|| "--".into());
        frame.render_widget(Paragraph::new(format!(
            "Selected #{} | Status {state} | Completed L{lap} [{eligibility}] {}\nS1 {} | S2 {} | S3 {} (completed lap)\nBest {} | Session {session}\n* Provisional archive best; eligibility history incomplete\nTyres, flags, weather and track view unavailable",
            car.driver.number,lap_time(car.lap_time.value()),lap_time(car.sectors[0].value()),lap_time(car.sectors[1].value()),lap_time(car.sectors[2].value()),best(car.personal_best.value())
        )).style(Style::default().fg(Color::White)),areas[3]);
    }
    frame.render_widget(
        Paragraph::new(if let Some(input) = &app.navigation_input {
            format!("Go to > {input}\ntime MM:SS.mmm | lap N (selected driver) | Enter apply | Esc cancel\noffset +/-MM:SS.mmm | recording MM:SS.mmm\nTimeline = recording + offset; zero = first core observation\n{}", app.navigation_error.as_deref().unwrap_or("Seeking pauses playback; Space resumes"))
        } else {
            format!("Space play/pause | 1/2/5/0 speed | Home restart | g go to | q quit\nLeft/Right 1s | PgUp/PgDn 60s | s sessions | Offset {:+} ms", replay.offset_ms())
        }).style(Style::default().fg(Color::White)),
        areas[4],
    );
}
fn clock(ms: u64) -> String {
    format!(
        "{:02}:{:02}.{:03}",
        ms / 60_000,
        (ms / 1000) % 60,
        ms % 1000
    )
}
fn gap(value: Option<&Gap>) -> String {
    match value {
        Some(Gap::Leader) => "leader".into(),
        Some(Gap::Seconds(v)) => format!("{:.3}", v.get()),
        Some(Gap::LapsBehind(n)) => format!("+{n}L"),
        None => "--".into(),
    }
}

fn lap_time(value: Option<&crate::domain::Seconds>) -> String {
    value
        .map(|v| clock((v.get() * 1000.0).round() as u64))
        .unwrap_or_else(|| "--".into())
}
