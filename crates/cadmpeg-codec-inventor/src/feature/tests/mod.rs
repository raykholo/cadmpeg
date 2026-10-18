// SPDX-License-Identifier: Apache-2.0
use super::{
    inventory, parse_boolean, parse_boundary_patch, parse_chamfer, parse_edge_item,
    parse_entity_style_link, parse_feature, parse_fillet_edge_selection, parse_fillet_edge_set,
    parse_label, parse_part_operation, parse_pattern_feature, parse_placement,
    parse_profile_selection, parse_rdx_variable, parse_surface_body, parse_terminator,
    project_chamfer, project_extrusion, project_fillet, project_hole, ClassId, PmDcEntityStyleLink,
    PmDcEntityStyleLinkPayload, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureLabel,
    PmDcFeatureLabelPayload, PmDcFeatureLabelPayloadWire, PmDcFeaturePayload, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload, PmDcFeatureReferenceFamily,
    PmDcLinkedHeader, PmDcPatternFamily, PmDcWorkPlane, PmDcWorkPlanePayload, ProjectionIndex,
    BOOLEAN_TYPE, CHAMFER_CLASS_ID, END_OF_FEATURES_TYPE, ENTITY_STYLE_LINK_TYPE,
    EXTRUSION_CLASS_ID, FEATURE_LABEL_TYPE, FEATURE_TYPE, FILLET_CLASS_ID, HOLE_CLASS_ID,
    MIRROR_FEATURE_TYPE, PLANE_SURFACE_FORM, RECTANGULAR_PATTERN_FEATURE_TYPE, WORK_PLANE_TYPE,
};
use crate::container::InventorContainer;
use crate::pmdc::{PmDcContentHeader, PmDcReferenceList, PmDcU32List};
use crate::record_identity::Located;
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
use crate::test_support::test_fixtures::{content, parse, primary_envelope_fixture};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    edge_treatments::{ChamferSpec, RadiusSpec},
    holes::{HoleKind, HolePlacement},
    BooleanOp, DesignParameter, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, Feature,
    FeatureDefinition, FeatureOperation, FeatureResultTopology, LinearTermination,
};
use cadmpeg_ir::features::{ParameterId, ParameterValue};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::sketches::{SketchId, SketchPlacement};
use std::collections::BTreeMap;

#[test]
fn feature_projection_refuses_collection_limit_before_token_index() {
    let inventory = super::FeatureInventory {
        containers: Vec::new(),
        features: vec![test_feature(0, 0, &[])],
        pattern_features: Vec::new(),
        terminators: Vec::new(),
        properties: Vec::new(),
        labels: Vec::new(),
        entity_style_links: Vec::new(),
        kernel_entity_links: Vec::new(),
        work_planes: Vec::new(),
        origin_folders: Vec::new(),
        sheet_metal_groups: Vec::new(),
        issues: Vec::new(),
    };
    let design = crate::design::DesignInventory {
        parameters: Vec::new(),
        expressions: Vec::new(),
        units: Vec::new(),
        parameter_tables: Vec::new(),
        model_roots: Vec::new(),
        brep_segments: Vec::new(),
        issues: Vec::new(),
    };
    let sketch = crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
        projections: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(matches!(
        super::project(&ctx, &inventory, &design, &sketch, &[], &[], &[]),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "index Inventor feature token"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert_eq!(
        super::project(&ctx, &inventory, &design, &sketch, &[], &[], &[])
            .expect("feature projection")
            .unresolved_features,
        1
    );
}

#[test]
fn feature_result_refuses_entity_limit_before_creation() {
    let source = test_feature(0, 1, &[(0, 1)]);
    let properties = [
        test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[3]),
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
    ];
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(matches!(
        super::feature_result(&ctx, &source, 0, &index),
        Some(Err(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "project Inventor feature result topology"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert!(matches!(
        super::feature_result(&ctx, &source, 0, &index),
        Some(Ok(_))
    ));
}

#[test]
fn duplicate_feature_result_members_skip_without_entity_charge() {
    let source = test_feature(0, 1, &[(0, 1)]);
    let properties = [
        test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[3, 3]),
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
    ];
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(super::feature_result(&ctx, &source, 0, &index).is_none());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert!(super::feature_result(&ctx, &source, 0, &index).is_none());
}

#[test]
fn feature_result_refuses_collection_limit_before_distinct_precheck() {
    let source = test_feature(0, 1, &[(0, 1)]);
    let properties = [
        test_property(
            1,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[3, 3]),
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) },
        ),
    ];
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(matches!(
        super::feature_result(&ctx, &source, 0, &index),
        Some(Err(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "precheck distinct Inventor feature result bodies"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert!(super::feature_result(&ctx, &source, 0, &index).is_none());
}

#[test]
fn feature_projection_refuses_entity_limit_before_creation() {
    let source = test_feature(0, 0, &[]);
    let label = test_label(0, 1, EXTRUSION_CLASS_ID, &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    assert!(matches!(
        super::admit_projected_feature(&ctx, &source, &label, "extrude"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "project Inventor feature"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service projection context");
    assert!(super::admit_projected_feature(&ctx, &source, &label, "extrude").is_ok());
}

fn inventory_with_record(
    type_id: [u8; 16],
    payload: &[u8],
    policy: DecodePolicy,
) -> Result<super::FeatureInventory, CodecError> {
    let bytes = primary_envelope_fixture();
    let payload = payload.to_vec();
    let arena = DecodeArena::new();
    let (setup_ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("envelope view");
    let mut container = InventorContainer::open(&setup_ctx, source).expect("framed envelope");
    let segment = &mut container.rse.segments[0];
    segment.kind = SegmentKind::PmDc;
    let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
        panic!("framed bulk fixture");
    };
    let RecordFrameState::Framed(table) = &mut bulk.records else {
        panic!("framed record fixture");
    };
    table.records[0].type_id = type_id;
    table.records[0].payload = View::over_retained(&payload);
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input view");
    inventory(&ctx, &container.rse)
}

#[test]
fn feature_terminator_refuses_collection_limit_before_push() {
    let mut payload = content(0);
    payload.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        inventory_with_record(END_OF_FEATURES_TYPE, &payload, DecodePolicy::service())
            .expect("terminator is admitted")
            .terminators
            .len(),
        1
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(matches!(
        inventory_with_record(END_OF_FEATURES_TYPE, &payload, policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admit Inventor PmDc feature terminator record"
                && limit.used == 0
    ));
}

#[test]
fn feature_terminator_refuses_retained_limit_before_identity_copy() {
    let mut payload = content(0);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 31;
    assert!(matches!(
        inventory_with_record(END_OF_FEATURES_TYPE, &payload, policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor PmDc record type id"
                && limit.used == 0
    ));
    policy.limits.max_retained_bytes = 32;
    assert!(matches!(
        inventory_with_record(END_OF_FEATURES_TYPE, &payload, policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor PmDc record segment token"
                && limit.used == 32
    ));
}

#[test]
fn feature_parse_issue_refuses_collection_limit_before_push() {
    assert_eq!(
        inventory_with_record(END_OF_FEATURES_TYPE, &[], DecodePolicy::service())
            .expect("truncated terminator becomes an issue")
            .issues
            .len(),
        1
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(matches!(
        inventory_with_record(END_OF_FEATURES_TYPE, &[], policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admit Inventor PmDc feature issue"
                && limit.used == 0
    ));
}

#[test]
fn feature_parse_issue_refuses_entity_limit_before_push() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    assert!(matches!(
        inventory_with_record(END_OF_FEATURES_TYPE, &[], policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor PmDc feature issue"
    ));
    assert_eq!(
        inventory_with_record(END_OF_FEATURES_TYPE, &[], DecodePolicy::service())
            .expect("service issue")
            .issues
            .len(),
        1
    );
}

#[test]
fn feature_parse_issue_copies_refuse_retained_limits_before_creation() {
    let detail_len = inventory_with_record(END_OF_FEATURES_TYPE, &[], DecodePolicy::service())
        .expect("truncated terminator becomes an issue")
        .issues[0]
        .detail
        .len();
    for (limit_bytes, operation, used) in [
        (
            detail_len - 1,
            "retain Inventor PmDc feature issue detail",
            0,
        ),
        (
            detail_len,
            "retain Inventor PmDc feature issue type id",
            detail_len,
        ),
        (
            detail_len + 32,
            "retain Inventor PmDc feature issue segment token",
            detail_len + 32,
        ),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(limit_bytes);
        assert!(matches!(
            inventory_with_record(END_OF_FEATURES_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == operation
                    && limit.used == cadmpeg_core::decode::u64_from_index(used)
        ));
    }
}

pub(super) fn segment() -> cadmpeg_ir::ids::IdentityKey {
    cadmpeg_ir::identity_key!("generated")
}

fn references(values: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000u16.to_le_bytes());
    bytes.extend_from_slice(
        &(u32::try_from(values.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    if !values.is_empty() {
        bytes.extend_from_slice(
            &(u32::try_from(values.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(&0u32.to_le_bytes());
    }
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn utf16(value: &str) -> Vec<u8> {
    let units = value.encode_utf16().collect::<Vec<_>>();
    let mut bytes = (u32::try_from(units.len()).expect("fixture value fits u32"))
        .to_le_bytes()
        .to_vec();
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

pub(super) fn reference(index: u32) -> crate::pmdc::PmDcReference {
    crate::pmdc::PmDcReference::new(index, index != 0).expect("test reference index fits 31 bits")
}

pub(super) fn reference_list(values: &[u32]) -> PmDcReferenceList {
    PmDcReferenceList::new(
        2,
        (!values.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([
            u32::try_from(values.len()).expect("fixture value fits u32"),
            0,
        ])),
        values.iter().copied().map(reference).collect(),
    )
    .expect("test list metadata matches length")
}

pub(super) fn test_header() -> PmDcContentHeader {
    PmDcContentHeader {
        header_value: 0,
        header_id: 0,
        next: reference(0),
        flags: 0,
        context: reference(0),
        source_index: 0,
    }
}

fn test_property(ordinal: u32, kind: PmDcFeaturePropertyKind) -> PmDcFeatureProperty {
    Located::new(
        PmDcFeaturePropertyPayload {
            save_version_major: 16,
            header: test_header(),
            kind,
        },
        crate::record_identity::RecordTypeId::try_from(format!("{ordinal:032x}"))
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

pub(super) fn test_feature(ordinal: u32, slot_count: usize, slots: &[(usize, u32)]) -> PmDcFeature {
    let mut references = vec![reference(0); slot_count];
    for (slot, record_ordinal) in slots {
        references[*slot] = reference(record_ordinal + 1);
    }
    Located::new(
        PmDcFeaturePayload {
            save_version_major: 16,
            header: test_header(),
            state: 69,
            outline_value: 0,
            properties: PmDcReferenceList::new(
                2,
                (!references.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([
                    u32::try_from(slot_count).expect("fixture value fits u32"),
                    0,
                ])),
                references,
            )
            .expect("test list metadata matches length"),
            value: 0,
        },
        crate::record_identity::RecordTypeId::from_bytes(FEATURE_TYPE),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        ordinal,
    )
}

#[test]
fn located_label_retains_empty_name_and_rejects_wrong_class_width() {
    let label = test_label(0, 1, EXTRUSION_CLASS_ID, &[]);
    let valid = serde_json::to_value(&label).expect("valid label fixture");
    let admitted: PmDcFeatureLabel =
        serde_json::from_value(valid.clone()).expect("valid label fixture");
    assert_eq!(
        serde_json::to_value(admitted).expect("valid label fixture"),
        valid
    );
    for (field, value) in [("class_id", "a".repeat(31)), ("class_id", "a".repeat(33))] {
        let mut wire = valid.clone();
        wire[field] = serde_json::json!(value);
        assert!(serde_json::from_value::<PmDcFeatureLabel>(wire)
            .expect_err("invalid label")
            .to_string()
            .contains(field));
    }
    let mut unnamed = valid.clone();
    unnamed["name"] = serde_json::json!("");
    let label: PmDcFeatureLabel = serde_json::from_value(unnamed.clone()).expect("optional label");
    assert!(label.name().is_none());
    assert_eq!(serde_json::to_value(label).expect("label JSON"), unnamed);
    let mut wire = valid;
    wire["class_id"] = serde_json::json!("z".repeat(32));
    assert!(serde_json::from_value::<PmDcFeatureLabel>(wire).is_err());
}

pub(super) fn test_label(
    owner_ordinal: u32,
    index: u32,
    class_id: ClassId,
    participants: &[u32],
) -> PmDcFeatureLabel {
    Located::new(
        PmDcFeatureLabelPayload::try_from(PmDcFeatureLabelPayloadWire {
            save_version_major: 16,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 0,
                values: [0; 2],
                owner: reference(owner_ordinal + 1),
                parent: reference(0),
                next: reference(0),
            },
            index,
            participants: reference_list(participants),
            name: format!("Feature {index}"),
            class_id: class_id.into(),
        })
        .expect("valid label fixture"),
        crate::record_identity::RecordTypeId::from_bytes(FEATURE_LABEL_TYPE),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        owner_ordinal + 1000,
    )
}

fn raw_parameter(ordinal: u32) -> crate::design::PmDcParameter {
    Located::new(
        crate::design::PmDcParameterPayload {
            save_version_major: 16,
            header: crate::pmdc::PmDcContentHeader {
                header_value: 0,
                header_id: 0,
                next: reference(0),
                flags: 0,
                context: reference(0),
                source_index: ordinal,
            },
            name: format!("p{ordinal}"),
            name_value: 0,
            unit: reference(0),
            formula: reference(0),
            nominal_value: cadmpeg_ir::scalar::FiniteReal::ZERO,
            model_value: cadmpeg_ir::scalar::FiniteReal::ZERO,
            tolerance: 0,
            terminal_value: 0,
        },
        crate::record_identity::RecordTypeId::try_from(
            "264d8790d011f8d10008cabc0663dc09".to_owned(),
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

fn neutral_parameter(raw: &crate::design::PmDcParameter, value: ParameterValue) -> DesignParameter {
    DesignParameter {
        id: ParameterId::mint(format!(
            "inventor:design:parameter#{}",
            raw.identity.record_ordinal
        ))
        .expect("identity grammar"),
        owner: None,
        ordinal: raw.identity.record_ordinal,
        name: raw.name.clone(),
        expression: String::new(),
        display: None,
        value: Some(value),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(raw.id()),
    }
}

// The fixture builder keeps each independently indexed record family explicit.
#[allow(clippy::too_many_arguments)]
fn test_projection_index<'a>(
    properties: &'a [PmDcFeatureProperty],
    parameters: &'a [crate::design::PmDcParameter],
    neutral_parameters: &'a [DesignParameter],
    sketches: &'a [crate::sketch::PmDcSketch],
    neutral_sketches: &'a [Sketch],
    directions: &'a [crate::sketch::PmDcDirection],
    transforms: &'a [crate::sketch::PmDcTransform],
    entity_style_links: &'a [PmDcEntityStyleLink],
) -> ProjectionIndex<'a> {
    ProjectionIndex {
        properties: properties
            .iter()
            .map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    Some(record),
                )
            })
            .collect(),
        parameters: parameters
            .iter()
            .map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    Some(record),
                )
            })
            .collect(),
        parameter_values: neutral_parameters
            .iter()
            .filter_map(|parameter| {
                Some((parameter.native_ref.as_deref()?, parameter.value.as_ref()?))
            })
            .collect(),
        sketches: sketches
            .iter()
            .map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    Some(record),
                )
            })
            .collect(),
        sketch_ids: neutral_sketches
            .iter()
            .filter_map(|sketch| {
                sketch
                    .native_ref
                    .as_deref()
                    .map(|native| (native, sketch.id.clone()))
            })
            .collect(),
        directions: directions
            .iter()
            .map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    Some(record),
                )
            })
            .collect(),
        transforms: transforms
            .iter()
            .map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    Some(record),
                )
            })
            .collect(),
        entity_style_links: entity_style_links
            .iter()
            .map(|record| {
                (
                    record.identity.segment_token.as_str(),
                    record.identity.record_ordinal,
                )
            })
            .collect(),
        work_planes: std::collections::HashMap::new(),
        containing_labels: std::collections::HashMap::new(),
        sketch_entities: std::collections::HashMap::new(),
        sketch_entity_ids: std::collections::HashMap::new(),
        features: std::collections::HashMap::new(),
        owner_labels: std::collections::HashMap::new(),
        sheet_metal_groups: std::collections::HashMap::new(),
    }
}

#[test]
fn parses_generated_feature_and_terminator() {
    let mut feature = content(7);
    feature.extend_from_slice(&(-1i32).to_le_bytes());
    feature.extend_from_slice(&42u32.to_le_bytes());
    feature.extend_from_slice(&2u16.to_le_bytes());
    feature.extend_from_slice(&0x3000u16.to_le_bytes());
    feature.extend_from_slice(&2u32.to_le_bytes());
    feature.extend_from_slice(&[0; 8]);
    feature.extend_from_slice(&0x8000_0004u32.to_le_bytes());
    feature.extend_from_slice(&5u32.to_le_bytes());
    feature.extend_from_slice(&9u32.to_le_bytes());
    let parsed = parse(&feature, |ctx, source| {
        parse_feature(ctx, source, 16).expect("feature")
    });
    assert_eq!(parsed.state, -1);
    assert_eq!(parsed.outline_value, 42);
    assert_eq!(parsed.properties.references().len(), 2);
    assert!(parsed.properties.references()[0].qualified());
    assert_eq!(parsed.value, 9);

    let mut terminator = content(8);
    terminator.extend_from_slice(&(-1i32).to_le_bytes());
    let parsed = parse(&terminator, |_, source| {
        parse_terminator(source, 16).expect("terminator")
    });
    assert_eq!(parsed.state, -1);
}

fn pattern_feature_bytes(version: u8, family: PmDcPatternFamily) -> Vec<u8> {
    let mut bytes = content(21);
    bytes.extend_from_slice(&69u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&references(&[]));
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&references(&[0x8000_0010, 0x8000_0011]));
    for index in 0..6 {
        bytes.extend_from_slice(&(0x8000_0020u32 + index).to_le_bytes());
    }
    bytes.push(1);
    match family {
        PmDcPatternFamily::Rectangular => {
            let remaining = if version > 20 { 26 } else { 20 };
            for index in 0..remaining {
                bytes.extend_from_slice(&(0x8000_0040u32 + index).to_le_bytes());
            }
        }
        PmDcPatternFamily::Mirror => {
            for index in 0..5 {
                bytes.extend_from_slice(&(0x8000_0040u32 + index).to_le_bytes());
            }
            if version > 20 {
                for index in 0..6 {
                    bytes.extend_from_slice(&(0x100u32 + index).to_le_bytes());
                }
            }
            for index in 0..2 {
                bytes.extend_from_slice(&(0x8000_0050u32 + index).to_le_bytes());
            }
        }
    }
    bytes
}

fn feature_label_bytes() -> Vec<u8> {
    let mut label = Vec::new();
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&18u16.to_le_bytes());
    label.extend_from_slice(&[0; 20]);
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend(references(&[]));
    label.extend(utf16("a"));
    label.extend_from_slice(&[0xab; 16]);
    label
}

#[test]
fn feature_label_class_id_refuses_retained_limit_before_hex_copy() {
    let bytes = feature_label_bytes();
    let arena = DecodeArena::new();
    let (ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("label view");
    assert_eq!(
        parse_label(&ctx, source, 22)
            .expect("label is admitted")
            .name
            .as_str(),
        "a"
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 32;
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("label view");
    assert!(matches!(
        parse_label(&ctx, source, 22),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor feature label class id"
                && limit.used == 1
    ));
}

#[test]
fn feature_record_forms_refuse_collection_limit_before_push() {
    let mut feature = content(0);
    feature.extend_from_slice(&[0; 8]);
    feature.extend(references(&[]));
    feature.extend_from_slice(&0u32.to_le_bytes());
    let mut property = content(0);
    property.extend(utf16("a"));
    property.extend_from_slice(&0u32.to_le_bytes());
    property.push(1);
    let mut entity_link = Vec::new();
    entity_link.extend_from_slice(&0u32.to_le_bytes());
    entity_link.extend_from_slice(&16u16.to_le_bytes());
    entity_link.extend_from_slice(&[0; 32]);
    let cases = [
        (
            FEATURE_TYPE,
            feature,
            0,
            "admit Inventor PmDc feature record",
        ),
        (
            RECTANGULAR_PATTERN_FEATURE_TYPE,
            pattern_feature_bytes(16, PmDcPatternFamily::Rectangular),
            28,
            "admit Inventor PmDc pattern feature record",
        ),
        (
            MIRROR_FEATURE_TYPE,
            pattern_feature_bytes(16, PmDcPatternFamily::Mirror),
            15,
            "admit Inventor PmDc pattern feature record",
        ),
        (
            FEATURE_LABEL_TYPE,
            feature_label_bytes(),
            0,
            "admit Inventor PmDc feature label record",
        ),
        (
            ENTITY_STYLE_LINK_TYPE,
            entity_link,
            0,
            "admit Inventor PmDc entity style link record",
        ),
        (
            BOOLEAN_TYPE,
            property,
            0,
            "admit Inventor PmDc feature property record",
        ),
    ];
    for (type_id, payload, parser_items, operation) in cases {
        let admitted = inventory_with_record(type_id, &payload, DecodePolicy::service())
            .expect("feature record is admitted");
        assert_eq!(
            admitted.features.len()
                + admitted.pattern_features.len()
                + admitted.labels.len()
                + admitted.entity_style_links.len()
                + admitted.properties.len(),
            1,
            "{operation}"
        );
        assert!(admitted.issues.is_empty());
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = parser_items;
        assert!(matches!(
            inventory_with_record(type_id, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
                    && limit.used == parser_items
        ));
    }
}

#[test]
fn parses_generated_pattern_feature_branches() {
    for (version, family, slots, extensions) in [
        (16, PmDcPatternFamily::Rectangular, 26, 0),
        (21, PmDcPatternFamily::Rectangular, 32, 0),
        (16, PmDcPatternFamily::Mirror, 13, 0),
        (21, PmDcPatternFamily::Mirror, 13, 6),
    ] {
        let bytes = pattern_feature_bytes(version, family);
        let parsed = parse(&bytes, |ctx, source| {
            parse_pattern_feature(ctx, source, version, family).expect("pattern feature")
        });
        assert_eq!(parsed.family, family);
        assert_eq!(parsed.participants.references().len(), 2);
        assert_eq!(parsed.property_slots.len(), slots);
        assert_eq!(parsed.extension_values.len(), extensions);
    }
}

#[test]
fn pattern_feature_property_slots_refuse_collection_limit_before_allocation() {
    for (version, family, slots) in [
        (16, PmDcPatternFamily::Rectangular, 26),
        (21, PmDcPatternFamily::Rectangular, 32),
        (16, PmDcPatternFamily::Mirror, 13),
        (21, PmDcPatternFamily::Mirror, 13),
    ] {
        let bytes = pattern_feature_bytes(version, family);
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2 + slots - 1;
        let arena = DecodeArena::new();
        let (ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("pattern feature view");
        assert!(matches!(
            parse_pattern_feature(&ctx, source, version, family),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor pattern feature property slots"
                    && limit.used == 2
        ));
    }
}

#[test]
fn pattern_feature_extension_values_refuse_collection_limit_before_allocation() {
    let bytes = pattern_feature_bytes(21, PmDcPatternFamily::Mirror);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2 + 13 + 6 - 1;
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("pattern feature view");
    assert!(matches!(
        parse_pattern_feature(&ctx, source, 21, PmDcPatternFamily::Mirror),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admit Inventor pattern feature extension values"
                && limit.used == 15
    ));
    let (ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("pattern feature view");
    assert_eq!(
        parse_pattern_feature(&ctx, source, 21, PmDcPatternFamily::Mirror)
            .expect("pattern feature is admitted")
            .extension_values
            .len(),
        6
    );
}

#[test]
fn projects_generated_fillet_and_chamfer() {
    let raw_radius = raw_parameter(20);
    let neutral_radius = neutral_parameter(
        &raw_radius,
        ParameterValue::Length(Length::new(2.5).expect("finite length fixture")),
    );
    let fillet_properties = vec![
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
                continuity: reference(7),
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
            6,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
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
    let fillet = test_feature(100, 16, &[(0, 2), (11, 1), (15, 8)]);
    let label = test_label(100, 7, FILLET_CLASS_ID, &[]);
    let index = test_projection_index(
        &fillet_properties,
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
    let (projected, result) = project_fillet(&ctx, &fillet, &label, &index)
        .expect("fillet candidate")
        .expect("fillet projection");
    assert!(matches!(
        projected.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
            if matches!(groups[0].radius, RadiusSpec::Constant { radius: actual_radius } if actual_radius.get() == 2.5)
    ));
    assert_eq!(
        result
            .bodies()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>(),
        vec![fillet_properties[8].id()]
    );

    let raw_distance = raw_parameter(40);
    let neutral_distance = neutral_parameter(
        &raw_distance,
        ParameterValue::Length(Length::new(1.25).expect("finite length fixture")),
    );
    let chamfer_properties = vec![
        test_property(
            31,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items: reference_list(&[34]),
            },
        ),
        test_property(
            32,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Chamfer,
                type_value: 2,
                value: 0,
            },
        ),
        test_property(
            33,
            PmDcFeaturePropertyKind::EdgeItem {
                index_references: PmDcU32List::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                    vec![17],
                )
                .expect("test list metadata matches length"),
                index_reference_value: -1,
                value: 0,
            },
        ),
        test_property(
            34,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ),
        test_property(
            35,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[37]),
            },
        ),
        test_property(
            36,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
    ];
    let chamfer = test_feature(101, 12, &[(0, 31), (2, 40), (4, 32), (5, 34), (11, 35)]);
    let label = test_label(101, 8, CHAMFER_CLASS_ID, &[]);
    let index = test_projection_index(
        &chamfer_properties,
        std::slice::from_ref(&raw_distance),
        std::slice::from_ref(&neutral_distance),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let (projected, _) = project_chamfer(&ctx, &chamfer, &label, &index)
        .expect("chamfer candidate")
        .expect("chamfer projection");
    assert!(matches!(
        projected.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups,
            flip_direction: true
        }) if matches!(groups[0].spec, ChamferSpec::Distance { distance: actual_distance } if actual_distance.get() == 1.25)
    ));
}

fn generated_extrusion(
    selections: &[u32],
    policy: DecodePolicy,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let raw_length = raw_parameter(70);
    let raw_taper = raw_parameter(71);
    let neutral_parameters = vec![
        neutral_parameter(
            &raw_length,
            ParameterValue::Length(Length::new(12.0).expect("finite length fixture")),
        ),
        neutral_parameter(
            &raw_taper,
            ParameterValue::Angle(Angle::new(0.1).expect("finite angle fixture")),
        ),
    ];
    let raw_sketch = Located::new(
        crate::sketch::PmDcSketchPayload {
            save_version_major: 16,
            header: test_header(),
            state: 0,
            count_value: 0,
            entities: PmDcReferenceList::new(8, None, Vec::new()).expect("empty entity list"),
            transform: reference(0),
            direction: reference(0),
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
            save_version_major: 16,
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
        60,
    );
    let entity_link = Located::new(
        PmDcEntityStyleLinkPayload {
            save_version_major: 16,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 0,
                values: [0; 2],
                owner: reference(0),
                parent: reference(0),
                next: reference(0),
            },
            value: 0,
            associative_id: 1,
            entity_type: 1,
        },
        crate::record_identity::RecordTypeId::from_bytes(ENTITY_STYLE_LINK_TYPE),
        segment()
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        51,
    );
    let properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 1,
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch,
                items: reference_list(selections),
            },
        ),
        test_property(
            3,
            PmDcFeaturePropertyKind::ProfileSelection {
                entity_link: reference(52),
                value: 0,
            },
        ),
        test_property(
            4,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ),
        test_property(
            5,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Extent,
                type_value: 11,
                value: 1,
            },
        ),
        test_property(
            6,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: false,
            },
        ),
        test_property(
            7,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(&[9]),
            },
        ),
        test_property(
            8,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: reference(30),
            },
        ),
    ];
    let feature = test_feature(
        100,
        27,
        &[
            (0, 1),
            (1, 2),
            (2, 60),
            (3, 4),
            (4, 70),
            (5, 71),
            (6, 5),
            (7, 6),
            (23, 2),
            (26, 7),
        ],
    );
    let label = test_label(100, 5, EXTRUSION_CLASS_ID, &[51]);
    let raw_parameters = vec![raw_length, raw_taper];
    let index = test_projection_index(
        &properties,
        &raw_parameters,
        &neutral_parameters,
        std::slice::from_ref(&raw_sketch),
        std::slice::from_ref(&neutral_sketch),
        std::slice::from_ref(&direction),
        &[],
        std::slice::from_ref(&entity_link),
    );
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
    project_extrusion(&ctx, &feature, &label, &index)
}

#[test]
fn duplicate_extrusion_selections_skip_before_feature_entity_charge() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    assert!(generated_extrusion(&[4, 4], policy).is_none());
}

#[test]
fn projects_generated_extrusion() {
    let (projected, _) = generated_extrusion(&[4], DecodePolicy::service())
        .expect("extrusion candidate")
        .expect("extrusion projection");
    assert!(matches!(
        projected.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            direction: ExtrudeDirection::Explicit {
                vector: geometry_1,
                ..
            },
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    draft: Some(actual_draft),
                    ..
                }
            },
            op: BooleanOp::NewBody,
            ..
        }) if ( actual_length.get() == 12.0 && actual_draft.get() == 0.1) && matches!(geometry_1.get(), Vector3 { z: -1.0, .. })
    ));
}

#[test]
fn projects_generated_hole() {
    let raw_parameters = (70..76).map(raw_parameter).collect::<Vec<_>>();
    let neutral_parameters = [
        ParameterValue::Length(Length::new(5.0).expect("finite length fixture")),
        ParameterValue::Length(Length::new(20.0).expect("finite length fixture")),
        ParameterValue::Length(Length::new(9.0).expect("finite length fixture")),
        ParameterValue::Length(Length::new(3.0).expect("finite length fixture")),
        ParameterValue::Angle(Angle::new(1.5).expect("finite angle fixture")),
        ParameterValue::Angle(Angle::new(2.0).expect("finite angle fixture")),
    ]
    .into_iter()
    .zip(&raw_parameters)
    .map(|(value, raw)| neutral_parameter(raw, value))
    .collect::<Vec<_>>();
    let transform = Located::new(
        crate::sketch::PmDcTransformPayload {
            save_version_major: 16,
            header: test_header(),
            prefix_present: false,
            matrix: crate::compact_matrix::CompactMatrix::try_from_rows(
                0,
                0,
                [
                    [1.0, 0.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 2.0],
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
            save_version_major: 16,
            header: test_header(),
            entity_flags: 0,
            parameter: cadmpeg_ir::scalar::FiniteReal::ZERO,
            extension: None,
            direction: [
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ZERO,
                cadmpeg_ir::scalar::FiniteReal::ONE.negated(),
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
    let properties = vec![
        test_property(
            1,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Hole,
                type_value: 3,
                value: 2,
            },
        ),
        test_property(
            2,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Extent,
                type_value: 11,
                value: 5,
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
            PmDcFeaturePropertyKind::Placement {
                transform: reference(61),
                point: reference(90),
                value: reference(91),
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
    ];
    let feature = test_feature(
        100,
        25,
        &[
            (0, 1),
            (1, 70),
            (2, 71),
            (3, 72),
            (4, 73),
            (5, 74),
            (6, 75),
            (8, 60),
            (9, 2),
            (16, 61),
            (17, 3),
            (21, 4),
            (24, 5),
        ],
    );
    let label = test_label(100, 9, HOLE_CLASS_ID, &[]);
    let index = test_projection_index(
        &properties,
        &raw_parameters,
        &neutral_parameters,
        &[],
        &[],
        std::slice::from_ref(&direction),
        std::slice::from_ref(&transform),
        &[],
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let (projected, _) = project_hole(&ctx, &feature, &label, &index)
        .expect("hole candidate")
        .expect("hole projection");
    assert!(matches!(
        projected.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            placements,
            shape,

            extent: Some(LinearTermination::ThroughAll {}),
            ..
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: HoleKind::CounterboreDrilled {
                    diameter: actual_diameter,
                    depth: actual_depth,
                    drill_point_angle: actual_drill_point_angle
                },
                ..
            }, Some(actual_diameter_2),) if (matches!(
            placements.as_deref(),
            Some([HolePlacement::Directed {
                position: geometry_1,
                direction: geometry_2
            }])
         if matches!(geometry_1.get(), Point3 { x: 10.0, y: 20.0, z: 30.0 }) && matches!(geometry_2.get(), Vector3 { z: -1.0, .. }))) && actual_diameter.get() == 9.0 && actual_depth.get() == 3.0 && actual_drill_point_angle.get() == 2.0 && actual_diameter_2.get() == 5.0)));
}

#[test]
fn parses_generated_feature_properties_and_label() {
    let mut enumeration = content(10);
    enumeration.extend_from_slice(&5i16.to_le_bytes());
    enumeration.extend_from_slice(&3u16.to_le_bytes());
    let parsed = parse(&enumeration, |ctx, source| {
        parse_part_operation(ctx, source, 16).expect("enumeration")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::PartOperation,
            type_value: 5,
            value: 3
        }
    ));

    let mut chamfer = content(10);
    chamfer.extend_from_slice(&2i16.to_le_bytes());
    chamfer.extend_from_slice(&0u16.to_le_bytes());
    chamfer.extend_from_slice(&0u32.to_le_bytes());
    let parsed = parse(&chamfer, |ctx, source| {
        parse_chamfer(ctx, source, 16).expect("chamfer enumeration")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Chamfer,
            type_value: 2,
            value: 0
        }
    ));

    let mut fillet_selection = content(10);
    fillet_selection.extend_from_slice(&4u32.to_le_bytes());
    fillet_selection.extend_from_slice(&0u32.to_le_bytes());
    let parsed = parse(&fillet_selection, |ctx, source| {
        parse_fillet_edge_selection(ctx, source, 16).expect("fillet edge selection")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::WideEnumeration {
            type_value: 4,
            value: 0
        }
    ));

    let mut boolean = content(11);
    boolean.extend_from_slice(&utf16("solid"));
    boolean.extend_from_slice(&7u32.to_le_bytes());
    boolean.push(1);
    let parsed = parse(&boolean, |ctx, source| {
        parse_boolean(ctx, source, 16).expect("Boolean")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::Boolean { value: true, .. }
    ));

    let mut collection = content(12);
    collection.extend_from_slice(&references(&[0x8000_0004, 0x8000_0005]));
    let parsed = parse(&collection, |ctx, source| {
        parse_boundary_patch(ctx, source, 16).expect("boundary patch")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::References { items, .. }
            if items.references().len() == 2
    ));

    let mut rdx = content(13);
    rdx.extend_from_slice(&utf16("RDxVar1"));
    rdx.extend_from_slice(&0u32.to_le_bytes());
    rdx.extend_from_slice(&2u32.to_le_bytes());
    rdx.extend_from_slice(&3u32.to_le_bytes());
    parse(&rdx, |ctx, source| {
        parse_rdx_variable(ctx, source, 16).expect("RDx variable")
    });

    let mut surface = content(14);
    surface.extend_from_slice(&0x8000_0006u32.to_le_bytes());
    parse(&surface, |ctx, source| {
        parse_surface_body(ctx, source, 16).expect("surface body")
    });

    let mut selection = content(15);
    selection.extend_from_slice(&0x8000_0007u32.to_le_bytes());
    selection.push(0);
    parse(&selection, |ctx, source| {
        parse_profile_selection(ctx, source, 16).expect("profile selection")
    });

    let mut entity_link = Vec::new();
    entity_link.extend_from_slice(&0u32.to_le_bytes());
    entity_link.extend_from_slice(&16u16.to_le_bytes());
    entity_link.extend_from_slice(&0u32.to_le_bytes());
    entity_link.extend_from_slice(&0u32.to_le_bytes());
    entity_link.extend_from_slice(&0x8000_0008u32.to_le_bytes());
    entity_link.extend_from_slice(&0u32.to_le_bytes());
    entity_link.extend_from_slice(&0x8000_0009u32.to_le_bytes());
    entity_link.extend_from_slice(&1u32.to_le_bytes());
    entity_link.extend_from_slice(&2u32.to_le_bytes());
    entity_link.extend_from_slice(&3u32.to_le_bytes());
    let parsed = parse(&entity_link, |_, source| {
        parse_entity_style_link(source, 16).expect("entity-style link")
    });
    assert_eq!(parsed.header.owner.index(), 8);
    assert_eq!(parsed.header.next.index(), 9);
    assert_eq!(parsed.associative_id, 2);

    let mut placement = content(17);
    placement.extend_from_slice(&0x8000_0009u32.to_le_bytes());
    placement.extend_from_slice(&0x8000_000au32.to_le_bytes());
    placement.extend_from_slice(&0x8000_000bu32.to_le_bytes());
    parse(&placement, |ctx, source| {
        parse_placement(ctx, source, 16).expect("placement")
    });

    let mut fillet_set = content(18);
    fillet_set.extend_from_slice(&0x8000_0010u32.to_le_bytes());
    fillet_set.extend_from_slice(&0x8000_0011u32.to_le_bytes());
    fillet_set.extend_from_slice(&0x8000_0012u32.to_le_bytes());
    fillet_set.extend_from_slice(&0x8000_0013u32.to_le_bytes());
    let parsed = parse(&fillet_set, |ctx, source| {
        parse_fillet_edge_set(ctx, source, 16).expect("fillet edge set")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::FilletEdgeSet { radius, .. } if radius.index() == 17
    ));

    let mut edge_item = content(18);
    edge_item.extend_from_slice(&2u16.to_le_bytes());
    edge_item.extend_from_slice(&0x3000u16.to_le_bytes());
    edge_item.extend_from_slice(&1u32.to_le_bytes());
    edge_item.extend_from_slice(&[1u32.to_le_bytes(), 0u32.to_le_bytes()].concat());
    edge_item.extend_from_slice(&42u32.to_le_bytes());
    edge_item.extend_from_slice(&0i32.to_le_bytes());
    edge_item.extend_from_slice(&7u32.to_le_bytes());
    let parsed = parse(&edge_item, |ctx, source| {
        parse_edge_item(ctx, source, 16).expect("edge item")
    });
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::EdgeItem {
            index_reference_value: 0,
            value: 7,
            ..
        }
    ));

    let mut label = Vec::new();
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&18u16.to_le_bytes());
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&0x8000_000du32.to_le_bytes());
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&0u32.to_le_bytes());
    label.extend_from_slice(&4u32.to_le_bytes());
    label.extend_from_slice(&references(&[0x8000_0013]));
    label.extend_from_slice(&utf16("Extrude1"));
    label.extend_from_slice(&[0xabu8; 16]);
    let parsed = parse(&label, |ctx, source| {
        parse_label(ctx, source, 16).expect("label")
    });
    assert_eq!(parsed.name, "Extrude1");
    assert_eq!(parsed.participants.references().len(), 1);
    assert_eq!(
        parsed.class_id,
        ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([0xab; 16]))
    );
}

#[test]
fn class_id_admits_only_the_canonical_lowercase_spelling() {
    let lower = "ab".repeat(16);
    assert_eq!(
        String::from(ClassId::try_from(lower.clone()).expect("lowercase class id")),
        lower
    );
    assert!(ClassId::try_from("AB".repeat(16)).is_err());
}

#[test]
fn class_id_native_writer_streams_the_existing_hex_bytes() {
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        value: &'a ClassId,
    }
    let class_id = ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([0xab; 16]));
    let owned = String::from(class_id);
    assert_eq!(
        serde_json::to_vec(&class_id).expect("borrowed class id"),
        serde_json::to_vec(&owned).expect("owned class id")
    );
    let record = Record {
        id: "inventor:pmdc:class-id#1",
        value: &class_id,
    };
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id": record.id, "value": owned}),
    );
}

#[test]
fn feature_label_native_writer_refuses_retained_limit_before_clone() {
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        value: &'a PmDcFeatureLabelPayload,
    }
    let reference =
        crate::pmdc::PmDcReference::new(1, false).expect("test reference index fits 31 bits");
    let label = PmDcFeatureLabelPayload::try_from(PmDcFeatureLabelPayloadWire {
        save_version_major: 16,
        header: PmDcLinkedHeader {
            header_value: 0,
            header_id: 18,
            values: [0, 0],
            owner: reference,
            parent: reference,
            next: reference,
        },
        index: 3,
        participants: crate::pmdc::PmDcReferenceList::new(
            8,
            Some(crate::pmdc::PmDcListMetadata::U16([1, 2])),
            vec![reference],
        )
        .expect("paired participants"),
        name: "Extrude1".to_owned(),
        class_id: "ab".repeat(16),
    })
    .expect("feature label");
    let owned = PmDcFeatureLabelPayloadWire::from(label.clone());
    assert_eq!(
        serde_json::to_vec(&label).expect("borrowed feature label"),
        serde_json::to_vec(&owned).expect("owned feature label")
    );
    let record = Record {
        id: "inventor:pmdc:feature-label#1",
        value: &label,
    };
    crate::pmdc::PMDC_LIST_CLONE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id": record.id, "value": owned}),
    );
    crate::pmdc::PMDC_LIST_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

mod bodies;
mod extents;
mod holes;
mod patterns;
mod records;
mod sheet_metal;
