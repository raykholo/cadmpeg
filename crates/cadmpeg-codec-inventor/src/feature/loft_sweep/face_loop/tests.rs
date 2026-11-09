// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::face_selection::{SelectorForm, Surface};
use crate::test_support::test_fixtures::{content_at, parse};
use cadmpeg_ir::math::Point3;

const FACE_ITEM: u32 = 31;
const FACE_SOURCE: u32 = 136;
const PARTICIPANT: u32 = 20;
const FACE: i64 = 900;

fn reference(ordinal: u32) -> PmDcReference {
    PmDcReference::from_packed(0x8000_0000 | (ordinal + 1))
}

fn point(x: f64, y: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3::new(x, y, 0.0)).expect("finite point")
}

/// Edge item ordinal and selector source of the edge at `position`.
fn item_of(position: u32) -> u32 {
    40 + 2 * position
}

fn source_of(position: u32) -> u32 {
    138 + 2 * position
}

/// The edge revision at `position`: one revision, record index
/// `1000 + position`, from corner `position` to the next corner of a square.
fn square_edge(position: u32) -> EdgeVertices {
    let corners = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
    let at = usize::try_from(position).expect("position");
    let from = corners[at % 4];
    let to = corners[(at + 1) % 4];
    EdgeVertices {
        item: item_of(position),
        revisions: vec![Revision {
            owner: 1000 + i64::from(position),
            start: point(from.0, from.1),
            end: point(to.0, to.1),
        }],
    }
}

fn selected_face() -> SelectedFace {
    SelectedFace {
        owner: FACE,
        owners: vec![FACE],
        generators: 1,
        item: FACE_ITEM,
        repeated_attribute: false,
        unshared_family: false,
        identical_owners: false,
        form: SelectorForm::Direct,
        surface: Surface::Plane,
        state: 0,
        feature_sources: Vec::new(),
    }
}

/// A complete fixture: a face loop over a square of four edges.
struct Fixture {
    stored: StoredLoop,
    peer: StoredPeer,
    items: Vec<(u32, Item)>,
    face: SelectedFace,
    edges: Vec<(u32, EdgeVertices)>,
    boundary: Vec<Revision>,
}

fn fixture() -> Fixture {
    let positions = 0..4;
    let edge_items = positions
        .clone()
        .map(|position| reference(item_of(position)));
    let mut listed = vec![reference(FACE_ITEM)];
    listed.extend(edge_items.clone());
    let mut items = vec![(
        FACE_ITEM,
        Item::Face {
            sources: vec![FACE_SOURCE],
        },
    )];
    items.extend(positions.clone().map(|position| {
        (
            item_of(position),
            Item::Edge {
                sources: vec![source_of(position)],
            },
        )
    }));
    Fixture {
        stored: StoredLoop {
            face_item: reference(FACE_ITEM),
            edge_items: edge_items.collect(),
            peer: reference(21),
            value: 1,
        },
        peer: StoredPeer {
            items: listed,
            participant: reference(PARTICIPANT),
        },
        items,
        face: selected_face(),
        edges: positions
            .clone()
            .map(|position| (source_of(position), square_edge(position)))
            .collect(),
        boundary: positions
            .flat_map(|position| square_edge(position).revisions)
            .chain([Revision {
                owner: 1999,
                start: point(9.0, 9.0),
                end: point(9.0, 10.0),
            }])
            .collect(),
    }
}

fn run(fixture: &Fixture) -> Result<FaceLoop, Refusal> {
    let item = |reference: PmDcReference| {
        fixture
            .items
            .iter()
            .find(|(ordinal, _)| Some(*ordinal) == reference.record_ordinal())
            .map(|(_, item)| item.clone())
    };
    let face = |source: u32| (source == FACE_SOURCE).then_some(&fixture.face);
    let edge = |source: u32| {
        fixture
            .edges
            .iter()
            .find(|(candidate, _)| *candidate == source)
            .map(|(_, edge)| edge)
    };
    let boundary = |source: u32| (source == FACE_SOURCE).then_some(fixture.boundary.as_slice());
    join(&Operands {
        participant: PARTICIPANT,
        stored: &fixture.stored,
        peer: Some(&fixture.peer),
        item: &item,
        face: &face,
        edge: &edge,
        boundary: &boundary,
    })
}

#[test]
fn a_square_loop_joins_its_edges_in_stored_order() {
    let joined = run(&fixture()).expect("joined loop");
    assert_eq!(joined.face, FACE);
    assert_eq!(
        joined
            .edges
            .iter()
            .map(|edge| (edge.source, edge.owner))
            .collect::<Vec<_>>(),
        [(138, 1000), (140, 1001), (142, 1002), (144, 1003)]
    );
    assert_eq!(joined.edges[0].from, point(0.0, 0.0));
    assert_eq!(joined.edges[3].to, point(0.0, 0.0));
    assert!(joined.closed());
}

#[test]
fn a_stored_edge_that_runs_against_the_chain_is_traversed_reversed() {
    let mut fixture = fixture();
    let revision = &mut fixture.edges[1].1.revisions[0];
    std::mem::swap(&mut revision.start, &mut revision.end);
    fixture.boundary[1] = revision.clone();
    let joined = run(&fixture).expect("joined loop");
    assert_eq!(joined.edges[1].from, point(4.0, 0.0));
    assert_eq!(joined.edges[1].to, point(4.0, 4.0));
    assert!(joined.closed());
}

#[test]
fn a_loop_may_start_with_an_edge_that_runs_against_the_chain() {
    let mut fixture = fixture();
    let revision = &mut fixture.edges[0].1.revisions[0];
    std::mem::swap(&mut revision.start, &mut revision.end);
    fixture.boundary[0] = revision.clone();
    let joined = run(&fixture).expect("joined loop");
    assert_eq!(joined.edges[0].from, point(0.0, 0.0));
    assert!(joined.closed());
}

#[test]
fn one_closed_edge_and_two_edges_with_shared_vertices_are_loops() {
    let ring = |revisions: Vec<(u32, (FinitePoint3, FinitePoint3))>| {
        let mut fixture = fixture();
        fixture.stored.edge_items.truncate(revisions.len());
        fixture.peer.items.truncate(revisions.len() + 1);
        fixture.edges.truncate(revisions.len());
        fixture.boundary.clear();
        for (position, (_, (start, end))) in revisions.iter().enumerate() {
            let revision = Revision {
                owner: 1000 + i64::try_from(position).expect("position"),
                start: *start,
                end: *end,
            };
            fixture.edges[position].1.revisions = vec![revision.clone()];
            fixture.boundary.push(revision);
        }
        run(&fixture)
    };
    let circle = ring(vec![(0, (point(1.0, 0.0), point(1.0, 0.0)))]);
    assert_eq!(circle.expect("one closed edge").edges.len(), 1);
    let halves = ring(vec![
        (0, (point(1.0, 0.0), point(-1.0, 0.0))),
        (1, (point(1.0, 0.0), point(-1.0, 0.0))),
    ]);
    assert_eq!(halves.expect("two arcs").edges.len(), 2);
    let open = ring(vec![(0, (point(1.0, 0.0), point(-1.0, 0.0)))]);
    assert_eq!(open, Err(Refusal::Chain));
}

#[test]
fn edges_stored_out_of_chain_order_or_with_a_gap_are_refused() {
    let mut swapped = fixture();
    swapped.stored.edge_items.swap(1, 2);
    swapped.peer.items.swap(2, 3);
    assert_eq!(run(&swapped), Err(Refusal::Chain));
    let mut gap = fixture();
    let end = gap.edges[1].1.revisions[0].end;
    gap.edges[1].1.revisions[0].end = point(end.as_raw().x, end.as_raw().y + 1.0);
    gap.boundary[1] = gap.edges[1].1.revisions[0].clone();
    assert_eq!(run(&gap), Err(Refusal::Chain));
    let mut missing = fixture();
    missing.stored.edge_items.pop();
    missing.peer.items.pop();
    assert_eq!(run(&missing), Err(Refusal::Chain));
}

#[test]
fn an_edge_revision_outside_the_face_boundary_is_refused() {
    let mut off = fixture();
    off.boundary.remove(2);
    assert_eq!(run(&off), Err(Refusal::Face));
    let mut elsewhere = fixture();
    elsewhere.boundary[2].owner = 5000;
    assert_eq!(run(&elsewhere), Err(Refusal::Face));
    let mut none = fixture();
    none.boundary.clear();
    assert_eq!(run(&none), Err(Refusal::Face));
}

#[test]
fn only_the_revision_on_the_face_boundary_is_taken() {
    let mut fixture = fixture();
    fixture.edges[1].1.revisions.insert(
        0,
        Revision {
            owner: 77,
            start: point(50.0, 50.0),
            end: point(60.0, 60.0),
        },
    );
    let joined = run(&fixture).expect("joined loop");
    assert_eq!(joined.edges[1].owner, 1001);
    // Two revisions on the boundary have no single choice.
    fixture.boundary.push(Revision {
        owner: 77,
        start: point(50.0, 50.0),
        end: point(60.0, 60.0),
    });
    assert_eq!(run(&fixture), Err(Refusal::Face));
}

#[test]
fn two_items_that_select_one_revision_are_refused() {
    let mut fixture = fixture();
    fixture.edges[2].1.revisions = fixture.edges[0].1.revisions.clone();
    fixture.edges[2].1.item = item_of(2);
    assert_eq!(run(&fixture), Err(Refusal::Repeated));
}

#[test]
fn an_unjoined_selector_or_a_selector_of_another_item_is_refused() {
    let mut unjoined = fixture();
    unjoined.edges.remove(1);
    assert_eq!(run(&unjoined), Err(Refusal::Selector));
    let mut other_item = fixture();
    other_item.edges[1].1.item += 1;
    assert_eq!(run(&other_item), Err(Refusal::Selector));
    let mut other_face_item = fixture();
    other_face_item.face.item += 1;
    assert_eq!(run(&other_face_item), Err(Refusal::Selector));
}

#[test]
fn an_item_of_the_wrong_form_or_with_several_selectors_is_refused() {
    let mut two_sources = fixture();
    two_sources.items[2].1 = Item::Edge {
        sources: vec![source_of(1), 9],
    };
    assert_eq!(run(&two_sources), Err(Refusal::Items));
    let mut face_as_edge = fixture();
    face_as_edge.items[2].1 = Item::Face {
        sources: vec![source_of(1)],
    };
    assert_eq!(run(&face_as_edge), Err(Refusal::Items));
    let mut edge_as_face = fixture();
    edge_as_face.items[0].1 = Item::Edge {
        sources: vec![FACE_SOURCE],
    };
    assert_eq!(run(&edge_as_face), Err(Refusal::Items));
    let mut repeated = fixture();
    repeated.items[3].1 = repeated.items[2].1.clone();
    assert_eq!(run(&repeated), Err(Refusal::Items));
    let mut no_source = fixture();
    no_source.items[0].1 = Item::Face {
        sources: Vec::new(),
    };
    assert_eq!(run(&no_source), Err(Refusal::Items));
}

#[test]
fn a_peer_that_differs_from_the_participant_is_refused() {
    let mut value = fixture();
    value.stored.value = 0;
    assert_eq!(run(&value), Err(Refusal::Peer));
    let mut order = fixture();
    order.peer.items.swap(1, 2);
    assert_eq!(run(&order), Err(Refusal::Peer));
    let mut face = fixture();
    face.peer.items[0] = reference(FACE_ITEM + 1);
    assert_eq!(run(&face), Err(Refusal::Peer));
    let mut back = fixture();
    back.peer.participant = reference(PARTICIPANT + 1);
    assert_eq!(run(&back), Err(Refusal::Peer));
    let mut empty = fixture();
    empty.stored.edge_items.clear();
    empty.peer.items.truncate(1);
    assert_eq!(run(&empty), Err(Refusal::Peer));
}

#[test]
fn a_participant_without_a_peer_record_is_refused() {
    let fixture = fixture();
    let nothing = |_: PmDcReference| None;
    let no_face = |_: u32| None;
    let no_edge = |_: u32| None;
    let no_boundary = |_: u32| None;
    let operands = Operands {
        participant: PARTICIPANT,
        stored: &fixture.stored,
        peer: None,
        item: &nothing,
        face: &no_face,
        edge: &no_edge,
        boundary: &no_boundary,
    };
    assert_eq!(join(&operands), Err(Refusal::Peer));
}

#[test]
fn a_joined_loop_that_was_edited_out_of_order_is_not_closed() {
    let joined = run(&fixture()).expect("joined loop");
    let mut gap = joined.clone();
    gap.edges[2].from = point(8.0, 8.0);
    assert!(!gap.closed());
    let mut open_end = joined.clone();
    open_end.edges[3].to = point(8.0, 8.0);
    assert!(!open_end.closed());
    let mut repeated = joined.clone();
    repeated.edges[1].source = repeated.edges[0].source;
    assert!(!repeated.closed());
    let mut same_owner = joined.clone();
    same_owner.edges[1].owner = same_owner.edges[0].owner;
    assert!(!same_owner.closed());
    let mut is_face = joined.clone();
    is_face.face = is_face.edges[0].owner;
    assert!(!is_face.closed());
    let mut empty = joined;
    empty.edges.clear();
    assert!(!empty.closed());
}

fn reference_bytes(ordinal: u32) -> [u8; 4] {
    (0x8000_0000 | (ordinal + 1)).to_le_bytes()
}

fn list_bytes(ordinals: &[u32]) -> Vec<u8> {
    let mut bytes = 0x3000_0002u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&u32::try_from(ordinals.len()).expect("count").to_le_bytes());
    for value in [10u32, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for ordinal in ordinals {
        bytes.extend_from_slice(&reference_bytes(*ordinal));
    }
    bytes
}

fn loop_bytes(version: u8) -> Vec<u8> {
    let mut bytes = content_at(5, version, &stamps(version));
    bytes.extend_from_slice(&reference_bytes(7));
    bytes.extend_from_slice(&list_bytes(&[8, 9, 10]));
    bytes.extend_from_slice(&reference_bytes(6));
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes
}

fn peer_bytes(version: u8, state: i32) -> Vec<u8> {
    let mut bytes = content_at(6, version, &[]);
    bytes.extend_from_slice(&state.to_le_bytes());
    bytes.extend_from_slice(&list_bytes(&[7, 8, 9, 10]));
    bytes.extend_from_slice(&reference_bytes(5));
    bytes
}

fn stamps(version: u8) -> Vec<i32> {
    match version {
        ..=22 => Vec::new(),
        23 => vec![-1],
        _ => vec![-1, 0],
    }
}

#[test]
fn a_participant_stores_its_face_item_edge_items_peer_and_value() {
    for version in [16, 19, 23, 28, 31] {
        let bytes = loop_bytes(version);
        let stored =
            parse(&bytes, |ctx, source| parse_loop(ctx, source, version)).expect("participant");
        assert_eq!(stored.face_item().record_ordinal(), Some(7));
        assert_eq!(
            stored
                .edge_items
                .iter()
                .map(|item| item.record_ordinal())
                .collect::<Vec<_>>(),
            [Some(8), Some(9), Some(10)]
        );
        assert_eq!(stored.peer.record_ordinal(), Some(6));
        assert_eq!(stored.value, 1);
        for length in 0..bytes.len() {
            assert!(
                parse(&bytes[..length], |ctx, source| parse_loop(
                    ctx, source, version
                ))
                .is_err(),
                "version {version}, length {length}"
            );
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(parse(&trailing, |ctx, source| parse_loop(ctx, source, version)).is_err());
    }
}

#[test]
fn a_peer_stores_a_minus_one_state_its_items_and_the_participant() {
    let bytes = peer_bytes(19, -1);
    let stored = parse(&bytes, |ctx, source| parse_peer(ctx, source, 19)).expect("peer");
    assert_eq!(
        stored
            .items
            .iter()
            .map(|item| item.record_ordinal())
            .collect::<Vec<_>>(),
        [Some(7), Some(8), Some(9), Some(10)]
    );
    assert_eq!(stored.participant.record_ordinal(), Some(5));
    for length in 0..bytes.len() {
        assert!(
            parse(&bytes[..length], |ctx, source| parse_peer(ctx, source, 19)).is_err(),
            "length {length}"
        );
    }
    for state in [0, 1, -2] {
        assert!(parse(&peer_bytes(19, state), |ctx, source| parse_peer(
            ctx, source, 19
        ))
        .is_err());
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(parse(&trailing, |ctx, source| parse_peer(ctx, source, 19)).is_err());
}
