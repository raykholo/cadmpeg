// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::products::{JointOperands, OperandContainer};
use serde_json::Value;

use crate::test_support::{
    assembly_document_with_list, assembly_document_with_relations, relation_list_body,
    OccurrenceSpec, RelationSpec, SiteSpec,
};
use crate::SolidEdgeCodec;

/// Agreement of a computed value with the hand-computed one.
const EPS_VALUE: f64 = 1e-9;

/// The occurrence objects, the pick objects and the relation objects.
const FIRST: u32 = 20;
const SECOND: u32 = 21;
const PICKS: [u32; 2] = [40, 41];
const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn sites() -> [SiteSpec<'static>; 2] {
    [
        SiteSpec {
            id: 10,
            links: Some(["C:\\x\\a.par", "a.par"]),
            long: [None, None],
        },
        SiteSpec {
            id: 11,
            links: Some(["C:\\x\\b.par", "b.par"]),
            long: [None, None],
        },
    ]
}

/// The two occurrences: the first grounded at identity and the second free,
/// 10 mm along x.
fn occurrences() -> [OccurrenceSpec; 2] {
    let mut first = OccurrenceSpec::at_identity(FIRST, 10);
    first.state = 2;
    let mut second = OccurrenceSpec::at_identity(SECOND, 11);
    second.state = 4;
    second.translation = [0.01, 0.0, 0.0];
    [first, second]
}

fn ground_first() -> RelationSpec {
    RelationSpec::Ground {
        id: 30,
        reference: FIRST,
        rotation: IDENTITY,
        translation: [0.0; 3],
    }
}

fn decode_with(
    occurrences: &[OccurrenceSpec],
    relations: &[RelationSpec],
    picks: &[u32],
) -> DecodeResult {
    let listed: Vec<u32> = relations.iter().map(RelationSpec::id).collect();
    let bytes = assembly_document_with_relations(occurrences, &sites(), &listed, relations, picks);
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn decode(relations: &[RelationSpec]) -> DecodeResult {
    decode_with(&occurrences(), relations, &PICKS)
}

fn count(result: &DecodeResult, code: &str) -> usize {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == code)
        .count()
}

/// The records of the native arena of relations.
fn records(result: &DecodeResult) -> Vec<serde_json::Map<String, Value>> {
    result
        .ir()
        .native
        .namespace("solidedge")
        .and_then(|namespace| namespace.arenas().get("assembly_relations"))
        .map(|records| {
            records
                .iter()
                .map(|record| record.fields().unwrap())
                .collect()
        })
        .unwrap_or_default()
}

fn planar(flags: u32, offset: f64) -> RelationSpec {
    RelationSpec::Planar {
        id: 31,
        operands: [SECOND, FIRST],
        picks: PICKS,
        offset,
        flags,
        dimension: 50,
    }
}

#[test]
fn a_ground_relation_states_a_grounded_joint_at_the_occurrence_placement() {
    let result = decode(&[ground_first()]);
    let joints = &result.ir().model.assembly_joints;
    let [joint] = joints.as_slice() else {
        panic!("one joint")
    };
    let JointOperands::Grounded {
        connector,
        offset_frame,
    } = joint.operands()
    else {
        panic!("a grounded joint")
    };
    assert!(offset_frame.is_none());
    assert!(!connector.detached);
    let occurrence = &result.ir().model.occurrences[0];
    assert_eq!(connector.frame, occurrence.transform.unwrap());
    let OperandContainer::Occurrence {
        occurrence: named, ..
    } = &connector.operand.container
    else {
        panic!("an occurrence operand")
    };
    assert_eq!(named, &occurrence.id);
    let records = records(&result);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["kind"], "ground");
    let arena = &result.ir().native.namespace("solidedge").unwrap().arenas()["assembly_relations"];
    assert_eq!(
        joint.native_ref.as_deref(),
        Some(arena[0].id()),
        "the joint names its native record"
    );
    assert_eq!(count(&result, "assembly.ground-disagrees"), 0);
    assert_eq!(count(&result, "assembly.relation-unread"), 0);
    assert_eq!(count(&result, "assembly.relation-not-typed"), 0);
}

#[test]
fn a_ground_relation_of_an_occurrence_that_is_not_grounded_states_no_joint() {
    let mut free = occurrences();
    free[0].state = 4;
    let result = decode_with(&free, &[ground_first()], &PICKS);
    assert!(result.ir().model.assembly_joints.is_empty());
    assert_eq!(count(&result, "assembly.ground-disagrees"), 1);
    assert_eq!(records(&result).len(), 1);
}

#[test]
fn a_grounded_occurrence_without_a_ground_relation_adds_a_loss() {
    let result = decode(&[]);
    assert!(result.ir().model.assembly_joints.is_empty());
    assert_eq!(count(&result, "assembly.ground-disagrees"), 1);
    assert!(records(&result).is_empty());
}

#[test]
fn a_ground_relation_with_another_placement_is_not_transferred() {
    let relation = RelationSpec::Ground {
        id: 30,
        reference: FIRST,
        rotation: IDENTITY,
        translation: [0.5, 0.0, 0.0],
    };
    let result = decode(&[relation]);
    assert!(result.ir().model.assembly_joints.is_empty());
    assert_eq!(count(&result, "assembly.relation-unread"), 1);
    assert_eq!(count(&result, "assembly.ground-disagrees"), 0);
}

#[test]
fn a_planar_relation_states_its_operands_offset_and_sense() {
    let result = decode(&[ground_first(), planar(0x3002, 0.005)]);
    let records = records(&result);
    assert_eq!(records.len(), 2);
    let record = records.iter().find(|r| r["kind"] == "planar").unwrap();
    assert!((record["offset"].as_f64().unwrap() - 5.0).abs() <= EPS_VALUE);
    assert_eq!(record["opposed"], true);
    assert_eq!(record["flags"], 0x3002);
    assert_eq!(record["dimension"], 50);
    let operands = record["operands"].as_array().unwrap();
    assert_eq!(operands[0]["reference"], SECOND);
    assert_eq!(operands[0]["pick"], PICKS[0]);
    assert_eq!(operands[1]["reference"], FIRST);
    assert_eq!(operands[1]["pick"], PICKS[1]);
    let second = &result.ir().model.occurrences[1];
    assert_eq!(operands[0]["occurrence"], second.id.as_str());
    // The ground relation is the only typed joint.
    assert_eq!(result.ir().model.assembly_joints.len(), 1);
    assert_eq!(count(&result, "assembly.relation-not-typed"), 1);
}

#[test]
fn a_flush_planar_relation_is_not_opposed() {
    let result = decode(&[ground_first(), planar(0x3000, 0.0)]);
    let records = records(&result);
    let record = records.iter().find(|r| r["kind"] == "planar").unwrap();
    assert_eq!(record["opposed"], false);
    assert!(record["offset"].as_f64().unwrap().abs() <= EPS_VALUE);
}

#[test]
fn axial_and_tangent_relations_keep_their_flags() {
    let result = decode(&[
        ground_first(),
        RelationSpec::Axial {
            id: 32,
            operands: [SECOND, FIRST],
            picks: PICKS,
            flags: 0x8000_3000,
        },
        RelationSpec::Tangent {
            id: 33,
            operands: [SECOND, FIRST],
            picks: PICKS,
            flags: 0x3040,
            dimension: 51,
        },
    ]);
    let records = records(&result);
    let axial = records.iter().find(|r| r["kind"] == "axial").unwrap();
    assert_eq!(axial["flags"], 0x8000_3000_u32);
    assert!(axial.get("dimension").is_none());
    let tangent = records.iter().find(|r| r["kind"] == "tangent").unwrap();
    assert_eq!(tangent["flags"], 0x3040);
    assert_eq!(tangent["dimension"], 51);
    assert_eq!(count(&result, "assembly.relation-not-typed"), 1);
    assert_eq!(count(&result, "assembly.relation-unread"), 0);
}

#[test]
fn an_angular_relation_states_radians_and_its_operand_groups() {
    let result = decode(&[
        ground_first(),
        RelationSpec::Angular {
            id: 34,
            operands: [FIRST, SECOND],
            picks: PICKS,
            angle: std::f64::consts::FRAC_PI_6,
            flags: 0x3700,
            dimension: 52,
        },
    ]);
    let records = records(&result);
    let angular = records.iter().find(|r| r["kind"] == "angular").unwrap();
    assert!((angular["angle"].as_f64().unwrap() - std::f64::consts::FRAC_PI_6).abs() <= EPS_VALUE);
    assert_eq!(angular["flags"], 0x3700);
    assert_eq!(angular["dimension"], 52);
    let operands = angular["operands"].as_array().unwrap();
    assert_eq!(operands[0]["reference"], FIRST);
    assert_eq!(operands[1]["pick"], PICKS[1]);
}

#[test]
fn a_relation_whose_pick_is_not_a_pick_object_is_not_transferred() {
    let result = decode_with(
        &occurrences(),
        &[ground_first(), planar(0x3002, 0.0)],
        &[40],
    );
    assert_eq!(count(&result, "assembly.relation-unread"), 1);
    assert_eq!(records(&result).len(), 1);
}

#[test]
fn a_relation_that_names_no_occurrence_is_not_transferred() {
    let relation = RelationSpec::Planar {
        id: 31,
        operands: [99, FIRST],
        picks: PICKS,
        offset: 0.0,
        flags: 0x3002,
        dimension: 50,
    };
    let result = decode(&[ground_first(), relation]);
    assert_eq!(count(&result, "assembly.relation-unread"), 1);
    assert_eq!(records(&result).len(), 1);
}

#[test]
fn an_angular_relation_that_names_no_occurrence_is_not_transferred() {
    let spec = RelationSpec::Angular {
        id: 34,
        operands: [98, SECOND],
        picks: PICKS,
        angle: 0.5,
        flags: 0x3700,
        dimension: 52,
    };
    let result = decode(&[ground_first(), spec]);
    assert_eq!(count(&result, "assembly.relation-unread"), 1);
}

#[test]
fn a_relation_listed_twice_is_transferred_once() {
    let relations = [ground_first()];
    let bytes =
        assembly_document_with_relations(&occurrences(), &sites(), &[30, 30], &relations, &PICKS);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(count(&result, "assembly.relation-unread"), 1);
    assert_eq!(result.ir().model.assembly_joints.len(), 1);
}

#[test]
fn a_listed_object_that_is_not_a_relation_is_not_transferred() {
    let relations = [ground_first()];
    let bytes =
        assembly_document_with_relations(&occurrences(), &sites(), &[30, 40], &relations, &PICKS);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(count(&result, "assembly.relation-unread"), 1);
}

#[test]
fn a_document_without_a_relation_list_adds_nothing() {
    let bytes = crate::test_support::assembly_document(
        &[FIRST],
        &[OccurrenceSpec::at_identity(FIRST, 10)],
        &sites()[..1],
    );
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.assembly_joints.is_empty());
    assert_eq!(count(&result, "assembly.ground-disagrees"), 0);
    assert!(records(&result).is_empty());
}

/// The messages of the losses with `code`.
fn messages(result: &DecodeResult, code: &str) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == code)
        .map(|loss| loss.message.clone())
        .collect()
}

/// The translation column of a joint frame, in millimetres.
fn frame_translation(result: &DecodeResult) -> [f64; 3] {
    let [joint] = result.ir().model.assembly_joints.as_slice() else {
        panic!("one joint")
    };
    let JointOperands::Grounded { connector, .. } = joint.operands() else {
        panic!("a grounded joint")
    };
    let rows = connector.frame.affine_rows();
    [rows[0][3], rows[1][3], rows[2][3]]
}

fn decode_document(bytes: Vec<u8>) -> DecodeResult {
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

#[test]
fn a_state_word_other_than_2_or_4_states_no_ground_check() {
    // §9.6: only the state words 2 and 4 say whether a ground relation exists.
    let mut odd = occurrences();
    odd[0].state = 6;
    odd[1].state = 0;
    let result = decode_with(&odd, &[], &PICKS);
    assert_eq!(count(&result, "assembly.ground-disagrees"), 0);
    assert!(result.ir().model.assembly_joints.is_empty());
    let result = decode_with(&odd, &[ground_first()], &PICKS);
    assert_eq!(count(&result, "assembly.ground-disagrees"), 0);
    assert_eq!(result.ir().model.assembly_joints.len(), 1);
}

#[test]
fn an_unreadable_occurrence_placement_takes_the_frame_of_its_ground_relation() {
    let mut unreadable = occurrences();
    unreadable[0].unit = 2.0;
    // The stored translation is in metres and the frame is in millimetres.
    let relation = RelationSpec::Ground {
        id: 30,
        reference: FIRST,
        rotation: IDENTITY,
        translation: [0.5, 0.0, 0.0],
    };
    let result = decode_with(&unreadable, &[relation], &PICKS);
    let translation = frame_translation(&result);
    assert!((translation[0] - 500.0).abs() <= EPS_VALUE);
    assert!(translation[1].abs() <= EPS_VALUE && translation[2].abs() <= EPS_VALUE);
    assert_eq!(count(&result, "assembly.relation-unread"), 0);
    assert_eq!(count(&result, "assembly.ground-disagrees"), 0);
}

#[test]
fn an_absent_relation_list_leaves_a_grounded_occurrence_without_a_relation() {
    // §9.6: an occurrence has a ground relation if and only if its state word is 2.
    let result = decode_document(assembly_document_with_list(
        &occurrences(),
        &sites(),
        None,
        &[],
        &PICKS,
    ));
    assert_eq!(count(&result, "assembly.ground-disagrees"), 1);
    assert_eq!(count(&result, "assembly.relation-unread"), 0);
    assert!(result.ir().model.assembly_joints.is_empty());
}

#[test]
fn an_absent_relation_list_with_no_grounded_occurrence_adds_nothing() {
    let mut free = occurrences();
    free[0].state = 4;
    let result = decode_document(assembly_document_with_list(
        &free,
        &sites(),
        None,
        &[],
        &PICKS,
    ));
    assert_eq!(count(&result, "assembly.ground-disagrees"), 0);
    assert_eq!(count(&result, "assembly.relation-unread"), 0);
}

#[test]
fn a_relation_list_without_a_readable_count_adds_one_loss() {
    // The count is the `u32` at object offset 16; the object ends at offset 18.
    let mut list = relation_list_body(&[30]);
    list.truncate(10);
    let result = decode_document(assembly_document_with_list(
        &occurrences(),
        &sites(),
        Some(list),
        &[ground_first()],
        &PICKS,
    ));
    assert_eq!(count(&result, "assembly.relation-unread"), 1);
    assert!(records(&result).is_empty());
    // The ground set is empty, so the grounded occurrence has no relation.
    assert_eq!(count(&result, "assembly.ground-disagrees"), 1);
}

#[test]
fn a_ground_relation_longer_than_its_fields_is_read() {
    // §9.6 states the fields through offset 116; the object can continue.
    let relation = RelationSpec::Patched {
        spec: Box::new(ground_first()),
        at: 127,
        bytes: vec![0xAB; 9],
    };
    let result = decode(&[relation]);
    assert_eq!(result.ir().model.assembly_joints.len(), 1);
    assert_eq!(count(&result, "assembly.relation-unread"), 0);
    assert_eq!(records(&result).len(), 1);
}

#[test]
fn an_unexpected_constant_word_adds_a_loss_and_keeps_the_relation() {
    // §9.7: the planar word at offset 40 is `1`; the tangent word at offset 52 is `4`.
    let planar_spec = RelationSpec::Patched {
        spec: Box::new(planar(0x3002, 0.0)),
        at: 40,
        bytes: 2_u32.to_le_bytes().to_vec(),
    };
    let tangent_spec = RelationSpec::Patched {
        spec: Box::new(RelationSpec::Tangent {
            id: 33,
            operands: [SECOND, FIRST],
            picks: PICKS,
            flags: 0x3040,
            dimension: 51,
        }),
        at: 52,
        bytes: 5_u32.to_le_bytes().to_vec(),
    };
    let result = decode(&[ground_first(), planar_spec, tangent_spec]);
    let records = records(&result);
    assert!(records.iter().any(|r| r["kind"] == "planar"));
    assert!(records.iter().any(|r| r["kind"] == "tangent"));
    let notes = messages(&result, "assembly.relation-unread");
    assert_eq!(notes.len(), 2);
    assert!(notes
        .iter()
        .any(|m| m.contains("offset 40") && m.contains("0x2")));
    assert!(notes
        .iter()
        .any(|m| m.contains("offset 52") && m.contains("0x5")));
}

#[test]
fn a_count_above_the_entries_present_reads_the_entries_present() {
    // The count is the `u32` at object offset 16, which is offset 8 of the body.
    let mut list = relation_list_body(&[30]);
    list[8..12].copy_from_slice(&3_u32.to_le_bytes());
    let result = decode_document(assembly_document_with_list(
        &occurrences(),
        &sites(),
        Some(list),
        &[ground_first()],
        &PICKS,
    ));
    assert_eq!(count(&result, "assembly.relation-unread"), 1);
    assert_eq!(result.ir().model.assembly_joints.len(), 1);
    assert_eq!(records(&result).len(), 1);
}

#[test]
fn a_bad_entry_mark_skips_that_entry_and_continues() {
    // Three entries of five bytes follow the count and one bitmap byte, that is
    // from object offset 21. The mark of entry 1 is at offset 21 + 5 + 4.
    let axial = RelationSpec::Axial {
        id: 32,
        operands: [SECOND, FIRST],
        picks: PICKS,
        flags: 0x8000_3000,
    };
    let relations = [ground_first(), planar(0x3002, 0.0), axial];
    let mut list = relation_list_body(&[30, 31, 32]);
    list[30 - 8] = 1;
    let result = decode_document(assembly_document_with_list(
        &occurrences(),
        &sites(),
        Some(list),
        &relations,
        &PICKS,
    ));
    let notes = messages(&result, "assembly.relation-unread");
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("entry 1"));
    let records = records(&result);
    assert_eq!(records.len(), 2);
    assert!(records.iter().any(|r| r["kind"] == "ground"));
    assert!(records.iter().any(|r| r["kind"] == "axial"));
    assert_eq!(result.ir().model.assembly_joints.len(), 1);
}

#[test]
fn two_ground_relations_of_one_occurrence_state_one_joint() {
    let again = RelationSpec::Ground {
        id: 35,
        reference: FIRST,
        rotation: IDENTITY,
        translation: [0.0; 3],
    };
    let result = decode(&[ground_first(), again]);
    assert_eq!(result.ir().model.assembly_joints.len(), 1);
    assert_eq!(count(&result, "assembly.relation-unread"), 1);
    assert_eq!(count(&result, "assembly.ground-disagrees"), 0);
}
