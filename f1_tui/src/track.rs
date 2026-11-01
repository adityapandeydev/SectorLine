use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::canvas::{Context, Line};

use crate::models::{CarTelemetry, Driver, TrackLayout};

pub struct CircuitRenderer;

impl CircuitRenderer {
    /// Renders the complete Silverstone circuit layout and live car positions on the Ratatui canvas context.
    pub fn draw(
        ctx: &mut Context,
        track: &TrackLayout,
        cars: &[CarTelemetry],
        drivers: &[Driver],
        selected_code: &str,
    ) {
        let pts = &track.points;
        if pts.len() < 2 {
            return;
        }

        // 1. Draw circuit track layout lines
        let track_color = Color::Rgb(80, 95, 115); // Sleek asphalt slate
        for i in 0..pts.len() {
            let next_idx = (i + 1) % pts.len();
            let p1 = &pts[i];
            let p2 = &pts[next_idx];
            ctx.draw(&Line {
                x1: p1.x,
                y1: p1.y,
                x2: p2.x,
                y2: p2.y,
                color: track_color,
            });
        }

        // 2. Draw Key Corner Labels
        for corner in &track.corners {
            let (cx, cy) = match (corner.x, corner.y) {
                (Some(x), Some(y)) => (x, y),
                _ => {
                    let idx = ((corner.distance_pct * pts.len() as f64) as usize) % pts.len();
                    (pts[idx].x, pts[idx].y)
                }
            };

            // Display key corners cleanly (skip duplicates or intermediate apexes)
            let is_key_turn = match corner.name.as_str() {
                "Abbey" | "Village" | "Brooklands" | "Copse" | "Maggotts" | "Becketts" | "Stowe" | "Club" => true,
                "Prima Variante" | "Curva Grande" | "Roggia" | "Lesmo 1" | "Ascari Entry" | "Parabolica" => true,
                "La Source" | "Eau Rouge" | "Raidillon" | "Pouhon" | "Blanchimont" | "Bus Stop Chicane" => true,
                "Sainte Devote" | "Casino" | "Grand Hotel Hairpin" | "Tunnel" | "Tabac" | "Rascasse" => true,
                "Castle Entry" | "Castle Exit" | "Caspian Straight" => true,
                _ => corner.number == 1 || corner.number == 15,
            };

            if is_key_turn {
                let short_name = match corner.name.as_str() {
                    "Becketts Entry" | "Becketts Apex" => "Becketts",
                    "Castle Entry" | "Old City Castle" | "Castle Apex" => "Castle",
                    "Caspian Straight" => "Straight",
                    n if n.starts_with("Turn ") => "",
                    n => n,
                };

                let label = if !short_name.is_empty() {
                    format!("T{} {}", corner.number, short_name)
                } else {
                    format!("T{}", corner.number)
                };

                ctx.print(
                    cx + 1.2,
                    cy - 0.4,
                    ratatui::text::Span::styled(
                        label,
                        Style::default().fg(Color::Rgb(140, 155, 175)).add_modifier(Modifier::DIM),
                    ),
                );
            }
        }

        // 3. Draw Start / Finish Line
        if let Some(start_pt) = pts.first() {
            ctx.print(
                start_pt.x - 3.0,
                start_pt.y + 1.5,
                ratatui::text::Span::styled("S/F", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            );
        }

        // 4. Draw Live Car Positions
        for car in cars {
            let is_selected = car.driver == selected_code;
            let team_color = drivers
                .iter()
                .find(|d| d.code == car.driver)
                .map(|d| d.parsed_color())
                .unwrap_or(Color::Cyan);

            if is_selected {
                // Highlight target with marker brackets
                ctx.print(
                    car.x - 1.2,
                    car.y,
                    ratatui::text::Span::styled(
                        format!("[{}]", car.driver),
                        Style::default()
                            .fg(Color::Yellow)
                            .bg(Color::Rgb(30, 30, 40))
                            .add_modifier(Modifier::BOLD),
                    ),
                );
            } else {
                // Team colored car dot with driver number
                ctx.print(
                    car.x - 0.5,
                    car.y,
                    ratatui::text::Span::styled(
                        format!("{}", car.number),
                        Style::default().fg(team_color).add_modifier(Modifier::BOLD),
                    ),
                );
            }
        }
    }
}
