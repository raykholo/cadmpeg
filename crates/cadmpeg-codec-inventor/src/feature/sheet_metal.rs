// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal face and flange groups, the bend and corner property records
//! their plates share, and the plate projection.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureOperation,
    FeatureResultTopology, PlanarProfileRef,
};
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

use super::{
    admit_projected_feature, body_result, boolean, insert_history_property, length_parameter,
    property, resolve_direction, slot_property, ClassId, PmDcFeature, PmDcFeatureLabel,
    PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload, PmDcFeatureReferenceFamily,
    ProjectionIndex,
};
use crate::pmdc::{content_header, reference_list, Cursor, PmDcContentHeader, PmDcReferenceList};
use crate::record_identity::{Located, RecordPayload};

/// Group record of one sheet-metal face.
pub(super) const FACE_GROUP_TYPE: [u8; 16] = [
    0x90, 0x4b, 0xc1, 0xc4, 0xd3, 0x11, 0xff, 0x28, 0x60, 0x00, 0x4d, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];
/// Group record of one sheet-metal flange.
pub(super) const FLANGE_GROUP_TYPE: [u8; 16] = [
    0x08, 0xdc, 0xdd, 0xc3, 0xd3, 0x11, 0xd0, 0x97, 0x60, 0x00, 0xa7, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];
pub(super) const BEND_DEFINITION_TYPE: [u8; 16] = [
    0x92, 0x59, 0x6b, 0xa9, 0xd3, 0x11, 0xef, 0xa8, 0x60, 0x00, 0xb3, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];
pub(super) const BEND_RELIEF_TYPE: [u8; 16] = [
    0x64, 0x88, 0x05, 0x96, 0xd3, 0x11, 0x9a, 0x76, 0x60, 0x00, 0x81, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];
pub(super) const BEND_SETTINGS_TYPE: [u8; 16] = [
    0xe9, 0x34, 0x50, 0x26, 0xd5, 0x11, 0x4c, 0x08, 0x10, 0x00, 0xe5, 0x96, 0xc2, 0x8b, 0x7a, 0xb5,
];
pub(super) const CORNER_DEFINITION_TYPE: [u8; 16] = [
    0x83, 0xce, 0x77, 0x86, 0xa9, 0x46, 0xcd, 0xe1, 0x1e, 0xbd, 0x99, 0xb4, 0xfe, 0x02, 0xaf, 0xb2,
];
pub(super) const CORNER_EDGES_TYPE: [u8; 16] = [
    0xce, 0x2d, 0x9b, 0x29, 0xad, 0x43, 0xd8, 0x80, 0x2f, 0x13, 0x2c, 0x9a, 0x40, 0xc9, 0xba, 0x90,
];
pub(super) const CORNER_VALUE_TYPE: [u8; 16] = [
    0x05, 0x17, 0x72, 0xe5, 0xa7, 0x4f, 0x64, 0x1a, 0x01, 0x38, 0x6e, 0x8f, 0x3c, 0xc0, 0x22, 0x18,
];

/// Feature-label class of a sheet-metal bend.
pub(super) const BEND_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0xf2, 0x58, 0xf7, 0x4c, 0xd3, 0x11, 0x4f, 0x24, 0x60, 0x00, 0x47, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
]));
/// Feature-label class of a sheet-metal corner.
pub(super) const CORNER_CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x7f, 0x58, 0x80, 0x68, 0xb7, 0x43, 0x78, 0xa0, 0x55, 0x95, 0x15, 0x94, 0x6f, 0x7a, 0x1c, 0x12,
]));

/// Fixed reference-tuple families of the sheet-metal property records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcSheetMetalFamily {
    /// Ten references, three state bytes, and one reference.
    BendDefinition,
    /// Six references.
    BendRelief,
    /// Three references.
    BendSettings,
    /// Nine references.
    CornerDefinition,
}

impl PmDcSheetMetalFamily {
    fn of(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            BEND_DEFINITION_TYPE => Some(Self::BendDefinition),
            BEND_RELIEF_TYPE => Some(Self::BendRelief),
            BEND_SETTINGS_TYPE => Some(Self::BendSettings),
            CORNER_DEFINITION_TYPE => Some(Self::CornerDefinition),
            _ => None,
        }
    }

    /// References before the state bytes and the state-byte count.
    fn layout(self) -> (usize, usize) {
        match self {
            Self::BendDefinition => (10, 3),
            Self::BendRelief => (6, 0),
            Self::BendSettings => (3, 0),
            Self::CornerDefinition => (9, 0),
        }
    }
}

/// Parse one fixed sheet-metal property record selected by `type_id`.
pub(super) fn parse_sheet_metal_property(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
    type_id: [u8; 16],
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let family = PmDcSheetMetalFamily::of(type_id)
        .ok_or_else(|| CodecError::malformed("Inventor PmDc sheet-metal record type is unknown"))?;
    let (count, state_count) = family.layout();
    let total = count + usize::from(state_count > 0);
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, crate::pmdc::extended(version))?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(total),
        "admit Inventor sheet-metal property references",
    )?;
    let mut references =
        ctx.vector_storage(total, "admit Inventor sheet-metal property references")?;
    for _ in 0..count {
        references.push(cursor.reference("sheet-metal property reference")?);
    }
    let mut states = Vec::new();
    if state_count > 0 {
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(state_count),
            "retain Inventor sheet-metal property states",
        )?;
        ctx.reserve_capacity(
            &mut states,
            state_count,
            "retain Inventor sheet-metal property states",
        )?;
        for _ in 0..state_count {
            states.push(cursor.u8("sheet-metal property state")?);
        }
        references.push(cursor.reference("sheet-metal property final reference")?);
    }
    cursor.finish("sheet-metal property")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::SheetMetal {
            family,
            references,
            states: NativeBytes::from(states),
        },
    ))
}

macro_rules! sheet_metal_parser {
    ($name:ident, $type_id:ident) => {
        pub(super) fn $name(
            ctx: &DecodeContext<'_>,
            source: View<'_>,
            version: u8,
        ) -> Result<PmDcFeaturePropertyPayload, CodecError> {
            parse_sheet_metal_property(ctx, source, version, $type_id)
        }
    };
}

sheet_metal_parser!(parse_bend_definition, BEND_DEFINITION_TYPE);
sheet_metal_parser!(parse_bend_relief, BEND_RELIEF_TYPE);
sheet_metal_parser!(parse_bend_settings, BEND_SETTINGS_TYPE);
sheet_metal_parser!(parse_corner_definition, CORNER_DEFINITION_TYPE);

/// Browser form of a sheet-metal group record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PmDcSheetMetalGroupForm {
    /// A face: its plate, optionally followed by the plate's bend.
    Face,
    /// A flange: its plate, the plate's bend, and the plate's corner.
    Flange,
}

impl PmDcSheetMetalGroupForm {
    fn name(self) -> &'static str {
        match self {
            Self::Face => "face",
            Self::Flange => "flange",
        }
    }
}

/// Group record that owns the feature label of one sheet-metal face or flange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcSheetMetalGroupPayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) form: PmDcSheetMetalGroupForm,
    value: i32,
    pub(crate) members: PmDcReferenceList,
    terminal: u32,
}

pub(crate) type PmDcSheetMetalGroup = Located<PmDcSheetMetalGroupPayload>;

impl RecordPayload for PmDcSheetMetalGroupPayload {
    const KIND: &'static str = "sheet-metal-group";
}

/// Parse a face or flange group record selected by `type_id`.
pub(super) fn parse_group(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
    type_id: [u8; 16],
) -> Result<PmDcSheetMetalGroupPayload, CodecError> {
    let form = match type_id {
        FACE_GROUP_TYPE => PmDcSheetMetalGroupForm::Face,
        FLANGE_GROUP_TYPE => PmDcSheetMetalGroupForm::Flange,
        _ => {
            return Err(CodecError::malformed(
                "Inventor PmDc sheet-metal group type is unknown",
            ))
        }
    };
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, 0)?;
    let value = cursor.i32("sheet-metal group value")?;
    let members = reference_list(ctx, &mut cursor, 2, "sheet-metal group members")?;
    let terminal = cursor.u32("sheet-metal group terminal value")?;
    cursor.finish("sheet-metal group")?;
    Ok(PmDcSheetMetalGroupPayload {
        save_version_major: version,
        header,
        form,
        value,
        members,
        terminal,
    })
}

/// Plate slots that the bend member repeats: `(plate slot, bend slot)`.
const BEND_SHARED_SLOTS: [(usize, usize); 4] = [(4, 3), (7, 0), (3, 2), (6, 5)];
/// Plate slots that a flange bend also repeats.
const FLANGE_BEND_SHARED_SLOTS: [(usize, usize); 2] = [(10, 4), (11, 7)];
/// Plate slots that a flange corner repeats: `(plate slot, corner slot)`.
const FLANGE_CORNER_SHARED_SLOTS: [(usize, usize); 4] = [(9, 0), (12, 3), (0, 4), (6, 1)];

/// The feature record a group member names, with its label class.
fn member<'a>(
    token: &'a str,
    reference: crate::pmdc::PmDcReference,
    class_id: ClassId,
    index: &'a ProjectionIndex<'a>,
) -> Option<&'a PmDcFeature> {
    let ordinal = reference.record_ordinal()?;
    let feature = index
        .features
        .get(&(token, ordinal))
        .and_then(Option::as_ref)?;
    let label: &PmDcFeatureLabel = index
        .owner_labels
        .get(&(token, Some(ordinal)))
        .and_then(Option::as_ref)?;
    (label.class_id() == class_id).then_some(*feature)
}

/// `source` slot `own` and `other` slot `theirs` name the same nonnull record.
fn shares(source: &PmDcFeature, other: &PmDcFeature, own: usize, theirs: usize) -> bool {
    match (
        source.properties.references().get(own),
        other.properties.references().get(theirs),
    ) {
        (Some(left), Some(right)) => left.index() != 0 && left.index() == right.index(),
        _ => false,
    }
}

/// The property in `slot` is a sheet-metal record of `family`.
fn sheet_metal_slot(
    source: &PmDcFeature,
    slot: usize,
    family: PmDcSheetMetalFamily,
    index: &ProjectionIndex<'_>,
) -> bool {
    slot_property(source, slot, index).is_some_and(|property| {
        matches!(&property.kind, PmDcFeaturePropertyKind::SheetMetal { family: actual, .. } if *actual == family)
    })
}

/// Project a sheet-metal plate: a planar profile of its group's sketch
/// thickened along the sketch normal.
pub(super) fn project_plate(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let token = source.identity.segment_token.as_str();
    let references = source.properties.references();
    if references.len() != 15
        || [5, 14].into_iter().any(|slot| {
            references
                .get(slot)
                .is_none_or(|reference| reference.index() != 0)
        })
    {
        return None;
    }
    let selection = slot_property(source, 0, index)?;
    let PmDcFeaturePropertyKind::ProfileSelection { entity_link, .. } = &selection.kind else {
        return None;
    };
    if !index
        .entity_style_links
        .contains(&(token, entity_link.index().checked_sub(1)?))
    {
        return None;
    }
    let flip = boolean(source, 2, index)?;
    let thickness =
        cadmpeg_ir::scalar::PositiveLength::try_from(length_parameter(source, 3, index)?).ok()?;
    let edges = slot_property(source, 7, index)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::EdgeCollection,
        items,
    } = &edges.kind
    else {
        return None;
    };
    if !items.references().is_empty() {
        return None;
    }
    let wrapper = index
        .containing_labels
        .get(&(token, source.identity.record_ordinal))
        .and_then(Option::as_ref)?;
    let group = index
        .sheet_metal_groups
        .get(&(token, wrapper.header.owner.record_ordinal()?))
        .and_then(Option::as_ref)?;
    let [sketch_reference, features @ ..] = wrapper.participants.references() else {
        return None;
    };
    if features != group.members.references()
        || features.first()?.record_ordinal() != Some(source.identity.record_ordinal)
    {
        return None;
    }
    let sketch = index
        .sketches
        .get(&(token, sketch_reference.record_ordinal()?))
        .and_then(Option::as_ref)?;
    if sketch.direction.index() == 0 {
        return None;
    }
    let null_from = |first: usize| {
        (first..14).all(|slot| {
            references
                .get(slot)
                .is_some_and(|reference| reference.index() == 0)
        })
    };
    match (group.form, features) {
        (PmDcSheetMetalGroupForm::Face, [_]) => {
            if references.get(4)?.index() != 0 || !null_from(9) {
                return None;
            }
        }
        (PmDcSheetMetalGroupForm::Face, [_, bend]) => {
            let bend = member(token, *bend, BEND_CLASS_ID, index)?;
            if !sheet_metal_slot(source, 4, PmDcSheetMetalFamily::BendDefinition, index)
                || !null_from(9)
                || !BEND_SHARED_SLOTS
                    .into_iter()
                    .all(|(own, theirs)| shares(source, bend, own, theirs))
            {
                return None;
            }
        }
        (PmDcSheetMetalGroupForm::Flange, [_, bend, corner]) => {
            let bend = member(token, *bend, BEND_CLASS_ID, index)?;
            let corner = member(token, *corner, CORNER_CLASS_ID, index)?;
            if !sheet_metal_slot(source, 4, PmDcSheetMetalFamily::BendDefinition, index)
                || !sheet_metal_slot(source, 9, PmDcSheetMetalFamily::CornerDefinition, index)
                || !sheet_metal_slot(source, 10, PmDcSheetMetalFamily::BendRelief, index)
                || !sheet_metal_slot(source, 11, PmDcSheetMetalFamily::BendSettings, index)
                || !matches!(
                    slot_property(source, 12, index)?.kind,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::CornerEdges,
                        ..
                    }
                )
                || !matches!(
                    slot_property(source, 13, index)?.kind,
                    PmDcFeaturePropertyKind::Value { .. }
                )
                || resolve_direction(source, 1, index).is_none()
                || !BEND_SHARED_SLOTS
                    .into_iter()
                    .chain(FLANGE_BEND_SHARED_SLOTS)
                    .all(|(own, theirs)| shares(source, bend, own, theirs))
                || !FLANGE_CORNER_SHARED_SLOTS
                    .into_iter()
                    .all(|(own, theirs)| shares(source, corner, own, theirs))
            {
                return None;
            }
        }
        _ => return None,
    }
    if group.form == PmDcSheetMetalGroupForm::Face && sketch.direction != *references.get(1)? {
        return None;
    }
    let side = if flip {
        cadmpeg_ir::features::SheetMetalThicknessSide::Reverse
    } else {
        cadmpeg_ir::features::SheetMetalThicknessSide::Forward
    };
    let sketch_id = index.sketch_ids.get(sketch.id().as_str())?;
    let sketch_id = match sketch_id.try_clone_for_decode(ctx, "retain Inventor plate sketch id") {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(selection.id_len()?),
        "retain Inventor plate selection id",
    ) {
        return Some(Err(error));
    }
    let profile = match PlanarProfileRef::sketch_selection(sketch_id, vec![selection.id()], ctx) {
        Ok(Ok(profile)) => profile,
        Ok(Err(_)) => return None,
        Err(limit) => return Some(Err(limit.into())),
    };
    let (feature_id, result) = match body_result(ctx, source, 6, index)? {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = admit_projected_feature(ctx, source, label, "sheet_metal_plate") {
        return Some(Err(error));
    }
    let mut feature = Feature {
        id: feature_id,
        ordinal: u64::from(label.index),
        name: label.name().map(str::to_owned),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some("sheet_metal_plate".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
                profile,
                thickness,
                side,
            }),
        ),
        native_ref: Some(source.id()),
    };
    if let Err(error) =
        insert_history_property(ctx, &mut feature, "sheet_metal_group", group.form.name())
    {
        return Some(Err(error));
    }
    Some(Ok((feature, result)))
}
