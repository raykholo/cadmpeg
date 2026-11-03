// SPDX-License-Identifier: Apache-2.0
//! Indexed lookup order and duplicate handling.

use super::*;

#[test]
fn design_path_index_keeps_the_first_duplicate_design() {
    let text = redirections_json("root.f3d", &[("part.f3d", "role")]);
    let mut table = super::super::parse(
        &cadmpeg_test_support::service_decode_context(),
        text.as_bytes(),
    )
    .unwrap();
    let mut duplicate = table.designs[1].clone();
    duplicate.display_name = "later design".into();
    table.designs.push(duplicate);
    let designs = table.designs_by_path();
    assert_eq!(designs["part.f3d"].display_name, "component0");
    assert!(!designs.contains_key("absent.f3d"));
}

#[test]
fn placement_role_index_keeps_source_order_and_repeated_names_once() {
    let placements = [
        OccurrencePlacement {
            frame_bytes: Vec::new(),
            occurrence_guid: None,
            carrier_record_index: 8,
            owner: None,
            link_names: vec!["role".into(), "other".into(), "role".into()],
            transform: None,
        },
        OccurrencePlacement {
            frame_bytes: Vec::new(),
            occurrence_guid: None,
            carrier_record_index: 3,
            owner: None,
            link_names: vec!["role".into()],
            transform: None,
        },
    ];
    let roles = super::super::placement_indices_by_role(&placements);
    let selected = super::super::occurrence_transforms_with_precedence(
        Vec::new(),
        roles["role"].iter().map(|&index| &placements[index]),
        "role",
    );
    assert_eq!(
        selected
            .iter()
            .map(|placement| placement.carrier_record_index)
            .collect::<Vec<_>>(),
        [8, 3]
    );
    assert_eq!(roles["other"], [0]);
}
