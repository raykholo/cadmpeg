// SPDX-License-Identifier: Apache-2.0
//! Typed `AmDc` assembly-constraint records (specification section 9.1).

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::pmdc::PmDcReference;
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::records::RseRecordFrame;
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

mod geometry;
mod joint;
mod selection;

pub(crate) use geometry::OperandGeometryRecord;
pub(crate) use joint::project_joints;

use selection::SelectionKey;
pub(crate) use selection::{OperandSelection, ProjectionHint};

pub(super) const CONSTRAINT_TYPE: [u8; 16] = [
    0x47, 0xf0, 0x86, 0x4e, 0xd1, 0x11, 0xc3, 0x6c, 0x00, 0x08, 0x5c, 0xbb, 0x0f, 0xed, 0xdc, 0x09,
];
const OPERAND_TYPE: [u8; 16] = [
    0x52, 0xce, 0x1f, 0xb9, 0x51, 0x4f, 0x50, 0x91, 0xde, 0x31, 0xed, 0xb4, 0x20, 0xf0, 0x4d, 0x7f,
];
const PARAMETER_ROLE_TYPE: [u8; 16] = [
    0xe1, 0xc6, 0x8e, 0x25, 0x3d, 0x4f, 0x53, 0x30, 0xcc, 0x06, 0x27, 0xaf, 0x50, 0x9e, 0xe4, 0x44,
];
const OCCURRENCE_INSTANCE_TYPE: [u8; 16] = crate::pmdc::inventor_id(0x9087_4d61);
const PLANE_TYPE: [u8; 16] = [
    0x42, 0xdf, 0x52, 0xce, 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const POINT_TYPE: [u8; 16] = [
    0x3e, 0xdf, 0x52, 0xce, 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const DIRECTION_TYPE: [u8; 16] = [
    0x40, 0xdf, 0x52, 0xce, 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
const LINE_TYPE: [u8; 16] = [
    0x89, 0x6c, 0xf0, 0x8e, 0xd1, 0x11, 0x3c, 0x04, 0x60, 0x00, 0x7c, 0xb8, 0x01, 0xf3, 0x1b, 0xb0,
];
const LIST_MARKER: u16 = 2;
const LIST_TAG: u16 = 0x3000;
const TERMINAL: u8 = 3;
const OPERAND_PAYLOAD_LEN: usize = 18;
const PARAMETER_ROLE_PAYLOAD_LEN: usize = 16;
/// A constraint stores two or three operands and at most three parameters.
const MAX_LIST_LEN: usize = 16;

/// Constraint family selected by the record's `kind` byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConstraintKind {
    Mate,
    Flush,
    Angle,
    Tangent,
    Insert,
    RotationMotion,
    WithoutBrowserEntry,
    Symmetry,
}

impl ConstraintKind {
    const fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Mate,
            2 => Self::Flush,
            3 => Self::Angle,
            4 => Self::Tangent,
            5 => Self::Insert,
            6 => Self::RotationMotion,
            11 => Self::WithoutBrowserEntry,
            20 => Self::Symmetry,
            _ => return None,
        })
    }
}

/// Geometry class given by the type of an operand's geometry record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperandGeometry {
    Plane,
    Line,
    Point,
    Direction,
}

impl OperandGeometry {
    fn from_type(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            PLANE_TYPE => Some(Self::Plane),
            LINE_TYPE => Some(Self::Line),
            POINT_TYPE => Some(Self::Point),
            DIRECTION_TYPE => Some(Self::Direction),
            _ => None,
        }
    }
}

/// One operand of a constraint, resolved through its operand record when it has one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ConstraintOperandRecord {
    pub(crate) reference: PmDcReference,
    /// Present when the reference selects an operand record.
    pub(crate) occurrence_reference: Option<PmDcReference>,
    pub(crate) geometry_reference: Option<PmDcReference>,
    pub(crate) geometry_type: Option<String>,
    pub(crate) geometry: Option<OperandGeometry>,
    /// The stored plane, line, point, or direction (section 9.3).
    pub(crate) stored_geometry: Option<OperandGeometryRecord>,
    /// `occurrence_id` of the occurrence that owns the operand geometry.
    pub(crate) occurrence_id: Option<u32>,
    /// What the operand's selection key names (section 9.2).
    pub(crate) projection_hint: Option<ProjectionHint>,
    pub(crate) selection: Option<OperandSelection>,
}

/// One parameter of a constraint, resolved through its parameter-role record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConstraintParameterRecord {
    pub(crate) reference: PmDcReference,
    pub(crate) role: Option<u16>,
    pub(crate) parameter_reference: Option<PmDcReference>,
}

/// Native `AmDc` assembly constraint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AssemblyConstraintRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) next_reference: PmDcReference,
    pub(crate) flags: u32,
    pub(crate) context_reference: PmDcReference,
    pub(crate) source_index: u32,
    pub(crate) state: i32,
    pub(crate) owner_reference: PmDcReference,
    pub(crate) kind_code: u8,
    pub(crate) kind: ConstraintKind,
    pub(crate) operands: Vec<ConstraintOperandRecord>,
    pub(crate) parameters: Vec<ConstraintParameterRecord>,
    pub(crate) solution: u8,
    pub(crate) name_index: u32,
    pub(crate) limit_state: u32,
}

/// Constraint fields read from one payload, before reference resolution.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ConstraintPayload {
    pub(super) header_value: u32,
    pub(super) header_id: u16,
    pub(super) next_reference: PmDcReference,
    pub(super) flags: u32,
    pub(super) context_reference: PmDcReference,
    pub(super) source_index: u32,
    pub(super) state: i32,
    pub(super) owner_reference: PmDcReference,
    pub(super) kind_code: u8,
    pub(super) kind: ConstraintKind,
    pub(super) operands: Vec<PmDcReference>,
    pub(super) parameters: Vec<PmDcReference>,
    pub(super) solution: u8,
    pub(super) name_index: u32,
    pub(super) limit_state: u32,
}

/// Decodes every constraint in every framed `AmDc` segment.
pub(crate) fn inventory(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
    occurrences: &[super::AssemblyOccurrence],
) -> Result<(Vec<AssemblyConstraintRecord>, Vec<RecordIssue>), CodecError> {
    let mut constraints = Vec::new();
    let mut issues = Vec::new();
    for segment in &document.segments {
        if segment.kind != SegmentKind::AmDc {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        let token = segment.pair.token.as_str();
        let version = segment.registry.and_then(|registry| {
            crate::pmdc::content_header_extension_len(registry.version_major)
                .map(|extension| (registry.version_major, extension))
        });
        let mut keys = Vec::new();
        for record in &table.records {
            let result = match record.type_id {
                selection::FACE_KEY_TYPE => selection::parse_face_key(ctx, record, false),
                selection::FACE_AXIS_KEY_TYPE => selection::parse_face_key(ctx, record, true),
                selection::WORK_FEATURE_KEY_TYPE => selection::parse_work_feature_key(ctx, record),
                _ => continue,
            };
            ctx.charge_work(1, "scan Inventor constraint selection key")?;
            let result = result.and_then(|mut key| {
                name_face_key(ctx, &mut key, &table.records)?;
                Ok(key)
            });
            match result {
                Ok(key) => {
                    ctx.charge_collection_items(1, "admit Inventor constraint selection key")?;
                    keys.push(key);
                }
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => push_issue(ctx, &mut issues, segment, record.ordinal, error)?,
            }
        }
        for record in &table.records {
            if record.type_id != CONSTRAINT_TYPE {
                continue;
            }
            ctx.charge_work(1, "scan Inventor assembly constraint record")?;
            let result = parse(ctx, record.payload).and_then(|payload| {
                resolve(
                    ctx,
                    SegmentRecords {
                        token,
                        records: &table.records,
                        occurrences,
                        keys: &keys,
                        version,
                    },
                    record.ordinal,
                    &payload,
                )
            });
            match result {
                Ok(constraint) => {
                    ctx.charge_collection_items(1, "admit Inventor assembly constraint")?;
                    constraints.push(constraint);
                }
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => push_issue(ctx, &mut issues, segment, record.ordinal, error)?,
            }
        }
    }
    Ok((constraints, issues))
}

fn push_issue(
    ctx: &DecodeContext<'_>,
    issues: &mut Vec<RecordIssue>,
    segment: &crate::rse::SegmentDescriptor<'_>,
    record_ordinal: u32,
    error: CodecError,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "admit Inventor assembly issue")?;
    ctx.charge_entities(1, "admit Inventor assembly issue")?;
    ctx.charge_formatted_retained(
        format_args!("{error}"),
        "retain Inventor assembly issue detail",
    )?;
    issues.push(RecordIssue {
        family: RecordIssueFamily::Assembly,
        segment_token: segment.pair.token.key().clone(),
        record_ordinal,
        detail: crate::issue_detail(error)?,
    });
    Ok(())
}

/// Follows a face key through its name map to the naming record.
fn name_face_key(
    ctx: &DecodeContext<'_>,
    key: &mut SelectionKey,
    records: &[RseRecordFrame<'_>],
) -> Result<(), CodecError> {
    let OperandSelection::Face {
        name_map_reference,
        name_key,
        name_record,
        name_type,
        imported_name,
        ..
    } = &mut key.selection
    else {
        return Ok(());
    };
    let Some(map) =
        target(records, *name_map_reference).filter(|map| map.type_id == selection::NAME_MAP_TYPE)
    else {
        return Ok(());
    };
    let Some(name) = selection::name_map_lookup(ctx, map.payload, *name_key)?
        .and_then(|reference| target(records, reference))
    else {
        return Ok(());
    };
    *name_record = Some(name.ordinal);
    *name_type = Some(ctx.format_retained(
        format_args!("{}", crate::pmdc::type_id_string(name.type_id)),
        "retain Inventor constraint name type",
    )?);
    if name.type_id == selection::IMPORTED_FACE_NAME_TYPE {
        *imported_name = Some(selection::imported_face_name(ctx, name.payload)?);
    }
    Ok(())
}

pub(super) fn parse(
    ctx: &DecodeContext<'_>,
    payload: View<'_>,
) -> Result<ConstraintPayload, CodecError> {
    let mut view = payload;
    let header_value = crate::reader::u32(&mut view, "constraint header value")?;
    let header_id = crate::reader::u16(&mut view, "constraint header id")?;
    let next_reference = crate::reader::pmdc_reference(&mut view, "constraint next reference")?;
    let flags = crate::reader::u32(&mut view, "constraint flags")?;
    let context_reference =
        crate::reader::pmdc_reference(&mut view, "constraint context reference")?;
    let source_index = crate::reader::u32(&mut view, "constraint source index")?;
    let state = crate::reader::i32(&mut view, "constraint state")?;
    let owner_reference = crate::reader::pmdc_reference(&mut view, "constraint owner reference")?;
    let kind_code = crate::reader::u8(&mut view, "constraint kind")?;
    let kind = ConstraintKind::from_byte(kind_code).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "Inventor assembly constraint kind {kind_code} is not defined"
        ))
    })?;
    let operands = reference_list(ctx, &mut view, "constraint operand list")?;
    let parameters = reference_list(ctx, &mut view, "constraint parameter list")?;
    let solution = crate::reader::u8(&mut view, "constraint solution")?;
    let name_index = crate::reader::u32(&mut view, "constraint name index")?;
    let limit_state = crate::reader::u32(&mut view, "constraint limit state")?;
    let terminal = crate::reader::u8(&mut view, "constraint terminal")?;
    if terminal != TERMINAL {
        return Err(CodecError::malformed(format_args!(
            "Inventor assembly constraint terminal is {terminal}, expected {TERMINAL}"
        )));
    }
    if view.position() != view.end() {
        return Err(CodecError::Malformed(
            "Inventor assembly constraint record has trailing bytes".into(),
        ));
    }
    Ok(ConstraintPayload {
        header_value,
        header_id,
        next_reference,
        flags,
        context_reference,
        source_index,
        state,
        owner_reference,
        kind_code,
        kind,
        operands,
        parameters,
        solution,
        name_index,
        limit_state,
    })
}

fn reference_list(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    field: &'static str,
) -> Result<Vec<PmDcReference>, CodecError> {
    let marker = [
        crate::reader::u16(view, field)?,
        crate::reader::u16(view, field)?,
    ];
    if marker != [LIST_MARKER, LIST_TAG] {
        return Err(CodecError::malformed(format_args!(
            "Inventor {field} marker is {marker:?}"
        )));
    }
    let count = crate::reader::u32(view, field)?;
    let length = usize::try_from(count)
        .ok()
        .filter(|length| *length <= MAX_LIST_LEN)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("Inventor {field} count {count} exceeds limit"))
        })?;
    if length == 0 {
        return Ok(Vec::new());
    }
    let repeated = crate::reader::u32(view, field)?;
    let zero = crate::reader::u32(view, field)?;
    if repeated != count || zero != 0 {
        return Err(CodecError::malformed(format_args!(
            "Inventor {field} metadata is [{repeated}, {zero}] for count {count}"
        )));
    }
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(length),
        "admit Inventor assembly constraint references",
    )?;
    let mut references =
        ctx.vector_storage(length, "admit Inventor assembly constraint references")?;
    for _ in 0..length {
        references.push(crate::reader::pmdc_reference(view, field)?);
    }
    Ok(references)
}

fn target<'r, 'a>(
    records: &'r [RseRecordFrame<'a>],
    reference: PmDcReference,
) -> Option<&'r RseRecordFrame<'a>> {
    let ordinal = reference.record_ordinal()?;
    let record = records.get(usize::try_from(ordinal).ok()?)?;
    (record.ordinal == ordinal).then_some(record)
}

/// The records of one `AmDc` segment that constraint resolution reads.
#[derive(Clone, Copy)]
struct SegmentRecords<'s, 'a> {
    token: &'s str,
    records: &'s [RseRecordFrame<'a>],
    occurrences: &'s [super::AssemblyOccurrence],
    keys: &'s [SelectionKey],
    /// Segment major version and its content-header extension length.
    version: Option<(u8, usize)>,
}

fn resolve(
    ctx: &DecodeContext<'_>,
    segment: SegmentRecords<'_, '_>,
    record_ordinal: u32,
    payload: &ConstraintPayload,
) -> Result<AssemblyConstraintRecord, CodecError> {
    let SegmentRecords { token, records, .. } = segment;
    let mut operands = ctx.vector_storage(
        payload.operands.len(),
        "admit Inventor assembly constraint operands",
    )?;
    for reference in &payload.operands {
        operands.push(resolve_operand(ctx, segment, *reference)?);
    }
    let mut parameters = ctx.vector_storage(
        payload.parameters.len(),
        "admit Inventor assembly constraint parameters",
    )?;
    for reference in &payload.parameters {
        parameters.push(resolve_parameter(*reference, records)?);
    }
    ctx.charge_entities(1, "admit Inventor native assembly constraint")?;
    let id = ctx.format_retained(
        format_args!("inventor:assembly:constraint#{token}-{record_ordinal}"),
        "retain Inventor assembly constraint id",
    )?;
    let segment_token =
        ctx.copy_retained_text(token, "retain Inventor assembly constraint token")?;
    Ok(AssemblyConstraintRecord {
        id,
        segment_token,
        record_ordinal,
        header_value: payload.header_value,
        header_id: payload.header_id,
        next_reference: payload.next_reference,
        flags: payload.flags,
        context_reference: payload.context_reference,
        source_index: payload.source_index,
        state: payload.state,
        owner_reference: payload.owner_reference,
        kind_code: payload.kind_code,
        kind: payload.kind,
        operands,
        parameters,
        solution: payload.solution,
        name_index: payload.name_index,
        limit_state: payload.limit_state,
    })
}

fn resolve_operand(
    ctx: &DecodeContext<'_>,
    segment: SegmentRecords<'_, '_>,
    reference: PmDcReference,
) -> Result<ConstraintOperandRecord, CodecError> {
    let SegmentRecords {
        token,
        records,
        occurrences,
        keys,
        version,
    } = segment;
    let mut operand = ConstraintOperandRecord {
        reference,
        occurrence_reference: None,
        geometry_reference: None,
        geometry_type: None,
        geometry: None,
        stored_geometry: None,
        occurrence_id: None,
        projection_hint: None,
        selection: None,
    };
    let Some(record) = target(records, reference) else {
        return Ok(operand);
    };
    if record.type_id != OPERAND_TYPE {
        return Ok(operand);
    }
    let mut view = record.payload;
    if view.window().len() != OPERAND_PAYLOAD_LEN {
        return Err(CodecError::malformed(format_args!(
            "Inventor constraint operand record is {} bytes, expected {OPERAND_PAYLOAD_LEN}",
            view.window().len()
        )));
    }
    crate::reader::u32(&mut view, "constraint operand header value")?;
    crate::reader::u16(&mut view, "constraint operand header id")?;
    let occurrence_reference =
        crate::reader::pmdc_reference(&mut view, "constraint operand occurrence")?;
    let geometry_reference =
        crate::reader::pmdc_reference(&mut view, "constraint operand geometry")?;
    if crate::reader::u32(&mut view, "constraint operand trailer")? != 0 {
        return Err(CodecError::Malformed(
            "Inventor constraint operand trailer is not zero".into(),
        ));
    }
    if let Some(geometry) = target(records, geometry_reference) {
        operand.geometry_type = Some(ctx.format_retained(
            format_args!("{}", crate::pmdc::type_id_string(geometry.type_id)),
            "retain Inventor constraint geometry type",
        )?);
        operand.geometry = OperandGeometry::from_type(geometry.type_id);
        if let (Some(class), Some((major, extension))) = (operand.geometry, version) {
            operand.stored_geometry =
                Some(geometry::parse(geometry.payload, class, major, extension)?);
        }
    }
    let instance = target(records, occurrence_reference)
        .filter(|instance| instance.type_id == OCCURRENCE_INSTANCE_TYPE)
        .map(|instance| instance.ordinal);
    if let Some(instance) = instance {
        let mut owners = occurrences.iter().filter(|occurrence| {
            occurrence.segment_token == token
                && occurrence.related_references.iter().any(|related| {
                    PmDcReference::from_packed(*related).record_ordinal() == Some(instance)
                })
        });
        operand.occurrence_id = match (owners.next(), owners.next()) {
            (Some(owner), None) => Some(owner.occurrence_id),
            _ => None,
        };
    }
    if let Some(geometry) = geometry_reference.record_ordinal() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(keys.len()),
            "join Inventor constraint selection key",
        )?;
        let mut matches = keys
            .iter()
            .filter(|key| key.geometry_ordinal == Some(geometry));
        if let (Some(key), None) = (matches.next(), matches.next()) {
            ctx.charge_retained(
                selection::retained_len(&key.selection),
                "retain Inventor constraint operand selection",
            )?;
            operand.projection_hint = Some(key.hint);
            operand.selection = Some(key.selection.clone());
        }
    }
    operand.occurrence_reference =
        (occurrence_reference.index() != 0).then_some(occurrence_reference);
    operand.geometry_reference = Some(geometry_reference);
    Ok(operand)
}

fn resolve_parameter(
    reference: PmDcReference,
    records: &[RseRecordFrame<'_>],
) -> Result<ConstraintParameterRecord, CodecError> {
    let mut parameter = ConstraintParameterRecord {
        reference,
        role: None,
        parameter_reference: None,
    };
    let Some(record) = target(records, reference) else {
        return Ok(parameter);
    };
    if record.type_id != PARAMETER_ROLE_TYPE {
        return Ok(parameter);
    }
    let mut view = record.payload;
    if view.window().len() != PARAMETER_ROLE_PAYLOAD_LEN {
        return Err(CodecError::malformed(format_args!(
            "Inventor constraint parameter-role record is {} bytes, expected {PARAMETER_ROLE_PAYLOAD_LEN}",
            view.window().len()
        )));
    }
    crate::reader::u32(&mut view, "constraint parameter-role header value")?;
    crate::reader::u16(&mut view, "constraint parameter-role header id")?;
    parameter.role = Some(crate::reader::u16(&mut view, "constraint parameter role")?);
    parameter.parameter_reference = Some(crate::reader::pmdc_reference(
        &mut view,
        "constraint parameter reference",
    )?);
    if crate::reader::u32(&mut view, "constraint parameter-role trailer")? != 0 {
        return Err(CodecError::Malformed(
            "Inventor constraint parameter-role trailer is not zero".into(),
        ));
    }
    Ok(parameter)
}

#[cfg(test)]
mod tests;
