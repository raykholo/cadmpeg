// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;

use super::geometry::{self, GeometryShape};
use super::selection::SelectionKey;
use super::selection::{
    imported_face_name, name_map_lookup, parse_face_key, parse_work_feature_key, KeySegmentKind,
    FACE_AXIS_KEY_TYPE, FACE_KEY_TYPE, IMPORTED_FACE_NAME_TYPE, NAME_MAP_TYPE,
    WORK_FEATURE_KEY_TYPE,
};
use super::SegmentRecords;
use super::{
    name_face_key, parse, resolve, ConstraintKind, OperandGeometry, OperandSelection,
    ProjectionHint, CONSTRAINT_TYPE, LINE_TYPE, OCCURRENCE_INSTANCE_TYPE, OPERAND_TYPE,
    PARAMETER_ROLE_TYPE, PLANE_TYPE,
};
use crate::assembly::AssemblyOccurrence;
use crate::pmdc::PmDcReference;
use crate::records::test_frame;
use crate::test_support::test_fixtures::{push_u16, push_u32};

const QUALIFIED: u32 = 0x8000_0000;

fn push_list(bytes: &mut Vec<u8>, references: &[u32]) {
    push_u16(bytes, 2);
    push_u16(bytes, 0x3000);
    let count = u32::try_from(references.len()).expect("fixture count fits u32");
    push_u32(bytes, count);
    if count != 0 {
        push_u32(bytes, count);
        push_u32(bytes, 0);
        for reference in references {
            push_u32(bytes, *reference);
        }
    }
}

fn constraint_bytes(kind: u8, operands: &[u32], parameters: &[u32], limit_state: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 0x3f);
    push_u32(&mut bytes, QUALIFIED | 0x72);
    push_u32(&mut bytes, 0x0200);
    push_u32(&mut bytes, QUALIFIED | 3);
    push_u32(&mut bytes, 16);
    push_u32(&mut bytes, u32::MAX);
    push_u32(&mut bytes, QUALIFIED | 0x27);
    bytes.push(kind);
    push_list(&mut bytes, operands);
    push_list(&mut bytes, parameters);
    bytes.push(0);
    push_u32(&mut bytes, 2);
    push_u32(&mut bytes, limit_state);
    bytes.push(3);
    bytes
}

fn operand_bytes(occurrence: u32, geometry: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 0x60);
    push_u32(&mut bytes, occurrence);
    push_u32(&mut bytes, geometry);
    push_u32(&mut bytes, 0);
    bytes
}

fn parameter_role_bytes(role: u16, parameter: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 0x62);
    push_u16(&mut bytes, role);
    push_u32(&mut bytes, parameter);
    push_u32(&mut bytes, 0);
    bytes
}

fn segment<'s, 'a>(
    records: &'s [crate::records::RseRecordFrame<'a>],
    occurrences: &'s [AssemblyOccurrence],
    keys: &'s [SelectionKey],
    version: Option<(u8, usize)>,
) -> SegmentRecords<'s, 'a> {
    SegmentRecords {
        token: "segment",
        records,
        occurrences,
        keys,
        version,
    }
}

fn context(arena: &DecodeArena) -> DecodeContext<'_> {
    DecodeContext::from_root_bytes(&[], arena, &DecodePolicy::service())
        .expect("service context")
        .0
}

fn displayed(result: Result<impl std::fmt::Debug, CodecError>) -> String {
    match result {
        Ok(value) => format!("unexpected success {value:?}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn parses_a_mate_with_two_operands_and_one_value() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let bytes = constraint_bytes(1, &[QUALIFIED | 2, QUALIFIED | 3], &[QUALIFIED | 4], 0);
    assert_eq!(bytes.len(), 85);
    let payload = parse(&ctx, View::over_retained(&bytes)).expect("mate record");
    assert_eq!(payload.kind, ConstraintKind::Mate);
    assert_eq!(payload.state, -1);
    assert_eq!(payload.owner_reference.record_ordinal(), Some(0x26));
    assert_eq!(
        payload
            .operands
            .iter()
            .map(|reference| reference.record_ordinal())
            .collect::<Vec<_>>(),
        [Some(1), Some(2)]
    );
    assert_eq!(payload.parameters.len(), 1);
    assert_eq!(payload.name_index, 2);
}

#[test]
fn parses_every_defined_kind_and_the_limit_form() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    for (code, kind) in [
        (1, ConstraintKind::Mate),
        (2, ConstraintKind::Flush),
        (3, ConstraintKind::Angle),
        (4, ConstraintKind::Tangent),
        (5, ConstraintKind::Insert),
        (6, ConstraintKind::RotationMotion),
        (11, ConstraintKind::WithoutBrowserEntry),
        (20, ConstraintKind::Symmetry),
    ] {
        let bytes = constraint_bytes(code, &[QUALIFIED | 2, QUALIFIED | 3], &[], 0);
        let payload = parse(&ctx, View::over_retained(&bytes)).expect("defined kind");
        assert_eq!(payload.kind, kind);
        assert!(payload.parameters.is_empty());
    }
    let limited = constraint_bytes(
        2,
        &[QUALIFIED | 2, QUALIFIED | 3],
        &[QUALIFIED | 4, QUALIFIED | 5, QUALIFIED | 6],
        6,
    );
    assert_eq!(limited.len(), 93);
    let payload = parse(&ctx, View::over_retained(&limited)).expect("limited flush");
    assert_eq!(payload.parameters.len(), 3);
    assert_eq!(payload.limit_state, 6);
}

#[test]
fn refuses_undefined_kinds_and_broken_framing() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let undefined = constraint_bytes(7, &[QUALIFIED | 2], &[], 0);
    assert!(displayed(parse(&ctx, View::over_retained(&undefined))).contains("kind 7"));

    let mut terminal = constraint_bytes(1, &[QUALIFIED | 2], &[], 0);
    if let Some(last) = terminal.last_mut() {
        *last = 4;
    }
    assert!(displayed(parse(&ctx, View::over_retained(&terminal))).contains("terminal is 4"));

    let mut trailing = constraint_bytes(1, &[QUALIFIED | 2], &[], 0);
    trailing.push(0);
    assert!(displayed(parse(&ctx, View::over_retained(&trailing))).contains("trailing bytes"));

    let mut metadata = constraint_bytes(1, &[QUALIFIED | 2], &[], 0);
    // The repeated count of the operand list follows the 31-byte prefix,
    // the list marker, and the count.
    metadata[39] = 9;
    assert!(displayed(parse(&ctx, View::over_retained(&metadata))).contains("metadata"));

    let mut marker = constraint_bytes(1, &[QUALIFIED | 2], &[], 0);
    marker[31] = 3;
    assert!(displayed(parse(&ctx, View::over_retained(&marker))).contains("marker"));
}

#[test]
fn resolves_operands_parameters_and_the_occurrence() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let constraint = constraint_bytes(1, &[QUALIFIED | 2, QUALIFIED | 3], &[QUALIFIED | 4], 0);
    let assembly_side = operand_bytes(0, QUALIFIED | 6);
    let part_side = operand_bytes(QUALIFIED | 7, QUALIFIED | 8);
    let role = parameter_role_bytes(3, QUALIFIED | 9);
    let instance = [0_u8; 34];
    let empty: [u8; 0] = [];
    let records = [
        test_frame(0, CONSTRAINT_TYPE, View::over_retained(&constraint)),
        test_frame(1, OPERAND_TYPE, View::over_retained(&assembly_side)),
        test_frame(2, OPERAND_TYPE, View::over_retained(&part_side)),
        test_frame(3, PARAMETER_ROLE_TYPE, View::over_retained(&role)),
        test_frame(4, [0; 16], View::over_retained(&empty)),
        test_frame(5, PLANE_TYPE, View::over_retained(&empty)),
        test_frame(6, OCCURRENCE_INSTANCE_TYPE, View::over_retained(&instance)),
        test_frame(7, LINE_TYPE, View::over_retained(&empty)),
        test_frame(8, [0; 16], View::over_retained(&empty)),
        test_frame(9, [0; 16], View::over_retained(&empty)),
    ];
    let occurrences = [AssemblyOccurrence {
        segment_token: "segment".into(),
        record_ordinal: 9,
        header_value: 0,
        header_id: 0,
        next_reference: 0,
        flags: 0,
        owner_reference: 0,
        node_index: 0,
        state: [0; 2],
        ordinal_key: 0,
        related_references: vec![QUALIFIED | 7],
        child_reference: 0,
        occurrence_id: 77,
    }];
    let payload = parse(&ctx, records[0].payload).expect("mate record");
    let record = resolve(
        &ctx,
        segment(&records, &occurrences, &[], None),
        0,
        &payload,
    )
    .expect("resolved constraint");
    assert_eq!(record.id, "inventor:assembly:constraint#segment-0");
    let [assembly, part] = record.operands.as_slice() else {
        panic!("two operands");
    };
    assert_eq!(assembly.occurrence_reference, None);
    assert_eq!(assembly.occurrence_id, None);
    assert_eq!(assembly.geometry, Some(OperandGeometry::Plane));
    assert_eq!(
        assembly.geometry_type.as_deref(),
        Some("42df52ced011d0d20008ccbc0663dc09")
    );
    assert_eq!(part.occurrence_id, Some(77));
    assert_eq!(part.geometry, Some(OperandGeometry::Line));
    let [value] = record.parameters.as_slice() else {
        panic!("one parameter");
    };
    assert_eq!(value.role, Some(3));
    assert_eq!(
        value
            .parameter_reference
            .and_then(PmDcReference::record_ordinal),
        Some(8)
    );
}

#[test]
fn operands_without_an_operand_record_stay_unresolved() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let constraint = constraint_bytes(4, &[QUALIFIED | 2, QUALIFIED | 0x28], &[], 0);
    let other: [u8; 4] = [0; 4];
    let records = [
        test_frame(0, CONSTRAINT_TYPE, View::over_retained(&constraint)),
        test_frame(1, [1; 16], View::over_retained(&other)),
    ];
    let payload = parse(&ctx, records[0].payload).expect("tangent record");
    let record = resolve(&ctx, segment(&records, &[], &[], None), 0, &payload).expect("resolved");
    assert!(record
        .operands
        .iter()
        .all(|operand| operand.geometry_reference.is_none() && operand.geometry.is_none()));

    let short = operand_bytes(0, QUALIFIED | 1);
    let records = [
        test_frame(0, CONSTRAINT_TYPE, View::over_retained(&constraint)),
        test_frame(1, OPERAND_TYPE, View::over_retained(&short[..17])),
    ];
    let payload = parse(&ctx, records[0].payload).expect("tangent record");
    assert!(displayed(resolve(
        &ctx,
        segment(&records, &[], &[], None),
        0,
        &payload
    ))
    .contains("17 bytes"));
}

fn push_text(bytes: &mut Vec<u8>, text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    push_u32(
        bytes,
        u32::try_from(units.len()).expect("fixture text fits u32"),
    );
    for unit in units {
        push_u16(bytes, unit);
    }
}

fn key_header(bytes: &mut Vec<u8>) {
    push_u32(bytes, 0);
    push_u16(bytes, 0x120);
    push_u32(bytes, 0);
    push_u32(bytes, 0x0200);
    push_u32(bytes, QUALIFIED | 3);
    push_u32(bytes, 0x36);
    push_u32(bytes, u32::MAX);
}

const PART_KEY: &str = "PmDCSegment::partsegment:revision:6";

fn face_key_bytes(axis: bool, geometry: u32, map: u32, name: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    key_header(&mut bytes);
    push_u16(&mut bytes, 0x0500);
    push_u32(&mut bytes, 0);
    push_text(&mut bytes, PART_KEY);
    push_text(&mut bytes, "10");
    push_u16(&mut bytes, 1);
    push_u32(&mut bytes, 2);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 2);
    for _ in 0..2 {
        push_u16(&mut bytes, 8);
        push_u16(&mut bytes, 0x3000);
        push_u32(&mut bytes, 0);
    }
    bytes.push(0);
    if !axis {
        bytes.push(1);
    }
    push_u32(&mut bytes, geometry);
    if !axis {
        push_u32(&mut bytes, 0);
    }
    push_u32(&mut bytes, map);
    push_u32(&mut bytes, name);
    bytes
}

fn name_map_bytes(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 0x1d);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, entries.last().map_or(0, |entry| entry.0));
    push_u32(
        &mut bytes,
        u32::try_from(entries.len()).expect("fixture count"),
    );
    for (key, reference) in entries {
        push_u32(&mut bytes, *key);
        push_u32(&mut bytes, *reference);
    }
    bytes
}

fn imported_name_bytes(groups: &[&[i32]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 0x121);
    push_u16(&mut bytes, 5);
    push_u16(&mut bytes, 0);
    push_list(&mut bytes, &[10]);
    bytes.extend_from_slice(&[0; 20]);
    push_u32(&mut bytes, 180);
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 2);
    push_u16(&mut bytes, 0x3000);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0x1000_0000);
    push_u32(&mut bytes, 15);
    push_u32(&mut bytes, u32::MAX);
    push_u32(&mut bytes, 0);
    push_u32(
        &mut bytes,
        u32::try_from(groups.len()).expect("fixture count"),
    );
    for group in groups {
        push_u32(
            &mut bytes,
            u32::try_from(group.len() + 1).expect("fixture width"),
        );
        for value in *group {
            push_u32(&mut bytes, u32::from_le_bytes(value.to_le_bytes()));
        }
        push_u32(&mut bytes, 0);
    }
    push_u32(&mut bytes, 0);
    bytes
}

fn work_feature_part_bytes(geometry: u32, key: &str, source_index: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    key_header(&mut bytes);
    push_u32(&mut bytes, geometry);
    bytes.push(0);
    bytes.push(5);
    push_u32(&mut bytes, 0);
    push_text(&mut bytes, key);
    push_text(&mut bytes, "62");
    push_u16(&mut bytes, 1);
    push_u32(&mut bytes, source_index);
    push_u32(&mut bytes, 0);
    bytes
}

fn work_feature_local_bytes(geometry: u32, target: u32, source_index: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    key_header(&mut bytes);
    push_u32(&mut bytes, geometry);
    bytes.push(1);
    bytes.push(5);
    push_u32(&mut bytes, target);
    push_u32(&mut bytes, source_index);
    push_u32(&mut bytes, 0);
    bytes
}

#[test]
fn face_keys_follow_the_name_map_to_an_imported_face_name() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let key = face_key_bytes(false, QUALIFIED | 2, QUALIFIED | 3, 8);
    let map = name_map_bytes(&[(7, QUALIFIED | 5), (8, QUALIFIED | 4)]);
    let name = imported_name_bytes(&[&[40, 0, 700], &[40, 0, 701]]);
    let empty: [u8; 0] = [];
    let records = [
        test_frame(0, FACE_KEY_TYPE, View::over_retained(&key)),
        test_frame(1, PLANE_TYPE, View::over_retained(&empty)),
        test_frame(2, NAME_MAP_TYPE, View::over_retained(&map)),
        test_frame(3, IMPORTED_FACE_NAME_TYPE, View::over_retained(&name)),
        test_frame(4, [0; 16], View::over_retained(&empty)),
    ];
    let mut parsed = parse_face_key(&ctx, &records[0], false, Some(25)).expect("face key");
    assert_eq!(parsed.geometry_ordinal, Some(1));
    assert_eq!(parsed.hint, ProjectionHint::PartFace);
    name_face_key(&ctx, &mut parsed, &records).expect("named face key");
    let OperandSelection::Face {
        part,
        values,
        name_key,
        name_record,
        name_type,
        imported_name,
        ..
    } = &parsed.selection
    else {
        panic!("face selection");
    };
    assert_eq!(part.segment_kind, KeySegmentKind::Part);
    assert_eq!(part.part_segment, "partsegment");
    assert_eq!(part.revision_key, "revision");
    assert_eq!(part.key_number, "6");
    assert_eq!(part.occurrence_key, "10");
    assert_eq!(*values, [2, 2]);
    assert_eq!(*name_key, 8);
    assert_eq!(*name_record, Some(3));
    assert_eq!(
        name_type.as_deref(),
        Some("fdd5bd47d34aa6b6ed0a11a3138fd1af")
    );
    assert_eq!(
        imported_name.as_deref(),
        Some([vec![40, 0, 700], vec![40, 0, 701]].as_slice())
    );

    let axis = face_key_bytes(true, QUALIFIED | 2, QUALIFIED | 3, 7);
    let frame = test_frame(0, FACE_AXIS_KEY_TYPE, View::over_retained(&axis));
    let mut parsed = parse_face_key(&ctx, &frame, true, Some(25)).expect("axis key");
    assert_eq!(parsed.hint, ProjectionHint::PartFaceAxis);
    name_face_key(&ctx, &mut parsed, &records).expect("named axis key");
    let OperandSelection::Face {
        name_record,
        imported_name,
        ..
    } = &parsed.selection
    else {
        panic!("face selection");
    };
    // Key 7 names a record of another naming type; only its identity is kept.
    assert_eq!(*name_record, Some(4));
    assert_eq!(*imported_name, None);
}

#[test]
fn work_feature_keys_name_part_subassembly_or_local_geometry() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let part = work_feature_part_bytes(QUALIFIED | 5, "PmDCSegment::seg:rev:9", 4);
    let frame = test_frame(0, WORK_FEATURE_KEY_TYPE, View::over_retained(&part));
    let parsed = parse_work_feature_key(&ctx, &frame, Some(25)).expect("part work feature");
    assert_eq!(parsed.geometry_ordinal, Some(4));
    assert_eq!(parsed.hint, ProjectionHint::PartWorkFeature);
    assert!(matches!(
        parsed.selection,
        OperandSelection::WorkFeature {
            source_index: 4,
            ..
        }
    ));

    let sub = work_feature_part_bytes(QUALIFIED | 5, "AmDcSegment::seg:rev:12", 2);
    let frame = test_frame(0, WORK_FEATURE_KEY_TYPE, View::over_retained(&sub));
    let parsed = parse_work_feature_key(&ctx, &frame, Some(25)).expect("subassembly work feature");
    assert_eq!(parsed.hint, ProjectionHint::SubassemblyWorkFeature);

    let local = work_feature_local_bytes(QUALIFIED | 5, QUALIFIED | 0x27, 0x40);
    let frame = test_frame(0, WORK_FEATURE_KEY_TYPE, View::over_retained(&local));
    let parsed = parse_work_feature_key(&ctx, &frame, Some(25)).expect("assembly geometry");
    assert_eq!(parsed.hint, ProjectionHint::AssemblyGeometry);
    let OperandSelection::AssemblyGeometry {
        target_reference,
        target_source_index,
        ..
    } = parsed.selection
    else {
        panic!("assembly geometry");
    };
    assert_eq!(target_reference.record_ordinal(), Some(0x26));
    assert_eq!(target_source_index, 0x40);
}

#[test]
fn selection_keys_refuse_broken_grammar() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let other = work_feature_part_bytes(QUALIFIED | 5, "PmXXSegment::seg:rev:9", 4);
    let frame = test_frame(0, WORK_FEATURE_KEY_TYPE, View::over_retained(&other));
    assert!(displayed(parse_work_feature_key(&ctx, &frame, Some(25))).contains("design segment"));

    let short = work_feature_part_bytes(QUALIFIED | 5, "PmDCSegment::seg:rev", 4);
    let frame = test_frame(0, WORK_FEATURE_KEY_TYPE, View::over_retained(&short));
    assert!(displayed(parse_work_feature_key(&ctx, &frame, Some(25))).contains("three components"));

    let mut form = work_feature_local_bytes(QUALIFIED | 5, QUALIFIED | 0x27, 0x40);
    form[30] = 2;
    let frame = test_frame(0, WORK_FEATURE_KEY_TYPE, View::over_retained(&form));
    assert!(displayed(parse_work_feature_key(&ctx, &frame, Some(25))).contains("form is 2"));

    let mut trailing = face_key_bytes(false, QUALIFIED | 2, QUALIFIED | 3, 8);
    trailing.push(0);
    let frame = test_frame(0, FACE_KEY_TYPE, View::over_retained(&trailing));
    assert!(displayed(parse_face_key(&ctx, &frame, false, Some(25))).contains("trailing bytes"));

    let descending = name_map_bytes(&[(8, QUALIFIED | 4), (7, QUALIFIED | 5)]);
    assert!(
        displayed(name_map_lookup(&ctx, View::over_retained(&descending), 7))
            .contains("not ascending")
    );

    let mut width = imported_name_bytes(&[&[40, 0, 700]]);
    let group = width.len() - 24;
    width[group] = 99;
    assert!(displayed(imported_face_name(&ctx, View::over_retained(&width))).contains("width 99"));
}

#[test]
fn operands_take_the_selection_key_of_their_geometry() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let constraint = constraint_bytes(1, &[QUALIFIED | 2], &[], 0);
    let operand = operand_bytes(0, QUALIFIED | 3);
    let local = work_feature_local_bytes(QUALIFIED | 3, QUALIFIED | 0x27, 0x40);
    let empty: [u8; 0] = [];
    let records = [
        test_frame(0, CONSTRAINT_TYPE, View::over_retained(&constraint)),
        test_frame(1, OPERAND_TYPE, View::over_retained(&operand)),
        test_frame(2, PLANE_TYPE, View::over_retained(&empty)),
        test_frame(3, WORK_FEATURE_KEY_TYPE, View::over_retained(&local)),
    ];
    let key = parse_work_feature_key(&ctx, &records[3], Some(25)).expect("assembly geometry");
    let payload = parse(&ctx, records[0].payload).expect("mate record");
    let record =
        resolve(&ctx, segment(&records, &[], &[key], None), 0, &payload).expect("resolved");
    let [operand] = record.operands.as_slice() else {
        panic!("one operand");
    };
    assert_eq!(
        operand.projection_hint,
        Some(ProjectionHint::AssemblyGeometry)
    );
    assert!(matches!(
        operand.selection,
        Some(OperandSelection::AssemblyGeometry { .. })
    ));
}

fn push_f64s(bytes: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

fn geometry_prefix(extension_len: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 0x140);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0x0002_0200);
    push_u32(&mut bytes, QUALIFIED | 3);
    push_u32(&mut bytes, 0x50);
    for _ in 0..extension_len {
        push_u32(&mut bytes, u32::MAX);
    }
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, QUALIFIED | 0x30);
    push_u32(&mut bytes, 0);
    bytes
}

fn plane_bytes(version: u8, extension_len: usize, marker: bool) -> Vec<u8> {
    let mut bytes = geometry_prefix(extension_len);
    if version >= 22 {
        push_u32(&mut bytes, 0);
    }
    if marker {
        push_u32(&mut bytes, 0x203);
    }
    push_u32(&mut bytes, 0);
    push_f64s(&mut bytes, &[1.5, -2.0, 3.25, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    bytes
}

#[test]
fn operand_geometry_records_store_planes_lines_points_and_directions() {
    for (version, extension_len, marker, length) in [
        (23, 1, false, 118),
        (25, 2, false, 122),
        (23, 1, true, 122),
        (21, 0, false, 110),
    ] {
        let bytes = plane_bytes(version, extension_len, marker);
        assert_eq!(bytes.len(), length);
        let record = geometry::parse(
            View::over_retained(&bytes),
            OperandGeometry::Plane,
            version,
            extension_len,
        )
        .expect("plane");
        assert_eq!(record.frame_marker, marker);
        assert_eq!(record.document_reference.record_ordinal(), Some(0x2f));
        assert_eq!(
            record.shape,
            GeometryShape::Plane {
                origin: [1.5, -2.0, 3.25],
                u_axis: [1.0, 0.0, 0.0],
                v_axis: [0.0, 1.0, 0.0],
            }
        );
    }

    let mut line = geometry_prefix(2);
    push_u32(&mut line, 0);
    push_list(&mut line, &[]);
    push_f64s(&mut line, &[0.5, 0.25, 0.0, 0.0, 0.0, -1.0]);
    line.push(0);
    assert_eq!(line.len(), 103);
    let record =
        geometry::parse(View::over_retained(&line), OperandGeometry::Line, 25, 2).expect("line");
    assert_eq!(
        record.shape,
        GeometryShape::Line {
            origin: [0.5, 0.25, 0.0],
            direction: [0.0, 0.0, -1.0],
        }
    );

    let mut point = geometry_prefix(1);
    push_u32(&mut point, 0);
    push_f64s(&mut point, &[0.0, 0.0, -0.25]);
    push_list(&mut point, &[]);
    push_list(&mut point, &[]);
    let record =
        geometry::parse(View::over_retained(&point), OperandGeometry::Point, 23, 1).expect("point");
    assert_eq!(
        record.shape,
        GeometryShape::Point {
            position: [0.0, 0.0, -0.25]
        }
    );

    let mut direction = geometry_prefix(2);
    push_u32(&mut direction, 0);
    push_f64s(&mut direction, &[0.0, -1.0, 0.0]);
    assert_eq!(direction.len(), 70);
    let record = geometry::parse(
        View::over_retained(&direction),
        OperandGeometry::Direction,
        25,
        2,
    )
    .expect("direction");
    assert_eq!(
        record.shape,
        GeometryShape::Direction {
            vector: [0.0, -1.0, 0.0]
        }
    );
}

#[test]
fn operand_geometry_records_refuse_broken_grammar() {
    let mut trailing = plane_bytes(25, 2, false);
    trailing.push(0);
    assert!(displayed(geometry::parse(
        View::over_retained(&trailing),
        OperandGeometry::Plane,
        25,
        2
    ))
    .contains("trailing bytes"));

    let mut infinite = plane_bytes(25, 2, false);
    let origin = infinite.len() - 72;
    infinite[origin..origin + 8].copy_from_slice(&f64::INFINITY.to_le_bytes());
    assert!(displayed(geometry::parse(
        View::over_retained(&infinite),
        OperandGeometry::Plane,
        25,
        2
    ))
    .contains("not finite"));

    let mut line = geometry_prefix(1);
    push_u32(&mut line, 0);
    push_list(&mut line, &[QUALIFIED | 4]);
    push_f64s(&mut line, &[0.0; 6]);
    line.push(0);
    assert!(displayed(geometry::parse(
        View::over_retained(&line),
        OperandGeometry::Line,
        23,
        1
    ))
    .contains("empty reference list"));
}

#[test]
fn operands_store_their_geometry_at_a_supported_version() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let constraint = constraint_bytes(1, &[QUALIFIED | 2], &[], 0);
    let operand = operand_bytes(0, QUALIFIED | 3);
    let plane = plane_bytes(25, 2, false);
    let records = [
        test_frame(0, CONSTRAINT_TYPE, View::over_retained(&constraint)),
        test_frame(1, OPERAND_TYPE, View::over_retained(&operand)),
        test_frame(2, PLANE_TYPE, View::over_retained(&plane)),
    ];
    let payload = parse(&ctx, records[0].payload).expect("mate record");
    let record = resolve(
        &ctx,
        segment(&records, &[], &[], Some((25, 2))),
        0,
        &payload,
    )
    .expect("resolved");
    let [operand] = record.operands.as_slice() else {
        panic!("one operand");
    };
    assert!(matches!(
        operand
            .stored_geometry
            .as_ref()
            .map(|geometry| &geometry.shape),
        Some(GeometryShape::Plane { .. })
    ));
    let record = resolve(&ctx, segment(&records, &[], &[], None), 0, &payload).expect("resolved");
    assert!(record
        .operands
        .iter()
        .all(|operand| operand.stored_geometry.is_none()));
}

/// The byte offset of the face-key value separator in `face_key_bytes`.
fn face_key_separator_offset() -> usize {
    26 + 2 + 4 + (4 + 2 * PART_KEY.len()) + (4 + 2 * 2) + 2 + 4
}

#[test]
fn selection_keys_at_version_19_store_no_separator_or_trailer() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let mut face = face_key_bytes(false, QUALIFIED | 2, QUALIFIED | 3, 8);
    let separator = face_key_separator_offset();
    assert_eq!(&face[separator..separator + 4], &[0; 4]);
    face.drain(separator..separator + 4);
    let frame = test_frame(0, FACE_KEY_TYPE, View::over_retained(&face));
    let parsed = parse_face_key(&ctx, &frame, false, Some(19)).expect("version 19 face key");
    let OperandSelection::Face { values, .. } = parsed.selection else {
        panic!("face selection");
    };
    assert_eq!(values, [2, 2]);
    assert!(displayed(parse_face_key(&ctx, &frame, false, Some(25))).contains("separator is 2"));

    for mut bytes in [
        work_feature_part_bytes(QUALIFIED | 5, PART_KEY, 4),
        work_feature_local_bytes(QUALIFIED | 5, QUALIFIED | 0x27, 0x40),
    ] {
        bytes.truncate(bytes.len() - 4);
        let frame = test_frame(0, WORK_FEATURE_KEY_TYPE, View::over_retained(&bytes));
        parse_work_feature_key(&ctx, &frame, Some(19)).expect("version 19 work-feature key");
        assert!(displayed(parse_work_feature_key(&ctx, &frame, Some(25))).contains("trailer"));
    }

    for version in [Some(21), Some(15), None] {
        let frame = test_frame(0, FACE_KEY_TYPE, View::over_retained(&face));
        assert!(
            displayed(parse_face_key(&ctx, &frame, false, version)).contains("not established"),
            "{version:?}"
        );
        let local = work_feature_local_bytes(QUALIFIED | 5, QUALIFIED | 0x27, 0x40);
        let frame = test_frame(0, WORK_FEATURE_KEY_TYPE, View::over_retained(&local));
        assert!(
            displayed(parse_work_feature_key(&ctx, &frame, version)).contains("not established"),
            "{version:?}"
        );
    }
}

#[test]
fn line_point_and_direction_geometry_at_version_19_store_no_value_word() {
    let mut line = geometry_prefix(0);
    push_list(&mut line, &[]);
    push_f64s(&mut line, &[0.5, 0.25, 0.0, 0.0, 0.0, -1.0]);
    line.push(0);
    assert_eq!(line.len(), 91);
    let record =
        geometry::parse(View::over_retained(&line), OperandGeometry::Line, 19, 0).expect("line");
    assert_eq!(
        record.shape,
        GeometryShape::Line {
            origin: [0.5, 0.25, 0.0],
            direction: [0.0, 0.0, -1.0],
        }
    );
    assert!(displayed(geometry::parse(
        View::over_retained(&line),
        OperandGeometry::Line,
        21,
        0
    ))
    .contains("not established"));

    let mut point = geometry_prefix(0);
    push_f64s(&mut point, &[7.5, -11.0, 14.0]);
    push_list(&mut point, &[]);
    push_list(&mut point, &[]);
    assert_eq!(point.len(), 74);
    let record =
        geometry::parse(View::over_retained(&point), OperandGeometry::Point, 19, 0).expect("point");
    assert_eq!(
        record.shape,
        GeometryShape::Point {
            position: [7.5, -11.0, 14.0]
        }
    );

    let mut direction = geometry_prefix(0);
    push_f64s(&mut direction, &[0.0, 0.0, -1.0]);
    assert_eq!(direction.len(), 58);
    let record = geometry::parse(
        View::over_retained(&direction),
        OperandGeometry::Direction,
        19,
        0,
    )
    .expect("direction");
    assert_eq!(
        record.shape,
        GeometryShape::Direction {
            vector: [0.0, 0.0, -1.0]
        }
    );
}

#[test]
fn an_undecoded_operand_geometry_keeps_its_constraint() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let constraint = constraint_bytes(1, &[QUALIFIED | 2], &[], 0);
    let operand = operand_bytes(0, QUALIFIED | 3);
    let mut line = geometry_prefix(0);
    push_list(&mut line, &[]);
    push_f64s(&mut line, &[0.0; 6]);
    line.push(0);
    let records = [
        test_frame(0, CONSTRAINT_TYPE, View::over_retained(&constraint)),
        test_frame(1, OPERAND_TYPE, View::over_retained(&operand)),
        test_frame(2, LINE_TYPE, View::over_retained(&line)),
    ];
    let payload = parse(&ctx, records[0].payload).expect("mate record");
    let record = resolve(
        &ctx,
        segment(&records, &[], &[], Some((21, 0))),
        0,
        &payload,
    )
    .expect("the constraint resolves without its geometry");
    let [operand] = record.operands.as_slice() else {
        panic!("one operand");
    };
    assert!(operand.stored_geometry.is_none());
    assert!(operand
        .geometry_issue
        .as_deref()
        .is_some_and(|detail| detail.contains("not established")));
}
