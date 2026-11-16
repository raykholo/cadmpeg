// SPDX-License-Identifier: Apache-2.0
use crate::history::resolve_split_tool_coplanar_faces;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalPlane, AsmHistoricalTopology,
    AsmHistoricalTopologyDelta,
};
use crate::records::dimensions::DesignRecipeReference;
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::topology::face::{DesignFaceOperand, DesignFaceOperandDraft};
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::math::{Point3, Vector3};

fn face(slot: i64) -> FaceId {
    FaceId::mint(format!("f3d:brep:entity#{slot}")).expect("identity grammar")
}

fn planar(faces: &[(i64, f64, Vector3)]) -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        faces: faces.iter().map(|&(slot, ..)| slot).collect(),
        face_surfaces: faces
            .iter()
            .map(|&(slot, ..)| AsmHistoricalCarrierBinding {
                entity: slot,
                carrier: slot + 1000,
            })
            .collect(),
        surface_planes: faces
            .iter()
            .map(|&(slot, z, normal)| AsmHistoricalPlane {
                surface: slot + 1000,
                origin: Point3::new(0.0, 0.0, z),
                normal,
            })
            .collect(),
        ..AsmHistoricalTopology::default()
    }
}

fn tool(references: &[&[i64]]) -> DesignFaceOperand {
    let reference = |candidates: &[i64]| DesignRecipeReference {
        selector: 1,
        selector_offset: 1_411,
        token: "1".into(),
        token_integer: 0,
        token_offset: 1_415,
        design_reference: 540,
        design_reference_offset: 1_420,
        candidate_faces: candidates.iter().copied().map(face).collect(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
        operand_offset: None,
    };
    DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: "f3d:Design/BulkStream.dat:design-face-operand#200".into(),
        scope_record_index: 42,
        scope_reference_ordinal: 1,
        group: None,
        record_index: 200,
        byte_offset: 1_200,
        class_tag: "490".to_owned().try_into().unwrap(),
        paired_byte_offset: 1_300,
        paired_class_tag: "267".to_owned().try_into().unwrap(),
        recipe_record_index: 203,
        recipe_record_byte_offset: 1_400,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#203".into(),
        recipe_prefix_offset: 1_411,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: references.iter().map(|faces| reference(faces)).collect(),
        recipe_kind: ConstructionRecipeKind::Face,
        recipe_program_offset: 1_430,
        recipe_program: vec![0, -1],
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 204,
        next_byte_offset: 1_500,
    })
    .unwrap()
}

fn delta(inserted_faces: &[i64], inserted_bodies: &[i64]) -> AsmHistoricalTopologyDelta {
    let mut delta = AsmHistoricalTopologyDelta::default();
    delta.faces.inserted = inserted_faces.to_vec();
    delta.bodies.inserted = inserted_bodies.to_vec();
    delta
}

#[test]
fn split_tool_faces_on_the_plane_where_the_result_bodies_meet_resolve() {
    let up = Vector3::new(0.0, 0.0, 1.0);
    let down = Vector3::new(0.0, 0.0, -1.0);
    let before = planar(&[(1, 5.0, up), (2, 5.0, up), (3, 5.0, up)]);
    let result = planar(&[(1, 5.0, up), (20, 5.0, up), (21, 5.0, down)]);
    let operand = tool(&[&[1, 2], &[1, 2]]);
    let resolved =
        resolve_split_tool_coplanar_faces(&operand, &before, &result, &delta(&[20, 21], &[9]));
    assert_eq!(resolved, Some(vec![1, 2]));
}

#[test]
fn split_tool_faces_stay_unresolved_without_the_meeting_plane() {
    let up = Vector3::new(0.0, 0.0, 1.0);
    let down = Vector3::new(0.0, 0.0, -1.0);
    let before = planar(&[(1, 5.0, up), (2, 5.0, up)]);
    let result = planar(&[(20, 9.0, up), (21, 9.0, down)]);
    let operand = tool(&[&[1, 2]]);
    let split = |tool: &DesignFaceOperand, before: &AsmHistoricalTopology, delta| {
        resolve_split_tool_coplanar_faces(tool, before, &result, &delta)
    };
    // Result faces lie in another plane.
    assert_eq!(split(&operand, &before, delta(&[20, 21], &[9])), None);
    let meeting = planar(&[(20, 5.0, up), (21, 5.0, down)]);
    let inserted = delta(&[20, 21], &[9]);
    let on_plane = |tool: &DesignFaceOperand, before: &AsmHistoricalTopology, delta| {
        resolve_split_tool_coplanar_faces(tool, before, &meeting, &delta)
    };
    assert_eq!(
        on_plane(&operand, &before, inserted.clone()),
        Some(vec![1, 2])
    );
    // No body is inserted.
    assert_eq!(on_plane(&operand, &before, delta(&[20, 21], &[])), None);
    // Only one result face lies on the plane.
    assert_eq!(on_plane(&operand, &before, delta(&[20], &[9])), None);
    // The references name different face sets.
    assert_eq!(
        on_plane(&tool(&[&[1, 2], &[1]]), &before, inserted.clone()),
        None
    );
    // The tool faces lie in parallel planes.
    let offset = planar(&[(1, 5.0, up), (2, 6.0, up)]);
    assert_eq!(on_plane(&operand, &offset, inserted.clone()), None);
    // A face outside the preceding topology rejects the selection.
    assert_eq!(on_plane(&tool(&[&[1, 7]]), &before, inserted), None);
}
