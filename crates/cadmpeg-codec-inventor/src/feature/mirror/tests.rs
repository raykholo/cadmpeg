// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::tests::{reference, reference_list, segment, test_header, test_label};
use crate::feature::{
    parse_pattern_feature, PmDcFeaturePropertyPayload, PmDcPatternFeaturePayload,
    PmDcWorkPlanePayload, MIRROR_FEATURE_TYPE, WORK_PLANE_TYPE,
};
use crate::record_identity::{Located, RecordTypeId};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
use cadmpeg_ir::scalar::FiniteReal;

fn pattern(
    version: u8,
    body: bool,
    seeds: &[u32],
    overrides: &[(usize, u32)],
) -> PmDcPatternFeature {
    let mut slots = vec![reference(0); 13];
    for (slot, ordinal) in [(0, 100), (2, 107), (5, 110), (6, 101), (7, 102), (12, 103)] {
        slots[slot] = reference(ordinal + 1);
    }
    slots[if body { 10 } else { 9 }] = reference(105);
    for &(slot, packed) in overrides {
        slots[slot] = reference(packed);
    }
    Located::new(
        PmDcPatternFeaturePayload {
            save_version_major: version,
            header: test_header(),
            state: 0,
            outline_value: 0,
            properties: reference_list(&[]),
            value: 0,
            participants: reference_list(seeds),
            family: PmDcPatternFamily::Mirror,
            property_slots: slots,
            control: 0,
            extension_values: vec![
                0;
                if version >= 29 {
                    14
                } else if version >= 21 {
                    6
                } else {
                    0
                }
            ],
        },
        RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE),
        segment(),
        20,
    )
}

fn property(ordinal: u32, kind: PmDcFeaturePropertyKind) -> PmDcFeatureProperty {
    let type_id = match &kind {
        PmDcFeaturePropertyKind::Boolean { .. } => super::super::BOOLEAN_TYPE,
        PmDcFeaturePropertyKind::References { family, .. } => match family {
            PmDcFeatureReferenceFamily::ObjectCollection => super::super::OBJECT_COLLECTION_TYPE,
            PmDcFeatureReferenceFamily::FeatureDimensions => super::super::FEATURE_DIMENSIONS_TYPE,
            _ => panic!("fixture reference family"),
        },
        PmDcFeaturePropertyKind::SurfaceBody { .. } => super::super::SURFACE_BODY_TYPE,
        PmDcFeaturePropertyKind::RdxVariable { .. } => super::super::RDX_VARIABLE_TYPE,
        _ => panic!("fixture property kind"),
    };
    Located::new(
        PmDcFeaturePropertyPayload {
            save_version_major: 19,
            header: test_header(),
            kind,
        },
        RecordTypeId::from_bytes(type_id),
        segment(),
        ordinal,
    )
}

struct Fixture {
    properties: Vec<PmDcFeatureProperty>,
    transforms: Vec<PmDcTransform>,
    planes: Vec<PmDcWorkPlane>,
    label: PmDcFeatureLabel,
}

impl Fixture {
    fn new(body: bool) -> Self {
        let properties = vec![
            property(
                101,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: body,
                },
            ),
            property(
                102,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: false,
                },
            ),
            property(
                104,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[106]),
                },
            ),
            property(
                105,
                PmDcFeaturePropertyKind::SurfaceBody { body: reference(1) },
            ),
            property(
                107,
                PmDcFeaturePropertyKind::RdxVariable {
                    name: String::new(),
                    name_value: 0,
                    nominal_value: 0,
                    model_value: 0,
                },
            ),
            property(
                110,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::FeatureDimensions,
                    items: reference_list(&[]),
                },
            ),
        ];
        let transforms = vec![Located::new(
            crate::sketch::PmDcTransformPayload {
                save_version_major: 19,
                header: test_header(),
                prefix_present: false,
                matrix: crate::compact_matrix::CompactMatrix::try_from_rows(
                    0x8421, 0x7bde, IDENTITY,
                )
                .expect("identity compact matrix"),
            },
            RecordTypeId::from_bytes(crate::pmdc::inventor_id(0x9087_4d18)),
            segment(),
            100,
        )];
        let planes = vec![Located::new(
            PmDcWorkPlanePayload {
                save_version_major: 19,
                header: test_header(),
                entity_flags: 0,
                values: vec![0; 3],
                marker: false,
                origin: [
                    FiniteReal::new(2.0).expect("finite"),
                    FiniteReal::ZERO,
                    FiniteReal::ZERO,
                ],
                u_axis: [FiniteReal::ZERO, FiniteReal::ONE, FiniteReal::ZERO],
                v_axis: [FiniteReal::ZERO, FiniteReal::ZERO, FiniteReal::ONE],
            },
            RecordTypeId::from_bytes(WORK_PLANE_TYPE),
            segment(),
            103,
        )];
        let class =
            ClassId::try_from("a326c2b6d211263660002cab01f31bb0".to_owned()).expect("mirror class");
        Self {
            properties,
            transforms,
            planes,
            label: test_label(20, 2, class, &[]),
        }
    }

    fn index(&self) -> MirrorIndex<'_> {
        MirrorIndex::new(
            &self.properties,
            &self.transforms,
            &self.planes,
            &[],
            &[],
            std::iter::empty(),
        )
    }
}

fn seed(ordinal: u32) -> Option<FeatureId> {
    [2, 4].contains(&ordinal).then(|| {
        history_feature_id(&crate::feature::tests::test_feature(ordinal, 0, &[]).identity)
            .expect("feature id")
    })
}

#[test]
fn mirrors_keep_ordered_feature_seeds_and_reflect_about_the_offset_plane() {
    let fixture = Fixture::new(false);
    for version in [15, 16, 19, 20, 21, 22, 23, 25, 28, 29, 30, 31] {
        let source = pattern(version, false, &[5, 3], &[]);
        let resolved = fixture
            .index()
            .resolve(&source, &fixture.label, seed)
            .expect("feature mirror");
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
            resolved.definition
        else {
            panic!("pattern")
        };
        assert_eq!(
            seeds,
            vec![
                PatternSeed::Feature(seed(4).expect("seed")),
                PatternSeed::Feature(seed(2).expect("seed"))
            ]
        );
        let PatternTransform::Mirror {
            plane_origin,
            plane_normal,
        } = pattern.definition()
        else {
            panic!("reflection")
        };
        assert_eq!(plane_origin.get(), Point3::new(20.0, 0.0, 0.0));
        assert_eq!(plane_normal.get(), Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(
            resolved.members,
            FeatureResultMembers::new(
                vec![NonBlankString::new(fixture.properties[3].id()).expect("body id")],
                Vec::new(),
                Vec::new(),
                Vec::new()
            )
            .expect("members")
        );
    }
}

#[test]
fn body_mirrors_select_the_native_input_collection() {
    let fixture = Fixture::new(true);
    let source = pattern(19, true, &[], &[]);
    let resolved = fixture
        .index()
        .resolve(&source, &fixture.label, |_| None)
        .expect("body mirror");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = resolved.definition
    else {
        panic!("pattern")
    };
    assert_eq!(
        seeds,
        vec![PatternSeed::Bodies(BodySelection::Native(
            fixture.properties[2].id()
        ))]
    );
    assert_eq!(resolved.result_native, fixture.properties[2].id());
}

#[test]
fn mirrors_refuse_missing_duplicate_and_unordered_seed_bindings() {
    let fixture = Fixture::new(false);
    for seeds in [vec![], vec![0], vec![7], vec![3, 3]] {
        assert!(fixture
            .index()
            .resolve(&pattern(19, false, &seeds, &[]), &fixture.label, seed)
            .is_none());
    }
    assert!(fixture
        .index()
        .resolve(&pattern(19, false, &[3], &[]), &fixture.label, |_| None)
        .is_none());
    assert!(Fixture::new(true)
        .index()
        .resolve(&pattern(19, true, &[3], &[]), &fixture.label, seed)
        .is_none());
}

#[test]
fn mirror_seeds_precede_the_mirror_under_the_same_model_root() {
    use crate::feature::history::{History, HistoryNodeKind, HistoryPlacement};

    let source = crate::feature::tests::test_feature(2, 0, &[]);
    let mut history = History::default();
    let before = HistoryPlacement {
        ordinal: 1,
        kind: HistoryNodeKind::Feature,
        below_end_of_part: false,
        model_history: false,
        model_root: 30,
    };
    history.placements.insert(2, before);
    history.placements.insert(
        20,
        HistoryPlacement {
            ordinal: 2,
            kind: HistoryNodeKind::Pattern,
            ..before
        },
    );
    assert_eq!(seed_feature_id(&history, 20, &source.identity), seed(2));
    for invalid in [
        HistoryPlacement {
            ordinal: 2,
            ..before
        },
        HistoryPlacement {
            ordinal: 3,
            ..before
        },
        HistoryPlacement {
            model_root: 31,
            ..before
        },
        HistoryPlacement {
            kind: HistoryNodeKind::Sketch,
            ..before
        },
    ] {
        history.placements.insert(2, invalid);
        assert!(seed_feature_id(&history, 20, &source.identity).is_none());
    }
    history.placements.insert(2, before);
    let mut wrong_type = source.identity.clone();
    wrong_type.type_id = RecordTypeId::from_bytes(WORK_PLANE_TYPE);
    assert!(seed_feature_id(&history, 20, &wrong_type).is_none());
}

#[test]
fn mirrors_refuse_unclosed_results_and_missing_plane_or_transform() {
    let fixture = Fixture::new(false);
    for slot in [0, 2, 5, 6, 7, 9, 12] {
        assert!(fixture
            .index()
            .resolve(
                &pattern(19, false, &[3], &[(slot, 0)]),
                &fixture.label,
                seed
            )
            .is_none());
    }
    let mut duplicate = Fixture::new(false);
    duplicate.properties.push(duplicate.properties[2].clone());
    assert!(duplicate
        .index()
        .resolve(&pattern(19, false, &[3], &[]), &duplicate.label, seed)
        .is_none());
}

#[test]
fn mirror_projection_keeps_resource_refusals() {
    let fixture = Fixture::new(false);
    let source = pattern(19, false, &[3], &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert!(matches!(
        fixture.index().project(&ctx, &source, &fixture.label, seed),
        Some(Err(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::Entities
    ));
}

#[test]
fn mirror_result_and_plane_slots_do_not_shift_at_version_extensions() {
    for version in [15, 16, 19, 20, 21, 22, 23, 25, 28, 29, 30, 31] {
        let bytes =
            crate::feature::tests::pattern_feature_bytes(version, PmDcPatternFamily::Mirror);
        let parsed = parse_pattern_feature(
            cadmpeg_core::decode::View::over_retained(&bytes),
            version,
            PmDcPatternFamily::Mirror,
        )
        .expect("mirror carrier");
        assert_eq!(parsed.property_slots.len(), 13);
        for (slot, reference) in [(9, 0x43), (10, 0x44), (11, 0x50), (12, 0x51)] {
            assert_eq!(parsed.property_slots[slot].index(), reference);
        }
    }
}

#[test]
fn mirrors_refuse_suffix_shapes_that_disagree_with_the_version() {
    let fixture = Fixture::new(false);
    for version in [19, 21, 29] {
        let source = pattern(version, false, &[3], &[]);
        for change in 0..3 {
            let mut payload = (*source).clone();
            match change {
                0 => {
                    payload.property_slots.pop();
                }
                1 => payload.property_slots.push(reference(0)),
                _ => payload.extension_values.push(0),
            }
            let malformed = Located::new(
                payload,
                RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE),
                segment(),
                20,
            );
            assert!(fixture
                .index()
                .resolve(&malformed, &fixture.label, seed)
                .is_none());
        }
    }
}

#[test]
fn mirrors_refuse_unsupported_record_versions() {
    let fixture = Fixture::new(false);
    for version in [0, 14, 24, 26, 27, 32] {
        assert!(fixture
            .index()
            .resolve(&pattern(version, false, &[3], &[]), &fixture.label, seed)
            .is_none());
    }
}

#[test]
fn mirrors_require_the_dimensions_family_and_operand_record_types() {
    let fixture = Fixture::new(false);
    assert!(fixture
        .index()
        .resolve(&pattern(19, false, &[3], &[(5, 105)]), &fixture.label, seed)
        .is_none());
    for index in [0, 1, 2, 3, 4, 5] {
        let mut fixture = Fixture::new(false);
        fixture.properties[index].identity.type_id = RecordTypeId::from_bytes(WORK_PLANE_TYPE);
        assert!(fixture
            .index()
            .resolve(&pattern(19, false, &[3], &[]), &fixture.label, seed)
            .is_none());
    }
    let mut fixture = Fixture::new(false);
    fixture.transforms[0].identity.type_id = RecordTypeId::from_bytes(WORK_PLANE_TYPE);
    assert!(fixture
        .index()
        .resolve(&pattern(19, false, &[3], &[]), &fixture.label, seed)
        .is_none());
    let mut fixture = Fixture::new(false);
    fixture.planes[0].identity.type_id = RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE);
    assert!(fixture
        .index()
        .resolve(&pattern(19, false, &[3], &[]), &fixture.label, seed)
        .is_none());
}

#[test]
fn body_mirror_results_use_generated_children_instead_of_source_bodies() {
    let mut fixture = Fixture::new(true);
    fixture.properties.push(property(
        106,
        PmDcFeaturePropertyKind::SurfaceBody { body: reference(2) },
    ));
    let source = pattern(25, true, &[], &[(8, 150)]);
    let child = crate::feature::tests::test_feature(40, 4, &[(3, 106)]);
    let child_label = test_label(40, 3, BODY_INSTANCE_CLASS, &[]);
    let label = test_label(20, 2, fixture.label.class_id(), &[41]);
    let token = segment();
    let children = [child];
    let labels = [child_label];
    let index = MirrorIndex::new(
        &fixture.properties,
        &fixture.transforms,
        &fixture.planes,
        &children,
        &labels,
        std::iter::once((token.as_str(), 149)),
    );
    let result = index
        .resolve(&source, &label, |_| None)
        .expect("body instance result");
    assert_eq!(result.result_native, label.id());
    assert_eq!(
        result.members,
        FeatureResultMembers::new(
            vec![NonBlankString::new(fixture.properties[6].id()).expect("body id")],
            Vec::new(),
            Vec::new(),
            Vec::new()
        )
        .expect("result members")
    );
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = result.definition
    else {
        panic!("pattern")
    };
    assert_eq!(
        seeds,
        vec![PatternSeed::Bodies(BodySelection::Native(
            fixture.properties[2].id()
        ))]
    );
    let missing = MirrorIndex::new(
        &fixture.properties,
        &fixture.transforms,
        &fixture.planes,
        &children,
        &labels,
        std::iter::empty(),
    );
    assert!(missing.resolve(&source, &label, |_| None).is_none());
    let repeated = MirrorIndex::new(
        &fixture.properties,
        &fixture.transforms,
        &fixture.planes,
        &children,
        &labels,
        [(token.as_str(), 149), (token.as_str(), 149)].into_iter(),
    );
    assert!(repeated.resolve(&source, &label, |_| None).is_none());
}

#[test]
fn mirrors_refuse_nonorthogonal_plane_frames_and_nonidentity_placements() {
    let source = pattern(19, false, &[3], &[]);
    let mut fixture = Fixture::new(false);
    let mut plane = (*fixture.planes[0]).clone();
    plane.u_axis = plane.v_axis;
    fixture.planes[0] = Located::new(
        plane,
        RecordTypeId::from_bytes(WORK_PLANE_TYPE),
        segment(),
        103,
    );
    assert!(fixture
        .index()
        .resolve(&source, &fixture.label, seed)
        .is_none());
    let mut fixture = Fixture::new(false);
    let mut transform = (*fixture.transforms[0]).clone();
    let mut rows = IDENTITY;
    rows[0][3] = 1.0;
    transform.matrix = crate::compact_matrix::CompactMatrix::try_from_rows(0x8429, 0x7bd6, rows)
        .expect("translated matrix");
    fixture.transforms[0] = Located::new(
        transform,
        RecordTypeId::from_bytes(crate::pmdc::inventor_id(0x9087_4d18)),
        segment(),
        100,
    );
    assert!(fixture
        .index()
        .resolve(&source, &fixture.label, seed)
        .is_none());
}

#[test]
fn native_validation_compares_the_reflection_seeds_and_result_identity() {
    let fixture = Fixture::new(false);
    let source = pattern(19, false, &[3], &[]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (mut feature, mut result) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, seed)
        .expect("mirror")
        .expect("projection");
    assert!(fixture
        .index()
        .matches_projection(&source, &fixture.label, &feature, &result, seed));
    result.native_ref = Some(source.id());
    assert!(!fixture
        .index()
        .matches_projection(&source, &fixture.label, &feature, &result, seed));
    result.native_ref = Some(fixture.properties[2].id());
    assert!(!fixture.index().matches_projection(
        &source,
        &fixture.label,
        &feature,
        &result,
        |_| seed(4)
    ));
    feature.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: vec![PatternSeed::Feature(seed(2).expect("seed"))],
            pattern: PatternKind::new(PatternTransform::Mirror {
                plane_origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("origin"),
                plane_normal: FeatureDirection3::new(Vector3::new(1.0, 0.0, 0.0)).expect("normal"),
            })
            .expect("reflection"),
        }),
    );
    assert!(!fixture
        .index()
        .matches_projection(&source, &fixture.label, &feature, &result, seed));
}

#[test]
fn native_validation_requires_canonical_feature_and_result_ids() {
    let fixture = Fixture::new(false);
    let source = pattern(19, false, &[3], &[]);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (feature, result) = fixture
        .index()
        .project(&ctx, &source, &fixture.label, seed)
        .expect("mirror")
        .expect("projection");
    let mut changed_result = result.clone();
    changed_result.id = FeatureResultTopologyId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
        fixture.transforms[0].identity.key().expect("record key"),
    );
    assert!(!fixture.index().matches_projection(
        &source,
        &fixture.label,
        &feature,
        &changed_result,
        seed
    ));
    let mut changed_feature = feature.clone();
    changed_feature.id = seed(4).expect("other feature id");
    changed_result = result.clone();
    changed_result.output_of = changed_feature.id.clone();
    assert!(!fixture.index().matches_projection(
        &source,
        &fixture.label,
        &changed_feature,
        &changed_result,
        seed
    ));
    let mut changed_feature = feature.clone();
    changed_feature.native_ref = Some(fixture.transforms[0].id());
    assert!(!fixture.index().matches_projection(
        &source,
        &fixture.label,
        &changed_feature,
        &result,
        seed
    ));
}

#[test]
fn mirrors_refuse_nonzero_variables_and_boolean_options() {
    let source = pattern(19, false, &[3], &[]);
    let mut fixture = Fixture::new(false);
    fixture.properties[4] = property(
        107,
        PmDcFeaturePropertyKind::RdxVariable {
            name: String::new(),
            name_value: 0,
            nominal_value: 1,
            model_value: 1,
        },
    );
    assert!(fixture
        .index()
        .resolve(&source, &fixture.label, seed)
        .is_none());
    let mut fixture = Fixture::new(false);
    fixture.properties[1] = property(
        102,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    );
    assert!(fixture
        .index()
        .resolve(&source, &fixture.label, seed)
        .is_none());
    let fixture = Fixture::new(false);
    assert!(fixture
        .index()
        .resolve(&pattern(19, false, &[3], &[(3, 1)]), &fixture.label, seed)
        .is_none());
    assert!(fixture
        .index()
        .resolve(
            &pattern(19, false, &[3], &[(11, 103)]),
            &fixture.label,
            seed
        )
        .is_some());
    assert!(fixture
        .index()
        .resolve(
            &pattern(19, false, &[3], &[(11, 106)]),
            &fixture.label,
            seed
        )
        .is_none());
    let mut fixture = Fixture::new(false);
    fixture.properties.push(property(
        108,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    ));
    assert!(fixture
        .index()
        .resolve(
            &pattern(19, false, &[3], &[(11, 109)]),
            &fixture.label,
            seed
        )
        .is_none());
    let mut payload = (*source).clone();
    payload.control = 1;
    let source = Located::new(
        payload,
        RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE),
        segment(),
        20,
    );
    let fixture = Fixture::new(false);
    assert!(fixture
        .index()
        .resolve(&source, &fixture.label, seed)
        .is_none());
}
