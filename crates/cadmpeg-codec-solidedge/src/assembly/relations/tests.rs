// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::products::{JointOperands, OperandContainer};
use serde_json::Value;

use crate::test_support::{
    assembly_document_with_relations, OccurrenceSpec, RelationSpec, SiteSpec,
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
