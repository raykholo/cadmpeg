// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

fn record(index: usize, name: &str, fields: Vec<Token>) -> Record {
    Record {
        index,
        name: name.into(),
        tokens: fields.into(),
        offset: 0,
        len: 0,
    }
}

fn fixture() -> Vec<Record> {
    vec![
        record(0, "asmheader", vec![]),
        record(
            1,
            "body",
            vec![
                Token::Ref(-1),
                Token::Long(1),
                Token::Long(-1),
                Token::Ref(2),
                Token::Ref(-1),
                Token::Ref(-1),
            ],
        ),
        record(
            2,
            "lump",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(3),
                Token::Ref(1),
            ],
        ),
        record(
            3,
            "shell",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(4),
                Token::Ref(-1),
                Token::Ref(2),
            ],
        ),
        record(
            4,
            "face",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Long(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(3),
                Token::Ref(-1),
                Token::Ref(5),
                Token::False,
                Token::False,
            ],
        ),
        record(
            5,
            "plane-surface",
            vec![
                Token::Ref(-1),
                Token::Long(-1),
                Token::Long(-1),
                Token::Position([0.0; 3]),
                Token::Vector3([0.0, 0.0, 1.0]),
                Token::Vector3([1.0, 0.0, 0.0]),
            ],
        ),
    ]
}

fn compare(a: &[Record], b: &[Record]) -> Outcome {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_loaded_input(&arena, &DecodePolicy::default(), 0)
        .expect("synthetic fixture budget");
    tagging_identity(&ctx, a, b).expect("synthetic fixture budget")
}

fn replace(records: &mut [Record], record: usize, slot: usize, token: Token) {
    Arc::make_mut(&mut records[record].tokens)[slot] = token;
}

#[test]
fn tagging_identity_accepts_equal_surfaces_and_attribute_changes() {
    let a = fixture();
    let mut b = fixture();
    replace(&mut b, 4, 0, Token::Ref(6));
    b.push(record(
        6,
        "NMx_Brep_tag-NamingMatching-attrib",
        vec![Token::Ref(4)],
    ));
    assert_eq!(compare(&a, &b), Outcome::Same);
}

#[test]
fn tagging_identity_uses_absolute_surface_parameter_tolerance() {
    let a = fixture();
    let mut b = fixture();
    replace(
        &mut b,
        5,
        3,
        Token::Position([EPS_TAGGING_SURFACE_PARAMETER_E10 / 2.0, 0.0, 0.0]),
    );
    assert_eq!(compare(&a, &b), Outcome::Same);
    replace(
        &mut b,
        5,
        3,
        Token::Position([EPS_TAGGING_SURFACE_PARAMETER_E10 * 2.0, 0.0, 0.0]),
    );
    assert_eq!(compare(&a, &b), Outcome::Changed);
}

#[test]
fn tagging_identity_detects_body_and_face_count_changes() {
    let a = fixture();
    let mut b = fixture();
    b.push(record(6, "body", vec![Token::Ref(-1)]));
    assert_eq!(compare(&a, &b), Outcome::Changed);
    let mut b = fixture();
    let mut face = b[4].clone();
    face.index = 6;
    b.push(face);
    assert_eq!(compare(&a, &b), Outcome::Changed);
}

#[test]
fn tagging_identity_detects_surface_kind_changes() {
    let a = fixture();
    let mut b = fixture();
    b[5] = record(
        5,
        "sphere-surface",
        vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Long(-1),
            Token::Position([0.0; 3]),
            Token::Double(1.0),
            Token::Vector3([0.0, 0.0, 1.0]),
            Token::Vector3([1.0, 0.0, 0.0]),
        ],
    );
    assert_eq!(compare(&a, &b), Outcome::Changed);
}

#[test]
fn tagging_identity_refuses_missing_cyclic_and_unsupported_geometry() {
    let a = fixture();
    let mut b = fixture();
    replace(&mut b, 4, 5, Token::Ref(99));
    assert_eq!(
        compare(&a, &b),
        Outcome::Undecided(Refusal::StateNotMaterialized)
    );
    b[3].name = "subshell".into();
    replace(&mut b, 3, 3, Token::Ref(3));
    replace(&mut b, 4, 5, Token::Ref(3));
    assert_eq!(
        compare(&a, &b),
        Outcome::Undecided(Refusal::IncompleteOwnership)
    );
    let mut b = fixture();
    b[5].name = "spline-surface".into();
    assert_eq!(
        compare(&a, &b),
        Outcome::Undecided(Refusal::UnsupportedSurface)
    );
    let mut b = fixture();
    replace(&mut b, 5, 3, Token::Position([f64::NAN, 0.0, 0.0]));
    assert_eq!(
        compare(&a, &b),
        Outcome::Undecided(Refusal::UnsupportedSurface)
    );
}

#[test]
fn tagging_identity_refuses_unpaired_faces_and_changed_trims_or_placement() {
    let a = fixture();
    let mut b = fixture();
    b[4].index = 6;
    replace(&mut b, 3, 5, Token::Ref(6));
    assert_eq!(
        compare(&a, &b),
        Outcome::Undecided(Refusal::UnpairedEntities)
    );
    let mut b = fixture();
    replace(&mut b, 4, 8, Token::True);
    assert_eq!(compare(&a, &b), Outcome::Undecided(Refusal::OtherGeometry));
    let mut b = fixture();
    replace(&mut b, 1, 5, Token::Ref(6));
    b.push(record(6, "transform", vec![Token::Double(1.0)]));
    assert_eq!(compare(&a, &b), Outcome::Undecided(Refusal::OtherGeometry));
}

#[test]
fn tagging_identity_materializes_sparse_slots_and_normalizes_revision_references() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_loaded_input(&arena, &DecodePolicy::default(), 0)
        .expect("synthetic fixture budget");
    let stored = record(0, "body", vec![Token::Ref(100)]);
    let versions = BTreeMap::from([(4, 99), (7, 100)]);
    let slots = HashMap::from([(99, 4), (100, 7)]);
    let records = materialize(&ctx, &versions, &slots, |_| Some(&stored))
        .expect("materialization budget")
        .expect("complete materialized state");
    assert_eq!(
        records.iter().map(|r| r.index).collect::<Vec<_>>(),
        vec![4, 7]
    );
    assert_eq!(records[0].ref_at(0), Some(7));
    assert!(
        materialize(&ctx, &versions, &HashMap::new(), |_| Some(&stored))
            .expect("synthetic fixture budget")
            .is_none()
    );
    assert!(materialize(&ctx, &versions, &slots, |_| None)
        .expect("synthetic fixture budget")
        .is_none());
}

#[test]
fn tagging_identity_charges_decode_budget() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 0;
    let ctx =
        DecodeContext::for_loaded_input(&arena, &policy, 0).expect("synthetic fixture budget");
    assert!(tagging_identity(&ctx, &fixture(), &fixture()).is_err());
}

#[test]
fn tagging_identity_refuses_absent_joins_predecessors_and_materialized_states() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_loaded_input(&arena, &DecodePolicy::default(), 0)
        .expect("synthetic fixture budget");
    let states = BTreeMap::from([(0, fixture()), (1, fixture())]);
    for (before, after, complete, expected) in [
        (None, None, true, Outcome::Undecided(Refusal::NoStateJoin)),
        (
            None,
            Some(0),
            true,
            Outcome::Undecided(Refusal::NoPredecessor),
        ),
        (
            Some(1),
            Some(2),
            true,
            Outcome::Undecided(Refusal::StateNotMaterialized),
        ),
        (
            Some(0),
            Some(1),
            false,
            Outcome::Undecided(Refusal::StateNotMaterialized),
        ),
        (Some(0), Some(1), true, Outcome::Same),
    ] {
        assert_eq!(
            compare_pair(&ctx, before, after, &states, complete).expect("synthetic fixture budget"),
            expected
        );
    }
}

#[test]
fn tagging_identity_probe_refuses_missing_state_data() {
    use crate::feature::history_states::{Binding, StateRef};
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_loaded_input(&arena, &DecodePolicy::default(), 0)
        .expect("synthetic fixture budget");
    let mut tag = FeatureHistoryState {
        id: "join".into(),
        feature: "feature".into(),
        source_index: 1,
        kind: "native:Tagging".into(),
        state_before: None,
        state_after: None,
        body_key: None,
        binding: Binding::Bound,
    };
    let index = StateIndex::default();
    assert_eq!(
        probe(&ctx, &[], RefWidth::Four, &index, &[tag.clone()]).expect("synthetic fixture budget"),
        vec![Outcome::Undecided(Refusal::NoStateJoin)]
    );
    tag.state_after = Some(StateRef {
        ordinal: 0,
        state_id: 1,
    });
    assert_eq!(
        probe(&ctx, &[], RefWidth::Four, &index, &[tag.clone()]).expect("synthetic fixture budget"),
        vec![Outcome::Undecided(Refusal::NoPredecessor)]
    );
    tag.state_before = Some(0);
    tag.state_after = Some(StateRef {
        ordinal: 1,
        state_id: 2,
    });
    assert_eq!(
        probe(&ctx, &[], RefWidth::Four, &index, &[tag]).expect("synthetic fixture budget"),
        vec![Outcome::Undecided(Refusal::StateNotMaterialized)]
    );
}

#[test]
fn tagging_identity_compares_normalized_surface_directions() {
    let mut a = fixture();
    let mut b = fixture();
    let small = EPS_TAGGING_SURFACE_PARAMETER_E10 / 100.0;
    for records in [&mut a, &mut b] {
        replace(records, 5, 5, Token::Vector3([small, 0.0, 0.0]));
    }
    replace(&mut a, 5, 4, Token::Vector3([0.0, 0.0, small]));
    replace(&mut b, 5, 4, Token::Vector3([0.0, small, 0.0]));
    assert_eq!(compare(&a, &b), Outcome::Changed);
}

#[test]
fn tagging_identity_refuses_a_changed_surface_chart_with_equal_decoded_geometry() {
    let mut a = fixture();
    a[5] = record(
        5,
        "cone-surface",
        vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Long(-1),
            Token::Position([0.0; 3]),
            Token::Vector3([0.0, 0.0, 1.0]),
            Token::Vector3([1.0, 0.0, 0.0]),
            Token::Double(1.0),
            Token::Double(0.0),
            Token::Double(1.0),
            Token::Double(1.0),
        ],
    );
    let mut b = a.clone();
    replace(&mut b, 5, 9, Token::Double(2.0));
    assert_eq!(compare(&a, &b), Outcome::Undecided(Refusal::OtherGeometry));
    replace(&mut b, 5, 5, Token::Vector3([2.0, 0.0, 0.0]));
    assert_eq!(compare(&a, &b), Outcome::Changed);
}

#[test]
fn tagging_identity_charges_string_copies_to_decode_budget() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 100;
    let ctx =
        DecodeContext::for_loaded_input(&arena, &policy, 0).expect("synthetic fixture budget");
    let stored = record(0, "attribute", vec![Token::Str("x".repeat(200))]);
    assert!(materialize(
        &ctx,
        &BTreeMap::from([(0, 0)]),
        &HashMap::from([(0, 0)]),
        |_| Some(&stored)
    )
    .is_err());
}
