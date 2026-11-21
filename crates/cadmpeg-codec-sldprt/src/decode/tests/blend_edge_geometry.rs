// SPDX-License-Identifier: Apache-2.0
//! Geometry loss reporting for generated blend operands.

use cadmpeg_ir::features::edge_treatments::{
    FilletGroup, RadiusSpec, VariableRadii, VariableRadius,
};
use cadmpeg_ir::features::{
    EdgeSelection, Feature, FeatureDefinition, FeatureId, FeatureOperation, GeneratedEdgeRef,
};
use cadmpeg_ir::scalar::PositiveLength;

use crate::loss::SldprtLossCode;

fn feature(ordinal: u32, operation: FeatureOperation) -> Feature {
    serde_json::from_value(serde_json::json!({
        "id": format!("synthetic:test:id#feature-{ordinal}"),
        "ordinal": ordinal,
        "definition": FeatureDefinition::Operation(operation),
        "dependencies": [],
        "source_properties": {},
        "source_content": [],
        "outputs": [],
    }))
    .expect("feature")
}

#[test]
fn generated_blend_identity_emits_a_source_geometry_loss() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model
        .features
        .push(feature(0, FeatureOperation::StoredGeometry {}));
    let producer = FeatureId::mint("synthetic:test:id#feature-0").expect("producer");
    let selected = ["1", "2"]
        .into_iter()
        .map(|local| GeneratedEdgeRef::new(producer.clone(), local.into()).expect("edge"))
        .collect();
    ir.model.features.push(feature(
        1,
        FeatureOperation::Fillet {
            groups: vec![FilletGroup {
                edges: EdgeSelection::generated(selected, "native:edges".into())
                    .expect("selection"),
                radius: RadiusSpec::Constant {
                    radius: PositiveLength::new(2.0).expect("radius"),
                },
                tangency_weight: None,
            }]
            .try_into()
            .expect("group"),
        },
    ));
    ir.model.features[1].dependencies.insert(producer);
    let mut report = super::empty_report(true);
    crate::decode::append_design_losses(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .expect("loss audit");
    let losses = report
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::FeatureBlendEdgeGeometryUnavailable.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Warning);
    assert!(report.losses.iter().any(|loss| {
        loss.code == SldprtLossCode::FeatureBlendEdgeChainUnavailable.kind()
            && loss
                .message
                .contains("edge traversal order, and traversal senses")
    }));
    assert!(losses[0]
        .message
        .starts_with("2 selected generated blend edge(s)"));
}

#[test]
fn variable_stations_emit_the_missing_source_start_vertex_loss() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model
        .features
        .push(feature(0, FeatureOperation::StoredGeometry {}));
    let producer = FeatureId::mint("synthetic:test:id#feature-0").expect("producer");
    let edge = GeneratedEdgeRef::new(producer.clone(), "1".into()).expect("edge");
    let points = VariableRadii::new(vec![
        VariableRadius {
            parameter: 0.0,
            radius: PositiveLength::new(2.0).expect("radius").into(),
        },
        VariableRadius {
            parameter: 1.0,
            radius: PositiveLength::new(5.0).expect("radius").into(),
        },
    ])
    .expect("stations");
    ir.model.features.push(feature(
        1,
        FeatureOperation::Fillet {
            groups: vec![FilletGroup {
                edges: EdgeSelection::generated(vec![edge], "native:edge".into())
                    .expect("selection"),
                radius: RadiusSpec::Variable { points },
                tangency_weight: None,
            }]
            .try_into()
            .expect("group"),
        },
    ));
    ir.model.features[1].dependencies.insert(producer);
    let operation = ir.model.features[1].evaluation.definition().operation();
    assert_eq!(
        crate::history::blend_edges::missing_chain_count(operation),
        1
    );
    assert_eq!(
        crate::history::blend_edges::missing_station_start_count(operation),
        1
    );
    let mut report = super::empty_report(true);
    crate::decode::append_design_losses(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &ir,
        &mut report,
    )
    .expect("loss audit");
    let losses = report
        .losses
        .iter()
        .filter(|loss| {
            loss.code == SldprtLossCode::FeatureVariableFilletStartVertexUnavailable.kind()
        })
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Warning);
    assert!(losses[0]
        .message
        .contains("first radius station to a resolved source-chain start vertex"));
}
