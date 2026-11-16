// SPDX-License-Identifier: Apache-2.0
//! Resolve the tool face operand of a Split body feature in its input state.

use super::body_index::CompleteBodyIndex;
use super::output_properties::{classify_face_plane, planes_are_equivalent};
use super::{selection, stable_ref};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalTopology, AsmHistoricalTopologyDelta, AsmHistory,
};

pub(super) fn resolve_split_tool_face(
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &AsmHistoricalTopology,
) -> Option<i64> {
    if operand.group_record_index().is_some()
        || operand.group_member_ordinal().is_some()
        || operand.scope_reference_ordinal != 1
        || operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face
        || operand.recipe_program != [0, -1]
    {
        return None;
    }
    let [reference] = operand.recipe_references.as_slice() else {
        return None;
    };
    let candidates = selection::faces_in_topology(&reference.candidate_faces, topology);
    let [face] = candidates.as_slice() else {
        return None;
    };
    stable_ref(face.as_str())
}

pub(super) fn split_tool_coplanar_slots(
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &AsmHistoricalTopology,
    history: &AsmHistory,
    state: &AsmDeltaState,
) -> Vec<i64> {
    history
        .topology(state)
        .zip(state.transition.as_ref())
        .and_then(|(result, transition)| {
            resolve_split_tool_coplanar_faces(operand, topology, result, &transition.topology)
        })
        .unwrap_or_default()
}

/// Resolve a Split tool face operand whose persistent reference names several
/// preceding faces. Every reference of the recipe names the same preceding face
/// set, every face of the set lies in one plane, the Split inserts a body, and
/// the result faces that the Split inserts or updates include at least one
/// inserted face and two faces in total in that plane.
pub(crate) fn resolve_split_tool_coplanar_faces(
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &AsmHistoricalTopology,
    result: &AsmHistoricalTopology,
    delta: &AsmHistoricalTopologyDelta,
) -> Option<Vec<i64>> {
    if operand.group_record_index().is_some()
        || operand.group_member_ordinal().is_some()
        || operand.scope_reference_ordinal != 1
        || operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face
        || operand.recipe_program != [0, -1]
        || delta.bodies.inserted.is_empty()
    {
        return None;
    }
    let (first, rest) = operand.recipe_references.split_first()?;
    let mut named = first.candidate_faces.clone();
    named.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    named.dedup();
    for reference in rest {
        let mut other = reference.candidate_faces.clone();
        other.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        other.dedup();
        if other != named {
            return None;
        }
    }
    let mut slots = Vec::new();
    for face in selection::faces_in_topology(&named, topology) {
        slots.push(stable_ref(face.as_str())?);
    }
    if slots.len() < 2 || slots.len() != named.len() {
        return None;
    }
    slots.sort_unstable();
    slots.dedup();
    let mut planes = slots
        .iter()
        .map(|&slot| classify_face_plane(topology, slot));
    let (origin, normal) = planes.next()??;
    for plane in planes {
        let (other_origin, other_normal) = plane?;
        if !planes_are_equivalent(origin, normal, other_origin, other_normal) {
            return None;
        }
    }
    result_meets_on_plane(origin, normal, result, delta).then_some(slots)
}

/// Whether the result faces that the Split inserts or updates include at least
/// one inserted face and two faces in total in the given plane.
fn result_meets_on_plane(
    origin: cadmpeg_ir::math::Point3,
    normal: cadmpeg_ir::math::Vector3,
    result: &AsmHistoricalTopology,
    delta: &AsmHistoricalTopologyDelta,
) -> bool {
    let on_plane = |face: i64| {
        classify_face_plane(result, face).is_some_and(|(other_origin, other_normal)| {
            planes_are_equivalent(origin, normal, other_origin, other_normal)
        })
    };
    let inserted = delta
        .faces
        .inserted
        .iter()
        .filter(|&&face| on_plane(face))
        .count();
    let updated = delta
        .faces
        .updated
        .iter()
        .filter(|&&face| on_plane(face))
        .count();
    inserted >= 1 && inserted + updated >= 2
}

/// Faces of the bodies that a Split transition deletes, for a tool group whose
/// member selects no decoded operand. The Split inserts a body, every deleted
/// body is complete in the preceding state, and the faces of the deleted
/// bodies lie in one plane that holds the result faces of the Split.
pub(crate) fn consumed_tool_body_faces(
    topology: &AsmHistoricalTopology,
    result: &AsmHistoricalTopology,
    delta: &AsmHistoricalTopologyDelta,
) -> Option<Vec<i64>> {
    if delta.bodies.inserted.is_empty() || delta.bodies.deleted.is_empty() {
        return None;
    }
    let index = CompleteBodyIndex::new(topology);
    let mut faces = Vec::new();
    for &body in &delta.bodies.deleted {
        faces.extend(index.faces(body)?);
    }
    faces.sort_unstable();
    faces.dedup();
    let mut planes = faces
        .iter()
        .map(|&face| classify_face_plane(topology, face));
    let (origin, normal) = planes.next()??;
    for plane in planes {
        let (other_origin, other_normal) = plane?;
        if !planes_are_equivalent(origin, normal, other_origin, other_normal) {
            return None;
        }
    }
    result_meets_on_plane(origin, normal, result, delta).then_some(faces)
}

/// Whether `native` names the role-`0x21` tool group of the Split scope, with
/// one member that decodes to no entity-selection operand.
pub(super) fn undecoded_tool_group(
    scope: &crate::records::feature::scope::DesignParameterScope,
    native: &str,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    entity_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Option<()> {
    let mut named = groups.iter().filter(|group| group.id == native);
    let (Some(group), None) = (named.next(), named.next()) else {
        return None;
    };
    let [member] = group.members() else {
        return None;
    };
    (group.scope_record_index == scope.record_index
        && group.role()
            == crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X21
        && !entity_operands.iter().any(|operand| {
            operand.scope_record_index == scope.record_index
                && operand.record_index() == member.value
        }))
    .then_some(())
}
