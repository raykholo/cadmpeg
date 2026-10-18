// SPDX-License-Identifier: Apache-2.0
//! Typed `PmDc` feature records and feature-list terminators.
use cadmpeg_ir::native::bytes::NativeBytes;

use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::FeatureResultTopologyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::{
    features::{
        edge_treatments::{ChamferGroup, ChamferSpec, FilletGroup, RadiusSpec},
        holes::{HoleKind, HolePlacement},
        BooleanOp, DesignParameter, DistinctMembers, EdgeSelection, ExtrudeDirection,
        ExtrudeExtent, ExtrudeSide, ExtrudeStart, ExtrusionDirectionSource, Feature,
        FeatureContent, FeatureDefinition, FeatureId, FeatureOperation, FeatureResultTopology,
        LinearTermination, ParameterValue,
    },
    scalar::{Angle, Length},
};
use serde::{Deserialize, Serialize};

use crate::pmdc::{
    content_header, inventor_id, reference_list, type_id_string, u32_list, Cursor,
    PmDcContentHeader, PmDcReferenceList, PmDcU32List,
};
use crate::record_identity::{push_record, Located, RecordPayload};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};
use crate::{design::DesignInventory, sketch::SketchInventory};

mod history;

const EPS_FEATURE_PROJECT_HOLE_E10: f64 = 1.0e-10;

const FEATURE_TYPE: [u8; 16] = [
    0x91, 0x4d, 0x87, 0x90, 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const END_OF_FEATURES_TYPE: [u8; 16] = [
    0x24, 0xfd, 0x41, 0x8f, 0xd2, 0x11, 0xac, 0x6e, 0x00, 0x08, 0x2a, 0xab, 0x32, 0xa3, 0xdc, 0x09,
];
const BOOLEAN_TYPE: [u8; 16] = inventor_id(0x9087_4d28);
const SURFACE_BODY_TYPE: [u8; 16] = inventor_id(0x9087_4d47);
const ENTITY_STYLE_LINK_TYPE: [u8; 16] = inventor_id(0x9087_4d15);
const PART_OPERATION_TYPE: [u8; 16] = [
    0x28, 0xbe, 0x9a, 0x72, 0xd1, 0x11, 0x44, 0x09, 0x00, 0x08, 0x4e, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
const BOUNDARY_PATCH_TYPE: [u8; 16] = [
    0x91, 0x73, 0x94, 0x22, 0xd1, 0x11, 0x07, 0xcf, 0x00, 0x08, 0x35, 0xbd, 0x06, 0x63, 0xdc, 0x09,
];
const EXTENT_TYPE: [u8; 16] = [
    0x29, 0x7d, 0x63, 0x92, 0xd1, 0x11, 0x3c, 0xb9, 0x00, 0x08, 0x31, 0xbd, 0x06, 0x63, 0xdc, 0x09,
];
const FEATURE_DIMENSIONS_TYPE: [u8; 16] = [
    0x71, 0xf2, 0x3e, 0xd8, 0xd2, 0x11, 0x50, 0x94, 0xa0, 0x00, 0x49, 0x80, 0x36, 0x03, 0xc8, 0xc9,
];
const RDX_VARIABLE_TYPE: [u8; 16] = [
    0xdf, 0xd5, 0x1d, 0xbb, 0xd1, 0x11, 0x6e, 0x72, 0x00, 0x08, 0x17, 0xbd, 0x06, 0x63, 0xdc, 0x09,
];
const AUXILIARY_ENUM_TYPE: [u8; 16] = [
    0x73, 0x39, 0xfd, 0xce, 0x7a, 0x4e, 0x40, 0x11, 0xbe, 0xe3, 0x43, 0x89, 0x79, 0x08, 0xba, 0x92,
];
const OBJECT_COLLECTION_TYPE: [u8; 16] = [
    0xae, 0x70, 0x68, 0x0e, 0xd1, 0x4a, 0x1e, 0x86, 0xd7, 0x62, 0x48, 0xb0, 0x3c, 0x2a, 0x96, 0xe1,
];
const HOLE_TYPE: [u8; 16] = [
    0x11, 0x7c, 0xcd, 0x43, 0xd2, 0x11, 0x96, 0x58, 0xa0, 0x00, 0x21, 0x80, 0x36, 0x03, 0xc8, 0xc9,
];
const PLACEMENT_TYPE: [u8; 16] = [
    0x2c, 0x92, 0x56, 0x72, 0x4d, 0x4d, 0x6d, 0x70, 0x94, 0x27, 0xfd, 0x96, 0x4d, 0x84, 0xdf, 0x16,
];
const FILLET_EDGE_SETS_TYPE: [u8; 16] = [
    0xda, 0xe9, 0x48, 0x1b, 0xd2, 0x11, 0xdc, 0x2c, 0x00, 0x08, 0x3e, 0xab, 0x1b, 0x14, 0xdc, 0x09,
];
const FILLET_EDGE_SET_TYPE: [u8; 16] = [
    0x16, 0x41, 0xd6, 0xaa, 0xd2, 0x11, 0xdb, 0x2c, 0x00, 0x08, 0x3e, 0xab, 0x1b, 0x14, 0xdc, 0x09,
];
const EDGE_COLLECTION_TYPE: [u8; 16] = inventor_id(0x9087_4d51);
const EDGE_ITEM_TYPE: [u8; 16] = [
    0x82, 0x69, 0x5c, 0x37, 0xd1, 0x11, 0x51, 0x6b, 0x00, 0x08, 0xa1, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
const FILLET_TYPE: [u8; 16] = [
    0x27, 0x88, 0xf2, 0x78, 0xc5, 0x4d, 0xd7, 0xbe, 0x43, 0x13, 0xb3, 0x98, 0x60, 0x39, 0xb5, 0x2e,
];
const CHAMFER_TYPE: [u8; 16] = [
    0x32, 0x00, 0xaa, 0x7d, 0xd2, 0x11, 0x2b, 0x83, 0x60, 0x00, 0xf3, 0xa8, 0x9d, 0xcc, 0xef, 0xb0,
];
const FILLET_EDGE_SELECTION_TYPE: [u8; 16] = [
    0x4a, 0x37, 0x49, 0x49, 0xd2, 0x11, 0x00, 0x1d, 0x00, 0x08, 0x3b, 0xab, 0x1b, 0x14, 0xdc, 0x09,
];
const RECTANGULAR_PATTERN_FEATURE_TYPE: [u8; 16] = [
    0x44, 0x32, 0x67, 0x20, 0xd2, 0x11, 0xc5, 0x1d, 0x60, 0x00, 0x2a, 0xab, 0x01, 0xf3, 0x1b, 0xb0,
];
const MIRROR_FEATURE_TYPE: [u8; 16] = [
    0xb5, 0xa9, 0xd9, 0xfa, 0xd2, 0x11, 0x05, 0x33, 0x60, 0x00, 0x2c, 0xab, 0x01, 0xf3, 0x1b, 0xb0,
];
const PROFILE_SELECTION_TYPE: [u8; 16] = [
    0x3b, 0x24, 0x77, 0xa4, 0xd1, 0x11, 0x8f, 0x96, 0x00, 0x08, 0x26, 0xbd, 0x06, 0x63, 0xdc, 0x09,
];
const FEATURE_CONTAINER_TYPE: [u8; 16] = inventor_id(0x9087_4d16);
const KERNEL_ENTITY_LINK_TYPE: [u8; 16] = [
    0x21, 0x75, 0x0f, 0xcc, 0xd1, 0x11, 0x27, 0x80, 0xe3, 0x86, 0x19, 0x96, 0x22, 0x59, 0x01, 0x7a,
];
const WORK_PLANE_TYPE: [u8; 16] = [
    0x42, 0xdf, 0x52, 0xce, 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const TERMINATION_FLAG_TYPE: [u8; 16] = [
    0xfd, 0x24, 0x89, 0x3d, 0xd2, 0x11, 0xfa, 0x99, 0x00, 0x08, 0x40, 0xab, 0x06, 0x63, 0xdc, 0x09,
];
const SURFACE_TYPE: [u8; 16] = [
    0xf0, 0x80, 0x12, 0x15, 0xd4, 0x11, 0x9e, 0x8e, 0xef, 0x90, 0xa8, 0xac, 0x7f, 0x9a, 0xc3, 0xff,
];
/// Surface-form value of a plane in a surface record.
const PLANE_SURFACE_FORM: u32 = 25;
/// Work-plane value that precedes the explicit plane frame in its extended form.
const WORK_PLANE_FRAME_MARKER: u32 = 0x203;
const FEATURE_LABEL_TYPE: [u8; 16] = [
    0x2b, 0xa4, 0x48, 0x2b, 0xd2, 0x11, 0x58, 0x64, 0x60, 0x00, 0x74, 0xb7, 0x9b, 0x49, 0xeb, 0xb0,
];

#[derive(Debug)]
pub(crate) struct FeatureInventory {
    pub(crate) containers: Vec<PmDcFeatureContainer>,
    pub(crate) features: Vec<PmDcFeature>,
    pub(crate) pattern_features: Vec<PmDcPatternFeature>,
    pub(crate) terminators: Vec<PmDcFeatureTerminator>,
    pub(crate) properties: Vec<PmDcFeatureProperty>,
    pub(crate) labels: Vec<PmDcFeatureLabel>,
    pub(crate) entity_style_links: Vec<PmDcEntityStyleLink>,
    pub(crate) kernel_entity_links: Vec<PmDcKernelEntityLink>,
    pub(crate) work_planes: Vec<PmDcWorkPlane>,
    pub(crate) issues: Vec<RecordIssue>,
}

pub(crate) struct FeatureProjection {
    pub(crate) features: Vec<Feature>,
    pub(crate) result_topologies: Vec<FeatureResultTopology>,
    pub(crate) unresolved_features: usize,
    pub(crate) unresolved_states: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcFeaturePropertyPayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) kind: PmDcFeaturePropertyKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcFeaturePropertyKind {
    Enumeration {
        family: PmDcFeatureEnumFamily,
        type_value: i16,
        value: u16,
    },
    WideEnumeration {
        type_value: u32,
        value: u32,
    },
    Boolean {
        name: String,
        name_value: u32,
        value: bool,
    },
    References {
        family: PmDcFeatureReferenceFamily,
        items: PmDcReferenceList,
    },
    RdxVariable {
        name: String,
        name_value: u32,
        nominal_value: u32,
        model_value: u32,
    },
    SurfaceBody {
        body: crate::pmdc::PmDcReference,
    },
    ProfileSelection {
        entity_link: crate::pmdc::PmDcReference,
        value: u8,
    },
    Placement {
        transform: crate::pmdc::PmDcReference,
        point: crate::pmdc::PmDcReference,
        value: crate::pmdc::PmDcReference,
    },
    FilletEdgeSet {
        edges: crate::pmdc::PmDcReference,
        radius: crate::pmdc::PmDcReference,
        selection: crate::pmdc::PmDcReference,
        continuity: crate::pmdc::PmDcReference,
    },
    EdgeItem {
        index_references: PmDcU32List,
        index_reference_value: i32,
        value: u32,
    },
    TerminationFlag {
        value: u8,
    },
    PlaneSurface {
        references: [crate::pmdc::PmDcReference; 3],
        state: u8,
        surface_form: u32,
        value: u32,
        origin: [cadmpeg_ir::scalar::FiniteReal; 3],
        u_axis: [cadmpeg_ir::scalar::FiniteReal; 3],
        v_axis: [cadmpeg_ir::scalar::FiniteReal; 3],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcFeatureEnumFamily {
    PartOperation,
    Extent,
    Hole,
    Fillet,
    Chamfer,
    Auxiliary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PmDcPatternFamily {
    Rectangular,
    Mirror,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcPatternFeaturePayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    state: i32,
    outline_value: u32,
    pub(crate) properties: PmDcReferenceList,
    value: u32,
    pub(crate) participants: PmDcReferenceList,
    family: PmDcPatternFamily,
    pub(crate) property_slots: Vec<crate::pmdc::PmDcReference>,
    control: u8,
    extension_values: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcFeatureReferenceFamily {
    BoundaryPatch,
    FeatureDimensions,
    ObjectCollection,
    FilletEdgeSets,
    EdgeCollection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcLinkedHeader {
    header_value: u32,
    header_id: u16,
    values: [u32; 2],
    pub(crate) owner: crate::pmdc::PmDcReference,
    pub(crate) parent: crate::pmdc::PmDcReference,
    pub(crate) next: crate::pmdc::PmDcReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct ClassId(NativeBytes<[u8; 16]>);

impl std::fmt::Display for ClassId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for ClassId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl TryFrom<String> for ClassId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("class_id must contain 32 lowercase hexadecimal digits".into());
        }
        let mut bytes = [0; 16];
        for (byte, digits) in bytes.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
            let digit = |value: u8| match value {
                b'0'..=b'9' => value - b'0',
                _ => value - b'a' + 10,
            };
            *byte = digit(digits[0]) * 16 + digit(digits[1]);
        }
        Ok(Self(bytes.into()))
    }
}

impl From<ClassId> for String {
    fn from(value: ClassId) -> Self {
        type_id_string(value.0.into_inner())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "PmDcFeatureLabelPayloadWire")]
pub(crate) struct PmDcFeatureLabelPayload {
    save_version_major: u8,
    pub(crate) header: PmDcLinkedHeader,
    index: u32,
    pub(crate) participants: PmDcReferenceList,
    name: String,
    class_id: ClassId,
}

#[derive(Serialize)]
struct PmDcFeatureLabelPayloadRef<'a> {
    save_version_major: u8,
    header: &'a PmDcLinkedHeader,
    index: u32,
    participants: &'a PmDcReferenceList,
    name: &'a str,
    class_id: &'a ClassId,
}

impl Serialize for PmDcFeatureLabelPayload {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PmDcFeatureLabelPayloadRef {
            save_version_major: self.save_version_major,
            header: &self.header,
            index: self.index,
            participants: &self.participants,
            name: self.name.as_str(),
            class_id: &self.class_id,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct PmDcFeatureLabelPayloadWire {
    save_version_major: u8,
    header: PmDcLinkedHeader,
    index: u32,
    participants: PmDcReferenceList,
    name: String,
    class_id: String,
}

impl TryFrom<PmDcFeatureLabelPayloadWire> for PmDcFeatureLabelPayload {
    type Error = String;
    fn try_from(wire: PmDcFeatureLabelPayloadWire) -> Result<Self, Self::Error> {
        Ok(Self {
            save_version_major: wire.save_version_major,
            header: wire.header,
            index: wire.index,
            participants: wire.participants,
            name: wire.name,
            class_id: ClassId::try_from(wire.class_id)?,
        })
    }
}

impl From<PmDcFeatureLabelPayload> for PmDcFeatureLabelPayloadWire {
    fn from(value: PmDcFeatureLabelPayload) -> Self {
        Self {
            save_version_major: value.save_version_major,
            header: value.header,
            index: value.index,
            participants: value.participants,
            name: value.name.as_str().to_owned(),
            class_id: value.class_id.into(),
        }
    }
}

impl PmDcFeatureLabelPayload {
    pub(crate) fn name(&self) -> Option<&str> {
        (!self.name.trim().is_empty()).then_some(self.name.as_str())
    }

    pub(crate) fn class_id(&self) -> ClassId {
        self.class_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcEntityStyleLinkPayload {
    save_version_major: u8,
    pub(crate) header: PmDcLinkedHeader,
    value: u32,
    associative_id: u32,
    entity_type: u32,
}

/// Link from a selection to one entity of a `PmDc` kernel body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcKernelEntityLinkPayload {
    save_version_major: u8,
    header_value: u32,
    header_id: u16,
    pub(crate) selection_source: u32,
    state: u8,
    pub(crate) body: crate::pmdc::PmDcReference,
    entity_id: u32,
    entity_type: u32,
}

/// A planar work feature with its resolved frame in internal centimetres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcWorkPlanePayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    entity_flags: u32,
    values: Vec<u32>,
    marker: bool,
    pub(crate) origin: [cadmpeg_ir::scalar::FiniteReal; 3],
    pub(crate) u_axis: [cadmpeg_ir::scalar::FiniteReal; 3],
    pub(crate) v_axis: [cadmpeg_ir::scalar::FiniteReal; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcFeaturePayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    state: i32,
    outline_value: u32,
    pub(crate) properties: PmDcReferenceList,
    value: u32,
}

/// Owner of one browser feature tree and the B-rep segment it builds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcFeatureContainerPayload {
    save_version_major: u8,
    header_value: u32,
    header_id: u16,
    pub(crate) next: crate::pmdc::PmDcReference,
    flags: u32,
    identifier: NativeBytes<[u8; 16]>,
    pub(crate) name: String,
    items: PmDcReferenceList,
    values: [u32; 15],
    pub(crate) segment: String,
    value: cadmpeg_ir::scalar::FiniteReal,
    terminal: u32,
}

/// Segment name of the container whose tree is the part modeling history.
pub(crate) const MODEL_HISTORY_SEGMENT: &str = "PmBRepSegment";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcFeatureTerminatorPayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    state: i32,
}

pub(crate) fn inventory(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
) -> Result<FeatureInventory, CodecError> {
    let mut inventory = FeatureInventory {
        containers: Vec::new(),
        features: Vec::new(),
        pattern_features: Vec::new(),
        terminators: Vec::new(),
        properties: Vec::new(),
        labels: Vec::new(),
        entity_style_links: Vec::new(),
        kernel_entity_links: Vec::new(),
        work_planes: Vec::new(),
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
            let parsed = match record.type_id {
                FEATURE_TYPE => parse_feature(ctx, record.payload, version).and_then(|feature| {
                    push_record(
                        ctx,
                        &mut inventory.features,
                        feature,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc feature record",
                    )
                }),
                RECTANGULAR_PATTERN_FEATURE_TYPE | MIRROR_FEATURE_TYPE => {
                    let family = if record.type_id == RECTANGULAR_PATTERN_FEATURE_TYPE {
                        PmDcPatternFamily::Rectangular
                    } else {
                        PmDcPatternFamily::Mirror
                    };
                    parse_pattern_feature(ctx, record.payload, version, family).and_then(
                        |feature| {
                            push_record(
                                ctx,
                                &mut inventory.pattern_features,
                                feature,
                                record.type_id,
                                segment.pair.token.key(),
                                record.ordinal,
                                "admit Inventor PmDc pattern feature record",
                            )
                        },
                    )
                }
                END_OF_FEATURES_TYPE => {
                    parse_terminator(record.payload, version).and_then(|terminator| {
                        push_record(
                            ctx,
                            &mut inventory.terminators,
                            terminator,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc feature terminator record",
                        )
                    })
                }
                FEATURE_CONTAINER_TYPE => {
                    parse_container(ctx, record.payload, version).and_then(|container| {
                        push_record(
                            ctx,
                            &mut inventory.containers,
                            container,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc feature container record",
                        )
                    })
                }
                FEATURE_LABEL_TYPE => parse_label(ctx, record.payload, version).and_then(|label| {
                    push_record(
                        ctx,
                        &mut inventory.labels,
                        label,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc feature label record",
                    )
                }),
                ENTITY_STYLE_LINK_TYPE => parse_entity_style_link(record.payload, version)
                    .and_then(|link| {
                        push_record(
                            ctx,
                            &mut inventory.entity_style_links,
                            link,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc entity style link record",
                        )
                    }),
                KERNEL_ENTITY_LINK_TYPE => parse_kernel_entity_link(record.payload, version)
                    .and_then(|link| {
                        push_record(
                            ctx,
                            &mut inventory.kernel_entity_links,
                            link,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc kernel entity link record",
                        )
                    }),
                WORK_PLANE_TYPE => parse_work_plane(record.payload, version).and_then(|plane| {
                    push_record(
                        ctx,
                        &mut inventory.work_planes,
                        plane,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc work plane record",
                    )
                }),
                SURFACE_TYPE => parse_surface(record.payload, version).and_then(|surface| {
                    surface.map_or(Ok(()), |surface| {
                        push_record(
                            ctx,
                            &mut inventory.properties,
                            surface,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc surface record",
                        )
                    })
                }),
                type_id => feature_property_parser(type_id).map_or_else(
                    || Ok(()),
                    |parser| {
                        parser(ctx, record.payload, version).and_then(|property| {
                            push_record(
                                ctx,
                                &mut inventory.properties,
                                property,
                                record.type_id,
                                segment.pair.token.key(),
                                record.ordinal,
                                "admit Inventor PmDc feature property record",
                            )
                        })
                    },
                ),
            };
            if let Err(error) = parsed {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                ctx.charge_collection_items(1, "admit Inventor PmDc feature issue")?;
                ctx.charge_entities(1, "admit Inventor PmDc feature issue")?;
                ctx.charge_formatted_retained(
                    format_args!("{error}"),
                    "retain Inventor PmDc feature issue detail",
                )?;
                ctx.charge_retained(32, "retain Inventor PmDc feature issue type id")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(segment.pair.token.as_str().len()),
                    "retain Inventor PmDc feature issue segment token",
                )?;
                inventory.issues.push(RecordIssue {
                    family: RecordIssueFamily::Feature {
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

fn parse_pattern_feature(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
    family: PmDcPatternFamily,
) -> Result<PmDcPatternFeaturePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("pattern-feature state")?.cast_signed();
    let outline_value = cursor.u32("pattern-feature outline value")?;
    let properties = reference_list(ctx, &mut cursor, 2, "pattern-feature properties")?;
    let value = cursor.u32("pattern-feature value")?;
    let participants = reference_list(ctx, &mut cursor, 2, "pattern-feature participants")?;
    let slot_count: usize = match family {
        PmDcPatternFamily::Rectangular if version > 20 => 32,
        PmDcPatternFamily::Rectangular => 26,
        PmDcPatternFamily::Mirror => 13,
    };
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(slot_count),
        "admit Inventor pattern feature property slots",
    )?;
    let mut property_slots =
        ctx.vector_storage(slot_count, "admit Inventor pattern feature property slots")?;
    for _ in 0..6 {
        property_slots.push(cursor.reference("pattern-feature property slot")?);
    }
    let control = cursor.u8("pattern-feature control")?;
    let mut extension_values = Vec::new();
    match family {
        PmDcPatternFamily::Rectangular => {
            let remaining = if version > 20 { 26 } else { 20 };
            for _ in 0..remaining {
                property_slots.push(cursor.reference("pattern-feature property slot")?);
            }
        }
        PmDcPatternFamily::Mirror => {
            for _ in 0..5 {
                property_slots.push(cursor.reference("pattern-feature property slot")?);
            }
            if version > 20 {
                ctx.charge_collection_items(6, "admit Inventor pattern feature extension values")?;
                ctx.reserve_capacity(
                    &mut extension_values,
                    6,
                    "admit Inventor pattern feature extension values",
                )?;
                for _ in 0..6 {
                    extension_values.push(cursor.u32("pattern-feature extension value")?);
                }
            }
            for _ in 0..2 {
                property_slots.push(cursor.reference("pattern-feature property slot")?);
            }
        }
    }
    cursor.finish("pattern feature")?;
    Ok(PmDcPatternFeaturePayload {
        save_version_major: version,
        header,
        state,
        outline_value,
        properties,
        value,
        participants,
        family,
        property_slots,
        control,
        extension_values,
    })
}

fn parse_feature(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("feature state")?.cast_signed();
    let outline_value = cursor.u32("feature outline value")?;
    let properties = reference_list(ctx, &mut cursor, 2, "feature property list")?;
    let value = cursor.u32("feature value")?;
    cursor.finish("feature")?;
    Ok(PmDcFeaturePayload {
        save_version_major: version,
        header,
        state,
        outline_value,
        properties,
        value,
    })
}

fn parse_container(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureContainerPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("feature container header value")?;
    let header_id = cursor.u16("feature container header id")?;
    let next = cursor.reference("feature container next reference")?;
    let flags = cursor.u32("feature container flags")?;
    let identifier = cursor.take_array("feature container identifier")?;
    let name = cursor.utf16(ctx, "feature container name")?;
    let items = reference_list(ctx, &mut cursor, 2, "feature container items")?;
    let values = cursor.u32_array("feature container values")?;
    let segment = cursor.utf16(ctx, "feature container segment name")?;
    let value = cursor.f64("feature container value")?;
    let terminal = cursor.u32("feature container terminal value")?;
    cursor.finish("feature container")?;
    Ok(PmDcFeatureContainerPayload {
        save_version_major: version,
        header_value,
        header_id,
        next,
        flags,
        identifier: identifier.into(),
        name,
        items,
        values,
        segment,
        value,
        terminal,
    })
}

fn parse_terminator(
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureTerminatorPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("feature terminator state")?.cast_signed();
    cursor.finish("feature terminator")?;
    Ok(PmDcFeatureTerminatorPayload {
        save_version_major: version,
        header,
        state,
    })
}

type PropertyParser =
    for<'a> fn(&DecodeContext<'a>, View<'a>, u8) -> Result<PmDcFeaturePropertyPayload, CodecError>;

fn feature_property_parser(type_id: [u8; 16]) -> Option<PropertyParser> {
    match type_id {
        PART_OPERATION_TYPE => Some(parse_part_operation),
        EXTENT_TYPE => Some(parse_extent),
        HOLE_TYPE => Some(parse_hole),
        FILLET_TYPE => Some(parse_fillet),
        CHAMFER_TYPE => Some(parse_chamfer),
        FILLET_EDGE_SELECTION_TYPE => Some(parse_fillet_edge_selection),
        AUXILIARY_ENUM_TYPE => Some(parse_auxiliary_enum),
        BOOLEAN_TYPE => Some(parse_boolean),
        BOUNDARY_PATCH_TYPE => Some(parse_boundary_patch),
        FEATURE_DIMENSIONS_TYPE => Some(parse_feature_dimensions),
        OBJECT_COLLECTION_TYPE => Some(parse_object_collection),
        FILLET_EDGE_SETS_TYPE => Some(parse_fillet_edge_sets),
        RDX_VARIABLE_TYPE => Some(parse_rdx_variable),
        SURFACE_BODY_TYPE => Some(parse_surface_body),
        PROFILE_SELECTION_TYPE => Some(parse_profile_selection),
        PLACEMENT_TYPE => Some(parse_placement),
        FILLET_EDGE_SET_TYPE => Some(parse_fillet_edge_set),
        EDGE_COLLECTION_TYPE => Some(parse_edge_collection),
        EDGE_ITEM_TYPE => Some(parse_edge_item),
        TERMINATION_FLAG_TYPE => Some(parse_termination_flag),
        _ => None,
    }
}

fn property(
    version: u8,
    header: PmDcContentHeader,
    kind: PmDcFeaturePropertyKind,
) -> PmDcFeaturePropertyPayload {
    PmDcFeaturePropertyPayload {
        save_version_major: version,
        header,
        kind,
    }
}

fn parse_enumeration(
    source: View<'_>,
    version: u8,
    family: PmDcFeatureEnumFamily,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let type_value = cursor.i16("feature enumeration type value")?;
    let value = cursor.u16("feature enumeration value")?;
    cursor.finish("feature enumeration")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::Enumeration {
            family,
            type_value,
            value,
        },
    ))
}

macro_rules! enum_parser {
    ($name:ident, $family:ident) => {
        fn $name(
            _: &DecodeContext<'_>,
            source: View<'_>,
            version: u8,
        ) -> Result<PmDcFeaturePropertyPayload, CodecError> {
            parse_enumeration(source, version, PmDcFeatureEnumFamily::$family)
        }
    };
}

enum_parser!(parse_part_operation, PartOperation);
enum_parser!(parse_extent, Extent);
enum_parser!(parse_hole, Hole);
enum_parser!(parse_fillet, Fillet);
enum_parser!(parse_auxiliary_enum, Auxiliary);

fn parse_chamfer(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let type_value = cursor.i16("chamfer enumeration type value")?;
    let value = cursor.u16("chamfer enumeration value")?;
    let terminal = cursor.u32("chamfer enumeration terminal")?;
    if terminal != 0 {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc chamfer enumeration terminal value is {terminal}"
        )));
    }
    cursor.finish("chamfer enumeration")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Chamfer,
            type_value,
            value,
        },
    ))
}

fn parse_fillet_edge_selection(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let type_value = cursor.u32("fillet edge-selection type value")?;
    let value = cursor.u32("fillet edge-selection value")?;
    cursor.finish("fillet edge-selection enumeration")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::WideEnumeration { type_value, value },
    ))
}

fn parse_boolean(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let name = cursor.utf16(ctx, "feature Boolean name")?;
    let name_value = cursor.u32("feature Boolean name value")?;
    let raw = cursor.u8("feature Boolean raw value")?;
    if raw > 1 {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc feature Boolean value is {raw}"
        )));
    }
    cursor.finish("feature Boolean")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::Boolean {
            name,
            name_value,
            value: raw != 0,
        },
    ))
}

fn parse_references(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
    family: PmDcFeatureReferenceFamily,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let items = reference_list(ctx, &mut cursor, 2, "feature-property references")?;
    cursor.finish("feature-property references")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::References { family, items },
    ))
}

macro_rules! reference_parser {
    ($name:ident, $family:ident) => {
        fn $name(
            ctx: &DecodeContext<'_>,
            source: View<'_>,
            version: u8,
        ) -> Result<PmDcFeaturePropertyPayload, CodecError> {
            parse_references(ctx, source, version, PmDcFeatureReferenceFamily::$family)
        }
    };
}

reference_parser!(parse_boundary_patch, BoundaryPatch);
reference_parser!(parse_feature_dimensions, FeatureDimensions);
reference_parser!(parse_object_collection, ObjectCollection);
reference_parser!(parse_fillet_edge_sets, FilletEdgeSets);
reference_parser!(parse_edge_collection, EdgeCollection);

fn parse_rdx_variable(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let name = cursor.utf16(ctx, "feature RDx variable name")?;
    let name_value = cursor.u32("feature RDx variable name value")?;
    let nominal_value = cursor.u32("feature RDx variable nominal value")?;
    let model_value = cursor.u32("feature RDx variable model value")?;
    cursor.finish("feature RDx variable")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::RdxVariable {
            name,
            name_value,
            nominal_value,
            model_value,
        },
    ))
}

fn parse_surface_body(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let body = cursor.reference("feature surface body reference")?;
    cursor.finish("feature surface body")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SurfaceBody { body },
    ))
}

fn parse_profile_selection(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_link = cursor.reference("profile selection entity link")?;
    let value = cursor.u8("profile selection value")?;
    cursor.finish("profile selection")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::ProfileSelection { entity_link, value },
    ))
}

fn parse_kernel_entity_link(
    source: View<'_>,
    version: u8,
) -> Result<PmDcKernelEntityLinkPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("kernel entity link header value")?;
    let header_id = cursor.u16("kernel entity link header id")?;
    let selection_source = cursor.u32("kernel entity link selection source")?;
    let state = cursor.u8("kernel entity link state")?;
    let body = cursor.reference("kernel entity link body reference")?;
    let entity_id = cursor.u32("kernel entity link entity id")?;
    let entity_type = cursor.u32("kernel entity link entity type")?;
    cursor.finish("kernel entity link")?;
    Ok(PmDcKernelEntityLinkPayload {
        save_version_major: version,
        header_value,
        header_id,
        selection_source,
        state,
        body,
        entity_id,
        entity_type,
    })
}

fn f64_triple(
    cursor: &mut Cursor<'_>,
    field: &'static str,
) -> Result<[cadmpeg_ir::scalar::FiniteReal; 3], CodecError> {
    Ok([cursor.f64(field)?, cursor.f64(field)?, cursor.f64(field)?])
}

fn parse_work_plane(source: View<'_>, version: u8) -> Result<PmDcWorkPlanePayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_flags = cursor.u32("work plane entity flags")?;
    let mut values = vec![
        cursor.u32("work plane value")?,
        cursor.u32("work plane value")?,
    ];
    if version > 21 {
        values.push(cursor.u32("work plane value")?);
    }
    let marker = cursor.peek_u32("work plane frame marker")? == WORK_PLANE_FRAME_MARKER;
    if marker {
        cursor.u32("work plane frame marker")?;
    }
    values.push(cursor.u32("work plane value")?);
    let origin = f64_triple(&mut cursor, "work plane origin")?;
    let u_axis = f64_triple(&mut cursor, "work plane u axis")?;
    let v_axis = f64_triple(&mut cursor, "work plane v axis")?;
    cursor.finish("work plane")?;
    Ok(PmDcWorkPlanePayload {
        save_version_major: version,
        header,
        entity_flags,
        values,
        marker,
        origin,
        u_axis,
        v_axis,
    })
}

fn parse_termination_flag(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let value = cursor.u8("termination flag value")?;
    cursor.finish("termination flag")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::TerminationFlag { value },
    ))
}

/// Parse a surface record. Only the plane form is typed; other forms are not
/// admitted.
fn parse_surface(
    source: View<'_>,
    version: u8,
) -> Result<Option<PmDcFeaturePropertyPayload>, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let references = [
        cursor.reference("surface reference")?,
        cursor.reference("surface reference")?,
        cursor.reference("surface reference")?,
    ];
    let state = cursor.u8("surface state")?;
    let form = cursor.u32("surface form")?;
    if form != PLANE_SURFACE_FORM {
        return Ok(None);
    }
    let value = cursor.u32("surface value")?;
    let origin = f64_triple(&mut cursor, "surface plane origin")?;
    let u_axis = f64_triple(&mut cursor, "surface plane u axis")?;
    let v_axis = f64_triple(&mut cursor, "surface plane v axis")?;
    cursor.finish("plane surface")?;
    Ok(Some(property(
        version,
        header,
        PmDcFeaturePropertyKind::PlaneSurface {
            references,
            state,
            surface_form: form,
            value,
            origin,
            u_axis,
            v_axis,
        },
    )))
}

fn parse_entity_style_link(
    source: View<'_>,
    version: u8,
) -> Result<PmDcEntityStyleLinkPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = linked_header(&mut cursor)?;
    let value = cursor.u32("entity-style link value")?;
    let associative_id = cursor.u32("entity-style link associative id")?;
    let entity_type = cursor.u32("entity-style link entity type")?;
    cursor.finish("entity-style link")?;
    Ok(PmDcEntityStyleLinkPayload {
        save_version_major: version,
        header,
        value,
        associative_id,
        entity_type,
    })
}

fn parse_placement(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let transform = cursor.reference("feature placement transform reference")?;
    let point = cursor.reference("feature placement point reference")?;
    let value = cursor.reference("feature placement value reference")?;
    cursor.finish("feature placement")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::Placement {
            transform,
            point,
            value,
        },
    ))
}

fn parse_fillet_edge_set(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let edges = cursor.reference("fillet edge-set edges reference")?;
    let radius = cursor.reference("fillet edge-set radius reference")?;
    let selection = cursor.reference("fillet edge-set selection reference")?;
    let continuity = cursor.reference("fillet edge-set continuity reference")?;
    cursor.finish("fillet edge set")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::FilletEdgeSet {
            edges,
            radius,
            selection,
            continuity,
        },
    ))
}

fn parse_edge_item(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let index_references = u32_list(ctx, &mut cursor, 2, "edge-item index references")?;
    let index_reference_value = if index_references.values().is_empty() {
        -1
    } else {
        cursor.i32("edge-item index reference value")?
    };
    let value = cursor.u32("edge-item value")?;
    cursor.finish("edge item")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::EdgeItem {
            index_references,
            index_reference_value,
            value,
        },
    ))
}

fn linked_header(cursor: &mut Cursor<'_>) -> Result<PmDcLinkedHeader, CodecError> {
    Ok(PmDcLinkedHeader {
        header_value: cursor.u32("linked header value")?,
        header_id: cursor.u16("linked header id")?,
        values: [
            cursor.u32("linked header value 0")?,
            cursor.u32("linked header value 1")?,
        ],
        owner: cursor.reference("linked header owner reference")?,
        parent: cursor.reference("linked header parent reference")?,
        next: cursor.reference("linked header next reference")?,
    })
}

fn parse_label(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeatureLabelPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = linked_header(&mut cursor)?;
    let index = cursor.u32("feature label index")?;
    let participants = reference_list(ctx, &mut cursor, 2, "feature-label participants")?;
    let name = cursor.utf16(ctx, "feature label")?;
    let class_id_bytes = cursor.take_array("feature-label class id")?;
    ctx.charge_retained(32, "retain Inventor feature label class id")?;
    let class_id = type_id_string(class_id_bytes);
    cursor.finish("feature label")?;
    PmDcFeatureLabelPayload::try_from(PmDcFeatureLabelPayloadWire {
        save_version_major: version,
        header,
        index,
        participants,
        name,
        class_id,
    })
    .map_err(CodecError::malformed)
}

const EXTRUSION_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x31, 0x11, 0xa9, 0x0c, 0xd0, 0x11, 0x8b, 0x83, 0x00, 0x08, 0x19, 0xb0, 0x05, 0x24, 0xdc, 0x09,
]));
const FILLET_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0xdc, 0x15, 0xf7, 0xf1, 0xd1, 0x11, 0x42, 0x05, 0x00, 0x08, 0x30, 0xb0, 0x05, 0x24, 0xdc, 0x09,
]));
const CHAMFER_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x3f, 0x71, 0x00, 0xf9, 0xd2, 0x11, 0x8b, 0x6f, 0x60, 0x00, 0xf0, 0xa8, 0x9d, 0xcc, 0xef, 0xb0,
]));
const HOLE_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x1a, 0x7d, 0x75, 0x1f, 0xd2, 0x11, 0x9c, 0x54, 0xa0, 0x00, 0x20, 0x80, 0x36, 0x03, 0xc8, 0xc9,
]));

/// Native family names of feature-label classes outside the neutral families.
const NATIVE_CLASSES: &[([u8; 16], &str)] = &[
    (
        [
            0x05, 0xda, 0xba, 0x90, 0xd1, 0x11, 0x38, 0x61, 0x00, 0x08, 0x0b, 0xbd, 0x06, 0x63,
            0xdc, 0x09,
        ],
        "Sweep",
    ),
    (
        [
            0x06, 0x19, 0xaa, 0x28, 0xd4, 0x11, 0xc7, 0x7a, 0x60, 0x00, 0xc4, 0xa4, 0xca, 0xe1,
            0xfb, 0xb0,
        ],
        "Thread",
    ),
    (
        [
            0x12, 0x96, 0x3c, 0xb8, 0xd1, 0x11, 0x8a, 0x28, 0x60, 0x00, 0x8b, 0xb8, 0x01, 0xf3,
            0x1b, 0xb0,
        ],
        "Shell",
    ),
    (
        [
            0x2d, 0x11, 0xa9, 0x0c, 0xd0, 0x11, 0x8b, 0x83, 0x00, 0x08, 0x19, 0xb0, 0x05, 0x24,
            0xdc, 0x09,
        ],
        "Base",
    ),
    (
        [
            0x33, 0x67, 0x23, 0xc0, 0x7e, 0x40, 0xea, 0xcb, 0x2e, 0xff, 0x66, 0x9e, 0x9a, 0xdd,
            0x87, 0xf8,
        ],
        "Combine",
    ),
    (
        [
            0x3a, 0x6f, 0xed, 0x36, 0xd4, 0x11, 0x42, 0x88, 0x10, 0x00, 0xfc, 0xa5, 0xbb, 0xa6,
            0x47, 0xb5,
        ],
        "RevolveSurface",
    ),
    (
        [
            0x5f, 0x01, 0x54, 0x00, 0xd2, 0x11, 0x25, 0x1c, 0x60, 0x00, 0x67, 0xb7, 0x9b, 0x49,
            0xeb, 0xb0,
        ],
        "Revolve",
    ),
    (
        [
            0x68, 0x6e, 0x16, 0xd3, 0x33, 0x4f, 0x63, 0x71, 0x61, 0xa6, 0xa1, 0x9b, 0xdd, 0x3d,
            0x2b, 0xe8,
        ],
        "DeleteFace",
    ),
    (
        [
            0x7f, 0x58, 0x80, 0x68, 0xb7, 0x43, 0x78, 0xa0, 0x55, 0x95, 0x15, 0x94, 0x6f, 0x7a,
            0x1c, 0x12,
        ],
        "SheetMetalCorner",
    ),
    (
        [
            0x80, 0x8b, 0x00, 0xaf, 0xd4, 0x11, 0x40, 0x88, 0x10, 0x00, 0xfc, 0xa5, 0xbb, 0xa6,
            0x47, 0xb5,
        ],
        "LoftSurface",
    ),
    (
        [
            0x85, 0x5b, 0x33, 0xe7, 0x04, 0x45, 0xdf, 0x5c, 0x25, 0xb4, 0x8c, 0xb8, 0xa8, 0xb2,
            0x69, 0x22,
        ],
        "Decal",
    ),
    (
        [
            0x8c, 0x39, 0x5e, 0x28, 0x4c, 0x4f, 0xeb, 0x8b, 0xc9, 0x81, 0x4c, 0xab, 0xad, 0xec,
            0x94, 0x39,
        ],
        "Emboss",
    ),
    (
        [
            0x94, 0x26, 0xc2, 0xb6, 0xd2, 0x11, 0x26, 0x36, 0x60, 0x00, 0x2c, 0xab, 0x01, 0xf3,
            0x1b, 0xb0,
        ],
        "CircularPattern",
    ),
    (
        [
            0xa3, 0x26, 0xc2, 0xb6, 0xd2, 0x11, 0x26, 0x36, 0x60, 0x00, 0x2c, 0xab, 0x01, 0xf3,
            0x1b, 0xb0,
        ],
        "Mirror",
    ),
    (
        [
            0xaa, 0xf2, 0x10, 0x93, 0xd3, 0x44, 0x20, 0x9a, 0x04, 0xcc, 0x79, 0x96, 0x56, 0xcd,
            0x7e, 0x09,
        ],
        "FlatPatternDefinition",
    ),
    (
        [
            0xb5, 0x4d, 0xd6, 0xf6, 0xd3, 0x11, 0xc4, 0x96, 0x60, 0x00, 0xf2, 0xb2, 0x1d, 0x6e,
            0xef, 0xb0,
        ],
        "CornerRound",
    ),
    (
        [
            0xba, 0xa8, 0x71, 0x83, 0xd3, 0x11, 0x34, 0x7b, 0xc0, 0x00, 0xe3, 0x95, 0x45, 0xdf,
            0x72, 0x4f,
        ],
        "Split",
    ),
    (
        [
            0xc6, 0x5f, 0x58, 0x55, 0xd3, 0x11, 0x73, 0x0c, 0x60, 0x00, 0x3a, 0xa9, 0x9d, 0xcc,
            0xef, 0xb0,
        ],
        "SheetMetalPlate",
    ),
    (
        [
            0xda, 0x14, 0x19, 0xb4, 0xd2, 0x11, 0xda, 0xd0, 0x60, 0x00, 0x12, 0xa9, 0x9d, 0xcc,
            0xef, 0xb0,
        ],
        "Coil",
    ),
    (
        [
            0xda, 0xc2, 0x7a, 0x76, 0xd2, 0x11, 0x0d, 0x2e, 0x60, 0x00, 0x2a, 0xab, 0x01, 0xf3,
            0x1b, 0xb0,
        ],
        "RectangularPattern",
    ),
    (
        [
            0xde, 0xa6, 0xa6, 0x8f, 0x4c, 0x4b, 0x52, 0x91, 0x88, 0xca, 0x31, 0xb5, 0x63, 0x01,
            0x98, 0xc8,
        ],
        "Move",
    ),
    (
        [
            0xe0, 0xef, 0x54, 0x2f, 0xd1, 0x11, 0x34, 0x09, 0x00, 0x08, 0x4e, 0xba, 0x32, 0xa3,
            0xdc, 0x09,
        ],
        "Loft",
    ),
    (
        [
            0xe6, 0xda, 0x3f, 0x26, 0x92, 0x4b, 0x5d, 0xab, 0x8e, 0x1b, 0xc5, 0xa9, 0x3f, 0xea,
            0xd7, 0xde,
        ],
        "Rotate",
    ),
    (
        [
            0xe8, 0x99, 0xdb, 0xdf, 0xd3, 0x11, 0x65, 0xaf, 0xc0, 0x00, 0x01, 0x85, 0xe8, 0x9c,
            0x6b, 0x4f,
        ],
        "Tagging",
    ),
    (
        [
            0xe9, 0x9f, 0x83, 0x2b, 0x7a, 0x42, 0x95, 0xa1, 0xd9, 0x76, 0x33, 0x83, 0xbe, 0xa1,
            0x7c, 0xce,
        ],
        "MoveBody",
    ),
    (
        [
            0xec, 0xf4, 0x81, 0x72, 0x9c, 0x4d, 0x5f, 0x92, 0x5f, 0x21, 0x92, 0xb4, 0xa8, 0x30,
            0x96, 0xac,
        ],
        "Thicken",
    ),
    (
        [
            0xf0, 0xa4, 0x97, 0xe5, 0xd4, 0x11, 0xd8, 0x84, 0x10, 0x00, 0xfa, 0xa5, 0xbb, 0xa6,
            0x47, 0xb5,
        ],
        "ExtrudeSurface",
    ),
    (
        [
            0xf2, 0x58, 0xf7, 0x4c, 0xd3, 0x11, 0x4f, 0x24, 0x60, 0x00, 0x47, 0xa9, 0x9d, 0xcc,
            0xef, 0xb0,
        ],
        "SheetMetalBend",
    ),
];

#[derive(Clone, Copy)]
pub(crate) enum FeatureFamily {
    Extrusion,
    Fillet,
    Chamfer,
    Hole,
}

impl FeatureFamily {
    pub(crate) fn class_id(self) -> ClassId {
        match self {
            Self::Extrusion => EXTRUSION_CLASS_ID,
            Self::Fillet => FILLET_CLASS_ID,
            Self::Chamfer => CHAMFER_CLASS_ID,
            Self::Hole => HOLE_CLASS_ID,
        }
    }

    pub(crate) fn output_slot(self) -> usize {
        match self {
            Self::Extrusion => 26,
            Self::Fillet => 15,
            Self::Chamfer => 11,
            Self::Hole => 24,
        }
    }

    fn from_class_id(class_id: ClassId) -> Option<Self> {
        [Self::Extrusion, Self::Fillet, Self::Chamfer, Self::Hole]
            .into_iter()
            .find(|family| family.class_id() == class_id)
    }

    pub(crate) fn from_definition(definition: &FeatureDefinition) -> Option<Self> {
        match definition {
            FeatureDefinition::Operation(FeatureOperation::Extrude { .. }) => Some(Self::Extrusion),
            FeatureDefinition::Operation(FeatureOperation::Fillet { .. }) => Some(Self::Fillet),
            FeatureDefinition::Operation(FeatureOperation::Chamfer { .. }) => Some(Self::Chamfer),
            FeatureDefinition::Operation(FeatureOperation::Hole { .. }) => Some(Self::Hole),
            _ => None,
        }
    }
}

struct ProjectionIndex<'a> {
    properties: HashMap<(&'a str, u32), Option<&'a PmDcFeatureProperty>>,
    parameters: HashMap<(&'a str, u32), Option<&'a crate::design::PmDcParameter>>,
    parameter_values: HashMap<&'a str, &'a ParameterValue>,
    sketches: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcSketch>>,
    sketch_ids: HashMap<&'a str, cadmpeg_ir::sketches::SketchId>,
    directions: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcDirection>>,
    transforms: HashMap<(&'a str, u32), Option<&'a crate::sketch::PmDcTransform>>,
    entity_style_links: HashSet<(&'a str, u32)>,
    work_planes: HashMap<(&'a str, u32), Option<&'a PmDcWorkPlane>>,
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    inventory: &FeatureInventory,
    design: &DesignInventory,
    sketch: &SketchInventory,
    parameters: &[DesignParameter],
    sketches: &[Sketch],
) -> Result<FeatureProjection, CodecError> {
    let total = inventory
        .features
        .len()
        .checked_add(inventory.pattern_features.len())
        .ok_or_else(|| ctx.refuse_codec_limit("Inventor feature count", u64::MAX, u64::MAX))?;
    let mut feature_tokens = HashSet::new();
    for token in inventory
        .features
        .iter()
        .map(|feature| feature.identity.segment_token.as_str())
        .chain(
            inventory
                .pattern_features
                .iter()
                .map(|feature| feature.identity.segment_token.as_str()),
        )
    {
        ctx.insert_hash_set(&mut feature_tokens, token, "index Inventor feature token")?;
    }
    if feature_tokens.len() > 1 {
        return Ok(FeatureProjection {
            features: Vec::new(),
            result_topologies: Vec::new(),
            unresolved_features: total,
            unresolved_states: 0,
        });
    }

    let (unique_properties, _properties_storage) = ctx.unique_index(
        inventory.properties.iter().map(|record| {
            (
                {
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    )
                },
                record,
            )
        }),
        |key| {
            cadmpeg_core::decode::u64_from_index(key.0.len())
                .checked_add(5)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("index Inventor feature properties", 0, u64::MAX)
                })
        },
        "index Inventor feature properties",
    )?;
    let (unique_parameters, _parameters_storage) = ctx.unique_index(
        design.parameters.iter().map(|record| {
            (
                {
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    )
                },
                record,
            )
        }),
        |key| {
            cadmpeg_core::decode::u64_from_index(key.0.len())
                .checked_add(5)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("index Inventor feature parameters", 0, u64::MAX)
                })
        },
        "index Inventor feature parameters",
    )?;
    let (unique_sketches, _sketches_storage) = ctx.unique_index(
        sketch.sketches.iter().map(|record| {
            (
                {
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    )
                },
                record,
            )
        }),
        |key| {
            cadmpeg_core::decode::u64_from_index(key.0.len())
                .checked_add(5)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("index Inventor feature sketches", 0, u64::MAX)
                })
        },
        "index Inventor feature sketches",
    )?;
    let (unique_directions, _directions_storage) = ctx.unique_index(
        sketch.directions.iter().map(|record| {
            (
                {
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    )
                },
                record,
            )
        }),
        |key| {
            cadmpeg_core::decode::u64_from_index(key.0.len())
                .checked_add(5)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("index Inventor feature directions", 0, u64::MAX)
                })
        },
        "index Inventor feature directions",
    )?;
    let (unique_work_planes, _work_planes_storage) = ctx.unique_index(
        inventory.work_planes.iter().map(|record| {
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
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("index Inventor feature work planes", 0, u64::MAX)
                })
        },
        "index Inventor feature work planes",
    )?;
    let (unique_transforms, _transforms_storage) = ctx.unique_index(
        sketch.transforms.iter().map(|record| {
            (
                {
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    )
                },
                record,
            )
        }),
        |key| {
            cadmpeg_core::decode::u64_from_index(key.0.len())
                .checked_add(5)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("index Inventor feature transforms", 0, u64::MAX)
                })
        },
        "index Inventor feature transforms",
    )?;
    let index = ProjectionIndex {
        properties: unique_properties,
        parameters: unique_parameters,
        parameter_values: {
            ctx.charge_collection_items(
                cadmpeg_core::decode::u64_from_index(
                    parameters
                        .iter()
                        .filter(|parameter| {
                            parameter.native_ref.is_some() && parameter.value.is_some()
                        })
                        .count(),
                ),
                "index Inventor feature parameter values",
            )?;
            parameters
                .iter()
                .filter_map(|parameter| {
                    Some((parameter.native_ref.as_deref()?, parameter.value.as_ref()?))
                })
                .collect()
        },
        sketches: unique_sketches,
        sketch_ids: {
            let mut ids = HashMap::new();
            for sketch in sketches {
                if let Some(native) = sketch.native_ref.as_deref() {
                    let id = sketch
                        .id
                        .try_clone_for_decode(ctx, "retain Inventor feature sketch id")?;
                    ctx.insert_hash_map(&mut ids, native, id, "index Inventor feature sketch ids")?;
                }
            }
            ids
        },
        directions: unique_directions,
        transforms: unique_transforms,
        entity_style_links: {
            let mut links = HashSet::new();
            for record in &inventory.entity_style_links {
                let key = (
                    record.identity.segment_token.as_str(),
                    record.identity.record_ordinal,
                );
                ctx.insert_hash_set(&mut links, key, "index Inventor entity style link")?;
            }
            for record in &inventory.kernel_entity_links {
                let key = (
                    record.identity.segment_token.as_str(),
                    record.identity.record_ordinal,
                );
                ctx.insert_hash_set(&mut links, key, "index Inventor kernel entity link")?;
            }
            links
        },
        work_planes: unique_work_planes,
    };
    // The label owner is a one-based reference. Keying on the optional record
    // ordinal keeps the null reference out of the ordinal space, so a label
    // with no owner never claims the feature at ordinal 0.
    let (labels, _labels_storage) = ctx.unique_index(
        inventory.labels.iter().map(|label| {
            (
                {
                    (
                        label.identity.segment_token.as_str(),
                        label.header.owner.record_ordinal(),
                    )
                },
                label,
            )
        }),
        |key| {
            cadmpeg_core::decode::u64_from_index(key.0.len())
                .checked_add(5)
                .ok_or_else(|| ctx.refuse_codec_limit("index Inventor feature labels", 0, u64::MAX))
        },
        "index Inventor feature labels",
    )?;
    let token = feature_tokens.iter().next().copied();
    let history = match token {
        Some(token) => history::build(ctx, token, inventory, &sketch.sketches)?,
        None => history::History::default(),
    };
    let label_of = |ordinal: u32| {
        token.and_then(|token| {
            labels
                .get(&(token, Some(ordinal)))
                .and_then(Option::as_ref)
                .copied()
        })
    };
    let mut projected = Vec::new();
    let mut neutral = 0_usize;
    for feature in &inventory.features {
        let label = label_of(feature.identity.record_ordinal);
        let value = match label.and_then(|label| {
            FeatureFamily::from_class_id(label.class_id()).map(|family| (label, family))
        }) {
            Some((label, family)) => match family {
                FeatureFamily::Extrusion => project_extrusion(ctx, feature, label, &index),
                FeatureFamily::Fillet => project_fillet(ctx, feature, label, &index),
                FeatureFamily::Chamfer => project_chamfer(ctx, feature, label, &index),
                FeatureFamily::Hole => project_hole(ctx, feature, label, &index),
            }
            .transpose()?,
            None => None,
        };
        ctx.charge_collection_items(1, "project Inventor feature pair")?;
        match value {
            Some((projected_feature, result)) => {
                neutral += 1;
                projected.push(HistoryItem {
                    record: feature.identity.record_ordinal,
                    kind: history::HistoryNodeKind::Feature,
                    flags: feature.header.flags,
                    label,
                    feature: projected_feature,
                    result: Some(result),
                });
            }
            None if history.placement(feature.identity.record_ordinal).is_some() => {
                projected.push(HistoryItem {
                    record: feature.identity.record_ordinal,
                    kind: history::HistoryNodeKind::Feature,
                    flags: feature.header.flags,
                    label,
                    feature: native_feature(ctx, &feature.identity, label, feature.id())?,
                    result: None,
                });
            }
            None => {}
        }
    }
    for pattern in &inventory.pattern_features {
        if history.placement(pattern.identity.record_ordinal).is_none() {
            continue;
        }
        let label = label_of(pattern.identity.record_ordinal);
        ctx.charge_collection_items(1, "project Inventor pattern feature")?;
        projected.push(HistoryItem {
            record: pattern.identity.record_ordinal,
            kind: history::HistoryNodeKind::Pattern,
            flags: pattern.header.flags,
            label,
            feature: native_feature(ctx, &pattern.identity, label, pattern.id())?,
            result: None,
        });
    }
    for record in &sketch.sketches {
        let Some(placement) = history.placement(record.identity.record_ordinal) else {
            continue;
        };
        if placement.kind != history::HistoryNodeKind::Sketch {
            continue;
        }
        let label = label_of(record.identity.record_ordinal);
        ctx.charge_collection_items(1, "project Inventor sketch history node")?;
        projected.push(HistoryItem {
            record: record.identity.record_ordinal,
            kind: history::HistoryNodeKind::Sketch,
            flags: record.header.flags,
            label,
            feature: sketch_feature(ctx, record, label, &index)?,
            result: None,
        });
    }
    apply_history(ctx, &mut projected, &history, inventory, token)?;
    let unresolved_states = projected
        .iter()
        .filter(|item| item.result.is_some() && history.placement(item.record).is_none())
        .count();
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(projected.len()),
        "count Inventor feature ordinals",
    )?;
    let ordinal_counts = projected.iter().map(|item| item.feature.ordinal).fold(
        HashMap::<u64, usize>::new(),
        |mut counts, ordinal| {
            *counts.entry(ordinal).or_default() += 1;
            counts
        },
    );
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(
            ordinal_counts.values().filter(|count| **count > 1).count(),
        ),
        "collect duplicate Inventor feature ordinals",
    )?;
    let duplicate_ordinals = ordinal_counts
        .into_iter()
        .filter_map(|(ordinal, count)| (count > 1).then_some(ordinal))
        .collect::<HashSet<_>>();
    projected.retain(|item| {
        let keep = !duplicate_ordinals.contains(&item.feature.ordinal);
        if !keep && item.result.is_some() {
            neutral -= 1;
        }
        keep
    });
    ctx.sort_unstable_by(
        &mut projected,
        |left, right| left.feature.ordinal.cmp(&right.feature.ordinal),
        |_| 0,
        "Inventor projected features sort",
    )?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(projected.len()),
        "collect Inventor projected features",
    )?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(projected.len()),
        "collect Inventor feature topologies",
    )?;
    let mut features = Vec::new();
    let mut result_topologies = Vec::new();
    for item in projected {
        features.push(item.feature);
        if let Some(result) = item.result {
            result_topologies.push(result);
        }
    }
    Ok(FeatureProjection {
        unresolved_features: total.checked_sub(neutral).ok_or_else(|| {
            CodecError::malformed("Inventor feature projection exceeds inventory")
        })?,
        unresolved_states: unresolved_states.min(neutral),
        features,
        result_topologies,
    })
}

/// One projected history node and the record that supplies it.
struct HistoryItem<'a> {
    record: u32,
    kind: history::HistoryNodeKind,
    flags: u32,
    label: Option<&'a PmDcFeatureLabel>,
    feature: Feature,
    result: Option<FeatureResultTopology>,
}

/// Apply browser order, suppression, end-of-part state, and consumed-node
/// dependencies to the projected history nodes.
fn apply_history(
    ctx: &DecodeContext<'_>,
    projected: &mut [HistoryItem<'_>],
    history: &history::History,
    inventory: &FeatureInventory,
    token: Option<&str>,
) -> Result<(), CodecError> {
    if history.placements.is_empty() {
        return Ok(());
    }
    let mut unplaced = projected
        .iter()
        .enumerate()
        .filter(|(_, item)| history.placement(item.record).is_none())
        .map(|(position, item)| (item.label.map(|label| label.index), item.record, position))
        .collect::<Vec<_>>();
    ctx.sort_unstable_by(
        &mut unplaced,
        |left, right| {
            (left.0.is_none(), left.0, left.1).cmp(&(right.0.is_none(), right.0, right.1))
        },
        |_| 0,
        "order unplaced Inventor features",
    )?;
    let mut next = history.next_ordinal;
    for (_, _, position) in unplaced {
        ctx.charge_work(1, "order unplaced Inventor features")?;
        if let Some(item) = projected.get_mut(position) {
            item.feature.ordinal = next;
            next = next
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("Inventor feature ordinal overflows"))?;
        }
    }
    let mut ids = HashMap::new();
    for item in projected.iter() {
        ctx.insert_hash_map(
            &mut ids,
            item.record,
            item.feature.id.clone(),
            "index Inventor history feature ids",
        )?;
    }
    let mut patterns = HashMap::new();
    for pattern in inventory
        .pattern_features
        .iter()
        .filter(|pattern| Some(pattern.identity.segment_token.as_str()) == token)
    {
        ctx.insert_hash_map(
            &mut patterns,
            pattern.identity.record_ordinal,
            pattern,
            "index Inventor history pattern features",
        )?;
    }
    for item in projected.iter_mut() {
        let Some(placement) = history.placement(item.record) else {
            continue;
        };
        item.feature.ordinal = placement.ordinal;
        let flagged = item.flags & history::SUPPRESSED_FLAG != 0;
        item.feature.suppressed = Some(flagged || placement.below_end_of_part);
        if flagged {
            insert_history_property(ctx, &mut item.feature, "suppressed", "true")?;
        }
        if placement.below_end_of_part {
            insert_history_property(ctx, &mut item.feature, "end_of_part", "below")?;
        }
        if !placement.model_history {
            insert_history_property(ctx, &mut item.feature, "history", "flat_pattern")?;
        }
        let consumed: Vec<u32> = match item.kind {
            history::HistoryNodeKind::Sketch => Vec::new(),
            history::HistoryNodeKind::Feature => item
                .label
                .map(|label| {
                    label
                        .participants
                        .references()
                        .iter()
                        .filter_map(|reference| reference.record_ordinal())
                        .collect()
                })
                .unwrap_or_default(),
            history::HistoryNodeKind::Pattern => patterns
                .get(&item.record)
                .map(|pattern| {
                    pattern
                        .participants
                        .references()
                        .iter()
                        .filter_map(|reference| reference.record_ordinal())
                        .collect()
                })
                .unwrap_or_default(),
        };
        for ordinal in consumed {
            let Some(dependency) = ids.get(&ordinal) else {
                continue;
            };
            if history
                .placement(ordinal)
                .is_none_or(|earlier| earlier.ordinal >= placement.ordinal)
            {
                continue;
            }
            let dependency =
                dependency.try_clone_for_decode(ctx, "retain Inventor feature dependency")?;
            item.feature.dependencies.insert(
                ctx,
                dependency,
                "admit Inventor feature dependency",
            )?;
        }
    }
    Ok(())
}

fn insert_history_property(
    ctx: &DecodeContext<'_>,
    feature: &mut Feature,
    key: &'static str,
    value: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(key.len() + value.len()),
        "retain Inventor feature history property",
    )?;
    ctx.insert_btree_map(
        &mut feature.source_properties,
        cadmpeg_core::text::NonBlankString::new(key.to_owned()).ok_or_else(|| {
            CodecError::malformed("Inventor feature history property key is blank")
        })?,
        value.to_owned(),
        "project Inventor feature history property",
    )?;
    Ok(())
}

fn history_feature_id(
    ctx: &DecodeContext<'_>,
    identity: &crate::record_identity::RecordIdentity,
) -> Result<FeatureId, CodecError> {
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(
            "inventor:design:feature#".len() + identity.segment_token.as_str().len() + 11,
        ),
        "retain Inventor history feature identity",
    )?;
    Ok(FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
        identity.key(ctx)?,
    ))
}

fn admit_history_feature(
    ctx: &DecodeContext<'_>,
    label: Option<&PmDcFeatureLabel>,
    native_id: &str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "project Inventor history feature")?;
    ctx.charge_entities(1, "project Inventor history feature")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(
            label.map_or(0, |label| label.name.len()) + native_id.len(),
        ),
        "retain Inventor history feature name and native id",
    )?;
    Ok(())
}

/// A feature record whose operation graph is not closed, retained at its
/// history position with its native family.
fn native_feature(
    ctx: &DecodeContext<'_>,
    identity: &crate::record_identity::RecordIdentity,
    label: Option<&PmDcFeatureLabel>,
    native_id: String,
) -> Result<Feature, CodecError> {
    admit_history_feature(ctx, label, &native_id)?;
    let kind = native_kind(label.map(|label| label.class_id()));
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(kind.as_str().len()),
        "retain Inventor native feature kind",
    )?;
    Ok(Feature {
        id: history_feature_id(ctx, identity)?,
        ordinal: label.map_or(0, |label| u64::from(label.index)),
        name: label.and_then(|label| label.name()).map(str::to_owned),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some("native".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind,
                parameters: BTreeMap::new(),
            }),
        ),
        native_ref: Some(native_id),
    })
}

/// A planar sketch node of the history.
fn sketch_feature(
    ctx: &DecodeContext<'_>,
    record: &crate::sketch::PmDcSketch,
    label: Option<&PmDcFeatureLabel>,
    index: &ProjectionIndex<'_>,
) -> Result<Feature, CodecError> {
    let native_id = record.id();
    admit_history_feature(ctx, label, &native_id)?;
    let sketch = match index.sketch_ids.get(native_id.as_str()) {
        Some(id) => Some(id.try_clone_for_decode(ctx, "retain Inventor history sketch id")?),
        None => None,
    };
    Ok(Feature {
        id: history_feature_id(ctx, &record.identity)?,
        ordinal: label.map_or(0, |label| u64::from(label.index)),
        name: label.and_then(|label| label.name()).map(str::to_owned),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some("sketch".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(sketch),
            }),
        ),
        native_ref: Some(native_id),
    })
}

/// Native family of a feature-label class identifier.
fn native_kind(class_id: Option<ClassId>) -> cadmpeg_ir::features::NativeFeatureKind {
    use cadmpeg_ir::features::NativeFeatureKind;
    let Some(class_id) = class_id else {
        return NativeFeatureKind::Other("InventorFeature".into());
    };
    if let Some(family) = FeatureFamily::from_class_id(class_id) {
        return match family {
            FeatureFamily::Extrusion => NativeFeatureKind::Extrude,
            FeatureFamily::Fillet => NativeFeatureKind::Fillet,
            FeatureFamily::Chamfer => NativeFeatureKind::Chamfer,
            FeatureFamily::Hole => NativeFeatureKind::Other("Hole".into()),
        };
    }
    NATIVE_CLASSES
        .iter()
        .find(|(bytes, _)| *bytes == class_id.0.into_inner())
        .map_or_else(
            || NativeFeatureKind::Other(format!("InventorClass:{class_id}")),
            |(_, name)| NativeFeatureKind::Other((*name).to_owned()),
        )
}

fn project_extrusion(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let operation = enum16(source, 0, PmDcFeatureEnumFamily::PartOperation, index)?;
    let op = match operation {
        1 => BooleanOp::NewBody,
        2 => BooleanOp::Cut,
        3 => BooleanOp::Join,
        4 => BooleanOp::Intersect,
        _ => return None,
    };
    let boundary = references(source, 1, PmDcFeatureReferenceFamily::BoundaryPatch, index)?;
    if source.properties.references().get(23)? != source.properties.references().get(1)? {
        return None;
    }
    for (position, selection) in boundary.references().iter().enumerate() {
        if let Err(error) = ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(position),
            "check distinct Inventor extrusion selections",
        ) {
            return Some(Err(error));
        }
        if boundary.references()[..position]
            .iter()
            .any(|prior| prior.index() == selection.index())
        {
            return None;
        }
    }
    let mut selections = Vec::new();
    for reference in boundary.references() {
        let property = resolve_property(
            source.identity.segment_token.as_str(),
            reference.index(),
            index,
        )?;
        let PmDcFeaturePropertyKind::ProfileSelection { entity_link, .. } = &property.kind else {
            return None;
        };
        let ordinal = entity_link.index().checked_sub(1)?;
        if !index
            .entity_style_links
            .contains(&(source.identity.segment_token.as_str(), ordinal))
        {
            return None;
        }
        if let Err(error) = ctx.charge_collection_items(1, "collect Inventor extrusion selection") {
            return Some(Err(error));
        }
        if let Err(error) = ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(property.id_len()?),
            "retain Inventor extrusion selection id",
        ) {
            return Some(Err(error));
        }
        selections.push(property.id());
    }
    if selections.is_empty() || label.participants.references().len() != 1 {
        return None;
    }
    let sketch_reference = label.participants.references().first()?;
    let sketch = index
        .sketches
        .get(&(
            source.identity.segment_token.as_str(),
            sketch_reference.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)?;
    let sketch_native_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(sketch.id_len()?),
        "resolve Inventor extrusion sketch native id",
    );
    let _sketch_native_reservation = match sketch_native_reservation {
        Ok(reservation) => reservation,
        Err(error) => return Some(Err(error)),
    };
    let sketch_id = index.sketch_ids.get(sketch.id().as_str())?;
    let sketch_id = match sketch_id.try_clone_for_decode(ctx, "retain Inventor extrusion sketch id")
    {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };

    let direction_record = resolve_direction(source, 2, index)?;
    let mut direction = cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(
        direction_record.direction[0].get(),
        direction_record.direction[1].get(),
        direction_record.direction[2].get(),
    ))?;
    if boolean(source, 3, index)? {
        direction = direction.reversed();
    }
    let extent_value = enum16(source, 6, PmDcFeatureEnumFamily::Extent, index)?;
    let symmetric = boolean(source, 7, index)?;
    let second_extent = source
        .properties
        .references()
        .get(29)
        .is_some_and(|reference| reference.index() != 0);
    let first = extrusion_side(source, index, ExtrusionSideSlots::FIRST, extent_value)?;
    let start = if extent_value == EXTENT_FROM_TO {
        ExtrudeStart::FromFace {
            face: termination_target(source, index, 8, 10, 17)?,
            offset: None,
        }
    } else {
        ExtrudeStart::ProfilePlane {}
    };
    let extent = match (extent_value, symmetric, second_extent) {
        (EXTENT_SYMMETRIC_DISTANCE | EXTENT_THROUGH_ALL, true, false) => {
            ExtrudeExtent::Symmetric { side: first }
        }
        (EXTENT_SYMMETRIC_DISTANCE, false, _) | (_, true, true) => return None,
        (_, true, false) => return None,
        (_, false, true) => {
            let second_value = enum16(source, 29, PmDcFeatureEnumFamily::Extent, index)?;
            if !matches!(
                second_value,
                EXTENT_DISTANCE | EXTENT_THROUGH_NEXT | EXTENT_THROUGH_ALL
            ) || extent_value == EXTENT_FROM_TO
            {
                return None;
            }
            ExtrudeExtent::TwoSided {
                first,
                second: extrusion_side(source, index, ExtrusionSideSlots::SECOND, second_value)?,
            }
        }
        (_, false, false) => ExtrudeExtent::OneSided { side: first },
    };
    let (feature_id, result) = match feature_result(ctx, source, 26, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let source_properties = match boolean_properties(ctx, source, &[20, 22], index) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = admit_projected_feature(ctx, source, label, "extrude") {
        return Some(Err(error));
    }
    let profile = match PlanarProfileRef::sketch_selection(sketch_id, selections, ctx) {
        Ok(Ok(profile)) => profile,
        Ok(Err(_)) => return None,
        Err(limit) => return Some(Err(limit.into())),
    };
    let feature = Feature {
        id: feature_id,
        ordinal: u64::from(label.index),
        name: label.name().map(str::to_owned),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties,
        source_tag: Some("extrude".into()),
        source_text: None,
        source_content: FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(profile),
                direction: ExtrudeDirection::Explicit {
                    vector: cadmpeg_ir::features::FeatureDirection3::from(direction),
                    source: Some(ExtrusionDirectionSource::Custom {}),
                },
                start,
                extent,
                op,
                solid: Some(true),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            }),
        ),
        native_ref: Some(source.id()),
    };
    Some(Ok((feature, result)))
}

const EXTENT_DISTANCE: u16 = 1;
const EXTENT_SYMMETRIC_DISTANCE: u16 = 2;
const EXTENT_THROUGH_NEXT: u16 = 4;
const EXTENT_THROUGH_ALL: u16 = 5;
const EXTENT_FROM_TO: u16 = 6;
const EXTENT_TO: u16 = 7;

/// Property slots of one extrusion side.
#[derive(Clone, Copy)]
struct ExtrusionSideSlots {
    length: usize,
    taper: usize,
}

impl ExtrusionSideSlots {
    const FIRST: Self = Self {
        length: 4,
        taper: 5,
    };
    const SECOND: Self = Self {
        length: 27,
        taper: 28,
    };
}

fn extrusion_side(
    source: &PmDcFeature,
    index: &ProjectionIndex<'_>,
    slots: ExtrusionSideSlots,
    extent: u16,
) -> Option<ExtrudeSide> {
    let taper =
        cadmpeg_ir::scalar::SlopeAngle::try_from(angle_parameter(source, slots.taper, index)?)
            .ok()?;
    let termination = match extent {
        EXTENT_DISTANCE | EXTENT_SYMMETRIC_DISTANCE => LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::from(
                cadmpeg_ir::scalar::PositiveLength::try_from(length_parameter(
                    source,
                    slots.length,
                    index,
                )?)
                .ok()?,
            ),
        },
        EXTENT_THROUGH_NEXT => LinearTermination::ThroughNext {},
        EXTENT_THROUGH_ALL => LinearTermination::ThroughAll {},
        EXTENT_FROM_TO | EXTENT_TO => LinearTermination::ToFace {
            face: termination_target(source, index, 11, 13, 18)?,
            offset: None,
        },
        _ => return None,
    };
    Some(ExtrudeSide {
        termination,
        draft: (taper.get() != 0.0).then_some(taper),
    })
}

/// Resolve an extent target: a work plane, or a model face recorded as a
/// termination flag with value 1 and its plane surface.
fn termination_target(
    source: &PmDcFeature,
    index: &ProjectionIndex<'_>,
    plane_slot: usize,
    flag_slot: usize,
    surface_slot: usize,
) -> Option<cadmpeg_ir::features::FaceSelection> {
    let references = source.properties.references();
    let plane = references.get(plane_slot)?.index();
    let flag = references.get(flag_slot)?.index();
    let surface = references.get(surface_slot)?.index();
    let token = source.identity.segment_token.as_str();
    if plane != 0 {
        if flag != 0 || surface != 0 {
            return None;
        }
        let plane = index
            .work_planes
            .get(&(token, plane.checked_sub(1)?))
            .and_then(Option::as_ref)?;
        return Some(cadmpeg_ir::features::FaceSelection::Native(plane.id()));
    }
    if !matches!(
        resolve_property(token, flag, index)?.kind,
        PmDcFeaturePropertyKind::TerminationFlag { value: 1 }
    ) {
        return None;
    }
    let surface = resolve_property(token, surface, index)?;
    if !matches!(surface.kind, PmDcFeaturePropertyKind::PlaneSurface { .. }) {
        return None;
    }
    Some(cadmpeg_ir::features::FaceSelection::Native(surface.id()))
}

fn admit_projected_feature(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    tag: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "project Inventor feature")?;
    ctx.charge_entities(1, "project Inventor feature")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(label.name.as_str().len()),
        "retain Inventor projected feature name",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(tag.len()),
        "retain Inventor projected feature tag",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(source.id_len().ok_or_else(|| {
            CodecError::Malformed("Inventor identifier length exceeds address space".into())
        })?),
        "retain Inventor projected feature native id",
    )?;
    Ok(())
}

fn project_fillet(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    if enum16(source, 11, PmDcFeatureEnumFamily::Fillet, index)? != 0
        || source.properties.references().get(1)?.index() != 0
        || source.properties.references().get(10)?.index() != 0
    {
        return None;
    }
    let sets = references(source, 0, PmDcFeatureReferenceFamily::FilletEdgeSets, index)?;
    let mut groups = Vec::new();
    for reference in sets.references() {
        let set = resolve_property(
            source.identity.segment_token.as_str(),
            reference.index(),
            index,
        )?;
        let PmDcFeaturePropertyKind::FilletEdgeSet {
            edges,
            radius,
            selection,
            continuity,
        } = &set.kind
        else {
            return None;
        };
        let selection = resolve_property(
            source.identity.segment_token.as_str(),
            selection.index(),
            index,
        )?;
        if !matches!(
            selection.kind,
            PmDcFeaturePropertyKind::WideEnumeration {
                type_value: 4,
                value: 0
            }
        ) || !matches!(
            resolve_property(
                source.identity.segment_token.as_str(),
                continuity.index(),
                index
            )?
            .kind,
            PmDcFeaturePropertyKind::Boolean { value: false, .. }
        ) {
            return None;
        }
        let edge_collection =
            resolve_property(source.identity.segment_token.as_str(), edges.index(), index)?;
        let PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::EdgeCollection,
            items,
        } = &edge_collection.kind
        else {
            return None;
        };
        if !closed_edge_items(source.identity.segment_token.as_str(), items, index) {
            return None;
        }
        let radius = cadmpeg_ir::scalar::PositiveLength::new(
            length_reference(
                source.identity.segment_token.as_str(),
                radius.index(),
                index,
            )?
            .get(),
        )?;
        if let Err(error) = ctx.charge_collection_items(1, "collect Inventor fillet group") {
            return Some(Err(error));
        }
        if let Err(error) = ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(edge_collection.id_len()?),
            "retain Inventor fillet edge collection id",
        ) {
            return Some(Err(error));
        }
        groups.push(FilletGroup {
            edges: EdgeSelection::Native(edge_collection.id()),
            radius: RadiusSpec::Constant { radius },
            tangency_weight: None,
        });
    }
    if groups.is_empty() {
        return None;
    }
    let (feature_id, result) = match feature_result(ctx, source, 15, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let source_properties = match boolean_properties(ctx, source, &[2, 3, 4, 5, 8], index) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = admit_projected_feature(ctx, source, label, "fillet") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties,
            source_tag: Some("fillet".into()),
            source_text: None,
            source_content: FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Fillet {
                    groups: groups.try_into().ok()?,
                }),
            ),
            native_ref: Some(source.id()),
        },
        result,
    )))
}

fn project_chamfer(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    if enum16(source, 4, PmDcFeatureEnumFamily::Chamfer, index)? != 0 {
        return None;
    }
    let edges = slot_property(source, 0, index)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::EdgeCollection,
        items,
    } = &edges.kind
    else {
        return None;
    };
    if !closed_edge_items(source.identity.segment_token.as_str(), items, index) {
        return None;
    }
    let distance =
        cadmpeg_ir::scalar::PositiveLength::try_from(length_parameter(source, 2, index)?).ok()?;
    let flip_direction = boolean(source, 5, index)?;
    let (feature_id, result) = match feature_result(ctx, source, 11, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let source_properties = match boolean_properties(ctx, source, &[6, 9], index) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = admit_projected_feature(ctx, source, label, "chamfer") {
        return Some(Err(error));
    }
    if let Err(error) = ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(edges.id_len()?),
        "retain Inventor chamfer edge collection id",
    ) {
        return Some(Err(error));
    }
    if let Err(error) = ctx.charge_collection_items(1, "collect Inventor chamfer group") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties,
            source_tag: Some("chamfer".into()),
            source_text: None,
            source_content: FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Chamfer {
                    groups: cadmpeg_ir::features::NonEmptyMembers::one(ChamferGroup {
                        edges: EdgeSelection::Native(edges.id()),
                        spec: ChamferSpec::Distance { distance },
                    }),
                    flip_direction,
                }),
            ),
            native_ref: Some(source.id()),
        },
        result,
    )))
}

fn project_hole(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let hole_form = enum16(source, 0, PmDcFeatureEnumFamily::Hole, index)?;
    if boolean(source, 17, index)? {
        return None;
    }
    let diameter = length_parameter(source, 1, index)?;
    let depth = length_parameter(source, 2, index)?;
    let head_diameter = length_parameter(source, 3, index)?;
    let head_depth = length_parameter(source, 4, index)?;
    let head_angle = angle_parameter(source, 5, index)?;
    let point_angle = angle_parameter(source, 6, index)?;
    let kind = match hole_form {
        0 if point_angle.get() == 0.0 => HoleKind::Simple,
        0 => HoleKind::SimpleDrilled {
            drill_point_angle: cadmpeg_ir::scalar::InteriorAngle::try_from(point_angle).ok()?,
        },
        1 => HoleKind::Countersink {
            diameter: cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?,
            angle: cadmpeg_ir::scalar::InteriorAngle::try_from(head_angle).ok()?,
        },
        2 if point_angle.get() == 0.0 => HoleKind::Counterbore {
            diameter: cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?,
            depth: cadmpeg_ir::scalar::PositiveLength::try_from(head_depth).ok()?,
        },
        2 => HoleKind::CounterboreDrilled {
            diameter: cadmpeg_ir::scalar::PositiveLength::try_from(head_diameter).ok()?,
            depth: cadmpeg_ir::scalar::PositiveLength::try_from(head_depth).ok()?,
            drill_point_angle: cadmpeg_ir::scalar::InteriorAngle::try_from(point_angle).ok()?,
        },
        _ => return None,
    };
    let extent = match enum16(source, 9, PmDcFeatureEnumFamily::Extent, index)? {
        1 => LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::from(
                cadmpeg_ir::scalar::PositiveLength::try_from(depth).ok()?,
            ),
        },
        4 => LinearTermination::ThroughNext {},
        5 => LinearTermination::ThroughAll {},
        _ => return None,
    };
    let transform_reference = source.properties.references().get(8)?;
    let transform = index
        .transforms
        .get(&(
            source.identity.segment_token.as_str(),
            transform_reference.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)?;
    if transform.matrix.rows()[3]
        .iter()
        .zip([0.0, 0.0, 0.0, 1.0])
        .any(|(actual, expected)| (actual - expected).abs() > EPS_FEATURE_PROJECT_HOLE_E10)
    {
        return None;
    }
    let direction_record = resolve_direction(source, 16, index)?;
    let direction = cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(
        direction_record.direction[0].get(),
        direction_record.direction[1].get(),
        direction_record.direction[2].get(),
    ))?;
    let placement = slot_property(source, 21, index)?;
    let PmDcFeaturePropertyKind::Placement {
        transform: placement_transform,
        point,
        value,
    } = &placement.kind
    else {
        return None;
    };
    if placement_transform.index() != transform_reference.index()
        || point.index() == 0
        || value.index() == 0
    {
        return None;
    }
    let position = cadmpeg_ir::features::FinitePoint3::new(Point3::new(
        transform.matrix.rows()[0][3] * 10.0,
        transform.matrix.rows()[1][3] * 10.0,
        transform.matrix.rows()[2][3] * 10.0,
    ))?;
    let shape = cadmpeg_ir::features::holes::HoleShape::new(
        cadmpeg_ir::features::holes::HoleConstruction::Form {
            kind,
            specification: None,
        },
        None,
        Some(cadmpeg_ir::scalar::PositiveLength::try_from(diameter).ok()?),
    )
    .ok()?;
    let (feature_id, result) = match feature_result(ctx, source, 24, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = admit_projected_feature(ctx, source, label, "hole") {
        return Some(Err(error));
    }
    if let Err(error) = ctx.charge_collection_items(1, "project Inventor hole placement") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            id: feature_id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some("hole".into()),
            source_text: None,
            source_content: FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Hole {
                    profile: None,
                    profile_filter: None,
                    face: None,
                    direction: None,
                    placements: Some(vec![HolePlacement::Directed {
                        position,
                        direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
                    }]),
                    shape,

                    extent: Some(extent),
                    bottom: None,
                    taper_angle: None,
                    allow_multi_profile_faces: None,
                }),
            ),
            native_ref: Some(source.id()),
        },
        result,
    )))
}

fn feature_result(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(FeatureId, FeatureResultTopology), CodecError>> {
    let collection = slot_property(source, slot, index)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items,
    } = &collection.kind
    else {
        return None;
    };
    let mut bodies = Vec::new();
    for reference in items.references() {
        let body = resolve_property(
            source.identity.segment_token.as_str(),
            reference.index(),
            index,
        )?;
        if !matches!(body.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
            return None;
        }
        if let Err(error) = ctx.charge_collection_items(1, "collect Inventor feature result body") {
            return Some(Err(error));
        }
        if let Err(error) = ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(body.id_len()?),
            "retain Inventor feature result body id",
        ) {
            return Some(Err(error));
        }
        bodies.push(cadmpeg_core::text::NonBlankString::new(body.id())?);
    }
    if bodies.is_empty() {
        return None;
    }
    let members = match cadmpeg_ir::features::FeatureResultMembers::new(
        bodies,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        ctx,
        "precheck distinct Inventor feature result bodies",
    ) {
        Ok(Ok(members)) => members,
        Ok(Err(_)) => return None,
        Err(limit) => return Some(Err(limit.into())),
    };
    let key_len = source.identity.segment_token.as_str().len()
        + 1
        + match usize::try_from(source.identity.record_ordinal.max(1).ilog10()) {
            Ok(value) => value,
            Err(_) => {
                return Some(Err(CodecError::Malformed(
                    "Inventor numeric value exceeds target range".into(),
                )))
            }
        }
        + 1;
    let feature_id_len = "inventor:design:feature#".len() + key_len;
    let result_id_len = "inventor:design:feature-result#".len() + key_len;
    if let Err(error) = ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(feature_id_len + result_id_len),
        "retain Inventor feature result identities",
    ) {
        return Some(Err(error));
    }
    if let Err(error) = ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(collection.id_len()?),
        "retain Inventor feature result source id",
    ) {
        return Some(Err(error));
    }
    if let Err(error) = ctx.charge_collection_items(1, "project Inventor feature result topology") {
        return Some(Err(error));
    }
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature result topology") {
        return Some(Err(error));
    }
    let mut copied_storage = match ctx.reserve_scoped(0, "compose Inventor feature result key") {
        Ok(storage) => storage,
        Err(error) => return Some(Err(error)),
    };
    let feature_id = match copied_storage.with_storage(|| {
        Ok::<_, CodecError>(FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
            source.identity.key(ctx)?,
        ))
    }) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let result = FeatureResultTopology::new(
        match copied_storage.with_storage(|| {
            Ok::<_, CodecError>(FeatureResultTopologyId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
                source.identity.key(ctx)?,
            ))
        }) {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        },
        match feature_id.try_clone_for_decode(ctx, "retain Inventor feature result identities") {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        },
        members,
        Some(collection.id()),
    );
    Some(Ok((feature_id, result)))
}

fn closed_edge_items(token: &str, items: &PmDcReferenceList, index: &ProjectionIndex<'_>) -> bool {
    !items.references().is_empty()
        && items.references().iter().all(|reference| {
            resolve_property(token, reference.index(), index).is_some_and(|property| {
                matches!(
                    &property.kind,
                    PmDcFeaturePropertyKind::EdgeItem {
                        index_references,
                        ..
                    } if !index_references.values().is_empty()
                )
            })
        })
}

fn slot_property<'a>(
    source: &PmDcFeature,
    slot: usize,
    index: &'a ProjectionIndex<'a>,
) -> Option<&'a PmDcFeatureProperty> {
    resolve_property(
        source.identity.segment_token.as_str(),
        source.properties.references().get(slot)?.index(),
        index,
    )
}

fn resolve_property<'a>(
    token: &str,
    reference: u32,
    index: &'a ProjectionIndex<'a>,
) -> Option<&'a PmDcFeatureProperty> {
    index
        .properties
        .get(&(token, reference.checked_sub(1)?))
        .and_then(Option::as_ref)
        .copied()
}

fn references<'a>(
    source: &PmDcFeature,
    slot: usize,
    family: PmDcFeatureReferenceFamily,
    index: &'a ProjectionIndex<'a>,
) -> Option<&'a PmDcReferenceList> {
    match &slot_property(source, slot, index)?.kind {
        PmDcFeaturePropertyKind::References {
            family: actual,
            items,
        } if *actual == family => Some(items),
        _ => None,
    }
}

fn enum16(
    source: &PmDcFeature,
    slot: usize,
    family: PmDcFeatureEnumFamily,
    index: &ProjectionIndex<'_>,
) -> Option<u16> {
    let expected_type = match family {
        PmDcFeatureEnumFamily::PartOperation => 5,
        PmDcFeatureEnumFamily::Extent => 11,
        PmDcFeatureEnumFamily::Hole => 3,
        PmDcFeatureEnumFamily::Fillet | PmDcFeatureEnumFamily::Chamfer => 2,
        PmDcFeatureEnumFamily::Auxiliary => return None,
    };
    match slot_property(source, slot, index)?.kind {
        PmDcFeaturePropertyKind::Enumeration {
            family: actual,
            type_value,
            value,
        } if actual == family && type_value == expected_type => Some(value),
        _ => None,
    }
}

fn boolean(source: &PmDcFeature, slot: usize, index: &ProjectionIndex<'_>) -> Option<bool> {
    match slot_property(source, slot, index)?.kind {
        PmDcFeaturePropertyKind::Boolean { value, .. } => Some(value),
        _ => None,
    }
}

fn resolve_direction<'a>(
    source: &PmDcFeature,
    slot: usize,
    index: &'a ProjectionIndex<'a>,
) -> Option<&'a crate::sketch::PmDcDirection> {
    let reference = source.properties.references().get(slot)?;
    index
        .directions
        .get(&(
            source.identity.segment_token.as_str(),
            reference.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)
        .copied()
}

fn length_parameter(
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Option<Length> {
    length_reference(
        source.identity.segment_token.as_str(),
        source.properties.references().get(slot)?.index(),
        index,
    )
}

fn length_reference(token: &str, reference: u32, index: &ProjectionIndex<'_>) -> Option<Length> {
    let parameter = index
        .parameters
        .get(&(token, reference.checked_sub(1)?))
        .and_then(Option::as_ref)?;
    match index.parameter_values.get(parameter.id().as_str())? {
        ParameterValue::Length(value) if value.get() >= 0.0 => Some(*value),
        _ => None,
    }
}

fn angle_parameter(
    source: &PmDcFeature,
    slot: usize,
    index: &ProjectionIndex<'_>,
) -> Option<Angle> {
    let reference = source.properties.references().get(slot)?;
    let parameter = index
        .parameters
        .get(&(
            source.identity.segment_token.as_str(),
            reference.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)?;
    match index.parameter_values.get(parameter.id().as_str())? {
        ParameterValue::Angle(value) => Some(*value),
        _ => None,
    }
}

fn boolean_properties(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    slots: &[usize],
    index: &ProjectionIndex<'_>,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let mut properties = BTreeMap::new();
    for slot in slots {
        if let Some(value) = boolean(source, *slot, index) {
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(
                    "property_".len()
                        + usize::try_from(slot.max(&1).ilog10()).map_err(|_| {
                            CodecError::Malformed(
                                "Inventor numeric value exceeds target range".into(),
                            )
                        })?
                        + 1
                        + "_boolean".len(),
                ),
                "retain Inventor feature property name",
            )?;
            ctx.charge_retained(
                if value { 4 } else { 5 },
                "retain Inventor feature property value",
            )?;
            ctx.insert_btree_map(
                &mut properties,
                cadmpeg_core::nonblank_literal!("property_{slot}_boolean"),
                value.to_string(),
                "project Inventor feature boolean property",
            )?;
        }
    }
    Ok(properties)
}

pub(crate) type PmDcFeatureProperty = Located<PmDcFeaturePropertyPayload>;

impl RecordPayload for PmDcFeaturePropertyPayload {
    const KIND: &'static str = "feature-property";
}

pub(crate) type PmDcPatternFeature = Located<PmDcPatternFeaturePayload>;

impl RecordPayload for PmDcPatternFeaturePayload {
    const KIND: &'static str = "pattern-feature";
}

pub(crate) type PmDcFeatureLabel = Located<PmDcFeatureLabelPayload>;

impl RecordPayload for PmDcFeatureLabelPayload {
    const KIND: &'static str = "feature-label";
}

pub(crate) type PmDcEntityStyleLink = Located<PmDcEntityStyleLinkPayload>;

impl RecordPayload for PmDcEntityStyleLinkPayload {
    const KIND: &'static str = "entity-style-link";
}

pub(crate) type PmDcFeature = Located<PmDcFeaturePayload>;

impl RecordPayload for PmDcFeaturePayload {
    const KIND: &'static str = "feature";
}

pub(crate) type PmDcKernelEntityLink = Located<PmDcKernelEntityLinkPayload>;

impl RecordPayload for PmDcKernelEntityLinkPayload {
    const KIND: &'static str = "kernel-entity-link";
}

pub(crate) type PmDcWorkPlane = Located<PmDcWorkPlanePayload>;

impl RecordPayload for PmDcWorkPlanePayload {
    const KIND: &'static str = "work-plane";
}

pub(crate) type PmDcFeatureContainer = Located<PmDcFeatureContainerPayload>;

impl RecordPayload for PmDcFeatureContainerPayload {
    const KIND: &'static str = "feature-container";
}

pub(crate) type PmDcFeatureTerminator = Located<PmDcFeatureTerminatorPayload>;

impl RecordPayload for PmDcFeatureTerminatorPayload {
    const KIND: &'static str = "feature-terminator";
}

#[cfg(test)]
mod tests;
