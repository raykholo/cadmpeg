// SPDX-License-Identifier: Apache-2.0
//! Midpoint of a historical edge slot named by a feature input.
use crate::history_records::AsmHistoricalTopology;
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use cadmpeg_ir::math::Point3;

type TagKey = (i64, String, Vec<i64>);

fn edge_sample(topology: &AsmHistoricalTopology, slot: i64) -> Option<Point3> {
    let mut samples = topology.edge_samples.iter().filter(|s| s.edge == slot);
    let sample = samples.next()?;
    samples.next().is_none().then_some(sample.interior)
}

/// The sorted persistent tag groups of edge `slot`, or `None` when it has none.
fn edge_tag_set(topology: &AsmHistoricalTopology, slot: i64) -> Option<Vec<TagKey>> {
    let mut tags: Vec<TagKey> = topology
        .persistent_subentity_tags
        .iter()
        .filter(|tag| tag.entity_kind == AsmHistoricalEntityKind::Edge && tag.entity_ref == slot)
        .map(|tag| {
            (
                tag.selector,
                tag.token.clone(),
                tag.design_references.clone(),
            )
        })
        .collect();
    if tags.is_empty() {
        return None;
    }
    tags.sort();
    Some(tags)
}

/// The edge of `lookup` that carries exactly the persistent tag groups of
/// edge `slot` in `source`.
///
/// `slot` has to be an edge of `source` and not an edge of `lookup`. The tag
/// set has to be non-empty and carried by no other edge of `source`, and by
/// exactly one edge of `lookup`.
fn tag_successor(
    source: &AsmHistoricalTopology,
    lookup: &AsmHistoricalTopology,
    slot: i64,
) -> Option<i64> {
    if !source.edges.contains(&slot) || lookup.edges.contains(&slot) {
        return None;
    }
    let tags = edge_tag_set(source, slot)?;
    if source
        .edges
        .iter()
        .any(|&edge| edge != slot && edge_tag_set(source, edge).as_ref() == Some(&tags))
    {
        return None;
    }
    let mut matches = lookup
        .edges
        .iter()
        .copied()
        .filter(|&edge| edge_tag_set(lookup, edge).as_ref() == Some(&tags));
    let successor = matches.next()?;
    matches.next().is_none().then_some(successor)
}

/// The carrier midpoint of historical edge `slot`.
///
/// `source` is the state that names the slot, `lookup` the preceding state of
/// the output transition. A slot that is an edge of `source` is sampled there.
/// Otherwise a slot that is an edge of `lookup` is sampled there. A slot that
/// is an edge of neither, or an edge of `source` only, maps to the one edge of
/// `lookup` that carries its persistent tag groups.
pub(super) fn historical_edge_midpoint(
    source: Option<&AsmHistoricalTopology>,
    lookup: &AsmHistoricalTopology,
    slot: i64,
) -> Option<Point3> {
    if let Some(point) = source.and_then(|source| edge_sample(source, slot)) {
        return Some(point);
    }
    if let Some(point) = edge_sample(lookup, slot) {
        return Some(point);
    }
    let successor = tag_successor(source?, lookup, slot)?;
    edge_sample(lookup, successor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_records::{AsmHistoricalEdgeSample, AsmHistoricalPersistentSubentityTag};

    fn tag(edge: i64, selector: i64, token: &str) -> AsmHistoricalPersistentSubentityTag {
        AsmHistoricalPersistentSubentityTag {
            entity_kind: AsmHistoricalEntityKind::Edge,
            entity_ref: edge,
            selector,
            token: token.into(),
            design_references: vec![7],
            ordinal: 0,
        }
    }

    fn state(
        edges: &[(i64, f64)],
        tags: Vec<AsmHistoricalPersistentSubentityTag>,
    ) -> AsmHistoricalTopology {
        AsmHistoricalTopology {
            edges: edges.iter().map(|e| e.0).collect(),
            edge_samples: edges
                .iter()
                .map(|&(edge, x)| AsmHistoricalEdgeSample {
                    edge,
                    interior: Point3::new(x, 0.0, 0.0),
                    tangent: None,
                })
                .collect(),
            persistent_subentity_tags: tags,
            ..AsmHistoricalTopology::default()
        }
    }

    #[test]
    fn slot_in_source_state_is_sampled_there() {
        let source = state(&[(5, 1.0)], Vec::new());
        let lookup = state(&[(6, 2.0)], Vec::new());
        assert_eq!(
            historical_edge_midpoint(Some(&source), &lookup, 5),
            Some(Point3::new(1.0, 0.0, 0.0))
        );
    }

    #[test]
    fn stale_slot_maps_to_unique_tag_successor() {
        let source = state(&[(5, 1.0), (8, 4.0)], vec![tag(5, 2, "a"), tag(8, 2, "b")]);
        let lookup = state(&[(6, 2.0), (8, 4.0)], vec![tag(6, 2, "a"), tag(8, 2, "b")]);
        assert_eq!(
            historical_edge_midpoint(Some(&source), &lookup, 5),
            Some(Point3::new(1.0, 0.0, 0.0))
        );
        // Without a source sample the lineage path applies.
        let mut source = source;
        source.edge_samples.retain(|s| s.edge != 5);
        assert_eq!(
            historical_edge_midpoint(Some(&source), &lookup, 5),
            Some(Point3::new(2.0, 0.0, 0.0))
        );
    }

    #[test]
    fn stale_slot_refuses_ambiguous_missing_or_untagged_lineage() {
        let mut source = state(&[(5, 1.0)], vec![tag(5, 2, "a")]);
        source.edge_samples.clear();
        let ambiguous = state(&[(6, 2.0), (7, 3.0)], vec![tag(6, 2, "a"), tag(7, 2, "a")]);
        assert_eq!(historical_edge_midpoint(Some(&source), &ambiguous, 5), None);
        let missing = state(&[(6, 2.0)], vec![tag(6, 2, "b")]);
        assert_eq!(historical_edge_midpoint(Some(&source), &missing, 5), None);
        let untagged = state(&[(6, 2.0)], Vec::new());
        let mut bare = state(&[(5, 1.0)], Vec::new());
        bare.edge_samples.clear();
        assert_eq!(historical_edge_midpoint(Some(&bare), &untagged, 5), None);
        assert_eq!(historical_edge_midpoint(None, &missing, 5), None);
        // Another source edge with the same tag set makes the source side ambiguous.
        let mut twin = state(&[(5, 1.0), (9, 5.0)], vec![tag(5, 2, "a"), tag(9, 2, "a")]);
        twin.edge_samples.clear();
        let lookup = state(&[(6, 2.0)], vec![tag(6, 2, "a")]);
        assert_eq!(historical_edge_midpoint(Some(&twin), &lookup, 5), None);
    }
}
