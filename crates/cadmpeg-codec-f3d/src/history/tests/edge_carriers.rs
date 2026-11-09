// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::TAU;

use cadmpeg_ir::features::{FinitePoint3, HistoricalEdgeCarrier};
use cadmpeg_ir::geometry::analytic::{CircleCurve, EllipseCurve, LineCurve};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CurveId, EdgeId, IdentityKey, PointId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Edge, EdgeCarrier, Point, Vertex};
use cadmpeg_ir::units::FiniteVector;

#[test]
fn historical_edge_samples_retain_analytic_carriers_and_parameter_ranges() {
    let center = Point3::new(2.0, 3.0, 4.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let circle = CircleCurve::try_new(center, axis, reference, 5.0).unwrap();
    let line = LineCurve::try_new(center, reference).unwrap();
    let ellipse = EllipseCurve::try_new(center, axis, reference, 5.0, 2.0).unwrap();
    let spline = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![center, Point3::new(7.0, 3.0, 4.0)],
        None,
        false,
    )
    .unwrap();
    let cases = [
        (
            SolvedCurveGeometry::Circle(circle),
            [0.0, TAU],
            Some(HistoricalEdgeCarrier::Circle(circle)),
        ),
        (
            SolvedCurveGeometry::Line(line),
            [-2.0, 3.0],
            Some(HistoricalEdgeCarrier::Line(line)),
        ),
        (
            SolvedCurveGeometry::Ellipse(ellipse),
            [0.25, 1.75],
            Some(HistoricalEdgeCarrier::Ellipse(ellipse)),
        ),
        (SolvedCurveGeometry::Nurbs(spline), [0.0, 1.0], None),
    ];
    for (geometry, range, expected) in cases {
        let mut brep = cadmpeg_asm::brep::AsmBrep::default();
        let curve = CurveId::mint("test:brep:curve#10").unwrap();
        let vertex = VertexId::mint("test:brep:vertex#30").unwrap();
        brep.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(geometry),
            parameter_range: None,
            source_object: None,
        });
        if matches!(expected, Some(HistoricalEdgeCarrier::Circle(_))) {
            let point = PointId::mint("test:brep:point#40").unwrap();
            brep.points.push(Point::new(
                point.clone(),
                FinitePoint3::new(Point3::new(7.0, 3.0, 4.0)).unwrap(),
                None,
            ));
            brep.vertices.push(Vertex {
                id: vertex.clone(),
                point,
                tolerance: None,
            });
        }
        brep.edges.push(Edge {
            id: EdgeId::mint("test:brep:edge#20").unwrap(),
            carrier: EdgeCarrier::new(Some(curve), Some(range)).unwrap(),
            start: vertex.clone(),
            end: vertex,
            tolerance: None,
        });
        let topology = super::super::historical_topology(&brep).unwrap().unwrap();
        let sample = &topology.edge_samples[0];
        assert_eq!(topology.edge_samples.len(), 1);
        assert_eq!(sample.edge, 20);
        assert_eq!(sample.carrier, expected);
        assert_eq!(sample.parameter_range.map(FiniteVector::get), Some(range));
        if let Some(HistoricalEdgeCarrier::Circle(actual)) = sample.carrier {
            assert_eq!(actual.center().get(), center);
            assert_eq!(*actual.frame().axis().as_raw(), axis);
            assert_eq!(actual.radius().get(), 5.0);
            assert_eq!(range[1] - range[0], TAU);
        }
        let prefix = IdentityKey::try_new("test:input").unwrap();
        let projected = super::super::project_input_edge_geometry(&topology, &prefix);
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].carrier, expected);
        assert_eq!(projected[0].parameter_range, sample.parameter_range);
        if matches!(expected, Some(HistoricalEdgeCarrier::Circle(_))) {
            assert_eq!(
                projected[0].start,
                FinitePoint3::new(Point3::new(7.0, 3.0, 4.0))
            );
            assert_eq!(projected[0].end, projected[0].start);
        }
    }
}
