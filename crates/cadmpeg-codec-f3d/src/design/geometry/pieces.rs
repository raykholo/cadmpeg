// SPDX-License-Identifier: Apache-2.0
//! Exact join of a stored piece loop to a face of the sketch arrangement.
//!
//! A typed profile selection lists, for each loop of a region, one entry per
//! curve piece: a traversal flag, the ordinal of the piece along its curve,
//! and the number of pieces the curve has. The pieces of a curve are the runs
//! between nodes at which another curve of the same region meets it. A loop
//! joins a face when the directed pieces of the loop and of the face boundary
//! are the same multiset. Each directed piece belongs to exactly one face, so
//! the join does not depend on the listed entry order.

use super::{
    all_curve_arrangement, point_in_polygon, ProfileGeometryWork, SketchArrangement,
    SketchArrangementFace, SketchArrangementPiece, EPS_GEOMETRY_ARC_ANGULAR_TOLERANCE_E9,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{SketchProfileBoundaryUse, SketchProfileRegion};
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityId, SketchGeometryDefinition};
use std::collections::{HashMap, HashSet};

/// One stored loop entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::design) struct StoredPiece {
    pub(in crate::design) curve: SketchEntityId,
    /// One-based ordinal of the piece along the curve.
    pub(in crate::design) piece: u32,
    /// Number of pieces of the curve.
    pub(in crate::design) pieces: u32,
    /// Whether the loop traverses the piece against the curve's stored
    /// direction.
    pub(in crate::design) reversed: bool,
}

/// One stored loop of a region.
pub(in crate::design) struct StoredLoop {
    pub(in crate::design) outer: bool,
    pub(in crate::design) pieces: Vec<StoredPiece>,
}

/// The arrangement of every non-construction curve of one sketch, read
/// through stored piece loops.
pub(in crate::design) struct PieceArrangement<'a> {
    arrangement: SketchArrangement,
    entities: &'a [SketchEntity],
    piece_index: HashMap<(&'a SketchEntityId, [u64; 2]), usize>,
}

impl<'a> PieceArrangement<'a> {
    pub(in crate::design) fn new(
        sketch: &Sketch,
        entities: &'a [SketchEntity],
        tolerance: f64,
        work: &ProfileGeometryWork,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<Self>, CodecError> {
        let Some(arrangement) = all_curve_arrangement(sketch, entities, tolerance, work, ctx)?
        else {
            return Ok(None);
        };
        let mut piece_index = HashMap::new();
        for (index, piece) in arrangement.pieces.iter().enumerate() {
            let Some(entity) = entities.iter().find(|entity| entity.id() == &piece.entity) else {
                return Ok(None);
            };
            let key = [piece.range[0].to_bits(), piece.range[1].to_bits()];
            if piece_index.insert((entity.id(), key), index).is_some() {
                return Ok(None);
            }
        }
        Ok(Some(Self {
            arrangement,
            entities,
            piece_index,
        }))
    }

    /// The region whose exterior and holes the loops are, or `None` when a
    /// loop is not exactly the directed pieces of one face, or the holes are
    /// not exactly the islands directly inside the exterior face.
    pub(in crate::design) fn region(&self, loops: &[StoredLoop]) -> Option<SketchProfileRegion> {
        let curves = loops
            .iter()
            .flat_map(|stored| stored.pieces.iter().map(|piece| &piece.curve))
            .collect::<HashSet<_>>();
        let mut layouts = HashMap::new();
        for curve in &curves {
            layouts.insert(*curve, self.curve_layout(curve, &curves)?);
        }
        let faces = &self.arrangement.faces;
        let face_runs = faces
            .iter()
            .map(|face| self.face_runs(face, &layouts))
            .collect::<Vec<_>>();
        let mut exterior = None;
        let mut holes = Vec::new();
        for stored in loops {
            let mut wanted = stored.pieces.clone();
            wanted.sort_unstable();
            let mut matching = face_runs.iter().enumerate().filter_map(|(index, runs)| {
                let mut runs = runs.clone()?;
                if !stored.outer {
                    for run in &mut runs {
                        run.reversed = !run.reversed;
                    }
                    runs.sort_unstable();
                }
                (runs == wanted).then_some(index)
            });
            let index = matching.next()?;
            if matching.next().is_some() {
                return None;
            }
            if stored.outer {
                if exterior.replace(index).is_some() {
                    return None;
                }
            } else {
                if holes.contains(&index) {
                    return None;
                }
                holes.push(index);
            }
        }
        let exterior_index = exterior?;
        if holes.contains(&exterior_index) {
            return None;
        }
        let exterior = &faces[exterior_index];
        // A hole ring is the outer walk of its connected part, which is the
        // reversed boundary of the only bounded face the part has.
        let mut hole_parts = HashSet::new();
        for index in &holes {
            let part = faces[*index].component;
            if faces.iter().filter(|face| face.component == part).count() != 1
                || !hole_parts.insert(part)
            {
                return None;
            }
        }
        if direct_islands(exterior, faces) != hole_parts {
            return None;
        }
        let rings = holes
            .iter()
            .map(|index| reversed_ring(&faces[*index].boundary))
            .collect();
        SketchProfileRegion::trimmed(exterior.boundary.clone(), rings).ok()
    }

    /// The pieces of `curve` grouped between the nodes at which another curve
    /// of `curves` meets it, as `(piece index, ordinal, count)`. `None` when
    /// the pieces of the curve do not chain.
    fn curve_layout(
        &self,
        curve: &SketchEntityId,
        curves: &HashSet<&SketchEntityId>,
    ) -> Option<HashMap<usize, (u32, u32)>> {
        let pieces = &self.arrangement.pieces;
        if pieces.iter().any(|piece| piece.coincident.contains(curve)) {
            return None;
        }
        let closed = self.entities.iter().any(|entity| {
            entity.id() == curve
                && matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Circle { .. }
                )
        });
        let mut own = pieces
            .iter()
            .enumerate()
            .filter(|(_, piece)| &piece.entity == curve)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if own.is_empty() {
            return None;
        }
        // A closed curve numbers the piece across its seam first.
        let key = |index: usize| {
            let range = pieces[index].range;
            if closed && range[1] > std::f64::consts::TAU + EPS_GEOMETRY_ARC_ANGULAR_TOLERANCE_E9 {
                range[0] - std::f64::consts::TAU
            } else {
                range[0]
            }
        };
        own.sort_by(|left, right| key(*left).total_cmp(&key(*right)));
        let count = own.len();
        let joints = if closed { count } else { count - 1 };
        let mut split = Vec::with_capacity(joints);
        for joint in 0..joints {
            let before = &pieces[own[joint]];
            let after = &pieces[own[(joint + 1) % count]];
            if before.nodes[1] != after.nodes[0] {
                return None;
            }
            let node = before.nodes[1];
            split.push(
                pieces
                    .iter()
                    .any(|other| other.nodes.contains(&node) && joins_curves(other, curve, curves)),
            );
        }
        // A piece whose edge merged another selected curve ends at nodes of
        // that curve.
        let overlapped = |index: usize| {
            pieces[index]
                .coincident
                .iter()
                .any(|other| curves.contains(other))
        };
        for (position, index) in own.iter().enumerate() {
            if overlapped(*index) {
                if position > 0 {
                    split[position - 1] = true;
                } else if closed {
                    split[count - 1] = true;
                }
                if position < joints {
                    split[position] = true;
                }
            }
        }
        let mut layout = HashMap::new();
        if closed {
            let splits = split.iter().filter(|joint| **joint).count();
            if splits <= 1 {
                for index in &own {
                    layout.insert(*index, (1, 1));
                }
                return Some(layout);
            }
            let mut start = 0;
            while !split[(start + count - 1) % count] {
                start = (start + count - 1) % count;
            }
            let mut ordinal = 1;
            for offset in 0..count {
                let position = (start + offset) % count;
                if offset > 0 && split[(position + count - 1) % count] {
                    ordinal += 1;
                }
                layout.insert(own[position], (ordinal, u32::try_from(splits).ok()?));
            }
        } else {
            let total = u32::try_from(split.iter().filter(|joint| **joint).count() + 1).ok()?;
            let mut ordinal = 1;
            for (position, index) in own.iter().enumerate() {
                layout.insert(*index, (ordinal, total));
                if position < joints && split[position] {
                    ordinal += 1;
                }
            }
        }
        Some(layout)
    }

    /// The directed pieces of a face boundary as sorted stored entries, with
    /// consecutive boundary uses of one piece merged. `None` when a use
    /// belongs to a curve outside the region.
    fn face_runs(
        &self,
        face: &SketchArrangementFace,
        layouts: &HashMap<&SketchEntityId, HashMap<usize, (u32, u32)>>,
    ) -> Option<Vec<StoredPiece>> {
        let mut runs = Vec::<StoredPiece>::new();
        for use_ in &face.boundary {
            let [first, second] = use_.parameter_range.endpoints();
            let range = [first.min(second), first.max(second)];
            let key = [range[0].to_bits(), range[1].to_bits()];
            let index = *self.piece_index.get(&(&use_.entity, key))?;
            let (piece, pieces) = *layouts.get(&use_.entity)?.get(&index)?;
            let run = StoredPiece {
                curve: use_.entity.clone(),
                piece,
                pieces,
                reversed: use_.reversed,
            };
            if runs.last() != Some(&run) {
                runs.push(run);
            }
        }
        if runs.len() > 1 && runs.first() == runs.last() {
            runs.pop();
        }
        runs.sort_unstable();
        Some(runs)
    }
}

/// Whether `other` carries a curve of `curves` other than `curve`.
fn joins_curves(
    other: &SketchArrangementPiece,
    curve: &SketchEntityId,
    curves: &HashSet<&SketchEntityId>,
) -> bool {
    (&other.entity != curve && curves.contains(&other.entity))
        || other
            .coincident
            .iter()
            .any(|entity| entity != curve && curves.contains(entity))
}

/// Roots of the connected parts directly inside `face`: the parts that no
/// other part inside the face encloses.
fn direct_islands(face: &SketchArrangementFace, faces: &[SketchArrangementFace]) -> HashSet<usize> {
    face.islands
        .iter()
        .filter(|(root, point)| {
            !face.islands.iter().any(|(other, _)| {
                other != root
                    && faces.iter().any(|inner| {
                        inner.component == *other && point_in_polygon(*point, &inner.polyline)
                    })
            })
        })
        .map(|(root, _)| *root)
        .collect()
}

/// The ring that walks the same boundary the other way.
fn reversed_ring(boundary: &[SketchProfileBoundaryUse]) -> Vec<SketchProfileBoundaryUse> {
    boundary
        .iter()
        .rev()
        .map(|use_| SketchProfileBoundaryUse {
            entity: use_.entity.clone(),
            parameter_range: use_.parameter_range,
            reversed: !use_.reversed,
        })
        .collect()
}
