// SPDX-License-Identifier: Apache-2.0
//! Extrusion distances name the design parameter that sets them.

use super::*;

fn parameter_id(ordinal: u32) -> String {
    format!("inventor:design:parameter#{ordinal}")
}

fn bound(feature: &Feature, key: &str) -> Option<String> {
    feature.source_properties.get(key).cloned()
}

#[test]
fn one_sided_blind_distance_names_its_parameter() {
    let feature = extent_extrusion(1, false, &[], Vec::new()).expect("blind distance");
    assert_eq!(
        bound(&feature, crate::feature::EXTENT_DISTANCE_PARAMETER),
        Some(parameter_id(70))
    );
    assert_eq!(
        bound(&feature, crate::feature::EXTENT_SECOND_DISTANCE_PARAMETER),
        None
    );
}

#[test]
fn two_sided_blind_distances_name_one_parameter_each() {
    let feature = extent_extrusion(
        1,
        false,
        &[(27, 72), (28, 71), (29, 22)],
        vec![enum_property(22, PmDcFeatureEnumFamily::Extent, 1)],
    )
    .expect("two-sided");
    assert_eq!(
        bound(&feature, crate::feature::EXTENT_DISTANCE_PARAMETER),
        Some(parameter_id(70))
    );
    assert_eq!(
        bound(&feature, crate::feature::EXTENT_SECOND_DISTANCE_PARAMETER),
        Some(parameter_id(72))
    );
}

#[test]
fn symmetric_and_through_extents_name_no_parameter() {
    let symmetric = extent_extrusion(2, true, &[], Vec::new()).expect("symmetric");
    assert_eq!(
        bound(&symmetric, crate::feature::EXTENT_DISTANCE_PARAMETER),
        None
    );
    let through = extent_extrusion(5, false, &[], Vec::new()).expect("through all");
    assert_eq!(
        bound(&through, crate::feature::EXTENT_DISTANCE_PARAMETER),
        None
    );
}
