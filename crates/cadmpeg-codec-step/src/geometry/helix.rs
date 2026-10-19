// SPDX-License-Identifier: Apache-2.0
//! Cubic B-spline restatement of a procedural helix.
//!
//! STEP has no helix entity. A helix `C(t)=center+s(t)·(major·cos t+minor·sin t)+pitch·q(t)`
//! with `q=t/(2π)` and `s=1+apex_factor·q` is transcendental in `t`, so no
//! B-spline states it exactly. The restatement interpolates the helix and its
//! first derivative at uniform knots over the stored interval `[t0,t1]`
//! (piecewise cubic Hermite interpolation). The B-spline parameter equals the
//! helix angle, so edge parameters and endpoints carry over unchanged.
//!
//! For one span of length `h`, each coordinate of the interpolation error is at
//! most `h⁴/384 · max|C⁗|`. `C⁗ = s·R⁗ + 4·s'·R‴` because `s` is linear and the
//! pitch term is linear, with `R=major·cos t+minor·sin t` and
//! `|R⁽ᵏ⁾| ≤ |major|+|minor|`. The model-space error over the three coordinates
//! is therefore at most `√3 · h⁴/384 · (max|s| + 4·|apex_factor|/(2π)) ·
//! (|major|+|minor|)`. The span count is the least count whose bound does not
//! exceed the requested fit tolerance.

use cadmpeg_ir::geometry::HelixCurveConstruction;
use cadmpeg_ir::math::{Point3, Vector3};

/// Largest span count one restatement may use.
pub(crate) const MAX_HELIX_SPANS: u32 = 4096;
/// Denominator of the cubic Hermite remainder `h⁴/(4!·2⁴)`.
const HERMITE_REMAINDER_DENOMINATOR: f64 = 384.0;

/// A clamped, non-rational cubic B-spline that restates one helix.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HelixBspline {
    /// Distinct knot values, ascending, from `t0` to `t1`.
    pub(crate) knots: Vec<f64>,
    /// Multiplicity of each distinct knot: 4 at the ends, 2 inside.
    pub(crate) multiplicities: Vec<usize>,
    /// Control points in model space.
    pub(crate) poles: Vec<Point3>,
    /// Proven upper bound of the model-space distance between the helix and
    /// the B-spline at equal parameter.
    pub(crate) error_bound: f64,
}

/// Restate `helix` as a cubic B-spline whose distance from the helix at equal
/// parameter is at most `tolerance`.
///
/// Returns `None` for a non-positive or non-finite tolerance, an empty angle
/// interval, a non-finite intermediate value, or a fit that needs more than
/// [`MAX_HELIX_SPANS`] spans.
pub(crate) fn helix_bspline(
    helix: &HelixCurveConstruction,
    tolerance: f64,
) -> Option<HelixBspline> {
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return None;
    }
    let [start, end] = helix.angle_range().get();
    let length = end - start;
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    let major = helix.major().get();
    let minor = helix.minor().get();
    let apex_factor = helix.apex_factor().get();
    // `s` is linear in `t`, so its largest magnitude is at an interval end.
    let radial_max = radial_scale(helix, start)
        .abs()
        .max(radial_scale(helix, end).abs());
    let radial_rate = apex_factor.abs() / std::f64::consts::TAU;
    let derivative_bound = 4.0f64.mul_add(radial_rate, radial_max) * (major.norm() + minor.norm());
    let bound = |span: f64| {
        3.0_f64.sqrt() * span.powi(4) / HERMITE_REMAINDER_DENOMINATOR * derivative_bound
    };
    let spans =
        (1..=MAX_HELIX_SPANS).find(|&count| bound(length / f64::from(count)) <= tolerance)?;
    let span = length / f64::from(spans);
    let knot = |index: u32| {
        if index == spans {
            end
        } else {
            f64::from(index).mul_add(span, start)
        }
    };
    let count = usize::try_from(spans).ok()?;
    let mut knots = Vec::with_capacity(count + 1);
    let mut multiplicities = Vec::with_capacity(count + 1);
    let mut poles = Vec::with_capacity(2 * count + 2);
    // Each span is the Hermite cubic in Bézier form: P(a), P(a)+h/3·C'(a),
    // P(b)-h/3·C'(b), P(b). With multiplicity-2 interior knots the shared
    // Bézier end point is the midpoint of its two neighbours (equal spans and a
    // common derivative), so the B-spline stores only the two inner poles.
    let (mut point, mut tangent) = helix_point_tangent(helix, start);
    poles.push(point);
    for index in 0..spans {
        let lower = knot(index);
        let upper = knot(index + 1);
        let third = (upper - lower) / 3.0;
        let (next_point, next_tangent) = helix_point_tangent(helix, upper);
        poles.push(translated(point, tangent, third));
        poles.push(translated(next_point, next_tangent, -third));
        knots.push(lower);
        multiplicities.push(if index == 0 { 4 } else { 2 });
        point = next_point;
        tangent = next_tangent;
    }
    poles.push(point);
    knots.push(end);
    multiplicities.push(4);
    let error_bound = bound(span);
    let finite = poles
        .iter()
        .all(|pole| pole.x.is_finite() && pole.y.is_finite() && pole.z.is_finite());
    (finite && poles.len() == 2 * count + 2).then_some(HelixBspline {
        knots,
        multiplicities,
        poles,
        error_bound,
    })
}

fn translated(point: Point3, direction: Vector3, scale: f64) -> Point3 {
    Point3::new(
        direction.x.mul_add(scale, point.x),
        direction.y.mul_add(scale, point.y),
        direction.z.mul_add(scale, point.z),
    )
}

/// Revolution fraction `q` of the pitch and apex terms at angle `t`.
///
/// `q` is measured from angle zero, not from the interval start.
fn revolution_fraction(t: f64) -> f64 {
    t / std::f64::consts::TAU
}

/// Radial scale `s=1+apex_factor·q` at angle `t`.
fn radial_scale(helix: &HelixCurveConstruction, t: f64) -> f64 {
    helix
        .apex_factor()
        .get()
        .mul_add(revolution_fraction(t), 1.0)
}

/// Point and first derivative of the helix at angle `t`.
pub(crate) fn helix_point_tangent(helix: &HelixCurveConstruction, t: f64) -> (Point3, Vector3) {
    let center = helix.center().get();
    let major = helix.major().get();
    let minor = helix.minor().get();
    let pitch = helix.pitch().get();
    let apex_factor = helix.apex_factor().get();
    let inverse_revolution = 1.0 / std::f64::consts::TAU;
    let fraction = revolution_fraction(t);
    let scale = radial_scale(helix, t);
    let (sin, cos) = t.sin_cos();
    let radial = Vector3::new(
        major.x * cos + minor.x * sin,
        major.y * cos + minor.y * sin,
        major.z * cos + minor.z * sin,
    );
    let radial_first = Vector3::new(
        minor.x * cos - major.x * sin,
        minor.y * cos - major.y * sin,
        minor.z * cos - major.z * sin,
    );
    let point = Point3::new(
        center.x + scale * radial.x + fraction * pitch.x,
        center.y + scale * radial.y + fraction * pitch.y,
        center.z + scale * radial.z + fraction * pitch.z,
    );
    let rate = apex_factor * inverse_revolution;
    let tangent = Vector3::new(
        scale * radial_first.x + rate * radial.x + inverse_revolution * pitch.x,
        scale * radial_first.y + rate * radial.y + inverse_revolution * pitch.y,
        scale * radial_first.z + rate * radial.z + inverse_revolution * pitch.z,
    );
    (point, tangent)
}

#[cfg(test)]
mod tests;
