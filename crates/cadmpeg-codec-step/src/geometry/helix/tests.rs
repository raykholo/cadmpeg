// SPDX-License-Identifier: Apache-2.0
use super::{helix_bspline, helix_point_tangent, HelixBspline};
use cadmpeg_ir::geometry::{HelixCurveConstruction, HelixFrame};
use cadmpeg_ir::math::{Point3, Vector3};

const SAMPLES_PER_SPAN: u32 = 16;

fn helix(
    angle_range: [f64; 2],
    radius: f64,
    rise: f64,
    apex_factor: f64,
) -> HelixCurveConstruction {
    HelixCurveConstruction::try_new(
        angle_range,
        HelixFrame {
            center: Point3::new(1.0, -2.0, 3.0),
            major: Vector3::new(radius, 0.0, 0.0),
            minor: Vector3::new(0.0, radius, 0.0),
            pitch: Vector3::new(0.0, 0.0, rise),
            axis: Vector3::new(0.0, 0.0, 1.0),
        },
        apex_factor,
        None,
    )
    .expect("admitted helix")
}

/// De Boor evaluation of the clamped cubic restatement.
fn bspline_point(fit: &HelixBspline, t: f64) -> Point3 {
    let knots: Vec<f64> = fit
        .knots
        .iter()
        .zip(&fit.multiplicities)
        .flat_map(|(knot, count)| std::iter::repeat_n(*knot, *count))
        .collect();
    let degree = 3;
    let last = fit.poles.len() - 1;
    let span = (degree..=last)
        .rev()
        .find(|&index| knots[index] <= t)
        .unwrap_or(degree);
    let mut points: Vec<Point3> = (0..=degree).map(|j| fit.poles[span - degree + j]).collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let index = span - degree + j;
            let denominator = knots[index + degree + 1 - r] - knots[index];
            let alpha = if denominator == 0.0 {
                0.0
            } else {
                (t - knots[index]) / denominator
            };
            let (a, b) = (points[j - 1], points[j]);
            points[j] = Point3::new(
                (1.0 - alpha) * a.x + alpha * b.x,
                (1.0 - alpha) * a.y + alpha * b.y,
                (1.0 - alpha) * a.z + alpha * b.z,
            );
        }
    }
    points[degree]
}

fn largest_deviation(helix: &HelixCurveConstruction, fit: &HelixBspline) -> f64 {
    let [start, end] = helix.angle_range().get();
    let spans = u32::try_from(fit.knots.len() - 1).expect("span count fits u32");
    let samples = SAMPLES_PER_SPAN * spans;
    (0..=samples)
        .map(|index| {
            let t = start + (end - start) * f64::from(index) / f64::from(samples);
            let (exact, _) = helix_point_tangent(helix, t);
            let fitted = bspline_point(fit, t);
            Vector3::new(fitted.x - exact.x, fitted.y - exact.y, fitted.z - exact.z).norm()
        })
        .fold(0.0, f64::max)
}

#[test]
fn restatement_stays_within_the_fit_tolerance() {
    for (helix, tolerance) in [
        (helix([0.0, std::f64::consts::TAU], 2.0, 10.0, 0.0), 5.0e-7),
        (helix([0.0187, 0.58], 15.0, 150.0, 0.0), 1.0e-6),
        (helix([-3.0, 9.0], 40.0, -4.0, 0.0), 1.0e-4),
        (helix([1.0, 7.0], 5.0, 3.0, 0.75), 1.0e-5),
    ] {
        let fit = helix_bspline(&helix, tolerance).expect("bounded restatement");
        assert!(fit.error_bound <= tolerance);
        assert_eq!(fit.poles.len(), 2 * (fit.knots.len() - 1) + 2);
        let [start, end] = helix.angle_range().get();
        assert_eq!(fit.knots.first(), Some(&start));
        assert_eq!(fit.knots.last(), Some(&end));
        assert_eq!(fit.multiplicities.first(), Some(&4));
        assert_eq!(fit.multiplicities.last(), Some(&4));
        assert!(fit.multiplicities[1..fit.multiplicities.len() - 1]
            .iter()
            .all(|count| *count == 2));
        let deviation = largest_deviation(&helix, &fit);
        assert!(
            deviation <= tolerance,
            "deviation {deviation} exceeds tolerance {tolerance}"
        );
        // The ends interpolate the helix ends exactly.
        let (first, _) = helix_point_tangent(&helix, start);
        let (last, _) = helix_point_tangent(&helix, end);
        assert_eq!(fit.poles.first(), Some(&first));
        assert_eq!(fit.poles.last(), Some(&last));
    }
}

#[test]
fn a_tighter_tolerance_uses_more_spans() {
    let helix = helix([0.0, 4.0 * std::f64::consts::PI], 10.0, 5.0, 0.0);
    let coarse = helix_bspline(&helix, 1.0e-3).expect("coarse fit");
    let fine = helix_bspline(&helix, 1.0e-6).expect("fine fit");
    assert!(fine.knots.len() > coarse.knots.len());
}

#[test]
fn unbounded_or_degenerate_fits_are_refused() {
    let turn = helix([0.0, std::f64::consts::TAU], 2.0, 10.0, 0.0);
    assert!(helix_bspline(&turn, 0.0).is_none());
    assert!(helix_bspline(&turn, -1.0).is_none());
    assert!(helix_bspline(&turn, f64::NAN).is_none());
    assert!(helix_bspline(&helix([1.0, 1.0], 2.0, 10.0, 0.0), 1.0e-3).is_none());
    // A fit that needs more than the span cap is refused, not truncated.
    let long = helix([0.0, 1.0e4], 1.0e3, 1.0, 0.0);
    assert!(helix_bspline(&long, 1.0e-9).is_none());
}
