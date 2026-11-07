use sectorline::domain::Availability;
use sectorline::domain::UtcMillis;
use sectorline::domain::geometry::{Fix, Motion, Point, Trace, Transform, sample};
fn ring() -> Vec<Point> {
    (0..=240)
        .map(|n| {
            let a = n as f64 * std::f64::consts::TAU / 240.0;
            Point {
                x: 1000.0 * a.cos(),
                y: 600.0 * a.sin(),
            }
        })
        .collect()
}
#[test]
fn geometry_preserves_rotation_scale_and_terminal_cell_aspect() {
    let points = ring();
    for rotation in [0.0, 37.0, 90.0, 258.0] {
        let fit = Transform::fit(&points, 80, 20, rotation, 2.0).unwrap();
        let center = fit.project(Point { x: 0.0, y: 0.0 }).unwrap();
        assert!((center.x - 39.5).abs() < 0.01 && (center.y - 9.5).abs() < 0.01);
        assert!(points.iter().all(|p| fit.project(*p).is_some()));
    }
    let fit = Transform::fit(&points, 80, 30, 0.0, 2.0).unwrap();
    let p = fit.project(Point { x: 100.0, y: 100.0 }).unwrap();
    let zero = fit.project(Point { x: 0.0, y: 0.0 }).unwrap();
    assert!(((p.x - zero.x) / (p.y - zero.y) - 2.0).abs() < 1e-9);
    let rotated = Transform::fit(&points, 80, 30, 90.0, 2.0).unwrap();
    let x = rotated.project(Point { x: 100.0, y: 0.0 }).unwrap();
    let zero = rotated.project(Point { x: 0.0, y: 0.0 }).unwrap();
    assert!((x.x - zero.x).abs() < 1e-9 && x.y > zero.y);
    assert!(Transform::fit(&points, 0, 20, 0.0, 2.0).is_none());
    assert!(Transform::fit(&points, 80, 20, 0.0, 0.0).is_none());
}
#[test]
fn geometry_rejects_degenerate_and_discontinuous_lap_traces() {
    assert!(Trace::new(ring()).is_ok());
    assert!(Trace::new(vec![Point { x: 1.0, y: 1.0 }; 100]).is_err());
    let mut points = ring();
    points[100].x = 1e8;
    assert!(Trace::new(points).is_err());
    let mut points = ring();
    points.truncate(140);
    assert!(Trace::new(points).is_err());
    assert!(
        Trace::new(vec![
            Point {
                x: f64::NAN,
                y: 0.0
            };
            100
        ])
        .is_err()
    );
}
fn fix(at: i64, x: f64) -> Fix {
    Fix {
        at,
        available: Availability::Known(UtcMillis(at)),
        point: Some(Point { x, y: 0.0 }),
        ambiguous: false,
    }
}
#[test]
fn geometry_interpolates_only_admitted_short_pit_free_segments() {
    let a = fix(1000, -10.0);
    let b = fix(1250, 10.0);
    let middle = sample(a, Some(b), 1125, 1500, true, 100.0).unwrap();
    assert_eq!(middle.motion, Motion::Interpolated);
    assert_eq!(
        middle.point.x, 0.0,
        "Crossing an axis must interpolate raw XY, never wrap distance"
    );
    for (next, pit_free) in [
        (fix(2000, 10.0), true),
        (fix(1250, 1000.0), true),
        (b, false),
    ] {
        assert_eq!(
            sample(a, Some(next), 1125, 1500, pit_free, 100.0)
                .unwrap()
                .point
                .x,
            -10.0
        );
    }
    let mut future = b;
    future.available = Availability::Known(UtcMillis(1600));
    assert_eq!(
        sample(a, Some(future), 1125, 1500, true, 100.0)
            .unwrap()
            .motion,
        Motion::Held
    );
    future.available = Availability::Unknown;
    assert_eq!(
        sample(a, Some(future), 1125, 1500, true, 100.0)
            .unwrap()
            .point
            .x,
        -10.0
    );
    let mut missing = a;
    missing.point = None;
    assert!(sample(missing, Some(b), 1125, 1500, true, 100.0).is_none());
    missing = a;
    missing.ambiguous = true;
    assert!(sample(missing, Some(b), 1125, 1500, true, 100.0).is_none());
    assert!(
        sample(a, None, 3500, 4000, true, 100.0).is_none(),
        "Stale locations must leave the map"
    );
}

