// SPDX-License-Identifier: Apache-2.0

use super::super::pieces::{PieceArrangement, StoredLoop, StoredPiece};
use super::*;
use cadmpeg_ir::features::SketchProfileBoundaryUse;

const STORED_PIECE_TOLERANCE: f64 = 1.0e-7;

struct Drawing {
    sketch: Sketch,
    entities: Vec<SketchEntity>,
}

impl Drawing {
    fn new() -> Self {
        let id = SketchId::mint("synthetic:test:id#stored-pieces").unwrap();
        Self {
            sketch: Sketch {
                id,
                name: None,
                configuration: None,
                visible: None,
                placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
                profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                native_ref: None,
            },
            entities: Vec::new(),
        }
    }

    fn add(&mut self, name: &str, geometry: SketchGeometryDefinition) -> SketchEntityId {
        let id = SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap();
        self.entities.push(SketchEntity::new(
            id.clone(),
            self.sketch.id.clone(),
            SketchGeometry::try_from(geometry).unwrap(),
        ));
        id
    }

    fn line(&mut self, name: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntityId {
        self.add(
            name,
            SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            },
        )
    }

    fn circle(&mut self, name: &str, center: (f64, f64), radius: f64) -> SketchEntityId {
        self.add(
            name,
            SketchGeometryDefinition::Circle {
                center: Point2::new(center.0, center.1),
                radius: Length::new(radius).unwrap(),
            },
        )
    }

    fn region(&self, loops: &[StoredLoop]) -> Option<SketchProfileRegion> {
        crate::test_support::with_decode_context(|ctx| {
            let arrangement = PieceArrangement::new(
                &self.sketch,
                &self.entities,
                STORED_PIECE_TOLERANCE,
                &ProfileGeometryWork::new(),
                ctx,
            )?;
            Ok::<_, cadmpeg_core::CodecError>(arrangement.and_then(|found| found.region(loops)))
        })
        .unwrap()
    }
}

fn piece(curve: &SketchEntityId, piece: u32, pieces: u32, reversed: bool) -> StoredPiece {
    StoredPiece {
        curve: curve.clone(),
        piece,
        pieces,
        reversed,
    }
}

fn outer(pieces: Vec<StoredPiece>) -> StoredLoop {
    StoredLoop {
        outer: true,
        pieces,
    }
}

fn hole(pieces: Vec<StoredPiece>) -> StoredLoop {
    StoredLoop {
        outer: false,
        pieces,
    }
}

fn trimmed(region: Option<SketchProfileRegion>) -> (Vec<SketchProfileBoundaryUse>, usize) {
    let Some(SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }) = region
    else {
        panic!("trimmed region");
    };
    (outer_boundary.into_iter().collect(), hole_boundaries.len())
}

/// A rectangle split by a line from its left side to its right side.
struct SplitRectangle {
    drawing: Drawing,
    top: SketchEntityId,
    right: SketchEntityId,
    bottom: SketchEntityId,
    left: SketchEntityId,
    divider: SketchEntityId,
}

fn chord_rectangle() -> SplitRectangle {
    let mut drawing = Drawing::new();
    let top = drawing.line("top", (0.0, 0.0), (10.0, 0.0));
    let right = drawing.line("right", (10.0, 0.0), (10.0, -10.0));
    let bottom = drawing.line("bottom", (10.0, -10.0), (0.0, -10.0));
    let left = drawing.line("left", (0.0, -10.0), (0.0, 0.0));
    let chord = drawing.line("chord", (0.0, -4.0), (10.0, -4.0));
    SplitRectangle {
        drawing,
        top,
        right,
        bottom,
        left,
        divider: chord,
    }
}

#[test]
fn stored_pieces_select_the_face_whose_directed_pieces_they_are() {
    let c = chord_rectangle();
    // Counterclockwise walk of the upper face: the right side upward, the top
    // leftward, the left side downward, the chord rightward.
    let upper = outer(vec![
        piece(&c.right, 1, 2, true),
        piece(&c.top, 1, 1, true),
        piece(&c.left, 2, 2, true),
        piece(&c.divider, 1, 1, false),
    ]);
    let (boundary, holes) = trimmed(c.drawing.region(&[upper]));
    assert_eq!(holes, 0);
    assert_eq!(boundary.len(), 4);
    assert!(boundary.iter().any(|use_| use_.entity == c.top));
    assert!(boundary.iter().all(|use_| use_.entity != c.bottom));
    // Counterclockwise walk of the lower face.
    let lower = outer(vec![
        piece(&c.bottom, 1, 1, true),
        piece(&c.right, 2, 2, true),
        piece(&c.divider, 1, 1, true),
        piece(&c.left, 1, 2, true),
    ]);
    let (boundary, _) = trimmed(c.drawing.region(&[lower]));
    assert!(boundary.iter().any(|use_| use_.entity == c.bottom));
    assert!(boundary.iter().all(|use_| use_.entity != c.top));
}

#[test]
fn stored_pieces_join_in_any_listed_order() {
    let c = chord_rectangle();
    let scrambled = outer(vec![
        piece(&c.divider, 1, 1, false),
        piece(&c.right, 1, 2, true),
        piece(&c.left, 2, 2, true),
        piece(&c.top, 1, 1, true),
    ]);
    assert!(c.drawing.region(&[scrambled]).is_some());
}

#[test]
fn stored_pieces_refuse_a_changed_ordinal_count_or_direction() {
    let c = chord_rectangle();
    let loop_with = |right: StoredPiece, chord: StoredPiece| {
        outer(vec![
            right,
            piece(&c.top, 1, 1, true),
            piece(&c.left, 2, 2, true),
            chord,
        ])
    };
    let chord = piece(&c.divider, 1, 1, false);
    let right = piece(&c.right, 1, 2, true);
    assert!(c
        .drawing
        .region(&[loop_with(right.clone(), chord.clone())])
        .is_some());
    // The other piece of the right side bounds the lower face.
    let wrong_ordinal = loop_with(piece(&c.right, 2, 2, true), chord.clone());
    assert_eq!(c.drawing.region(&[wrong_ordinal]), None);
    // The right side has two pieces, not three.
    let wrong_count = loop_with(piece(&c.right, 1, 3, true), chord.clone());
    assert_eq!(c.drawing.region(&[wrong_count]), None);
    // The upper face walks the chord rightward.
    let wrong_direction = loop_with(right, piece(&c.divider, 1, 1, true));
    assert_eq!(c.drawing.region(&[wrong_direction]), None);
}

#[test]
fn stored_pieces_refuse_a_loop_that_is_only_part_of_a_face() {
    let c = chord_rectangle();
    let missing = outer(vec![
        piece(&c.right, 1, 2, true),
        piece(&c.top, 1, 1, true),
        piece(&c.divider, 1, 1, false),
    ]);
    assert_eq!(c.drawing.region(&[missing]), None);
}

#[test]
fn stored_pieces_number_the_pieces_of_a_curve_between_region_curves_only() {
    // A curve across the bottom of the rectangle that is not part of the
    // region splits the right side, but the upper region's right side keeps
    // two pieces.
    let mut c = chord_rectangle();
    c.drawing.line("outside", (-3.0, -8.0), (13.0, -8.0));
    let upper = outer(vec![
        piece(&c.right, 1, 2, true),
        piece(&c.top, 1, 1, true),
        piece(&c.left, 2, 2, true),
        piece(&c.divider, 1, 1, false),
    ]);
    assert!(c.drawing.region(&[upper]).is_some());
}

#[test]
fn stored_pieces_name_one_curve_twice_when_its_pieces_differ() {
    // A circle cut by two vertical lines. The strip between the lines is
    // bounded by the circle's top arc and bottom arc.
    let mut drawing = Drawing::new();
    let circle = drawing.circle("circle", (0.0, 0.0), 5.0);
    let right = drawing.line("right", (2.0, -6.0), (2.0, 6.0));
    let left = drawing.line("left", (-2.0, -6.0), (-2.0, 6.0));
    // The arc across the seam is piece 1, then the top, left and bottom arcs.
    let strip = outer(vec![
        piece(&right, 2, 3, false),
        piece(&circle, 2, 4, false),
        piece(&left, 2, 3, true),
        piece(&circle, 4, 4, false),
    ]);
    let (boundary, holes) = trimmed(drawing.region(&[strip]));
    assert_eq!(holes, 0);
    assert_eq!(
        boundary.iter().filter(|use_| use_.entity == circle).count(),
        2
    );
    let swapped = outer(vec![
        piece(&right, 2, 3, false),
        piece(&circle, 2, 4, false),
        piece(&left, 2, 3, true),
        piece(&circle, 3, 4, false),
    ]);
    assert_eq!(drawing.region(&[swapped]), None);
}

fn square_with_circle() -> (Drawing, [SketchEntityId; 4], SketchEntityId) {
    let mut drawing = Drawing::new();
    let sides = [
        drawing.line("south", (0.0, 0.0), (10.0, 0.0)),
        drawing.line("east", (10.0, 0.0), (10.0, 10.0)),
        drawing.line("north", (10.0, 10.0), (0.0, 10.0)),
        drawing.line("west", (0.0, 10.0), (0.0, 0.0)),
    ];
    let circle = drawing.circle("island", (5.0, 5.0), 2.0);
    (drawing, sides, circle)
}

fn square_loop(sides: &[SketchEntityId; 4]) -> StoredLoop {
    outer(sides.iter().map(|side| piece(side, 1, 1, false)).collect())
}

#[test]
fn stored_pieces_give_the_island_as_a_hole() {
    let (drawing, sides, circle) = square_with_circle();
    let region = drawing.region(&[square_loop(&sides), hole(vec![piece(&circle, 1, 1, true)])]);
    let Some(SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }) = region
    else {
        panic!("trimmed region with a hole");
    };
    assert_eq!(outer_boundary.len(), 4);
    let [ring] = hole_boundaries.as_slice() else {
        panic!("one hole ring");
    };
    assert!(ring
        .iter()
        .all(|use_| use_.entity == circle && use_.reversed));
}

#[test]
fn stored_pieces_refuse_an_island_that_is_not_a_listed_hole() {
    let (drawing, sides, _) = square_with_circle();
    assert_eq!(drawing.region(&[square_loop(&sides)]), None);
}

#[test]
fn stored_pieces_refuse_a_second_island_that_is_not_listed() {
    let (mut drawing, sides, circle) = square_with_circle();
    let second = drawing.circle("second", (8.0, 2.0), 1.0);
    let region = drawing.region(&[square_loop(&sides), hole(vec![piece(&circle, 1, 1, true)])]);
    assert_eq!(region, None);
    let both = drawing.region(&[
        square_loop(&sides),
        hole(vec![piece(&circle, 1, 1, true)]),
        hole(vec![piece(&second, 1, 1, true)]),
    ]);
    assert!(both.is_some());
}

#[test]
fn stored_pieces_do_not_list_an_island_inside_a_hole() {
    let (mut drawing, sides, circle) = square_with_circle();
    drawing.circle("nested", (5.0, 5.0), 0.5);
    let region = drawing.region(&[square_loop(&sides), hole(vec![piece(&circle, 1, 1, true)])]);
    assert!(region.is_some());
}

#[test]
fn stored_pieces_refuse_a_hole_walked_the_exterior_way() {
    let (drawing, sides, circle) = square_with_circle();
    let region = drawing.region(&[square_loop(&sides), hole(vec![piece(&circle, 1, 1, false)])]);
    assert_eq!(region, None);
}
