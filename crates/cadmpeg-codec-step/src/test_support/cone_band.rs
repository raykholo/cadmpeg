// SPDX-License-Identifier: Apache-2.0
//! A sheet that is one band of a cone between two full circles, whose two
//! loops carry no outer or inner classification.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{CircleCurve, ConeSurface};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing,
    Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;

/// Radii of the two rings, in source order: the inner ring first.
pub(crate) const RING_RADII: [f64; 2] = [2.0, 3.0];

fn id<T>(
    mint: fn(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
    kind: &str,
    key: &str,
) -> T {
    mint(format!("test:model:{kind}#{key}")).expect("identity grammar")
}

/// A cone band about the z axis between full circles at heights `1` and `2`.
/// The cone has radius `1` at the origin and half-angle `pi / 4`, so the
/// rings have radii `2` and `3`.
pub(crate) fn unclassified_cone_band() -> CadIr {
    let mut ir = CadIr::empty();
    let cone = SolvedSurfaceGeometry::Cone(
        ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("cone"),
    );
    ir.model.surfaces.push(Surface {
        id: id(SurfaceId::mint, "surface", "band"),
        geometry: SurfaceGeometry::Solved(cone),
        source_object: None,
    });
    let mut loops = Vec::new();
    for (index, radius) in RING_RADII.into_iter().enumerate() {
        let key = format!("ring-{index}");
        let height = radius - 1.0;
        let start = Point3::new(radius, 0.0, height);
        ir.model.points.push(Point::new(
            id(PointId::mint, "point", &key),
            FinitePoint3::new(start).expect("finite position"),
            None,
        ));
        ir.model.vertices.push(Vertex {
            id: id(VertexId::mint, "vertex", &key),
            point: id(PointId::mint, "point", &key),
            tolerance: None,
        });
        ir.model.curves.push(Curve {
            parameter_range: None,
            id: id(CurveId::mint, "curve", &key),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(0.0, 0.0, height),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    radius,
                )
                .expect("circle"),
            )),
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: id(EdgeId::mint, "edge", &key),
            carrier: EdgeCarrier::new(
                Some(id(CurveId::mint, "curve", &key)),
                Some([0.0, std::f64::consts::TAU]),
            )
            .expect("closed circle"),
            start: id(VertexId::mint, "vertex", &key),
            end: id(VertexId::mint, "vertex", &key),
            tolerance: None,
        });
        ir.model.coedges.push(Coedge {
            id: id(CoedgeId::mint, "coedge", &key),
            owner_loop: id(LoopId::mint, "loop", &key),
            edge: id(EdgeId::mint, "edge", &key),
            radial_next: id(CoedgeId::mint, "coedge", &key),
            sense: if index == 0 {
                Sense::Reversed
            } else {
                Sense::Forward
            },
            pcurves: Vec::new(),
            use_curve: None,
        });
        ir.model.loops.push(Loop {
            id: id(LoopId::mint, "loop", &key),
            face: id(FaceId::mint, "face", "band"),
            boundary: LoopBoundary::Ring(
                LoopRing::new(vec![id(CoedgeId::mint, "coedge", &key)], Vec::new())
                    .expect("non-empty ring"),
            ),
        });
        loops.push(id(LoopId::mint, "loop", &key));
    }
    ir.model.faces.push(Face {
        id: id(FaceId::mint, "face", "band"),
        shell: id(ShellId::mint, "shell", "band"),
        surface: id(SurfaceId::mint, "surface", "band"),
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(loops),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(
        Shell::new(
            id(ShellId::mint, "shell", "band"),
            id(RegionId::mint, "region", "band"),
            vec![id(FaceId::mint, "face", "band")],
            Vec::new(),
            Vec::new(),
        )
        .expect("shell with a face"),
    );
    ir.model.regions.push(Region {
        id: id(RegionId::mint, "region", "band"),
        body: id(BodyId::mint, "body", "band"),
        shells: vec![id(ShellId::mint, "shell", "band")],
    });
    ir.model.bodies.push(Body {
        id: id(BodyId::mint, "body", "band"),
        kind: BodyKind::Sheet,
        regions: vec![id(RegionId::mint, "region", "band")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.finalize();
    ir
}
