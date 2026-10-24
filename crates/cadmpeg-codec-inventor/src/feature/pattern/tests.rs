// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::tests::{
    neutral_parameter, raw_parameter, reference, reference_list, segment, test_header,
    test_projection_index, test_property,
};
use crate::record_identity::{Located, RecordTypeId};
use crate::test_support::test_fixtures::{content_at, parse as with_bytes};
use cadmpeg_ir::features::ParameterValue;
use cadmpeg_ir::scalar::Length;

#[test]
fn is_linear_pattern_accepts_linear_and_rejects_mirror_and_native_features() {
    let source = pattern(19, false);
    let mut feature = with_bytes(&[], |ctx, _| {
        native_feature(ctx, &source.identity, None, source.id())
    })
    .expect("valid synthetic native feature");
    assert!(!is_linear_pattern(&feature));

    for (transform, expected) in [
        (
            PatternTransform::Linear {
                direction: None,
                spacing: PositiveLength::new(1.0).expect("positive spacing"),
                count: 2,
                second: None,
            },
            true,
        ),
        (
            PatternTransform::Mirror {
                plane_origin: cadmpeg_ir::features::FinitePoint3::new(
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                )
                .expect("finite origin"),
                plane_normal: FeatureDirection3::new(Vector3::new(1.0, 0.0, 0.0))
                    .expect("nonzero normal"),
            },
            false,
        ),
    ] {
        feature.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: vec![PatternSeed::Feature(feature.id.clone())],
                pattern: PatternKind::new(transform).expect("valid synthetic pattern"),
            }),
        );
        assert_eq!(is_linear_pattern(&feature), expected);
    }
}

fn suppression(version: u8, pairs: &[[u32; 2]]) -> Vec<u8> {
    let stamps: &[i32] = match version {
        0..=22 => &[],
        23..=24 => &[-1],
        _ => &[-1, 0],
    };
    let mut bytes = content_at(1, version, stamps);
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000u16.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(pairs.len())
            .expect("valid synthetic pattern fixture")
            .to_le_bytes(),
    );
    if !pairs.is_empty() {
        bytes.extend_from_slice(&[0; 8]);
    }
    for pair in pairs {
        for value in pair {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn suppression_coordinates_and_header_stamps_are_bounded() {
    for version in [19, 23, 25, 28, 29, 30, 31] {
        let bytes = suppression(version, &[[2, 3], [4, 1]]);
        let operand = with_bytes(&bytes, |ctx, source| {
            parse(ctx, SUPPRESSION_TYPE, source, version)
        })
        .expect("valid synthetic pattern fixture");
        assert!(matches!(operand, Operand::Suppression(pairs) if pairs == [[2, 3], [4, 1]]));
        for end in 0..bytes.len() {
            assert!(with_bytes(&bytes[..end], |ctx, source| parse(
                ctx,
                SUPPRESSION_TYPE,
                source,
                version
            ))
            .is_err());
        }
        let mut extra = bytes;
        extra.push(0);
        assert!(with_bytes(&extra, |ctx, source| parse(
            ctx,
            SUPPRESSION_TYPE,
            source,
            version
        ))
        .is_err());
    }
    let mut oversized = suppression(19, &[]);
    oversized.extend_from_slice(&[0; 8]);
    oversized[26..30].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(with_bytes(&oversized, |ctx, source| parse(
        ctx,
        SUPPRESSION_TYPE,
        source,
        19
    ))
    .is_err());
}

fn cached_line(type_id: [u8; 16], version: u8, span: [f64; 3]) -> Vec<u8> {
    let mut bytes = cached_selection_prefix(version, -1, 1, 0, 23);
    for value in [0.0; 3].into_iter().chain(span).chain([0.0; 3]) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&93u32.to_le_bytes());
    if type_id == EDGE_DIRECTION_TYPE {
        bytes.extend_from_slice(&94u32.to_le_bytes());
    }
    bytes
}

fn cached_selection_prefix(
    version: u8,
    state: i32,
    value: u32,
    zero: u16,
    curve_form: u32,
) -> Vec<u8> {
    let mut bytes = content_at(2, version, &[]);
    bytes.extend_from_slice(&state.to_le_bytes());
    for reference in [91u32, 92] {
        bytes.extend_from_slice(&reference.to_le_bytes());
    }
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes.extend_from_slice(&zero.to_le_bytes());
    bytes.extend_from_slice(&curve_form.to_le_bytes());
    bytes
}

#[test]
fn unsupported_selection_forms_do_not_apply_single_line_invariants() {
    for type_id in [EDGE_DIRECTION_TYPE, LINE_DIRECTION_TYPE] {
        for (state, value, zero, curve) in [
            (-1, 1, 0, 24),
            (0, 1, 0, 24),
            (-1, 0, 0, 24),
            (-1, 1, 1, 24),
            (-1, 2, 0, 23),
        ] {
            let bytes = cached_selection_prefix(25, state, value, zero, curve);
            assert!(matches!(
                with_bytes(&bytes, |ctx, source| parse(ctx, type_id, source, 25)),
                Ok(Operand::Unresolved)
            ));
        }
        for (state, zero) in [(0, 0), (-1, 1)] {
            let bytes = cached_selection_prefix(25, state, 1, zero, 23);
            assert!(with_bytes(&bytes, |ctx, source| parse(ctx, type_id, source, 25)).is_err());
        }
    }
}

#[test]
fn cached_line_span_has_no_stamps_and_rejects_invalid_geometry() {
    for version in [19, 25, 28] {
        for type_id in [EDGE_DIRECTION_TYPE, LINE_DIRECTION_TYPE] {
            let bytes = cached_line(type_id, version, [0.0, 8.0, 0.0]);
            let operand = with_bytes(&bytes, |ctx, source| parse(ctx, type_id, source, version))
                .expect("valid synthetic pattern fixture");
            assert!(
                matches!(operand, Operand::Line { span, .. } if span == Vector3::new(0.0, 8.0, 0.0))
            );
            for end in 0..bytes.len() {
                assert!(with_bytes(&bytes[..end], |ctx, source| parse(
                    ctx, type_id, source, version
                ))
                .is_err());
            }
            for span in [[0.0; 3], [f64::NAN, 1.0, 0.0], [f64::INFINITY, 1.0, 0.0]] {
                let bytes = cached_line(type_id, version, span);
                assert!(
                    with_bytes(&bytes, |ctx, source| parse(ctx, type_id, source, version)).is_err()
                );
            }
        }
    }
}

fn pattern(version: u8, second: bool) -> PmDcPatternFeature {
    let base = if version >= 21 { 18 } else { 12 };
    let mut slots = vec![reference(0); base + 14];
    for (slot, index) in [
        (0, 81),
        (1, 82),
        (2, 83),
        (4, 84),
        (5, 85),
        (6, 91),
        (9, 86),
        (11, 91),
        (base, 87),
        (base + 2, 71),
        (base + 4, 88),
        (base + 6, 91),
        (base + 8, 91),
        (base + 10, 91),
        (base + 12, 91),
    ] {
        slots[slot] = reference(index);
    }
    if second {
        for (slot, index) in [
            (base + 1, 89),
            (base + 3, 72),
            (base + 5, 90),
            (base + 7, 91),
            (base + 9, 91),
            (base + 11, 91),
            (base + 13, 91),
        ] {
            slots[slot] = reference(index);
        }
    }
    Located::new(
        super::super::PmDcPatternFeaturePayload {
            save_version_major: version,
            header: crate::pmdc::PmDcContentHeader {
                flags_extension: (version >= 28).then_some(0),
                ..test_header()
            },
            state: -1,
            outline_value: 0,
            properties: reference_list(&[]),
            value: 0,
            participants: reference_list(&[3, 8]),
            family: PmDcPatternFamily::Rectangular,
            property_slots: slots,
            control: 0,
            extension_values: match version {
                29 => vec![0; 8],
                30.. => vec![0; 14],
                _ => Vec::new(),
            },
        },
        RecordTypeId::from_bytes(super::super::RECTANGULAR_PATTERN_FEATURE_TYPE),
        segment(),
        20,
    )
}

fn project_fixture(
    source: &PmDcPatternFeature,
    change: impl FnOnce(&mut Operands, &mut history::History, &mut Vec<PmDcFeatureProperty>),
) -> Option<Feature> {
    let mut properties = vec![
        test_property(
            82,
            PmDcFeaturePropertyKind::RdxVariable {
                name: String::new(),
                name_value: 0,
                nominal_value: 0,
                model_value: 0,
            },
        ),
        test_property(
            84,
            PmDcFeaturePropertyKind::References {
                family: super::super::PmDcFeatureReferenceFamily::FeatureDimensions,
                items: reference_list(&[]),
            },
        ),
        test_property(
            85,
            PmDcFeaturePropertyKind::References {
                family: super::super::PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[96]),
            },
        ),
        test_property(
            86,
            PmDcFeaturePropertyKind::RdxVariable {
                name: String::new(),
                name_value: 0,
                nominal_value: 3,
                model_value: 3,
            },
        ),
        test_property(
            88,
            PmDcFeaturePropertyKind::RdxVariable {
                name: String::new(),
                name_value: 0,
                nominal_value: 2,
                model_value: 2,
            },
        ),
        test_property(
            90,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
        ),
        test_property(
            95,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(97),
            },
        ),
    ];
    let raw = [raw_parameter(70), raw_parameter(71)];
    let neutral = [
        neutral_parameter(
            &raw[0],
            ParameterValue::Length(Length::new(6.0).expect("valid synthetic pattern fixture")),
        ),
        neutral_parameter(
            &raw[1],
            ParameterValue::Length(Length::new(4.0).expect("valid synthetic pattern fixture")),
        ),
    ];
    let transform = Located::new(
        crate::sketch::PmDcTransformPayload {
            save_version_major: source.save_version_major,
            header: test_header(),
            prefix_present: false,
            matrix: crate::compact_matrix::CompactMatrix::try_from_rows(
                0x8421,
                0x7bde,
                [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            )
            .expect("valid synthetic pattern fixture"),
        },
        RecordTypeId::from_bytes(crate::pmdc::inventor_id(0x9087_4d18)),
        segment(),
        80,
    );
    let mut operands = Operands::default();
    for (ordinal, type_id) in [
        (81, SUPPRESSION_TYPE),
        (83, SELECTION_CONTROL_TYPE),
        (87, LINE_DIRECTION_TYPE),
        (89, LINE_DIRECTION_TYPE),
    ] {
        operands.note_record("generated", ordinal, type_id);
    }
    operands.records.insert(
        ("generated".into(), 81),
        Some(Operand::Suppression(Vec::new())),
    );
    operands.records.insert(
        ("generated".into(), 83),
        Some(Operand::SelectionControl {
            references: [reference(0); 2],
            value: 0,
        }),
    );
    for (ordinal, span) in [
        (87, Vector3::new(5.0, 0.0, 0.0)),
        (89, Vector3::new(0.0, -2.0, 0.0)),
    ] {
        operands.records.insert(
            ("generated".into(), ordinal),
            Some(Operand::Line {
                reverse: reference(91),
                geometry: reference(92),
                links: vec![reference(93)],
                span,
            }),
        );
    }
    for ordinal in [2, 7] {
        operands.note_record("generated", ordinal, super::super::FEATURE_TYPE);
    }
    operands.note_record("generated", 91, crate::pmdc::inventor_id(0xce52_df3e));
    operands.note_record("generated", 92, crate::pmdc::inventor_id(0x9087_4d15));
    let mut history = history::History::default();
    for (record, ordinal) in [(2, 2), (7, 5), (20, 7)] {
        history.placements.insert(
            record,
            history::HistoryPlacement {
                ordinal,
                kind: history::HistoryNodeKind::Feature,
                below_end_of_part: false,
                model_history: true,
            },
        );
    }
    change(&mut operands, &mut history, &mut properties);
    let transforms = [transform];
    let index = test_projection_index(&properties, &raw, &neutral, &[], &[], &[], &transforms, &[]);
    let (feature, result) = with_bytes(&[], |ctx, _| {
        project(ctx, source, None, &index, &operands, &history)
    })?
    .expect("synthetic pattern projection succeeds");
    let results = [result];
    assert!(matches_source(
        source,
        &feature,
        &properties,
        &neutral,
        &transforms,
        &results
    ));
    let base = if source.save_version_major >= 21 {
        18
    } else {
        12
    };
    let axes = if source.property_slots[base + 1].index() == 0 {
        1
    } else {
        2
    };
    for number in [2, 5]
        .into_iter()
        .chain((0..axes).flat_map(|axis| [6, 8, 10, 12].map(|offset| base + offset + axis)))
    {
        let mut payload = (**source).clone();
        payload.property_slots[number] = reference(99);
        let changed_source = Located::new(
            payload,
            source.identity.type_id.clone(),
            segment(),
            source.identity.record_ordinal,
        );
        let mut changed_properties = properties.clone();
        changed_properties.push(test_property(
            98,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ));
        assert!(!matches_source(
            &changed_source,
            &feature,
            &changed_properties,
            &neutral,
            &transforms,
            &results
        ));
    }
    assert!(!matches_source(
        source,
        &feature,
        &properties,
        &neutral,
        &[],
        &results
    ));
    let mut changed = feature.clone();
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
        feature.evaluation.definition()
    else {
        panic!("pattern");
    };
    let mut transform = pattern.definition().clone();
    let PatternTransform::Linear { count, .. } = &mut transform else {
        panic!("linear");
    };
    *count += 1;
    changed.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: seeds.clone(),
            pattern: PatternKind::new(transform).expect("valid synthetic pattern fixture"),
        }),
    );
    assert!(!matches_source(
        source,
        &changed,
        &properties,
        &neutral,
        &transforms,
        &results
    ));
    let mut changed_seeds = seeds.clone();
    changed_seeds.reverse();
    changed.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: changed_seeds,
            pattern: pattern.clone(),
        }),
    );
    assert!(!matches_source(
        source,
        &changed,
        &properties,
        &neutral,
        &transforms,
        &results
    ));
    Some(feature)
}

#[test]
fn rectangular_patterns_keep_exact_seeds_and_both_directions() {
    for version in [19, 23, 25, 29, 30] {
        for second in [false, true] {
            let feature = project_fixture(&pattern(version, second), |_, _, _| {})
                .expect("valid synthetic pattern fixture");
            let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
                feature.evaluation.definition()
            else {
                panic!("pattern");
            };
            assert_eq!(
                seeds,
                &[
                    PatternSeed::Feature(
                        cadmpeg_ir::features::FeatureId::mint(
                            "inventor:design:feature#generated-2"
                        )
                        .expect("valid synthetic pattern fixture")
                    ),
                    PatternSeed::Feature(
                        cadmpeg_ir::features::FeatureId::mint(
                            "inventor:design:feature#generated-7"
                        )
                        .expect("valid synthetic pattern fixture")
                    )
                ]
            );
            let PatternTransform::Linear {
                direction,
                spacing,
                count,
                second: actual,
            } = pattern.definition()
            else {
                panic!("linear pattern");
            };
            assert_eq!(*count, 3);
            assert_eq!(spacing.get(), 6.0);
            assert_eq!(
                direction.expect("valid synthetic pattern fixture"),
                Vector3::new(1.0, 0.0, 0.0)
            );
            assert_eq!(actual.is_some(), second);
            if let Some(actual) = actual {
                assert_eq!(actual.count, 2);
                assert_eq!(actual.spacing.get(), 4.0);
                assert_eq!(actual.direction, Vector3::new(0.0, -1.0, 0.0));
            }
        }
    }
}

#[test]
fn rectangular_patterns_withhold_incomplete_or_unrepresentable_operands() {
    assert!(project_fixture(&pattern(19, false), |operands, _, _| {
        operands.records.insert(
            ("generated".into(), 81),
            Some(Operand::Suppression(vec![[2, 1]])),
        );
    })
    .is_none());
    assert!(project_fixture(&pattern(19, false), |operands, _, _| {
        operands.records.insert(
            ("generated".into(), 83),
            Some(Operand::SelectionControl {
                references: [reference(96), reference(0)],
                value: 1,
            }),
        );
    })
    .is_none());
    assert!(project_fixture(&pattern(19, false), |_, history, _| {
        history
            .placements
            .get_mut(&7)
            .expect("valid synthetic pattern fixture")
            .ordinal = 10;
    })
    .is_none());
    assert!(project_fixture(&pattern(19, false), |_, history, _| {
        history
            .placements
            .get_mut(&7)
            .expect("valid synthetic pattern fixture")
            .model_history = false;
    })
    .is_none());
    assert!(project_fixture(&pattern(19, false), |_, _, properties| {
        properties.retain(|record| record.identity.record_ordinal != 86);
    })
    .is_none());
    assert!(project_fixture(&pattern(19, true), |operands, _, _| {
        operands.records.remove(&("generated".into(), 89));
    })
    .is_none());
    assert!(project_fixture(&pattern(19, false), |operands, _, _| {
        operands.raw.clear();
    })
    .is_none());
    let source = pattern(19, false);
    let mut payload = (*source).clone();
    payload.participants = reference_list(&[3, 3]);
    let duplicate = Located::new(payload, source.identity.type_id.clone(), segment(), 20);
    assert!(project_fixture(&duplicate, |_, _, _| {}).is_none());
    let mut payload = (*source).clone();
    payload.property_slots[8] = reference(96);
    let unsupported = Located::new(payload, source.identity.type_id.clone(), segment(), 20);
    assert!(project_fixture(&unsupported, |_, _, _| {}).is_none());
}

#[test]
fn rectangular_patterns_require_typed_dimensions_and_unique_operands() {
    for version in [19, 25] {
        assert!(
            project_fixture(&pattern(version, false), |_, _, properties| {
                properties.retain(|record| record.identity.record_ordinal != 84);
            })
            .is_none()
        );
        assert!(
            project_fixture(&pattern(version, false), |_, _, properties| {
                properties.retain(|record| record.identity.record_ordinal != 84);
                properties.push(test_property(
                    84,
                    PmDcFeaturePropertyKind::References {
                        family: super::super::PmDcFeatureReferenceFamily::ObjectCollection,
                        items: reference_list(&[]),
                    },
                ));
            })
            .is_none()
        );
        for (ordinal, type_id) in [
            (81, SUPPRESSION_TYPE),
            (83, SELECTION_CONTROL_TYPE),
            (87, LINE_DIRECTION_TYPE),
        ] {
            assert!(project_fixture(&pattern(version, false), |operands, _, _| {
                operands.note_record("generated", ordinal, type_id);
            })
            .is_none());
        }
    }
}

#[test]
fn later_rectangular_patterns_keep_additional_operands_native() {
    for version in [21, 25, 29, 30] {
        for number in 12..18 {
            let source = pattern(version, false);
            let mut payload = (*source).clone();
            payload.property_slots[number] = reference(91);
            let source = Located::new(payload, source.identity.type_id.clone(), segment(), 20);
            assert!(project_fixture(&source, |_, _, _| {}).is_none());
        }
    }
}

#[test]
fn rectangular_patterns_require_unambiguous_model_history_seeds() {
    assert!(project_fixture(&pattern(19, false), |_, history, _| {
        for placement in history.placements.values_mut() {
            placement.model_history = false;
        }
    })
    .is_none());
}

#[test]
fn rectangular_pattern_grid_counts_require_a_bounded_nontrivial_grid() {
    for counts in [[0, 2], [1, 1], [u32::MAX, 2]] {
        assert!(project_fixture(&pattern(19, true), |_, _, properties| {
            for (ordinal, count) in [(86, counts[0]), (88, counts[1])] {
                properties.retain(|record| record.identity.record_ordinal != ordinal);
                properties.push(test_property(
                    ordinal,
                    PmDcFeaturePropertyKind::RdxVariable {
                        name: String::new(),
                        name_value: 0,
                        nominal_value: count,
                        model_value: count,
                    },
                ));
            }
        })
        .is_none());
    }
}

#[test]
fn rectangular_pattern_direction_controls_keep_unsettled_modes_native() {
    for version in [19, 25] {
        let base = if version >= 21 { 18 } else { 12 };
        for axis in 0..2 {
            for offset in [6, 8, 10, 12] {
                let source = pattern(version, true);
                let mut payload = (*source).clone();
                payload.property_slots[base + offset + axis] = reference(99);
                let source = Located::new(payload, source.identity.type_id.clone(), segment(), 20);
                assert!(project_fixture(&source, |_, _, properties| {
                    properties.push(test_property(
                        98,
                        PmDcFeaturePropertyKind::Boolean {
                            name: String::new(),
                            name_value: 0,
                            value: true,
                        },
                    ));
                })
                .is_none());
            }
        }
    }
}

#[test]
fn rectangular_pattern_body_mode_keeps_its_body_selection_native() {
    let source = pattern(19, false);
    let mut payload = (*source).clone();
    payload.participants = reference_list(&[]);
    payload.property_slots[6] = reference(99);
    payload.property_slots[10] = reference(86);
    let source = Located::new(payload, source.identity.type_id.clone(), segment(), 20);
    assert!(project_fixture(&source, |_, _, properties| {
        properties.push(test_property(
            98,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ));
    })
    .is_none());
}

#[test]
fn a_work_axis_direction_uses_the_shared_version_25_grammar() {
    let mut bytes = content_at(7, 25, &[-1, 3]);
    for value in [0u32; 4] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0x203u32.to_le_bytes());
    for value in [0.0, 0.0, 0.0, 0.0, 0.0, -4.0] {
        bytes.extend_from_slice(&f64::to_le_bytes(value));
    }
    bytes.push(0);
    let axis = with_bytes(&bytes, |ctx, source| {
        super::super::parse_work_axis(ctx, source, 25)
    })
    .expect("valid synthetic pattern fixture");
    let properties = [Located::new(
        axis,
        RecordTypeId::from_bytes(WORK_AXIS_TYPE),
        segment(),
        87,
    )];
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    let source = pattern(25, false);
    let mut operands = Operands::default();
    assert!(direction(&source, 22, &index, &operands).is_none());
    operands.note_record("generated", 87, WORK_AXIS_TYPE);
    let actual =
        direction(&source, 22, &index, &operands).expect("valid synthetic pattern fixture");
    assert_eq!(actual, Vector3::new(0.0, 0.0, -1.0));
    operands.note_record("generated", 87, WORK_AXIS_TYPE);
    assert!(direction(&source, 22, &index, &operands).is_none());
    let mut cached_axis = properties[0].clone();
    cached_axis.identity.record_ordinal = 91;
    let feature = project_fixture(&source, |operands, _, properties| {
        operands
            .raw
            .insert(("generated".into(), 91), Some(WORK_AXIS_TYPE));
        properties.push(cached_axis);
    })
    .expect("valid synthetic pattern fixture");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
        feature.evaluation.definition()
    else {
        panic!("pattern");
    };
    let PatternTransform::Linear { direction, .. } = pattern.definition() else {
        panic!("linear");
    };
    assert_eq!(
        direction.expect("valid synthetic pattern fixture"),
        Vector3::new(0.0, 0.0, -1.0)
    );
}

#[test]
fn rectangular_pattern_direction_reversal_comes_from_its_selection() {
    let feature = project_fixture(&pattern(19, false), |operands, _, properties| {
        let Some(Some(Operand::Line { reverse, .. })) =
            operands.records.get_mut(&("generated".into(), 87))
        else {
            panic!("line selection");
        };
        *reverse = reference(98);
        properties.push(test_property(
            97,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ));
    })
    .expect("valid synthetic pattern fixture");
    let FeatureDefinition::Operation(FeatureOperation::Pattern {
        pattern: construction,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("pattern");
    };
    let PatternTransform::Linear { direction, .. } = construction.definition() else {
        panic!("linear");
    };
    assert_eq!(
        direction.expect("valid synthetic pattern fixture"),
        Vector3::new(-1.0, 0.0, 0.0)
    );
    assert!(project_fixture(&pattern(19, false), |operands, _, _| {
        operands
            .raw
            .insert(("generated".into(), 91), Some(WORK_AXIS_TYPE));
    })
    .is_none());
}

#[test]
fn suppression_coordinates_reject_zero_seed_and_repetition() {
    for pairs in [vec![[0, 2]], vec![[1, 1]], vec![[2, 1], [2, 1]]] {
        let bytes = suppression(19, &pairs);
        assert!(with_bytes(&bytes, |ctx, source| parse(
            ctx,
            SUPPRESSION_TYPE,
            source,
            19
        ))
        .is_err());
    }
}

#[test]
fn suppression_coordinates_must_fit_both_instance_counts() {
    let count = |ordinal, value| {
        test_property(
            ordinal,
            PmDcFeaturePropertyKind::RdxVariable {
                name: String::new(),
                name_value: 0,
                nominal_value: value,
                model_value: value,
            },
        )
    };
    let properties = [count(86, 3), count(88, 2)];
    for (pair, valid) in [([3, 2], true), ([4, 2], false), ([3, 3], false)] {
        let mut operands = Operands::default();
        operands.note_record("generated", 81, SUPPRESSION_TYPE);
        operands.records.insert(
            ("generated".into(), 81),
            Some(Operand::Suppression(vec![pair])),
        );
        assert_eq!(
            operands
                .validate_suppression(&pattern(19, true), &properties)
                .is_ok(),
            valid
        );
    }
    let mut operands = Operands::default();
    operands.note_record("generated", 81, SUPPRESSION_TYPE);
    operands.records.insert(
        ("generated".into(), 81),
        Some(Operand::Suppression(vec![[2, 2]])),
    );
    assert!(operands
        .validate_suppression(&pattern(19, false), &properties)
        .is_err());
}

#[test]
fn suppression_validation_requires_positive_resolved_counts_even_for_an_empty_list() {
    let count = |ordinal, value| {
        test_property(
            ordinal,
            PmDcFeaturePropertyKind::RdxVariable {
                name: String::new(),
                name_value: 0,
                nominal_value: value,
                model_value: value,
            },
        )
    };
    for pairs in [vec![], vec![[2, 1]]] {
        let bytes = suppression(19, &pairs);
        let mut operands = Operands::default();
        operands.note_record("generated", 81, SUPPRESSION_TYPE);
        with_bytes(&bytes, |ctx, source| {
            operands.read(ctx, "generated", 81, SUPPRESSION_TYPE, source, 19)
        })
        .expect("valid synthetic suppression record");
        let source = pattern(19, true);
        assert!(operands
            .validate_suppression(&source, &[count(86, 3), count(88, 2)])
            .is_ok());
        for properties in [
            vec![],
            vec![count(86, 0), count(88, 2)],
            vec![count(86, 3), count(88, 0)],
            vec![count(86, 3)],
            vec![count(86, 3), count(86, 3), count(88, 2)],
        ] {
            assert!(operands.validate_suppression(&source, &properties).is_err());
        }
    }
}
