// SPDX-License-Identifier: Apache-2.0
use super::move_face::{history, SCOPE_ID};
use crate::history::bind_feature_face_selections;
use crate::history::split_tool::resolve_split_tool_coplanar_faces;
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
    // One inserted face and one updated face lie on the plane.
    let mut updated = delta(&[20], &[9]);
    updated.faces.updated = vec![21];
    assert_eq!(on_plane(&operand, &before, updated), Some(vec![1, 2]));
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

fn bind_tool(slots: Vec<i64>) -> cadmpeg_ir::features::FaceSelection {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureInputTopology, FeatureOperation, SplitBodyTool,
    };
    let mut scope = DesignParameterScope::empty(SCOPE_ID, DesignFeatureKind::Split, 42);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut operand = tool(&[&[10, 20]]);
    operand.scope_record_index = 42;
    operand.resolved_face_slots = slots;
    let feature_id = FeatureId::mint("f3d:test:feature#42").expect("identity grammar");
    let mut feature = Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::SplitBody {
                targets: cadmpeg_ir::features::BodySelection::Native("f3d:test:targets".into()),
                tool: SplitBodyTool::Faces(FaceSelection::Native(operand.id.clone())),
            },
        )),
        native_ref: Some(SCOPE_ID.into()),
    };
    let mut input_topologies = vec![FeatureInputTopology {
        id: crate::ids::feature_input_topology_id(&feature_id, 1),
        input_of: feature_id,
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        faces: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        edges: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        vertices: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        edge_geometry: Vec::new(),
        native_ref: None,
    }];
    bind_feature_face_selections(
        std::slice::from_mut(&mut feature),
        &mut input_topologies,
        crate::history::FeatureFaceSelectionInputs {
            scopes: std::slice::from_ref(&scope),
            groups: &[],
            operands: std::slice::from_ref(&operand),
            entity_operands: &[],
            body_recipe_operands: &[],
            histories: &mut [history(8.0, 5.0)],
            scope_histories: &std::collections::HashMap::new(),
        },
    );
    let FeatureDefinition::Operation(FeatureOperation::SplitBody {
        tool: SplitBodyTool::Faces(tools),
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("expected a SplitBody");
    };
    tools.clone()
}

#[test]
fn split_tool_bound_in_the_header_predecessor_state_when_the_scope_stores_none() {
    let cadmpeg_ir::features::FaceSelection::Historical { faces, .. } = bind_tool(vec![10, 20])
    else {
        panic!("expected a historical selection");
    };
    let faces: Vec<&str> = faces
        .iter()
        .map(cadmpeg_ir::ids::HistoricalFaceId::as_str)
        .collect();
    assert_eq!(faces.len(), 2);
    assert!(
        faces[0].ends_with(":10") && faces[1].ends_with(":20"),
        "{faces:?}"
    );
}

#[test]
fn split_tool_without_proven_faces_stays_native() {
    assert!(matches!(
        bind_tool(Vec::new()),
        cadmpeg_ir::features::FaceSelection::Native(_)
    ));
}

fn with_tool_bodies(
    mut topology: AsmHistoricalTopology,
    bodies: &[(i64, i64)],
) -> AsmHistoricalTopology {
    use crate::history_records::AsmHistoricalRelation;
    for &(body, face) in bodies {
        let (region, shell) = (body + 1000, body + 2000);
        topology.bodies.push(body);
        topology.regions.push(region);
        topology.shells.push(shell);
        topology.body_regions.push(AsmHistoricalRelation {
            owner_ref: body,
            member_refs: vec![region],
        });
        topology.region_shells.push(AsmHistoricalRelation {
            owner_ref: region,
            member_refs: vec![shell],
        });
        topology.shell_faces.push(AsmHistoricalRelation {
            owner_ref: shell,
            member_refs: vec![face],
        });
    }
    topology
}

#[test]
fn consumed_tool_bodies_resolve_to_their_faces_on_the_meeting_plane() {
    use crate::history::split_tool::consumed_tool_body_faces;
    let up = Vector3::new(0.0, 0.0, 1.0);
    let down = Vector3::new(0.0, 0.0, -1.0);
    let before = with_tool_bodies(
        planar(&[(1, 0.0, up), (200, 5.0, up), (201, 5.0, down)]),
        &[(100, 200), (101, 201)],
    );
    let result = planar(&[(20, 5.0, up), (21, 5.0, down)]);
    let mut split = delta(&[20], &[2]);
    split.faces.updated = vec![21];
    split.bodies.deleted = vec![100, 101];
    assert_eq!(
        consumed_tool_body_faces(&before, &result, &split),
        Some(vec![200, 201])
    );
    // The Split inserts no body.
    let mut no_body = split.clone();
    no_body.bodies.inserted.clear();
    assert_eq!(consumed_tool_body_faces(&before, &result, &no_body), None);
    // The Split deletes no body.
    let mut kept = split.clone();
    kept.bodies.deleted.clear();
    assert_eq!(consumed_tool_body_faces(&before, &result, &kept), None);
    // The deleted bodies' faces lie in different planes.
    let apart = with_tool_bodies(
        planar(&[(1, 0.0, up), (200, 5.0, up), (201, 6.0, down)]),
        &[(100, 200), (101, 201)],
    );
    assert_eq!(consumed_tool_body_faces(&apart, &result, &split), None);
    // The result faces lie in another plane.
    let elsewhere = planar(&[(20, 9.0, up), (21, 9.0, down)]);
    assert_eq!(consumed_tool_body_faces(&before, &elsewhere, &split), None);
    // A deleted body that the preceding state does not hold completely.
    let mut missing = split;
    missing.bodies.deleted.push(102);
    assert_eq!(consumed_tool_body_faces(&before, &result, &missing), None);
}
