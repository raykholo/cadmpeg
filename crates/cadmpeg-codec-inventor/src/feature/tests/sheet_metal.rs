// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::sheet_metal::{
    parse_group, parse_sheet_metal_property, PmDcSheetMetalFamily, PmDcSheetMetalGroup,
    PmDcSheetMetalGroupForm, PmDcSheetMetalGroupPayload, BEND_CLASS_ID, BEND_DEFINITION_TYPE,
    BEND_RELIEF_TYPE, BEND_SETTINGS_TYPE, CORNER_CLASS_ID, CORNER_DEFINITION_TYPE, FACE_GROUP_TYPE,
    FLANGE_GROUP_TYPE,
};

/// A sheet-metal group record at `ordinal` with one-based member references.
pub(super) fn group_fixture(ordinal: u32, members: &[u32]) -> PmDcSheetMetalGroup {
    group_record(ordinal, PmDcSheetMetalGroupForm::Face, members)
}

fn group_record(
    ordinal: u32,
    form: PmDcSheetMetalGroupForm,
    members: &[u32],
) -> PmDcSheetMetalGroup {
    let bytes = group_bytes(members);
    let type_id = match form {
        PmDcSheetMetalGroupForm::Face => FACE_GROUP_TYPE,
        PmDcSheetMetalGroupForm::Flange => FLANGE_GROUP_TYPE,
    };
    let payload: PmDcSheetMetalGroupPayload = parse(&bytes, |ctx, source| {
        parse_group(ctx, source, 19, type_id).expect("group fixture")
    });
    Located::new(
        payload,
        crate::record_identity::RecordTypeId::from_bytes(type_id),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        ordinal,
    )
}

fn group_bytes(members: &[u32]) -> Vec<u8> {
    let mut bytes = content(5);
    bytes.extend_from_slice(&(-1i32).to_le_bytes());
    bytes.extend_from_slice(&references(
        &members
            .iter()
            .map(|member| member | 0x8000_0000)
            .collect::<Vec<_>>(),
    ));
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes
}

fn tuple_bytes(references: &[u32], states: &[u8], tail: Option<u32>) -> Vec<u8> {
    let mut bytes = content(6);
    for reference in references {
        bytes.extend_from_slice(&(reference | 0x8000_0000).to_le_bytes());
    }
    bytes.extend_from_slice(states);
    if let Some(tail) = tail {
        bytes.extend_from_slice(&(tail | 0x8000_0000).to_le_bytes());
    }
    bytes
}

#[test]
fn sheet_metal_property_records_have_fixed_reference_tuples() {
    for (type_id, count, states, family) in [
        (
            BEND_DEFINITION_TYPE,
            10,
            &[1u8, 1, 0][..],
            PmDcSheetMetalFamily::BendDefinition,
        ),
        (
            BEND_RELIEF_TYPE,
            6,
            &[][..],
            PmDcSheetMetalFamily::BendRelief,
        ),
        (
            BEND_SETTINGS_TYPE,
            3,
            &[][..],
            PmDcSheetMetalFamily::BendSettings,
        ),
        (
            CORNER_DEFINITION_TYPE,
            9,
            &[][..],
            PmDcSheetMetalFamily::CornerDefinition,
        ),
    ] {
        let values = (1..=count).collect::<Vec<u32>>();
        let tail = (!states.is_empty()).then_some(40);
        let bytes = tuple_bytes(&values, states, tail);
        let parsed = parse(&bytes, |ctx, source| {
            parse_sheet_metal_property(ctx, source, 19, type_id).expect("sheet-metal record")
        });
        let PmDcFeaturePropertyKind::SheetMetal {
            family: actual,
            references,
            states: actual_states,
        } = &parsed.kind
        else {
            panic!("not a sheet-metal record");
        };
        assert_eq!(*actual, family);
        let mut expected = values.clone();
        expected.extend(tail);
        assert_eq!(
            references
                .iter()
                .map(|reference| reference.index())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(actual_states.as_slice(), states);
        let mut long = bytes.clone();
        long.push(0);
        assert!(parse(&long, |ctx, source| {
            parse_sheet_metal_property(ctx, source, 19, type_id).is_err()
        }));
        let short = &bytes[..bytes.len() - 1];
        assert!(parse(short, |ctx, source| {
            parse_sheet_metal_property(ctx, source, 19, type_id).is_err()
        }));
    }
}

#[test]
fn sheet_metal_group_records_list_their_member_features() {
    let face = group_record(7, PmDcSheetMetalGroupForm::Face, &[101]);
    assert_eq!(face.form, PmDcSheetMetalGroupForm::Face);
    assert_eq!(
        face.members
            .references()
            .iter()
            .map(|reference| reference.index())
            .collect::<Vec<_>>(),
        [101]
    );
    let flange = group_record(8, PmDcSheetMetalGroupForm::Flange, &[101, 102, 103]);
    assert_eq!(flange.form, PmDcSheetMetalGroupForm::Flange);
    assert_eq!(flange.members.references().len(), 3);
    let mut long = group_bytes(&[101]);
    long.push(0);
    assert!(parse(&long, |ctx, source| {
        parse_group(ctx, source, 19, FACE_GROUP_TYPE).is_err()
    }));
    assert!(parse(&group_bytes(&[101]), |ctx, source| {
        parse_group(ctx, source, 19, BEND_DEFINITION_TYPE).is_err()
    }));
}

/// Plate #100 with sketch #50 (direction #60) in the group record #200. A
/// flange plate names direction #62 in slot 1.
///
/// Properties: #1 region selection, #2 flip, #3 body, #4 edge collection,
/// #5 bend definition, #6 corner definition, #7 bend relief, #8 bend
/// settings, #9 corner-edge list, #10 value. Parameter #70 is the thickness.
/// Bend #101 and corner #102 repeat the plate properties that their group
/// form requires.
struct PlateFixture {
    form: PmDcSheetMetalGroupForm,
    members: Vec<u32>,
    flip: bool,
    plate_slot_1: u32,
    bend_slots: Vec<(usize, u32)>,
    corner_slots: Vec<(usize, u32)>,
    bend_class: ClassId,
}

impl PlateFixture {
    fn face() -> Self {
        Self {
            form: PmDcSheetMetalGroupForm::Face,
            members: vec![101],
            flip: false,
            plate_slot_1: 60,
            bend_slots: Vec::new(),
            corner_slots: Vec::new(),
            bend_class: BEND_CLASS_ID,
        }
    }

    fn face_with_bend() -> Self {
        Self {
            members: vec![101, 102],
            bend_slots: vec![(0, 4), (2, 70), (3, 5), (5, 3)],
            ..Self::face()
        }
    }

    fn flange() -> Self {
        Self {
            form: PmDcSheetMetalGroupForm::Flange,
            members: vec![101, 102, 103],
            plate_slot_1: 62,
            bend_slots: vec![(0, 4), (2, 70), (3, 5), (4, 7), (5, 3), (7, 8)],
            corner_slots: vec![(0, 6), (1, 3), (3, 9), (4, 1)],
            ..Self::face()
        }
    }

    fn plate_slots(&self) -> Vec<(usize, u32)> {
        let mut slots = vec![
            (0, 1),
            (1, self.plate_slot_1),
            (2, 2),
            (3, 70),
            (6, 3),
            (7, 4),
        ];
        if self.members.len() > 1 {
            slots.push((4, 5));
        }
        if self.form == PmDcSheetMetalGroupForm::Flange {
            slots.extend([(9, 6), (10, 7), (11, 8), (12, 9), (13, 10)]);
        }
        slots
    }

    fn project(&self) -> Option<Feature> {
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
                direction: reference(61),
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
        let direction = Located::new(
            crate::sketch::PmDcDirectionPayload {
                save_version_major: 19,
                header: test_header(),
                entity_flags: 0,
                parameter: cadmpeg_ir::scalar::FiniteReal::ZERO,
                extension: None,
                direction: [
                    cadmpeg_ir::scalar::FiniteReal::ONE,
                    cadmpeg_ir::scalar::FiniteReal::ZERO,
                    cadmpeg_ir::scalar::FiniteReal::ZERO,
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
            62,
        );
        let sheet_metal = |ordinal, family, count| {
            test_property(
                ordinal,
                PmDcFeaturePropertyKind::SheetMetal {
                    family,
                    references: vec![reference(0); count],
                    states: cadmpeg_ir::native::bytes::NativeBytes::default(),
                },
            )
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
                    value: self.flip,
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
            sheet_metal(5, PmDcSheetMetalFamily::BendDefinition, 11),
            sheet_metal(6, PmDcSheetMetalFamily::CornerDefinition, 9),
            sheet_metal(7, PmDcSheetMetalFamily::BendRelief, 6),
            sheet_metal(8, PmDcSheetMetalFamily::BendSettings, 3),
            test_property(
                9,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::CornerEdges,
                    items: reference_list(&[]),
                },
            ),
            test_property(10, PmDcFeaturePropertyKind::Value { value: 0 }),
        ];
        let plate = test_feature(100, 15, &self.plate_slots());
        let bend = test_feature(101, 9, &self.bend_slots);
        let corner = test_feature(102, 6, &self.corner_slots);
        let label = test_label(100, 4, crate::feature::SHEET_METAL_PLATE_CLASS_ID, &[]);
        let bend_label = test_label(101, 5, self.bend_class, &[]);
        let corner_label = test_label(102, 6, CORNER_CLASS_ID, &[]);
        let mut participants = vec![51];
        participants.extend(&self.members);
        let wrapper = test_label(
            200,
            3,
            ClassId::try_from("0".repeat(32)).expect("class"),
            &participants,
        );
        let group = group_record(200, self.form, &self.members);
        let mut index = test_projection_index(
            &properties,
            std::slice::from_ref(&raw_thickness),
            std::slice::from_ref(&neutral_thickness),
            std::slice::from_ref(&raw_sketch),
            std::slice::from_ref(&neutral_sketch),
            std::slice::from_ref(&direction),
            &[],
            &[],
        );
        index.entity_style_links.insert(("generated", 51));
        index
            .containing_labels
            .insert(("generated", 100), Some(&wrapper));
        index
            .sheet_metal_groups
            .insert(("generated", 200), Some(&group));
        for (record, record_label) in [
            (&plate, &label),
            (&bend, &bend_label),
            (&corner, &corner_label),
        ] {
            let ordinal = record.identity.record_ordinal;
            index.features.insert(("generated", ordinal), Some(record));
            index
                .owner_labels
                .insert(("generated", Some(ordinal)), Some(record_label));
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        crate::feature::sheet_metal::project_plate(&ctx, &plate, &label, &index)
            .map(|projected| projected.expect("plate projection").0)
    }
}

fn assert_base_flange(plate: &Feature, group: &str) {
    assert_plate_side(
        plate,
        group,
        cadmpeg_ir::features::SheetMetalThicknessSide::Forward,
    );
}

fn assert_plate_side(
    plate: &Feature,
    group: &str,
    expected: cadmpeg_ir::features::SheetMetalThicknessSide,
) {
    assert!(matches!(
        plate.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
            thickness,
            side,
            ..
        }) if thickness.get() == 3.048 && *side == expected
    ));
    assert_eq!(plate.source_tag.as_deref(), Some("sheet_metal_plate"));
    assert_eq!(
        plate
            .source_properties
            .get("sheet_metal_group")
            .map(String::as_str),
        Some(group)
    );
}

#[test]
fn sheet_metal_plate_projects_base_flange() {
    assert_base_flange(&PlateFixture::face().project().expect("plate"), "face");
    // The face plate thickness follows its sketch direction record.
    assert!(PlateFixture {
        plate_slot_1: 62,
        ..PlateFixture::face()
    }
    .project()
    .is_none());
    // The plate is the first group member.
    assert!(PlateFixture {
        members: vec![102, 101],
        ..PlateFixture::face()
    }
    .project()
    .is_none());
}

#[test]
fn bent_face_and_flange_plates_project_their_walls() {
    assert_base_flange(
        &PlateFixture::face_with_bend().project().expect("bent face"),
        "face",
    );
    assert_base_flange(&PlateFixture::flange().project().expect("flange"), "flange");
}

#[test]
fn group_members_must_repeat_the_plate_properties() {
    // The bend repeats the bend definition.
    let mut fixture = PlateFixture::face_with_bend();
    fixture.bend_slots.retain(|(slot, _)| *slot != 3);
    assert!(fixture.project().is_none());
    // The bend member has the bend class.
    assert!(PlateFixture {
        bend_class: CORNER_CLASS_ID,
        ..PlateFixture::face_with_bend()
    }
    .project()
    .is_none());
    // The flange corner repeats the region selection.
    let mut fixture = PlateFixture::flange();
    fixture.corner_slots.retain(|(slot, _)| *slot != 4);
    assert!(fixture.project().is_none());
    // A flange bend repeats the bend relief.
    let mut fixture = PlateFixture::flange();
    fixture.bend_slots.retain(|(slot, _)| *slot != 4);
    assert!(fixture.project().is_none());
    // A face group has no corner member.
    assert!(PlateFixture {
        form: PmDcSheetMetalGroupForm::Face,
        ..PlateFixture::flange()
    }
    .project()
    .is_none());
}

#[test]
fn a_flipped_plate_lies_opposite_the_sketch_normal() {
    let plate = PlateFixture {
        flip: true,
        ..PlateFixture::face_with_bend()
    }
    .project()
    .expect("flipped plate");
    assert_plate_side(
        &plate,
        "face",
        cadmpeg_ir::features::SheetMetalThicknessSide::Reverse,
    );
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

#[test]
fn inventory_admits_sheet_metal_groups_and_property_records() {
    let inventory = inventory_with_record(
        FLANGE_GROUP_TYPE,
        &group_bytes(&[3, 4, 5]),
        DecodePolicy::service(),
    )
    .expect("group inventory");
    assert_eq!(inventory.sheet_metal_groups.len(), 1);
    assert_eq!(
        inventory.sheet_metal_groups[0].form,
        PmDcSheetMetalGroupForm::Flange
    );
    let inventory = inventory_with_record(
        BEND_SETTINGS_TYPE,
        &tuple_bytes(&[1, 2, 3], &[], None),
        DecodePolicy::service(),
    )
    .expect("property inventory");
    assert!(matches!(
        &inventory.properties[0].kind,
        PmDcFeaturePropertyKind::SheetMetal {
            family: PmDcSheetMetalFamily::BendSettings,
            references,
            ..
        } if references.len() == 3
    ));
    let mut truncated = group_bytes(&[3]);
    truncated.truncate(truncated.len() - 2);
    let inventory = inventory_with_record(FACE_GROUP_TYPE, &truncated, DecodePolicy::service())
        .expect("issue inventory");
    assert!(inventory.sheet_metal_groups.is_empty());
    assert_eq!(inventory.issues.len(), 1);
}
