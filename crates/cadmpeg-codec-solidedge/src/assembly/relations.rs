// SPDX-License-Identifier: Apache-2.0
//! Assembly relations (`docs/formats/solidedge.md` §9.5 to §9.8): the ground,
//! planar, axial, tangent and angular relations between occurrences.
//!
//! The `Constraints Collection Object` lists the relation objects. Each ground
//! relation becomes a grounded `AssemblyJoint` whose connector frame is the
//! placement of the grounded occurrence. A ground relation and the state word
//! of the occurrence (§9.2) state the same fact: where they disagree the
//! occurrence states no joint and the document adds
//! `assembly.ground-disagrees`. A document without a relation list holds no
//! ground relation.
//!
//! Every relation becomes a record of the native arena `assembly_relations`
//! with its operand occurrences, the identifier of the pick object of each
//! operand, its offset or angle, its flag bits and its dimension object. The
//! placement relations have no typed joint: a typed joint states a frame for
//! each connector, and the pick object layout is not decoded. A document that
//! holds such a relation adds `assembly.relation-not-typed` once. A relation
//! that does not read, a relation list entry that does not read and a constant
//! word with another value add `assembly.relation-unread`.
//!
//! Stored lengths are metres and the native record states millimetres.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{AssemblyJoint, JointConnector, JointId, JointOperand};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use serde::Serialize;

use crate::extents::MM_PER_M;
use crate::layout::{
    assembly_angular, assembly_angular_operand, assembly_axial, assembly_ground, assembly_planar,
    assembly_tangent, constraint_entry, constraints_collection,
};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, Object, TypeTable};

/// The class of the object that lists the relations.
const COLLECTION_CLASS: &str = "Constraints Collection Object";
/// The class of a pick object.
const PICK_CLASS: &str = "CLSID_JMembassy";
const GROUND_CLASS: &str = "CLSID_JAsmGroundConstraint";
const PLANAR_CLASS: &str = "CLSID_JAsmPlanarConstraint";
const AXIAL_CLASS: &str = "CLSID_JAsmAxialConstraint";
const TANGENT_CLASS: &str = "CLSID_JAsmTangentConstraint";
const ANGULAR_CLASS: &str = "CLSID_JAsmAngularConstraint";
/// The `u32` words of a relation object that the specification states as `1`.
const ONE: u32 = 1;
/// The `u32` word at offset 52 of a tangent relation (§9.7).
const TANGENT_TAIL: u32 = 4;
/// The byte of an angular operand group between the pick and the repeated
/// operand identifier (§9.8).
const GROUP_TAG: u8 = 2;
/// The byte of a relation list entry after the object identifier (§9.5).
const ENTRY_MARK: u8 = 0;
/// Bit of a planar relation's flags that marks opposed plane normals (§9.7).
const OPPOSED_FLAG: u32 = 0x2;
/// Largest deviation of a stored `1` from `1`.
const EPS_UNIT: f64 = 1e-9;
/// Largest deviation of a ground relation's placement from the occurrence's.
const EPS_SAME_PLACEMENT: f64 = 1e-9;

/// The state of an occurrence the relations name.
pub(super) struct Placed {
    pub(super) occurrence: OccurrenceId,
    pub(super) transform: Option<Transform>,
    /// Whether the state word of §9.2 says the occurrence is grounded, or
    /// `None` where it states neither value.
    pub(super) grounded: Option<bool>,
}

/// The occurrences of a document by the object identifier of their
/// `CLSID_JAsmReference` object.
pub(super) type Placements = BTreeMap<u32, Placed>;

#[derive(Serialize)]
struct OperandRecord {
    occurrence: OccurrenceId,
    reference: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pick: Option<u32>,
}

/// One relation as the native arena states it.
#[derive(Serialize)]
struct RelationRecord {
    id: String,
    kind: &'static str,
    object: u32,
    operands: Vec<OperandRecord>,
    /// The plane offset in millimetres.
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<f64>,
    /// The angle in radians.
    #[serde(skip_serializing_if = "Option::is_none")]
    angle: Option<f64>,
    /// Whether the plane normals oppose.
    #[serde(skip_serializing_if = "Option::is_none")]
    opposed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    flags: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimension: Option<u32>,
}

/// A relation read from its object.
struct Relation {
    record: RelationRecord,
    /// The occurrence a ground relation grounds.
    grounded: Option<u32>,
    /// The placement a ground relation states, where it is a rigid placement.
    frame: Option<Transform>,
    /// Whether a ground relation states another placement than its occurrence.
    mismatch: bool,
    /// Constant words that hold another value than the specification states.
    notes: Vec<String>,
}

fn relation_id(object: u32) -> String {
    format!("solidedge:assembly:relation#{object}")
}

fn joint_id(object: u32) -> JointId {
    JointId::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "model", "joint"),
        object,
    )
}

fn word(bytes: &[u8], at: usize) -> Result<u32, String> {
    View::u32_le_at(bytes, at).ok_or_else(|| format!("the word at offset {at} is unreadable"))
}

fn real(bytes: &[u8], at: usize) -> Result<f64, String> {
    View::f64_le_at(bytes, at)
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("the real at offset {at} is not finite"))
}

/// Check a constant word. Another value is noted and the relation is kept.
fn expect_word(bytes: &[u8], at: usize, want: u32, notes: &mut Vec<String>) -> Result<(), String> {
    let found = word(bytes, at)?;
    if found != want {
        notes.push(format!(
            "the constant word at offset {at} is {found:#x}, not {want:#x}"
        ));
    }
    Ok(())
}

/// The identifiers the relation list names. An entry that does not read is
/// skipped and noted; the entries have a fixed size.
fn listing(cluster: &Cluster<'_>, types: &TypeTable, losses: &mut Vec<LossNote>) -> Vec<u32> {
    let Some(object) = cluster
        .objects()
        .find(|object| types.class_name(object.form, object.flags) == Some(COLLECTION_CLASS))
    else {
        return Vec::new();
    };
    let bytes = object.bytes;
    let unread = |reason: String| {
        SolidEdgeLossCode::AssemblyRelationUnread.note(format!(
            "the relation list {reason}; the relations are not transferred"
        ))
    };
    let Some(count) = View::u32_le_at(bytes, constraints_collection::COUNT) else {
        losses.push(unread("states no readable count".into()));
        return Vec::new();
    };
    let Ok(declared) = usize::try_from(count) else {
        losses.push(unread(format!(
            "states the count {count}, which is too large"
        )));
        return Vec::new();
    };
    let first = constraints_collection::COUNT
        .checked_add(4)
        .and_then(|at| at.checked_add(declared.div_ceil(8)));
    let room = first
        .and_then(|first| bytes.len().checked_sub(first))
        .map_or(0, |rest| rest / constraint_entry::LEN);
    if declared > room {
        losses.push(SolidEdgeLossCode::AssemblyRelationUnread.note(format!(
            "the relation list states {declared} relation(s) and holds {room} entries; \
             the others are not transferred"
        )));
    }
    let mut ids = Vec::new();
    let Some(first) = first else {
        return ids;
    };
    for index in 0..declared.min(room) {
        let Some(at) = index
            .checked_mul(constraint_entry::LEN)
            .and_then(|offset| first.checked_add(offset))
        else {
            break;
        };
        let id = at
            .checked_add(constraint_entry::OBJECT)
            .and_then(|object| View::u32_le_at(bytes, object));
        let mark = at
            .checked_add(constraint_entry::MARK)
            .and_then(|mark| bytes.get(mark).copied());
        match (id, mark) {
            (Some(id), Some(ENTRY_MARK)) => ids.push(id),
            (Some(_), Some(mark)) => {
                losses.push(SolidEdgeLossCode::AssemblyRelationUnread.note(format!(
                    "entry {index} of the relation list has the mark {mark:#x}, not \
                     {ENTRY_MARK:#x}; it is not transferred"
                )));
            }
            _ => losses.push(SolidEdgeLossCode::AssemblyRelationUnread.note(format!(
                "entry {index} of the relation list is unreadable; it is not transferred"
            ))),
        }
    }
    ids
}

/// The occurrence an operand word names.
fn operand(placements: &Placements, id: u32) -> Result<&Placed, String> {
    placements
        .get(&id)
        .ok_or_else(|| format!("the object {id:#x} is not an occurrence of the document"))
}

/// The pick object an operand word names.
fn pick(cluster: &Cluster<'_>, types: &TypeTable, id: u32) -> Result<u32, String> {
    let object = cluster
        .object(id)
        .ok_or_else(|| format!("the pick object {id:#x} is not in the cluster"))?;
    match types.class_name(object.form, object.flags) {
        Some(PICK_CLASS) => Ok(id),
        _ => Err(format!("the object {id:#x} is not a {PICK_CLASS}")),
    }
}

fn operand_record(
    placements: &Placements,
    reference: u32,
    pick: Option<u32>,
) -> Result<OperandRecord, String> {
    Ok(OperandRecord {
        occurrence: operand(placements, reference)?.occurrence.clone(),
        reference,
        pick,
    })
}

fn check_len(bytes: &[u8], len: usize, kind: &str) -> Result<(), String> {
    if bytes.len() == len {
        Ok(())
    } else {
        Err(format!(
            "the {kind} object has {} bytes, not {len}",
            bytes.len()
        ))
    }
}

fn ground(placements: &Placements, object: &Object<'_>) -> Result<Relation, String> {
    let bytes = object.bytes;
    if bytes.len() < assembly_ground::LEN {
        return Err(format!(
            "the ground relation object has {} bytes, fewer than the {} its fields need",
            bytes.len(),
            assembly_ground::LEN
        ));
    }
    let reference = word(bytes, assembly_ground::REFERENCE)?;
    let placed = operand(placements, reference)?;
    let mut rotation = [0.0_f64; 9];
    for (index, value) in rotation.iter_mut().enumerate() {
        let at = index
            .checked_mul(8)
            .and_then(|offset| offset.checked_add(assembly_ground::ROTATION))
            .ok_or("the rotation offset overflows")?;
        *value = real(bytes, at)?;
    }
    let mut translation = [0.0_f64; 3];
    for (index, value) in translation.iter_mut().enumerate() {
        let at = index
            .checked_mul(8)
            .and_then(|offset| offset.checked_add(assembly_ground::TRANSLATION))
            .ok_or("the translation offset overflows")?;
        *value = real(bytes, at)?;
    }
    if (real(bytes, assembly_ground::UNIT)? - 1.0).abs() > EPS_UNIT {
        return Err("the word after the translation is not 1".into());
    }
    let r = [
        [rotation[0], rotation[1], rotation[2]],
        [rotation[3], rotation[4], rotation[5]],
        [rotation[6], rotation[7], rotation[8]],
    ];
    // CADIR rows are the columns of the stored rotation (§9.2).
    let frame = if super::proper_rotation(&r) {
        Transform::affine(std::array::from_fn(|j| {
            [r[0][j], r[1][j], r[2][j], translation[j] * MM_PER_M]
        }))
    } else {
        None
    };
    let mismatch = placed.transform.is_some_and(|transform| {
        let rows = transform.affine_rows();
        !(0..3).all(|j| {
            (0..3).all(|i| (rows[j][i] - rotation[i * 3 + j]).abs() <= EPS_SAME_PLACEMENT)
                && (rows[j][3] - translation[j] * MM_PER_M).abs() <= EPS_SAME_PLACEMENT
        })
    });
    Ok(Relation {
        record: RelationRecord {
            id: relation_id(object.id),
            kind: "ground",
            object: object.id,
            operands: vec![operand_record(placements, reference, None)?],
            offset: None,
            angle: None,
            opposed: None,
            flags: None,
            dimension: None,
        },
        grounded: Some(reference),
        frame,
        mismatch,
        notes: Vec::new(),
    })
}

/// The two operands of a planar, axial or tangent relation.
fn pair(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    placements: &Placements,
    bytes: &[u8],
) -> Result<Vec<OperandRecord>, String> {
    let first = word(bytes, assembly_planar::OPERAND_FIRST)?;
    let second = word(bytes, assembly_planar::OPERAND_SECOND)?;
    let first_pick = pick(cluster, types, word(bytes, assembly_planar::PICK_FIRST)?)?;
    let second_pick = pick(cluster, types, word(bytes, assembly_planar::PICK_SECOND)?)?;
    Ok(vec![
        operand_record(placements, first, Some(first_pick))?,
        operand_record(placements, second, Some(second_pick))?,
    ])
}

fn placement(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    placements: &Placements,
    object: &Object<'_>,
    class: &str,
) -> Result<Relation, String> {
    let bytes = object.bytes;
    let mut notes = Vec::new();
    let mut record = RelationRecord {
        id: relation_id(object.id),
        kind: "",
        object: object.id,
        operands: Vec::new(),
        offset: None,
        angle: None,
        opposed: None,
        flags: None,
        dimension: None,
    };
    match class {
        PLANAR_CLASS => {
            check_len(bytes, assembly_planar::LEN, "planar relation")?;
            record.operands = pair(cluster, types, placements, bytes)?;
            record.kind = "planar";
            record.offset = Some(real(bytes, assembly_planar::OFFSET)? * MM_PER_M);
            expect_word(bytes, assembly_planar::ONE, ONE, &mut notes)?;
            let flags = word(bytes, assembly_planar::FLAGS)?;
            record.opposed = Some(flags & OPPOSED_FLAG != 0);
            record.flags = Some(flags);
            record.dimension = Some(word(bytes, assembly_planar::DIMENSION)?);
        }
        AXIAL_CLASS => {
            check_len(bytes, assembly_axial::LEN, "axial relation")?;
            record.operands = pair(cluster, types, placements, bytes)?;
            record.kind = "axial";
            expect_word(bytes, assembly_axial::ONE, ONE, &mut notes)?;
            record.flags = Some(word(bytes, assembly_axial::FLAGS)?);
            expect_word(bytes, assembly_axial::TAIL, ONE, &mut notes)?;
        }
        _ => {
            check_len(bytes, assembly_tangent::LEN, "tangent relation")?;
            record.operands = pair(cluster, types, placements, bytes)?;
            record.kind = "tangent";
            expect_word(bytes, assembly_tangent::ONE, ONE, &mut notes)?;
            record.flags = Some(word(bytes, assembly_tangent::FLAGS)?);
            record.dimension = Some(word(bytes, assembly_tangent::DIMENSION)?);
            expect_word(bytes, assembly_tangent::TAIL, TANGENT_TAIL, &mut notes)?;
        }
    }
    Ok(Relation {
        record,
        grounded: None,
        frame: None,
        mismatch: false,
        notes,
    })
}

/// One operand group of an angular relation at `at`.
fn angular_group(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    placements: &Placements,
    bytes: &[u8],
    at: usize,
) -> Result<OperandRecord, String> {
    let reference = word(bytes, at + assembly_angular_operand::REFERENCE)?;
    let again = word(bytes, at + assembly_angular_operand::REFERENCE_AGAIN)?;
    if again != reference {
        return Err("an operand group names two different occurrences".into());
    }
    if bytes.get(at + assembly_angular_operand::TAG) != Some(&GROUP_TAG) {
        return Err("an operand group lacks its tag byte".into());
    }
    let picked = pick(
        cluster,
        types,
        word(bytes, at + assembly_angular_operand::PICK)?,
    )?;
    operand_record(placements, reference, Some(picked))
}

fn angular(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    placements: &Placements,
    object: &Object<'_>,
) -> Result<Relation, String> {
    let bytes = object.bytes;
    let mut notes = Vec::new();
    check_len(bytes, assembly_angular::LEN, "angular relation")?;
    let first = angular_group(
        cluster,
        types,
        placements,
        bytes,
        assembly_angular::FIRST_GROUP,
    )?;
    let second = angular_group(
        cluster,
        types,
        placements,
        bytes,
        assembly_angular::SECOND_GROUP,
    )?;
    expect_word(bytes, assembly_angular::ONE, ONE, &mut notes)?;
    Ok(Relation {
        record: RelationRecord {
            id: relation_id(object.id),
            kind: "angular",
            object: object.id,
            operands: vec![first, second],
            offset: None,
            angle: Some(real(bytes, assembly_angular::ANGLE)?),
            opposed: None,
            flags: Some(word(bytes, assembly_angular::FLAGS)?),
            dimension: Some(word(bytes, assembly_angular::DIMENSION)?),
        },
        grounded: None,
        frame: None,
        mismatch: false,
        notes,
    })
}

fn read_relation(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    placements: &Placements,
    id: u32,
) -> Result<Relation, String> {
    let object = cluster
        .object(id)
        .ok_or_else(|| "the object is not in the cluster".to_owned())?;
    match types.class_name(object.form, object.flags) {
        Some(GROUND_CLASS) => ground(placements, &object),
        Some(class @ (PLANAR_CLASS | AXIAL_CLASS | TANGENT_CLASS)) => {
            placement(cluster, types, placements, &object, class)
        }
        Some(ANGULAR_CLASS) => angular(cluster, types, placements, &object),
        Some(other) => Err(format!("the object is of class {other}")),
        None => Err("the object has no class".into()),
    }
}

/// State the relations of an assembly: a grounded joint per ground relation
/// and a native record per relation.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    cluster: &Cluster<'_>,
    types: &TypeTable,
    placements: &Placements,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let ids = listing(cluster, types, losses);
    let mut seen = BTreeSet::new();
    let mut relations = Vec::new();
    for id in ids {
        if !seen.insert(id) {
            losses.push(SolidEdgeLossCode::AssemblyRelationUnread.note(format!(
                "relation object {id:#x} is listed twice; the second entry is not transferred"
            )));
            continue;
        }
        match read_relation(cluster, types, placements, id) {
            Ok(relation) => {
                for note in &relation.notes {
                    losses.push(SolidEdgeLossCode::AssemblyRelationUnread.note(format!(
                        "relation object {id:#x}: {note}; the relation is kept"
                    )));
                }
                relations.push(relation);
            }
            Err(reason) => losses.push(SolidEdgeLossCode::AssemblyRelationUnread.note(format!(
                "relation object {id:#x} does not read: {reason}; it is not transferred"
            ))),
        }
    }
    let grounds: BTreeSet<u32> = relations
        .iter()
        .filter_map(|relation| relation.grounded)
        .collect();
    let mut disagree = BTreeSet::new();
    for (reference, placed) in placements {
        let listed = grounds.contains(reference);
        if placed.grounded.is_some_and(|grounded| grounded != listed) {
            disagree.insert(*reference);
            losses.push(SolidEdgeLossCode::AssemblyGroundDisagrees.note(format!(
                "occurrence object {reference:#x} {} ground relation and its state word says it \
                 is {}grounded; it states no grounded joint",
                if listed { "has a" } else { "has no" },
                if listed { "not " } else { "" },
            )));
        }
    }
    let mut jointed = BTreeSet::new();
    let mut untyped = 0_usize;
    for relation in &relations {
        ctx.charge_entities(1, "project Solid Edge assembly relation")?;
        let Some(reference) = relation.grounded else {
            untyped += 1;
            continue;
        };
        let Some(placed) = placements.get(&reference) else {
            continue;
        };
        if disagree.contains(&reference) {
            continue;
        }
        if !jointed.insert(reference) {
            losses.push(SolidEdgeLossCode::AssemblyRelationUnread.note(format!(
                "relation object {:#x} grounds the occurrence object {reference:#x} again; it \
                 states no second grounded joint",
                relation.record.object
            )));
            continue;
        }
        if relation.mismatch {
            losses.push(SolidEdgeLossCode::AssemblyRelationUnread.note(format!(
                "the placement of relation object {:#x} differs from the placement of the \
                 occurrence object {reference:#x}; it states no grounded joint",
                relation.record.object
            )));
            continue;
        }
        let Some(frame) = placed.transform.or(relation.frame) else {
            losses.push(SolidEdgeLossCode::AssemblyRelationUnread.note(format!(
                "neither the occurrence object {reference:#x} nor its ground relation states a \
                 placement; it states no grounded joint"
            )));
            continue;
        };
        let mut joint = AssemblyJoint::grounded(
            joint_id(relation.record.object),
            JointConnector {
                operand: JointOperand::occurrence(
                    placed.occurrence.clone(),
                    reference.to_string(),
                    Vec::new(),
                ),
                frame,
                detached: false,
            },
            None,
        );
        joint.native_ref = Some(relation.record.id.clone());
        ir.model.assembly_joints.push(joint);
    }
    if untyped > 0 {
        losses.push(SolidEdgeLossCode::AssemblyRelationNotTyped.note(format!(
            "{untyped} placement relation(s) are kept as native records only: a typed joint \
             needs a frame for each connector and the pick object layout is not decoded"
        )));
    }
    if relations.is_empty() {
        return Ok(());
    }
    let namespace = ir.native.namespace_mut(cadmpeg_ir::codec_id!("solidedge"));
    namespace.set_arena_from(
        ctx,
        "assembly_relations",
        relations.into_iter().map(|relation| relation.record),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
