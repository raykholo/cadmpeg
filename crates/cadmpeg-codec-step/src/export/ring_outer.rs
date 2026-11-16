// SPDX-License-Identifier: Apache-2.0
//! The outer bound of an unclassified cone band.
//!
//! A source that does not classify a face's loops leaves every bound a plain
//! `FACE_BOUND`. A band on a cone between two coaxial full circles then has no
//! bound that a reader can take as the outer boundary. The ring of larger
//! radius is the outer bound of such a band.

use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::ids::LoopId;
use cadmpeg_ir::topology::{Face, FaceLoops, LoopBoundary};

use super::Builder;

/// Relative tolerance for a ring axis along the cone axis and a ring centre on it.
const EPS_RING_COAXIAL: f64 = 1.0e-9;

impl Builder<'_> {
    /// The loop to write as `FACE_OUTER_BOUND` on an unclassified face of two or
    /// more loops that are each one full circle about the axis of the cone
    /// carrier, when one circle has the largest radius.
    pub(super) fn unclassified_ring_outer(&self, face: &Face) -> Option<LoopId> {
        let FaceLoops::Unspecified { loops } = &face.loops else {
            return None;
        };
        if loops.len() < 2 {
            return None;
        }
        let surface = self.surfaces.get(face.surface.as_str())?;
        let Some(SolvedSurfaceGeometry::Cone(cone)) = surface.geometry.solved() else {
            return None;
        };
        let apex = cone.origin().get();
        let axis = *cone.frame().axis().as_raw();
        let mut rings = Vec::with_capacity(loops.len());
        for loop_id in loops {
            let lp = self.loops.get(loop_id.as_str())?;
            let LoopBoundary::Ring(ring) = &lp.boundary else {
                return None;
            };
            let [coedge] = ring.coedges() else {
                return None;
            };
            let edge = self.edges.get(self.coedges.get(coedge.as_str())?.edge.as_str())?;
            let curve = self.curves.get(edge.curve()?.as_str())?;
            let Some(SolvedCurveGeometry::Circle(circle)) = curve.geometry.solved() else {
                return None;
            };
            let direction = *circle.frame().axis().as_raw();
            let offset = circle.center().get().vector_from(apex);
            let radius = circle.radius().get();
            let scale = radius.max(1.0);
            if (direction.dot(axis).abs() - 1.0).abs() > EPS_RING_COAXIAL
                || offset.cross(axis).norm() > EPS_RING_COAXIAL * scale
            {
                return None;
            }
            rings.push((loop_id, radius));
        }
        let (outer, largest) = rings
            .iter()
            .copied()
            .max_by(|left, right| left.1.total_cmp(&right.1))?;
        let unique = rings.iter().filter(|ring| ring.1 >= largest).count() == 1;
        unique.then(|| outer.clone())
    }
}

#[cfg(test)]
mod tests;
