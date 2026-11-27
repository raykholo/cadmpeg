// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::tests::{reference_list, test_feature, test_label, test_property};
use crate::test_support::test_fixtures::{content_at, parse};
use std::collections::HashMap;

const SELECTOR_ORDINALS: [u32; 2] = [30, 31];

fn types() -> HashMap<u32, &'static str> {
    HashMap::from([
        (30, SELECTOR_TYPE_ID),
        (31, SELECTOR_TYPE_ID),
        (32, WRAPPER_TYPE_ID),
        (23, RESULT_TYPE_ID),
    ])
}

fn properties(control: bool) -> Vec<PmDcFeatureProperty> {
    vec![
        test_property(
            20,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceSelectors,
                items: reference_list(&SELECTOR_ORDINALS.map(|ordinal| ordinal + 1)),
            },
        ),
        test_property(
            21,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: control,
            },
        ),
        test_property(
            22,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BodySelection,
                items: reference_list(&[33]),
            },
        ),
    ]
}

fn source() -> PmDcFeature {
    test_feature(0, 4, &[(0, 20), (1, 21), (2, 22), (3, 23)])
}

fn read(
    source: &PmDcFeature,
    properties: &[PmDcFeatureProperty],
    types: &HashMap<u32, &'static str>,
) -> Result<FeatureOperation, NativeReason> {
    operation(
        source,
        |reference| {
            properties
                .iter()
                .find(|property| property.identity.record_ordinal + 1 == reference.index())
        },
        |ordinal| types.get(&ordinal).copied(),
    )
}

#[test]
fn selector_list_ends_after_the_list_and_one_owner_reference() {
    for (version, stamps) in [(16, &[][..]), (19, &[][..]), (23, &[-1][..])] {
        let mut bytes = content_at(3, version, stamps);
        for value in [2u16, 0x3000] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [2u32, 5, 0, 0x8000_0007, 0x8000_0008, 0x8000_0003] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let parser = property_parser(SELECTORS_TYPE).expect("selector-list parser");
        parse(&bytes, |ctx, source| {
            let parsed = parser(ctx, source, version).expect("selector list");
            let PmDcFeaturePropertyKind::References { family, items } = parsed.kind else {
                panic!("selector list")
            };
            assert_eq!(family, PmDcFeatureReferenceFamily::FaceSelectors);
            assert_eq!(
                items
                    .references()
                    .iter()
                    .map(|reference| reference.index())
                    .collect::<Vec<_>>(),
                [7, 8]
            );
        });
        parse(&bytes[..bytes.len() - 4], |ctx, source| {
            assert!(parser(ctx, source, version).is_err());
        });
        bytes.push(0);
        parse(&bytes, |ctx, source| {
            assert!(parser(ctx, source, version).is_err());
        });
    }
    assert!(property_parser([0; 16]).is_none());
}

#[test]
fn a_false_control_types_the_deletion_with_a_native_face_selection() {
    let properties = properties(false);
    let operation = read(&source(), &properties, &types()).expect("typed deletion");
    assert_eq!(
        operation,
        FeatureOperation::DeleteFace {
            faces: FaceSelection::Native(properties[0].id()),
            heal: HEAL_OF_FALSE_CONTROL,
        }
    );
}

#[test]
fn delete_face_refusals_name_the_failing_slot() {
    let all = types();
    let ok = properties(false);
    assert_eq!(
        read(&test_feature(0, 3, &[]), &ok, &all),
        Err(NativeReason::Slots)
    );
    assert_eq!(
        read(&source(), &properties(true), &all),
        Err(NativeReason::Control)
    );
    let mut wrong = all.clone();
    wrong.insert(31, "other");
    assert_eq!(read(&source(), &ok, &wrong), Err(NativeReason::Selectors));
    let selectors = |members: &[u32]| {
        test_property(
            20,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceSelectors,
                items: reference_list(members),
            },
        )
    };
    let mut repeated = ok.clone();
    repeated[0] = selectors(&[31, 31]);
    assert_eq!(
        read(&source(), &repeated, &all),
        Err(NativeReason::Selectors)
    );
    let mut empty = ok.clone();
    empty[0] = selectors(&[]);
    assert_eq!(read(&source(), &empty, &all), Err(NativeReason::Selectors));
    let body_selection = |family, members: &[u32]| {
        test_property(
            22,
            PmDcFeaturePropertyKind::References {
                family,
                items: reference_list(members),
            },
        )
    };
    let mut collection = ok.clone();
    collection[2] = body_selection(PmDcFeatureReferenceFamily::FaceCollection, &[31, 32]);
    assert_eq!(
        read(&source(), &collection, &all),
        Err(NativeReason::Selection)
    );
    let mut two = ok.clone();
    two[2] = body_selection(PmDcFeatureReferenceFamily::BodySelection, &[33, 33]);
    assert_eq!(read(&source(), &two, &all), Err(NativeReason::Selection));
    let mut foreign = all.clone();
    foreign.insert(32, "other");
    assert_eq!(read(&source(), &ok, &foreign), Err(NativeReason::Selection));
    let mut no_result = all;
    no_result.remove(&23);
    assert_eq!(read(&source(), &ok, &no_result), Err(NativeReason::Result));
}

fn placed(source: &PmDcFeature, typed: bool) -> Vec<cadmpeg_ir::features::Feature> {
    use cadmpeg_ir::features::evaluation::FeatureEvaluation;
    use cadmpeg_ir::features::{Feature, FeatureContent, FeatureId, NativeFeatureKind};
    let definition = if typed {
        FeatureDefinition::Operation(FeatureOperation::DeleteFace {
            faces: FaceSelection::Native(source.id()),
            heal: HEAL_OF_FALSE_CONTROL,
        })
    } else {
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: NativeFeatureKind::DeleteFace,
            parameters: std::collections::BTreeMap::default(),
        })
    };
    vec![Feature {
        id: FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
            cadmpeg_ir::identity_key!("generated"),
        ),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: typed.then(|| SOURCE_TAG.to_owned()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: Some(source.id()),
        product_definition: None,
    }]
}

fn inventory(source: PmDcFeature, class: ClassId, control: bool) -> FeatureInventory {
    let mut inventory = FeatureInventory {
        features: vec![source],
        labels: vec![test_label(0, 1, class, &[])],
        properties: properties(control),
        ..Default::default()
    };
    inventory.record_types.insert(
        crate::feature::tests::segment(),
        types()
            .into_iter()
            .map(|(ordinal, type_id)| (ordinal, Some(type_id.to_owned())))
            .collect(),
    );
    inventory
}

#[test]
fn a_refused_record_that_stays_native_emits_the_pinned_loss() {
    let source = source();
    let features = placed(&source, false);
    let inventory = inventory(source, CLASS_ID, true);
    assert!(native_losses(&inventory, &[]).is_empty());
    let losses = native_losses(&inventory, &features);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.delete-face-operands-native"
    );
    assert_eq!(
        losses[0].code.local_code(),
        InventorLossCode::FeatureDeleteFaceOperandsNative.code()
    );
    assert!(losses[0].message.contains("slot 1 is true"));
}

#[test]
fn a_typed_record_has_no_native_loss() {
    let source = source();
    let features = placed(&source, true);
    let inventory = inventory(source, CLASS_ID, false);
    assert!(native_losses(&inventory, &features).is_empty());
}

#[test]
fn the_native_loss_uses_the_label_class_and_record_owner() {
    let source = source();
    let features = placed(&source, false);
    let mut inventory = inventory(source, CLASS_ID, true);
    inventory.labels = vec![test_label(1, 1, CLASS_ID, &[])];
    assert!(native_losses(&inventory, &features).is_empty());
    inventory.labels = vec![test_label(0, 1, super::super::THREAD_CLASS_ID, &[])];
    assert!(native_losses(&inventory, &features).is_empty());
    inventory.labels = vec![test_label(0, 1, CLASS_ID, &[])];
    assert_eq!(native_losses(&inventory, &features).len(), 1);
}

#[test]
fn typed_features_emit_one_assumption_loss() {
    let source = source();
    assert!(assumption_losses(&placed(&source, false)).is_empty());
    let mut features = placed(&source, true);
    features.extend(placed(&source, true));
    let losses = assumption_losses(&features);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.delete-face-heal-assumed"
    );
    assert!(losses[0].message.starts_with("2 Inventor DeleteFace"));
}
