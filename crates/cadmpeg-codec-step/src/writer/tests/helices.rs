// SPDX-License-Identifier: Apache-2.0
//! Procedural helix edges restated as fitted B-splines.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::{admission::EvaluationAdmission, model_curve_point_by_id};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    CurveGeometry, HelixCurveConstruction, HelixFrame, ProceduralCurve, ProceduralCurveDefinition,
    SolvedCurveGeometry,
};
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::EdgeCarrier;
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

const SAMPLES: u32 = 256;

/// The unit-cube fixture with its vertical edge from (0,0,0) to (0,0,10)
/// carried by one full helix turn with the same end points.
fn cube_with_helix_edge() -> CadIr {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let edge = ir
        .model
        .edges
        .iter_mut()
        .find(|edge| edge.id.as_str().ends_with("edge#8"))
        .expect("vertical cube edge");
    let curve_id = edge.carrier.curve().cloned().expect("edge curve");
    edge.carrier = EdgeCarrier::new(Some(curve_id.clone()), Some([0.0, std::f64::consts::TAU]))
        .expect("helix edge carrier");
    let construction = ProceduralCurve::new(
        "test:model:procedural_curve#helix"
            .try_into()
            .expect("identity grammar"),
        ProceduralCurveDefinition::Helix(
            HelixCurveConstruction::try_new(
                [0.0, std::f64::consts::TAU],
                HelixFrame {
                    center: Point3::new(2.0, 0.0, 0.0),
                    major: Vector3::new(-2.0, 0.0, 0.0),
                    minor: Vector3::new(0.0, -2.0, 0.0),
                    pitch: Vector3::new(0.0, 0.0, 10.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
                0.0,
                None,
            )
            .expect("admitted helix"),
        ),
    );
    let curve = ir
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .expect("edge curve record");
    curve.geometry = CurveGeometry::Procedural {
        construction: construction.id.clone(),
        cache: None,
    };
    ir.model.procedural_curves.push(construction);
    ir
}

#[test]
fn helix_edge_round_trips_within_the_fit_tolerance_and_the_shell_stays_closed() {
    let ir = cube_with_helix_edge();
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write helix cube");
    let codes: Vec<_> = report.losses.iter().map(|loss| loss.code.clone()).collect();
    assert!(codes.contains(&StepLossCode::ProceduralCurveApproximated.kind()));
    for absent in [
        StepLossCode::EdgeCurveUnsupported,
        StepLossCode::ProceduralDefinitionNotWritten,
        StepLossCode::ProceduralReducedToCarrier,
    ] {
        assert!(!codes.contains(&absent.kind()), "{absent:?} reported");
    }
    let text = String::from_utf8(output.clone()).expect("STEP is UTF-8");
    assert!(text.contains("CLOSED_SHELL"));
    assert_eq!(text.matches("ADVANCED_FACE").count(), 6);
    assert_eq!(text.matches("EDGE_CURVE").count(), 12);
    assert_eq!(text.matches("B_SPLINE_CURVE_WITH_KNOTS").count(), 1);

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode helix cube");
    let model = &decoded.ir().model;
    assert_eq!(model.faces.len(), 6);
    assert_eq!(model.edges.len(), 12);
    assert_eq!(model.shells.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let fitted = model
        .curves
        .iter()
        .find(|curve| matches!(curve.geometry.solved(), Some(SolvedCurveGeometry::Nurbs(_))))
        .expect("restated helix carrier");
    let tolerance = ir.tolerances.linear.get();
    let source_index = ModelIndex::build(&ir, StandardIndex);
    let decoded_index = ModelIndex::build(decoded.ir(), StandardIndex);
    let helix_curve = ir
        .model
        .curves
        .iter()
        .find(|curve| matches!(curve.geometry, CurveGeometry::Procedural { .. }))
        .expect("source helix curve");
    for index in 0..=SAMPLES {
        let t = std::f64::consts::TAU * f64::from(index) / f64::from(SAMPLES);
        let exact = model_curve_point_by_id(
            EvaluationAdmission::Standard,
            &source_index,
            &helix_curve.id,
            t,
        )
        .expect("helix point")
        .get();
        let restated =
            model_curve_point_by_id(EvaluationAdmission::Standard, &decoded_index, &fitted.id, t)
                .expect("restated point")
                .get();
        let deviation = Vector3::new(
            restated.x - exact.x,
            restated.y - exact.y,
            restated.z - exact.z,
        )
        .norm();
        assert!(
            deviation <= tolerance,
            "deviation {deviation} at {t} exceeds {tolerance}"
        );
    }
}
