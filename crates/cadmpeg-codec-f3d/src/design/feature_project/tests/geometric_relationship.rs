// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::{
    native_scope_definition, project_parameter_design_with_edge_identities, ProjectInputs,
};
use crate::design::test_support::{parameter_record, synthetic_feature_timelines};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::Severity;

#[test]
fn geometric_relationship_retains_eight_item_timeline_and_source_references() {
    for source_kind in ["Geometric Relationship", "GeometricRelationship"] {
        let scopes = (1..=8)
            .map(|index| {
                let kind = if index == 4 {
                    DesignFeatureKind::try_from(source_kind.to_owned()).unwrap()
                } else {
                    DesignFeatureKind::JointOrigin
                };
                let mut scope = DesignParameterScope::empty(
                    &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{index}"),
                    kind,
                    index,
                );
                if index == 4 {
                    scope
                        .try_edit(|draft| {
                            draft.reference_members =
                                crate::records::identity::ReferenceRun::unlocated(vec![
                                    41,
                                    u32::MAX,
                                ]);
                            draft.frame_length = 256;
                            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                            draft.layout_fixture_references();
                            draft.layout_fixture_tail();
                        })
                        .unwrap();
                } else {
                    scope.with_joint_origin_transform(
                        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    );
                }
                scope
            })
            .collect::<Vec<_>>();
        let timelines = synthetic_feature_timelines(&scopes);
        let projected = crate::test_support::with_decode_context(|ctx| {
            project_parameter_design_with_edge_identities(
                ctx,
                &ProjectInputs {
                    scopes: &scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .unwrap()
        });
        assert_eq!(projected.features.len(), 8);
        for (index, scope) in scopes.iter().enumerate() {
            let feature = projected
                .features
                .iter()
                .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
                .expect("projected scope");
            assert_eq!(feature.ordinal, u64::try_from(index).unwrap());
            if index == 3 {
                assert!(matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Native { kind, parameters })
                        if kind.as_str() == source_kind && parameters.is_empty()
                ));
                assert_eq!(feature.source_tag.as_deref(), Some(source_kind));
                assert_eq!(feature.source_properties["reference:0"], "41");
                assert_eq!(feature.source_properties["reference:1"], "4294967295");
            } else {
                assert!(matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem { .. })
                ));
            }
        }
        assert_eq!(projected.projection_losses.len(), 1);
        let loss = &projected.projection_losses[0];
        assert_eq!(
            loss.code,
            F3dLossCode::FeatureGeometricRelationshipNative.kind()
        );
        assert_eq!(loss.severity, Severity::Warning);
        assert!(loss.message.contains(&scopes[3].id));
    }
}

#[test]
fn geometric_relationship_retains_parameter_expression() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#4",
        DesignFeatureKind::try_from("Geometric Relationship".to_owned()).unwrap(),
        4,
    );
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(95),
        "2 mm",
        "FeatureInput",
        Some("mm"),
        "Offset",
        0.2,
    ))
    .unwrap();
    let definition = native_scope_definition(&scope, &[(0, &parameter)]).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Native { kind, parameters })
            if kind.as_str() == "Geometric Relationship" && parameters["Offset"] == "2 mm"
    ));
}
