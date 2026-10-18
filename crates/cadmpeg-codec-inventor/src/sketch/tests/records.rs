// SPDX-License-Identifier: Apache-2.0
//! Sketch arcs, splines, blocks, relations, and profile faces.

use super::super::{
    parse_constraint, parse_direction, parse_entity, parse_sketch_layout, parse_transform, project,
    PmDcSketchConstraintKind, PmDcSketchEntityKind, PmDcSplineCurve, SketchConstraintTag,
    SketchEntityTag, SketchInventory, BLOCK_DEFINITION_TYPE, BLOCK_INSTANCE_TYPE, BLOCK_PROXY_TYPE,
    CIRCLE_TYPE, COINCIDENT_TYPE, DIRECTION_TYPE, LINE_TYPE, POINT_TYPE, SKETCH_TYPE, SPLINE_TYPE,
    TRANSFORM_TYPE,
};
use super::{constraint_header, content, entity_prefix, list, point_bytes};
use crate::record_identity::{Located, RecordPayload};
use crate::test_support::test_fixtures::parse;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::sketches::{SketchGeometryDefinition, SketchLocus};

fn located<T: RecordPayload>(payload: T, type_id: [u8; 16], ordinal: u32) -> Located<T> {
    Located::new(
        payload,
        crate::record_identity::RecordTypeId::from_bytes(type_id),
        cadmpeg_ir::identity_key!("segment")
            .try_clone_for_decode(
                &cadmpeg_test_support::service_decode_context(),
                "Inventor located fixture token",
            )
            .expect("service fixture token"),
        ordinal,
    )
}

fn f64s(bytes: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

/// Planar sketch at ordinal 2 with identity transform (0) and +Z direction (1).
fn placement() -> (
    Located<super::super::PmDcTransformPayload>,
    Located<super::super::PmDcDirectionPayload>,
) {
    let transform = parse(
        &{
            let mut bytes = content(0);
            bytes.extend_from_slice(&0x8421u16.to_le_bytes());
            bytes.extend_from_slice(&0x7bdeu16.to_le_bytes());
            bytes
        },
        |_, source| parse_transform(source, 19).expect("transform"),
    );
    let direction = parse(
        &{
            let mut bytes = content(1);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            f64s(&mut bytes, &[0.0, 0.0, 0.0, 1.0]);
            bytes
        },
        |_, source| parse_direction(source, 19).expect("direction"),
    );
    (
        located(transform, TRANSFORM_TYPE, 0),
        located(direction, DIRECTION_TYPE, 1),
    )
}

fn sketch_bytes(index: u32, members: &[u32]) -> Vec<u8> {
    let mut bytes = content(index);
    bytes.extend_from_slice(&2i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend(list(8, members));
    bytes.extend_from_slice(&0x8000_0001u32.to_le_bytes());
    bytes.extend_from_slice(&0x8000_0002u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend(list(2, &[]));
    bytes
}

fn entity(
    bytes: &[u8],
    tag: SketchEntityTag,
    type_id: [u8; 16],
    ordinal: u32,
) -> super::super::PmDcSketchEntity {
    let payload = parse(bytes, |ctx, source| {
        parse_entity(ctx, tag, source, 19).expect("generated entity")
    });
    located(payload, type_id, ordinal)
}

fn line(index: u32, sketch: u32, points: &[u32], start: [f64; 2], end: [f64; 2]) -> Vec<u8> {
    let mut bytes = entity_prefix(index, sketch, 0);
    bytes.extend(list(2, points));
    bytes.extend(list(2, &[]));
    f64s(
        &mut bytes,
        &[start[0], start[1], end[0] - start[0], end[1] - start[1]],
    );
    bytes
}

fn circle(index: u32, sketch: u32, points: &[u32], center: u32, radius: f64) -> Vec<u8> {
    let mut bytes = entity_prefix(index, sketch, 0);
    bytes.extend(list(2, points));
    bytes.extend(list(2, &[]));
    bytes.extend_from_slice(&center.to_le_bytes());
    bytes.extend_from_slice(&radius.to_le_bytes());
    bytes.push(0);
    bytes
}

fn context() -> (DecodeArena, DecodePolicy) {
    (DecodeArena::new(), DecodePolicy::service())
}

#[test]
fn circle_point_list_selects_counterclockwise_arc_or_closed_circle() {
    // Points 3 (center), 4 (start at angle 0), 5 (end at angle pi/2); arc 6;
    // full circle 7 with two null leading references.
    let sketch = parse(&sketch_bytes(2, &[4, 5, 6, 7, 8]), |ctx, source| {
        parse_sketch_layout(ctx, source, 19, false).expect("sketch")
    });
    let (transform, direction) = placement();
    let inventory = SketchInventory {
        sketches: vec![located(sketch, SKETCH_TYPE, 2)],
        entities: vec![
            entity(
                &point_bytes(3, 3, [0.0, 0.0]),
                SketchEntityTag::Point,
                POINT_TYPE,
                3,
            ),
            entity(
                &point_bytes(4, 3, [1.0, 0.0]),
                SketchEntityTag::Point,
                POINT_TYPE,
                4,
            ),
            entity(
                &point_bytes(5, 3, [0.0, 1.0]),
                SketchEntityTag::Point,
                POINT_TYPE,
                5,
            ),
            entity(
                &circle(6, 3, &[5, 6], 4, 1.0),
                SketchEntityTag::Circle,
                CIRCLE_TYPE,
                6,
            ),
            entity(
                &circle(7, 3, &[0, 0], 4, 2.0),
                SketchEntityTag::Circle,
                CIRCLE_TYPE,
                7,
            ),
        ],
        transforms: vec![transform],
        directions: vec![direction],
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let (arena, policy) = context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let projected = project(&ctx, &inventory, &[]).expect("projection");
    assert_eq!(projected.sketches.len(), 1);
    let arc = projected
        .entities
        .iter()
        .find(|entity| entity.id().as_str().ends_with("-6"))
        .expect("arc entity");
    let SketchGeometryDefinition::Arc {
        radius,
        start_angle,
        end_angle,
        ..
    } = *arc.geometry.definition()
    else {
        panic!("circle with endpoints is an arc");
    };
    assert!((radius.get() - 10.0).abs() < 1e-12);
    assert!(start_angle.get().abs() < 1e-12);
    assert!((end_angle.get() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert_eq!(arc.endpoint_refs.len(), 2);
    let full = projected
        .entities
        .iter()
        .find(|entity| entity.id().as_str().ends_with("-7"))
        .expect("circle entity");
    assert!(matches!(
        *full.geometry.definition(),
        SketchGeometryDefinition::Circle { .. }
    ));
    // Only the closed circle forms a profile; the open arc does not.
    assert_eq!(projected.sketches[0].profiles.len(), 1);
}

#[test]
fn line_point_list_uses_first_two_points_and_point_on_line_relation() {
    // Square 0..2 with a point on its bottom edge at (1, 0).
    let mut members = (4..=13).collect::<Vec<u32>>();
    members.push(14);
    let sketch = parse(&sketch_bytes(2, &members), |ctx, source| {
        parse_sketch_layout(ctx, source, 19, false).expect("sketch")
    });
    let corners = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
    let mut entities = Vec::new();
    for (offset, corner) in corners.iter().enumerate() {
        let ordinal = 3 + u32::try_from(offset).expect("fixture index");
        entities.push(entity(
            &point_bytes(ordinal, 3, *corner),
            SketchEntityTag::Point,
            POINT_TYPE,
            ordinal,
        ));
    }
    entities.push(entity(
        &point_bytes(7, 3, [1.0, 0.0]),
        SketchEntityTag::Point,
        POINT_TYPE,
        7,
    ));
    for (offset, (start, end)) in [(4, 5), (5, 6), (6, 7), (7, 4)].into_iter().enumerate() {
        let ordinal = 8 + u32::try_from(offset).expect("fixture index");
        let points = if offset == 0 {
            vec![start, end, 8]
        } else {
            vec![start, end]
        };
        entities.push(entity(
            &line(
                ordinal,
                3,
                &points,
                corners[usize::try_from(start - 4).expect("index")],
                corners[usize::try_from(end - 4).expect("index")],
            ),
            SketchEntityTag::Line,
            LINE_TYPE,
            ordinal,
        ));
    }
    // Coincident 13 places point 7 on line 8; coincident 12 binds corner 3
    // to the start of line 8.
    let mut on_line = constraint_header(13, 0);
    on_line.extend_from_slice(&9u32.to_le_bytes());
    on_line.extend_from_slice(&8u32.to_le_bytes());
    let mut at_start = constraint_header(12, 0);
    at_start.extend_from_slice(&9u32.to_le_bytes());
    at_start.extend_from_slice(&4u32.to_le_bytes());
    let constraints = [(12, at_start), (13, on_line)]
        .into_iter()
        .map(|(ordinal, bytes)| {
            located(
                parse(&bytes, |ctx, source| {
                    parse_constraint(ctx, SketchConstraintTag::Coincident, source, 19)
                        .expect("coincident")
                }),
                COINCIDENT_TYPE,
                ordinal,
            )
        })
        .collect();
    let (transform, direction) = placement();
    let inventory = SketchInventory {
        sketches: vec![located(sketch, SKETCH_TYPE, 2)],
        entities,
        transforms: vec![transform],
        directions: vec![direction],
        constraints,
        issues: Vec::new(),
    };
    let (arena, policy) = context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let projected = project(&ctx, &inventory, &[]).expect("projection");
    assert_eq!(projected.unresolved_sketches, 0);
    assert_eq!(projected.sketches[0].profiles.len(), 1);
    assert_eq!(projected.sketches[0].profiles[0].len(), 4);
    let definition = |suffix: &str| {
        projected
            .constraints
            .iter()
            .find(|constraint| constraint.id.as_str().ends_with(suffix))
            .map(|constraint| constraint.definition.clone())
            .expect("projected relation")
    };
    assert!(matches!(
        definition("-13").kind(),
        cadmpeg_ir::sketches::SketchConstraintDefinitionInput::PointOnObject { .. }
    ));
    assert!(matches!(
        definition("-12").kind(),
        cadmpeg_ir::sketches::SketchConstraintDefinitionInput::CoincidentLoci { loci }
            if matches!(loci[0], SketchLocus::Start(_))
    ));
}

#[test]
fn branched_component_contributes_each_bounded_face() {
    // Two unit squares that share the edge from (1, 0) to (1, 1).
    let positions = [
        [0.0, 0.0],
        [1.0, 0.0],
        [2.0, 0.0],
        [2.0, 1.0],
        [1.0, 1.0],
        [0.0, 1.0],
    ];
    let edges = [(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 0), (1, 4)];
    let mut members = Vec::new();
    let mut entities = Vec::new();
    for (offset, position) in positions.iter().enumerate() {
        let ordinal = 3 + u32::try_from(offset).expect("fixture index");
        members.push(ordinal + 1);
        entities.push(entity(
            &point_bytes(ordinal, 3, *position),
            SketchEntityTag::Point,
            POINT_TYPE,
            ordinal,
        ));
    }
    for (offset, (start, end)) in edges.into_iter().enumerate() {
        let ordinal = 9 + u32::try_from(offset).expect("fixture index");
        members.push(ordinal + 1);
        let start_ordinal = 3 + u32::try_from(start).expect("fixture index");
        let end_ordinal = 3 + u32::try_from(end).expect("fixture index");
        entities.push(entity(
            &line(
                ordinal,
                3,
                &[start_ordinal + 1, end_ordinal + 1],
                positions[start],
                positions[end],
            ),
            SketchEntityTag::Line,
            LINE_TYPE,
            ordinal,
        ));
    }
    let sketch = parse(&sketch_bytes(2, &members), |ctx, source| {
        parse_sketch_layout(ctx, source, 19, false).expect("sketch")
    });
    let (transform, direction) = placement();
    let inventory = SketchInventory {
        sketches: vec![located(sketch, SKETCH_TYPE, 2)],
        entities,
        transforms: vec![transform],
        directions: vec![direction],
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let (arena, policy) = context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let projected = project(&ctx, &inventory, &[]).expect("projection");
    let profiles = &projected.sketches[0].profiles;
    assert_eq!(profiles.len(), 2);
    assert!(profiles.iter().all(|profile| profile.len() == 4));
    let shared = projected
        .entities
        .iter()
        .find(|entity| entity.id().as_str().ends_with("-15"))
        .expect("shared edge")
        .id()
        .clone();
    let uses = profiles
        .iter()
        .flatten()
        .filter(|use_| use_.entity == shared)
        .map(|use_| use_.reversed)
        .collect::<Vec<_>>();
    assert_eq!(uses.len(), 2);
    assert_ne!(uses[0], uses[1]);
}

#[test]
fn construction_flag_is_bit_0x00080000() {
    let mut bytes = point_bytes(3, 3, [0.0, 0.0]);
    bytes[22..26].copy_from_slice(&0x0000_0040u32.to_le_bytes());
    let solved = entity(&bytes, SketchEntityTag::Point, POINT_TYPE, 3);
    bytes[22..26].copy_from_slice(&0x0008_0000u32.to_le_bytes());
    let construction = entity(&bytes, SketchEntityTag::Point, POINT_TYPE, 4);
    assert_eq!(solved.entity_flags & super::super::CONSTRUCTION_FLAG, 0);
    assert_ne!(
        construction.entity_flags & super::super::CONSTRUCTION_FLAG,
        0
    );
}

fn spline_bytes(index: u32, sketch: u32, points: [u32; 2], weights: &[f64]) -> Vec<u8> {
    let mut bytes = entity_prefix(index, sketch, 0);
    bytes.extend(list(2, &points));
    bytes.extend(list(2, &[]));
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&1e-9f64.to_le_bytes());
    for value in [6u32, 6, 8] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    f64s(&mut bytes, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    let weight_count = u32::try_from(weights.len()).expect("fixture count");
    bytes.extend_from_slice(&weight_count.to_le_bytes());
    bytes.extend_from_slice(&weight_count.to_le_bytes());
    bytes.extend_from_slice(&8u32.to_le_bytes());
    f64s(&mut bytes, weights);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(&8u32.to_le_bytes());
    f64s(&mut bytes, &[0.0, 0.0, 1.0, 1.0, 2.0, 0.0]);
    bytes.extend_from_slice(&1e-12f64.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    f64s(&mut bytes, &[0.0, 1.0]);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes
}

#[test]
fn spline_selectors_store_or_omit_curve_geometry() {
    let stored = entity(
        &spline_bytes(5, 3, [4, 5], &[]),
        SketchEntityTag::Spline,
        SPLINE_TYPE,
        5,
    );
    let PmDcSketchEntityKind::Spline {
        curve:
            PmDcSplineCurve::Stored {
                degree,
                knots,
                poles,
                weights,
                ..
            },
        ..
    } = &stored.kind
    else {
        panic!("selector 4 stores a B-spline");
    };
    assert_eq!(*degree, 2);
    assert_eq!(knots.len(), 6);
    assert_eq!(poles.len(), 3);
    assert!(weights.is_empty());
    let rational = entity(
        &spline_bytes(5, 3, [4, 5], &[1.0, 0.5, 1.0]),
        SketchEntityTag::Spline,
        SPLINE_TYPE,
        5,
    );
    assert!(matches!(
        &rational.kind,
        PmDcSketchEntityKind::Spline {
            curve: PmDcSplineCurve::Stored { weights, .. },
            ..
        } if weights.len() == 3
    ));
    let mut unstored = entity_prefix(6, 3, 0);
    unstored.extend(list(2, &[4, 5]));
    unstored.extend(list(2, &[]));
    for value in [5u32, 0, 0x196a] {
        unstored.extend_from_slice(&value.to_le_bytes());
    }
    let unstored = entity(&unstored, SketchEntityTag::Spline, SPLINE_TYPE, 6);
    assert!(matches!(
        &unstored.kind,
        PmDcSketchEntityKind::Spline {
            curve: PmDcSplineCurve::Unstored { key: 0x196a, .. },
            ..
        }
    ));
    let mut bad = entity_prefix(6, 3, 0);
    bad.extend(list(2, &[4, 5]));
    bad.extend(list(2, &[]));
    bad.extend_from_slice(&6u32.to_le_bytes());
    assert!(parse(&bad, |ctx, source| {
        parse_entity(ctx, SketchEntityTag::Spline, source, 19)
    })
    .is_err());
}

#[test]
fn generated_relation_records_parse_their_operands() {
    let relation = |tag, tail: &[u8], parameter: bool| {
        let mut bytes = if parameter {
            constraint_header(9, 0x8000_0010)
        } else {
            let mut bytes = constraint_header(9, 0);
            bytes.truncate(bytes.len() - 4);
            bytes
        };
        bytes.extend_from_slice(tail);
        parse(&bytes, |ctx, source| {
            parse_constraint(ctx, tag, source, 19).expect("generated relation")
        })
        .kind
    };
    let refs = |values: &[u32]| {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>()
    };
    assert!(matches!(
        relation(SketchConstraintTag::Midpoint, &refs(&[5, 6]), true),
        PmDcSketchConstraintKind::Midpoint { curve, point } if curve.index() == 5 && point.index() == 6
    ));
    let mut aligned = refs(&[0, 5, 6]);
    for value in [0.0, 0.0, f64::MAX, 0.0, f64::MAX, 0.0] {
        aligned.extend_from_slice(&value.to_le_bytes());
    }
    assert!(matches!(
        relation(SketchConstraintTag::AlignedDistance, &aligned, true),
        PmDcSketchConstraintKind::AlignedDistance { first, .. } if first.index() == 5
    ));
    assert!(matches!(
        relation(SketchConstraintTag::Symmetric, &refs(&[5, 6, 7]), true),
        PmDcSketchConstraintKind::Symmetric { axis, .. } if axis.index() == 7
    ));
    assert!(matches!(
        relation(SketchConstraintTag::Fix, &refs(&[5]), true),
        PmDcSketchConstraintKind::Fix { entity } if entity.index() == 5
    ));
    let mut collinear = refs(&[5, 6]);
    collinear.extend_from_slice(&0u16.to_le_bytes());
    assert!(matches!(
        relation(SketchConstraintTag::Collinear, &collinear, true),
        PmDcSketchConstraintKind::Collinear { .. }
    ));
    let mut mirror = refs(&[5, 6, 7, 8, 8, 1, 9]);
    mirror.truncate(mirror.len());
    assert!(matches!(
        relation(SketchConstraintTag::MirrorPair, &mirror, true),
        PmDcSketchConstraintKind::MirrorPair { result, source, value: 1, .. }
            if result.index() == 5 && source.index() == 6
    ));
    assert!(matches!(
        relation(SketchConstraintTag::BlockRelation, &list(2, &[5, 6, 7]), false),
        PmDcSketchConstraintKind::BlockRelation { members } if members.references().len() == 3
    ));
    let mut angle = refs(&[0, 5, 6]);
    angle.extend_from_slice(&0.25f64.to_le_bytes());
    angle.extend_from_slice(&0.5f64.to_le_bytes());
    angle.extend_from_slice(&0x0au16.to_le_bytes());
    assert!(matches!(
        relation(SketchConstraintTag::Angle, &angle, true),
        PmDcSketchConstraintKind::Angle { quadrant: 0x0a, .. }
    ));
    let mut point_angle = refs(&[0, 5, 6, 6]);
    point_angle.extend_from_slice(&0.0f64.to_le_bytes());
    point_angle.extend_from_slice(&0.8f64.to_le_bytes());
    point_angle.extend_from_slice(&[0, 0, 1]);
    assert!(matches!(
        relation(SketchConstraintTag::PointAngle, &point_angle, true),
        PmDcSketchConstraintKind::PointAngle { state, .. } if state.into_inner() == [0, 0, 1]
    ));
    assert!(matches!(
        relation(SketchConstraintTag::Offset, &refs(&[5, 6, 7, 8]), true),
        PmDcSketchConstraintKind::Offset { pairs } if pairs[1][1].index() == 8
    ));
}

#[test]
fn block_instance_places_definition_members_and_proxies_in_host() {
    // Host sketch 2 lists instance 9 and proxy 10. Definition 11 lists
    // points 12 and 13 and line 14. The instance turns the block by a half
    // turn and moves it by (3, 4).
    let host = parse(&sketch_bytes(2, &[10, 11]), |ctx, source| {
        parse_sketch_layout(ctx, source, 19, false).expect("host")
    });
    let mut definition = sketch_bytes(11, &[13, 14, 15]);
    definition.extend(list(2, &[10]));
    definition.extend_from_slice(&13u32.to_le_bytes());
    definition.extend_from_slice(&[7; 16]);
    definition.extend_from_slice(&0u32.to_le_bytes());
    let definition = parse(&definition, |ctx, source| {
        parse_sketch_layout(ctx, source, 19, true).expect("definition")
    });
    let mut instance = entity_prefix(9, 3, 0);
    instance.extend_from_slice(&12u32.to_le_bytes());
    instance.extend_from_slice(&0u32.to_le_bytes());
    instance.extend_from_slice(&0x0100u16.to_le_bytes());
    instance.extend_from_slice(&0x00c0u16.to_le_bytes());
    f64s(&mut instance, &[-1.0, 0.0, 3.0, 0.0, -1.0, 4.0]);
    instance.extend_from_slice(&1u32.to_le_bytes());
    let mut proxy = entity_prefix(10, 3, 0);
    proxy.extend(list(2, &[10]));
    proxy.extend_from_slice(&13u32.to_le_bytes());
    let (transform, direction) = placement();
    let inventory = SketchInventory {
        sketches: vec![
            located(host, SKETCH_TYPE, 2),
            located(definition, BLOCK_DEFINITION_TYPE, 11),
        ],
        entities: vec![
            entity(
                &instance,
                SketchEntityTag::BlockInstance,
                BLOCK_INSTANCE_TYPE,
                9,
            ),
            entity(&proxy, SketchEntityTag::BlockProxy, BLOCK_PROXY_TYPE, 10),
            entity(
                &point_bytes(12, 12, [1.0, 0.0]),
                SketchEntityTag::Point,
                POINT_TYPE,
                12,
            ),
            entity(
                &point_bytes(13, 12, [1.0, 2.0]),
                SketchEntityTag::Point,
                POINT_TYPE,
                13,
            ),
            entity(
                &line(14, 12, &[13, 14], [1.0, 0.0], [1.0, 2.0]),
                SketchEntityTag::Line,
                LINE_TYPE,
                14,
            ),
        ],
        transforms: vec![transform],
        directions: vec![direction],
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let (arena, policy) = context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let projected = project(&ctx, &inventory, &[]).expect("projection");
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.unresolved_entities, 0);
    let position = |suffix: &str| {
        let entity = projected
            .entities
            .iter()
            .find(|entity| entity.id().as_str().ends_with(suffix))
            .expect("placed entity");
        match *entity.geometry.definition() {
            SketchGeometryDefinition::Point { position } => position.get(),
            _ => panic!("placed point"),
        }
    };
    // Proxy 10 places point 12 at (3 - 1, 4 - 0) cm.
    let proxied = position("#segment-10");
    assert!((proxied.u - 20.0).abs() < 1e-9 && (proxied.v - 40.0).abs() < 1e-9);
    // The copy of point 13 without a proxy lands at (2, 2) cm.
    let copied = position("#segment-9-13");
    assert!((copied.u - 20.0).abs() < 1e-9 && (copied.v - 20.0).abs() < 1e-9);
    let line = projected
        .entities
        .iter()
        .find(|entity| entity.id().as_str().ends_with("#segment-9-14"))
        .expect("placed line");
    assert_eq!(
        line.endpoint_refs,
        [
            "inventor:pmdc:sketch-entity#segment-10".to_owned(),
            "inventor:pmdc:sketch-entity#segment-13".to_owned()
        ]
    );
    assert_eq!(
        line.geometry_ref.as_deref(),
        None,
        "a copy is its definition member"
    );
    let block = projected
        .constraints
        .iter()
        .find(|constraint| constraint.id.as_str().ends_with("#segment-9"))
        .expect("block instance relation");
    assert!(matches!(
        block.definition.kind(),
        cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Native { native_kind, entities, .. }
            if native_kind.as_str() == "block_instance" && entities.len() == 3
    ));
}
