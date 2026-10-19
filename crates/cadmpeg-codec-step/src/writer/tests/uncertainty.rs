// SPDX-License-Identifier: Apache-2.0
//! The written length uncertainty.

#![allow(clippy::unwrap_used)]

use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::{StepSchema, StepWriteOptions};

/// The written coincidence distance covers tolerant edges: their vertices lie off their curves
/// by up to the edge tolerance, so a smaller global uncertainty would make the topology
/// inconsistent.
#[test]
fn uncertainty_covers_tolerant_edges_and_vertices() {
    let mut ir: CadIr = unit_cube().unwrap();
    let tolerance = cadmpeg_ir::scalar::PositiveReal::new(0.05).unwrap();
    ir.model.edges[0].tolerance = Some(tolerance);
    ir.model.vertices[0].tolerance = Some(cadmpeg_ir::scalar::PositiveReal::new(0.02).unwrap());
    let mut encoded = Vec::new();
    write_step(
        &ir,
        &mut encoded,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(encoded).unwrap();
    let measure = text
        .split("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap();
    assert!((measure - 0.05).abs() < 1e-12, "{measure}");
}
