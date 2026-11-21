// SPDX-License-Identifier: Apache-2.0
//! Bounded source lines at four persistent planar faces.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;
use std::collections::HashSet;

use crate::brep::attrib::NativeIdentity;
use crate::carchive::rules::idrep::SurfaceIdentity;

const EPS_INTERSECTION_DETERMINANT: f64 = 1.0e-10;
const EPS_PLANE_RESIDUAL_MM: f64 = 1.0e-7;
const EPS_SOURCE_EDGE_LENGTH_MM: f64 = 1.0e-9;
const EPS_BLEND_DIRECTION: f64 = 1.0e-8;
const EPS_BLEND_TANGENCY_MM: f64 = 1.0e-6;

#[derive(Clone, Copy)]
struct Plane {
    origin: Point3,
    normal: Vector3,
}

#[derive(Clone, Copy)]
pub(super) struct SourceLine {
    pub(super) start: FinitePoint3,
    pub(super) end: FinitePoint3,
    pub(super) interior: FinitePoint3,
}

fn intersection(planes: [Plane; 3]) -> Option<FinitePoint3> {
    let [a, b, c] = planes;
    let determinant = a.normal.dot(b.normal.cross(c.normal));
    if determinant.abs() <= EPS_INTERSECTION_DETERMINANT {
        return None;
    }
    let b_offset = b.normal.dot(b.origin.vector_from(a.origin));
    let c_offset = c.normal.dot(c.origin.vector_from(a.origin));
    let displacement = (c.normal.cross(a.normal).scale(b_offset)
        + a.normal.cross(b.normal).scale(c_offset))
    .scale(1.0 / determinant);
    let point = FinitePoint3::new(a.origin.translated(displacement, 1.0))?;
    planes
        .iter()
        .all(|plane| {
            plane
                .normal
                .dot(point.get().vector_from(plane.origin))
                .abs()
                <= EPS_PLANE_RESIDUAL_MM
        })
        .then_some(point)
}

fn bounded_line(planes: [Plane; 4]) -> Option<SourceLine> {
    let [a, b, start, end] = planes;
    let start = intersection([a, b, start])?;
    let end = intersection([a, b, end])?;
    if start.get().distance(end.get()) <= EPS_SOURCE_EDGE_LENGTH_MM {
        return None;
    }
    let (a, b) = (start.get(), end.get());
    let interior = FinitePoint3::new(Point3::new(
        a.x * 0.5 + b.x * 0.5,
        a.y * 0.5 + b.y * 0.5,
        a.z * 0.5 + b.z * 0.5,
    ))?;
    Some(SourceLine {
        start,
        end,
        interior,
    })
}

fn face_for<'a>(
    key: &SurfaceIdentity,
    persistent: &'a [NativeIdentity<FaceId>],
) -> Option<&'a FaceId> {
    let mut target = None;
    for identity in persistent
        .iter()
        .filter(|identity| identity.values.as_slice() == key.values())
    {
        if target.is_some_and(|previous| previous != &identity.target) {
            return None;
        }
        target = Some(&identity.target);
    }
    target
}

fn plane_for<'a>(ir: &'a CadIr, id: &FaceId) -> Option<(Plane, &'a BodyId)> {
    let face = ir.geometry.faces.iter().find(|face| &face.id == id)?;
    let surface = ir
        .geometry
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)?;
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = &surface.geometry else {
        return None;
    };
    let shell = ir
        .geometry
        .shells
        .iter()
        .find(|shell| shell.id == face.shell)?;
    let region = ir
        .geometry
        .regions
        .iter()
        .find(|region| region.id == shell.region)?;
    Some((
        Plane {
            origin: plane.origin().get(),
            normal: *plane.frame().axis().as_raw(),
        },
        &region.body,
    ))
}

fn face_edges<'a>(ir: &'a CadIr, face: &FaceId) -> HashSet<&'a EdgeId> {
    let loops = ir
        .geometry
        .loops
        .iter()
        .filter(|boundary| &boundary.face == face)
        .map(|boundary| &boundary.id)
        .collect::<HashSet<_>>();
    ir.geometry
        .coedges
        .iter()
        .filter(|coedge| loops.contains(&coedge.owner_loop))
        .map(|coedge| &coedge.edge)
        .collect()
}

fn carrier_matches(
    surface: &SurfaceGeometry,
    planes: [Plane; 2],
    sample: SourceLine,
    fillet: bool,
) -> bool {
    let Some(direction) = sample.end.get().vector_from(sample.start.get()).unit() else {
        return false;
    };
    match (fillet, surface) {
        (true, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder))) => {
            let axis = *cylinder.frame().axis().as_raw();
            direction.cross(axis).norm() <= EPS_BLEND_DIRECTION
                && planes.iter().all(|plane| {
                    let distance = plane
                        .normal
                        .dot(cylinder.origin().get().vector_from(plane.origin))
                        .abs();
                    (distance - cylinder.radius().get()).abs() <= EPS_BLEND_TANGENCY_MM
                })
        }
        (true, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))) => true,
        (false, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane))) => {
            plane.frame().axis().as_raw().dot(direction).abs() <= EPS_BLEND_DIRECTION
        }
        _ => false,
    }
}

fn blend_matches(
    ir: &CadIr,
    persistent: &[NativeIdentity<FaceId>],
    consumer: u32,
    supports: [&FaceId; 4],
    planes: [Plane; 2],
    sample: SourceLine,
    fillet: bool,
) -> bool {
    let boundaries = supports.map(|face| face_edges(ir, face));
    persistent
        .iter()
        .filter(|identity| {
            identity.values.first() == Some(&crate::carchive::rules::idrep::FILLET_SURFACE_CLASS)
                && identity.values.get(1) == Some(&consumer)
        })
        .any(|identity| {
            let edges = face_edges(ir, &identity.target);
            if boundaries
                .iter()
                .any(|boundary| edges.is_disjoint(boundary))
            {
                return false;
            }
            let surface = ir
                .geometry
                .faces
                .iter()
                .find(|face| face.id == identity.target)
                .and_then(|face| {
                    ir.geometry
                        .surfaces
                        .iter()
                        .find(|surface| surface.id == face.surface)
                });
            surface
                .is_some_and(|surface| carrier_matches(&surface.geometry, planes, sample, fillet))
        })
}

pub(super) fn source_line(
    ir: &CadIr,
    persistent: &[NativeIdentity<FaceId>],
    keys: &[SurfaceIdentity; 4],
    consumer: u32,
    fillet: bool,
) -> Option<SourceLine> {
    if keys.iter().any(|key| !key.same_document(&keys[0])) {
        return None;
    }
    let first = face_for(&keys[0], persistent)?;
    let second = face_for(&keys[1], persistent)?;
    let a = plane_for(ir, first)?;
    let b = plane_for(ir, second)?;
    let start = face_for(&keys[2], persistent)?;
    let end = face_for(&keys[3], persistent)?;
    let c = plane_for(ir, start)?;
    let d = plane_for(ir, end)?;
    if [b.1, c.1, d.1].iter().any(|body| *body != a.1) {
        return None;
    }
    let sample = bounded_line([a.0, b.0, c.0, d.0])?;
    blend_matches(
        ir,
        persistent,
        consumer,
        [first, second, start, end],
        [a.0, b.0],
        sample,
        fillet,
    )
    .then_some(sample)
}

#[cfg(test)]
mod tests {
    use super::{bounded_line, carrier_matches, Plane};
    use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    fn plane(origin: [f64; 3], normal: [f64; 3]) -> Plane {
        Plane {
            origin: Point3::from(origin),
            normal: Vector3::from(normal),
        }
    }

    #[test]
    fn ordered_end_faces_bound_the_source_line() {
        let source = bounded_line([
            plane([2.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            plane([0.0, 3.0, 0.0], [0.0, 1.0, 0.0]),
            plane([0.0, 0.0, 8.0], [0.0, 0.0, 1.0]),
            plane([0.0, 0.0, -4.0], [0.0, 0.0, -1.0]),
        ])
        .expect("bounded source edge");
        assert!(source.start.get().distance(Point3::new(2.0, 3.0, 8.0)) < 1.0e-12);
        assert!(source.end.get().distance(Point3::new(2.0, 3.0, -4.0)) < 1.0e-12);
        assert!(source.interior.get().distance(Point3::new(2.0, 3.0, 2.0)) < 1.0e-12);
    }

    #[test]
    fn final_blend_carrier_checks_the_source_supports() {
        let x = plane([0.0; 3], [1.0, 0.0, 0.0]);
        let y = plane([0.0; 3], [0.0, 1.0, 0.0]);
        let a = plane([0.0, 0.0, 4.0], [0.0, 0.0, 1.0]);
        let b = plane([0.0, 0.0, -2.0], [0.0, 0.0, 1.0]);
        let source = bounded_line([x, y, a, b]).expect("source line");
        let cylinder = |origin, axis| {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                CylinderSurface::try_new(origin, axis, Vector3::new(1.0, 0.0, 0.0), 2.0)
                    .expect("cylinder"),
            ))
        };
        let aligned = cylinder(Point3::new(2.0, 2.0, 0.0), Vector3::new(0.0, 0.0, 1.0));
        let offset = cylinder(Point3::new(2.0, 3.0, 0.0), Vector3::new(0.0, 0.0, 1.0));
        let transverse = cylinder(Point3::new(2.0, 2.0, 0.0), Vector3::new(0.0, 1.0, 0.0));
        assert!(carrier_matches(&aligned, [x, y], source, true));
        assert!(!carrier_matches(&offset, [x, y], source, true));
        assert!(!carrier_matches(&transverse, [x, y], source, true));
        let bevel = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(2.0, 0.0, 0.0),
                Vector3::new(1.0, 1.0, 0.0).unit().expect("normal"),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("bevel plane"),
        ));
        assert!(carrier_matches(&bevel, [x, y], source, false));
        assert!(!carrier_matches(&bevel, [x, y], source, true));
        assert!(!carrier_matches(&aligned, [x, y], source, false));
    }

    #[test]
    fn parallel_supports_and_unbounded_ends_keep_native_geometry() {
        let x = plane([0.0; 3], [1.0, 0.0, 0.0]);
        let y = plane([0.0; 3], [0.0, 1.0, 0.0]);
        let z = plane([0.0, 0.0, 1.0], [0.0, 0.0, 1.0]);
        assert!(bounded_line([x, x, z, z]).is_none());
        assert!(bounded_line([x, y, x, z]).is_none());
        assert!(bounded_line([x, y, z, z]).is_none());
    }
}
