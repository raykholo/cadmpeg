// SPDX-License-Identifier: Apache-2.0
//! Source geometry of selected fillet and chamfer edges.

use crate::brep::attrib::NativeIdentity;
use crate::records::{FeatureInputEdgeSelection, FeatureInputLane};
use crate::resolved_features::component_paths::{
    compact_edge_path_value, compact_edge_selection_set_value,
};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DistinctMembers, EdgeSelection, FeatureOperation};
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureInputTopology, GeneratedEdgeRef, HistoricalEdgeGeometry,
};
use cadmpeg_ir::ids::{FaceId, FeatureInputTopologyId, HistoricalEdgeId};
use cadmpeg_ir::CadIr;
use std::collections::{HashMap, HashSet};

mod geometry;

#[derive(Default)]
pub(crate) struct Inputs {
    operands: HashMap<String, Vec<crate::carchive::EdgeOperand>>,
    reference_rules: HashMap<String, crate::carchive::ReferenceRules>,
    native_selections: HashMap<String, Vec<FeatureInputEdgeSelection>>,
}

impl Inputs {
    pub(crate) fn insert(&mut self, lane: &str, edges: Vec<crate::carchive::EdgeOperand>) {
        self.operands.insert(lane.to_owned(), edges);
    }

    pub(crate) fn insert_reference_rules(
        &mut self,
        lane: &str,
        rules: crate::carchive::ReferenceRules,
    ) {
        self.reference_rules.insert(lane.to_owned(), rules);
    }

    pub(crate) fn retain_operands(
        &mut self,
        lanes: &mut [crate::records::FeatureInputLane],
    ) -> Result<(), cadmpeg_core::CodecError> {
        for lane in lanes {
            let Some(edges) = self.operands.get(&lane.id) else {
                continue;
            };
            let mut markers = HashMap::<&str, HashSet<u64>>::new();
            for name in &lane.names {
                for edge in edges {
                    let offset = cadmpeg_core::decode::u64_from_index(edge.marker);
                    if usize::try_from(name.offset)
                        .ok()
                        .is_some_and(|at| edge.owner.contains(&at))
                    {
                        markers.entry(&name.id).or_default().insert(offset);
                    }
                }
            }
            self.native_selections
                .insert(lane.id.clone(), lane.edge_selections.clone());
            lane.edge_selections.retain(|selection| {
                markers
                    .get(selection.object_name_ref.as_str())
                    .is_none_or(|owned| owned.contains(&selection.offset))
            });
            for (ordinal, selection) in lane.edge_selections.iter_mut().enumerate() {
                selection.ordinal = u32::try_from(ordinal).map_err(|_| {
                    cadmpeg_core::CodecError::malformed("SLDPRT blend operand ordinal exceeds u32")
                })?;
            }
        }
        Ok(())
    }
    pub(crate) fn native_lanes(
        mut self,
        mut lanes: Vec<FeatureInputLane>,
    ) -> Vec<FeatureInputLane> {
        for lane in &mut lanes {
            if let Some(selections) = self.native_selections.remove(&lane.id) {
                lane.edge_selections = selections;
            }
        }
        lanes
    }
}

pub(crate) fn missing_geometry_count(operation: &FeatureOperation) -> usize {
    let count = |selection: &EdgeSelection| match selection {
        EdgeSelection::Generated { edges, .. } => edges.len(),
        EdgeSelection::HistoricalPartial { unresolved, .. } => unresolved.len(),
        _ => 0,
    };
    match operation {
        FeatureOperation::Fillet { groups } => groups.iter().map(|group| count(&group.edges)).sum(),
        FeatureOperation::Chamfer { groups, .. } => {
            groups.iter().map(|group| count(&group.edges)).sum()
        }
        _ => 0,
    }
}

pub(crate) fn missing_chain_count(operation: &FeatureOperation) -> usize {
    match operation {
        FeatureOperation::Fillet { groups } => groups.len(),
        FeatureOperation::Chamfer { groups, .. } => groups.len(),
        _ => 0,
    }
}

pub(crate) fn missing_station_start_count(operation: &FeatureOperation) -> usize {
    use cadmpeg_ir::features::edge_treatments::{RadiusForm, RadiusSpec};
    match operation {
        FeatureOperation::Fillet { groups } => groups
            .iter()
            .filter(|group| {
                matches!(
                    group.radius,
                    RadiusSpec::Variable { .. }
                        | RadiusSpec::Unresolved {
                            form: Some(RadiusForm::Variable)
                        }
                )
            })
            .count(),
        _ => 0,
    }
}

struct SourceEdge<'a> {
    selection: &'a FeatureInputEdgeSelection,
    generated: GeneratedEdgeRef,
    geometry: Option<geometry::SourceLine>,
    key: Option<[crate::carchive::rules::idrep::SurfaceIdentity; 4]>,
}

fn generated_selection(selection: &EdgeSelection) -> Option<(&str, &[GeneratedEdgeRef])> {
    match selection {
        EdgeSelection::Generated { edges, native } => Some((native.as_str(), edges.as_slice())),
        _ => None,
    }
}

fn group_selections(feature: &Feature) -> Vec<&EdgeSelection> {
    match feature.evaluation.definition().operation() {
        FeatureOperation::Fillet { groups } => groups.iter().map(|group| &group.edges).collect(),
        FeatureOperation::Chamfer { groups, .. } => {
            groups.iter().map(|group| &group.edges).collect()
        }
        _ => Vec::new(),
    }
}

fn replace_groups(feature: &mut Feature, replacements: &[(String, EdgeSelection)]) {
    feature.evaluation.edit(|definition, _| {
        let replace = |selection: &mut EdgeSelection| {
            let Some((native, _)) = generated_selection(selection) else {
                return;
            };
            if let Some((_, replacement)) = replacements.iter().find(|(key, _)| key == native) {
                *selection = replacement.clone();
            }
        };
        match definition {
            FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                for group in groups.iter_mut() {
                    replace(&mut group.edges);
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
                for group in groups.iter_mut() {
                    replace(&mut group.edges);
                }
            }
            _ => {}
        }
    });
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    persistent: &[NativeIdentity<FaceId>],
    lanes: &[FeatureInputLane],
    inputs: &Inputs,
) -> Result<(), CodecError> {
    let active = ir
        .model
        .configurations
        .iter()
        .enumerate()
        .filter(|(_, configuration)| configuration.active)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [active] = active.as_slice() else {
        return Ok(());
    };
    let assigned =
        super::configuration::configuration_lane_assignments(&ir.model.configurations, lanes);
    let scoped = assigned
        .iter()
        .filter(|(configuration, _)| configuration == active)
        .map(|(_, lane)| *lane)
        .collect::<HashSet<_>>();
    let producers =
        crate::resolved_features::projections::compact_edge_feature_ids(&ir.model.features);
    let last_body_feature = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.suppressed != Some(true))
        .filter(|feature| {
            !matches!(
                feature.evaluation.definition().operation(),
                FeatureOperation::TreeNode { .. }
                    | FeatureOperation::Sketch { .. }
                    | FeatureOperation::DatumPrincipalPlane { .. }
            )
        })
        .map(|feature| feature.ordinal)
        .max();
    for index in 0..ir.model.features.len() {
        let feature = &ir.model.features[index];
        if group_selections(feature).is_empty() {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let eligible = last_body_feature == Some(feature.ordinal);
        let mut sources = Vec::new();
        for (_, lane) in lanes
            .iter()
            .enumerate()
            .filter(|(index, lane)| lane.configuration.is_none() || scoped.contains(index))
        {
            let Some(references) = inputs.operands.get(&lane.id) else {
                continue;
            };
            for selection in lane
                .edge_selections
                .iter()
                .filter(|selection| selection.feature_ref == native_ref)
            {
                let Some(producer) = selection
                    .terminal_feature_ref
                    .as_deref()
                    .and_then(|native| producers.get(native))
                else {
                    continue;
                };
                let generated =
                    GeneratedEdgeRef::new(producer.clone(), compact_edge_path_value(selection))
                        .map_err(CodecError::malformed)?;
                let offset = usize::try_from(selection.offset).ok();
                let reference = references
                    .iter()
                    .find(|reference| Some(reference.marker) == offset);
                let variable =
                    missing_station_start_count(feature.evaluation.definition().operation()) > 0;
                let keys = if let Some(reference) = reference {
                    reference.faces
                } else if variable {
                    match offset.zip(inputs.reference_rules.get(&lane.id)) {
                        Some((offset, rules)) => {
                            rules.surface_hops(ctx, &lane.native_payload, offset)?
                        }
                        None => None,
                    }
                } else {
                    continue;
                };
                let consumer = lane
                    .names
                    .iter()
                    .find(|name| name.id == selection.object_name_ref)
                    .and_then(|name| name.object_id.and_then(crate::records::ObjectId::value))
                    .filter(|id| {
                        lane.names
                            .iter()
                            .filter(|name| {
                                name.object_id.and_then(crate::records::ObjectId::value)
                                    == Some(*id)
                            })
                            .count()
                            == 1
                    });
                let fillet = matches!(
                    feature.evaluation.definition().operation(),
                    FeatureOperation::Fillet { .. }
                );
                let geometry = eligible
                    .then(|| {
                        keys.as_ref().zip(consumer).and_then(|(keys, consumer)| {
                            geometry::source_line(ir, persistent, keys, consumer, fillet)
                        })
                    })
                    .flatten();
                sources.push(SourceEdge {
                    selection,
                    generated,
                    geometry,
                    key: keys,
                });
            }
        }
        if sources.is_empty() {
            continue;
        }
        let key = feature
            .id
            .as_str()
            .rsplit_once('#')
            .map_or(feature.id.as_str(), |(_, key)| key);
        let state =
            FeatureInputTopologyId::mint(format!("sldprt:feature-input:topology#{key}:{active}"))
                .map_err(CodecError::malformed)?;
        let mut geometry = Vec::new();
        let mut admitted = HashSet::new();
        let mut replacements = Vec::new();
        for selection in group_selections(feature) {
            let Some((old_native, selected)) = generated_selection(selection) else {
                continue;
            };
            let selected_sources = sources
                .iter()
                .filter(|source| selected.contains(&source.generated))
                .collect::<Vec<_>>();
            let mut seen = HashSet::new();
            let sources = selected_sources
                .iter()
                .copied()
                .filter(|source| {
                    seen.insert((
                        source.generated.feature.clone(),
                        source.generated.local_id.clone(),
                        source.key,
                    ))
                })
                .collect::<Vec<_>>();
            if sources.is_empty() {
                continue;
            }
            let unmatched = selected
                .iter()
                .filter(|edge| !sources.iter().any(|source| &source.generated == *edge))
                .collect::<Vec<_>>();
            let native = if unmatched.is_empty() {
                compact_edge_selection_set_value(
                    &selected_sources
                        .iter()
                        .map(|source| source.selection)
                        .collect::<Vec<_>>(),
                )
            } else {
                old_native.to_owned()
            };
            let mut edges = Vec::new();
            let mut unresolved = unmatched
                .iter()
                .map(|edge| format!("{}:{}", edge.feature, edge.local_id.as_str()))
                .collect::<Vec<_>>();
            for source in &sources {
                let Some(sample) = source.geometry else {
                    unresolved.push(source.selection.id.clone());
                    continue;
                };
                let key = source
                    .selection
                    .id
                    .rsplit_once('#')
                    .map_or(source.selection.id.as_str(), |(_, key)| key);
                let id = HistoricalEdgeId::mint(format!("sldprt:feature-input:edge#{key}"))
                    .map_err(CodecError::malformed)?;
                if admitted.insert(id.clone()) {
                    geometry.push(HistoricalEdgeGeometry {
                        edge: id.clone(),
                        start: Some(sample.start),
                        end: Some(sample.end),
                        interior: sample.interior,
                        tangent: None,
                        faces: DistinctMembers::default(),
                    });
                }
                edges.push(id);
            }
            let replacement = if edges.is_empty() {
                let mut generated = HashSet::new();
                EdgeSelection::generated(
                    sources
                        .iter()
                        .filter(|source| {
                            generated.insert((
                                source.generated.feature.clone(),
                                source.generated.local_id.clone(),
                            ))
                        })
                        .map(|source| source.generated.clone())
                        .chain(unmatched.into_iter().cloned())
                        .collect(),
                    native,
                )
            } else if unresolved.is_empty() {
                EdgeSelection::historical(state.clone(), edges, native)
            } else {
                EdgeSelection::historical_partial(state.clone(), edges, unresolved, native)
            }
            .map_err(CodecError::malformed)?;
            replacements.push((old_native.to_owned(), replacement));
        }
        let input_of = feature.id.clone();
        let native_ref = feature.native_ref.clone();
        if !geometry.is_empty() {
            ctx.charge_entities(
                1 + u64_from_index(geometry.len()),
                "admit SLDPRT blend source edges",
            )?;
            ir.model
                .feature_input_topologies
                .push(FeatureInputTopology {
                    id: state,
                    input_of,
                    bodies: DistinctMembers::default(),
                    faces: DistinctMembers::default(),
                    edges: geometry
                        .iter()
                        .map(|sample| sample.edge.clone())
                        .collect::<Vec<_>>()
                        .try_into()
                        .map_err(CodecError::malformed)?,
                    vertices: DistinctMembers::default(),
                    edge_geometry: geometry,
                    native_ref,
                });
        }
        replace_groups(&mut ir.model.features[index], &replacements);
        let feature = &ir.model.features[index];
        if let Some(state) = ir.model.configurations[*active]
            .feature_states
            .get_mut(&feature.id)
        {
            state.definition = feature.evaluation.definition().clone();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::missing_geometry_count;
    use cadmpeg_ir::features::edge_treatments::{
        ChamferGroup, ChamferSpec, FilletGroup, RadiusSpec,
    };
    use cadmpeg_ir::features::{EdgeSelection, FeatureId, FeatureOperation, GeneratedEdgeRef};

    fn generated() -> EdgeSelection {
        let producer = FeatureId::mint("synthetic:test:id#producer").expect("feature id");
        let edges = ["1", "2"]
            .into_iter()
            .map(|local| GeneratedEdgeRef::new(producer.clone(), local.into()).expect("edge"))
            .collect();
        EdgeSelection::generated(edges, "native:selection".into()).expect("selection")
    }

    #[test]
    fn completed_operand_owner_filters_its_result_cache() {
        let lane_id = "synthetic:test:lane#1";
        let name = |id, offset| {
            serde_json::json!({
                "id": id, "parent": lane_id, "ordinal": 0, "offset": offset,
                "value": "Blend",
            })
        };
        let selection = |offset, owner| {
            serde_json::json!({
                "id": format!("synthetic:test:edge#{offset}"), "parent": lane_id,
                "ordinal": 9, "offset": offset, "object_name_ref": owner,
                "feature_ref": "synthetic:test:feature#1", "local_edge_ids": [1],
            })
        };
        let lane = serde_json::from_value(serde_json::json!({
            "id": lane_id, "native_payload": "",
            "names": [name("owned", 20), name("other", 120)],
            "edge_selections": [selection(30, "owned"), selection(40, "owned"), selection(130, "other")],
        })).expect("synthetic lane");
        let mut lanes = [lane];
        let mut inputs = super::Inputs::default();
        inputs.insert(
            lane_id,
            vec![crate::carchive::EdgeOperand {
                owner: 12..100,
                marker: 40,
                faces: None,
            }],
        );
        inputs.retain_operands(&mut lanes).expect("operand roles");
        assert_eq!(
            lanes[0]
                .edge_selections
                .iter()
                .map(|edge| (edge.offset, edge.ordinal))
                .collect::<Vec<_>>(),
            vec![(40, 0), (130, 1)]
        );
        let native = inputs.native_lanes(lanes.to_vec());
        assert_eq!(
            native[0]
                .edge_selections
                .iter()
                .map(|edge| edge.offset)
                .collect::<Vec<_>>(),
            vec![30, 40, 130]
        );
    }

    #[test]
    fn each_generated_blend_edge_requires_source_geometry() {
        let fillet = FeatureOperation::Fillet {
            groups: vec![FilletGroup {
                edges: generated(),
                radius: RadiusSpec::Unresolved { form: None },
                tangency_weight: None,
            }]
            .try_into()
            .expect("group"),
        };
        let chamfer = FeatureOperation::Chamfer {
            groups: vec![ChamferGroup {
                edges: generated(),
                spec: ChamferSpec::Unresolved { form: None },
            }]
            .try_into()
            .expect("group"),
            flip_direction: false,
        };
        assert_eq!(missing_geometry_count(&fillet), 2);
        assert_eq!(missing_geometry_count(&chamfer), 2);
        assert_eq!(super::missing_chain_count(&fillet), 1);
        assert_eq!(super::missing_chain_count(&chamfer), 1);
    }
}
