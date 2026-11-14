// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::features::{
    BodySelection, BooleanKind, CombineOperands, DistinctMembers, Feature, FeatureDefinition,
    FeatureEvaluation, FeatureId, FeatureOperation,
};

use super::bind;
use crate::records::bodies::{DesignBodyBinding, DesignBodyBindingWire};
use crate::records::feature::combine::{
    DesignCombineBodyNode, DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation,
    DesignCombineTools,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};

const STREAM: &str = "f3d:Design/BulkStream.dat";

fn scope_id(record_index: u32) -> String {
    format!("{STREAM}:design-parameter-scope#{record_index}")
}

fn scope(kind: DesignFeatureKind, record_index: u32) -> DesignParameterScope {
    DesignParameterScope::empty(&scope_id(record_index), kind, record_index)
}

fn feature(name: &str, ordinal: u64, native_ref: &str) -> Feature {
    Feature {
        id: FeatureId::mint(format!("test:model:feature#{name}")).unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Combine {
                operands: CombineOperands::new(
                    BodySelection::Native("target".into()),
                    BodySelection::NativeSet(
                        vec!["tool-a".to_owned(), "tool-b".to_owned()]
                            .try_into()
                            .unwrap(),
                    ),
                )
                .unwrap(),
                op: BooleanKind::Join,
                keep_tools: false,
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some(native_ref.to_owned()),
        product_definition: None,
    }
}

/// A Combine scope in the operand-list form with one browser node per tool.
fn combine_scope(tools: &[(u32, u32)]) -> DesignParameterScope {
    let mut scope = scope(DesignFeatureKind::Combine, 400);
    let selection = |(entity, creator): (u32, u32), record_index| DesignCombineBodySelection {
        record_index,
        external_identity: None,
        body_node: Some(DesignCombineBodyNode {
            record_index: entity + 1,
            body_entity_suffix: entity,
            creating_scope_record_index: creator,
        }),
    };
    let mut rows = tools.iter().zip(12u32..).map(|(tool, at)| selection(*tool, at));
    if let DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
        *slot = Some(DesignCombineOperation {
            form: DesignCombineForm::OperandLists,
            operation: BooleanKind::Join,
            operation_offset: 0,
            keep_tools: false,
            keep_tools_offset: 0,
            target_record_index: 11,
            target_body_node: None,
            tools: DesignCombineTools {
                first: rows.next().unwrap(),
                additional: rows.collect(),
            },
        });
    }
    scope
}

fn pair(entity_suffix: u64, history_only: bool) -> DesignBodyBinding {
    DesignBodyBinding::try_from(DesignBodyBindingWire {
        component_owner: None,
        component_owner_unresolved: false,
        history_only,
        id: format!("{STREAM}:design-body-binding#{}", entity_suffix * 100),
        stream: "Design/BulkStream.dat".into(),
        pair_count: 1,
        pair_ordinal: 0,
        asm_body_key: 9,
        asm_body_key_offset: entity_suffix * 100,
        entity_suffix,
        entity_suffix_offset: entity_suffix * 100 + 8,
        blob_name: "BREP.tool.smbh".into(),
        blob_name_offset: entity_suffix * 100 + 16,
        body: None,
    })
    .unwrap()
}

fn tools_of(feature: &Feature) -> &BodySelection {
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
        feature.evaluation.definition()
    else {
        panic!("Combine feature");
    };
    operands.tools()
}

#[test]
fn history_only_tools_become_generated_bodies_of_their_creating_features() {
    let scopes = vec![
        scope(DesignFeatureKind::Extrude, 100),
        scope(DesignFeatureKind::Extrude, 200),
        combine_scope(&[(150, 100), (250, 200)]),
    ];
    let mut features = vec![
        feature("first", 1, &scope_id(100)),
        feature("second", 2, &scope_id(200)),
        feature("combine", 3, &scope_id(400)),
    ];
    let bindings = [pair(150, true), pair(250, true)];
    let losses = bind(&mut features, &scopes, &bindings, &[]);
    assert!(losses.is_empty(), "{losses:?}");
    let BodySelection::Generated { bodies, native } = tools_of(&features[2]) else {
        panic!("generated tools");
    };
    let generators = bodies
        .iter()
        .map(|body| body.feature.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        generators,
        [features[0].id.as_str(), features[1].id.as_str()]
    );
    assert_eq!(bodies[0].local_id.as_str(), bindings[0].id());
    assert_eq!(bodies[1].local_id.as_str(), bindings[1].id());
    assert_eq!(native.as_str(), scope_id(400));
    for generator in [&features[0], &features[1]] {
        assert!(features[2].dependencies.contains(&generator.id));
    }
}

#[test]
fn a_tool_that_is_not_proven_leaves_every_tool_native_and_reports_the_reason() {
    let scopes = || {
        vec![
            scope(DesignFeatureKind::Extrude, 100),
            scope(DesignFeatureKind::Extrude, 200),
            combine_scope(&[(150, 100), (250, 200)]),
        ]
    };
    let features = || {
        vec![
            feature("first", 1, &scope_id(100)),
            feature("second", 2, &scope_id(200)),
            feature("combine", 3, &scope_id(400)),
        ]
    };
    // A pair that is not history-only, an absent pair, and a creating scope that
    // is not a projected feature each leave the selection native.
    let cases = [
        (vec![pair(150, true), pair(250, false)], features()),
        (vec![pair(150, true)], features()),
        (vec![pair(150, true), pair(250, true)], {
            let mut rows = features();
            rows.remove(1);
            rows
        }),
        (vec![pair(150, true), pair(250, true)], {
            let mut rows = features();
            rows[1].ordinal = 3;
            rows
        }),
    ];
    for (bindings, mut features) in cases {
        let losses = bind(&mut features, &scopes(), &bindings, &[]);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.combine-tool-body-unbound"
        );
        let combine = features.last().unwrap();
        assert!(matches!(tools_of(combine), BodySelection::NativeSet(_)));
        assert!(combine.dependencies.is_empty());
    }
}

fn naming_space(component_record_index: u64) -> crate::records::recipes::DesignComponentNamingSpace {
    crate::records::recipes::DesignComponentNamingSpace {
        id: format!("{STREAM}:design-component-naming-space#{component_record_index}"),
        byte_offset: component_record_index,
        component_record_index,
        context_uuid: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
            .to_owned()
            .try_into()
            .expect("GUID"),
        context_uuid_offset: component_record_index + 12,
        name: None,
        name_offset: None,
    }
}

#[test]
fn a_derived_base_creator_uses_the_single_derived_instance_of_its_naming_space() {
    let scopes = |instances: &[u32]| {
        let mut scopes = vec![
            scope(DesignFeatureKind::Extrude, 100),
            scope(DesignFeatureKind::native("Derived Base Feature"), 350),
            combine_scope(&[(150, 100), (360, 350)]),
        ];
        scopes.extend(
            instances
                .iter()
                .map(|record| scope(DesignFeatureKind::DerivedInstance, *record)),
        );
        scopes
    };
    let features = |instances: &[u32]| {
        let mut rows = vec![
            feature("first", 1, &scope_id(100)),
            feature("combine", 4, &scope_id(400)),
        ];
        rows.extend(
            instances
                .iter()
                .zip(2u64..)
                .map(|(record, ordinal)| feature(&format!("instance{record}"), ordinal, &scope_id(*record))),
        );
        rows
    };
    let spaces = [naming_space(10), naming_space(300)];
    let bindings = [pair(150, true), pair(360, true)];

    let mut one = features(&[380]);
    let losses = bind(&mut one, &scopes(&[380]), &bindings, &spaces);
    assert!(losses.is_empty(), "{losses:?}");
    let BodySelection::Generated { bodies, .. } = tools_of(&one[1]) else {
        panic!("generated tools");
    };
    assert_eq!(bodies[1].feature.as_str(), one[2].id.as_str());

    // An instance in another naming space or a second instance in the same
    // space does not identify the generating feature.
    for instances in [&[200_u32][..], &[380, 390][..], &[][..]] {
        let mut rows = features(instances);
        let losses = bind(&mut rows, &scopes(instances), &bindings, &spaces);
        assert_eq!(losses.len(), 1, "{instances:?}");
        assert!(matches!(tools_of(&rows[1]), BodySelection::NativeSet(_)));
    }
}
