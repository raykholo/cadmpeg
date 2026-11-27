// SPDX-License-Identifier: Apache-2.0
//! `DeleteFace` operands, selector-list property, admission, and native losses
//! ([spec §13.7](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#137-deleteface-property-vector)).

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashSet;

use super::{
    property, ClassId, FeatureInventory, PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeaturePropertyPayload, PmDcFeatureReferenceFamily, PropertyParser,
};
use crate::loss::InventorLossCode;
use crate::pmdc::{content_header, reference_list, Cursor, PmDcReference};

pub(super) const CLASS_BYTES: [u8; 16] = [
    0x68, 0x6e, 0x16, 0xd3, 0x33, 0x4f, 0x63, 0x71, 0x61, 0xa6, 0xa1, 0x9b, 0xdd, 0x3d, 0x2b, 0xe8,
];
pub(crate) const CLASS_ID: ClassId =
    ClassId(cadmpeg_ir::native::bytes::NativeBytes::new(CLASS_BYTES));

/// Slot-0 record type: the face selector list.
pub(super) const SELECTORS_TYPE: [u8; 16] = [
    0x74, 0x4d, 0x87, 0x90, 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc, 0x06, 0x63, 0xdc, 0x09,
];
/// Record type of a face naming selector.
const SELECTOR_TYPE_ID: &str = "2c81aa03d211ff2600081fbb32a3dc09";
/// Record type of a surface-body wrapper.
const WRAPPER_TYPE_ID: &str = "474d8790d011f8d10008cabc0663dc09";
/// Slot-3 record type: the result entity array.
const RESULT_TYPE_ID: &str = "e0291ea3c5454f60dc9ca38c8e08262c";

pub(super) fn property_parser(type_id: [u8; 16]) -> Option<PropertyParser> {
    (type_id == SELECTORS_TYPE).then_some(parse_selectors as PropertyParser)
}

fn parse_selectors(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let items = reference_list(&mut cursor, 2, "delete-face selectors")?;
    cursor.reference("delete-face selector owner")?;
    cursor.finish("delete-face selectors")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FaceSelectors,
            items,
        },
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Marker,
    Slots,
    Selectors,
    Control,
    Selection,
    Result,
}

impl NativeReason {
    pub(crate) const fn detail(self) -> &'static str {
        match self {
            Self::Marker => "property vector marker differs from type 2",
            Self::Slots => "property vector differs from the four-slot form",
            Self::Selectors => "slot 0 is not a list of distinct face naming selectors",
            Self::Control => "slot 1 is true; its construction role is unresolved",
            Self::Selection => "slot 2 is not a body selection with one surface-body wrapper",
            Self::Result => "slot 3 is not the result entity array",
        }
    }

    pub(crate) fn note(self) -> LossNote {
        InventorLossCode::FeatureDeleteFaceOperandsNative.note(format!(
            "Retained Inventor DeleteFace natively: {}.",
            self.detail()
        ))
    }
}

/// The neutral operation of a four-slot `DeleteFace` record.
pub(crate) fn operation<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    record_type: impl Fn(u32) -> Option<&'a str>,
) -> Result<FeatureOperation, NativeReason> {
    if source.properties.marker() != 2 {
        return Err(NativeReason::Marker);
    }
    let [selectors, control, selection, result] = source.properties.references() else {
        return Err(NativeReason::Slots);
    };
    let selectors = property(*selectors).ok_or(NativeReason::Selectors)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::FaceSelectors,
        items,
    } = &selectors.kind
    else {
        return Err(NativeReason::Selectors);
    };
    let mut distinct = HashSet::new();
    if items.references().is_empty()
        || items.references().iter().any(|member| {
            !member.qualified()
                || !distinct.insert(member.index())
                || member.record_ordinal().and_then(&record_type) != Some(SELECTOR_TYPE_ID)
        })
    {
        return Err(NativeReason::Selectors);
    }
    let Some(PmDcFeaturePropertyKind::Boolean { value, .. }) =
        property(*control).map(|property| &property.kind)
    else {
        return Err(NativeReason::Control);
    };
    if *value {
        return Err(NativeReason::Control);
    }
    let Some(PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::BodySelection,
        items,
    }) = property(*selection).map(|property| &property.kind)
    else {
        return Err(NativeReason::Selection);
    };
    let [wrapper] = items.references() else {
        return Err(NativeReason::Selection);
    };
    if !wrapper.qualified()
        || wrapper.record_ordinal().and_then(&record_type) != Some(WRAPPER_TYPE_ID)
    {
        return Err(NativeReason::Selection);
    }
    if result.record_ordinal().and_then(&record_type) != Some(RESULT_TYPE_ID) {
        return Err(NativeReason::Result);
    }
    Ok(FeatureOperation::DeleteFace {
        faces: FaceSelection::Native(selectors.id()),
        heal: HEAL_OF_FALSE_CONTROL,
    })
}

/// Whether the false slot-1 control heals the opening.
const HEAL_OF_FALSE_CONTROL: bool = false;

/// The neutral operation of `source` read from the properties of `inventory`.
fn inventory_operation(
    inventory: &FeatureInventory,
    source: &PmDcFeature,
) -> Result<FeatureOperation, NativeReason> {
    let token = source.identity.segment_token.as_str();
    let properties = inventory
        .properties
        .iter()
        .filter(|property| property.identity.segment_token.as_str() == token)
        .map(|property| (property.identity.record_ordinal, property))
        .collect::<std::collections::HashMap<_, _>>();
    let types = inventory.record_types.get(&source.identity.segment_token);
    operation(
        source,
        |reference| properties.get(&reference.record_ordinal()?).copied(),
        |ordinal| types?.get(&ordinal)?.as_deref(),
    )
}

fn owners(inventory: &FeatureInventory) -> HashSet<(&str, u32)> {
    inventory
        .labels
        .iter()
        .filter(|label| label.class_id() == CLASS_ID)
        .filter_map(|label| {
            Some((
                label.identity.segment_token.as_str(),
                label.header.owner.record_ordinal()?,
            ))
        })
        .collect()
}

/// One loss for each `DeleteFace` record that the projected history retains
/// as a native feature. A record that no model feature places has no loss.
pub(crate) fn native_losses(
    inventory: &FeatureInventory,
    features: &[cadmpeg_ir::features::Feature],
) -> Vec<LossNote> {
    let placed = features
        .iter()
        .filter(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            )
        })
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<HashSet<_>>();
    let owners = owners(inventory);
    inventory
        .features
        .iter()
        .filter(|source| {
            owners.contains(&(
                source.identity.segment_token.as_str(),
                source.identity.record_ordinal,
            ))
        })
        .filter(|source| placed.contains(source.id().as_str()))
        .filter_map(|source| inventory_operation(inventory, source).err())
        .map(NativeReason::note)
        .collect()
}

/// One note for the `DeleteFace` features that the model types with the
/// assumed healing reading of the false slot-1 control.
pub(crate) fn assumption_losses(features: &[cadmpeg_ir::features::Feature]) -> Vec<LossNote> {
    let count = features
        .iter()
        .filter(|feature| feature.source_tag.as_deref() == Some(SOURCE_TAG))
        .count();
    if count == 0 {
        return Vec::new();
    }
    vec![InventorLossCode::FeatureDeleteFaceHealAssumed.note(format!(
        "{count} Inventor DeleteFace operation(s) read the false slot-1 control as {}.",
        if HEAL_OF_FALSE_CONTROL {
            "healing the opening"
        } else {
            "leaving the opening"
        }
    ))]
}

/// Source tag of a typed `DeleteFace` feature.
pub(crate) const SOURCE_TAG: &str = "delete_face";

#[cfg(test)]
mod tests;
