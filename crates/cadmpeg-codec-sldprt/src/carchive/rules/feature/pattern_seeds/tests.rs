// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::HashMap;
use std::sync::Arc;

fn read_result(bytes: &[u8]) -> (Res<()>, Vec<PatternSeeds>) {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::desktop()).unwrap();
    let known = HashMap::from([(1, Arc::from("suObList"))]);
    let mut reader = Reader::new(&ctx, bytes, 0, known, 10, Some(17000));
    reader.object_starts = HashMap::from([(3, 100), (4, 200)]);
    reader.sources = HashMap::from([(100, 30), (200, 40)]);
    let result = read(&mut reader, &V::map([("oid", V::Int(50))]));
    (result, reader.pattern_seeds)
}

fn read_list(bytes: &[u8]) -> Res<Vec<PatternSeeds>> {
    let (result, records) = read_result(bytes);
    result.map(|()| records)
}

#[test]
fn owned_list_resolves_every_short_object_pointer_in_order() {
    assert_eq!(
        read_list(&[1, 128, 2, 0, 4, 0, 3, 0]).unwrap(),
        vec![PatternSeeds {
            owner: 50,
            at: 0,
            seeds: Some(vec![Some(40), Some(30)])
        }]
    );
}

#[test]
fn extended_count_and_long_object_pointer_resolve() {
    let bytes = [1, 128, 255, 255, 1, 0, 0, 0, 255, 127, 3, 0, 0, 0];
    assert_eq!(read_list(&bytes).unwrap()[0].seeds, Some(vec![Some(30)]));
}

#[test]
fn unresolved_member_keeps_other_source_identifiers() {
    for pointer in [0, 5] {
        let bytes = [1, 128, 2, 0, 3, 0, pointer, 0];
        assert_eq!(
            read_list(&bytes).unwrap()[0].seeds,
            Some(vec![Some(30), None])
        );
    }
}

#[test]
fn truncated_count_stops_without_a_partial_seed_list() {
    assert!(read_list(&[1, 128, 2, 0, 3, 0]).is_err());
    assert!(read_list(&[1, 128, 255, 255, 255, 255, 255, 255]).is_err());
}

#[test]
fn truncated_list_keeps_its_owner_and_byte_position() {
    let (result, records) = read_result(&[1, 128, 2, 0, 3, 0]);
    assert!(result.is_err());
    assert_eq!(
        records,
        vec![PatternSeeds {
            owner: 50,
            at: 0,
            seeds: None
        }]
    );
}

#[test]
fn empty_list_and_referenced_list_have_distinct_readings() {
    assert_eq!(read_list(&[1, 128, 0, 0]).unwrap()[0].seeds, Some(vec![]));
    assert_eq!(read_list(&[3, 0]).unwrap()[0].seeds, None);
}
