// SPDX-License-Identifier: Apache-2.0
use super::*;

fn mirror_record(participants: &[u32], slots: [u32; 13]) -> crate::feature::PmDcPatternFeature {
    Located::new(
        crate::feature::PmDcPatternFeaturePayload {
            save_version_major: 19,
            header: test_header(),
            state: 73,
            outline_value: 1,
            properties: reference_list(&[]),
            value: 0,
            participants: reference_list(participants),
            family: PmDcPatternFamily::Mirror,
            property_slots: slots.into_iter().map(reference).collect(),
            control: 0,
            extension_values: Vec::new(),
        },
        crate::record_identity::RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        120,
    )
}

fn identity_transform(ordinal: u32, translation: f64) -> crate::sketch::PmDcTransform {
    Located::new(
        crate::sketch::PmDcTransformPayload {
            save_version_major: 19,
            header: test_header(),
            prefix_present: false,
            matrix: crate::compact_matrix::CompactMatrix::try_from_rows(
                0,
                0,
                [
                    [1.0, 0.0, 0.0, translation],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
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
        ordinal,
    )
}

fn yz_plane(ordinal: u32) -> PmDcWorkPlane {
    let real = |value: f64| cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite fixture");
    Located::new(
        PmDcWorkPlanePayload {
            save_version_major: 19,
            header: test_header(),
            entity_flags: 0,
            values: vec![0; 3],
            marker: false,
            origin: [real(1.5), real(0.0), real(0.0)],
            u_axis: [real(0.0), real(1.0), real(0.0)],
            v_axis: [real(0.0), real(0.0), real(1.0)],
        },
        crate::record_identity::RecordTypeId::from_bytes(WORK_PLANE_TYPE),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        ordinal,
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

/// Slots: 0 transform #61, 6/7/11 Booleans #3/#4/#5, 9 result #6, 12 plane #41.
fn project_generated_mirror(
    participants: &[u32],
    first_boolean: bool,
    translation: f64,
    slot_8: u32,
) -> Option<Feature> {
    let record = mirror_record(participants, [61, 0, 0, 0, 0, 0, 3, 4, slot_8, 6, 0, 5, 41]);
    let properties = vec![
        boolean_property(2, first_boolean),
        boolean_property(3, false),
        boolean_property(4, false),
        test_property(
            5,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[8]),
            },
        ),
        test_property(
            7,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
    ];
    let transform = identity_transform(60, translation);
    let plane = yz_plane(40);
    let mut index = test_projection_index(
        &properties,
        &[],
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&transform),
        &[],
    );
    index.work_planes.insert(("generated", 40), Some(&plane));
    let label = test_label(120, 8, crate::feature::MIRROR_CLASS_ID, &[]);
    let mut history = crate::feature::history::History::default();
    for (ordinal, kind) in [
        (10, crate::feature::history::HistoryNodeKind::Feature),
        (11, crate::feature::history::HistoryNodeKind::Sketch),
        (120, crate::feature::history::HistoryNodeKind::Pattern),
    ] {
        history.placements.insert(
            ordinal,
            crate::feature::history::HistoryPlacement {
                ordinal: u64::from(ordinal),
                kind,
                below_end_of_part: false,
                model_history: true,
            },
        );
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    crate::feature::project_mirror(&ctx, &record, Some(&label), &index, &history)
        .map(|projected| projected.expect("mirror projection").0)
}

#[test]
fn feature_mirror_reflects_seed_features_across_the_stored_plane() {
    let mirror = project_generated_mirror(&[11], false, 0.0, 0).expect("feature mirror");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
        mirror.evaluation.definition()
    else {
        panic!("not a pattern");
    };
    assert_eq!(seeds.len(), 1);
    assert!(matches!(
        &seeds[0],
        cadmpeg_ir::features::patterns::PatternSeed::Feature(id)
            if id.as_str() == "inventor:design:feature#generated-10"
    ));
    assert!(matches!(
        pattern.definition(),
        cadmpeg_ir::features::patterns::PatternTransform::Mirror {
            plane_origin,
            plane_normal,
        } if plane_origin.get() == Point3::new(15.0, 0.0, 0.0)
            && plane_normal.get() == Vector3::new(1.0, 0.0, 0.0)
    ));
    assert_eq!(mirror.source_tag.as_deref(), Some("mirror"));
}

#[test]
fn mirror_outside_the_feature_branch_stays_native() {
    assert!(project_generated_mirror(&[], false, 0.0, 0).is_none());
    assert!(project_generated_mirror(&[12], false, 0.0, 0).is_none());
    assert!(project_generated_mirror(&[11], true, 0.0, 0).is_none());
    assert!(project_generated_mirror(&[11], false, 0.5, 0).is_none());
    assert!(project_generated_mirror(&[11], false, 0.0, 3).is_none());
}
