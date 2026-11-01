// SPDX-License-Identifier: Apache-2.0

use crate::brep::topology::{scan_plain, topology_header_prefix_end, topology_scopes};
use crate::test_support::parasolid::{framed_topology_fin, wide};
use cadmpeg_core::decode::View;

#[test]
fn analytic_node_ids_keep_tag_valued_words_inside_the_carrier() {
    for key in [7, 0x1_0000] {
        for node_id in [0x0052_0000, 0x0051_0000, 0x0010_0000] {
            let body = wide::line(key, node_id, [0.0; 3]);
            assert_eq!(topology_header_prefix_end(&body, 0, 0x1e), Some(body.len()));
            assert_eq!(
                topology_scopes(&body, false).excluded_starts,
                vec![1..body.len()]
            );
        }
    }
}

#[test]
fn a_fin_valued_coordinate_payload_stays_inside_its_line() {
    let fin = framed_topology_fin(19, [1; 9], false, false, false);
    let mut coordinates = [0; 24];
    coordinates[..fin.len()].copy_from_slice(&fin);
    let origin = [0, 8, 16].map(|at| View::f64_be_at(&coordinates, at).unwrap());
    assert!(origin.into_iter().all(f64::is_finite));
    let body = wide::line(7, 17, origin);
    assert_eq!(topology_header_prefix_end(&body, 0, 0x1e), Some(body.len()));
    assert!(scan_plain(&body).coedges().is_empty());
}

#[test]
fn bounded_curve_node_ids_keep_tag_valued_words_inside_the_wrapper() {
    let body = wide::bounded_curve(
        0x0052_0000,
        0x1_0000,
        &[0.0, 0.0, 0.0, 0.0, 0.005, 0.0, 0.0, 0.005],
    );
    assert_eq!(topology_header_prefix_end(&body, 0, 0x85), Some(body.len()));
    assert_eq!(
        topology_scopes(&body, false).excluded_starts,
        vec![1..body.len()]
    );
}

#[test]
fn surface_curve_node_ids_keep_tag_valued_words_inside_the_record() {
    let body = wide::sp_curve(0x0052_0000, 0x1_0000, 0x1_0001, false);
    assert_eq!(topology_header_prefix_end(&body, 0, 0x89), Some(body.len()));
    assert_eq!(
        topology_scopes(&body, false).excluded_starts,
        vec![1..body.len()]
    );
}

#[test]
fn spline_wrapper_ids_keep_tag_valued_words_inside_the_header() {
    for tag in [0x7c, 0x86] {
        let body = wide::curve_wrapper(tag, 0x1_0000, 0x0052_0000, 0x1_0001);
        assert_eq!(topology_header_prefix_end(&body, 0, tag), Some(body.len()));
        assert_eq!(
            topology_scopes(&body, false).excluded_starts,
            vec![1..body.len()]
        );
    }
}

#[test]
fn a_point_valued_control_array_stays_inside_its_spline_storage() {
    let point = crate::test_support::parasolid::world_point(211, [1.0, 2.0, 3.0]);
    let mut payload = [0; 48];
    payload[..point.len()].copy_from_slice(&point);
    let values = (0..6)
        .map(|index| View::f64_be_at(&payload, index * 8).unwrap())
        .collect::<Vec<_>>();
    assert!(values.iter().all(|value| value.is_finite()));
    let descriptor = wide::curve_descriptor(0x1_0001, [0x1_0000, 0x1_0002, 0x1_0003]);
    assert_eq!(
        topology_header_prefix_end(&descriptor, 0, 0x88),
        Some(descriptor.len())
    );
    let array = wide::float_values(0x1_0000, &values);
    let body = [descriptor, array].concat();
    assert!(scan_plain(&body).points().is_empty());
}
