// SPDX-License-Identifier: Apache-2.0
//! Resolve the tool face operand of a Split body feature in its input state.

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
/// set, every face of the set lies in one plane, and at least two planar faces
/// inserted by the Split lie in that plane while the Split inserts a body.
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
    let coplanar_inserted = delta
        .faces
        .inserted
        .iter()
        .filter_map(|&face| classify_face_plane(result, face))
        .filter(|&(other_origin, other_normal)| {
            planes_are_equivalent(origin, normal, other_origin, other_normal)
        })
        .count();
    (coplanar_inserted >= 2).then_some(slots)
}
