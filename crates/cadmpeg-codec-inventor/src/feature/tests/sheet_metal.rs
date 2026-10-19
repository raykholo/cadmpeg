// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::sheet_metal::{
    parse_group, parse_sheet_metal_property, PmDcSheetMetalFamily, PmDcSheetMetalGroup,
    PmDcSheetMetalGroupForm, PmDcSheetMetalGroupPayload, BEND_CLASS_ID, BEND_DEFINITION_TYPE,
    BEND_RELIEF_TYPE, BEND_SETTINGS_TYPE, CORNER_CLASS_ID, CORNER_DEFINITION_TYPE, FACE_GROUP_TYPE,
    FLANGE_GROUP_TYPE,
};
use cadmpeg_ir::features::{
    EdgeSelection, SheetMetalBendPosition, SheetMetalFlangeHeight, SheetMetalFlangeWidth,
    SheetMetalHeightDatum,
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
    definition: Option<DefinitionFixture>,
}

/// Flange definition #11 of a flange plate fixture. Parameters #72, #74, and
/// #76 are the height, angle, and bend radius; #12 is the edge item.
#[derive(Clone, Copy)]
struct DefinitionFixture {
    width_extent: u32,
    bend_position: u32,
    height_datum: u32,
    /// Record ordinal of the region selection the definition names.
    region: u32,
    /// The bend definition names the flange bend radius.
    shared_radius: bool,
}

impl DefinitionFixture {
    const EXACT: Self = Self {
        width_extent: 3,
        bend_position: 1,
        height_datum: 2,
        region: 1,
        shared_radius: true,
    };

    /// Record bytes; references are one-based record ordinals.
    fn bytes(self) -> Vec<u8> {
        let qualified = |ordinal: u32| (ordinal + 1) | 0x8000_0000;
        let mut bytes = content(11);
        bytes.extend_from_slice(&self.width_extent.to_le_bytes());
        // Height, angle, thickness, first flag, length, edge item, body, plane.
        for ordinal in [72, 74, 70, 2, 70, 12, 3, 40] {
            bytes.extend_from_slice(&qualified(ordinal).to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&references(&[41, 42, 43, 44].map(qualified)));
        // Auxiliary record, bend radius, second flag.
        for ordinal in [45, 76, 2] {
            bytes.extend_from_slice(&qualified(ordinal).to_le_bytes());
        }
        bytes.extend_from_slice(&self.bend_position.to_le_bytes());
        bytes.extend_from_slice(&self.height_datum.to_le_bytes());
        bytes.push(1);
        // Corner edges, direction, region selection.
        for ordinal in [9, 62, self.region] {
            bytes.extend_from_slice(&qualified(ordinal).to_le_bytes());
        }
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&qualified(2).to_le_bytes());
        bytes
    }

    fn kind(self) -> PmDcFeaturePropertyKind {
        parse(&self.bytes(), |ctx, source| {
            crate::feature::sheet_metal::parse_flange_definition(ctx, source, 19)
                .expect("flange definition")
        })
        .kind
    }
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
            definition: None,
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

    fn edge_flange(definition: DefinitionFixture) -> Self {
        Self {
            definition: Some(definition),
            ..Self::flange()
        }
    }

    fn project(&self) -> Option<Feature> {
        self.project_with_bend().0
    }

    /// The projected plate and the bend member its projection includes.
    fn project_with_bend(&self) -> (Option<Feature>, Option<u32>) {
        let raw_parameters = [70, 72, 74, 76].map(raw_parameter);
        let neutral_parameters = [
            ParameterValue::Length(Length::new(3.048).expect("finite length fixture")),
            ParameterValue::Length(Length::new(20.0).expect("finite length fixture")),
            ParameterValue::Angle(
                cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2)
                    .expect("finite angle fixture"),
            ),
            ParameterValue::Length(Length::new(3.048).expect("finite length fixture")),
        ];
        let neutral_parameters = raw_parameters
            .iter()
            .zip(neutral_parameters)
            .map(|(raw, value)| neutral_parameter(raw, value))
            .collect::<Vec<_>>();
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
            test_property(
                5,
                PmDcFeaturePropertyKind::SheetMetal {
                    family: PmDcSheetMetalFamily::BendDefinition,
                    references: (0..11)
                        .map(|position| {
                            let shared = self
                                .definition
                                .is_some_and(|definition| definition.shared_radius);
                            reference(if position == 5 && shared { 77 } else { 0 })
                        })
                        .collect(),
                    states: cadmpeg_ir::native::bytes::NativeBytes::default(),
                },
            ),
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
            test_property(
                12,
                PmDcFeaturePropertyKind::EdgeItem {
                    index_references: crate::pmdc::PmDcU32List::new(
                        2,
                        Some(crate::pmdc::PmDcListMetadata::U32([1, 0x1000_0000])),
                        vec![7],
                    )
                    .expect("test list metadata matches length"),
                    index_reference_value: 0,
                    value: 0,
                },
            ),
        ]
        .into_iter()
        .chain(
            self.definition
                .map(|definition| test_property(11, definition.kind())),
        )
        .collect::<Vec<_>>();
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
            &raw_parameters,
            &neutral_parameters,
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
        if let (Some(definition), Some(property)) = (
            self.definition,
            properties
                .iter()
                .find(|property| property.identity.record_ordinal == 11),
        ) {
            index
                .flange_definitions
                .insert(("generated", definition.region), Some(property));
        }
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
        (
            crate::feature::sheet_metal::project_plate(&ctx, &plate, &label, &index)
                .map(|projected| projected.expect("plate projection").0),
            crate::feature::sheet_metal::included_bend(&plate, &index),
        )
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

/// A corner round over edge items #7 and #12. Item #12 has `flags` and an
/// empty or nonempty index-reference list; `first_empty` empties item #7
/// and sets its flag bit 2.
fn corner_round_with_items(flags: u32, second_empty: bool, first_empty: bool) -> Option<Feature> {
    let raw_radius = raw_parameter(20);
    let neutral_radius = neutral_parameter(
        &raw_radius,
        ParameterValue::Length(Length::new(1.5).expect("finite length fixture")),
    );
    let item = |empty: bool| PmDcFeaturePropertyKind::EdgeItem {
        index_references: if empty {
            PmDcU32List::new(2, None, Vec::new()).expect("empty test list")
        } else {
            PmDcU32List::new(
                2,
                Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                vec![42],
            )
            .expect("test list metadata matches length")
        },
        index_reference_value: if empty { -1 } else { 0 },
        value: 0,
    };
    let with_flags = |ordinal: u32, kind: PmDcFeaturePropertyKind, flags: u32| {
        let plain = test_property(ordinal, kind);
        let mut header = test_header();
        header.flags = flags;
        Located::new(
            PmDcFeaturePropertyPayload {
                save_version_major: 16,
                header,
                kind: plain.kind.clone(),
                tail: None,
            },
            plain.identity.type_id.clone(),
            plain.identity.segment_token.clone(),
            ordinal,
        )
    };
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
                items: reference_list(&[8, 13]),
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::WideEnumeration {
                type_value: 4,
                value: 0,
            },
        ),
        with_flags(7, item(first_empty), if first_empty { 0x4 } else { 0 }),
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
        with_flags(12, item(second_empty), flags),
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
    crate::feature::project_corner_round(&ctx, &round, &label, &index)
        .map(|projected| projected.expect("corner round projection").0)
}

#[test]
fn an_edge_item_without_an_edge_reference_carries_flag_bit_2() {
    assert!(corner_round_with_items(0, false, false).is_some());
    assert!(corner_round_with_items(0x4, true, false).is_some());
    assert!(corner_round_with_items(0x804, true, false).is_some());
    // An empty item without flag bit 2 is outside the grammar.
    assert!(corner_round_with_items(0, true, false).is_none());
    // At least one item references an edge.
    assert!(corner_round_with_items(0x4, true, true).is_none());
    assert!(corner_round_with_items(0, false, true).is_some());
}

#[test]
fn flange_definition_record_has_a_fixed_grammar() {
    let bytes = DefinitionFixture::EXACT.bytes();
    let PmDcFeaturePropertyKind::FlangeDefinition(definition) = DefinitionFixture::EXACT.kind()
    else {
        panic!("not a flange definition");
    };
    assert_eq!(definition.width_extent, 3);
    assert_eq!(definition.height.index(), 73);
    assert_eq!(definition.angle.index(), 75);
    assert_eq!(definition.thickness.index(), 71);
    assert_eq!(definition.edge_item.index(), 13);
    assert_eq!(definition.bend_radius.index(), 77);
    assert_eq!(definition.bend_position, 1);
    assert_eq!(definition.height_datum, 2);
    assert_eq!(definition.corner_edges.index(), 10);
    assert_eq!(definition.direction.index(), 63);
    assert_eq!(definition.region_selection.index(), 2);
    assert_eq!(definition.flip.index(), 3);
    assert_eq!(definition.references().count(), 19);
    let parse_at = |bytes: &[u8], version| {
        parse(bytes, |ctx, source| {
            crate::feature::sheet_metal::parse_flange_definition(ctx, source, version).is_ok()
        })
    };
    let mut long = bytes.clone();
    long.push(0);
    assert!(!parse_at(&long, 19));
    assert!(!parse_at(&bytes[..bytes.len() - 1], 19));
    // Later segment versions have no known flange-definition grammar.
    assert!(!parse_at(&bytes, 23));
}

fn edge_flange(fixture: &PlateFixture) -> Option<FeatureOperation> {
    if let FeatureDefinition::Operation(operation) = fixture.project()?.evaluation.definition() {
        Some(operation.clone())
    } else {
        None
    }
}

#[test]
fn flange_plate_with_an_exact_definition_projects_one_edge_flange() {
    let fixture = PlateFixture::edge_flange(DefinitionFixture::EXACT);
    let Some(FeatureOperation::SheetMetalEdgeFlange {
        edges,
        height,
        angle,
        height_datum,
        bend_position,
        width,
        bend_radius,
    }) = edge_flange(&fixture)
    else {
        panic!("not an edge flange");
    };
    assert_eq!(
        edges,
        EdgeSelection::Native(format!("inventor:pmdc:feature-property#{}-12", segment()))
    );
    assert!(matches!(height, SheetMetalFlangeHeight::Distance(value) if value.get() == 20.0));
    assert_eq!(angle.get(), std::f64::consts::FRAC_PI_2);
    assert_eq!(height_datum, SheetMetalHeightDatum::OuterFaces);
    assert_eq!(bend_position, SheetMetalBendPosition::Outside);
    assert_eq!(width, SheetMetalFlangeWidth::FullEdge);
    assert_eq!(bend_radius.get(), 3.048);
    // The edge flange includes the group bend.
    assert_eq!(fixture.project_with_bend().1, Some(101));
    let inner_adjacent = PlateFixture::edge_flange(DefinitionFixture {
        bend_position: 3,
        height_datum: 1,
        ..DefinitionFixture::EXACT
    });
    assert!(matches!(
        edge_flange(&inner_adjacent),
        Some(FeatureOperation::SheetMetalEdgeFlange {
            height_datum: SheetMetalHeightDatum::InnerFaces,
            bend_position: SheetMetalBendPosition::Adjacent,
            ..
        })
    ));
}

#[test]
fn unproven_flange_definitions_keep_the_plate_wall() {
    for definition in [
        DefinitionFixture {
            height_datum: 3,
            ..DefinitionFixture::EXACT
        },
        DefinitionFixture {
            bend_position: 2,
            ..DefinitionFixture::EXACT
        },
        DefinitionFixture {
            bend_position: 4,
            ..DefinitionFixture::EXACT
        },
        DefinitionFixture {
            width_extent: 2,
            ..DefinitionFixture::EXACT
        },
        DefinitionFixture {
            shared_radius: false,
            ..DefinitionFixture::EXACT
        },
        // The definition names a different region selection.
        DefinitionFixture {
            region: 9,
            ..DefinitionFixture::EXACT
        },
    ] {
        let fixture = PlateFixture::edge_flange(definition);
        assert_base_flange(&fixture.project().expect("flange wall"), "flange");
    }
    // A face group has no flange definition.
    assert_eq!(PlateFixture::face_with_bend().project_with_bend().1, None);
}
