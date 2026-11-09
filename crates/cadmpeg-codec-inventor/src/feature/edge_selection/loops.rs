// SPDX-License-Identifier: Apache-2.0
//! Vertices of the input edge and face revisions that a face loop selects.

use std::collections::{HashMap, HashSet};

use cadmpeg_asm::sab::Record;
use cadmpeg_core::decode::unique_index;
use cadmpeg_ir::features::FinitePoint3;

use super::{geometry::endpoints, is_edge, matched};

/// Record slot of the first loop of a face.
const FACE_LOOP: usize = 4;
/// Record slot of the next loop of a face.
const LOOP_NEXT: usize = 3;
/// Record slot of the first coedge of a loop.
const LOOP_COEDGE: usize = 4;
/// Record slot of the next coedge of a loop.
const COEDGE_NEXT: usize = 3;
/// Record slot of the edge of a coedge.
const COEDGE_EDGE: usize = 6;

/// A stored revision of an input edge with its vertices.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::feature) struct Revision {
    /// Record index of the edge revision.
    pub(in crate::feature) owner: i64,
    pub(in crate::feature) start: FinitePoint3,
    pub(in crate::feature) end: FinitePoint3,
}

/// The input edge that an edge selector selects: the matched revisions that
/// are edges with stored vertices, in record order.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::feature) struct EdgeVertices {
    /// Record ordinal of the edge item that lists the selector.
    pub(in crate::feature) item: u32,
    pub(in crate::feature) revisions: Vec<Revision>,
}

fn index(records: &[Record]) -> HashMap<i64, &Record> {
    unique_index(
        records
            .iter()
            .filter_map(|record| Some((i64::try_from(record.index).ok()?, record))),
    )
    .into_iter()
    .filter_map(|(index, record)| Some((index, record?)))
    .collect()
}

/// An edge selector: its source index, its key, and the record ordinal of the
/// edge item that lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Selector {
    pub(super) source: u32,
    pub(super) key: u32,
    pub(super) item: u32,
}

/// The matched edge revisions of every edge selector, by selector source
/// index. A selector with no matched owner, or an owner that is not an edge
/// with stored vertices, selects nothing. The vertices do not depend on the
/// carrier curve.
pub(super) fn edges(
    records: &[Record],
    selectors: &[Selector],
    schema: u32,
) -> HashMap<u32, EdgeVertices> {
    let by_index = index(records);
    let mut matches = HashMap::<(u32, u32), HashSet<i64>>::new();
    for record in records {
        if let Some((source, key, owner)) = matched(record, schema) {
            matches.entry((source, key)).or_default().insert(owner);
        }
    }
    let selectors = unique_index(selectors.iter().map(|selector| (selector.source, selector)));
    let mut found = Vec::new();
    for selector in selectors.values().flatten() {
        let Some(owners) = matches.get(&(selector.source, selector.key)) else {
            continue;
        };
        let mut ordered = owners.iter().copied().collect::<Vec<_>>();
        ordered.sort_unstable();
        let revisions = ordered
            .iter()
            .map(|owner| {
                let edge = by_index.get(owner).filter(|edge| is_edge(edge))?;
                let (start, end) = endpoints(edge, &by_index)?;
                Some(Revision {
                    owner: *owner,
                    start,
                    end,
                })
            })
            .collect::<Option<Vec<_>>>();
        let Some(revisions) = revisions else {
            continue;
        };
        found.push((
            selector.source,
            EdgeVertices {
                item: selector.item,
                revisions,
            },
        ));
    }
    unique_index(found)
        .into_iter()
        .filter_map(|(source, edge)| Some((source, edge?)))
        .collect()
}

/// The edge revisions in the loops of the face revisions `owners`, without
/// repeats of one edge record. A face revision that is not a face, a
/// loop or coedge chain that does not end, or an edge without stored vertices
/// gives `None`.
pub(super) fn boundary(records: &[Record], owners: &[i64]) -> Option<Vec<Revision>> {
    let by_index = index(records);
    let mut edges = HashSet::new();
    let mut pairs = Vec::new();
    for owner in owners {
        let face = by_index.get(owner)?;
        let mut loops = HashSet::new();
        let mut next_loop = face.ref_at(FACE_LOOP);
        while let Some(loop_index) = next_loop {
            if !loops.insert(loop_index) {
                break;
            }
            let loop_record = by_index.get(&loop_index)?;
            let mut coedges = HashSet::new();
            let mut next_coedge = loop_record.ref_at(LOOP_COEDGE);
            while let Some(coedge_index) = next_coedge {
                if !coedges.insert(coedge_index) {
                    break;
                }
                let coedge = by_index.get(&coedge_index)?;
                let edge_index = coedge.ref_at(COEDGE_EDGE)?;
                if edges.insert(edge_index) {
                    let edge = by_index.get(&edge_index).filter(|edge| is_edge(edge))?;
                    let (start, end) = endpoints(edge, &by_index)?;
                    pairs.push(Revision {
                        owner: edge_index,
                        start,
                        end,
                    });
                }
                next_coedge = coedge.ref_at(COEDGE_NEXT);
            }
            next_loop = loop_record.ref_at(LOOP_NEXT);
        }
    }
    Some(pairs)
}

#[cfg(test)]
mod tests;
