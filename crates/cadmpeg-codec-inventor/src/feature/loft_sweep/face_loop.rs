// SPDX-License-Identifier: Apache-2.0
//! Face-loop sections of surface lofts: the join of their face item and edge
//! items to the edges of the input topology.

use std::collections::HashSet;

use cadmpeg_core::{
    decode::{DecodeContext, View},
    CodecError,
};
use cadmpeg_ir::features::FinitePoint3;

use crate::feature::{
    edge_selection::{
        loops::{EdgeVertices, Revision},
        EdgeInventory,
    },
    face_selection::SelectedFace,
    PmDcFeature, PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily, ProjectionIndex,
};
use crate::pmdc::{
    content_header, reference_list_with_context, state_content_header, Cursor, PmDcReference,
};

/// Record type of a face-loop participant of a feature label.
pub(in crate::feature) const LOOP_TYPE: [u8; 16] = [
    0x7c, 0xa8, 0x82, 0xb3, 0xf4, 0x45, 0xb9, 0xff, 0xfe, 0x4a, 0x74, 0x86, 0x10, 0x48, 0x13, 0xa4,
];
/// Record type of the peer record that a face-loop participant refers to.
pub(in crate::feature) const PEER_TYPE: [u8; 16] = [
    0xb5, 0x70, 0x49, 0xda, 0xcb, 0x41, 0xe6, 0x9b, 0xd6, 0x69, 0xc1, 0xab, 0x1e, 0xc8, 0x9a, 0xf8,
];
/// Value that ends a face-loop participant.
const LOOP_VALUE: u32 = 1;
/// Largest distance between two vertices that are the same loop vertex.
const EPS_LOOP_POINT_MM: f64 = 1.0e-5;

/// A face-loop participant record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::feature) struct StoredLoop {
    face_item: PmDcReference,
    edge_items: Vec<PmDcReference>,
    peer: PmDcReference,
    value: u32,
}

/// The peer record of a face-loop participant: one face item, the edge items,
/// and the reference back to the participant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::feature) struct StoredPeer {
    items: Vec<PmDcReference>,
    participant: PmDcReference,
}

impl StoredLoop {
    /// The face item that the participant lists.
    pub(in crate::feature) fn face_item(&self) -> PmDcReference {
        self.face_item
    }
}

pub(in crate::feature) fn parse_loop(
    ctx: &DecodeContext<'_>,
    payload: View<'_>,
    version: u8,
) -> Result<StoredLoop, CodecError> {
    let mut cursor = Cursor::new(payload);
    content_header(&mut cursor, version)?;
    let face_item = cursor.reference("face-loop face item")?;
    let edge_items = reference_list_with_context(ctx, &mut cursor, 2, "face-loop edge items")?;
    let peer = cursor.reference("face-loop peer")?;
    let value = cursor.u32("face-loop value")?;
    cursor.finish("face loop")?;
    Ok(StoredLoop {
        face_item,
        edge_items: edge_items.references().to_vec(),
        peer,
        value,
    })
}

pub(in crate::feature) fn parse_peer(
    ctx: &DecodeContext<'_>,
    payload: View<'_>,
    version: u8,
) -> Result<StoredPeer, CodecError> {
    let mut cursor = Cursor::new(payload);
    state_content_header(&mut cursor, version)?;
    if cursor.i32("face-loop peer state")? != -1 {
        return Err(CodecError::malformed(
            "Inventor face-loop peer state is not -1",
        ));
    }
    let items = reference_list_with_context(ctx, &mut cursor, 2, "face-loop peer items")?;
    let participant = cursor.reference("face-loop peer participant")?;
    cursor.finish("face-loop peer")?;
    Ok(StoredPeer {
        items: items.references().to_vec(),
        participant,
    })
}

/// Why a face-loop section joins no input edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::feature) enum Refusal {
    /// The label lists no participant, or more than one, of the loop type, or
    /// the feature has no single face-loop section.
    Participant,
    /// The participant does not end with the loop value, or its peer record
    /// does not list the same face item and edge items in order and refer
    /// back to the participant.
    Peer,
    /// The face item or an edge item is not a single-selector item of the
    /// stored form, or two edge items name the same selector.
    Items,
    /// A selector joins no input revision, or an item and its selected
    /// revision disagree.
    Selector,
    /// Two edge items select the same input edge revision.
    Repeated,
    /// The selected face has no stored boundary, or an edge item does not have
    /// exactly one matched revision in the loops of the face.
    Face,
    /// The edges do not form one closed chain in the stored order.
    Chain,
}

/// An input edge of a face loop, in stored order.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::feature) struct LoopEdge {
    /// Source index of the edge selector.
    pub(in crate::feature) source: u32,
    /// Entity index of the input edge revision.
    pub(in crate::feature) owner: i64,
    /// First vertex of the chain traversal.
    pub(in crate::feature) from: FinitePoint3,
    /// Last vertex of the chain traversal.
    pub(in crate::feature) to: FinitePoint3,
}

/// The ordered input edges of one face loop.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::feature) struct FaceLoop {
    /// Entity index of the lowest selected face revision.
    pub(in crate::feature) face: i64,
    pub(in crate::feature) edges: Vec<LoopEdge>,
}

impl FaceLoop {
    /// Whether the stored edges still satisfy the join: at least one edge,
    /// distinct selectors and revisions, no edge that is the face, each edge
    /// starting at the vertex where the previous edge ends, and the last edge
    /// ending where the first starts.
    pub(in crate::feature) fn closed(&self) -> bool {
        let sources = self
            .edges
            .iter()
            .map(|edge| edge.source)
            .collect::<HashSet<_>>();
        let owners = self
            .edges
            .iter()
            .map(|edge| edge.owner)
            .collect::<HashSet<_>>();
        let (Some(first), Some(last)) = (self.edges.first(), self.edges.last()) else {
            return false;
        };
        sources.len() == self.edges.len()
            && owners.len() == self.edges.len()
            && !owners.contains(&self.face)
            && near(last.to, first.from)
            && self
                .edges
                .windows(2)
                .all(|pair| near(pair[0].to, pair[1].from))
    }
}

/// The item that a reference selects: the selector source indexes of a face
/// item or an edge item of the stored form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::feature) enum Item {
    Face { sources: Vec<u32> },
    Edge { sources: Vec<u32> },
}

/// The operands of one face-loop section.
pub(in crate::feature) struct Operands<'a> {
    /// The participant record, by record ordinal.
    pub(in crate::feature) participant: u32,
    pub(in crate::feature) stored: &'a StoredLoop,
    pub(in crate::feature) peer: Option<&'a StoredPeer>,
    /// The item a reference selects, with its record ordinal.
    pub(in crate::feature) item: &'a dyn Fn(PmDcReference) -> Option<Item>,
    /// The face revision joined to a face-selector source index.
    pub(in crate::feature) face: &'a dyn Fn(u32) -> Option<&'a SelectedFace>,
    /// The edge revision joined to an edge-selector source index.
    pub(in crate::feature) edge: &'a dyn Fn(u32) -> Option<&'a EdgeVertices>,
    /// The edge revisions in the loops of the face revisions that a
    /// face-selector source index joins.
    pub(in crate::feature) boundary: &'a dyn Fn(u32) -> Option<&'a [Revision]>,
}

/// Joins the loop's face item and edge items to input topology.
///
/// The loop is joined when the peer record repeats the items, every item
/// names one selector that joins an input revision, the edges are distinct and
/// lie on the selected face, and the edges in stored order form one closed
/// chain, each sharing a vertex with the next and the last with the first.
pub(in crate::feature) fn join(operands: &Operands<'_>) -> Result<FaceLoop, Refusal> {
    let stored = operands.stored;
    if stored.value != LOOP_VALUE {
        return Err(Refusal::Peer);
    }
    let peer = operands.peer.ok_or(Refusal::Peer)?;
    let mut listed = vec![stored.face_item];
    listed.extend(stored.edge_items.iter().copied());
    if stored.edge_items.is_empty()
        || peer.items != listed
        || peer.participant.record_ordinal() != Some(operands.participant)
    {
        return Err(Refusal::Peer);
    }
    let Some(Item::Face { sources }) = (operands.item)(stored.face_item) else {
        return Err(Refusal::Items);
    };
    let [face_source] = sources.as_slice() else {
        return Err(Refusal::Items);
    };
    let face = (operands.face)(*face_source).ok_or(Refusal::Selector)?;
    if Some(face.item) != stored.face_item.record_ordinal() {
        return Err(Refusal::Selector);
    }
    let mut edges = Vec::new();
    let mut named = HashSet::new();
    for reference in &stored.edge_items {
        let Some(Item::Edge { sources }) = (operands.item)(*reference) else {
            return Err(Refusal::Items);
        };
        let [source] = sources.as_slice() else {
            return Err(Refusal::Items);
        };
        if !named.insert(*source) {
            return Err(Refusal::Items);
        }
        let edge = (operands.edge)(*source).ok_or(Refusal::Selector)?;
        if Some(edge.item) != reference.record_ordinal() {
            return Err(Refusal::Selector);
        }
        edges.push((*source, edge));
    }
    let boundary = (operands.boundary)(*face_source).ok_or(Refusal::Face)?;
    let mut used = HashSet::new();
    let mut revisions = Vec::new();
    for (source, edge) in &edges {
        let found = edge
            .revisions
            .iter()
            .filter(|revision| {
                boundary
                    .iter()
                    .any(|candidate| candidate.owner == revision.owner)
            })
            .collect::<Vec<_>>();
        let [revision] = found.as_slice() else {
            return Err(Refusal::Face);
        };
        if !used.insert(revision.owner) {
            return Err(Refusal::Repeated);
        }
        revisions.push((*source, *revision));
    }
    let chain = chain(
        &revisions
            .iter()
            .map(|(_, revision)| (revision.start, revision.end))
            .collect::<Vec<_>>(),
    )
    .ok_or(Refusal::Chain)?;
    Ok(FaceLoop {
        face: face.owner,
        edges: revisions
            .into_iter()
            .zip(chain)
            .map(|((source, revision), (from, to))| LoopEdge {
                source,
                owner: revision.owner,
                from,
                to,
            })
            .collect(),
    })
}

fn near(a: FinitePoint3, b: FinitePoint3) -> bool {
    a.as_raw().distance(*b.as_raw()) <= EPS_LOOP_POINT_MM
}

/// The traversal of edges in stored order as a closed chain: each edge runs
/// from the vertex that ends the previous edge, and the last edge ends at the
/// first vertex. An edge whose two vertices both match the previous end, other
/// than a lone closed edge, has no traversal.
fn chain(ends: &[(FinitePoint3, FinitePoint3)]) -> Option<Vec<(FinitePoint3, FinitePoint3)>> {
    let &(first_start, first_end) = ends.first()?;
    if ends.len() == 1 {
        return near(first_start, first_end).then(|| vec![(first_start, first_end)]);
    }
    for (from, to) in [(first_start, first_end), (first_end, first_start)] {
        let mut walk = vec![(from, to)];
        let mut at = to;
        let mut complete = true;
        for &(start, end) in &ends[1..] {
            let next = match (near(at, start), near(at, end)) {
                (true, false) => (start, end),
                (false, true) => (end, start),
                _ => {
                    complete = false;
                    break;
                }
            };
            at = next.1;
            walk.push(next);
        }
        if complete && near(at, from) {
            return Some(walk);
        }
    }
    None
}

/// The joins of the face-loop sections of a surface loft, in section-list
/// order.
///
/// A section is a face-loop section when its `source_value` equals the source
/// index of its own content header. A loft without such a section has no
/// join.
pub(in crate::feature) fn sections<'a>(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &'a ProjectionIndex<'a>,
    inputs: &'a EdgeInventory,
) -> Vec<Result<FaceLoop, Refusal>> {
    let token = source.identity.segment_token.as_str();
    let property = |ordinal: u32| {
        index
            .properties
            .get(&(token, ordinal))
            .and_then(Option::as_ref)
            .copied()
    };
    let Some(slot) = source
        .properties
        .references()
        .first()
        .and_then(|reference| reference.record_ordinal())
        .and_then(property)
    else {
        return Vec::new();
    };
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::LoftSections,
        items,
    } = &slot.kind
    else {
        return Vec::new();
    };
    let loops = items
        .references()
        .iter()
        .filter_map(|reference| reference.record_ordinal())
        .filter(|ordinal| {
            property(*ordinal).is_some_and(|section| {
                matches!(
                    section.kind,
                    PmDcFeaturePropertyKind::LoftSection { source_value, .. }
                        if source_value == section.header.source_index
                )
            })
        })
        .collect::<Vec<_>>();
    let participants = label
        .participants
        .references()
        .iter()
        .filter_map(|reference| reference.record_ordinal())
        .filter(|ordinal| inputs.loops.contains_key(&(token.to_owned(), *ordinal)))
        .collect::<Vec<_>>();
    let item = |reference: PmDcReference| -> Option<Item> {
        item_at(reference.record_ordinal().and_then(property)?)
    };
    let face = |source: u32| inputs.faces.get(&(token.to_owned(), source));
    let edge = |source: u32| inputs.loop_edges.get(&(token.to_owned(), source));
    let boundary = |source: u32| {
        inputs
            .loop_faces
            .get(&(token.to_owned(), source))
            .map(Vec::as_slice)
    };
    loops
        .into_iter()
        .map(|_| {
            let joined = (|| {
                let [participant] = participants.as_slice() else {
                    return Err(Refusal::Participant);
                };
                let stored = inputs
                    .loops
                    .get(&(token.to_owned(), *participant))
                    .ok_or(Refusal::Participant)?;
                let peer = stored
                    .peer
                    .record_ordinal()
                    .and_then(|ordinal| inputs.peers.get(&(token.to_owned(), ordinal)));
                join(&Operands {
                    participant: *participant,
                    stored,
                    peer,
                    item: &item,
                    face: &face,
                    edge: &edge,
                    boundary: &boundary,
                })
            })();
            joined
        })
        .collect()
}

/// The selector sources of a stored-form face item or edge item.
fn item_at(property: &PmDcFeatureProperty) -> Option<Item> {
    match &property.kind {
        PmDcFeaturePropertyKind::FaceItem { index_references } => Some(Item::Face {
            sources: index_references.values().to_vec(),
        }),
        PmDcFeaturePropertyKind::EdgeItem {
            index_references,
            index_reference_value: -1,
            value: 0,
        } => Some(Item::Edge {
            sources: index_references.values().to_vec(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
