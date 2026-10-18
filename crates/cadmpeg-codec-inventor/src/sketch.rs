// SPDX-License-Identifier: Apache-2.0
//! Typed planar-sketch records and closed neutral sketch graphs.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    NativeOperandField, Sketch, SketchConstraint, SketchConstraintDefinitionInput,
    SketchConstraintId, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus, SketchNativeOperand, SketchPlacement,
};
use cadmpeg_ir::{
    features::{DesignParameter, ParameterId},
    scalar::{Angle, FiniteReal, Length, PositiveReal},
};
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};

use crate::compact_matrix::{CompactMatrix, CompactMatrix3};
use crate::pmdc::{
    content_header, inventor_id, reference_list, Cursor, PmDcContentHeader, PmDcReference,
    PmDcReferenceList,
};
use crate::record_identity::{push_record, Located, RecordPayload};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};
use cadmpeg_ir::native::bytes::NativeBytes;

const EPS_SKETCH_LINE_CARRIER_MATCHES_E10: f64 = 1.0e-10;
const EPS_SKETCH_PROJECT_PLACEMENT_E10: f64 = 1.0e-10;
/// Relative agreement between an arc endpoint's center distance and the radius.
const EPS_SKETCH_ARC_RADIUS_RELATIVE: f64 = 1.0e-6;
/// Entity-flag bit that marks construction geometry.
const CONSTRUCTION_FLAG: u32 = 0x0008_0000;

const SKETCH_TYPE: [u8; 16] = inventor_id(0x9087_4d11);
const TRANSFORM_TYPE: [u8; 16] = inventor_id(0x9087_4d18);
const POINT_TYPE: [u8; 16] = sketch_entity_id(0xce52_df35);
const LINE_TYPE: [u8; 16] = sketch_entity_id(0xce52_df3a);
const CIRCLE_TYPE: [u8; 16] = sketch_entity_id(0xce52_df3b);
const DIRECTION_TYPE: [u8; 16] = sketch_entity_id(0xce52_df40);
const ELLIPSE_TYPE: [u8; 16] = [
    0x60, 0xd4, 0x07, 0x45, 0xd1, 0x11, 0xbe, 0xe6, 0x80, 0x00, 0x6f, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const COINCIDENT_TYPE: [u8; 16] = inventor_id(0x9087_4d94);
const PARALLEL_TYPE: [u8; 16] = inventor_id(0x9087_4d95);
const PERPENDICULAR_TYPE: [u8; 16] = inventor_id(0x9087_4d96);
const TANGENT_TYPE: [u8; 16] = inventor_id(0x9087_4d97);
const HORIZONTAL_TYPE: [u8; 16] = inventor_id(0x9087_4d98);
const VERTICAL_TYPE: [u8; 16] = inventor_id(0x9087_4d99);
const HORIZONTAL_DISTANCE_TYPE: [u8; 16] = [
    0x00, 0xc0, 0xac, 0x00, 0xd1, 0x11, 0x5f, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const VERTICAL_DISTANCE_TYPE: [u8; 16] = [
    0x40, 0xff, 0x83, 0x36, 0xd1, 0x11, 0x5f, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const RADIUS_TYPE: [u8; 16] = [
    0x00, 0xb7, 0x1b, 0x67, 0xd1, 0x11, 0x68, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const DIAMETER_TYPE: [u8; 16] = [
    0xe0, 0x96, 0xdf, 0x74, 0xd1, 0x11, 0x69, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const CIRCLE_CENTER_TYPE: [u8; 16] = [
    0x00, 0x8c, 0x10, 0xe1, 0xd1, 0x11, 0x02, 0xe6, 0x80, 0x00, 0x6d, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const EQUAL_RADIUS_TYPE: [u8; 16] = [
    0xd0, 0x7d, 0x2c, 0x44, 0xd1, 0x11, 0x89, 0xe6, 0x80, 0x00, 0x6f, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];

const SPLINE_TYPE: [u8; 16] = [
    0x46, 0x73, 0x7b, 0x31, 0xd3, 0x11, 0x7c, 0x7a, 0x60, 0x00, 0x1c, 0xb3, 0xd1, 0xc1, 0xfb, 0xb0,
];
const BLOCK_INSTANCE_TYPE: [u8; 16] = [
    0x4c, 0x12, 0xf1, 0x4c, 0x1d, 0x4f, 0x0f, 0xb4, 0xd3, 0xf8, 0x0f, 0x82, 0x44, 0x52, 0x8f, 0xcb,
];
const BLOCK_PROXY_TYPE: [u8; 16] = [
    0x7e, 0x71, 0xa9, 0x18, 0xee, 0x49, 0x61, 0x2a, 0x26, 0x8c, 0x54, 0x9c, 0x65, 0x84, 0xf7, 0xfe,
];
const MIRROR_NODE_TYPE: [u8; 16] = [
    0x38, 0x48, 0x53, 0x52, 0x9f, 0x44, 0x52, 0x55, 0xe1, 0xe1, 0xda, 0xb4, 0x10, 0xeb, 0x06, 0xc4,
];
const BLOCK_DEFINITION_TYPE: [u8; 16] = [
    0xc4, 0xaa, 0xa4, 0x86, 0x5f, 0x4a, 0x91, 0x66, 0x25, 0xd4, 0xae, 0x83, 0xc3, 0x12, 0x2a, 0x26,
];
const MIDPOINT_TYPE: [u8; 16] = sketch_entity_id(0x21e8_70bf);
const ALIGNED_DISTANCE_TYPE: [u8; 16] = [
    0x58, 0x85, 0x05, 0x11, 0xd2, 0x11, 0xe3, 0x95, 0x60, 0x00, 0x0c, 0xb3, 0x89, 0x32, 0xed, 0xb0,
];
const POINTS_VERTICAL_TYPE: [u8; 16] = [
    0x50, 0x52, 0xda, 0x64, 0xd1, 0x11, 0x5e, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const POINTS_HORIZONTAL_TYPE: [u8; 16] = [
    0xc0, 0xa3, 0x55, 0x8f, 0xd1, 0x11, 0x5e, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const SYMMETRIC_TYPE: [u8; 16] = [
    0x10, 0x7c, 0xdd, 0x0d, 0xd1, 0x11, 0x9c, 0xe6, 0x80, 0x00, 0x6f, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const FIX_TYPE: [u8; 16] = [
    0xb0, 0x7b, 0x17, 0x0f, 0xd1, 0x11, 0x96, 0xe6, 0x80, 0x00, 0x6f, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const COLLINEAR_TYPE: [u8; 16] = [
    0xe0, 0x7b, 0x9a, 0x5a, 0xd1, 0x11, 0x5d, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const EQUAL_LENGTH_TYPE: [u8; 16] = [
    0xe0, 0xc2, 0x81, 0xc6, 0xd1, 0x11, 0x10, 0xe6, 0x80, 0x00, 0x6e, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const MIRROR_PAIR_TYPE: [u8; 16] = [
    0x1f, 0x89, 0xdb, 0xfb, 0xd8, 0x47, 0x78, 0xb6, 0xb0, 0x96, 0x72, 0x85, 0xc7, 0xec, 0x8b, 0xb9,
];
const BLOCK_RELATION_TYPE: [u8; 16] = [
    0xcf, 0x8d, 0x56, 0xf2, 0x32, 0x4a, 0x27, 0xdd, 0xb6, 0x66, 0xf1, 0x89, 0x4a, 0x9d, 0x16, 0x17,
];
const SPLINE_POINT_TYPE: [u8; 16] = [
    0x9e, 0x14, 0x6d, 0x7c, 0xd1, 0x11, 0x5b, 0xd5, 0x00, 0x08, 0x47, 0xb0, 0x05, 0x24, 0xdc, 0x09,
];
const OFFSET_TYPE: [u8; 16] = [
    0xbc, 0x14, 0x4b, 0x4e, 0xd2, 0x11, 0xfe, 0xa4, 0x60, 0x00, 0x10, 0xb3, 0x89, 0x32, 0xed, 0xb0,
];
const POINT_ANGLE_TYPE: [u8; 16] = [
    0x84, 0x5c, 0x3b, 0xbf, 0xd2, 0x11, 0x2a, 0xe9, 0x60, 0x00, 0x4b, 0xb3, 0x89, 0x32, 0xed, 0xb0,
];
const ANGLE_TYPE: [u8; 16] = [
    0x10, 0x0a, 0x0d, 0x59, 0xd1, 0x11, 0xca, 0xe6, 0x80, 0x00, 0x6f, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];

const fn sketch_entity_id(time_low: u32) -> [u8; 16] {
    let first = time_low.to_le_bytes();
    [
        first[0], first[1], first[2], first[3], 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc,
        0x06, 0x63, 0xdc, 0x09,
    ]
}

#[derive(Debug)]
pub(crate) struct SketchInventory {
    pub(crate) sketches: Vec<PmDcSketch>,
    pub(crate) entities: Vec<PmDcSketchEntity>,
    pub(crate) transforms: Vec<PmDcTransform>,
    pub(crate) directions: Vec<PmDcDirection>,
    pub(crate) constraints: Vec<PmDcSketchConstraint>,
    pub(crate) issues: Vec<RecordIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcSketchConstraintPayload {
    save_version_major: u8,
    pub(crate) header: PmDcConstraintHeader,
    pub(crate) kind: PmDcSketchConstraintKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcConstraintHeader {
    pub(crate) content: PmDcContentHeader,
    state: i32,
    pub(crate) group: PmDcReference,
    pub(crate) scalar_map: PmDcReferenceScalarMap,
    pub(crate) reference_map: PmDcReferenceSelectorMap,
    pub(crate) parameter: PmDcReference,
}

type PmDcReferenceScalarMap = crate::pmdc::PmDcPairedMap<FiniteReal>;
type PmDcReferenceSelectorMap = crate::pmdc::PmDcPairedMap<u32>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcSketchConstraintKind {
    Coincident {
        first: PmDcReference,
        second: PmDcReference,
    },
    Parallel {
        first: PmDcReference,
        second: PmDcReference,
        orientation: u16,
    },
    Perpendicular {
        first: PmDcReference,
        second: PmDcReference,
        orientation: u16,
    },
    Tangent {
        first: PmDcReference,
        second: PmDcReference,
        extension: Option<u32>,
    },
    Horizontal {
        entity: PmDcReference,
        state: u8,
    },
    Vertical {
        entity: PmDcReference,
        state: u8,
    },
    HorizontalDistance {
        first: PmDcReference,
        second: PmDcReference,
        parameter: PmDcReference,
        values: [u32; 4],
    },
    VerticalDistance {
        first: PmDcReference,
        second: PmDcReference,
        parameter: PmDcReference,
        values: [u32; 4],
    },
    Radius {
        state: u32,
        entity: PmDcReference,
        values: [u32; 4],
    },
    Diameter {
        reference: PmDcReference,
        entity: PmDcReference,
        values: [u32; 4],
    },
    CircleCenter {
        entity: PmDcReference,
        center: PmDcReference,
    },
    EqualRadius {
        first: PmDcReference,
        second: PmDcReference,
    },
    Midpoint {
        curve: PmDcReference,
        point: PmDcReference,
    },
    AlignedDistance {
        value: u32,
        first: PmDcReference,
        second: PmDcReference,
        extent: [FiniteReal; 6],
    },
    PointsVertical {
        first: PmDcReference,
        second: PmDcReference,
    },
    PointsHorizontal {
        first: PmDcReference,
        second: PmDcReference,
    },
    Symmetric {
        first: PmDcReference,
        second: PmDcReference,
        axis: PmDcReference,
    },
    Fix {
        entity: PmDcReference,
    },
    Collinear {
        first: PmDcReference,
        second: PmDcReference,
        orientation: u16,
    },
    EqualLength {
        first: PmDcReference,
        second: PmDcReference,
    },
    MirrorPair {
        result: PmDcReference,
        source: PmDcReference,
        axis: PmDcReference,
        mirror: [PmDcReference; 2],
        value: u32,
        data: PmDcReference,
    },
    BlockRelation {
        members: PmDcReferenceList,
    },
    SplinePoint {
        spline: PmDcReference,
        point: PmDcReference,
    },
    Offset {
        pairs: [[PmDcReference; 2]; 2],
    },
    Angle {
        value: u32,
        first: PmDcReference,
        second: PmDcReference,
        label: [FiniteReal; 2],
        quadrant: u16,
    },
    PointAngle {
        value: u32,
        center: PmDcReference,
        first: PmDcReference,
        second: PmDcReference,
        label: [FiniteReal; 2],
        state: NativeBytes<[u8; 3]>,
    },
}

impl PmDcSketchConstraintKind {
    /// Whether the relation names a sketch coordinate axis, so it holds for
    /// a block copy only when the instance keeps the axes.
    pub(crate) fn depends_on_axes(&self) -> bool {
        matches!(
            self,
            Self::Horizontal { .. }
                | Self::Vertical { .. }
                | Self::HorizontalDistance { .. }
                | Self::VerticalDistance { .. }
                | Self::PointsVertical { .. }
                | Self::PointsHorizontal { .. }
        )
    }

    /// Every record reference stored by the relation-specific fields.
    pub(crate) fn references(&self) -> Vec<PmDcReference> {
        match self {
            Self::Coincident { first, second }
            | Self::Parallel { first, second, .. }
            | Self::Perpendicular { first, second, .. }
            | Self::Tangent { first, second, .. }
            | Self::CircleCenter {
                entity: first,
                center: second,
            }
            | Self::EqualRadius { first, second }
            | Self::Diameter {
                reference: first,
                entity: second,
                ..
            }
            | Self::Midpoint {
                curve: first,
                point: second,
            }
            | Self::AlignedDistance { first, second, .. }
            | Self::PointsVertical { first, second }
            | Self::PointsHorizontal { first, second }
            | Self::Collinear { first, second, .. }
            | Self::EqualLength { first, second }
            | Self::SplinePoint {
                spline: first,
                point: second,
            }
            | Self::Angle { first, second, .. } => vec![*first, *second],
            Self::Horizontal { entity, .. }
            | Self::Vertical { entity, .. }
            | Self::Radius { entity, .. }
            | Self::Fix { entity } => vec![*entity],
            Self::HorizontalDistance {
                first,
                second,
                parameter,
                ..
            }
            | Self::VerticalDistance {
                first,
                second,
                parameter,
                ..
            } => vec![*first, *second, *parameter],
            Self::Symmetric {
                first,
                second,
                axis,
            } => vec![*first, *second, *axis],
            Self::PointAngle {
                center,
                first,
                second,
                ..
            } => vec![*center, *first, *second],
            Self::MirrorPair {
                result,
                source,
                axis,
                mirror,
                data,
                ..
            } => vec![*result, *source, *axis, mirror[0], mirror[1], *data],
            Self::BlockRelation { members } => members.references().to_vec(),
            Self::Offset { pairs } => pairs.iter().flatten().copied().collect(),
        }
    }
}

pub(crate) struct SketchProjection {
    pub(crate) sketches: Vec<Sketch>,
    pub(crate) entities: Vec<SketchEntity>,
    pub(crate) constraints: Vec<SketchConstraint>,
    pub(crate) unresolved_sketches: usize,
    pub(crate) unresolved_entities: usize,
    pub(crate) unresolved_constraints: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcSketchPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) state: i32,
    pub(crate) count_value: u32,
    pub(crate) entities: PmDcReferenceList,
    pub(crate) transform: PmDcReference,
    pub(crate) direction: PmDcReference,
    pub(crate) values: [u32; 2],
    pub(crate) auxiliary: Option<PmDcReferenceList>,
    /// The sketch-block definition tail; absent for a planar sketch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) block: Option<PmDcBlockDefinition>,
}

/// The fields that follow the sketch layout in a sketch-block definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcBlockDefinition {
    pub(crate) instances: PmDcReferenceList,
    pub(crate) base_point: PmDcReference,
    pub(crate) identifier: NativeBytes<[u8; 16]>,
    pub(crate) value: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcSketchEntityPayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    entity_flags: u32,
    pub(crate) sketch: PmDcReference,
    pub(crate) kind: PmDcSketchEntityKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcSketchEntityKind {
    Point {
        position: [FiniteReal; 2],
        endpoint_of: PmDcReferenceList,
        center_of: PmDcReferenceList,
        #[serde(flatten)]
        tail: PointTail,
    },
    Line {
        points: PmDcReferenceList,
        auxiliary: Vec<PmDcReferenceList>,
        origin: [FiniteReal; 2],
        direction: [FiniteReal; 2],
    },
    Circle {
        points: PmDcReferenceList,
        auxiliary: Vec<PmDcReferenceList>,
        center: PmDcReference,
        radius: PositiveReal,
        state: u8,
    },
    Ellipse {
        points: PmDcReferenceList,
        auxiliary: Vec<PmDcReferenceList>,
        center: PmDcReference,
        major_direction: [FiniteReal; 2],
        major_radius: PositiveReal,
        minor_radius: PositiveReal,
        state: u8,
    },
    Spline {
        points: PmDcReferenceList,
        auxiliary: PmDcReferenceList,
        curve: PmDcSplineCurve,
    },
    BlockInstance {
        definition: PmDcReference,
        relation: PmDcReference,
        transform: CompactMatrix3,
        value: u32,
    },
    BlockProxy {
        instances: PmDcReferenceList,
        entity: PmDcReference,
    },
    MirrorNode {
        axis: PmDcReference,
        data: PmDcReference,
    },
}

impl PmDcSketchEntityKind {
    /// Every record reference stored by the entity-specific fields.
    pub(crate) fn references(&self) -> Vec<PmDcReference> {
        let mut references = Vec::new();
        match self {
            Self::Point {
                endpoint_of,
                center_of,
                tail,
                ..
            } => {
                references.extend_from_slice(endpoint_of.references());
                references.extend_from_slice(center_of.references());
                if let PointTail::Present { associations, .. } = tail {
                    references.extend_from_slice(associations.references());
                }
            }
            Self::Line {
                points, auxiliary, ..
            } => {
                references.extend_from_slice(points.references());
                for list in auxiliary {
                    references.extend_from_slice(list.references());
                }
            }
            Self::Circle {
                points,
                auxiliary,
                center,
                ..
            }
            | Self::Ellipse {
                points,
                auxiliary,
                center,
                ..
            } => {
                references.extend_from_slice(points.references());
                for list in auxiliary {
                    references.extend_from_slice(list.references());
                }
                references.push(*center);
            }
            Self::Spline {
                points, auxiliary, ..
            } => {
                references.extend_from_slice(points.references());
                references.extend_from_slice(auxiliary.references());
            }
            Self::BlockInstance {
                definition,
                relation,
                ..
            } => references.extend([*definition, *relation]),
            Self::BlockProxy { instances, entity } => {
                references.extend_from_slice(instances.references());
                references.push(*entity);
            }
            Self::MirrorNode { axis, data } => references.extend([*axis, *data]),
        }
        references
    }
}

/// The curve payload of a sketch spline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "curve_form", rename_all = "snake_case")]
pub(crate) enum PmDcSplineCurve {
    /// Selector 4: a stored B-spline.
    Stored {
        values: [u32; 2],
        degree: u32,
        knot_tolerance: FiniteReal,
        knot_capacity: u32,
        knots: Vec<FiniteReal>,
        weight_capacity: u32,
        /// Pole weights; empty for a non-rational spline.
        weights: Vec<FiniteReal>,
        pole_capacity: u32,
        poles: Vec<[FiniteReal; 2]>,
        pole_tolerance: FiniteReal,
        flags: [u32; 2],
        range: [FiniteReal; 2],
    },
    /// Selector 5: a curve whose geometry the sketch does not store.
    Unstored { value: u32, key: u32 },
}

/// The absent or complete state and association tail of a sketch point.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "PointTailWire")]
pub(crate) enum PointTail {
    Absent,
    Present {
        state: u32,
        associations: PmDcReferenceList,
    },
}

#[derive(Serialize)]
struct PointTailRef<'a> {
    state: Option<u32>,
    associations: Option<&'a PmDcReferenceList>,
}

impl Serialize for PointTail {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            Self::Absent => PointTailRef {
                state: None,
                associations: None,
            },
            Self::Present {
                state,
                associations,
            } => PointTailRef {
                state: Some(*state),
                associations: Some(associations),
            },
        };
        wire.serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct PointTailWire {
    state: Option<u32>,
    associations: Option<PmDcReferenceList>,
}

impl From<PointTail> for PointTailWire {
    fn from(tail: PointTail) -> Self {
        match tail {
            PointTail::Absent => Self {
                state: None,
                associations: None,
            },
            PointTail::Present {
                state,
                associations,
            } => Self {
                state: Some(state),
                associations: Some(associations),
            },
        }
    }
}

impl TryFrom<PointTailWire> for PointTail {
    type Error = &'static str;

    fn try_from(wire: PointTailWire) -> Result<Self, Self::Error> {
        match (wire.state, wire.associations) {
            (Some(state), Some(associations)) => Ok(Self::Present {
                state,
                associations,
            }),
            (None, None) => Ok(Self::Absent),
            _ => Err("point state and associations must be present together"),
        }
    }
}

const TRANSFORM_PREFIX: u32 = 0x203;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "PmDcTransformPayloadWire")]
pub(crate) struct PmDcTransformPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) prefix_present: bool,
    pub(crate) matrix: CompactMatrix,
}

impl Serialize for PmDcTransformPayload {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(6))?;
        let (value_mask, zero_mask) = self.matrix.masks();
        map.serialize_entry("save_version_major", &self.save_version_major)?;
        map.serialize_entry("header", &self.header)?;
        map.serialize_entry("prefix", &self.prefix_present.then_some(TRANSFORM_PREFIX))?;
        map.serialize_entry("value_mask", &value_mask)?;
        map.serialize_entry("zero_mask", &zero_mask)?;
        map.serialize_entry("matrix", &self.matrix.rows())?;
        map.end()
    }
}

#[derive(Serialize, Deserialize)]
struct PmDcTransformPayloadWire {
    save_version_major: u8,
    header: PmDcContentHeader,
    prefix: Option<u32>,
    #[serde(flatten)]
    matrix: CompactMatrix,
}

impl From<PmDcTransformPayload> for PmDcTransformPayloadWire {
    fn from(value: PmDcTransformPayload) -> Self {
        Self {
            save_version_major: value.save_version_major,
            header: value.header,
            prefix: value.prefix_present.then_some(TRANSFORM_PREFIX),
            matrix: value.matrix,
        }
    }
}

impl TryFrom<PmDcTransformPayloadWire> for PmDcTransformPayload {
    type Error = &'static str;

    fn try_from(wire: PmDcTransformPayloadWire) -> Result<Self, Self::Error> {
        let prefix_present = match wire.prefix {
            None => false,
            Some(TRANSFORM_PREFIX) => true,
            Some(_) => return Err("transform prefix must be 515 or null"),
        };
        Ok(Self {
            save_version_major: wire.save_version_major,
            header: wire.header,
            prefix_present,
            matrix: wire.matrix,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcDirectionPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) entity_flags: u32,
    pub(crate) parameter: FiniteReal,
    pub(crate) extension: Option<u32>,
    pub(crate) direction: [FiniteReal; 3],
}

pub(crate) fn inventory(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
) -> Result<SketchInventory, CodecError> {
    let mut inventory = SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmDc {
            continue;
        }
        let Some(version) = segment.registry.map(|join| join.version_major) else {
            continue;
        };
        if !(15..=22).contains(&version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in &table.records {
            let Some(tag) = SketchRecordTag::from_type_id(record.type_id) else {
                continue;
            };
            let result = match tag {
                SketchRecordTag::Sketch | SketchRecordTag::BlockDefinition => parse_sketch_layout(
                    ctx,
                    record.payload,
                    version,
                    matches!(tag, SketchRecordTag::BlockDefinition),
                )
                .and_then(|value| {
                    push_record(
                        ctx,
                        &mut inventory.sketches,
                        value,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc sketch record",
                    )
                }),
                SketchRecordTag::Entity(entity) => {
                    parse_entity(ctx, entity, record.payload, version).and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.entities,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc sketch entity record",
                        )
                    })
                }
                SketchRecordTag::Transform => {
                    parse_transform(record.payload, version).and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.transforms,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc transform record",
                        )
                    })
                }
                SketchRecordTag::Direction => {
                    parse_direction(record.payload, version).and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.directions,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc direction record",
                        )
                    })
                }
                SketchRecordTag::Constraint(constraint) => {
                    parse_constraint(ctx, constraint, record.payload, version).and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.constraints,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc sketch constraint record",
                        )
                    })
                }
            };
            if let Err(error) = result {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                ctx.charge_collection_items(1, "admit Inventor PmDc sketch issue")?;
                ctx.charge_entities(1, "admit Inventor PmDc sketch issue")?;
                ctx.charge_formatted_retained(
                    format_args!("{error}"),
                    "retain Inventor PmDc sketch issue detail",
                )?;
                ctx.charge_retained(32, "retain Inventor PmDc sketch issue type id")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(segment.pair.token.as_str().len()),
                    "retain Inventor PmDc sketch issue segment token",
                )?;
                inventory.issues.push(RecordIssue {
                    family: RecordIssueFamily::Sketch {
                        type_id: crate::record_identity::RecordTypeId::from_bytes(record.type_id),
                    },
                    segment_token: segment.pair.token.key().clone(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    Ok(inventory)
}

#[cfg(test)]
fn parse_sketch(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchPayload, CodecError> {
    parse_sketch_layout(ctx, source, version, false)
}

fn parse_sketch_layout(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
    block_definition: bool,
) -> Result<PmDcSketchPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("sketch state")?.cast_signed();
    let count_value = cursor.u32("sketch count value")?;
    let entities = reference_list(ctx, &mut cursor, 8, "sketch entity array")?;
    let transform = cursor.reference("sketch transform reference")?;
    let direction = cursor.reference("sketch direction reference")?;
    let values = [cursor.u32("sketch value 0")?, cursor.u32("sketch value 1")?];
    let auxiliary = (block_definition || cursor.remaining() != 0)
        .then(|| reference_list(ctx, &mut cursor, 2, "sketch auxiliary list"))
        .transpose()?;
    let block = if block_definition {
        Some(PmDcBlockDefinition {
            instances: reference_list(ctx, &mut cursor, 2, "block definition instance list")?,
            base_point: cursor.reference("block definition base point reference")?,
            identifier: NativeBytes::new(cursor.take_array::<16>("block definition identifier")?),
            value: cursor.u32("block definition value")?,
        })
    } else {
        None
    };
    cursor.finish("sketch")?;
    Ok(PmDcSketchPayload {
        save_version_major: version,
        header,
        state,
        count_value,
        entities,
        transform,
        direction,
        values,
        auxiliary,
        block,
    })
}

#[derive(Clone, Copy)]
enum SketchEntityTag {
    Point,
    Line,
    Circle,
    Ellipse,
    Spline,
    BlockInstance,
    BlockProxy,
    MirrorNode,
}

impl SketchEntityTag {
    fn from_type_id(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            POINT_TYPE => Some(Self::Point),
            LINE_TYPE => Some(Self::Line),
            CIRCLE_TYPE => Some(Self::Circle),
            ELLIPSE_TYPE => Some(Self::Ellipse),
            SPLINE_TYPE => Some(Self::Spline),
            BLOCK_INSTANCE_TYPE => Some(Self::BlockInstance),
            BLOCK_PROXY_TYPE => Some(Self::BlockProxy),
            MIRROR_NODE_TYPE => Some(Self::MirrorNode),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum SketchConstraintTag {
    Coincident,
    Parallel,
    Perpendicular,
    Tangent,
    Horizontal,
    Vertical,
    HorizontalDistance,
    VerticalDistance,
    Radius,
    Diameter,
    CircleCenter,
    EqualRadius,
    Midpoint,
    AlignedDistance,
    PointsVertical,
    PointsHorizontal,
    Symmetric,
    Fix,
    Collinear,
    EqualLength,
    MirrorPair,
    BlockRelation,
    SplinePoint,
    Offset,
    Angle,
    PointAngle,
}

impl SketchConstraintTag {
    fn from_type_id(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            COINCIDENT_TYPE => Some(Self::Coincident),
            PARALLEL_TYPE => Some(Self::Parallel),
            PERPENDICULAR_TYPE => Some(Self::Perpendicular),
            TANGENT_TYPE => Some(Self::Tangent),
            HORIZONTAL_TYPE => Some(Self::Horizontal),
            VERTICAL_TYPE => Some(Self::Vertical),
            HORIZONTAL_DISTANCE_TYPE => Some(Self::HorizontalDistance),
            VERTICAL_DISTANCE_TYPE => Some(Self::VerticalDistance),
            RADIUS_TYPE => Some(Self::Radius),
            DIAMETER_TYPE => Some(Self::Diameter),
            CIRCLE_CENTER_TYPE => Some(Self::CircleCenter),
            EQUAL_RADIUS_TYPE => Some(Self::EqualRadius),
            MIDPOINT_TYPE => Some(Self::Midpoint),
            ALIGNED_DISTANCE_TYPE => Some(Self::AlignedDistance),
            POINTS_VERTICAL_TYPE => Some(Self::PointsVertical),
            POINTS_HORIZONTAL_TYPE => Some(Self::PointsHorizontal),
            SYMMETRIC_TYPE => Some(Self::Symmetric),
            FIX_TYPE => Some(Self::Fix),
            COLLINEAR_TYPE => Some(Self::Collinear),
            EQUAL_LENGTH_TYPE => Some(Self::EqualLength),
            MIRROR_PAIR_TYPE => Some(Self::MirrorPair),
            BLOCK_RELATION_TYPE => Some(Self::BlockRelation),
            SPLINE_POINT_TYPE => Some(Self::SplinePoint),
            OFFSET_TYPE => Some(Self::Offset),
            ANGLE_TYPE => Some(Self::Angle),
            POINT_ANGLE_TYPE => Some(Self::PointAngle),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum SketchRecordTag {
    Sketch,
    BlockDefinition,
    Transform,
    Direction,
    Entity(SketchEntityTag),
    Constraint(SketchConstraintTag),
}

impl SketchRecordTag {
    fn from_type_id(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            SKETCH_TYPE => Some(Self::Sketch),
            BLOCK_DEFINITION_TYPE => Some(Self::BlockDefinition),
            TRANSFORM_TYPE => Some(Self::Transform),
            DIRECTION_TYPE => Some(Self::Direction),
            _ => SketchEntityTag::from_type_id(type_id)
                .map(Self::Entity)
                .or_else(|| SketchConstraintTag::from_type_id(type_id).map(Self::Constraint)),
        }
    }
}

fn parse_entity(
    ctx: &DecodeContext<'_>,
    tag: SketchEntityTag,
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchEntityPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_flags = cursor.u32("sketch entity flags")?;
    let sketch = cursor.reference("sketch entity sketch reference")?;
    let kind = match tag {
        SketchEntityTag::Point => parse_point(ctx, &mut cursor)?,
        SketchEntityTag::Line => parse_line(ctx, &mut cursor)?,
        SketchEntityTag::Circle => parse_circle(ctx, &mut cursor)?,
        SketchEntityTag::Ellipse => parse_ellipse(ctx, &mut cursor)?,
        SketchEntityTag::Spline => parse_spline(ctx, &mut cursor)?,
        SketchEntityTag::BlockInstance => PmDcSketchEntityKind::BlockInstance {
            definition: cursor.reference("block instance definition reference")?,
            relation: cursor.reference("block instance relation reference")?,
            transform: {
                let value_mask = cursor.u16("block instance value mask")?;
                let zero_mask = cursor.u16("block instance zero mask")?;
                CompactMatrix3::try_new(value_mask, zero_mask, |_| {
                    cursor.f64("block instance transform value")
                })?
            },
            value: cursor.u32("block instance value")?,
        },
        SketchEntityTag::BlockProxy => PmDcSketchEntityKind::BlockProxy {
            instances: {
                let instances = reference_list(ctx, &mut cursor, 2, "block proxy instance list")?;
                if instances.references().len() != 1 {
                    return Err(CodecError::Malformed(
                        "Inventor PmDc block proxy does not name exactly one instance".into(),
                    ));
                }
                instances
            },
            entity: cursor.reference("block proxy entity reference")?,
        },
        SketchEntityTag::MirrorNode => PmDcSketchEntityKind::MirrorNode {
            axis: cursor.reference("sketch mirror axis reference")?,
            data: cursor.reference("sketch mirror data reference")?,
        },
    };
    cursor.finish("sketch entity")?;
    Ok(PmDcSketchEntityPayload {
        save_version_major: version,
        header,
        entity_flags,
        sketch,
        kind,
    })
}

fn point2(cursor: &mut Cursor<'_>, field: &str) -> Result<[FiniteReal; 2], CodecError> {
    Ok([
        cursor.f64(&format!("{field} u"))?,
        cursor.f64(&format!("{field} v"))?,
    ])
}

fn point3(cursor: &mut Cursor<'_>, field: &str) -> Result<[FiniteReal; 3], CodecError> {
    Ok([
        cursor.f64(&format!("{field} x"))?,
        cursor.f64(&format!("{field} y"))?,
        cursor.f64(&format!("{field} z"))?,
    ])
}

fn parse_point(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcSketchEntityKind, CodecError> {
    let position = point2(cursor, "sketch point")?;
    let endpoint_of = reference_list(ctx, cursor, 2, "point endpoint-of list")?;
    let center_of = reference_list(ctx, cursor, 2, "point center-of list")?;
    let tail = if cursor.remaining() == 0 {
        PointTail::Absent
    } else {
        PointTail::Present {
            state: cursor.u32("point tail state")?,
            associations: reference_list(ctx, cursor, 2, "point association list")?,
        }
    };
    Ok(PmDcSketchEntityKind::Point {
        position,
        endpoint_of,
        center_of,
        tail,
    })
}

fn edge_prefix(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    fixed_tail: usize,
    field: &str,
) -> Result<(PmDcReferenceList, Vec<PmDcReferenceList>), CodecError> {
    let points = reference_list(ctx, cursor, 2, &format!("{field} point list"))?;
    let mut auxiliary = Vec::new();
    let tail8 = fixed_tail
        .checked_add(8)
        .ok_or_else(|| CodecError::malformed("Inventor edge tail size overflow"))?;
    let tail16 = fixed_tail
        .checked_add(16)
        .ok_or_else(|| CodecError::malformed("Inventor edge tail size overflow"))?;
    if cursor.remaining() >= tail8 && cursor.peek_u32("edge auxiliary-list marker")? == 0x3000_0002
    {
        auxiliary.push(reference_list(
            ctx,
            cursor,
            2,
            &format!("{field} auxiliary list 0"),
        )?);
    } else if cursor.remaining() >= tail16 {
        let gate = [
            cursor.u32("edge list gate 0")?,
            cursor.u32("edge list gate 1")?,
        ];
        if gate != [1, 0] {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc {field} list gate is {gate:?}"
            )));
        }
        auxiliary.push(reference_list(
            ctx,
            cursor,
            2,
            &format!("{field} auxiliary list 0"),
        )?);
        if cursor.remaining() >= tail8
            && cursor.peek_u32("edge auxiliary-list marker")? == 0x3000_0002
        {
            auxiliary.push(reference_list(
                ctx,
                cursor,
                2,
                &format!("{field} auxiliary list 1"),
            )?);
        }
    }
    if cursor.remaining() != fixed_tail {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} has {} bytes before its fixed tail, expected {fixed_tail}",
            cursor.remaining()
        )));
    }
    Ok((points, auxiliary))
}

fn parse_line(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcSketchEntityKind, CodecError> {
    let (points, auxiliary) = edge_prefix(ctx, cursor, 32, "line")?;
    let origin = point2(cursor, "line origin")?;
    let direction = point2(cursor, "line direction")?;
    Ok(PmDcSketchEntityKind::Line {
        points,
        auxiliary,
        origin,
        direction,
    })
}

fn parse_circle(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcSketchEntityKind, CodecError> {
    let (points, auxiliary) = edge_prefix(ctx, cursor, 13, "circle")?;
    let center = cursor.reference("circle center reference")?;
    let radius = cursor.f64("circle radius")?;
    let state = cursor.u8("circle state")?;
    let radius = PositiveReal::new(radius.get()).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc circle radius is not positive".into())
    })?;
    Ok(PmDcSketchEntityKind::Circle {
        points,
        auxiliary,
        center,
        radius,
        state,
    })
}

fn parse_ellipse(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcSketchEntityKind, CodecError> {
    let (points, auxiliary) = edge_prefix(ctx, cursor, 37, "ellipse")?;
    let center = cursor.reference("ellipse center reference")?;
    let major_direction = point2(cursor, "ellipse major direction")?;
    let major_radius = cursor.f64("ellipse major radius")?;
    let minor_radius = cursor.f64("ellipse minor radius")?;
    let state = cursor.u8("ellipse state")?;
    let major_radius = PositiveReal::new(major_radius.get()).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc ellipse radius is not positive".into())
    })?;
    let minor_radius = PositiveReal::new(minor_radius.get()).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc ellipse radius is not positive".into())
    })?;
    Ok(PmDcSketchEntityKind::Ellipse {
        points,
        auxiliary,
        center,
        major_direction,
        major_radius,
        minor_radius,
        state,
    })
}

fn parse_spline(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcSketchEntityKind, CodecError> {
    let points = reference_list(ctx, cursor, 2, "spline point list")?;
    let auxiliary = reference_list(ctx, cursor, 2, "spline auxiliary list")?;
    let curve = match cursor.u32("spline curve selector")? {
        SPLINE_STORED => {
            let values = [cursor.u32("spline value 0")?, cursor.u32("spline value 1")?];
            let degree = cursor.u32("spline degree")?;
            let knot_tolerance = cursor.f64("spline knot tolerance")?;
            let (knot_capacity, knots) = scalar_array(ctx, cursor, 1, "spline knots")?;
            let (weight_capacity, weights) = scalar_array(ctx, cursor, 1, "spline weights")?;
            let (pole_capacity, coordinates) = scalar_array(ctx, cursor, 2, "spline poles")?;
            if !weights.is_empty() && weights.len() * 2 != coordinates.len() {
                return Err(CodecError::Malformed(
                    "Inventor PmDc sketch spline weight count differs from its pole count".into(),
                ));
            }
            let mut poles =
                ctx.vector_storage(coordinates.len() / 2, "admit Inventor spline poles")?;
            for pair in coordinates.chunks_exact(2) {
                poles.push([pair[0], pair[1]]);
            }
            let pole_tolerance = cursor.f64("spline pole tolerance")?;
            let flags = [cursor.u32("spline flag 0")?, cursor.u32("spline flag 1")?];
            let range = [
                cursor.f64("spline range start")?,
                cursor.f64("spline range end")?,
            ];
            let (_, trailing) = scalar_array(ctx, cursor, 1, "spline trailing array")?;
            if !trailing.is_empty() {
                return Err(CodecError::Malformed(
                    "Inventor PmDc sketch spline trailing array is not empty".into(),
                ));
            }
            PmDcSplineCurve::Stored {
                values,
                degree,
                knot_tolerance,
                knot_capacity,
                knots,
                weight_capacity,
                weights,
                pole_capacity,
                poles,
                pole_tolerance,
                flags,
                range,
            }
        }
        SPLINE_UNSTORED => PmDcSplineCurve::Unstored {
            value: cursor.u32("spline value")?,
            key: cursor.u32("spline key")?,
        },
        selector => {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc sketch spline selector {selector} is outside the supported grammar"
            )));
        }
    };
    Ok(PmDcSketchEntityKind::Spline {
        points,
        auxiliary,
        curve,
    })
}

/// Spline curve selector for a stored B-spline.
const SPLINE_STORED: u32 = 4;
/// Spline curve selector for a curve without stored geometry.
const SPLINE_UNSTORED: u32 = 5;

/// A counted f64 array: `count:u32`, `capacity:u32`, `element_size:u32 = 8`,
/// then `count * width` values. Returns the capacity and the values.
fn scalar_array(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    width: usize,
    field: &'static str,
) -> Result<(u32, Vec<FiniteReal>), CodecError> {
    let count = cursor.u32(field)?;
    let capacity = cursor.u32(field)?;
    let element = cursor.u32(field)?;
    if element != 8 {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} element size is {element}"
        )));
    }
    let values = usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(width))
        .ok_or_else(|| CodecError::malformed("Inventor PmDc array length overflows"))?;
    if values
        .checked_mul(8)
        .is_none_or(|bytes| bytes > cursor.remaining())
    {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} count {count} exceeds the record"
        )));
    }
    let capacity_items = usize::try_from(capacity).map_err(|_| {
        CodecError::malformed("Inventor PmDc array capacity exceeds the target range")
    })?;
    if width == 1 && values > capacity_items {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} count {count} exceeds capacity {capacity}"
        )));
    }
    let mut result = ctx.vector_storage(values, "admit Inventor PmDc scalar array")?;
    for _ in 0..values {
        result.push(cursor.f64(field)?);
    }
    Ok((capacity, result))
}

fn parse_transform(source: View<'_>, version: u8) -> Result<PmDcTransformPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let prefix_present = cursor.peek_u32("transform prefix")? == TRANSFORM_PREFIX;
    if prefix_present {
        cursor.u32("transform prefix")?;
    }
    let value_mask = cursor.u16("transform value mask")?;
    let zero_mask = cursor.u16("transform zero mask")?;
    let matrix = CompactMatrix::try_new(value_mask, zero_mask, |_| {
        cursor.f64("transform explicit value")
    })?;
    cursor.finish("transform")?;
    Ok(PmDcTransformPayload {
        save_version_major: version,
        header,
        prefix_present,
        matrix,
    })
}

fn parse_direction(source: View<'_>, version: u8) -> Result<PmDcDirectionPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_flags = cursor.u32("direction entity flags")?;
    let parameter = cursor.f64("direction parameter")?;
    let extension = match cursor.remaining() {
        24 => None,
        28 => Some(cursor.u32("direction extension value")?),
        remaining => {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc direction has {remaining} bytes before its vector"
            )));
        }
    };
    let direction = point3(&mut cursor, "direction vector")?;
    cursor.finish("direction")?;
    Ok(PmDcDirectionPayload {
        save_version_major: version,
        header,
        entity_flags,
        parameter,
        extension,
        direction,
    })
}

fn map_header(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    field: &str,
) -> Result<(usize, Option<[u32; 2]>), CodecError> {
    let marker = [
        cursor.u16("constraint map marker 0")?,
        cursor.u16("constraint map marker 1")?,
    ];
    if marker != [6, 0x3000] {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} marker is {marker:?}"
        )));
    }
    let count = usize::try_from(cursor.u32("constraint map count")?)
        .map_err(|_| CodecError::Malformed("Inventor numeric value exceeds target range".into()))?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(count),
        "admit Inventor sketch constraint map",
    )?;
    let metadata = (count != 0)
        .then(|| {
            Ok::<_, CodecError>([
                cursor.u32("constraint map metadata 0")?,
                cursor.u32("constraint map metadata 1")?,
            ])
        })
        .transpose()?;
    Ok((count, metadata))
}

fn reference_scalar_map(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcReferenceScalarMap, CodecError> {
    let (count, metadata) = map_header(ctx, cursor, "constraint scalar map")?;
    let mut entries = ctx.vector_storage(count, "admit Inventor sketch constraint map")?;
    for index in 0..count {
        entries.push((
            cursor.reference("constraint scalar-map key")?,
            cursor.f64(&format!("constraint scalar-map value {index}"))?,
        ));
    }
    PmDcReferenceScalarMap::new(metadata, entries).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc scalar map metadata disagrees with length".into())
    })
}

fn reference_pair_map(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcReferenceSelectorMap, CodecError> {
    let (count, metadata) = map_header(ctx, cursor, "constraint reference map")?;
    let mut entries = ctx.vector_storage(count, "admit Inventor sketch constraint map")?;
    for _ in 0..count {
        entries.push((
            cursor.reference("constraint reference-map key")?,
            cursor.u32("constraint endpoint selector")?,
        ));
    }
    PmDcReferenceSelectorMap::new(metadata, entries).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc pair map metadata disagrees with length".into())
    })
}

fn parse_constraint_header(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    version: u8,
    has_parameter: bool,
) -> Result<PmDcConstraintHeader, CodecError> {
    let content = content_header(cursor)?;
    let state = cursor.u32("constraint state")?.cast_signed();
    let group = cursor.reference("constraint group reference")?;
    let (scalar_map, reference_map) = if version <= 16 {
        (
            PmDcReferenceScalarMap::empty(),
            PmDcReferenceSelectorMap::empty(),
        )
    } else {
        (
            reference_scalar_map(ctx, cursor)?,
            reference_pair_map(ctx, cursor)?,
        )
    };
    Ok(PmDcConstraintHeader {
        content,
        state,
        group,
        scalar_map,
        reference_map,
        parameter: if has_parameter {
            cursor.reference("constraint parameter reference")?
        } else {
            PmDcReference::from_packed(0)
        },
    })
}

fn parse_constraint(
    ctx: &DecodeContext<'_>,
    tag: SketchConstraintTag,
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchConstraintPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = parse_constraint_header(
        ctx,
        &mut cursor,
        version,
        !matches!(tag, SketchConstraintTag::BlockRelation),
    )?;
    let kind = match tag {
        SketchConstraintTag::Coincident => PmDcSketchConstraintKind::Coincident {
            first: cursor.reference("coincident constraint first reference")?,
            second: cursor.reference("coincident constraint second reference")?,
        },
        SketchConstraintTag::Parallel => PmDcSketchConstraintKind::Parallel {
            first: cursor.reference("parallel constraint first reference")?,
            second: cursor.reference("parallel constraint second reference")?,
            orientation: cursor.u16("parallel constraint orientation")?,
        },
        SketchConstraintTag::Perpendicular => PmDcSketchConstraintKind::Perpendicular {
            first: cursor.reference("perpendicular constraint first reference")?,
            second: cursor.reference("perpendicular constraint second reference")?,
            orientation: cursor.u16("perpendicular constraint orientation")?,
        },
        SketchConstraintTag::Tangent => PmDcSketchConstraintKind::Tangent {
            first: cursor.reference("tangent constraint first reference")?,
            second: cursor.reference("tangent constraint second reference")?,
            extension: (cursor.remaining() == 4)
                .then(|| cursor.u32("tangent constraint extension"))
                .transpose()?,
        },
        SketchConstraintTag::Horizontal => PmDcSketchConstraintKind::Horizontal {
            entity: cursor.reference("horizontal constraint entity reference")?,
            state: cursor.u8("horizontal constraint state")?,
        },
        SketchConstraintTag::Vertical => PmDcSketchConstraintKind::Vertical {
            entity: cursor.reference("vertical constraint entity reference")?,
            state: cursor.u8("vertical constraint state")?,
        },
        SketchConstraintTag::HorizontalDistance => {
            let (first, second, parameter, values) = distance_constraint_fields(&mut cursor)?;
            PmDcSketchConstraintKind::HorizontalDistance {
                first,
                second,
                parameter,
                values,
            }
        }
        SketchConstraintTag::VerticalDistance => {
            let (first, second, parameter, values) = distance_constraint_fields(&mut cursor)?;
            PmDcSketchConstraintKind::VerticalDistance {
                first,
                second,
                parameter,
                values,
            }
        }
        SketchConstraintTag::Radius => PmDcSketchConstraintKind::Radius {
            state: cursor.u32("radius constraint state")?,
            entity: cursor.reference("radius constraint entity reference")?,
            values: cursor.u32_array::<4>("radius constraint values")?,
        },
        SketchConstraintTag::Diameter => PmDcSketchConstraintKind::Diameter {
            reference: cursor.reference("diameter constraint reference")?,
            entity: cursor.reference("diameter constraint entity reference")?,
            values: cursor.u32_array::<4>("diameter constraint values")?,
        },
        SketchConstraintTag::CircleCenter => PmDcSketchConstraintKind::CircleCenter {
            entity: cursor.reference("circle-center constraint entity reference")?,
            center: cursor.reference("circle-center constraint center reference")?,
        },
        SketchConstraintTag::EqualRadius => PmDcSketchConstraintKind::EqualRadius {
            first: cursor.reference("equal-radius constraint first reference")?,
            second: cursor.reference("equal-radius constraint second reference")?,
        },
        SketchConstraintTag::Midpoint => PmDcSketchConstraintKind::Midpoint {
            curve: cursor.reference("midpoint constraint curve reference")?,
            point: cursor.reference("midpoint constraint point reference")?,
        },
        SketchConstraintTag::AlignedDistance => PmDcSketchConstraintKind::AlignedDistance {
            value: cursor.u32("aligned-distance value")?,
            first: cursor.reference("aligned-distance first reference")?,
            second: cursor.reference("aligned-distance second reference")?,
            extent: [
                cursor.f64("aligned-distance extent 0")?,
                cursor.f64("aligned-distance extent 1")?,
                cursor.f64("aligned-distance extent 2")?,
                cursor.f64("aligned-distance extent 3")?,
                cursor.f64("aligned-distance extent 4")?,
                cursor.f64("aligned-distance extent 5")?,
            ],
        },
        SketchConstraintTag::PointsVertical => PmDcSketchConstraintKind::PointsVertical {
            first: cursor.reference("vertical alignment first reference")?,
            second: cursor.reference("vertical alignment second reference")?,
        },
        SketchConstraintTag::PointsHorizontal => PmDcSketchConstraintKind::PointsHorizontal {
            first: cursor.reference("horizontal alignment first reference")?,
            second: cursor.reference("horizontal alignment second reference")?,
        },
        SketchConstraintTag::Symmetric => PmDcSketchConstraintKind::Symmetric {
            first: cursor.reference("symmetric constraint first reference")?,
            second: cursor.reference("symmetric constraint second reference")?,
            axis: cursor.reference("symmetric constraint axis reference")?,
        },
        SketchConstraintTag::Fix => PmDcSketchConstraintKind::Fix {
            entity: cursor.reference("fix constraint entity reference")?,
        },
        SketchConstraintTag::Collinear => PmDcSketchConstraintKind::Collinear {
            first: cursor.reference("collinear constraint first reference")?,
            second: cursor.reference("collinear constraint second reference")?,
            orientation: cursor.u16("collinear constraint orientation")?,
        },
        SketchConstraintTag::EqualLength => PmDcSketchConstraintKind::EqualLength {
            first: cursor.reference("equal-length constraint first reference")?,
            second: cursor.reference("equal-length constraint second reference")?,
        },
        SketchConstraintTag::MirrorPair => PmDcSketchConstraintKind::MirrorPair {
            result: cursor.reference("mirror pair result reference")?,
            source: cursor.reference("mirror pair source reference")?,
            axis: cursor.reference("mirror pair axis reference")?,
            mirror: [
                cursor.reference("mirror pair node reference 0")?,
                cursor.reference("mirror pair node reference 1")?,
            ],
            value: cursor.u32("mirror pair value")?,
            data: cursor.reference("mirror pair data reference")?,
        },
        SketchConstraintTag::BlockRelation => PmDcSketchConstraintKind::BlockRelation {
            members: reference_list(ctx, &mut cursor, 2, "block relation member list")?,
        },
        SketchConstraintTag::SplinePoint => PmDcSketchConstraintKind::SplinePoint {
            spline: cursor.reference("spline point spline reference")?,
            point: cursor.reference("spline point point reference")?,
        },
        SketchConstraintTag::Offset => PmDcSketchConstraintKind::Offset {
            pairs: [
                [
                    cursor.reference("offset source reference 0")?,
                    cursor.reference("offset result reference 0")?,
                ],
                [
                    cursor.reference("offset source reference 1")?,
                    cursor.reference("offset result reference 1")?,
                ],
            ],
        },
        SketchConstraintTag::Angle => PmDcSketchConstraintKind::Angle {
            value: cursor.u32("angle dimension value")?,
            first: cursor.reference("angle dimension first reference")?,
            second: cursor.reference("angle dimension second reference")?,
            label: [
                cursor.f64("angle dimension label u")?,
                cursor.f64("angle dimension label v")?,
            ],
            quadrant: cursor.u16("angle dimension quadrant")?,
        },
        SketchConstraintTag::PointAngle => PmDcSketchConstraintKind::PointAngle {
            value: cursor.u32("point angle value")?,
            center: cursor.reference("point angle center reference")?,
            first: cursor.reference("point angle first reference")?,
            second: cursor.reference("point angle second reference")?,
            label: [
                cursor.f64("point angle label u")?,
                cursor.f64("point angle label v")?,
            ],
            state: NativeBytes::new(cursor.take_array::<3>("point angle state")?),
        },
    };
    cursor.finish("sketch constraint")?;
    Ok(PmDcSketchConstraintPayload {
        save_version_major: version,
        header,
        kind,
    })
}

fn distance_constraint_fields(
    cursor: &mut Cursor<'_>,
) -> Result<(PmDcReference, PmDcReference, PmDcReference, [u32; 4]), CodecError> {
    let first = cursor.reference("distance constraint first reference")?;
    let second = cursor.reference("distance constraint second reference")?;
    let parameter = cursor.reference("distance constraint parameter reference")?;
    let values = cursor.u32_array::<4>("distance constraint values")?;
    Ok((first, second, parameter, values))
}

type RecordKey<'a> = (&'a str, u32);

/// A unique record table and the reservation that owns its storage.
type RecordTable<'a, T> = (
    HashMap<RecordKey<'a>, Option<&'a Located<T>>>,
    cadmpeg_core::decode::ScopedReservation<'a>,
);

fn record_index<'a, T>(
    ctx: &'a DecodeContext<'_>,
    records: &'a [Located<T>],
    operation: &'static str,
) -> Result<RecordTable<'a, T>, CodecError>
where
    T: RecordPayload,
{
    ctx.unique_index(
        records.iter().map(|record| {
            (
                (
                    record.identity.segment_token.as_str(),
                    record.identity.record_ordinal,
                ),
                record,
            )
        }),
        |key| {
            cadmpeg_core::decode::u64_from_index(key.0.len())
                .checked_add(5)
                .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))
        },
        operation,
    )
}

fn id_len_error() -> CodecError {
    CodecError::Malformed("Inventor identifier length exceeds address space".into())
}

/// Whether a sketch-entity record is a helper relation record rather than
/// geometry: a block instance or a sketch mirror node.
fn is_helper(entity: &PmDcSketchEntity) -> bool {
    matches!(
        entity.kind,
        PmDcSketchEntityKind::BlockInstance { .. } | PmDcSketchEntityKind::MirrorNode { .. }
    )
}

/// One solved sketch entity with the endpoint keys that join it into profiles.
struct ProjectedEntity<'a> {
    entity: SketchEntity,
    /// Profile adjacency keys of the stored start and end points.
    profile_keys: Vec<String>,
    /// Millimetre trace from the stored start to the stored end.
    trace: Vec<[f64; 2]>,
    /// Segment token of the source record.
    token: &'a str,
    /// Block instance that owns a copied definition entity.
    instance: Option<u32>,
    /// Source record ordinal: the sketch member, or the copied definition member.
    ordinal: u32,
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    inventory: &SketchInventory,
    parameters: &[DesignParameter],
) -> Result<SketchProjection, CodecError> {
    let (raw_sketches, _raw_sketches_storage) =
        record_index(ctx, &inventory.sketches, "index Inventor sketches")?;
    let (raw_entities, _raw_entities_storage) =
        record_index(ctx, &inventory.entities, "index Inventor sketch entities")?;
    let (transforms, _transforms_storage) = record_index(
        ctx,
        &inventory.transforms,
        "index Inventor sketch transforms",
    )?;
    let (directions, _directions_storage) = record_index(
        ctx,
        &inventory.directions,
        "index Inventor sketch directions",
    )?;
    let (raw_constraints, _raw_constraints_storage) = record_index(
        ctx,
        &inventory.constraints,
        "index Inventor sketch constraints",
    )?;
    let mut parameter_index = HashMap::new();
    for parameter in parameters {
        if let Some(native) = &parameter.native_ref {
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(native.len()),
                "retain Inventor sketch parameter native id",
            )?;
            let id = parameter
                .id
                .try_clone_for_decode(ctx, "retain Inventor sketch parameter id")?;
            ctx.insert_hash_map(
                &mut parameter_index,
                native.clone(),
                id,
                "index Inventor sketch parameter",
            )?;
        }
    }
    let lookup = Lookup {
        entities: &raw_entities,
        sketches: &raw_sketches,
    };
    // Block proxies by owning instance and copied definition member.
    let mut proxies = HashMap::new();
    for entity in &inventory.entities {
        if let PmDcSketchEntityKind::BlockProxy {
            instances,
            entity: member,
        } = &entity.kind
        {
            if let (Some(instance), Some(member)) = (
                instances
                    .references()
                    .first()
                    .and_then(|instance| instance.record_ordinal()),
                member.record_ordinal(),
            ) {
                ctx.insert_hash_map(
                    &mut proxies,
                    (entity.identity.segment_token.as_str(), instance, member),
                    entity.identity.record_ordinal,
                    "index Inventor sketch block proxies",
                )?;
            }
        }
    }

    // Stage 1: sketch members and block-instance copies.
    let mut projected = Vec::new();
    for entity in &inventory.entities {
        let key = (
            entity.identity.segment_token.as_str(),
            entity.identity.record_ordinal,
        );
        if is_helper(entity) {
            continue;
        }
        if !raw_entities.contains_key(&key) {
            continue;
        }
        let Some(sketch_ordinal) = entity.sketch.index().checked_sub(1) else {
            continue;
        };
        let Some(sketch) = raw_sketches
            .get(&(entity.identity.segment_token.as_str(), sketch_ordinal))
            .and_then(Option::as_ref)
        else {
            continue;
        };
        if sketch.block.is_some() {
            // Block-definition members transfer as instance copies below.
            continue;
        }
        if !sketch.entities.references().iter().any(|reference| {
            entity
                .identity
                .record_ordinal
                .checked_add(1)
                .is_some_and(|next| reference.index() == next)
        }) {
            continue;
        }
        let Some(member) = project_member(ctx, &lookup, &proxies, entity, sketch, None)? else {
            continue;
        };
        projected.push(member);
    }
    for instance in &inventory.entities {
        let PmDcSketchEntityKind::BlockInstance { definition, .. } = &instance.kind else {
            continue;
        };
        let token = instance.identity.segment_token.as_str();
        let (Some(host), Some(definition)) = (
            instance
                .sketch
                .record_ordinal()
                .and_then(|ordinal| raw_sketches.get(&(token, ordinal)))
                .and_then(Option::as_ref),
            definition
                .record_ordinal()
                .and_then(|ordinal| raw_sketches.get(&(token, ordinal)))
                .and_then(Option::as_ref),
        ) else {
            continue;
        };
        if host.block.is_some() || definition.block.is_none() {
            continue;
        }
        for reference in definition.entities.references() {
            ctx.charge_work(1, "expand Inventor sketch block instance")?;
            let Some(ordinal) = reference.record_ordinal() else {
                continue;
            };
            let Some(member) = raw_entities.get(&(token, ordinal)).and_then(Option::as_ref) else {
                continue;
            };
            if is_helper(member)
                || matches!(member.kind, PmDcSketchEntityKind::BlockProxy { .. })
                || proxies.contains_key(&(token, instance.identity.record_ordinal, ordinal))
            {
                continue;
            }
            if let Some(copy) = project_member(
                ctx,
                &lookup,
                &proxies,
                member,
                host,
                Some(instance.identity.record_ordinal),
            )? {
                projected.push(copy);
            }
        }
    }

    // Stage 2: sketches.
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(projected.len()),
        "index projected Inventor sketch entities",
    )?;
    let mut by_sketch = HashMap::<&str, Vec<usize>>::new();
    for (index, member) in projected.iter().enumerate() {
        ctx.push_hash_group(
            &mut by_sketch,
            member.entity.sketch.as_str(),
            index,
            "group projected Inventor sketch entities",
            "collect projected Inventor sketch entity",
        )?;
    }
    let mut projected_member_keys = HashSet::new();
    for member in &projected {
        if member.instance.is_none() {
            ctx.insert_hash_set(
                &mut projected_member_keys,
                (member.token, member.ordinal),
                "index projected Inventor sketch entity key",
            )?;
        }
    }
    let mut sketches = Vec::new();
    for sketch in &inventory.sketches {
        if sketch.block.is_some() {
            continue;
        }
        let key = (
            sketch.identity.segment_token.as_str(),
            sketch.identity.record_ordinal,
        );
        if !raw_sketches.contains_key(&key) {
            continue;
        }
        let Some(placement) = project_placement(sketch, &transforms, &directions) else {
            continue;
        };
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(
                projected_id_len(
                    "inventor:design:sketch#",
                    sketch.identity.segment_token.as_str(),
                    sketch.identity.record_ordinal,
                )
                .ok_or_else(id_len_error)?,
            ),
            "retain projected Inventor sketch id",
        )?;
        let Some(id) = sketch_id(sketch) else {
            continue;
        };
        let members = by_sketch
            .get(id.as_str())
            .map(|indices| {
                indices
                    .iter()
                    .map(|index| &projected[*index])
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let Ok(profiles) =
            cadmpeg_ir::sketches::SketchProfiles::try_from(build_member_profiles(ctx, &members)?)
        else {
            continue;
        };
        ctx.charge_collection_items(1, "project Inventor sketch")?;
        ctx.charge_entities(1, "project Inventor sketch")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(
                record_id_len(
                    sketch.identity.segment_token.as_str(),
                    sketch.identity.record_ordinal,
                    "sketch",
                )
                .ok_or_else(id_len_error)?,
            ),
            "retain projected Inventor sketch native reference",
        )?;
        sketches.push((
            sketch,
            Sketch {
                id,
                name: None,
                configuration: None,
                visible: None,
                placement,
                profiles,
                native_ref: Some(sketch.id()),
            },
        ));
    }

    // Stage 3: constraints and helper relations.
    let mut constraints = Vec::new();
    let mut projected_constraint_keys = HashSet::new();
    {
        let mut host_entities = HashMap::new();
        for member in &projected {
            if member.instance.is_none() {
                ctx.insert_hash_map(
                    &mut host_entities,
                    (member.token, member.ordinal),
                    &member.entity,
                    "index projected Inventor sketch entity key",
                )?;
            }
        }
        for constraint in &inventory.constraints {
            let key = (
                constraint.identity.segment_token.as_str(),
                constraint.identity.record_ordinal,
            );
            if !raw_constraints.contains_key(&key) {
                continue;
            }
            if let Some(result) =
                project_constraint(ctx, constraint, &host_entities, &parameter_index, None)
                    .transpose()?
            {
                ctx.charge_collection_items(1, "project Inventor sketch constraint")?;
                ctx.insert_hash_set(
                    &mut projected_constraint_keys,
                    key,
                    "index projected Inventor sketch constraint",
                )?;
                constraints.push(result);
            }
        }
        for helper in inventory.entities.iter().filter(|entity| is_helper(entity)) {
            if let Some(result) = project_helper(ctx, &lookup, helper, &host_entities, &projected)?
            {
                ctx.charge_collection_items(1, "project Inventor sketch constraint")?;
                ctx.insert_hash_set(
                    &mut projected_constraint_keys,
                    (
                        helper.identity.segment_token.as_str(),
                        helper.identity.record_ordinal,
                    ),
                    "index projected Inventor sketch constraint",
                )?;
                constraints.push(result);
            }
        }
        // Block-definition relations hold for every instance copy whose
        // placement keeps the sketch axes.
        for instance in &inventory.entities {
            let PmDcSketchEntityKind::BlockInstance {
                definition,
                transform,
                ..
            } = &instance.kind
            else {
                continue;
            };
            let token = instance.identity.segment_token.as_str();
            let Some(definition) = definition
                .record_ordinal()
                .and_then(|ordinal| raw_sketches.get(&(token, ordinal)))
                .and_then(Option::as_ref)
            else {
                continue;
            };
            if definition.block.is_none() {
                continue;
            }
            let Some(frame) = Frame::from_matrix(transform.rows()) else {
                continue;
            };
            let mut copies = HashMap::new();
            for member in &projected {
                if member.instance == Some(instance.identity.record_ordinal) {
                    ctx.insert_hash_map(
                        &mut copies,
                        (token, member.ordinal),
                        &member.entity,
                        "index Inventor sketch block copies",
                    )?;
                }
            }
            for (key, entity) in &host_entities {
                if let Some(Some(proxy)) = raw_entities.get(key) {
                    if let PmDcSketchEntityKind::BlockProxy {
                        instances,
                        entity: member,
                    } = &proxy.kind
                    {
                        if instances
                            .references()
                            .first()
                            .and_then(|owner| owner.record_ordinal())
                            == Some(instance.identity.record_ordinal)
                        {
                            if let Some(member) = member.record_ordinal() {
                                ctx.insert_hash_map(
                                    &mut copies,
                                    (token, member),
                                    *entity,
                                    "index Inventor sketch block copies",
                                )?;
                            }
                        }
                    }
                }
            }
            for reference in definition.entities.references() {
                ctx.charge_work(1, "expand Inventor sketch block relations")?;
                let Some(constraint) = reference
                    .record_ordinal()
                    .and_then(|ordinal| raw_constraints.get(&(token, ordinal)))
                    .and_then(Option::as_ref)
                else {
                    continue;
                };
                if !frame.preserves_axes() && constraint.kind.depends_on_axes() {
                    continue;
                }
                if let Some(result) = project_constraint(
                    ctx,
                    constraint,
                    &copies,
                    &parameter_index,
                    Some(instance.identity.record_ordinal),
                )
                .transpose()?
                {
                    ctx.charge_collection_items(1, "project Inventor sketch constraint")?;
                    constraints.push(result);
                }
            }
        }
    }

    // Stage 4: closure. A sketch transfers when every member of its entity
    // array projected as an entity or a relation.
    sketches.retain(|(raw, _)| {
        let mut seen = HashSet::new();
        let closed = raw.entities.references().iter().all(|reference| {
            let Some(ordinal) = reference.record_ordinal() else {
                return false;
            };
            if !seen.insert(ordinal) {
                return false;
            }
            let key = (raw.identity.segment_token.as_str(), ordinal);
            projected_member_keys.contains(&key) || projected_constraint_keys.contains(&key)
        });
        closed
    });
    let closed_sketch_ids = sketches
        .iter()
        .map(|(_, sketch)| sketch.id.as_str().to_owned())
        .collect::<HashSet<_>>();
    projected.retain(|member| closed_sketch_ids.contains(member.entity.sketch.as_str()));
    constraints.retain(|constraint| closed_sketch_ids.contains(constraint.sketch.as_str()));
    // A source record transfers when a kept entity or relation represents it.
    let mut represented = HashSet::new();
    for member in &projected {
        for native in [&member.entity.native_ref, &member.entity.geometry_ref]
            .into_iter()
            .flatten()
        {
            ctx.insert_hash_set(
                &mut represented,
                native.clone(),
                "index represented Inventor sketch records",
            )?;
        }
    }
    for constraint in &constraints {
        if let Some(native) = &constraint.native_ref {
            ctx.insert_hash_set(
                &mut represented,
                native.clone(),
                "index represented Inventor sketch records",
            )?;
        }
    }
    let unresolved_entities = inventory
        .entities
        .iter()
        .filter(|entity| !represented.contains(&entity.id()))
        .count();
    let unresolved_constraints = inventory
        .constraints
        .iter()
        .filter(|constraint| !represented.contains(&constraint.id()))
        .count();
    let mut represented_blocks = 0usize;
    for sketch in inventory
        .sketches
        .iter()
        .filter(|sketch| sketch.block.is_some())
    {
        let token = sketch.identity.segment_token.as_str();
        if sketch.entities.references().iter().any(|reference| {
            reference.record_ordinal().is_some_and(|ordinal| {
                represented.contains(&format!("inventor:pmdc:sketch-entity#{token}-{ordinal}"))
            })
        }) {
            represented_blocks += 1;
        }
    }
    let unresolved_sketches = inventory
        .sketches
        .len()
        .checked_sub(sketches.len() + represented_blocks)
        .ok_or_else(|| CodecError::malformed("Inventor sketches exceed inventory"))?;
    Ok(SketchProjection {
        sketches: sketches.into_iter().map(|(_, sketch)| sketch).collect(),
        entities: projected.into_iter().map(|member| member.entity).collect(),
        constraints,
        unresolved_sketches,
        unresolved_entities,
        unresolved_constraints,
    })
}

fn project_constraint(
    ctx: &DecodeContext<'_>,
    constraint: &PmDcSketchConstraint,
    entities: &HashMap<RecordKey<'_>, &SketchEntity>,
    parameters: &HashMap<String, ParameterId>,
    scope: Option<u32>,
) -> Option<Result<SketchConstraint, CodecError>> {
    macro_rules! admit {
        ($result:expr) => {
            if let Err(error) = $result {
                return Some(Err(error));
            }
        };
    }

    macro_rules! admitted_value {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            }
        };
    }
    let resolve = |reference: PmDcReference| {
        entities
            .get(&(
                constraint.identity.segment_token.as_str(),
                reference.index().checked_sub(1)?,
            ))
            .copied()
    };
    let id = |entity: &SketchEntity| {
        entity
            .id()
            .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")
    };
    let selector = |reference: PmDcReference| {
        constraint
            .header
            .reference_map
            .entries()
            .iter()
            .find(|(key, _)| key.index() == reference.index())
            .map_or(0, |(_, value)| *value)
    };
    // A dimension whose driving parameter did not transfer keeps its exact
    // operands as a source-native relation that names the parameter record.
    macro_rules! driving {
        ($reference:expr, $kind:literal, $members:expr) => {
            match resolve_parameter(ctx, constraint, $reference, parameters) {
                Some(result) => admitted_value!(result),
                None => {
                    let definition = admitted_value!(native_relation(
                        ctx,
                        constraint,
                        $kind,
                        &$members,
                        &[("parameter", $reference)],
                    ));
                    return finish_constraint(ctx, constraint, scope, definition, None, &$members);
                }
            }
        };
    }
    let (definition, orientation, members) = match &constraint.kind {
        PmDcSketchConstraintKind::Coincident { first, second } => {
            let members = vec![resolve(*first)?, resolve(*second)?];
            let definition = admitted_value!(coincident_definition(
                ctx,
                [members[0], members[1]],
                [selector(*first), selector(*second)],
            ));
            (definition, None, members)
        }
        PmDcSketchConstraintKind::Parallel {
            first,
            second,
            orientation,
        } => {
            let members = vec![resolve(*first)?, resolve(*second)?];
            (
                SketchConstraintDefinitionInput::Parallel {
                    first: admitted_value!(id(members[0])),
                    second: admitted_value!(id(members[1])),
                },
                Some(u32::from(*orientation)),
                members,
            )
        }
        PmDcSketchConstraintKind::Perpendicular {
            first,
            second,
            orientation,
        } => {
            let members = vec![resolve(*first)?, resolve(*second)?];
            (
                SketchConstraintDefinitionInput::Perpendicular {
                    first: admitted_value!(id(members[0])),
                    second: admitted_value!(id(members[1])),
                },
                Some(u32::from(*orientation)),
                members,
            )
        }
        PmDcSketchConstraintKind::Tangent {
            first,
            second,
            extension,
        } => {
            let members = vec![resolve(*first)?, resolve(*second)?];
            let loci = [
                endpoint_locus(members[0], selector(*first)),
                endpoint_locus(members[1], selector(*second)),
            ];
            let definition = match loci {
                [Some(first_locus), Some(second_locus)] => {
                    SketchConstraintDefinitionInput::TangentLoci {
                        first: admitted_value!(first_locus(ctx)),
                        second: admitted_value!(second_locus(ctx)),
                    }
                }
                _ => SketchConstraintDefinitionInput::Tangent {
                    first: admitted_value!(id(members[0])),
                    second: admitted_value!(id(members[1])),
                },
            };
            (definition, *extension, members)
        }
        PmDcSketchConstraintKind::Horizontal { entity, state } => {
            let member = resolve(*entity)?;
            (
                SketchConstraintDefinitionInput::Horizontal {
                    entity: admitted_value!(id(member)),
                },
                Some(u32::from(*state)),
                vec![member],
            )
        }
        PmDcSketchConstraintKind::Vertical { entity, state } => {
            let member = resolve(*entity)?;
            (
                SketchConstraintDefinitionInput::Vertical {
                    entity: admitted_value!(id(member)),
                },
                Some(u32::from(*state)),
                vec![member],
            )
        }
        PmDcSketchConstraintKind::HorizontalDistance {
            first,
            second,
            parameter,
            ..
        } => {
            let members = vec![resolve(*first)?, resolve(*second)?];
            let parameter = driving!(*parameter, "horizontal_distance", members);
            (
                SketchConstraintDefinitionInput::HorizontalDistance {
                    first: SketchLocus::Entity(admitted_value!(id(members[0]))),
                    second: SketchLocus::Entity(admitted_value!(id(members[1]))),
                    parameter,
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::VerticalDistance {
            first,
            second,
            parameter,
            ..
        } => {
            let members = vec![resolve(*first)?, resolve(*second)?];
            let parameter = driving!(*parameter, "vertical_distance", members);
            (
                SketchConstraintDefinitionInput::VerticalDistance {
                    first: SketchLocus::Entity(admitted_value!(id(members[0]))),
                    second: SketchLocus::Entity(admitted_value!(id(members[1]))),
                    parameter,
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Radius { entity, .. } => {
            let member = resolve(*entity)?;
            let parameter = driving!(constraint.header.parameter, "radius", [member]);
            (
                SketchConstraintDefinitionInput::Radius {
                    entity: admitted_value!(id(member)),
                    parameter,
                },
                None,
                vec![member],
            )
        }
        PmDcSketchConstraintKind::Diameter { entity, .. } => {
            let member = resolve(*entity)?;
            let parameter = driving!(constraint.header.parameter, "diameter", [member]);
            (
                SketchConstraintDefinitionInput::Diameter {
                    entity: admitted_value!(id(member)),
                    parameter,
                },
                None,
                vec![member],
            )
        }
        PmDcSketchConstraintKind::CircleCenter { entity, center } => {
            let members = vec![resolve(*entity)?, resolve(*center)?];
            admit!(ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index("circle_center_alignment".len()),
                "retain Inventor circle center kind"
            ));
            admit!(ctx.charge_collection_items(2, "collect Inventor circle center operands"));
            (
                SketchConstraintDefinitionInput::Native {
                    native_kind: cadmpeg_core::text::NonBlankString::new(
                        "circle_center_alignment",
                    )?,
                    native_state: Some(u64::from(constraint.header.state.cast_unsigned())),
                    native_flags: Some(u64::from(constraint.header.content.flags)),
                    native_properties: std::collections::BTreeMap::new(),
                    entities: admitted_value!(ctx.try_collect_retained_with(
                        members.iter().copied(),
                        "collect Inventor circle center entities",
                        &id
                    )),
                    parameter: None,
                    operands: vec![
                        admitted_value!(native_operand(
                            ctx,
                            constraint,
                            cadmpeg_core::nonblank_literal!("entity"),
                            *entity,
                        )),
                        admitted_value!(native_operand(
                            ctx,
                            constraint,
                            cadmpeg_core::nonblank_literal!("center"),
                            *center,
                        )),
                    ],
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::EqualRadius { first, second }
        | PmDcSketchConstraintKind::EqualLength { first, second } => {
            let members = vec![resolve(*first)?, resolve(*second)?];
            (
                SketchConstraintDefinitionInput::Equal {
                    first: admitted_value!(id(members[0])),
                    second: admitted_value!(id(members[1])),
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Midpoint { curve, point } => {
            let members = vec![resolve(*curve)?, resolve(*point)?];
            (
                SketchConstraintDefinitionInput::Midpoint {
                    point: SketchLocus::Entity(admitted_value!(id(members[1]))),
                    entity: admitted_value!(id(members[0])),
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::AlignedDistance { first, second, .. } => {
            let members = vec![resolve(*first)?, resolve(*second)?];
            let parameter = driving!(constraint.header.parameter, "aligned_distance", members);
            (
                SketchConstraintDefinitionInput::DistanceLoci {
                    first: SketchLocus::Entity(admitted_value!(id(members[0]))),
                    second: SketchLocus::Entity(admitted_value!(id(members[1]))),
                    parameter,
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::PointsVertical { first, second }
        | PmDcSketchConstraintKind::PointsHorizontal { first, second } => {
            let members = vec![resolve(*first)?, resolve(*second)?];
            let axis = if matches!(
                constraint.kind,
                PmDcSketchConstraintKind::PointsVertical { .. }
            ) {
                cadmpeg_ir::sketches::SketchCoordinateAxis::U
            } else {
                cadmpeg_ir::sketches::SketchCoordinateAxis::V
            };
            let relation = cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Entity(admitted_value!(id(members[0]))),
                SketchLocus::Entity(admitted_value!(id(members[1]))),
                axis,
            )
            .ok()?;
            (
                SketchConstraintDefinitionInput::SameCoordinate { relation },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Symmetric {
            first,
            second,
            axis,
        } => {
            let members = vec![resolve(*first)?, resolve(*second)?, resolve(*axis)?];
            (
                SketchConstraintDefinitionInput::Symmetric {
                    first: SketchLocus::Entity(admitted_value!(id(members[0]))),
                    second: SketchLocus::Entity(admitted_value!(id(members[1]))),
                    axis: admitted_value!(id(members[2])),
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::MirrorPair {
            result,
            source,
            axis,
            ..
        } => {
            let members = vec![resolve(*source)?, resolve(*result)?, resolve(*axis)?];
            (
                SketchConstraintDefinitionInput::Symmetric {
                    first: SketchLocus::Entity(admitted_value!(id(members[0]))),
                    second: SketchLocus::Entity(admitted_value!(id(members[1]))),
                    axis: admitted_value!(id(members[2])),
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Fix { entity } => {
            let member = resolve(*entity)?;
            (
                SketchConstraintDefinitionInput::Fixed {
                    entity: admitted_value!(id(member)),
                },
                None,
                vec![member],
            )
        }
        PmDcSketchConstraintKind::Collinear {
            first,
            second,
            orientation,
        } => {
            let members = vec![resolve(*first)?, resolve(*second)?];
            (
                SketchConstraintDefinitionInput::Collinear {
                    first: admitted_value!(id(members[0])),
                    second: admitted_value!(id(members[1])),
                },
                Some(u32::from(*orientation)),
                members,
            )
        }
        PmDcSketchConstraintKind::Angle {
            first,
            second,
            quadrant,
            ..
        } => {
            let members = vec![resolve(*first)?, resolve(*second)?];
            let parameter = driving!(constraint.header.parameter, "angle", members);
            (
                SketchConstraintDefinitionInput::Angle {
                    first: admitted_value!(id(members[0])),
                    second: admitted_value!(id(members[1])),
                    parameter,
                },
                Some(u32::from(*quadrant)),
                members,
            )
        }
        PmDcSketchConstraintKind::PointAngle {
            center,
            first,
            second,
            ..
        } => {
            let mut members = vec![resolve(*center)?, resolve(*first)?];
            if second != first {
                members.push(resolve(*second)?);
            }
            let mut definition = admitted_value!(native_relation(
                ctx,
                constraint,
                "point_angle",
                &members,
                &[
                    ("center", *center),
                    ("first", *first),
                    ("second", *second),
                    ("parameter", constraint.header.parameter),
                ],
            ));
            let driving =
                match resolve_parameter(ctx, constraint, constraint.header.parameter, parameters) {
                    Some(result) => Some(admitted_value!(result)),
                    None => None,
                };
            if let SketchConstraintDefinitionInput::Native { parameter, .. } = &mut definition {
                *parameter = driving;
            }
            (definition, None, members)
        }
        PmDcSketchConstraintKind::SplinePoint { spline, point } => {
            let members = vec![resolve(*spline)?, resolve(*point)?];
            let definition = admitted_value!(native_relation(
                ctx,
                constraint,
                "spline_point",
                &members,
                &[("spline", *spline), ("point", *point)],
            ));
            (definition, None, members)
        }
        PmDcSketchConstraintKind::Offset { pairs } => {
            let mut members = Vec::new();
            for reference in pairs.iter().flatten() {
                members.push(resolve(*reference)?);
            }
            let definition = admitted_value!(native_relation(
                ctx,
                constraint,
                "offset",
                &members,
                &[
                    ("source", pairs[0][0]),
                    ("result", pairs[0][1]),
                    ("source", pairs[1][0]),
                    ("result", pairs[1][1]),
                ],
            ));
            (definition, None, members)
        }
        PmDcSketchConstraintKind::BlockRelation { members: list } => {
            let [instance, proxies @ ..] = list.references() else {
                return None;
            };
            let mut members = Vec::new();
            for reference in proxies {
                members.push(resolve(*reference)?);
            }
            if members.is_empty() {
                return None;
            }
            let definition = admitted_value!(native_relation(
                ctx,
                constraint,
                "block_instance_relation",
                &members,
                &[("instance", *instance)],
            ));
            (definition, None, members)
        }
    };
    finish_constraint(ctx, constraint, scope, definition, orientation, &members)
}

/// Admit a projected relation over members of one sketch.
fn finish_constraint(
    ctx: &DecodeContext<'_>,
    constraint: &PmDcSketchConstraint,
    scope: Option<u32>,
    definition: SketchConstraintDefinitionInput,
    orientation: Option<u32>,
    members: &[&SketchEntity],
) -> Option<Result<SketchConstraint, CodecError>> {
    macro_rules! admit {
        ($result:expr) => {
            if let Err(error) = $result {
                return Some(Err(error));
            }
        };
    }

    macro_rules! admitted_value {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            }
        };
    }
    let first = members.first()?;
    if members.iter().any(|member| member.sketch != first.sketch) {
        return None;
    }
    admit!(ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(projected_id_len(
            "inventor:design:sketch-constraint#",
            constraint.identity.segment_token.as_str(),
            constraint.identity.record_ordinal
        )?),
        "retain projected Inventor sketch constraint id",
    ));
    admit!(ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(record_id_len(
            constraint.identity.segment_token.as_str(),
            constraint.identity.record_ordinal,
            "sketch-constraint"
        )?),
        "retain projected Inventor sketch constraint native reference",
    ));
    admit!(ctx.charge_entities(1, "project Inventor sketch constraint"));
    let id_text = match scope {
        None => format!(
            "inventor:design:sketch-constraint#{}-{}",
            constraint.identity.segment_token, constraint.identity.record_ordinal
        ),
        Some(instance) => format!(
            "inventor:design:sketch-constraint#{}-{instance}-{}",
            constraint.identity.segment_token, constraint.identity.record_ordinal
        ),
    };
    Some(Ok(SketchConstraint {
        id: SketchConstraintId::mint(id_text).ok()?,
        sketch: admitted_value!(first
            .sketch
            .try_clone_for_decode(ctx, "retain Inventor sketch constraint owner id")),
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition).ok()?,
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: Some(constraint.id()),
    }))
}

/// A deferred endpoint locus: the selector names the stored start (1) or
/// end (2) of a bounded entity.
type DeferredLocus<'a> = Box<dyn Fn(&DecodeContext<'_>) -> Result<SketchLocus, CodecError> + 'a>;

fn endpoint_locus(entity: &SketchEntity, selector: u32) -> Option<DeferredLocus<'_>> {
    if entity.endpoint_refs.len() != 2 {
        return None;
    }
    let start = match selector {
        1 => true,
        2 => false,
        _ => return None,
    };
    Some(Box::new(move |ctx| {
        let id = entity
            .id()
            .try_clone_for_decode(ctx, "retain Inventor sketch locus entity id")?;
        Ok(if start {
            SketchLocus::Start(id)
        } else {
            SketchLocus::End(id)
        })
    }))
}

/// Coincidence between two operands. A point at a curve's stored start or
/// end is an endpoint coincidence; another point on a curve lies on that
/// curve; curve-curve coincidences use their endpoint selectors.
fn coincident_definition(
    ctx: &DecodeContext<'_>,
    members: [&SketchEntity; 2],
    selectors: [u32; 2],
) -> Result<SketchConstraintDefinitionInput, CodecError> {
    let is_point = |entity: &SketchEntity| {
        matches!(
            *entity.geometry.definition(),
            SketchGeometryDefinition::Point { .. }
        )
    };
    let id = |entity: &SketchEntity| {
        entity
            .id()
            .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")
    };
    match (is_point(members[0]), is_point(members[1])) {
        (true, true) => Ok(SketchConstraintDefinitionInput::Coincident {
            entities: vec![id(members[0])?, id(members[1])?],
        }),
        (point_first, false) | (point_first @ false, true) => {
            if point_first || is_point(members[1]) {
                let (point, curve) = if point_first {
                    (members[0], members[1])
                } else {
                    (members[1], members[0])
                };
                let position = point.native_ref.as_deref().and_then(|native| {
                    curve
                        .endpoint_refs
                        .iter()
                        .position(|endpoint| endpoint == native)
                });
                let curve_locus = match position {
                    Some(0) => SketchLocus::Start(id(curve)?),
                    Some(1) => SketchLocus::End(id(curve)?),
                    _ => {
                        return Ok(SketchConstraintDefinitionInput::PointOnObject {
                            point: SketchLocus::Entity(id(point)?),
                            entity: id(curve)?,
                        });
                    }
                };
                let point_locus = SketchLocus::Entity(id(point)?);
                return Ok(SketchConstraintDefinitionInput::CoincidentLoci {
                    loci: if point_first {
                        vec![point_locus, curve_locus]
                    } else {
                        vec![curve_locus, point_locus]
                    },
                });
            }
            let loci = [
                endpoint_locus(members[0], selectors[0]),
                endpoint_locus(members[1], selectors[1]),
            ];
            match loci {
                [Some(first), Some(second)] => {
                    Ok(SketchConstraintDefinitionInput::CoincidentLoci {
                        loci: vec![first(ctx)?, second(ctx)?],
                    })
                }
                _ => Ok(SketchConstraintDefinitionInput::Coincident {
                    entities: vec![id(members[0])?, id(members[1])?],
                }),
            }
        }
    }
}

/// A source-native relation over resolved entities with record-reference operands.
fn native_relation(
    ctx: &DecodeContext<'_>,
    constraint: &PmDcSketchConstraint,
    kind: &'static str,
    members: &[&SketchEntity],
    operands: &[(&'static str, PmDcReference)],
) -> Result<SketchConstraintDefinitionInput, CodecError> {
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(kind.len()),
        "retain Inventor native sketch relation kind",
    )?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(members.len() + operands.len()),
        "collect Inventor native sketch relation operands",
    )?;
    let native_kind = cadmpeg_core::text::NonBlankString::new(kind)
        .ok_or_else(|| CodecError::malformed("Inventor native relation kind is blank"))?;
    let mut entities = Vec::new();
    for member in members {
        entities.push(
            member
                .id()
                .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")?,
        );
    }
    let mut native_operands = Vec::new();
    for (field, reference) in operands {
        let field = cadmpeg_core::text::NonBlankString::new(*field)
            .ok_or_else(|| CodecError::malformed("Inventor native operand field is blank"))?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(
                record_id_len(
                    constraint.identity.segment_token.as_str(),
                    reference.index(),
                    "record",
                )
                .ok_or_else(id_len_error)?,
            ),
            "retain Inventor sketch constraint operand native id",
        )?;
        native_operands.push(record_operand(
            constraint.identity.segment_token.as_str(),
            field,
            *reference,
        ));
    }
    Ok(SketchConstraintDefinitionInput::Native {
        native_kind,
        native_state: Some(u64::from(constraint.header.state.cast_unsigned())),
        native_flags: Some(u64::from(constraint.header.content.flags)),
        native_properties: std::collections::BTreeMap::new(),
        entities,
        parameter: None,
        operands: native_operands,
    })
}

fn resolve_parameter(
    ctx: &DecodeContext<'_>,
    constraint: &PmDcSketchConstraint,
    reference: PmDcReference,
    parameters: &HashMap<String, ParameterId>,
) -> Option<Result<ParameterId, CodecError>> {
    let ordinal = reference.index().checked_sub(1)?;
    let reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(record_id_len(
            constraint.identity.segment_token.as_str(),
            ordinal,
            "parameter",
        )?),
        "resolve Inventor sketch constraint parameter",
    );
    let _reservation = match reservation {
        Ok(reservation) => reservation,
        Err(error) => return Some(Err(error)),
    };
    let native = format!(
        "inventor:pmdc:parameter#{}-{}",
        constraint.identity.segment_token, ordinal
    );
    let parameter = parameters.get(&native)?;
    Some(parameter.try_clone_for_decode(ctx, "retain Inventor sketch constraint parameter id"))
}

fn native_operand(
    ctx: &DecodeContext<'_>,
    constraint: &PmDcSketchConstraint,
    field: cadmpeg_core::text::NonBlankString,
    reference: PmDcReference,
) -> Result<SketchNativeOperand, CodecError> {
    if let Some(ordinal) = reference.index().checked_sub(1) {
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(
                record_id_len(
                    constraint.identity.segment_token.as_str(),
                    ordinal,
                    "sketch-entity",
                )
                .ok_or_else(|| {
                    CodecError::Malformed("Inventor identifier length exceeds address space".into())
                })?,
            ),
            "retain Inventor sketch constraint operand native id",
        )?;
    }
    Ok(SketchNativeOperand {
        native_kind: cadmpeg_core::nonblank_literal!("record_reference"),
        field: Some(NativeOperandField {
            name: field,
            role: None,
        }),
        object_index: Some(reference.index()),
        native_ref: reference.index().checked_sub(1).map(|ordinal| {
            format!(
                "inventor:pmdc:sketch-entity#{}-{ordinal}",
                constraint.identity.segment_token
            )
        }),
    })
}

/// Raw record tables shared by entity projection.
struct Lookup<'a, 'b> {
    entities: &'b HashMap<RecordKey<'a>, Option<&'a PmDcSketchEntity>>,
    sketches: &'b HashMap<RecordKey<'a>, Option<&'a PmDcSketch>>,
}

impl<'a> Lookup<'a, '_> {
    fn entity(&self, token: &str, reference: PmDcReference) -> Option<&'a PmDcSketchEntity> {
        self.entities
            .get(&(token, reference.record_ordinal()?))
            .and_then(Option::as_ref)
            .copied()
    }
}

/// Agreement tolerance for block-instance frame orthonormality.
const EPS_SKETCH_BLOCK_FRAME: f64 = 1.0e-9;
/// Deepest chain of block proxies resolved through nested instances.
const MAX_BLOCK_DEPTH: usize = 4;

/// A rigid planar frame that maps block-definition coordinates into a host
/// sketch: `[u', v'] = [[a, b, tu], [c, d, tv]] * [u, v, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame([[f64; 3]; 2]);

impl Frame {
    const IDENTITY: Self = Self([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);

    /// Admits a rigid affine planar matrix with last row `[0, 0, 1]`.
    fn from_matrix(rows: [[f64; 3]; 3]) -> Option<Self> {
        let [first, second, last] = rows;
        let [a, b, _] = first;
        let [c, d, _] = second;
        let rigid = (last[0].abs() <= EPS_SKETCH_BLOCK_FRAME)
            && (last[1].abs() <= EPS_SKETCH_BLOCK_FRAME)
            && ((last[2] - 1.0).abs() <= EPS_SKETCH_BLOCK_FRAME)
            && ((a.hypot(c) - 1.0).abs() <= EPS_SKETCH_BLOCK_FRAME)
            && ((b.hypot(d) - 1.0).abs() <= EPS_SKETCH_BLOCK_FRAME)
            && (a.mul_add(b, c * d).abs() <= EPS_SKETCH_BLOCK_FRAME);
        rigid.then_some(Self([first, second]))
    }

    /// The frame that applies `self` and then `outer`.
    fn then(self, outer: Self) -> Self {
        let [[a, b, tu], [c, d, tv]] = self.0;
        let [[oa, ob, otu], [oc, od, otv]] = outer.0;
        Self([
            [
                oa.mul_add(a, ob * c),
                oa.mul_add(b, ob * d),
                oa.mul_add(tu, ob.mul_add(tv, otu)),
            ],
            [
                oc.mul_add(a, od * c),
                oc.mul_add(b, od * d),
                oc.mul_add(tu, od.mul_add(tv, otv)),
            ],
        ])
    }

    fn point(self, point: [f64; 2]) -> [f64; 2] {
        let [[a, b, tu], [c, d, tv]] = self.0;
        [
            a.mul_add(point[0], b.mul_add(point[1], tu)),
            c.mul_add(point[0], d.mul_add(point[1], tv)),
        ]
    }

    fn vector(self, vector: [f64; 2]) -> [f64; 2] {
        let [[a, b, _], [c, d, _]] = self.0;
        [
            a.mul_add(vector[0], b * vector[1]),
            c.mul_add(vector[0], d * vector[1]),
        ]
    }

    /// Whether the frame reverses orientation.
    fn reflects(self) -> bool {
        let [[a, b, _], [c, d, _]] = self.0;
        a.mul_add(d, -(b * c)) < 0.0
    }

    /// Whether the frame maps each sketch axis onto itself.
    fn preserves_axes(self) -> bool {
        let [[_, b, _], [c, _, _]] = self.0;
        b.abs() <= EPS_SKETCH_BLOCK_FRAME && c.abs() <= EPS_SKETCH_BLOCK_FRAME
    }
}

/// The entity that supplies geometry and the frame that places it: a block
/// proxy resolves through its instance to the copied definition member.
fn geometry_source<'a>(
    lookup: &Lookup<'a, '_>,
    entity: &'a PmDcSketchEntity,
    frame: Frame,
) -> Option<(&'a PmDcSketchEntity, Frame, Option<u32>)> {
    let mut current = entity;
    let mut current_frame = frame;
    let mut owner = None;
    for _ in 0..MAX_BLOCK_DEPTH {
        let PmDcSketchEntityKind::BlockProxy { instances, entity } = &current.kind else {
            return Some((current, current_frame, owner));
        };
        let instance = instances.references().first()?;
        let token = current.identity.segment_token.as_str();
        let instance_record = lookup.entity(token, *instance)?;
        let PmDcSketchEntityKind::BlockInstance { transform, .. } = &instance_record.kind else {
            return None;
        };
        let instance_frame = Frame::from_matrix(transform.rows())?;
        current_frame = instance_frame.then(current_frame);
        owner = Some(instance_record.identity.record_ordinal);
        current = lookup.entity(token, *entity)?;
    }
    None
}

fn framed_point(
    lookup: &Lookup<'_, '_>,
    token: &str,
    reference: PmDcReference,
    frame: Frame,
) -> Option<[f64; 2]> {
    let entity = lookup.entity(token, reference)?;
    let PmDcSketchEntityKind::Point { position, .. } = entity.kind else {
        return None;
    };
    let mapped = frame.point(position.map(FiniteReal::get));
    mapped
        .iter()
        .all(|value| value.is_finite())
        .then_some(mapped)
}

fn mm(point: [f64; 2]) -> Option<FinitePoint2Like> {
    let point = Point2::new(point[0] * 10.0, point[1] * 10.0);
    (point.u.is_finite() && point.v.is_finite()).then_some(point)
}

type FinitePoint2Like = Point2;

/// Solved geometry with its millimetre trace from stored start to end.
type TracedGeometry = (SketchGeometry, Vec<[f64; 2]>);

/// Number of chords that sample a circular arc for profile orientation.
const ARC_TRACE_SEGMENTS: u32 = 16;

/// Solved neutral geometry plus a millimetre trace from the stored start to
/// the stored end, used to orient profile faces.
fn project_geometry(
    ctx: &DecodeContext<'_>,
    lookup: &Lookup<'_, '_>,
    source: &PmDcSketchEntity,
    frame: Frame,
) -> Result<Option<TracedGeometry>, CodecError> {
    let token = source.identity.segment_token.as_str();
    Ok(match &source.kind {
        PmDcSketchEntityKind::Point { position, .. } => {
            let Some(position) = mm(frame.point(position.map(FiniteReal::get))) else {
                return Ok(None);
            };
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position })
                .ok()
                .map(|geometry| (geometry, Vec::new()))
        }
        PmDcSketchEntityKind::Line {
            points,
            origin,
            direction,
            ..
        } => {
            let [start, end, ..] = points.references() else {
                return Ok(None);
            };
            let (Some(raw_start), Some(raw_end)) = (
                framed_point(lookup, token, *start, Frame::IDENTITY),
                framed_point(lookup, token, *end, Frame::IDENTITY),
            ) else {
                return Ok(None);
            };
            if !line_carrier_matches(
                origin.map(FiniteReal::get),
                direction.map(FiniteReal::get),
                raw_start,
                raw_end,
            ) {
                return Ok(None);
            }
            let (Some(start), Some(end)) = (mm(frame.point(raw_start)), mm(frame.point(raw_end)))
            else {
                return Ok(None);
            };
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end })
                .ok()
                .map(|geometry| (geometry, vec![[start.u, start.v], [end.u, end.v]]))
        }
        PmDcSketchEntityKind::Circle {
            points,
            center,
            radius,
            ..
        } => {
            let Some(raw_center) = framed_point(lookup, token, *center, Frame::IDENTITY) else {
                return Ok(None);
            };
            let Some(center_mm) = mm(frame.point(raw_center)) else {
                return Ok(None);
            };
            let Some(radius_mm) = Length::new(radius.get() * 10.0) else {
                return Ok(None);
            };
            match curve_bounds(points) {
                None => None,
                Some(CurveBounds::Closed) => {
                    SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                        center: center_mm,
                        radius: radius_mm,
                    })
                    .ok()
                    .map(|geometry| (geometry, Vec::new()))
                }
                Some(CurveBounds::Bounded([start, end])) => {
                    let (Some(raw_start), Some(raw_end)) = (
                        framed_point(lookup, token, start, Frame::IDENTITY),
                        framed_point(lookup, token, end, Frame::IDENTITY),
                    ) else {
                        return Ok(None);
                    };
                    let (Some(start_angle), Some(end_angle)) = (
                        point_angle(raw_center, raw_start, radius.get()),
                        point_angle(raw_center, raw_end, radius.get()),
                    ) else {
                        return Ok(None);
                    };
                    let mut sweep = end_angle - start_angle;
                    if sweep <= 0.0 {
                        sweep += std::f64::consts::TAU;
                    }
                    let mut trace = Vec::new();
                    for step in 0..=ARC_TRACE_SEGMENTS {
                        let fraction = f64::from(step) / f64::from(ARC_TRACE_SEGMENTS);
                        let angle = sweep.mul_add(fraction, start_angle);
                        let point = frame.point([
                            radius.get().mul_add(angle.cos(), raw_center[0]),
                            radius.get().mul_add(angle.sin(), raw_center[1]),
                        ]);
                        trace.push([point[0] * 10.0, point[1] * 10.0]);
                    }
                    let (Some(mapped_start), Some(mapped_end)) =
                        (mm(frame.point(raw_start)), mm(frame.point(raw_end)))
                    else {
                        return Ok(None);
                    };
                    let angle_of =
                        |point: Point2| (point.v - center_mm.v).atan2(point.u - center_mm.u);
                    // A reflecting frame reverses the arc sense; the
                    // counterclockwise neutral arc then runs end to start.
                    let (from, to) = if frame.reflects() {
                        (angle_of(mapped_end), angle_of(mapped_start))
                    } else {
                        (angle_of(mapped_start), angle_of(mapped_end))
                    };
                    let (Some(from), Some(to)) = (Angle::new(from), Angle::new(to)) else {
                        return Ok(None);
                    };
                    SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                        center: center_mm,
                        radius: radius_mm,
                        start_angle: from,
                        end_angle: to,
                    })
                    .ok()
                    .map(|geometry| (geometry, trace))
                }
            }
        }
        PmDcSketchEntityKind::Ellipse {
            points,
            center,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => {
            if !matches!(curve_bounds(points), Some(CurveBounds::Closed)) {
                return Ok(None);
            }
            let Some(raw_center) = framed_point(lookup, token, *center, Frame::IDENTITY) else {
                return Ok(None);
            };
            let Some(center_mm) = mm(frame.point(raw_center)) else {
                return Ok(None);
            };
            let direction = frame.vector(major_direction.map(FiniteReal::get));
            let norm = direction[0].hypot(direction[1]);
            if !norm.is_finite() || norm <= f64::EPSILON {
                return Ok(None);
            }
            let (Some(angle), Some(major), Some(minor)) = (
                Angle::new(direction[1].atan2(direction[0])),
                Length::new(major_radius.get() * 10.0),
                Length::new(minor_radius.get() * 10.0),
            ) else {
                return Ok(None);
            };
            SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
                center: center_mm,
                major_angle: angle,
                radii: cadmpeg_ir::sketches::EllipseRadii {
                    major_radius: major,
                    minor_radius: minor,
                },
                bounds: None,
            })
            .ok()
            .map(|geometry| (geometry, Vec::new()))
        }
        PmDcSketchEntityKind::Spline { points, curve, .. } => {
            let [start, end, ..] = points.references() else {
                return Ok(None);
            };
            let (Some(start), Some(end)) = (
                framed_point(lookup, token, *start, frame),
                framed_point(lookup, token, *end, frame),
            ) else {
                return Ok(None);
            };
            match curve {
                PmDcSplineCurve::Stored {
                    degree,
                    knots,
                    weights,
                    poles,
                    ..
                } => {
                    ctx.charge_collection_items(
                        cadmpeg_core::decode::u64_from_index(knots.len() + poles.len()),
                        "project Inventor sketch spline",
                    )?;
                    let mut mapped = Vec::new();
                    let mut trace = Vec::new();
                    for pole in poles {
                        let point = frame.point(pole.map(FiniteReal::get));
                        let Some(point) = mm(point) else {
                            return Ok(None);
                        };
                        trace.push([point.u, point.v]);
                        mapped.push(point);
                    }
                    let knots = knots.iter().map(|knot| knot.get()).collect::<Vec<_>>();
                    let weights = (!weights.is_empty()).then(|| {
                        weights
                            .iter()
                            .map(|weight| weight.get())
                            .collect::<Vec<_>>()
                    });
                    let Ok(curve) = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                        ctx, *degree, knots, mapped, weights, false,
                    )?
                    else {
                        return Ok(None);
                    };
                    let ends = [
                        [start[0] * 10.0, start[1] * 10.0],
                        [end[0] * 10.0, end[1] * 10.0],
                    ];
                    if trace.first().copied() != Some(ends[0]) {
                        trace.insert(0, ends[0]);
                    }
                    if trace.last().copied() != Some(ends[1]) {
                        trace.push(ends[1]);
                    }
                    SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve })
                        .ok()
                        .map(|geometry| (geometry, trace))
                }
                PmDcSplineCurve::Unstored { .. } => Some((
                    SketchGeometry::native(cadmpeg_core::nonblank_literal!(
                        "inventor_unstored_curve"
                    )),
                    vec![
                        [start[0] * 10.0, start[1] * 10.0],
                        [end[0] * 10.0, end[1] * 10.0],
                    ],
                )),
            }
        }
        PmDcSketchEntityKind::BlockProxy { .. }
        | PmDcSketchEntityKind::BlockInstance { .. }
        | PmDcSketchEntityKind::MirrorNode { .. } => None,
    })
}

/// Project one sketch member or, with `instance`, one block-definition
/// member copied into the instance's host sketch.
fn project_member<'a>(
    ctx: &DecodeContext<'_>,
    lookup: &Lookup<'a, '_>,
    proxies: &HashMap<(&'a str, u32, u32), u32>,
    entity: &'a PmDcSketchEntity,
    host: &PmDcSketch,
    instance: Option<u32>,
) -> Result<Option<ProjectedEntity<'a>>, CodecError> {
    let token = entity.identity.segment_token.as_str();
    let frame = match instance {
        None => Frame::IDENTITY,
        Some(ordinal) => {
            let Some(record) = lookup
                .entities
                .get(&(token, ordinal))
                .and_then(Option::as_ref)
            else {
                return Ok(None);
            };
            let PmDcSketchEntityKind::BlockInstance { transform, .. } = &record.kind else {
                return Ok(None);
            };
            let Some(frame) = Frame::from_matrix(transform.rows()) else {
                return Ok(None);
            };
            frame
        }
    };
    let Some((source, frame, proxy_instance)) = geometry_source(lookup, entity, frame) else {
        return Ok(None);
    };
    let Some((geometry, trace)) = project_geometry(ctx, lookup, source, frame)? else {
        return Ok(None);
    };
    let scope = instance.or(proxy_instance);
    let id_text = match instance {
        None => format!(
            "inventor:design:sketch-entity#{}-{}",
            token, entity.identity.record_ordinal
        ),
        Some(ordinal) => format!(
            "inventor:design:sketch-entity#{}-{}-{}",
            token, ordinal, entity.identity.record_ordinal
        ),
    };
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(id_text.len()),
        "retain projected Inventor sketch entity id",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(
            projected_id_len(
                "inventor:design:sketch#",
                host.identity.segment_token.as_str(),
                host.identity.record_ordinal,
            )
            .ok_or_else(id_len_error)?,
        ),
        "retain projected Inventor sketch owner id",
    )?;
    let (Some(entity_id), Some(sketch_id)) = (SketchEntityId::mint(id_text).ok(), sketch_id(host))
    else {
        return Ok(None);
    };
    ctx.charge_collection_items(1, "project Inventor sketch entity")?;
    ctx.charge_entities(1, "project Inventor sketch entity")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(
            record_id_len(token, entity.identity.record_ordinal, "sketch-entity")
                .ok_or_else(id_len_error)?,
        ),
        "retain projected Inventor sketch entity native reference",
    )?;
    let mut endpoint_refs = Vec::new();
    let mut profile_keys = Vec::new();
    if let Some(endpoints) = bounded_endpoints(&source.kind) {
        for endpoint in endpoints {
            let Some(point) = lookup.entity(token, endpoint) else {
                return Ok(None);
            };
            let proxy = scope.and_then(|scope| {
                proxies
                    .get(&(token, scope, point.identity.record_ordinal))
                    .copied()
            });
            ctx.charge_collection_items(1, "collect Inventor sketch endpoint reference")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(
                    record_id_len(token, point.identity.record_ordinal, "sketch-entity")
                        .ok_or_else(id_len_error)?,
                ),
                "retain Inventor sketch endpoint native id",
            )?;
            let native = match proxy {
                Some(proxy) => format!("inventor:pmdc:sketch-entity#{token}-{proxy}"),
                None => point.id(),
            };
            let key = match (proxy, scope) {
                (None, Some(scope)) => format!("{native}@{scope}"),
                _ => native.clone(),
            };
            endpoint_refs.push(native);
            profile_keys.push(key);
        }
    }
    let construction = source.entity_flags & CONSTRUCTION_FLAG != 0;
    let geometry_ref = (!std::ptr::eq(source, entity)).then(|| source.id());
    Ok(Some(ProjectedEntity {
        entity: SketchEntity::new(entity_id, sketch_id, geometry)
            .with_construction(construction)
            .with_native_ref(Some(entity.id()))
            .with_geometry_ref(geometry_ref)
            .with_endpoint_refs(endpoint_refs),
        profile_keys,
        trace,
        token,
        instance,
        ordinal: entity.identity.record_ordinal,
    }))
}

/// Bound state of a circular or elliptical entity's stored point list.
enum CurveBounds {
    /// Two null references: the complete closed curve.
    Closed,
    /// Start and end point references of a counterclockwise bounded curve.
    Bounded([PmDcReference; 2]),
}

/// Classify a circular or elliptical point list. The first two references
/// are the start and end points; both null selects the closed curve.
fn curve_bounds(points: &PmDcReferenceList) -> Option<CurveBounds> {
    match points.references() {
        [] => Some(CurveBounds::Closed),
        [start, end, ..] => match (start.index(), end.index()) {
            (0, 0) => Some(CurveBounds::Closed),
            (0, _) | (_, 0) => None,
            (a, b) if a == b => None,
            _ => Some(CurveBounds::Bounded([*start, *end])),
        },
        [_] => None,
    }
}

/// Stored start and end point references of a bounded entity.
fn bounded_endpoints(kind: &PmDcSketchEntityKind) -> Option<[PmDcReference; 2]> {
    match kind {
        PmDcSketchEntityKind::Line { points, .. } => match points.references() {
            [start, end, ..] => Some([*start, *end]),
            _ => None,
        },
        PmDcSketchEntityKind::Circle { points, .. }
        | PmDcSketchEntityKind::Ellipse { points, .. } => match curve_bounds(points)? {
            CurveBounds::Bounded(ends) => Some(ends),
            CurveBounds::Closed => None,
        },
        PmDcSketchEntityKind::Spline { points, .. } => match points.references() {
            [start, end, ..] => Some([*start, *end]),
            _ => None,
        },
        PmDcSketchEntityKind::Point { .. }
        | PmDcSketchEntityKind::BlockInstance { .. }
        | PmDcSketchEntityKind::BlockProxy { .. }
        | PmDcSketchEntityKind::MirrorNode { .. } => None,
    }
}

/// Angle of an arc endpoint about its center, checked against the radius.
fn point_angle(center: [f64; 2], point: [f64; 2], radius: f64) -> Option<f64> {
    let du = point[0] - center[0];
    let dv = point[1] - center[1];
    let distance = du.hypot(dv);
    if !distance.is_finite() || (distance - radius).abs() > EPS_SKETCH_ARC_RADIUS_RELATIVE * radius
    {
        return None;
    }
    let angle = dv.atan2(du);
    angle.is_finite().then_some(angle)
}

fn line_carrier_matches(
    origin: [f64; 2],
    direction: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
) -> bool {
    let Some(unit) =
        cadmpeg_ir::features::FiniteVector3::new(Vector3::new(direction[0], direction[1], 0.0))
            .and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero)
    else {
        return false;
    };
    let span = Vector3::new(end[0] - start[0], end[1] - start[1], 0.0);
    let span_scale = span.x.abs().max(span.y.abs());
    let span_length = span.x.hypot(span.y);
    let Some(span) = cadmpeg_ir::features::FiniteVector3::new(span)
        .and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero)
    else {
        return false;
    };
    // The stored direction agrees with the segment when the end point lies
    // on the carrier through the start point: a short segment stores its
    // endpoints with less angular precision than its direction.
    let parallel_error = (unit.x * span.y - unit.y * span.x).abs();
    if parallel_error > EPS_SKETCH_LINE_CARRIER_MATCHES_E10
        && parallel_error * span_length > EPS_SKETCH_LINE_CARRIER_MATCHES_E10
    {
        return false;
    }
    let from_origin = Vector3::new(start[0] - origin[0], start[1] - origin[1], 0.0);
    if !from_origin.x.is_finite() || !from_origin.y.is_finite() {
        return false;
    }
    let offset_scale = from_origin.x.abs().max(from_origin.y.abs());
    if offset_scale == 0.0 {
        return true;
    }
    // Position agreement uses the finite segment scale, which stays unchanged
    // when the stored origin moves along the same infinite line.
    // Preserve the stored direction ratio when measuring position: rounding a
    // unit direction can rotate a distant incident point away from the line.
    let Some(exponent) =
        cadmpeg_ir::math::power_of_two_bound(direction[0].abs().max(direction[1].abs()))
    else {
        return false;
    };
    let [Some(dx), Some(dy)] = direction.map(|value| {
        cadmpeg_ir::math::scale_power_of_two(value, -exponent)
            .map(cadmpeg_ir::scalar::FiniteReal::get)
    }) else {
        return false;
    };
    // Scale position and span separately so a subnormal perpendicular offset
    // survives the determinant and a distant origin cannot overflow it.
    let (Some(offset_exponent), Some(span_exponent)) = (
        cadmpeg_ir::math::power_of_two_bound(offset_scale),
        cadmpeg_ir::math::power_of_two_bound(span_scale),
    ) else {
        return false;
    };
    let [Some(x), Some(y)] = [from_origin.x, from_origin.y].map(|value| {
        cadmpeg_ir::math::scale_power_of_two(value, -offset_exponent)
            .map(cadmpeg_ir::scalar::FiniteReal::get)
    }) else {
        return false;
    };
    let Some(scaled_span) = cadmpeg_ir::math::scale_power_of_two(span_scale, -span_exponent)
        .map(cadmpeg_ir::scalar::FiniteReal::get)
    else {
        return false;
    };
    let right = dy * x;
    let determinant = dx.mul_add(y, -right) - dy.mul_add(x, -right);
    let carrier_error = determinant.abs() / dx.hypot(dy) / scaled_span;
    cadmpeg_ir::math::scale_power_of_two(carrier_error, offset_exponent - span_exponent)
        .is_some_and(|error| error.get() <= EPS_SKETCH_LINE_CARRIER_MATCHES_E10)
}

fn project_placement(
    sketch: &PmDcSketch,
    transforms: &HashMap<(&str, u32), Option<&PmDcTransform>>,
    directions: &HashMap<(&str, u32), Option<&PmDcDirection>>,
) -> Option<SketchPlacement> {
    let transform = transforms
        .get(&(
            sketch.identity.segment_token.as_str(),
            sketch.transform.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)?;
    let direction = directions
        .get(&(
            sketch.identity.segment_token.as_str(),
            sketch.direction.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)?;
    let matrix = transform.matrix.rows();
    if matrix[3]
        .iter()
        .zip([0.0, 0.0, 0.0, 1.0])
        .any(|(actual, expected)| (actual - expected).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10)
    {
        return None;
    }
    let u_axis = Vector3::new(matrix[0][0], matrix[1][0], matrix[2][0]).unit()?;
    let v_axis = Vector3::new(matrix[0][1], matrix[1][1], matrix[2][1]).unit()?;
    let normal = Vector3::new(matrix[0][2], matrix[1][2], matrix[2][2]).unit()?;
    let stored_direction = Vector3::new(
        direction.direction[0].get(),
        direction.direction[1].get(),
        direction.direction[2].get(),
    )
    .unit()?;
    if u_axis.dot(v_axis).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10
        || u_axis.dot(normal).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10
        || v_axis.dot(normal).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10
        || u_axis.cross(v_axis).dot(normal) < 1.0 - EPS_SKETCH_PROJECT_PLACEMENT_E10
        || normal.dot(stored_direction) < 1.0 - EPS_SKETCH_PROJECT_PLACEMENT_E10
    {
        return None;
    }
    SketchPlacement::try_resolved(
        Point3::new(
            matrix[0][3] * 10.0,
            matrix[1][3] * 10.0,
            matrix[2][3] * 10.0,
        ),
        normal,
        u_axis,
    )
    .ok()
}

/// Smallest enclosed area, in square millimetres, of a profile face.
const EPS_SKETCH_PROFILE_FACE_AREA: f64 = 1.0e-9;

/// Whether a solved member is a bounded profile edge joined through its endpoints.
fn is_member_edge(member: &ProjectedEntity<'_>) -> bool {
    !member.entity.construction
        && member.profile_keys.len() == 2
        && member.profile_keys[0] != member.profile_keys[1]
        && member.trace.len() >= 2
        && matches!(
            *member.entity.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
                | SketchGeometryDefinition::Arc { .. }
                | SketchGeometryDefinition::Nurbs { .. }
                | SketchGeometryDefinition::Native { .. }
        )
}

/// Closed profile loops of one sketch. A complete circle or ellipse is one
/// loop. Bounded edges join through their endpoint keys; dangling edges are
/// pruned. A component whose vertices all have degree two is one loop; a
/// branched component contributes each bounded face of its planar graph.
fn build_member_profiles(
    ctx: &DecodeContext<'_>,
    members: &[&ProjectedEntity<'_>],
) -> Result<Vec<Vec<SketchEntityUse>>, CodecError> {
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(members.len()),
        "index Inventor profile source positions",
    )?;
    let mut profiles = Vec::<(usize, Vec<SketchEntityUse>)>::new();
    for (position, member) in members.iter().enumerate() {
        if member.entity.construction
            || !matches!(
                *member.entity.geometry.definition(),
                SketchGeometryDefinition::Circle { .. }
                    | SketchGeometryDefinition::Ellipse { bounds: None, .. }
            )
        {
            continue;
        }
        ctx.charge_collection_items(1, "project Inventor circular profile")?;
        ctx.charge_collection_items(1, "project Inventor circular profile use")?;
        profiles.push((
            position,
            vec![SketchEntityUse {
                entity: member
                    .entity
                    .id()
                    .try_clone_for_decode(ctx, "retain Inventor circular profile entity id")?,
                reversed: false,
            }],
        ));
    }
    let edges = members
        .iter()
        .enumerate()
        .filter(|(_, member)| is_member_edge(member))
        .map(|(position, member)| (position, *member))
        .collect::<Vec<_>>();
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(edges.len()),
        "collect Inventor profile lines",
    )?;
    // Prune edges with a free endpoint until every remaining vertex joins
    // two or more edges.
    let mut alive = vec![true; edges.len()];
    loop {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(edges.len()),
            "prune Inventor profile edges",
        )?;
        let mut degree = HashMap::<&str, usize>::new();
        for (index, (_, edge)) in edges.iter().enumerate() {
            if alive[index] {
                for key in &edge.profile_keys {
                    *degree.entry(key.as_str()).or_default() += 1;
                }
            }
        }
        let mut changed = false;
        for (index, (_, edge)) in edges.iter().enumerate() {
            if alive[index]
                && edge
                    .profile_keys
                    .iter()
                    .any(|key| degree.get(key.as_str()).copied().unwrap_or(0) < 2)
            {
                alive[index] = false;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut adjacency = HashMap::<&str, Vec<usize>>::new();
    for (index, (_, edge)) in edges.iter().enumerate() {
        if !alive[index] {
            continue;
        }
        for key in &edge.profile_keys {
            ctx.push_hash_group(
                &mut adjacency,
                key.as_str(),
                index,
                "index Inventor profile endpoint",
                "link Inventor profile line endpoint",
            )?;
        }
    }
    let mut visited = vec![false; edges.len()];
    for start in 0..edges.len() {
        ctx.charge_work(1, "traverse Inventor profile lines")?;
        if visited[start] || !alive[start] {
            continue;
        }
        let component = edge_component(ctx, start, &edges, &adjacency)?;
        for index in &component {
            visited[*index] = true;
        }
        let simple = component.iter().all(|index| {
            edges[*index]
                .1
                .profile_keys
                .iter()
                .all(|key| adjacency.get(key.as_str()).map_or(0, Vec::len) == 2)
        });
        let loops = if simple {
            simple_loop(ctx, start, &edges, &adjacency)?
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            component_faces(ctx, &component, &edges)?
        };
        for uses in loops {
            let Some(position) = uses.iter().map(|(index, _)| edges[*index].0).min() else {
                continue;
            };
            let mut profile = Vec::new();
            for (index, reversed) in uses {
                ctx.charge_collection_items(1, "collect Inventor profile use")?;
                profile.push(SketchEntityUse {
                    entity: edges[index]
                        .1
                        .entity
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor profile use id")?,
                    reversed,
                });
            }
            ctx.charge_collection_items(1, "project Inventor line profile")?;
            profiles.push((position, profile));
        }
    }
    ctx.sort_unstable_by(
        &mut profiles,
        |left, right| left.0.cmp(&right.0),
        |profile| {
            profile
                .1
                .iter()
                .map(|entity| entity.entity.as_str().len())
                .sum::<usize>()
        },
        "Inventor line profiles sort",
    )?;
    Ok(profiles.into_iter().map(|(_, profile)| profile).collect())
}

type ProfileEdge<'m, 'a> = (usize, &'m ProjectedEntity<'a>);

fn edge_component(
    ctx: &DecodeContext<'_>,
    start: usize,
    edges: &[ProfileEdge<'_, '_>],
    adjacency: &HashMap<&str, Vec<usize>>,
) -> Result<Vec<usize>, CodecError> {
    let mut component = HashSet::new();
    ctx.charge_collection_items(1, "queue Inventor profile line")?;
    let mut pending = vec![start];
    while let Some(index) = pending.pop() {
        ctx.charge_work(1, "scan Inventor profile component")?;
        if !ctx.insert_hash_set(&mut component, index, "collect Inventor profile component")? {
            continue;
        }
        for key in &edges[index].1.profile_keys {
            if let Some(neighbours) = adjacency.get(key.as_str()) {
                ctx.charge_collection_items(
                    cadmpeg_core::decode::u64_from_index(neighbours.len()),
                    "queue Inventor profile neighbours",
                )?;
                pending.extend(neighbours);
            }
        }
    }
    let mut component = component.into_iter().collect::<Vec<_>>();
    ctx.sort_unstable_by(
        &mut component,
        Ord::cmp,
        |_| std::mem::size_of::<usize>(),
        "sort Inventor profile component",
    )?;
    Ok(component)
}

/// The single loop of a component whose vertices all join two edges.
fn simple_loop(
    ctx: &DecodeContext<'_>,
    start: usize,
    edges: &[ProfileEdge<'_, '_>],
    adjacency: &HashMap<&str, Vec<usize>>,
) -> Result<Option<Vec<(usize, bool)>>, CodecError> {
    let first = edges[start].1;
    let start_key = first.profile_keys[0].as_str();
    let mut key = first.profile_keys[1].as_str();
    let mut current = start;
    let mut uses = vec![(start, false)];
    let mut seen = HashSet::from([start]);
    while key != start_key {
        ctx.charge_work(1, "traverse Inventor profile loop")?;
        let Some(next) = adjacency
            .get(key)
            .and_then(|indices| indices.iter().copied().find(|index| *index != current))
        else {
            return Ok(None);
        };
        if !seen.insert(next) {
            return Ok(None);
        }
        let edge = edges[next].1;
        let reversed = edge.profile_keys[1] == key;
        key = if reversed {
            edge.profile_keys[0].as_str()
        } else {
            edge.profile_keys[1].as_str()
        };
        current = next;
        uses.push((next, reversed));
    }
    Ok((uses.len() >= 2
        || matches!(
            *first.entity.geometry.definition(),
            SketchGeometryDefinition::Arc { .. } | SketchGeometryDefinition::Nurbs { .. }
        ))
    .then_some(uses))
}

/// Bounded faces of a branched planar edge graph. Each directed edge turns
/// at its end vertex onto the next edge clockwise from its reverse; the
/// resulting cycles with positive enclosed area are the bounded faces.
fn component_faces(
    ctx: &DecodeContext<'_>,
    component: &[usize],
    edges: &[ProfileEdge<'_, '_>],
) -> Result<Vec<Vec<(usize, bool)>>, CodecError> {
    // Half-edge (edge, reversed) leaves its origin vertex along this angle.
    let departure = |edge: usize, reversed: bool| {
        let trace = &edges[edge].1.trace;
        let (from, to) = if reversed {
            (trace[trace.len() - 1], trace[trace.len() - 2])
        } else {
            (trace[0], trace[1])
        };
        (to[1] - from[1]).atan2(to[0] - from[0])
    };
    let origin = |edge: usize, reversed: bool| {
        let keys = &edges[edge].1.profile_keys;
        if reversed {
            keys[1].as_str()
        } else {
            keys[0].as_str()
        }
    };
    let destination = |edge: usize, reversed: bool| origin(edge, !reversed);
    // Outgoing half-edges at each vertex in counterclockwise order.
    let mut outgoing = HashMap::<&str, Vec<(f64, usize, bool)>>::new();
    for &edge in component {
        for reversed in [false, true] {
            ctx.charge_work(1, "index Inventor profile half-edge")?;
            outgoing.entry(origin(edge, reversed)).or_default().push((
                departure(edge, reversed),
                edge,
                reversed,
            ));
        }
    }
    for list in outgoing.values_mut() {
        ctx.sort_unstable_by(
            list,
            |left, right| left.0.total_cmp(&right.0),
            |_| std::mem::size_of::<(f64, usize, bool)>(),
            "sort Inventor profile half-edges",
        )?;
    }
    let mut used = HashSet::new();
    let mut faces = Vec::new();
    for &edge in component {
        for reversed in [false, true] {
            if used.contains(&(edge, reversed)) {
                continue;
            }
            let mut face = Vec::new();
            let mut current = (edge, reversed);
            loop {
                ctx.charge_work(1, "traverse Inventor profile face")?;
                if !used.insert(current) {
                    break;
                }
                face.push(current);
                let vertex = destination(current.0, current.1);
                let Some(list) = outgoing.get(vertex) else {
                    break;
                };
                let Some(twin) = list.iter().position(|(_, other, other_reversed)| {
                    *other == current.0 && *other_reversed != current.1
                }) else {
                    break;
                };
                let next = list[(twin + list.len() - 1) % list.len()];
                current = (next.1, next.2);
            }
            if current != (edge, reversed) || face.is_empty() {
                continue;
            }
            let mut area = 0.0;
            for (index, backward) in &face {
                let trace = &edges[*index].1.trace;
                let points: Box<dyn Iterator<Item = &[f64; 2]>> = if *backward {
                    Box::new(trace.iter().rev())
                } else {
                    Box::new(trace.iter())
                };
                let points = points.collect::<Vec<_>>();
                for pair in points.windows(2) {
                    area += pair[0][0].mul_add(pair[1][1], -(pair[1][0] * pair[0][1]));
                }
            }
            if area * 0.5 > EPS_SKETCH_PROFILE_FACE_AREA {
                ctx.charge_collection_items(
                    cadmpeg_core::decode::u64_from_index(face.len()),
                    "collect Inventor profile face",
                )?;
                faces.push(face);
            }
        }
    }
    Ok(faces)
}

/// A block instance or sketch mirror node as a source-native relation.
fn project_helper(
    ctx: &DecodeContext<'_>,
    lookup: &Lookup<'_, '_>,
    helper: &PmDcSketchEntity,
    host_entities: &HashMap<RecordKey<'_>, &SketchEntity>,
    projected: &[ProjectedEntity<'_>],
) -> Result<Option<SketchConstraint>, CodecError> {
    let token = helper.identity.segment_token.as_str();
    let Some(host) = helper
        .sketch
        .record_ordinal()
        .and_then(|ordinal| lookup.sketches.get(&(token, ordinal)))
        .and_then(Option::as_ref)
    else {
        return Ok(None);
    };
    let Some(sketch) = sketch_id(host) else {
        return Ok(None);
    };
    let (kind, members, operands, properties) = match &helper.kind {
        PmDcSketchEntityKind::BlockInstance {
            definition,
            relation,
            transform,
            value,
        } => {
            let instance = helper.identity.record_ordinal;
            let mut members = Vec::new();
            for member in projected {
                let proxied = member.instance.is_none()
                    && member.token == token
                    && lookup
                        .entities
                        .get(&(token, member.ordinal))
                        .and_then(Option::as_ref)
                        .is_some_and(|record| {
                            matches!(
                                &record.kind,
                                PmDcSketchEntityKind::BlockProxy { instances, .. }
                                    if instances
                                        .references()
                                        .first()
                                        .and_then(|owner| owner.record_ordinal())
                                        == Some(instance)
                            )
                        });
                if member.instance == Some(instance) || proxied {
                    members.push(&member.entity);
                }
            }
            if members.is_empty() {
                return Ok(None);
            }
            let rows = transform.rows();
            let mut properties = std::collections::BTreeMap::new();
            properties.insert(
                "transform".to_owned(),
                format!(
                    "[[{}, {}, {}], [{}, {}, {}], [{}, {}, {}]]",
                    rows[0][0],
                    rows[0][1],
                    rows[0][2],
                    rows[1][0],
                    rows[1][1],
                    rows[1][2],
                    rows[2][0],
                    rows[2][1],
                    rows[2][2]
                ),
            );
            properties.insert("value".to_owned(), value.to_string());
            (
                "block_instance",
                members,
                vec![("definition", *definition), ("relation", *relation)],
                properties,
            )
        }
        PmDcSketchEntityKind::MirrorNode { axis, data } => {
            let Some(axis_entity) = axis
                .record_ordinal()
                .and_then(|ordinal| host_entities.get(&(token, ordinal)))
                .copied()
            else {
                return Ok(None);
            };
            (
                "sketch_mirror",
                vec![axis_entity],
                vec![("axis", *axis), ("data", *data)],
                std::collections::BTreeMap::new(),
            )
        }
        _ => return Ok(None),
    };
    if members.iter().any(|member| member.sketch != sketch) {
        return Ok(None);
    }
    let retained = properties
        .iter()
        .map(|(key, value)| key.len() + value.len())
        .sum::<usize>()
        + kind.len();
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(retained),
        "retain Inventor native sketch relation kind",
    )?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(members.len() + operands.len()),
        "collect Inventor native sketch relation operands",
    )?;
    let mut entities = Vec::new();
    for member in &members {
        entities.push(
            member
                .id()
                .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")?,
        );
    }
    let mut native_operands = Vec::new();
    for (field, reference) in operands {
        let field = cadmpeg_core::text::NonBlankString::new(field)
            .ok_or_else(|| CodecError::malformed("Inventor native operand field is blank"))?;
        native_operands.push(record_operand(token, field, reference));
    }
    let native_kind = cadmpeg_core::text::NonBlankString::new(kind)
        .ok_or_else(|| CodecError::malformed("Inventor native relation kind is blank"))?;
    let id_text = format!(
        "inventor:design:sketch-constraint#{}-{}",
        token, helper.identity.record_ordinal
    );
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(id_text.len()),
        "retain projected Inventor sketch constraint id",
    )?;
    ctx.charge_entities(1, "project Inventor sketch constraint")?;
    let Ok(id) = SketchConstraintId::mint(id_text) else {
        return Ok(None);
    };
    let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
        SketchConstraintDefinitionInput::Native {
            native_kind,
            native_state: None,
            native_flags: Some(u64::from(helper.entity_flags)),
            native_properties: properties,
            entities,
            parameter: None,
            operands: native_operands,
        },
    ) else {
        return Ok(None);
    };
    Ok(Some(SketchConstraint {
        id,
        sketch,
        definition,
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: Some(helper.id()),
    }))
}

/// A native operand that names one record in the same segment.
fn record_operand(
    token: &str,
    field: cadmpeg_core::text::NonBlankString,
    reference: PmDcReference,
) -> SketchNativeOperand {
    SketchNativeOperand {
        native_kind: cadmpeg_core::nonblank_literal!("record_reference"),
        field: Some(NativeOperandField {
            name: field,
            role: None,
        }),
        object_index: Some(reference.index()),
        native_ref: reference
            .record_ordinal()
            .map(|ordinal| format!("inventor:rse:record#{token}-{ordinal}")),
    }
}

fn projected_id_len(prefix: &str, token: &str, ordinal: u32) -> Option<usize> {
    Some(prefix.len() + token.len() + 1 + usize::try_from(ordinal.max(1).ilog10()).ok()? + 1)
}

fn record_id_len(token: &str, ordinal: u32, kind: &str) -> Option<usize> {
    Some(projected_id_len("inventor:pmdc:", token, ordinal)? + kind.len() + 1)
}

fn sketch_id(sketch: &PmDcSketch) -> Option<SketchId> {
    SketchId::mint(format!(
        "inventor:design:sketch#{}-{}",
        sketch.identity.segment_token, sketch.identity.record_ordinal
    ))
    .ok()
}

pub(crate) type PmDcSketch = Located<PmDcSketchPayload>;

impl RecordPayload for PmDcSketchPayload {
    const KIND: &'static str = "sketch";
}

pub(crate) type PmDcSketchEntity = Located<PmDcSketchEntityPayload>;

impl RecordPayload for PmDcSketchEntityPayload {
    const KIND: &'static str = "sketch-entity";
}

pub(crate) type PmDcTransform = Located<PmDcTransformPayload>;

impl RecordPayload for PmDcTransformPayload {
    const KIND: &'static str = "transform";
}

pub(crate) type PmDcDirection = Located<PmDcDirectionPayload>;

impl RecordPayload for PmDcDirectionPayload {
    const KIND: &'static str = "direction";
}

pub(crate) type PmDcSketchConstraint = Located<PmDcSketchConstraintPayload>;

impl RecordPayload for PmDcSketchConstraintPayload {
    const KIND: &'static str = "sketch-constraint";
}

#[cfg(test)]
mod tests {
    use super::{
        inventory, parse_constraint, parse_direction, parse_entity, parse_sketch, parse_transform,
        project, PmDcSketchConstraintKind, PmDcSketchEntityKind, PmDcTransformPayload,
        SketchConstraintTag, SketchEntityTag, SketchInventory, COINCIDENT_TYPE, DIRECTION_TYPE,
        HORIZONTAL_TYPE, LINE_TYPE, POINT_TYPE, SKETCH_TYPE, TRANSFORM_TYPE,
    };
    use crate::container::InventorContainer;
    use crate::pmdc::{PmDcReference, PmDcReferenceList};
    use crate::record_identity::Located;
    use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
    use crate::test_support::test_fixtures::{content, parse, primary_envelope_fixture};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::scalar::FiniteReal;
    use cadmpeg_ir::sketches::SketchPlacement;

    fn real(value: f64) -> FiniteReal {
        FiniteReal::new(value).expect("finite test scalar")
    }

    #[test]
    fn profile_builder_refuses_collection_limit_before_source_index() {
        let entity = circle_profile_entity();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("profile context");
        assert!(matches!(
            super::build_member_profiles(&ctx, &[&member(entity.clone(), Vec::new())]),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "index Inventor profile source positions"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service profile context");
        assert_eq!(
            super::build_member_profiles(&ctx, &[&member(entity.clone(), Vec::new())])
                .expect("circle profile")
                .len(),
            1
        );
    }

    fn member(
        entity: cadmpeg_ir::sketches::SketchEntity,
        profile_keys: Vec<String>,
    ) -> super::ProjectedEntity<'static> {
        super::ProjectedEntity {
            entity,
            profile_keys,
            trace: vec![[0.0, 0.0], [1.0, 0.0]],
            token: "segment",
            instance: None,
            ordinal: 0,
        }
    }

    fn circle_profile_entity() -> cadmpeg_ir::sketches::SketchEntity {
        cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#1")
                .expect("entity id"),
            cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
            cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Circle {
                    center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    radius: cadmpeg_ir::scalar::Length::new(1.0).expect("positive radius"),
                },
            )
            .expect("circle geometry"),
        )
    }

    #[test]
    fn profile_builder_refuses_collection_limits_before_circular_profile_allocations() {
        let entity = circle_profile_entity();
        let arena = DecodeArena::new();
        for (cap, operation) in [
            (1, "project Inventor circular profile"),
            (2, "project Inventor circular profile use"),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("profile context");
            assert!(matches!(
                super::build_member_profiles(&ctx, &[&member(entity.clone(), Vec::new())]),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == operation
            ));
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service profile context");
        assert_eq!(
            super::build_member_profiles(&ctx, &[&member(entity.clone(), Vec::new())])
                .expect("circle profile")
                .len(),
            1
        );
    }

    #[test]
    fn line_component_refuses_collection_limit_before_queue_creation() {
        let entity = cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#2")
                .expect("entity id"),
            cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
            cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
                    start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                },
            )
            .expect("line geometry"),
        )
        .with_endpoint_refs(vec!["point-a".into(), "point-b".into()]);
        let entity = member(entity, vec!["point-a".into(), "point-b".into()]);
        let adjacency = std::collections::HashMap::from([
            ("point-a", vec![0_usize]),
            ("point-b", vec![0_usize]),
        ]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("profile context");
        assert!(matches!(
            super::edge_component(&ctx, 0, &[(0, &entity)], &adjacency),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "queue Inventor profile line"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service profile context");
        assert_eq!(
            super::edge_component(&ctx, 0, &[(0, &entity)], &adjacency)
                .expect("line component")
                .len(),
            1
        );
    }

    #[test]
    fn sketch_projection_refuses_collection_limit_before_entity_index() {
        let point = parse(&point_bytes(0, 1, [0.0, 0.0]), |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point record")
        });
        let inventory = SketchInventory {
            sketches: Vec::new(),
            entities: vec![Located::new(
                point,
                crate::record_identity::RecordTypeId::from_bytes(POINT_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                0,
            )],
            transforms: Vec::new(),
            directions: Vec::new(),
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            project(&ctx, &inventory, &[]),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "index Inventor sketch entities"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert_eq!(
            project(&ctx, &inventory, &[])
                .expect("sketch projection")
                .unresolved_entities,
            1
        );
    }

    #[test]
    fn sketch_projection_refuses_entity_limit_before_entity_creation() {
        let mut sketch_bytes = content(0);
        sketch_bytes.extend_from_slice(&0_i32.to_le_bytes());
        sketch_bytes.extend_from_slice(&1_u32.to_le_bytes());
        sketch_bytes.extend(list(8, &[2]));
        sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&[0; 8]);
        sketch_bytes.extend(list(2, &[]));
        let sketch = parse(&sketch_bytes, |ctx, source| {
            parse_sketch(ctx, source, 22).expect("sketch record")
        });
        let point = parse(&point_bytes(1, 1, [0.0, 0.0]), |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point record")
        });
        let inventory = SketchInventory {
            sketches: vec![Located::new(
                sketch,
                crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                0,
            )],
            entities: vec![Located::new(
                point,
                crate::record_identity::RecordTypeId::from_bytes(POINT_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                1,
            )],
            transforms: Vec::new(),
            directions: Vec::new(),
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            project(&ctx, &inventory, &[]),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor sketch entity"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert!(project(&ctx, &inventory, &[]).is_ok());
    }

    #[test]
    fn sketch_reference_cannot_name_the_maximum_record_ordinal() {
        let mut sketch_bytes = content(0);
        sketch_bytes.extend_from_slice(&0_i32.to_le_bytes());
        sketch_bytes.extend_from_slice(&1_u32.to_le_bytes());
        sketch_bytes.extend(list(8, &[u32::MAX]));
        sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&[0; 8]);
        sketch_bytes.extend(list(2, &[]));
        let sketch = parse(&sketch_bytes, |ctx, source| {
            parse_sketch(ctx, source, 22).expect("sketch record")
        });
        let point = parse(&point_bytes(1, 1, [0.0, 0.0]), |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point record")
        });
        let inventory = SketchInventory {
            sketches: vec![Located::new(
                sketch,
                crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                0,
            )],
            entities: vec![Located::new(
                point,
                crate::record_identity::RecordTypeId::from_bytes(POINT_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                u32::MAX,
            )],
            transforms: Vec::new(),
            directions: Vec::new(),
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let projection = project(&ctx, &inventory, &[]).expect("sketch projection");
        assert_eq!(projection.unresolved_entities, 1);
    }

    #[test]
    fn sketch_projection_refuses_entity_limit_before_sketch_creation() {
        let mut transform = parse(
            &{
                let mut bytes = content(0);
                bytes.extend_from_slice(&0x8421_u16.to_le_bytes());
                bytes.extend_from_slice(&0x7bde_u16.to_le_bytes());
                bytes
            },
            |_, source| parse_transform(source, 22).expect("transform"),
        );
        transform.header.source_index = 0;
        let direction = parse(
            &{
                let mut bytes = content(1);
                bytes.extend_from_slice(&0_u32.to_le_bytes());
                bytes.extend_from_slice(&0.0_f64.to_le_bytes());
                bytes.extend_from_slice(&0.0_f64.to_le_bytes());
                bytes.extend_from_slice(&0.0_f64.to_le_bytes());
                bytes.extend_from_slice(&1.0_f64.to_le_bytes());
                bytes
            },
            |_, source| parse_direction(source, 22).expect("direction"),
        );
        let mut sketch_bytes = content(2);
        sketch_bytes.extend_from_slice(&0_i32.to_le_bytes());
        sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
        sketch_bytes.extend(list(8, &[]));
        sketch_bytes.extend_from_slice(&1_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&2_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&[0; 8]);
        sketch_bytes.extend(list(2, &[]));
        let sketch = parse(&sketch_bytes, |ctx, source| {
            parse_sketch(ctx, source, 22).expect("sketch")
        });
        let inventory = SketchInventory {
            sketches: vec![Located::new(
                sketch,
                crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                2,
            )],
            entities: Vec::new(),
            transforms: vec![Located::new(
                transform,
                crate::record_identity::RecordTypeId::from_bytes(TRANSFORM_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                0,
            )],
            directions: vec![Located::new(
                direction,
                crate::record_identity::RecordTypeId::from_bytes(DIRECTION_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                1,
            )],
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            project(&ctx, &inventory, &[]),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor sketch"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert_eq!(
            project(&ctx, &inventory, &[])
                .expect("service sketch projection")
                .sketches
                .len(),
            1
        );
    }

    #[test]
    fn sketch_constraint_projection_refuses_retained_limit_before_entity_copy() {
        let mut bytes = constraint_header(0, 0);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(1);
        let payload = parse(&bytes, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Horizontal, source, 22)
                .expect("horizontal constraint")
        });
        let constraint = Located::new(
            payload,
            crate::record_identity::RecordTypeId::from_bytes(HORIZONTAL_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            0,
        );
        let entity = cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#1")
                .expect("entity id"),
            cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
            cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                    position: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                },
            )
            .expect("point geometry"),
        );
        let entities = std::collections::HashMap::from([(("segment", 0), &entity)]);
        let parameters = std::collections::HashMap::new();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes =
            cadmpeg_core::decode::u64_from_index(entity.id().as_str().len()) - 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            super::project_constraint(&ctx, &constraint, &entities, &parameters, None),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor sketch constraint entity id"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert!(
            super::project_constraint(&ctx, &constraint, &entities, &parameters, None)
                .expect("horizontal constraint projects")
                .is_ok()
        );
    }

    #[test]
    fn sketch_constraint_projection_refuses_entity_limit_before_creation() {
        let mut bytes = constraint_header(0, 0);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(1);
        let payload = parse(&bytes, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Horizontal, source, 22)
                .expect("horizontal constraint")
        });
        let constraint = Located::new(
            payload,
            crate::record_identity::RecordTypeId::from_bytes(HORIZONTAL_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            0,
        );
        let entity = cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#1")
                .expect("entity id"),
            cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
            cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                    position: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                },
            )
            .expect("point geometry"),
        );
        let entities = std::collections::HashMap::from([(("segment", 0), &entity)]);
        let parameters = std::collections::HashMap::new();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            super::project_constraint(&ctx, &constraint, &entities, &parameters, None),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor sketch constraint"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert!(matches!(
            super::project_constraint(&ctx, &constraint, &entities, &parameters, None),
            Some(Ok(_))
        ));
    }

    fn inventory_with_record(
        type_id: [u8; 16],
        payload: &[u8],
        policy: DecodePolicy,
    ) -> Result<SketchInventory, CodecError> {
        let bytes = primary_envelope_fixture();
        let payload = payload.to_vec();
        let arena = DecodeArena::new();
        let (setup_ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("envelope view");
        let mut container = InventorContainer::open(&setup_ctx, source).expect("framed envelope");
        let segment = &mut container.rse.segments[0];
        segment.kind = SegmentKind::PmDc;
        let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
            panic!("framed bulk fixture");
        };
        let RecordFrameState::Framed(table) = &mut bulk.records else {
            panic!("framed record fixture");
        };
        table.records[0].type_id = type_id;
        table.records[0].payload = View::over_retained(&payload);
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input view");
        inventory(&ctx, &container.rse)
    }

    #[test]
    fn sketch_transform_record_refuses_collection_limit_before_push() {
        let mut payload = content(0);
        payload.extend_from_slice(&0x8421u16.to_le_bytes());
        payload.extend_from_slice(&0x7bdeu16.to_le_bytes());
        assert_eq!(
            inventory_with_record(TRANSFORM_TYPE, &payload, DecodePolicy::service())
                .expect("transform is admitted")
                .transforms
                .len(),
            1
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(matches!(
            inventory_with_record(TRANSFORM_TYPE, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor PmDc transform record"
                    && limit.used == 0
        ));
    }

    #[test]
    fn sketch_transform_record_refuses_retained_limit_before_identity_copy() {
        let mut payload = content(0);
        payload.extend_from_slice(&0x8421u16.to_le_bytes());
        payload.extend_from_slice(&0x7bdeu16.to_le_bytes());
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 31;
        assert!(matches!(
            inventory_with_record(TRANSFORM_TYPE, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor PmDc record type id"
                    && limit.used == 0
        ));
        policy.limits.max_retained_bytes = 32;
        assert!(matches!(
            inventory_with_record(TRANSFORM_TYPE, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor PmDc record segment token"
                    && limit.used == 32
        ));
    }

    #[test]
    fn sketch_parse_issue_refuses_collection_limit_before_push() {
        let admitted = inventory_with_record(TRANSFORM_TYPE, &[], DecodePolicy::service())
            .expect("truncated transform becomes an issue");
        assert_eq!(admitted.issues.len(), 1);
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(matches!(
            inventory_with_record(TRANSFORM_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor PmDc sketch issue"
                    && limit.used == 0
        ));
    }

    #[test]
    fn sketch_parse_issue_refuses_entity_limit_before_push() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        assert!(matches!(
            inventory_with_record(TRANSFORM_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit Inventor PmDc sketch issue"
        ));
        assert_eq!(
            inventory_with_record(TRANSFORM_TYPE, &[], DecodePolicy::service())
                .expect("service issue")
                .issues
                .len(),
            1
        );
    }

    #[test]
    fn sketch_parse_issue_copies_refuse_retained_limits_before_creation() {
        let detail_len = inventory_with_record(TRANSFORM_TYPE, &[], DecodePolicy::service())
            .expect("truncated transform becomes an issue")
            .issues[0]
            .detail
            .len();
        for (limit_bytes, operation, used) in [
            (
                detail_len - 1,
                "retain Inventor PmDc sketch issue detail",
                0,
            ),
            (
                detail_len,
                "retain Inventor PmDc sketch issue type id",
                detail_len,
            ),
            (
                detail_len + 32,
                "retain Inventor PmDc sketch issue segment token",
                detail_len + 32,
            ),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(limit_bytes);
            assert!(matches!(
                inventory_with_record(TRANSFORM_TYPE, &[], policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == operation
                        && limit.used == cadmpeg_core::decode::u64_from_index(used)
            ));
        }
    }

    #[test]
    fn sketch_record_forms_refuse_collection_limit_before_push() {
        let mut sketch = content(0);
        sketch.extend_from_slice(&0u32.to_le_bytes());
        sketch.extend_from_slice(&0u32.to_le_bytes());
        sketch.extend(list(8, &[]));
        sketch.extend_from_slice(&[0; 16]);
        let mut direction = content(0);
        direction.extend_from_slice(&[0; 36]);
        let mut constraint = constraint_header(0, 0);
        constraint.extend_from_slice(&0u32.to_le_bytes());
        constraint.push(1);
        for (type_id, payload, operation) in [
            (SKETCH_TYPE, sketch, "admit Inventor PmDc sketch record"),
            (
                POINT_TYPE,
                point_bytes(0, 0, [0.0, 0.0]),
                "admit Inventor PmDc sketch entity record",
            ),
            (
                DIRECTION_TYPE,
                direction,
                "admit Inventor PmDc direction record",
            ),
            (
                HORIZONTAL_TYPE,
                constraint,
                "admit Inventor PmDc sketch constraint record",
            ),
        ] {
            let admitted = inventory_with_record(type_id, &payload, DecodePolicy::service())
                .expect("sketch record is admitted");
            assert_eq!(
                admitted.sketches.len()
                    + admitted.entities.len()
                    + admitted.transforms.len()
                    + admitted.directions.len()
                    + admitted.constraints.len(),
                1
            );
            assert!(admitted.issues.is_empty());
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            assert!(matches!(
                inventory_with_record(type_id, &payload, policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == operation
                        && limit.used == 0
            ));
        }
    }

    fn list(marker: u16, references: &[u32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&marker.to_le_bytes());
        bytes.extend_from_slice(&0x3000u16.to_le_bytes());
        bytes.extend_from_slice(
            &(u32::try_from(references.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        if !references.is_empty() {
            if marker == 8 {
                bytes.extend_from_slice(&0u16.to_le_bytes());
                bytes.extend_from_slice(&0u16.to_le_bytes());
            } else {
                bytes.extend_from_slice(&0u32.to_le_bytes());
                bytes.extend_from_slice(&0u32.to_le_bytes());
            }
            for reference in references {
                bytes.extend_from_slice(&reference.to_le_bytes());
            }
        }
        bytes
    }

    fn entity_prefix(index: u32, sketch: u32, flags: u32) -> Vec<u8> {
        let mut bytes = content(index);
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&sketch.to_le_bytes());
        bytes
    }

    fn point_bytes(index: u32, sketch: u32, position: [f64; 2]) -> Vec<u8> {
        let mut bytes = entity_prefix(index, sketch, 0);
        for value in position {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend(list(2, &[]));
        bytes.extend(list(2, &[]));
        bytes
    }

    fn line_bytes(index: u32, sketch: u32, points: [u32; 2]) -> Vec<u8> {
        let mut bytes = entity_prefix(index, sketch, 0);
        bytes.extend(list(2, &points));
        bytes.extend(list(2, &[]));
        bytes.extend_from_slice(&0.0f64.to_le_bytes());
        bytes.extend_from_slice(&0.0f64.to_le_bytes());
        bytes.extend_from_slice(&1.0f64.to_le_bytes());
        bytes.extend_from_slice(&0.0f64.to_le_bytes());
        bytes
    }

    fn constraint_header(index: u32, parameter: u32) -> Vec<u8> {
        let mut bytes = content(index);
        bytes.extend_from_slice(&(-1i32).to_le_bytes());
        bytes.extend_from_slice(&0x8000_000cu32.to_le_bytes());
        bytes.extend(list(6, &[]));
        bytes.extend(list(6, &[]));
        bytes.extend_from_slice(&parameter.to_le_bytes());
        bytes
    }

    fn legacy_constraint_header(index: u32, parameter: u32) -> Vec<u8> {
        let mut bytes = content(index);
        bytes.extend_from_slice(&(-1i32).to_le_bytes());
        bytes.extend_from_slice(&0x8000_000cu32.to_le_bytes());
        bytes.extend_from_slice(&parameter.to_le_bytes());
        bytes
    }

    #[test]
    fn parses_generated_planar_geometry_branches() {
        let point = point_bytes(1, 3, [1.25, -2.5]);
        let parsed = parse(&point, |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Point {
                position,
                ..
            } if position.map(FiniteReal::get) == [1.25, -2.5]
        ));

        let line = line_bytes(2, 3, [4, 5]);
        let parsed = parse(&line, |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Line, source, 22).expect("line")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Line { ref points, .. }
                if points.references().iter().map(|value| value.index()).collect::<Vec<_>>() == [4, 5]
        ));

        let mut circle = entity_prefix(3, 3, 0);
        circle.extend(list(2, &[]));
        circle.extend(list(2, &[]));
        circle.extend_from_slice(&4u32.to_le_bytes());
        circle.extend_from_slice(&2.5f64.to_le_bytes());
        circle.push(1);
        let parsed = parse(&circle, |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Circle, source, 22).expect("circle")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Circle { radius, .. } if radius.get() == 2.5
        ));

        let mut ellipse = entity_prefix(4, 3, 0);
        ellipse.extend(list(2, &[]));
        ellipse.extend(list(2, &[]));
        ellipse.extend_from_slice(&4u32.to_le_bytes());
        ellipse.extend_from_slice(&1.0f64.to_le_bytes());
        ellipse.extend_from_slice(&0.0f64.to_le_bytes());
        ellipse.extend_from_slice(&3.0f64.to_le_bytes());
        ellipse.extend_from_slice(&2.0f64.to_le_bytes());
        ellipse.push(0);
        let parsed = parse(&ellipse, |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Ellipse, source, 22).expect("ellipse")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Ellipse {
                major_radius,
                minor_radius,
                ..
            } if major_radius.get() == 3.0 && minor_radius.get() == 2.0
        ));
    }

    #[test]
    fn parses_generated_constraint_branches() {
        for (type_id, tail, expected) in [
            (SketchConstraintTag::Coincident, vec![4, 5], "coincident"),
            (SketchConstraintTag::Parallel, vec![4, 5, 0], "parallel"),
            (
                SketchConstraintTag::Perpendicular,
                vec![4, 5, 0],
                "perpendicular",
            ),
            (SketchConstraintTag::Tangent, vec![4, 5, 0], "tangent"),
        ] {
            let mut bytes = constraint_header(9, 0);
            for (index, value) in tail.into_iter().enumerate() {
                if index == 2 && matches!(expected, "parallel" | "perpendicular") {
                    bytes.extend_from_slice(
                        &(u16::try_from(value).expect("fixture value fits u16")).to_le_bytes(),
                    );
                } else {
                    bytes.extend_from_slice(
                        &(u32::try_from(value).expect("fixture value fits u32")).to_le_bytes(),
                    );
                }
            }
            let parsed = parse(&bytes, |ctx, source| {
                parse_constraint(ctx, type_id, source, 22).expect(expected)
            });
            assert_eq!(
                match parsed.kind {
                    PmDcSketchConstraintKind::Coincident { .. } => "coincident",
                    PmDcSketchConstraintKind::Parallel { .. } => "parallel",
                    PmDcSketchConstraintKind::Perpendicular { .. } => "perpendicular",
                    PmDcSketchConstraintKind::Tangent { .. } => "tangent",
                    _ => "unexpected",
                },
                expected
            );
        }

        for (type_id, expected) in [
            (SketchConstraintTag::Horizontal, true),
            (SketchConstraintTag::Vertical, false),
        ] {
            let mut bytes = constraint_header(10, 0);
            bytes.extend_from_slice(&4u32.to_le_bytes());
            bytes.push(1);
            let parsed = parse(&bytes, |ctx, source| {
                parse_constraint(ctx, type_id, source, 22).expect("axis constraint")
            });
            assert_eq!(
                matches!(parsed.kind, PmDcSketchConstraintKind::Horizontal { .. }),
                expected
            );
        }
    }

    #[test]
    fn parses_generated_legacy_constraint_header_without_maps() {
        let mut bytes = legacy_constraint_header(9, 0);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&5u32.to_le_bytes());
        let parsed = parse(&bytes, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Coincident, source, 16)
                .expect("legacy coincident")
        });
        assert!(parsed.header.scalar_map.entries().is_empty());
        assert!(parsed.header.reference_map.entries().is_empty());
        assert!(matches!(
            parsed.kind,
            PmDcSketchConstraintKind::Coincident { first, second }
                if first.index() == 4 && second.index() == 5
        ));
    }

    #[test]
    fn parses_generated_dimensional_constraint_branches() {
        for type_id in [
            SketchConstraintTag::HorizontalDistance,
            SketchConstraintTag::VerticalDistance,
        ] {
            let mut bytes = constraint_header(11, 0);
            bytes.extend_from_slice(&4u32.to_le_bytes());
            bytes.extend_from_slice(&5u32.to_le_bytes());
            bytes.extend_from_slice(&12u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 16]);
            parse(&bytes, |ctx, source| {
                parse_constraint(ctx, type_id, source, 22).expect("distance constraint")
            });
        }
        let mut radius = constraint_header(12, 13);
        radius.extend_from_slice(&0u32.to_le_bytes());
        radius.extend_from_slice(&4u32.to_le_bytes());
        radius.extend_from_slice(&[0; 16]);
        parse(&radius, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Radius, source, 22).expect("radius")
        });

        let mut diameter = constraint_header(13, 14);
        diameter.extend_from_slice(&0u32.to_le_bytes());
        diameter.extend_from_slice(&4u32.to_le_bytes());
        diameter.extend_from_slice(&[0; 16]);
        parse(&diameter, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Diameter, source, 22).expect("diameter")
        });

        for type_id in [
            SketchConstraintTag::CircleCenter,
            SketchConstraintTag::EqualRadius,
        ] {
            let mut bytes = constraint_header(14, 0);
            bytes.extend_from_slice(&4u32.to_le_bytes());
            bytes.extend_from_slice(&5u32.to_le_bytes());
            parse(&bytes, |ctx, source| {
                parse_constraint(ctx, type_id, source, 22).expect("circle relation")
            });
        }
    }

    #[test]
    fn projects_generated_closed_square_and_resolved_plane() {
        let mut transform = parse(
            &{
                let mut bytes = content(0);
                bytes.extend_from_slice(&0x8421u16.to_le_bytes());
                bytes.extend_from_slice(&0x7bdeu16.to_le_bytes());
                bytes
            },
            |_, source| parse_transform(source, 22).expect("transform"),
        );
        let direction = parse(
            &{
                let mut bytes = content(1);
                bytes.extend_from_slice(&0u32.to_le_bytes());
                bytes.extend_from_slice(&0.0f64.to_le_bytes());
                bytes.extend_from_slice(&0.0f64.to_le_bytes());
                bytes.extend_from_slice(&0.0f64.to_le_bytes());
                bytes.extend_from_slice(&1.0f64.to_le_bytes());
                bytes
            },
            |_, source| parse_direction(source, 22).expect("direction"),
        );
        let mut sketch_bytes = content(2);
        sketch_bytes.extend_from_slice(&0i32.to_le_bytes());
        sketch_bytes.extend_from_slice(&8u32.to_le_bytes());
        sketch_bytes.extend(list(8, &[4, 5, 6, 7, 8, 9, 10, 11]));
        sketch_bytes.extend_from_slice(&1u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&2u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&[0; 8]);
        sketch_bytes.extend(list(2, &[]));
        let sketch = parse(&sketch_bytes, |ctx, source| {
            parse_sketch(ctx, source, 22).expect("sketch")
        });
        let points = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]];
        let mut entities = points
            .into_iter()
            .enumerate()
            .map(|(index, position)| {
                parse(
                    &point_bytes(
                        u32::try_from(index).expect("fixture value fits u32") + 3,
                        3,
                        position,
                    ),
                    |ctx, source| {
                        parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point")
                    },
                )
            })
            .collect::<Vec<_>>();
        for (index, endpoints) in [[4, 5], [5, 6], [6, 7], [7, 4]].into_iter().enumerate() {
            let mut line = parse(
                &line_bytes(
                    u32::try_from(index).expect("fixture value fits u32") + 7,
                    3,
                    endpoints,
                ),
                |ctx, source| parse_entity(ctx, SketchEntityTag::Line, source, 22).expect("line"),
            );
            let start = points[cadmpeg_core::decode::index_from_u32(endpoints[0]) - 4];
            let end = points[cadmpeg_core::decode::index_from_u32(endpoints[1]) - 4];
            let PmDcSketchEntityKind::Line {
                origin, direction, ..
            } = &mut line.kind
            else {
                unreachable!("generated line")
            };
            *origin = start.map(real);
            *direction = [real(end[0] - start[0]), real(end[1] - start[1])];
            entities.push(line);
        }
        transform.header.source_index = 0;
        let transform = Located::new(
            transform,
            crate::record_identity::RecordTypeId::from_bytes(TRANSFORM_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            0,
        );
        let direction = Located::new(
            direction,
            crate::record_identity::RecordTypeId::from_bytes(DIRECTION_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            1,
        );
        let located_sketch = Located::new(
            sketch.clone(),
            crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            2,
        );
        let entities = entities
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let type_id = if index < 4 { POINT_TYPE } else { LINE_TYPE };
                Located::new(
                    value,
                    crate::record_identity::RecordTypeId::from_bytes(type_id),
                    cadmpeg_ir::identity_key!("segment")
                        .try_clone_for_decode(
                            &cadmpeg_test_support::service_decode_context(),
                            "Inventor located fixture token",
                        )
                        .expect("service fixture token"),
                    u32::try_from(index).expect("fixture value fits u32") + 3,
                )
            })
            .collect();
        let mut inventory = SketchInventory {
            sketches: vec![located_sketch],
            entities,
            transforms: vec![transform],
            directions: vec![direction],
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let projected = project(&ctx, &inventory, &[]).expect("sketch projection");
        assert_eq!(projected.unresolved_sketches, 0);
        assert_eq!(projected.unresolved_entities, 0);
        assert_eq!(projected.sketches[0].profiles[0].len(), 4);
        assert!(matches!(
            projected.sketches[0].placement,
            SketchPlacement::Resolved { .. }
        ));

        let mut mapped_constraint = content(11);
        mapped_constraint.extend_from_slice(&(-1i32).to_le_bytes());
        mapped_constraint.extend_from_slice(&0u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&6u16.to_le_bytes());
        mapped_constraint.extend_from_slice(&0x3000u16.to_le_bytes());
        mapped_constraint.extend_from_slice(&1u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&[0; 8]);
        mapped_constraint.extend_from_slice(&4u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&0.5f64.to_le_bytes());
        mapped_constraint.extend(list(6, &[]));
        mapped_constraint.extend_from_slice(&0u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&4u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&5u32.to_le_bytes());
        let mapped_constraint = parse(&mapped_constraint, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Coincident, source, 22)
                .expect("mapped coincident")
        });
        inventory.constraints.push(Located::new(
            mapped_constraint,
            crate::record_identity::RecordTypeId::from_bytes(COINCIDENT_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            11,
        ));
        let mut sketch = sketch;
        let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
        references.push(PmDcReference::new(12, false).expect("test reference index fits 31 bits"));
        sketch.entities =
            PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
        inventory.sketches[0] = Located::new(
            sketch,
            crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            2,
        );
        // The scalar map gives a locus parameter; the point-point relation
        // projects and the sketch stays closed.
        let mapped = project(&ctx, &inventory, &[]).expect("mapped sketch projection");
        assert_eq!(mapped.unresolved_sketches, 0);
        assert_eq!(mapped.unresolved_constraints, 0);
        assert_eq!(mapped.constraints.len(), 1);
        let mut sketch = super::PmDcSketchPayload::clone(&inventory.sketches[0]);
        let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
        references.push(PmDcReference::new(14, false).expect("test reference index fits 31 bits"));
        sketch.entities =
            PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
        inventory.sketches[0] = Located::new(
            sketch,
            crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            2,
        );
        let incomplete = project(&ctx, &inventory, &[]).expect("incomplete sketch projection");
        assert_eq!(incomplete.unresolved_sketches, 1);
        assert_eq!(incomplete.unresolved_constraints, 1);
        assert!(incomplete.sketches.is_empty());
        assert!(incomplete.entities.is_empty());
    }
    #[test]
    fn point_tail_wire_requires_both_fields() {
        let list = serde_json::json!({"marker": 2, "metadata": null, "references": []});
        let mut wire = serde_json::json!({
            "form": "point", "position": [0.0, 0.0],
            "endpoint_of": list.clone(), "center_of": list.clone(),
            "state": null, "associations": null
        });
        let point: PmDcSketchEntityKind =
            serde_json::from_value(wire.clone()).expect("paired point fixture round-trips");
        assert_eq!(
            serde_json::to_value(point).expect("paired point fixture round-trips"),
            wire
        );
        wire["state"] = serde_json::json!(0);
        assert!(serde_json::from_value::<PmDcSketchEntityKind>(wire.clone()).is_err());
        wire["associations"] = list;
        let point: PmDcSketchEntityKind =
            serde_json::from_value(wire.clone()).expect("paired point fixture round-trips");
        assert_eq!(
            serde_json::to_value(point).expect("paired point fixture round-trips"),
            wire
        );
        wire["state"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<PmDcSketchEntityKind>(wire).is_err());
    }
    #[test]
    fn transform_prefix_wire_is_constant_or_absent() {
        let mut bytes = content(1);
        bytes.extend_from_slice(&0x8421u16.to_le_bytes());
        bytes.extend_from_slice(&0x7bdeu16.to_le_bytes());
        let transform = parse(&bytes, |_, source| {
            parse_transform(source, 22).expect("constant-prefix transform fixture is valid")
        });
        let mut wire =
            serde_json::to_value(transform).expect("constant-prefix transform fixture is valid");
        for (prefix, present) in [
            (serde_json::Value::Null, false),
            (serde_json::json!(515), true),
        ] {
            wire["prefix"] = prefix;
            let parsed: PmDcTransformPayload = serde_json::from_value(wire.clone())
                .expect("constant-prefix transform fixture is valid");
            assert_eq!(parsed.prefix_present, present);
            assert_eq!(
                serde_json::to_value(parsed).expect("constant-prefix transform fixture is valid"),
                wire
            );
        }
        wire["prefix"] = serde_json::json!(516);
        assert!(serde_json::from_value::<PmDcTransformPayload>(wire.clone()).is_err());
        wire.as_object_mut()
            .expect("constant-prefix transform fixture is valid")
            .remove("prefix");
        assert!(
            !serde_json::from_value::<PmDcTransformPayload>(wire)
                .expect("constant-prefix transform fixture is valid")
                .prefix_present
        );
    }
    #[test]
    fn large_collinear_sketch_carrier_matches() {
        assert!(super::line_carrier_matches(
            [0., 0.],
            [1e200, 1e200],
            [1e200, 1e200],
            [2e200, 2e200]
        ));
        assert!(!super::line_carrier_matches(
            [0., 0.],
            [1e200, 0.],
            [0., 0.],
            [0., 1e200]
        ));
    }

    #[test]
    fn sketch_carrier_agreement_is_independent_of_scale() {
        for scale in [f64::from_bits(1), 5e-11, 1.0, 1e200] {
            assert!(super::line_carrier_matches(
                [0.0, 0.0],
                [scale, 0.0],
                [scale, 0.0],
                [2.0 * scale, 0.0],
            ));
            assert!(super::line_carrier_matches(
                [0.0, 0.0],
                [scale, 0.0],
                [0.0, 0.0],
                [scale, 0.0],
            ));
            assert!(!super::line_carrier_matches(
                [0.0, 0.0],
                [scale, 0.0],
                [0.0, scale],
                [scale, scale],
            ));
        }
        for invalid in [0.0, f64::NAN, f64::INFINITY] {
            assert!(!super::line_carrier_matches(
                [0.0, 0.0],
                [invalid, 0.0],
                [0.0, 0.0],
                [1.0, 0.0],
            ));
        }
    }
    #[test]
    fn audit_regression_distant_parallel_segment_is_not_on_carrier() {
        for origin in [[0., 0.], [1e12, 0.]] {
            assert!(!super::line_carrier_matches(
                origin,
                [1., 0.],
                [1e12, 1.],
                [1e12 + 1., 1.]
            ));
            assert!(super::line_carrier_matches(
                origin,
                [1., 0.],
                [1e12, 0.],
                [1e12 + 1., 0.]
            ));
        }
    }
    #[test]
    fn audit_regression_distant_oblique_point_keeps_stored_direction_ratio() {
        assert!(super::line_carrier_matches(
            [0., 0.],
            [1., 12.],
            [1e12, 12e12],
            [1e12 + 1., 12e12 + 12.]
        ));
        assert!(!super::line_carrier_matches(
            [0., 0.],
            [1., 12.],
            [1e12, 12e12 + 1.],
            [1e12 + 1., 12e12 + 13.]
        ));
    }

    mod records;
    mod serialization;
}
