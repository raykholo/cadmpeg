// SPDX-License-Identifier: Apache-2.0
use super::*;

fn enumeration(
    ordinal: u32,
    family: PmDcFeatureEnumFamily,
    type_value: i16,
    value: u16,
) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::Enumeration {
            family,
            type_value,
            value,
        },
    )
}

fn boolean_property(ordinal: u32, value: bool) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value,
        },
    )
}

fn list(ordinal: u32, family: PmDcFeatureReferenceFamily, items: &[u32]) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::References {
            family,
            items: reference_list(items),
        },
    )
}

fn body(ordinal: u32) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::SurfaceBody {
            body: reference(30),
        },
    )
}

fn context() -> (DecodeArena, DecodePolicy) {
    (DecodeArena::new(), DecodePolicy::service())
}

#[test]
fn combine_joins_tool_bodies_into_the_base_body() {
    let properties = vec![
        list(1, PmDcFeatureReferenceFamily::BodyList, &[11]),
        list(2, PmDcFeatureReferenceFamily::ObjectCollection, &[10]),
        enumeration(3, PmDcFeatureEnumFamily::PartOperation, 5, 3),
        boolean_property(4, false),
        body(9),
        body(10),
    ];
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    let (arena, policy) = context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let source = test_feature(100, 4, &[(0, 1), (1, 2), (2, 3), (3, 4)]);
    let label = test_label(100, 3, crate::feature::COMBINE_CLASS_ID, &[]);
    let (combine, result) = crate::feature::project_combine(&ctx, &source, &label, &index)
        .expect("combine candidate")
        .expect("combine projection");
    let FeatureDefinition::Operation(FeatureOperation::Combine {
        operands,
        op: cadmpeg_ir::features::BooleanKind::Join,
        keep_tools: false,
    }) = combine.evaluation.definition()
    else {
        panic!("not a join combine");
    };
    assert!(matches!(
        operands.target(),
        cadmpeg_ir::features::BodySelection::Native(native) if native == &properties[1].id()
    ));
    assert!(matches!(
        operands.tools(),
        cadmpeg_ir::features::BodySelection::Native(native) if native == &properties[0].id()
    ));
    assert_eq!(result.bodies().len(), 1);

    let wide = vec![
        list(1, PmDcFeatureReferenceFamily::BodyList, &[11]),
        list(2, PmDcFeatureReferenceFamily::ObjectCollection, &[10, 11]),
        enumeration(3, PmDcFeatureEnumFamily::PartOperation, 5, 3),
        boolean_property(4, false),
        body(9),
        body(10),
    ];
    let index = test_projection_index(&wide, &[], &[], &[], &[], &[], &[], &[]);
    assert!(crate::feature::project_combine(&ctx, &source, &label, &index).is_none());
}

#[test]
fn solid_split_partitions_targets_at_a_work_plane() {
    let properties = vec![
        enumeration(1, PmDcFeatureEnumFamily::SplitForm, 3, 3),
        boolean_property(2, false),
        enumeration(3, PmDcFeatureEnumFamily::SplitSide, 4, 2),
        list(4, PmDcFeatureReferenceFamily::BoundaryPatch, &[]),
        boolean_property(6, false),
        list(7, PmDcFeatureReferenceFamily::BodyList, &[11]),
        list(8, PmDcFeatureReferenceFamily::ObjectCollection, &[10, 11]),
        body(9),
        body(10),
    ];
    let plane = Located::new(
        PmDcWorkPlanePayload {
            save_version_major: 19,
            header: test_header(),
            entity_flags: 0,
            values: vec![0; 3],
            marker: false,
            origin: [cadmpeg_ir::scalar::FiniteReal::ZERO; 3],
            u_axis: [
                cadmpeg_ir::scalar::FiniteReal::ONE,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
            ],
            v_axis: [
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ONE,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
            ],
        },
        crate::record_identity::RecordTypeId::from_bytes(WORK_PLANE_TYPE),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        40,
    );
    let mut index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    index.work_planes.insert(("generated", 40), Some(&plane));
    let (arena, policy) = context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let source = test_feature(
        100,
        10,
        &[
            (0, 1),
            (1, 2),
            (3, 3),
            (4, 4),
            (5, 40),
            (6, 6),
            (7, 7),
            (9, 8),
        ],
    );
    let label = test_label(100, 3, crate::feature::SPLIT_CLASS_ID, &[]);
    let (split, result) = crate::feature::project_split(&ctx, &source, &label, &index)
        .expect("split candidate")
        .expect("split projection");
    assert!(matches!(
        split.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: cadmpeg_ir::features::BodySelection::Native(targets),
            tools: cadmpeg_ir::features::FaceSelection::Native(tools),
        }) if targets == &properties[5].id() && tools == &plane.id()
    ));
    assert_eq!(result.bodies().len(), 2);
    let face_split = test_feature(
        100,
        10,
        &[
            (0, 1),
            (1, 2),
            (3, 3),
            (4, 4),
            (5, 40),
            (6, 6),
            (7, 7),
            (9, 8),
            (2, 6),
        ],
    );
    assert!(crate::feature::project_split(&ctx, &face_split, &label, &index).is_none());
}

#[test]
fn thicken_joins_material_with_an_unresolved_side() {
    let raw = raw_parameter(70);
    let neutral = neutral_parameter(
        &raw,
        ParameterValue::Length(Length::new(1.03).expect("finite length fixture")),
    );
    let mut properties = vec![
        list(1, PmDcFeatureReferenceFamily::FaceCollection, &[20]),
        enumeration(2, PmDcFeatureEnumFamily::PartOperation, 5, 3),
        boolean_property(3, true),
        boolean_property(4, false),
        boolean_property(5, true),
        enumeration(6, PmDcFeatureEnumFamily::ThickenForm, 3, 3),
        test_property(7, PmDcFeaturePropertyKind::Value { value: 1 }),
        list(8, PmDcFeatureReferenceFamily::ObjectCollection, &[10]),
        body(9),
    ];
    properties.push(body(10));
    let slots = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 4),
        (4, 4),
        (6, 70),
        (7, 4),
        (10, 4),
        (13, 6),
        (14, 7),
        (15, 8),
        (16, 5),
        (17, 4),
    ];
    let index = test_projection_index(
        &properties,
        std::slice::from_ref(&raw),
        std::slice::from_ref(&neutral),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let (arena, policy) = context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let label = test_label(100, 3, crate::feature::THICKEN_CLASS_ID, &[]);
    let (thicken, _) =
        crate::feature::project_thicken(&ctx, &test_feature(100, 18, &slots), &label, &index)
            .expect("thicken candidate")
            .expect("thicken projection");
    assert!(matches!(
        thicken.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces: cadmpeg_ir::features::FaceSelection::Native(_),
            thickness: Some(thickness),
            side: None,
        }) if (thickness.get() - 1.03).abs() < 1e-12
    ));
    assert_eq!(
        thicken
            .source_properties
            .get("property_2_boolean")
            .map(String::as_str),
        Some("true")
    );
    let mut cut = slots.to_vec();
    cut[1] = (1, 6);
    assert!(
        crate::feature::project_thicken(&ctx, &test_feature(100, 18, &cut), &label, &index)
            .is_none()
    );
}

#[test]
fn body_move_translates_by_its_sole_transform() {
    let properties = vec![
        list(1, PmDcFeatureReferenceFamily::ObjectCollection, &[10]),
        list(2, PmDcFeatureReferenceFamily::TransformList, &[61]),
        list(3, PmDcFeatureReferenceFamily::FeatureDimensions, &[]),
        body(9),
    ];
    let transform = |rows: [[f64; 4]; 4]| {
        Located::new(
            crate::sketch::PmDcTransformPayload {
                save_version_major: 19,
                header: test_header(),
                prefix_present: false,
                matrix: crate::compact_matrix::CompactMatrix::try_from_rows(0, 0, rows)
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
        )
    };
    let (arena, policy) = context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let source = test_feature(100, 3, &[(0, 1), (1, 2), (2, 3)]);
    let label = test_label(100, 3, crate::feature::MOVE_CLASS_ID, &[]);
    let translated = transform([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.25],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    let index = test_projection_index(
        &properties,
        &[],
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&translated),
        &[],
    );
    let (moved, _) = crate::feature::project_move(&ctx, &source, &label, &index)
        .expect("move candidate")
        .expect("move projection");
    assert!(matches!(
        moved.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::MoveBody {
            translation,
            rotation: None,
            copies: 0,
            ..
        }) if translation.get() == Vector3::new(0.0, 2.5, 0.0)
    ));
    let rotated = transform([
        [0.0, -1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.25],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    let index = test_projection_index(
        &properties,
        &[],
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&rotated),
        &[],
    );
    assert!(crate::feature::project_move(&ctx, &source, &label, &index).is_none());
}
