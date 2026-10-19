// SPDX-License-Identifier: Apache-2.0
//! Operand geometry records of assembly constraints (specification section 9.3).
//!
//! The record stores the selected plane, line, point, or direction in the
//! coordinates of the referenced document, in centimetres.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use super::OperandGeometry;
use crate::pmdc::PmDcReference;

const PLANE_FRAME_MARKER: u32 = 0x203;
const LIST_MARKER: u16 = 2;
const LIST_TAG: u16 = 0x3000;
/// First segment major version whose plane records store one more u32 value.
const PLANE_VALUE_VERSION: u8 = 22;

/// Stored shape of an operand geometry record, in referenced-document centimetres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub(crate) enum GeometryShape {
    /// `u_axis` x `v_axis` is the outward normal of a selected face.
    Plane {
        origin: [f64; 3],
        u_axis: [f64; 3],
        v_axis: [f64; 3],
    },
    Line {
        origin: [f64; 3],
        direction: [f64; 3],
    },
    Point {
        position: [f64; 3],
    },
    Direction {
        vector: [f64; 3],
    },
}

/// Typed operand geometry record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct OperandGeometryRecord {
    pub(crate) entity_flags: u32,
    /// The occurrence-context record of the referenced document; null for
    /// geometry of the assembly itself.
    pub(crate) document_reference: PmDcReference,
    pub(crate) frame_marker: bool,
    #[serde(flatten)]
    pub(crate) shape: GeometryShape,
}

fn f64_value(view: &mut View<'_>, field: &'static str) -> Result<f64, CodecError> {
    let value = f64::from_bits(crate::reader::u64(view, field)?);
    if !value.is_finite() {
        return Err(CodecError::malformed(format_args!(
            "Inventor {field} is not finite"
        )));
    }
    Ok(value)
}

fn triple(view: &mut View<'_>, field: &'static str) -> Result<[f64; 3], CodecError> {
    Ok([
        f64_value(view, field)?,
        f64_value(view, field)?,
        f64_value(view, field)?,
    ])
}

fn zero(view: &mut View<'_>, field: &'static str) -> Result<(), CodecError> {
    let value = crate::reader::u32(view, field)?;
    if value != 0 {
        return Err(CodecError::malformed(format_args!(
            "Inventor {field} is {value}, expected 0"
        )));
    }
    Ok(())
}

fn empty_list(view: &mut View<'_>, field: &'static str) -> Result<(), CodecError> {
    let marker = crate::reader::u16(view, field)?;
    let tag = crate::reader::u16(view, field)?;
    let count = crate::reader::u32(view, field)?;
    if marker != LIST_MARKER || tag != LIST_TAG || count != 0 {
        return Err(CodecError::malformed(format_args!(
            "Inventor {field} is not an empty reference list"
        )));
    }
    Ok(())
}

/// Parses an operand geometry record of `class` at a segment major version.
///
/// `extension_len` is the content-header extension length of that version.
pub(super) fn parse(
    payload: View<'_>,
    class: OperandGeometry,
    version: u8,
    extension_len: usize,
) -> Result<OperandGeometryRecord, CodecError> {
    let mut view = payload;
    crate::reader::u32(&mut view, "operand geometry header value")?;
    crate::reader::u16(&mut view, "operand geometry header id")?;
    crate::reader::pmdc_reference(&mut view, "operand geometry next reference")?;
    crate::reader::u32(&mut view, "operand geometry flags")?;
    crate::reader::pmdc_reference(&mut view, "operand geometry context reference")?;
    crate::reader::u32(&mut view, "operand geometry source index")?;
    for _ in 0..extension_len {
        crate::reader::i32(&mut view, "operand geometry header extension")?;
    }
    let entity_flags = crate::reader::u32(&mut view, "operand geometry entity flags")?;
    let document_reference =
        crate::reader::pmdc_reference(&mut view, "operand geometry document reference")?;
    zero(&mut view, "operand geometry reserved word")?;
    let mut frame_marker = false;
    let shape = match class {
        OperandGeometry::Plane => {
            if version >= PLANE_VALUE_VERSION {
                zero(&mut view, "operand plane value")?;
            }
            let mut probe = view;
            if crate::reader::u32(&mut probe, "operand plane frame marker")? == PLANE_FRAME_MARKER {
                frame_marker = true;
                view = probe;
            }
            zero(&mut view, "operand plane value")?;
            GeometryShape::Plane {
                origin: triple(&mut view, "operand plane origin")?,
                u_axis: triple(&mut view, "operand plane u axis")?,
                v_axis: triple(&mut view, "operand plane v axis")?,
            }
        }
        OperandGeometry::Line => {
            zero(&mut view, "operand line value")?;
            empty_list(&mut view, "operand line list")?;
            let origin = triple(&mut view, "operand line origin")?;
            let direction = triple(&mut view, "operand line direction")?;
            let state = crate::reader::u8(&mut view, "operand line state")?;
            if state != 0 {
                return Err(CodecError::malformed(format_args!(
                    "Inventor operand line state is {state}"
                )));
            }
            GeometryShape::Line { origin, direction }
        }
        OperandGeometry::Point => {
            zero(&mut view, "operand point value")?;
            let position = triple(&mut view, "operand point position")?;
            empty_list(&mut view, "operand point list")?;
            empty_list(&mut view, "operand point list")?;
            GeometryShape::Point { position }
        }
        OperandGeometry::Direction => {
            zero(&mut view, "operand direction value")?;
            GeometryShape::Direction {
                vector: triple(&mut view, "operand direction vector")?,
            }
        }
    };
    if view.position() != view.end() {
        return Err(CodecError::Malformed(
            "Inventor operand geometry record has trailing bytes".into(),
        ));
    }
    Ok(OperandGeometryRecord {
        entity_flags,
        document_reference,
        frame_marker,
        shape,
    })
}
