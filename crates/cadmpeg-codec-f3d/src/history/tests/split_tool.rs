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

fn entity_group(
    members: &[u32],
) -> crate::records::topology::construction::DesignConstructionOperandGroup {
    let member_offsets: Vec<u64> = (0..u64::try_from(members.len()).unwrap())
        .map(|ordinal| ordinal * 11)
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:group#43",
        "byte_offset": 0,
        "class_tag": "275",
        "record_index": 43,
        "scope_record_index": 42,
        "scope_reference_ordinal": 0,
        "members": members,
        "member_offsets": member_offsets,
        "role": 141_733_920_768_u64,
        "role_offset": 0,
        "paired_byte_offset": 200,
        "paired_class_tag": "261",
        "frame": {
            "member_count_offset": 0,
            "opaque_index": 1,
            "opaque_index_offset": 18,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 22,
            "trailing_flags": [],
            "trailing_record_indices": [],
            "trailing_record_offsets": [],
            "variant": false
        }
    }))
    .unwrap()
}

fn entity_selection(
    record_index: u32,
    candidates: &serde_json::Value,
) -> crate::records::topology::entity_selection::DesignEntitySelectionOperand {
    let mut value = serde_json::json!({
        "id": format!("f3d:Design/BulkStream.dat:entity-selection#{record_index}"),
        "scope_record_index": 42,
        "group_record_index": 43,
        "group_member_ordinal": 0,
        "record_index": record_index,
        "byte_offset": 0,
        "class_tag": "386",
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        "asset_id_offset": 0,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e",
        "context_id_offset": 0,
        "identity_record_index": record_index + 3,
        "identity_record_offset": 0,
        "primary_identity": 7,
        "primary_identity_offset": 21,
        "next_record_index": record_index + 4,
        "next_byte_offset": 29
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(candidates.as_object().unwrap().clone());
    serde_json::from_value(value).unwrap()
}

fn face_member(
    record_index: u32,
    face: i64,
) -> crate::records::topology::entity_selection::DesignEntitySelectionOperand {
    entity_selection(
        record_index,
        &serde_json::json!({"historical_face_candidates": [{
            "history_id": "f3d:history",
            "historical_entity_kind": "face",
            "historical_entity_ref": face,
            "face_slot": face
        }]}),
    )
}

fn vertex_member(
    record_index: u32,
    vertex: i64,
) -> crate::records::topology::entity_selection::DesignEntitySelectionOperand {
    entity_selection(
        record_index,
        &serde_json::json!({"historical_edge_candidates": [{
            "identity_ordinal": 0,
            "local_id": 7,
            "historical_entity_kind": "vertex",
            "historical_entity_ref": vertex,
            "edge_slots": [1, 2]
        }]}),
    )
}

fn split_scope() -> crate::records::feature::scope::DesignParameterScope {
    crate::records::feature::scope::DesignParameterScope::empty(
        SCOPE_ID,
        crate::records::feature::scope::DesignFeatureKind::Split,
        42,
    )
}

#[test]
fn entity_tool_members_name_one_face_and_the_vertices() {
    use crate::history::split_tool::entity_tool_members;
    let scope = split_scope();
    let group = entity_group(&[100, 101, 102]);
    let operands = [
        face_member(100, 5),
        vertex_member(101, 8),
        vertex_member(102, 9),
    ];
    assert_eq!(
        entity_tool_members(&scope, &group.id, std::slice::from_ref(&group), &operands),
        Some((5, vec![8, 9]))
    );
    // A member without a decoded selection.
    assert_eq!(
        entity_tool_members(
            &scope,
            &group.id,
            std::slice::from_ref(&group),
            &operands[..2]
        ),
        None
    );
    // Two face members.
    let twice = [
        face_member(100, 5),
        face_member(101, 6),
        vertex_member(102, 9),
    ];
    assert_eq!(
        entity_tool_members(&scope, &group.id, std::slice::from_ref(&group), &twice),
        None
    );
    // No face member.
    let none = entity_group(&[101, 102]);
    assert_eq!(
        entity_tool_members(
            &scope,
            &none.id,
            std::slice::from_ref(&none),
            &operands[1..]
        ),
        None
    );
}

fn with_vertices(
    mut topology: AsmHistoricalTopology,
    vertices: &[(i64, Point3)],
) -> AsmHistoricalTopology {
    use crate::history_records::AsmHistoricalPoint;
    for &(vertex, position) in vertices {
        topology.vertices.push(vertex);
        topology.vertex_points.push(AsmHistoricalCarrierBinding {
            entity: vertex,
            carrier: vertex + 5000,
        });
        topology.point_positions.push(AsmHistoricalPoint {
            point: vertex + 5000,
            position,
        });
    }
    topology
}

#[test]
fn split_tool_face_with_vertices_resolves_when_the_vertices_and_result_lie_on_its_plane() {
    use crate::history::split_tool::resolve_split_tool_face_and_vertices as resolve;
    let up = Vector3::new(0.0, 0.0, 1.0);
    let down = Vector3::new(0.0, 0.0, -1.0);
    let before = with_vertices(
        planar(&[(1, 5.0, up), (2, 9.0, up)]),
        &[
            (30, Point3::new(1.0, 2.0, 5.0)),
            (31, Point3::new(-4.0, 7.0, 5.0)),
            (32, Point3::new(0.0, 0.0, 9.0)),
        ],
    );
    let result = planar(&[(20, 5.0, up), (21, 5.0, down)]);
    let split = delta(&[20, 21], &[9]);
    assert_eq!(
        resolve(1, &[30, 31], &before, &result, &split),
        Some(vec![1])
    );
    // A vertex off the plane.
    assert_eq!(resolve(1, &[30, 32], &before, &result, &split), None);
    // A vertex outside the preceding topology.
    assert_eq!(resolve(1, &[30, 33], &before, &result, &split), None);
    // No vertex.
    assert_eq!(resolve(1, &[], &before, &result, &split), None);
    // A face outside the preceding topology.
    assert_eq!(resolve(3, &[30], &before, &result, &split), None);
    // The Split inserts no body.
    assert_eq!(
        resolve(1, &[30], &before, &result, &delta(&[20, 21], &[])),
        None
    );
    // The result faces lie in another plane.
    let elsewhere = planar(&[(20, 7.0, up), (21, 7.0, down)]);
    assert_eq!(resolve(1, &[30], &before, &elsewhere, &split), None);
}

#[test]
fn composed_delta_nets_the_transitions_of_a_state_chain() {
    use crate::history::split_tool::composed_delta;
    use crate::history_records::AsmHistoricalTopologyDelta;
    let step = |state_id: i64, previous: i64, delta: AsmHistoricalTopologyDelta| {
        let mut state =
            super::move_face::state(state_id, AsmHistoricalTopology::default(), None, 1).0;
        state.transition = Some(crate::history_records::AsmHistoricalTransition {
            geometry_unchanged: false,
            previous_state_id: Some(previous),
            records: Default::default(),
            topology: delta,
            record_revisions: Vec::new(),
            rows: Default::default(),
        });
        state
    };
    let mut first = delta(&[20, 21], &[9]);
    first.faces.deleted = vec![1];
    let mut second = AsmHistoricalTopologyDelta::default();
    second.faces.updated = vec![20, 2];
    second.faces.deleted = vec![21, 3];
    second.bodies.inserted = vec![10];
    let states = [
        step(3, 2, second),
        step(2, 1, first),
        super::move_face::state(1, AsmHistoricalTopology::default(), None, 1).0,
    ];
    let index: std::collections::HashMap<i64, Option<&crate::history_records::AsmDeltaState>> =
        states
            .iter()
            .map(|state| (state.state_id, Some(state)))
            .collect();
    let net = composed_delta(&states[0], 1, &index).unwrap();
    assert_eq!(net.bodies.inserted, vec![9, 10]);
    assert_eq!(net.faces.inserted, vec![20]);
    assert_eq!(net.faces.updated, vec![2]);
    assert_eq!(net.faces.deleted, vec![1, 3]);
    // The chain does not reach the requested state.
    assert!(composed_delta(&states[0], 7, &index).is_none());
}
