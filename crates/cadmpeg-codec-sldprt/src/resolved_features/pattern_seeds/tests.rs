// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::features::{
    patterns::PatternKind, DistinctMembers, FeatureContent, FeatureEvaluation,
};
use std::collections::BTreeMap;

fn fixture() -> (Vec<Feature>, Vec<FeatureHistory>) {
    let native = |source: u32| crate::records::Feature {
        id: format!("native-{source}"),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(crate::records::FeatureSource::from_value(source).unwrap()),
        ordinal: source,
        name: format!("feature-{source}"),
        kind: "Feature".into(),
        input_class: Some(
            if source == 20 {
                "moLPattern_c"
            } else {
                "moCut_c"
            }
            .into(),
        ),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let features = [10, 11, 20]
        .into_iter()
        .map(|source| Feature {
            product_definition: None,
            id: FeatureId::mint(format!("test:model:feature#{source}")).unwrap(),
            ordinal: source,
            name: None,
            suppressed: Some(false),
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                if source == 20 {
                    FeatureOperation::Pattern {
                        seeds: Vec::new(),
                        pattern: PatternKind::UNRESOLVED_LINEAR,
                    }
                } else {
                    FeatureOperation::BaseFeature {
                        bodies: cadmpeg_ir::features::BodySelection::Unresolved,
                    }
                },
            )),
            native_ref: Some(format!("native-{source}")),
        })
        .collect();
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: [10, 11, 20].into_iter().map(native).collect(),
    };
    (features, vec![history])
}

fn record(seeds: Option<Vec<u32>>) -> PatternSeeds {
    PatternSeeds {
        owner: 20,
        at: 100,
        seeds: seeds.map(|seeds| seeds.into_iter().map(Some).collect()),
    }
}

#[test]
fn complete_selection_preserves_authored_order_and_cardinality() {
    let (features, histories) = fixture();
    let resolved = resolve(&features, &histories, &[record(Some(vec![11, 10]))], "lane");
    assert_eq!(
        resolved[&features[2].id],
        vec![
            PatternSeed::Feature(features[1].id.clone()),
            PatternSeed::Feature(features[0].id.clone())
        ]
    );
}

#[test]
fn absent_empty_and_conflicting_rosters_keep_the_selection_native() {
    let (features, histories) = fixture();
    for sources in [None, Some(vec![])] {
        assert!(resolve(&features, &histories, &[record(sources)], "lane").is_empty());
    }
    assert!(resolve(
        &features,
        &histories,
        &[record(Some(vec![10])), record(Some(vec![11]))],
        "lane"
    )
    .is_empty());
}

#[test]
fn unresolved_and_self_members_preserve_the_other_seeds() {
    let (features, histories) = fixture();
    for unresolved in [99, 20] {
        let resolved = resolve(
            &features,
            &histories,
            &[record(Some(vec![10, unresolved]))],
            "lane",
        );
        assert_eq!(
            resolved[&features[2].id],
            vec![
                PatternSeed::Feature(features[0].id.clone()),
                PatternSeed::Native(format!("lane@seed-list:100:1:source={unresolved}")),
            ]
        );
    }
}

#[test]
fn repeated_members_preserve_their_source_identity_and_cardinality() {
    let (features, histories) = fixture();
    let resolved = resolve(&features, &histories, &[record(Some(vec![10, 10]))], "lane");
    assert_eq!(
        resolved[&features[2].id],
        vec![
            PatternSeed::Feature(features[0].id.clone()),
            PatternSeed::Feature(features[0].id.clone())
        ]
    );
}

#[test]
fn source_identity_conflict_retains_the_source_identifier() {
    let (features, mut histories) = fixture();
    histories[0].features[1].source_id = histories[0].features[0].source_id;
    let resolved = resolve(&features, &histories, &[record(Some(vec![10]))], "lane");
    assert_eq!(
        resolved[&features[2].id],
        vec![PatternSeed::Native("lane@seed-list:100:0:source=10".into())]
    );
}

#[test]
fn repeated_native_binding_keeps_the_source_identifier() {
    let (mut features, histories) = fixture();
    features[1].native_ref = features[0].native_ref.clone();
    let resolved = resolve(&features, &histories, &[record(Some(vec![10]))], "lane");
    assert_eq!(
        resolved[&features[2].id],
        vec![PatternSeed::Native("lane@seed-list:100:0:source=10".into())]
    );
}

#[test]
fn seed_installation_preserves_both_pattern_transforms() {
    use cadmpeg_ir::features::patterns::PatternTransform;
    use cadmpeg_ir::features::{FeatureDirection3, FinitePoint3};
    use cadmpeg_ir::scalar::PositiveLength;
    let direction = FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)).unwrap();
    for transform in [
        PatternTransform::Linear {
            direction: Some(direction),
            spacing: PositiveLength::new(10.0).unwrap(),
            count: 3,
            second: None,
            omitted_instances: Vec::new(),
            symmetric: false,
        },
        PatternTransform::Mirror {
            plane_origin: FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)).unwrap(),
            plane_normal: FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0))
                .unwrap(),
        },
    ] {
        let pattern = PatternKind::new(transform).unwrap();
        let mut definition = FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: Vec::new(),
            pattern: pattern.clone(),
        });
        let seed = FeatureId::mint("test:model:feature#seed").unwrap();
        install(&mut definition, &[PatternSeed::Feature(seed.clone())]);
        assert_eq!(
            definition,
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: vec![PatternSeed::Feature(seed)],
                pattern
            })
        );
    }
}

fn lane(slot: u32) -> FeatureInputLane {
    serde_json::from_value(serde_json::json!({
        "id": format!("sldprt:feature-input:resolved-features#{slot}"),
        "configuration": slot.to_string(), "native_payload": ""
    }))
    .unwrap()
}

fn configuration(
    slot: u32,
    active: bool,
    features: &[Feature],
) -> cadmpeg_ir::features::DesignConfiguration {
    use cadmpeg_ir::features::{
        ConfigurationEvaluation, ConfigurationFeatureState, ConfigurationId,
    };
    cadmpeg_ir::features::DesignConfiguration {
        id: ConfigurationId::mint(format!("test:model:configuration#{slot}")).unwrap(),
        ordinal: slot,
        active,
        source_index: Some(slot),
        name: None,
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        native_ref: None,
        feature_states: features
            .iter()
            .map(|feature| {
                (
                    feature.id.clone(),
                    ConfigurationFeatureState {
                        evaluation: ConfigurationEvaluation::Active {
                            outputs: DistinctMembers::default(),
                        },
                        dependencies: DistinctMembers::default(),
                        definition: feature.evaluation.definition().clone(),
                    },
                )
            })
            .collect(),
    }
}

fn sources(definition: &FeatureDefinition) -> &[PatternSeed] {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = definition else {
        panic!("pattern")
    };
    seeds
}

#[test]
fn configuration_lists_keep_separate_seeds_and_bind_the_active_model() {
    let (features, histories) = fixture();
    let mut ir = CadIr::empty();
    ir.model.configurations = vec![
        configuration(0, false, &features),
        configuration(1, true, &features),
    ];
    ir.model.features = features;
    let lanes = [lane(0), lane(1)];
    let mut selections = Selections::default();
    selections.insert(lanes[0].id.clone(), vec![record(Some(vec![10]))]);
    selections.insert(lanes[1].id.clone(), vec![record(Some(vec![11, 10]))]);
    let mut losses = Vec::new();
    selections.bind(&mut ir, &histories, &lanes, &mut losses);
    let features = &ir.model.features;
    let a = PatternSeed::Feature(features[0].id.clone());
    let b = PatternSeed::Feature(features[1].id.clone());
    assert_eq!(
        sources(features[2].evaluation.definition()),
        &[b.clone(), a.clone()]
    );
    assert_eq!(
        sources(&ir.model.configurations[0].feature_states[&features[2].id].definition),
        std::slice::from_ref(&a)
    );
    assert_eq!(
        sources(&ir.model.configurations[1].feature_states[&features[2].id].definition),
        &[b, a]
    );
    assert!(losses.is_empty());
}

#[test]
fn unread_active_selection_retains_native_with_a_pinned_loss() {
    let (features, histories) = fixture();
    let mut ir = CadIr::empty();
    ir.model.configurations = vec![
        configuration(0, false, &features),
        configuration(1, true, &features),
    ];
    ir.model.features = features;
    let lanes = [lane(0), lane(1)];
    let mut selections = Selections::default();
    selections.insert(lanes[0].id.clone(), vec![record(Some(vec![10]))]);
    selections.insert(lanes[1].id.clone(), vec![record(None)]);
    let mut losses = Vec::new();
    selections.bind(&mut ir, &histories, &lanes, &mut losses);
    assert!(sources(ir.model.features[2].evaluation.definition()).is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0], SldprtLossCode::FeaturePatternSeedsUnresolved.note(
        "Pattern test:model:feature#20 retains its seed selection at byte 100 in the native feature-input stream"
    ));
}
