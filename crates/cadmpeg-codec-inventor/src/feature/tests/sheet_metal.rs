// SPDX-License-Identifier: Apache-2.0
use super::*;

fn plate_index_fixture(
    flip: bool,
    sketch_direction: u32,
    wrapper: &[u32],
) -> (
    Vec<PmDcFeatureProperty>,
    Vec<crate::design::PmDcParameter>,
    Vec<DesignParameter>,
    crate::sketch::PmDcSketch,
    Sketch,
    PmDcFeatureLabel,
) {
    let raw_thickness = raw_parameter(70);
    let neutral_thickness = neutral_parameter(
        &raw_thickness,
        ParameterValue::Length(Length::new(3.048).expect("finite length fixture")),
    );
    let raw_sketch = Located::new(
        crate::sketch::PmDcSketchPayload {
            save_version_major: 19,
            header: test_header(),
            state: 0,
            count_value: 0,
            entities: PmDcReferenceList::new(8, None, Vec::new()).expect("empty entity list"),
            transform: reference(0),
            direction: reference(sketch_direction),
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
    );
    let neutral_sketch = Sketch {
        id: SketchId::mint(format!("inventor:design:sketch#{}-50", segment()))
            .expect("valid test fixture"),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid test fixture"),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some(raw_sketch.id()),
    };
    let properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::ProfileSelection {
                entity_link: reference(52),
                value: 0,
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: flip,
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items: reference_list(&[]),
            },
        ),
    ];
    let wrapper = test_label(
        200,
        3,
        ClassId::try_from("0".repeat(32)).expect("class"),
        wrapper,
    );
    (
        properties,
        vec![raw_thickness],
        vec![neutral_thickness],
        raw_sketch,
        neutral_sketch,
        wrapper,
    )
}

fn project_generated_plate(flip: bool, sketch_direction: u32, wrapper: &[u32]) -> Option<Feature> {
    let (properties, raw, neutral, raw_sketch, neutral_sketch, wrapper) =
        plate_index_fixture(flip, sketch_direction, wrapper);
    let plate = test_feature(100, 15, &[(0, 1), (1, 60), (2, 2), (3, 70), (6, 3), (7, 4)]);
    let label = test_label(100, 4, crate::feature::SHEET_METAL_PLATE_CLASS_ID, &[]);
    let mut index = test_projection_index(
        &properties,
        &raw,
        &neutral,
        std::slice::from_ref(&raw_sketch),
        std::slice::from_ref(&neutral_sketch),
        &[],
        &[],
        &[],
    );
    index.entity_style_links.insert(("generated", 51));
    index
        .containing_labels
        .insert(("generated", 100), Some(&wrapper));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    crate::feature::project_plate(&ctx, &plate, &label, &index)
        .map(|projected| projected.expect("plate projection").0)
}

#[test]
fn sheet_metal_plate_projects_base_flange() {
    let plate = project_generated_plate(false, 61, &[51, 101]).expect("plate");
    assert!(matches!(
        plate.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
            thickness,
            side: cadmpeg_ir::features::SheetMetalThicknessSide::Forward,
            ..
        }) if thickness.get() == 3.048
    ));
    assert_eq!(plate.source_tag.as_deref(), Some("sheet_metal_plate"));
    assert!(project_generated_plate(true, 61, &[51, 101]).is_none());
    assert!(project_generated_plate(false, 62, &[51, 101]).is_none());
    assert!(project_generated_plate(false, 61, &[51, 101, 102]).is_none());
}

#[test]
fn corner_round_accepts_an_absent_continuity_boolean() {
    let raw_radius = raw_parameter(20);
    let neutral_radius = neutral_parameter(
        &raw_radius,
        ParameterValue::Length(Length::new(1.5).expect("finite length fixture")),
    );
    let properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Fillet,
                type_value: 2,
                value: 0,
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FilletEdgeSets,
                items: reference_list(&[4]),
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::FilletEdgeSet {
                edges: reference(5),
                radius: reference(21),
                selection: reference(6),
                continuity: reference(0),
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items: reference_list(&[8]),
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::WideEnumeration {
                type_value: 4,
                value: 0,
            },
        ),
        test_property(
            7,
            PmDcFeaturePropertyKind::EdgeItem {
                index_references: PmDcU32List::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                    vec![42],
                )
                .expect("test list metadata matches length"),
                index_reference_value: 0,
                value: 0,
            },
        ),
        test_property(
            8,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[10]),
            },
        ),
        test_property(
            9,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
    ];
    let round = test_feature(100, 17, &[(0, 2), (11, 1), (15, 8)]);
    let label = test_label(100, 7, crate::feature::CORNER_ROUND_CLASS_ID, &[]);
    let index = test_projection_index(
        &properties,
        std::slice::from_ref(&raw_radius),
        std::slice::from_ref(&neutral_radius),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let (round, _) = crate::feature::project_corner_round(&ctx, &round, &label, &index)
        .expect("corner round candidate")
        .expect("corner round projection");
    assert_eq!(round.source_tag.as_deref(), Some("corner_round"));
    assert!(matches!(
        round.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
            if matches!(groups[0].radius, RadiusSpec::Constant { radius } if radius.get() == 1.5)
    ));
    let fillet_label = test_label(100, 7, FILLET_CLASS_ID, &[]);
    assert!(project_fillet(
        &ctx,
        &test_feature(100, 17, &[(0, 2), (11, 1), (15, 8)]),
        &fillet_label,
        &index
    )
    .is_none());
}
