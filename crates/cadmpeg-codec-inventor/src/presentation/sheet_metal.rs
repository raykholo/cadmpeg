// SPDX-License-Identifier: Apache-2.0
//! Typed `PmApp` sheet-metal rule and unfold-rule style records.
//!
//! Both record types start with the `PmApp` style header that rendering
//! styles also use. A sheet-metal rule references its unfold rule and its
//! material style; an unfold rule stores its unfold method and factor
//! expressions.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::pmdc::{reference_list, Cursor, PmDcReference, PmDcReferenceList};
use crate::record_identity::{push_record, Located, RecordPayload};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

/// Type identifier of a `PmApp` sheet-metal rule.
pub(crate) const SHEET_METAL_RULE_TYPE: [u8; 16] = [
    0x6b, 0xc0, 0xd6, 0x10, 0x08, 0x46, 0x23, 0x69, 0xc7, 0x3c, 0x0f, 0xac, 0xa2, 0x68, 0x55, 0x0f,
];
/// Type identifier of a `PmApp` unfold rule.
pub(crate) const UNFOLD_RULE_TYPE: [u8; 16] = [
    0x3e, 0x0b, 0x6b, 0xda, 0xf5, 0x41, 0x7a, 0x64, 0x8d, 0xa9, 0x1f, 0xbd, 0x09, 0x3f, 0xe2, 0x13,
];
/// Type identifier of a `PmApp` material style.
pub(crate) const MATERIAL_STYLE_TYPE: [u8; 16] = [
    0x6e, 0xd8, 0x59, 0x67, 0xd2, 0x11, 0x38, 0x78, 0x60, 0x00, 0x94, 0xb7, 0x0b, 0x02, 0xec, 0xb0,
];
/// Type identifier of a `PmApp` style category.
pub(crate) const STYLE_CATEGORY_TYPE: [u8; 16] = [
    0x20, 0xd0, 0xa6, 0xfd, 0xd2, 0x11, 0x8f, 0x93, 0x60, 0x00, 0xa1, 0xb7, 0x0b, 0x02, 0xec, 0xb0,
];

/// Number of text fields of a sheet-metal rule body.
const RULE_TEXT_COUNT: usize = 9;
/// Number of u32 value fields of a sheet-metal rule body.
const RULE_VALUE_COUNT: usize = 6;

/// Whether a `PmApp` segment major version uses the style header grammar of
/// the sheet-metal style records.
pub(crate) const fn supports_version(version: u8) -> bool {
    matches!(version, 17..=23)
}

/// The style header shared by `PmApp` style records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmAppStyleHeader {
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) state: u8,
    pub(crate) flags: u16,
    pub(crate) values: [u16; 2],
    pub(crate) default_state: u32,
    pub(crate) value: u32,
    /// The style category record that lists this style.
    pub(crate) category: PmDcReference,
    pub(crate) name: String,
    pub(crate) long_name: String,
    pub(crate) style_state: u16,
    pub(crate) label: String,
}

/// One `PmApp` sheet-metal rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmAppSheetMetalRule {
    pub(crate) segment_version_major: u8,
    #[serde(flatten)]
    pub(crate) header: PmAppStyleHeader,
    /// Expression texts in source order.
    pub(crate) texts: [String; RULE_TEXT_COUNT],
    /// Integer fields in source order.
    pub(crate) integers: [u32; RULE_VALUE_COUNT],
    pub(crate) unfold_rule: PmDcReference,
    pub(crate) material_style: PmDcReference,
}

/// One `PmApp` unfold rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmAppUnfoldRule {
    pub(crate) segment_version_major: u8,
    #[serde(flatten)]
    pub(crate) header: PmAppStyleHeader,
    pub(crate) method: u32,
    /// The first unfold factor expression.
    pub(crate) factor: String,
    pub(crate) secondary_factor: String,
    pub(crate) members: PmDcReferenceList,
    pub(crate) value_count: u32,
    pub(crate) scalars: [cadmpeg_ir::scalar::FiniteReal; 3],
    pub(crate) first_state: u8,
    pub(crate) second_state: u8,
    pub(crate) mode: u32,
    pub(crate) references: PmDcReferenceList,
    pub(crate) tertiary_factor: String,
}

impl RecordPayload for PmAppSheetMetalRule {
    const FAMILY: &'static str = "presentation";
    const KIND: &'static str = "sheet-metal-rule";
}

impl RecordPayload for PmAppUnfoldRule {
    const FAMILY: &'static str = "presentation";
    const KIND: &'static str = "unfold-rule";
}

/// Typed sheet-metal style records of every `PmApp` segment.
#[derive(Debug, Default)]
pub(crate) struct SheetMetalInventory {
    pub(crate) rules: Vec<Located<PmAppSheetMetalRule>>,
    pub(crate) unfold_rules: Vec<Located<PmAppUnfoldRule>>,
    /// Presentation record issues of the sheet-metal style records.
    pub(crate) issues: Vec<RecordIssue>,
}

pub(crate) fn inventory(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
) -> Result<SheetMetalInventory, CodecError> {
    let mut inventory = SheetMetalInventory::default();
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmApp {
            continue;
        }
        let Some(version) = segment.registry.map(|join| join.version_major) else {
            continue;
        };
        if !supports_version(version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        let token = segment.pair.token.key();
        for record in &table.records {
            let result = match record.type_id {
                SHEET_METAL_RULE_TYPE => parse_sheet_metal_rule(ctx, record.payload, version)
                    .and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.rules,
                            value,
                            record.type_id,
                            token,
                            record.ordinal,
                            "admit Inventor sheet-metal rule record",
                        )
                    }),
                UNFOLD_RULE_TYPE => {
                    parse_unfold_rule(ctx, record.payload, version).and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.unfold_rules,
                            value,
                            record.type_id,
                            token,
                            record.ordinal,
                            "admit Inventor unfold rule record",
                        )
                    })
                }
                _ => continue,
            };
            if let Err(error) = result {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                ctx.charge_collection_items(1, "admit Inventor presentation issue")?;
                ctx.charge_entities(1, "admit Inventor presentation issue")?;
                ctx.charge_formatted_retained(
                    format_args!("{error}"),
                    "retain Inventor presentation issue detail",
                )?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(segment.pair.token.as_str().len()),
                    "retain Inventor presentation issue token",
                )?;
                inventory.issues.push(RecordIssue {
                    family: RecordIssueFamily::Presentation,
                    segment_token: token.clone(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    Ok(inventory)
}

fn style_header(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmAppStyleHeader, CodecError> {
    Ok(PmAppStyleHeader {
        header_value: cursor.u32("style header value")?,
        header_id: cursor.u16("style header id")?,
        state: cursor.u8("style state")?,
        flags: cursor.u16("style flags")?,
        values: [cursor.u16("style value 0")?, cursor.u16("style value 1")?],
        default_state: cursor.u32("style default state")?,
        value: cursor.u32("style value")?,
        category: cursor.reference("style category reference")?,
        name: cursor.utf16(ctx, "style name")?,
        long_name: cursor.utf16(ctx, "style long name")?,
        style_state: cursor.u16("style label state")?,
        label: cursor.utf16(ctx, "style label")?,
    })
}

pub(crate) fn parse_sheet_metal_rule(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmAppSheetMetalRule, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = style_header(ctx, &mut cursor)?;
    let text = |cursor: &mut Cursor<'_>| cursor.utf16(ctx, "sheet-metal rule text");
    let text_0 = text(&mut cursor)?;
    let text_1 = text(&mut cursor)?;
    let text_2 = text(&mut cursor)?;
    let text_3 = text(&mut cursor)?;
    let text_4 = text(&mut cursor)?;
    let value_0 = cursor.u32("sheet-metal rule value 0")?;
    let value_1 = cursor.u32("sheet-metal rule value 1")?;
    let value_2 = cursor.u32("sheet-metal rule value 2")?;
    let text_5 = text(&mut cursor)?;
    let value_3 = cursor.u32("sheet-metal rule value 3")?;
    let text_6 = text(&mut cursor)?;
    let value_4 = cursor.u32("sheet-metal rule value 4")?;
    let unfold_rule = cursor.reference("sheet-metal rule unfold-rule reference")?;
    let material_style = cursor.reference("sheet-metal rule material reference")?;
    let text_7 = text(&mut cursor)?;
    let text_8 = text(&mut cursor)?;
    let value_5 = cursor.u32("sheet-metal rule value 5")?;
    cursor.finish("sheet-metal rule")?;
    Ok(PmAppSheetMetalRule {
        segment_version_major: version,
        header,
        texts: [
            text_0, text_1, text_2, text_3, text_4, text_5, text_6, text_7, text_8,
        ],
        integers: [value_0, value_1, value_2, value_3, value_4, value_5],
        unfold_rule,
        material_style,
    })
}

pub(crate) fn parse_unfold_rule(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmAppUnfoldRule, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = style_header(ctx, &mut cursor)?;
    let method = cursor.u32("unfold rule method")?;
    let factor = cursor.utf16(ctx, "unfold rule factor")?;
    let secondary_factor = cursor.utf16(ctx, "unfold rule secondary factor")?;
    let members = reference_list(ctx, &mut cursor, 2, "unfold rule member list")?;
    let value_count = cursor.u32("unfold rule value count")?;
    let scalars = [
        cursor.f64("unfold rule scalar 0")?,
        cursor.f64("unfold rule scalar 1")?,
        cursor.f64("unfold rule scalar 2")?,
    ];
    let first_state = cursor.u8("unfold rule state 0")?;
    let second_state = cursor.u8("unfold rule state 1")?;
    let mode = cursor.u32("unfold rule mode")?;
    let references = reference_list(ctx, &mut cursor, 2, "unfold rule reference list")?;
    let tertiary_factor = cursor.utf16(ctx, "unfold rule tertiary factor")?;
    cursor.finish("unfold rule")?;
    Ok(PmAppUnfoldRule {
        segment_version_major: version,
        header,
        method,
        factor,
        secondary_factor,
        members,
        value_count,
        scalars,
        first_state,
        second_state,
        mode,
        references,
        tertiary_factor,
    })
}

#[cfg(test)]
mod tests;
