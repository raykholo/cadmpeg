// SPDX-License-Identifier: Apache-2.0
use super::{topology, FaceRecipeIndex};
use crate::history_records::AsmHistoricalTopology;
use crate::records::topology::edge_identity::DesignEdgeOperand;

fn operand() -> DesignEdgeOperand {
    let face = super::operand();
    let node = &face.recipe_nodes[0];
    let mut value = serde_json::to_value(&face).unwrap();
    value["recipe_program"] = serde_json::to_value(&node.program).unwrap();
    value["recipe_structure"] = serde_json::json!({
        "root": 2,
        "sides": node.recipe_structure.as_ref().unwrap().sides,
    });
    serde_json::from_value(value).unwrap()
}

fn two_components() -> AsmHistoricalTopology {
    let mut topology = topology();
    let mut other = topology.clone();
    for face in &mut other.faces {
        *face += 1000;
    }
    for edge in &mut other.edge_vertices {
        edge.edge += 1000;
        edge.start_vertex += 1000;
        edge.end_vertex += 1000;
    }
    for relation in other.face_loops.iter_mut().chain(&mut other.loop_coedges) {
        relation.owner_ref += 1000;
        for member in &mut relation.member_refs {
            *member += 1000;
        }
    }
    for row in &mut other.coedge_topology {
        row.coedge += 1000;
        row.owner_loop += 1000;
        row.edge += 1000;
        row.next += 1000;
        row.previous += 1000;
    }
    topology.faces.extend(other.faces);
    topology.edge_vertices.extend(other.edge_vertices);
    topology.face_loops.extend(other.face_loops);
    topology.loop_coedges.extend(other.loop_coedges);
    topology.coedge_topology.extend(other.coedge_topology);
    topology
}

#[test]
fn endpoint_clauses_distinguish_edges_with_the_same_incident_face_lanes() {
    let mut operand = operand();
    for (ordinal, reference) in operand.recipe_references.iter_mut().take(3).enumerate() {
        reference.candidate_faces.push(crate::ids::brep_face_id(
            1100 + i64::try_from(ordinal).unwrap(),
        ));
    }
    let index = FaceRecipeIndex::new(&two_components());
    assert_eq!(index.resolve_edge(&operand), Some(10));
    operand.recipe_references[3]
        .candidate_faces
        .push(crate::ids::brep_face_id(1103));
    assert_eq!(index.resolve_edge(&operand), None);
}

#[test]
fn edge_endpoint_fans_keep_one_ordinal_per_persistent_name() {
    let mut operand = operand();
    let mut additional = operand.recipe_references[0].clone();
    additional.design_reference = 999;
    additional
        .candidate_faces
        .push(crate::ids::brep_face_id(103));
    operand.recipe_references.insert(1, additional);
    assert_eq!(
        FaceRecipeIndex::new(&topology()).resolve_edge(&operand),
        Some(10)
    );
}

#[test]
fn edge_endpoint_fans_require_both_complete_endpoint_clauses() {
    let index = FaceRecipeIndex::new(&topology());
    let mut operand = operand();
    operand.recipe_structure.as_mut().unwrap().sides[1]
        .scalars
        .pop();
    assert_eq!(index.resolve_edge(&operand), None);
    let mut operand = self::operand();
    operand.recipe_structure.as_mut().unwrap().sides[0].scalars[1] = 2;
    assert_eq!(index.resolve_edge(&operand), None);
}

#[test]
fn edge_endpoint_fans_require_the_sentinel_node_and_input_endpoints() {
    let mut operand = operand();
    operand.recipe_program[3] = 1;
    assert_eq!(
        FaceRecipeIndex::new(&topology()).resolve_edge(&operand),
        None
    );
    let mut topology = topology();
    topology.edge_vertices.clear();
    assert_eq!(
        FaceRecipeIndex::new(&topology).resolve_edge(&self::operand()),
        None
    );
}
