// SPDX-License-Identifier: Apache-2.0
//! Operand selection keys of assembly constraints (specification section 9.2).
//!
//! A constraint operand's geometry record is the target of one selection-key
//! record. The key names the part-side face, the face whose axis gives a line,
//! a part work feature by its `PmDC` source index, or an assembly-local geometry
//! record.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::pmdc::PmDcReference;
use crate::records::RseRecordFrame;

pub(super) const FACE_KEY_TYPE: [u8; 16] = crate::pmdc::inventor_id(0x9087_4d56);
pub(super) const FACE_AXIS_KEY_TYPE: [u8; 16] = [
    0xf3, 0x2c, 0xee, 0x1d, 0xd1, 0x11, 0xef, 0xb2, 0x60, 0x00, 0xc8, 0xb1, 0xf1, 0xbc, 0x87, 0xb0,
];
pub(super) const WORK_FEATURE_KEY_TYPE: [u8; 16] = [
    0x61, 0xb8, 0x48, 0x0c, 0xd2, 0x11, 0x05, 0x41, 0x60, 0x00, 0x93, 0xb7, 0xb0, 0x35, 0xc3, 0xb0,
];
pub(super) const NAME_MAP_TYPE: [u8; 16] = [
    0x42, 0x20, 0xe9, 0xcc, 0xd1, 0x11, 0xc5, 0x71, 0x00, 0x08, 0xa7, 0xba, 0x32, 0xa3, 0xdc, 0x09,
];
pub(super) const IMPORTED_FACE_NAME_TYPE: [u8; 16] = [
    0xfd, 0xd5, 0xbd, 0x47, 0xd3, 0x4a, 0xa6, 0xb6, 0xed, 0x0a, 0x11, 0xa3, 0x13, 0x8f, 0xd1, 0xaf,
];
const KEY_FORM: u16 = 0x0500;
const PART_KEY_PREFIX: &str = "PmDCSegment::";
const ASSEMBLY_KEY_PREFIX: &str = "AmDcSegment::";
const EMPTY_LIST_MARKER: u16 = 8;
const LIST_MARKER: u16 = 2;
const LIST_TAG: u16 = 0x3000;
const WORK_FEATURE_TAG: u8 = 5;
const NAME_FORM: u16 = 5;
const NAME_SPAN: u32 = 180;
const NAME_CLASS_FLAGS: u32 = 0x1000_0000;
const IMPORTED_NAME_CLASS: u32 = 15;
const NAME_GAP_LEN: usize = 20;
/// Name maps and naming records are small; these caps bound hostile counts.
const MAX_MAP_ENTRIES: usize = 1 << 16;
const MAX_PATH_LEN: usize = 64;
const MAX_NAME_GROUPS: usize = 64;
const MAX_GROUP_WIDTH: u32 = 64;
const MAX_KEY_UNITS: usize = 1024;

/// What a selection key names, by the key record's type and form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectionHint {
    /// A face of the referenced part; a plane operand is the face's plane.
    PartFace,
    /// A face of the referenced part whose axis is the line operand.
    PartFaceAxis,
    /// A part work feature selected by its `PmDC` source index.
    PartWorkFeature,
    /// A work feature of a subassembly selected by its `AmDc` source index.
    SubassemblyWorkFeature,
    /// A geometry record of the assembly itself.
    AssemblyGeometry,
}

/// Design segment family that a key string names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum KeySegmentKind {
    /// `PmDCSegment::`, the design segment of a part document.
    Part,
    /// `AmDcSegment::`, the design segment of an assembly document.
    Assembly,
}

/// The decoded `<kind>Segment::<segment>:<revision>:<number>` key string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PartKey {
    pub(crate) key: String,
    pub(crate) segment_kind: KeySegmentKind,
    /// Token of the referenced document's design segment.
    pub(crate) part_segment: String,
    pub(crate) revision_key: String,
    pub(crate) key_number: String,
    /// Decimal text naming the owning occurrence path element.
    pub(crate) occurrence_key: String,
}

/// Selection key of one operand geometry record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum OperandSelection {
    Face {
        key_record: u32,
        part: PartKey,
        values: [u32; 2],
        name_map_reference: PmDcReference,
        name_key: u32,
        /// The naming record that the name map gives for `name_key`.
        name_record: Option<u32>,
        name_type: Option<String>,
        /// Integer groups of an imported-face name; each group omits its width.
        imported_name: Option<Vec<Vec<i32>>>,
    },
    WorkFeature {
        key_record: u32,
        part: PartKey,
        source_index: u32,
    },
    AssemblyGeometry {
        key_record: u32,
        target_reference: PmDcReference,
        target_source_index: u32,
    },
}

/// A parsed key record and the geometry record it names.
#[derive(Debug)]
pub(super) struct SelectionKey {
    pub(super) geometry_ordinal: Option<u32>,
    pub(super) hint: ProjectionHint,
    pub(super) selection: OperandSelection,
}

fn header(view: &mut View<'_>) -> Result<(), CodecError> {
    crate::reader::u32(view, "selection key header value")?;
    crate::reader::u16(view, "selection key header id")?;
    crate::reader::pmdc_reference(view, "selection key next reference")?;
    crate::reader::u32(view, "selection key flags")?;
    crate::reader::pmdc_reference(view, "selection key context reference")?;
    crate::reader::u32(view, "selection key source index")?;
    crate::reader::i32(view, "selection key state")?;
    Ok(())
}

fn counted_text(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    field: &'static str,
) -> Result<String, CodecError> {
    let count = crate::reader::u32(view, field)?;
    let count = usize::try_from(count)
        .ok()
        .filter(|count| *count <= MAX_KEY_UNITS)
        .ok_or_else(|| CodecError::malformed(format_args!("Inventor {field} length {count}")))?;
    crate::reader::utf16_text(
        ctx,
        view,
        count,
        field,
        "retain Inventor selection key text",
    )
}

fn expect_u32(view: &mut View<'_>, expected: u32, field: &'static str) -> Result<(), CodecError> {
    let value = crate::reader::u32(view, field)?;
    if value != expected {
        return Err(CodecError::malformed(format_args!(
            "Inventor {field} is {value}, expected {expected}"
        )));
    }
    Ok(())
}

fn expect_u16(view: &mut View<'_>, expected: u16, field: &'static str) -> Result<(), CodecError> {
    let value = crate::reader::u16(view, field)?;
    if value != expected {
        return Err(CodecError::malformed(format_args!(
            "Inventor {field} is {value}, expected {expected}"
        )));
    }
    Ok(())
}

fn expect_u8(view: &mut View<'_>, expected: u8, field: &'static str) -> Result<(), CodecError> {
    let value = crate::reader::u8(view, field)?;
    if value != expected {
        return Err(CodecError::malformed(format_args!(
            "Inventor {field} is {value}, expected {expected}"
        )));
    }
    Ok(())
}

fn expect_end(view: &View<'_>, record: &'static str) -> Result<(), CodecError> {
    if view.position() != view.end() {
        return Err(CodecError::malformed(format_args!(
            "Inventor {record} record has trailing bytes"
        )));
    }
    Ok(())
}

/// Splits `PmDCSegment::<segment>:<revision>:<number>`.
fn part_key(
    ctx: &DecodeContext<'_>,
    key: String,
    occurrence_key: String,
) -> Result<PartKey, CodecError> {
    let (segment_kind, rest) = if let Some(rest) = key.strip_prefix(PART_KEY_PREFIX) {
        (KeySegmentKind::Part, rest)
    } else if let Some(rest) = key.strip_prefix(ASSEMBLY_KEY_PREFIX) {
        (KeySegmentKind::Assembly, rest)
    } else {
        return Err(CodecError::Malformed(
            "Inventor selection key does not name a design segment".into(),
        ));
    };
    let mut parts = rest.split(':');
    let (Some(segment), Some(revision), Some(number), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(CodecError::Malformed(
            "Inventor selection key does not have three components".into(),
        ));
    };
    if segment.is_empty()
        || revision.is_empty()
        || number.is_empty()
        || !number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(CodecError::Malformed(
            "Inventor selection key component is empty or not decimal".into(),
        ));
    }
    let part_segment = ctx.copy_retained_text(segment, "retain Inventor selection key segment")?;
    let revision_key =
        ctx.copy_retained_text(revision, "retain Inventor selection key revision")?;
    let key_number = ctx.copy_retained_text(number, "retain Inventor selection key number")?;
    Ok(PartKey {
        key,
        segment_kind,
        part_segment,
        revision_key,
        key_number,
        occurrence_key,
    })
}

/// Parses a face or face-axis key record.
pub(super) fn parse_face_key(
    ctx: &DecodeContext<'_>,
    record: &RseRecordFrame<'_>,
    axis: bool,
    major: Option<u8>,
) -> Result<SelectionKey, CodecError> {
    let separated = super::version_word(major, "selection key")?;
    let mut view = record.payload;
    header(&mut view)?;
    expect_u16(&mut view, KEY_FORM, "selection key form")?;
    expect_u32(&mut view, 0, "selection key reserved word")?;
    let key = counted_text(ctx, &mut view, "selection key string")?;
    let occurrence_key = counted_text(ctx, &mut view, "selection key occurrence")?;
    let part = part_key(ctx, key, occurrence_key)?;
    expect_u16(&mut view, 1, "selection key value marker")?;
    let first = crate::reader::u32(&mut view, "selection key first value")?;
    if separated {
        expect_u32(&mut view, 0, "selection key value separator")?;
    }
    let second = crate::reader::u32(&mut view, "selection key second value")?;
    for _ in 0..2 {
        expect_u16(&mut view, EMPTY_LIST_MARKER, "selection key list marker")?;
        expect_u16(&mut view, LIST_TAG, "selection key list tag")?;
        expect_u32(&mut view, 0, "selection key list count")?;
    }
    expect_u8(&mut view, 0, "selection key state")?;
    if !axis {
        expect_u8(&mut view, 1, "selection key face marker")?;
    }
    let geometry = crate::reader::pmdc_reference(&mut view, "selection key geometry")?;
    if !axis {
        expect_u32(&mut view, 0, "selection key face trailer")?;
    }
    let name_map_reference = crate::reader::pmdc_reference(&mut view, "selection key name map")?;
    let name_key = crate::reader::u32(&mut view, "selection key name")?;
    expect_end(&view, "selection key")?;
    Ok(SelectionKey {
        geometry_ordinal: geometry.record_ordinal(),
        hint: if axis {
            ProjectionHint::PartFaceAxis
        } else {
            ProjectionHint::PartFace
        },
        selection: OperandSelection::Face {
            key_record: record.ordinal,
            part,
            values: [first, second],
            name_map_reference,
            name_key,
            name_record: None,
            name_type: None,
            imported_name: None,
        },
    })
}

/// Parses a work-feature key record in its part or assembly form.
pub(super) fn parse_work_feature_key(
    ctx: &DecodeContext<'_>,
    record: &RseRecordFrame<'_>,
    major: Option<u8>,
) -> Result<SelectionKey, CodecError> {
    let trailer = super::version_word(major, "work-feature key")?;
    let mut view = record.payload;
    header(&mut view)?;
    let geometry = crate::reader::pmdc_reference(&mut view, "work-feature key geometry")?;
    let local = crate::reader::u8(&mut view, "work-feature key form")?;
    expect_u8(&mut view, WORK_FEATURE_TAG, "work-feature key tag")?;
    let (hint, selection) = match local {
        1 => {
            let target_reference =
                crate::reader::pmdc_reference(&mut view, "work-feature key target")?;
            let target_source_index =
                crate::reader::u32(&mut view, "work-feature key target source index")?;
            if trailer {
                expect_u32(&mut view, 0, "work-feature key trailer")?;
            }
            (
                ProjectionHint::AssemblyGeometry,
                OperandSelection::AssemblyGeometry {
                    key_record: record.ordinal,
                    target_reference,
                    target_source_index,
                },
            )
        }
        0 => {
            expect_u32(&mut view, 0, "work-feature key reserved word")?;
            let key = counted_text(ctx, &mut view, "work-feature key string")?;
            let occurrence_key = counted_text(ctx, &mut view, "work-feature key occurrence")?;
            let part = part_key(ctx, key, occurrence_key)?;
            expect_u16(&mut view, 1, "work-feature key value marker")?;
            let source_index = crate::reader::u32(&mut view, "work-feature key source index")?;
            if trailer {
                expect_u32(&mut view, 0, "work-feature key trailer")?;
            }
            (
                match part.segment_kind {
                    KeySegmentKind::Part => ProjectionHint::PartWorkFeature,
                    KeySegmentKind::Assembly => ProjectionHint::SubassemblyWorkFeature,
                },
                OperandSelection::WorkFeature {
                    key_record: record.ordinal,
                    part,
                    source_index,
                },
            )
        }
        other => {
            return Err(CodecError::malformed(format_args!(
                "Inventor work-feature key form is {other}"
            )))
        }
    };
    expect_end(&view, "work-feature key")?;
    Ok(SelectionKey {
        geometry_ordinal: geometry.record_ordinal(),
        hint,
        selection,
    })
}

/// Finds `name_key` in a name-map record; the map lists ascending keys.
pub(super) fn name_map_lookup(
    ctx: &DecodeContext<'_>,
    payload: View<'_>,
    name_key: u32,
) -> Result<Option<PmDcReference>, CodecError> {
    let mut view = payload;
    crate::reader::u32(&mut view, "name map header value")?;
    crate::reader::u16(&mut view, "name map header id")?;
    expect_u32(&mut view, 0, "name map reserved word")?;
    let maximum = crate::reader::u32(&mut view, "name map largest key")?;
    let count = crate::reader::u32(&mut view, "name map count")?;
    let count = usize::try_from(count)
        .ok()
        .filter(|count| *count <= MAX_MAP_ENTRIES)
        .ok_or_else(|| CodecError::malformed(format_args!("Inventor name map count {count}")))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(count),
        "scan Inventor constraint name map",
    )?;
    let mut previous = None;
    let mut found = None;
    for _ in 0..count {
        let key = crate::reader::u32(&mut view, "name map key")?;
        let reference = crate::reader::pmdc_reference(&mut view, "name map reference")?;
        if previous.is_some_and(|previous| key <= previous) {
            return Err(CodecError::Malformed(
                "Inventor name map keys are not ascending".into(),
            ));
        }
        previous = Some(key);
        if key == name_key {
            found = Some(reference);
        }
    }
    if previous.is_some_and(|last| last != maximum) {
        return Err(CodecError::Malformed(
            "Inventor name map largest key does not match its last key".into(),
        ));
    }
    expect_end(&view, "name map")?;
    Ok(found)
}

/// Parses an imported-face naming record into its integer groups.
pub(super) fn imported_face_name(
    ctx: &DecodeContext<'_>,
    payload: View<'_>,
) -> Result<Vec<Vec<i32>>, CodecError> {
    let mut view = payload;
    crate::reader::u32(&mut view, "face name header value")?;
    crate::reader::u16(&mut view, "face name header id")?;
    expect_u16(&mut view, NAME_FORM, "face name form")?;
    expect_u16(&mut view, 0, "face name reserved word")?;
    expect_u16(&mut view, LIST_MARKER, "face name path marker")?;
    expect_u16(&mut view, LIST_TAG, "face name path tag")?;
    let count = crate::reader::u32(&mut view, "face name path count")?;
    let length = usize::try_from(count)
        .ok()
        .filter(|length| *length <= MAX_PATH_LEN)
        .ok_or_else(|| CodecError::malformed(format_args!("Inventor face name path {count}")))?;
    if length != 0 {
        expect_u32(&mut view, count, "face name path repeated count")?;
        expect_u32(&mut view, 0, "face name path reserved word")?;
        for _ in 0..length {
            crate::reader::u32(&mut view, "face name path occurrence")?;
        }
    }
    let gap = crate::reader::take(&mut view, NAME_GAP_LEN, "face name gap")?;
    if gap.iter().any(|byte| *byte != 0) {
        return Err(CodecError::Malformed(
            "Inventor face name gap is not zero".into(),
        ));
    }
    expect_u32(&mut view, NAME_SPAN, "face name span")?;
    expect_u32(&mut view, 0, "face name span separator")?;
    expect_u16(&mut view, LIST_MARKER, "face name class marker")?;
    expect_u16(&mut view, LIST_TAG, "face name class tag")?;
    expect_u32(&mut view, 1, "face name class count")?;
    expect_u32(&mut view, 1, "face name class repeated count")?;
    expect_u32(&mut view, NAME_CLASS_FLAGS, "face name class flags")?;
    expect_u32(&mut view, IMPORTED_NAME_CLASS, "face name class")?;
    let null = crate::reader::i32(&mut view, "face name null")?;
    if null != -1 {
        return Err(CodecError::malformed(format_args!(
            "Inventor face name null is {null}"
        )));
    }
    expect_u32(&mut view, 0, "face name reserved word")?;
    let groups = crate::reader::u32(&mut view, "face name group count")?;
    let groups = usize::try_from(groups)
        .ok()
        .filter(|groups| *groups <= MAX_NAME_GROUPS)
        .ok_or_else(|| CodecError::malformed(format_args!("Inventor face name groups {groups}")))?;
    let mut result = ctx.vector_storage(groups, "admit Inventor face name groups")?;
    for _ in 0..groups {
        let width = crate::reader::u32(&mut view, "face name group width")?;
        if !(2..=MAX_GROUP_WIDTH).contains(&width) {
            return Err(CodecError::malformed(format_args!(
                "Inventor face name group width {width}"
            )));
        }
        let values = usize::try_from(width - 1).map_err(|_| {
            CodecError::Malformed("Inventor face name group width overflows".into())
        })?;
        let mut group = ctx.vector_storage(values, "admit Inventor face name group")?;
        for _ in 0..values {
            group.push(crate::reader::i32(&mut view, "face name group value")?);
        }
        expect_u32(&mut view, 0, "face name group separator")?;
        result.push(group);
    }
    expect_u32(&mut view, 0, "face name terminator")?;
    expect_end(&view, "face name")?;
    Ok(result)
}

/// Retained bytes a copy of `selection` holds.
pub(super) fn retained_len(selection: &OperandSelection) -> u64 {
    let part_len = |part: &PartKey| {
        [
            &part.key,
            &part.part_segment,
            &part.revision_key,
            &part.key_number,
            &part.occurrence_key,
        ]
        .iter()
        .map(|text| cadmpeg_core::decode::u64_from_index(text.len()))
        .sum::<u64>()
    };
    match selection {
        OperandSelection::Face {
            part,
            name_type,
            imported_name,
            ..
        } => {
            part_len(part)
                + name_type
                    .as_ref()
                    .map_or(0, |text| cadmpeg_core::decode::u64_from_index(text.len()))
                + imported_name.as_ref().map_or(0, |groups| {
                    groups
                        .iter()
                        .map(|group| cadmpeg_core::decode::u64_from_index(group.len() * 4))
                        .sum()
                })
        }
        OperandSelection::WorkFeature { part, .. } => part_len(part),
        OperandSelection::AssemblyGeometry { .. } => 0,
    }
}
