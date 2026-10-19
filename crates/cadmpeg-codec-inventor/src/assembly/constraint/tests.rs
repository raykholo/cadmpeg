// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;

use super::{
    parse, resolve, ConstraintKind, OperandGeometry, CONSTRAINT_TYPE, LINE_TYPE,
    OCCURRENCE_INSTANCE_TYPE, OPERAND_TYPE, PARAMETER_ROLE_TYPE, PLANE_TYPE,
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
    let record =
        resolve(&ctx, "segment", 0, &payload, &records, &occurrences).expect("resolved constraint");
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
    let record = resolve(&ctx, "segment", 0, &payload, &records, &[]).expect("resolved");
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
    assert!(displayed(resolve(&ctx, "segment", 0, &payload, &records, &[])).contains("17 bytes"));
}
