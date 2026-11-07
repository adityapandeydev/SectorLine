//! Display geometry in source XY units; no inference of rank, distance or pit phase.
use super::Availability;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
impl Point {
    pub fn valid(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.x.abs() < 1e9 && self.y.abs() < 1e9
    }
    pub fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}
#[derive(Debug, Clone)]
pub struct Trace {
    points: Vec<Point>,
    max_step: f64,
}
impl Trace {
    pub fn new(points: Vec<Point>) -> Result<Self, &'static str> {
        if !(50..=10_000).contains(&points.len()) || points.iter().any(|p| !p.valid()) {
            return Err("Insufficient or invalid lap coordinates");
        }
        let (min, max) = bounds(&points)?;
        let diagonal = min.distance(max);
        if diagonal < 1.0 || (max.x - min.x).min(max.y - min.y) < diagonal * 0.02 {
            return Err("Degenerate lap extent");
        }
        let max_step = diagonal * 0.06;
        let length = points.windows(2).map(|p| p[0].distance(p[1])).sum::<f64>();
        if points.windows(2).any(|p| p[0].distance(p[1]) > max_step)
            || points[0].distance(*points.last().unwrap()) > max_step
            || !(2.0 * diagonal..=20.0 * diagonal).contains(&length)
        {
            return Err("Lap trace has jumps, incomplete closure or implausible length");
        }
        Ok(Self { points, max_step })
    }
    pub fn points(&self) -> &[Point] {
        &self.points
    }
    pub fn max_step(&self) -> f64 {
        self.max_step
    }
}
fn bounds(points: &[Point]) -> Result<(Point, Point), &'static str> {
    let first = *points.first().ok_or("No coordinates")?;
    if points.iter().any(|p| !p.valid()) {
        return Err("Invalid coordinates");
    }
    Ok(points.iter().fold((first, first), |(a, b), p| {
        (
            Point {
                x: a.x.min(p.x),
                y: a.y.min(p.y),
            },
            Point {
                x: b.x.max(p.x),
                y: b.y.max(p.y),
            },
        )
    }))
}
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    center: Point,
    scale: f64,
    sin: f64,
    cos: f64,
    width: f64,
    height: f64,
    aspect: f64,
}
impl Transform {
    /// Cell aspect is physical cell height / width (default 2), not a map stretch.
    pub fn fit(
        points: &[Point],
        width: u16,
        height: u16,
        rotation: f64,
        aspect: f64,
    ) -> Option<Self> {
        if width < 3
            || height < 3
            || !rotation.is_finite()
            || !aspect.is_finite()
            || !(1.0..=3.0).contains(&aspect)
        {
            return None;
        }
        let (sin, cos) = rotation.to_radians().sin_cos();
        let rotated = points
            .iter()
            .map(|p| Point {
                x: p.x * cos - p.y * sin,
                y: p.x * sin + p.y * cos,
            })
            .collect::<Vec<_>>();
        let (min, max) = bounds(&rotated).ok()?;
        let dx = max.x - min.x;
        let dy = max.y - min.y;
        if dx <= 0.0 || dy <= 0.0 {
            return None;
        }
        let width = f64::from(width - 1);
        let height = f64::from(height - 1);
        let scale = ((width - 2.0) / dx).min((height - 2.0) * aspect / dy);
        if scale <= 0.0 {
            return None;
        }
        Some(Self {
            center: Point {
                x: (max.x + min.x) / 2.0,
                y: (max.y + min.y) / 2.0,
            },
            scale,
            sin,
            cos,
            width,
            height,
            aspect,
        })
    }
    pub fn project(&self, p: Point) -> Option<Point> {
        if !p.valid() {
            return None;
        }
        let p = Point {
            x: (p.x * self.cos - p.y * self.sin - self.center.x) * self.scale + self.width / 2.0,
            y: (p.x * self.sin + p.y * self.cos - self.center.y) * self.scale / self.aspect
                + self.height / 2.0,
        };
        (p.x >= 0.0 && p.x <= self.width && p.y >= 0.0 && p.y <= self.height).then_some(p)
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Fix {
    pub at: i64,
    pub available: Availability,
    pub point: Option<Point>,
    pub ambiguous: bool,
}
impl Fix {
    pub fn admitted(self, now: i64) -> bool {
        matches!(self.available, Availability::Known(t)|Availability::Estimated(t) if t.0<=now)
            && self.at <= now
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Observed,
    Held,
    Interpolated,
}
#[derive(Debug, Clone, Copy)]
pub struct MapPosition {
    pub point: Point,
    pub motion: Motion,
    pub age_ms: i64,
}
pub const MAP_DELAY_MS: i64 = 500;
pub const MAX_GAP_MS: i64 = 750;
/// Both endpoints must already be admitted; callers exclude whole pit/unknown laps.
pub fn sample(
    a: Fix,
    b: Option<Fix>,
    target: i64,
    now: i64,
    pit_free: bool,
    max_step: f64,
) -> Option<MapPosition> {
    let point = a.point.filter(|p| p.valid())?;
    if a.ambiguous || !a.admitted(now) || a.at > target || now.saturating_sub(a.at) > 2000 {
        return None;
    }
    if let Some(b) = b
        && let Some(next) = b.point.filter(|p| p.valid())
        && b.admitted(now)
        && !b.ambiguous
        && pit_free
        && b.at > a.at
        && b.at - a.at <= MAX_GAP_MS
        && target < b.at
        && point.distance(next) <= max_step
    {
        let t = (target - a.at) as f64 / (b.at - a.at) as f64;
        return Some(MapPosition {
            point: Point {
                x: point.x + (next.x - point.x) * t,
                y: point.y + (next.y - point.y) * t,
            },
            motion: Motion::Interpolated,
            age_ms: now - a.at,
        });
    }
    Some(MapPosition {
        point,
        motion: if target == a.at {
            Motion::Observed
        } else {
            Motion::Held
        },
        age_ms: now - a.at,
    })
}
