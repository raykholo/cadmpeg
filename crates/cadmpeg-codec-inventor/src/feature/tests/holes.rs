// SPDX-License-Identifier: Apache-2.0
use super::*;

fn sketch_point(ordinal: u32, sketch: u32, position: [f64; 2]) -> crate::sketch::PmDcSketchEntity {
    serde_json::from_value(serde_json::json!({
        "id": format!("inventor:pmdc:sketch-entity#generated-{ordinal}"),
        "entity_flags": 0,
        "header": {
            "context": {"index": 0, "qualified": false},
            "flags": 0,
            "header_id": 0,
            "header_value": 0,
            "next": {"index": 0, "qualified": false},
            "source_index": ordinal
        },
        "kind": {
            "form": "point",
            "position": position,
            "endpoint_of": {"marker": 2, "metadata": null, "references": []},
            "center_of": {"marker": 2, "metadata": null, "references": []}
        },
        "record_ordinal": ordinal,
        "save_version_major": 19,
        "segment_token": "generated",
        "sketch": {"index": sketch, "qualified": true},
        "type_id": "35df52ced011d0d20008ccbc0663dc09"
    }))
    .expect("sketch point fixture")
}

fn sketch_record(transform: u32, direction: u32, entities: &[u32]) -> crate::sketch::PmDcSketch {
    Located::new(
        crate::sketch::PmDcSketchPayload {
            save_version_major: 19,
            header: test_header(),
            state: 0,
            count_value: 0,
            entities: PmDcReferenceList::new(
                8,
                (!entities.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U16([
                    u16::try_from(entities.len()).expect("fixture count fits u16"),
                    0,
                ])),
                entities.iter().copied().map(reference).collect(),
            )
            .expect("entity list"),
            transform: reference(transform),
            direction: reference(direction),
            values: [0; 2],
            auxiliary: None,
            block: None,
        },
        crate::record_identity::RecordTypeId::try_from(
            "114d8790d011f8d10008cabc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        50,
    )
}

fn project_sketch_hole(
    sketch: &crate::sketch::PmDcSketch,
    centers: &[u32],
    slot_13: bool,
) -> Option<Feature> {
    let raw_parameters = (70..76).map(raw_parameter).collect::<Vec<_>>();
    let neutral_parameters = [
        ParameterValue::Length(Length::new(4.0).expect("finite length fixture")),
        ParameterValue::Length(Length::new(20.0).expect("finite length fixture")),
        ParameterValue::Length(Length::new(9.0).expect("finite length fixture")),
        ParameterValue::Length(Length::new(3.0).expect("finite length fixture")),
        ParameterValue::Angle(Angle::new(1.5).expect("finite angle fixture")),
        ParameterValue::Angle(Angle::new(0.0).expect("finite angle fixture")),
    ]
    .into_iter()
    .zip(&raw_parameters)
    .map(|(value, raw)| neutral_parameter(raw, value))
    .collect::<Vec<_>>();
    let transform = Located::new(
        crate::sketch::PmDcTransformPayload {
            save_version_major: 19,
            header: test_header(),
            prefix_present: false,
            matrix: crate::compact_matrix::CompactMatrix::try_from_rows(
                0,
                0,
                [
                    [0.0, -1.0, 0.0, 1.0],
                    [1.0, 0.0, 0.0, 2.0],
                    [0.0, 0.0, 1.0, 3.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            )
            .expect("finite explicit matrix fixture"),
        },
        crate::record_identity::RecordTypeId::try_from(
            "184d8790d011f8d10008cabc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        60,
    );
    let direction = Located::new(
        crate::sketch::PmDcDirectionPayload {
            save_version_major: 19,
            header: test_header(),
            entity_flags: 0,
            parameter: cadmpeg_ir::scalar::FiniteReal::ZERO,
            extension: None,
            direction: [
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ONE,
            ],
        },
        crate::record_identity::RecordTypeId::try_from(
            "40df52ced011d0d20008ccbc0663dc09".to_owned(),
        )
        .expect("test GUID"),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        61,
    );
    let points = vec![
        sketch_point(80, 51, [0.5, 0.0]),
        sketch_point(81, 51, [0.0, 0.25]),
        sketch_point(82, 52, [0.0, 0.0]),
    ];
    let properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Hole,
                type_value: 3,
                value: 0,
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Extent,
                type_value: 11,
                value: 1,
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::HoleCenters,
                items: reference_list(centers),
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[7]),
            },
        ),
        test_property(
            6,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
        test_property(9, PmDcFeaturePropertyKind::TerminationFlag { value: 1 }),
    ];
    let mut slots = vec![
        (0, 1),
        (1, 70),
        (2, 71),
        (3, 72),
        (4, 73),
        (5, 74),
        (6, 75),
        (7, 4),
        (8, 60),
        (9, 2),
        (16, 61),
        (17, 3),
        (24, 5),
    ];
    if slot_13 {
        slots.push((13, 9));
    }
    let feature = test_feature(100, 26, &slots);
    let label = test_label(100, 9, HOLE_CLASS_ID, &[51]);
    let mut index = test_projection_index(
        &properties,
        &raw_parameters,
        &neutral_parameters,
        std::slice::from_ref(sketch),
        &[],
        std::slice::from_ref(&direction),
        std::slice::from_ref(&transform),
        &[],
    );
    for point in &points {
        index.sketch_entities.insert(
            (
                point.identity.segment_token.as_str(),
                point.identity.record_ordinal,
            ),
            Some(point),
        );
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    project_hole(&ctx, &feature, &label, &index)
        .map(|projected| projected.expect("hole projection").0)
}

#[test]
fn sketch_point_hole_places_each_center_through_the_sketch_transform() {
    let hole = project_sketch_hole(&sketch_record(61, 62, &[81, 82]), &[81, 82], false)
        .expect("sketch-point hole");
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        placements: Some(placements),
        profile,
        extent: Some(LinearTermination::Blind { length }),
        ..
    }) = hole.evaluation.definition()
    else {
        panic!("not a placed hole");
    };
    assert!(profile.is_none());
    assert_eq!(length.get(), 20.0);
    let positions = placements
        .iter()
        .map(|placement| match placement {
            HolePlacement::Directed {
                position,
                direction,
            } => (position.get(), direction.get()),
            HolePlacement::Axis { .. } => panic!("axis placement"),
        })
        .collect::<Vec<_>>();
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[0].0, Point3::new(10.0, 25.0, 30.0));
    assert_eq!(positions[1].0, Point3::new(7.5, 20.0, 30.0));
    assert!(positions
        .iter()
        .all(|(_, direction)| direction.z == -1.0 && direction.x == 0.0));
}

#[test]
fn sketch_point_hole_refuses_foreign_points_and_targets() {
    assert!(project_sketch_hole(&sketch_record(61, 62, &[81, 82, 83]), &[83], false).is_none());
    assert!(project_sketch_hole(&sketch_record(61, 62, &[81]), &[82], false).is_none());
    assert!(project_sketch_hole(&sketch_record(61, 63, &[81, 82]), &[81, 82], false).is_none());
    assert!(project_sketch_hole(&sketch_record(64, 62, &[81, 82]), &[81, 82], false).is_none());
    assert!(project_sketch_hole(&sketch_record(61, 62, &[81, 82]), &[81, 82], true).is_none());
    assert!(project_sketch_hole(&sketch_record(61, 62, &[81, 82]), &[], false).is_none());
}
