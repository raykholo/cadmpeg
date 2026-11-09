// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_asm::sab::Token;
use cadmpeg_ir::math::Point3;

const SCHEMA: u32 = 44;

fn record(index: usize, name: &str, tokens: Vec<Token>) -> Record {
    Record {
        index,
        name: name.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    }
}

fn refs(index: usize, name: &str, len: usize, set: &[(usize, i64)]) -> Record {
    let mut tokens = vec![Token::Ref(-1); len];
    for (slot, value) in set {
        tokens[*slot] = Token::Ref(*value);
    }
    record(index, name, tokens)
}

/// A vertex record at `index` that refers to the point record `index + 1`.
fn vertex(index: usize) -> Record {
    refs(
        index,
        "vertex",
        6,
        &[(5, i64::try_from(index + 1).expect("index"))],
    )
}

fn point(index: usize, at: [f64; 3]) -> Record {
    record(
        index,
        "point",
        vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Position(at),
        ],
    )
}

/// An edge at `index` between the vertices `start` and `end`, in
/// centimetres.
fn edge(index: usize, start: usize, end: usize) -> Vec<Record> {
    let [a, b] = [start, end].map(|vertex| i64::try_from(vertex).expect("index"));
    vec![refs(index, "edge", 11, &[(3, a), (5, b)])]
}

fn matching(index: usize, owner: i64, source: i64, key: i64) -> Record {
    let mut tokens = vec![
        Token::Ref(-1),
        Token::Long(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(owner),
        Token::Str("INV_NMX_MATCHED_ATTRIB".into()),
        Token::Long(1),
        Token::Long(44),
    ];
    tokens.extend([1, source, -1, key, 0].map(Token::Long));
    record(index, "ATTRIB_CUSTOM-attrib", tokens)
}

/// Two vertices (10, 12) with points (11, 13) and an edge 20 between them.
fn segment(edge_index: usize, from: [f64; 3], to: [f64; 3], base: usize) -> Vec<Record> {
    let mut records = vec![
        vertex(base),
        point(base + 1, from),
        vertex(base + 2),
        point(base + 3, to),
    ];
    records.extend(edge(edge_index, base, base + 2));
    records
}

const fn selector(source: u32, key: u32, item: u32) -> Selector {
    Selector { source, key, item }
}

fn mm(value: f64) -> f64 {
    value * 10.0
}

#[test]
fn an_edge_selector_selects_every_matched_edge_revision_with_stored_vertices() {
    let mut records = segment(20, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 10);
    records.extend(segment(30, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 40));
    records.push(matching(50, 20, 7, 3));
    records.push(matching(51, 30, 7, 3));
    let found = edges(&records, &[selector(7, 3, 99)], SCHEMA);
    let edge = &found[&7];
    assert_eq!(edge.item, 99);
    assert_eq!(
        edge.revisions
            .iter()
            .map(|revision| revision.owner)
            .collect::<Vec<_>>(),
        [20, 30]
    );
    let [start, end] = [&edge.revisions[0].start, &edge.revisions[0].end];
    assert_eq!(*start.as_raw(), Point3::new(0.0, 0.0, 0.0));
    assert_eq!(*end.as_raw(), Point3::new(mm(1.0), 0.0, 0.0));
}

#[test]
fn a_selector_without_a_complete_matched_edge_selects_nothing() {
    let mut records = segment(20, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 10);
    records.push(matching(50, 20, 7, 3));
    // No match for the key, a match on a record that is not an edge, and a
    // second owner whose vertices are not stored.
    records.push(matching(51, 10, 8, 3));
    records.push(matching(52, 20, 9, 3));
    records.push(matching(53, 77, 9, 3));
    let found = edges(
        &records,
        &[selector(7, 4, 1), selector(8, 3, 1), selector(9, 3, 1)],
        SCHEMA,
    );
    assert!(found.is_empty());
    assert_eq!(edges(&records, &[selector(7, 3, 1)], SCHEMA).len(), 1);
}

/// A face 100 with an outer loop 101 over coedges 102 and 103 of the edges 20
/// and 30, and an inner loop 104 over the coedge 105 of the edge 40.
fn faced() -> Vec<Record> {
    let mut records = segment(20, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 10);
    records.extend(segment(30, [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], 50));
    records.extend(segment(40, [5.0, 5.0, 0.0], [6.0, 5.0, 0.0], 60));
    records.push(refs(100, "face", 9, &[(4, 101)]));
    records.push(refs(101, "loop", 6, &[(3, 104), (4, 102), (5, 100)]));
    records.push(refs(102, "coedge", 11, &[(3, 103), (6, 20), (8, 101)]));
    records.push(refs(103, "coedge", 11, &[(6, 30), (8, 101)]));
    records.push(refs(104, "loop", 6, &[(4, 105), (5, 100)]));
    records.push(refs(105, "coedge", 11, &[(6, 40), (8, 104)]));
    records
}

#[test]
fn a_face_boundary_lists_the_edges_of_every_loop_once() {
    let boundary = boundary(&faced(), &[100]).expect("boundary");
    assert_eq!(
        boundary.iter().map(|edge| edge.owner).collect::<Vec<_>>(),
        [20, 30, 40]
    );
    assert_eq!(
        *boundary[1].end.as_raw(),
        Point3::new(mm(1.0), mm(1.0), 0.0)
    );
    // Revisions that list the same edge give one entry.
    let mut records = faced();
    records.push(refs(110, "face", 9, &[(4, 101)]));
    assert_eq!(boundary_len(&records, &[100, 110]), Some(3));
}

fn boundary_len(records: &[Record], owners: &[i64]) -> Option<usize> {
    boundary(records, owners).map(|edges| edges.len())
}

fn replaced(mut records: Vec<Record>, record: Record) -> Vec<Record> {
    let slot = records
        .iter()
        .position(|candidate| candidate.index == record.index)
        .expect("record to replace");
    records[slot] = record;
    records
}

#[test]
fn a_face_boundary_with_a_missing_part_is_refused() {
    assert_eq!(boundary_len(&faced(), &[999]), None);
    let missing_edge = replaced(faced(), refs(105, "coedge", 11, &[(6, 999), (8, 104)]));
    assert_eq!(boundary_len(&missing_edge, &[100]), None);
    let no_vertices = replaced(faced(), refs(30, "edge", 11, &[]));
    assert_eq!(boundary_len(&no_vertices, &[100]), None);
    let not_an_edge = replaced(faced(), refs(30, "face", 9, &[]));
    assert_eq!(boundary_len(&not_an_edge, &[100]), None);
    let missing_loop = replaced(faced(), refs(100, "face", 9, &[(4, 999)]));
    assert_eq!(boundary_len(&missing_loop, &[100]), None);
}

#[test]
fn a_chain_that_returns_to_a_visited_record_ends_without_repeating_an_edge() {
    let cyclic = replaced(
        faced(),
        refs(103, "coedge", 11, &[(3, 102), (6, 30), (8, 101)]),
    );
    assert_eq!(boundary_len(&cyclic, &[100]), Some(3));
    let looping = replaced(
        faced(),
        refs(101, "loop", 6, &[(3, 101), (4, 102), (5, 100)]),
    );
    assert_eq!(boundary_len(&looping, &[100]), Some(2));
}
