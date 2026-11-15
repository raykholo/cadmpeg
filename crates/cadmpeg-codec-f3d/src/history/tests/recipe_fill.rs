// SPDX-License-Identifier: Apache-2.0
use std::collections::HashSet;

use crate::history::face_recipe::{
    negative_lane_face_replaces_selection, recipe_face_replaces_selection,
};

#[test]
fn a_recipe_without_a_closed_edge_node_replaces_any_selection() {
    assert!(recipe_face_replaces_selection(false, false, false));
    assert!(recipe_face_replaces_selection(false, false, true));
}

#[test]
fn a_closed_edge_recipe_face_fills_an_empty_selection_only() {
    assert!(recipe_face_replaces_selection(true, true, false));
    assert!(!recipe_face_replaces_selection(true, false, false));
}

#[test]
fn a_closed_edge_recipe_face_the_transition_left_unchanged_does_not_start_or_profile_an_extrude() {
    assert!(!recipe_face_replaces_selection(true, true, true));
}

#[test]
fn a_negative_lane_face_fills_an_empty_selection_outside_extrude_start_and_profile() {
    let changed = HashSet::from([2]);
    assert!(negative_lane_face_replaces_selection(
        &[],
        false,
        &changed,
        1
    ));
    assert!(!negative_lane_face_replaces_selection(
        &[3],
        false,
        &changed,
        2
    ));
}

#[test]
fn a_changed_negative_lane_face_replaces_an_unchanged_extrude_start_or_profile_selection() {
    let changed = HashSet::from([2, 3]);
    assert!(negative_lane_face_replaces_selection(
        &[1],
        true,
        &changed,
        2
    ));
    assert!(negative_lane_face_replaces_selection(
        &[],
        true,
        &changed,
        2
    ));
    // A changed selection stays, and an unchanged recipe face is no Extrude
    // start or profile face.
    assert!(!negative_lane_face_replaces_selection(
        &[3],
        true,
        &changed,
        2
    ));
    assert!(!negative_lane_face_replaces_selection(
        &[1],
        true,
        &changed,
        4
    ));
}
