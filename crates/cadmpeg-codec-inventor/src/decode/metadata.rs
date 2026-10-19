// SPDX-License-Identifier: Apache-2.0

//! Neutral document metadata selected from OLE property identities.
//!
//! A neutral key is selected only by `(FMTID, property ID)`. Each key lists its admitted
//! identities in precedence order; property names never select a key.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::property_set::PropertyValue;
use crate::rse::DocumentKind;

/// Public OLE `SummaryInformation` FMTID `F29F85E0-4FF9-1068-AB91-08002B27B3D9`.
pub(super) const FMTID_SUMMARY_INFORMATION: [u8; 16] = [
    0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3, 0xd9,
];
/// Public OLE `DocumentSummaryInformation` FMTID `D5CDD502-2E9C-101B-9397-08002B2CF9AE`.
pub(super) const FMTID_DOCUMENT_SUMMARY_INFORMATION: [u8; 16] = [
    0x02, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9, 0xae,
];
/// Inventor Summary Information FMTID `3D38DE39-0588-4C14-BB37-18F4D5DD31C7`.
pub(super) const FMTID_INVENTOR_SUMMARY: [u8; 16] = [
    0x39, 0xde, 0x38, 0x3d, 0x88, 0x05, 0x14, 0x4c, 0xbb, 0x37, 0x18, 0xf4, 0xd5, 0xdd, 0x31, 0xc7,
];
/// Inventor Document Summary Information FMTID `8CF58000-DA66-4AE6-8FF0-7B58406FB049`.
pub(super) const FMTID_INVENTOR_DOCUMENT_SUMMARY: [u8; 16] = [
    0x00, 0x80, 0xf5, 0x8c, 0x66, 0xda, 0xe6, 0x4a, 0x8f, 0xf0, 0x7b, 0x58, 0x40, 0x6f, 0xb0, 0x49,
];
/// Design Tracking Properties FMTID `32853F0F-3444-11D1-9E93-0060B03C1CA6`.
pub(super) const FMTID_DESIGN_TRACKING_PROPERTIES: [u8; 16] = [
    0x0f, 0x3f, 0x85, 0x32, 0x44, 0x34, 0xd1, 0x11, 0x9e, 0x93, 0x00, 0x60, 0xb0, 0x3c, 0x1c, 0xa6,
];
/// Design Tracking Control FMTID `D861FB30-3136-11D1-9E92-0060B03C1CA6`.
pub(super) const FMTID_DESIGN_TRACKING_CONTROL: [u8; 16] = [
    0x30, 0xfb, 0x61, 0xd8, 0x36, 0x31, 0xd1, 0x11, 0x9e, 0x92, 0x00, 0x60, 0xb0, 0x3c, 0x1c, 0xa6,
];

/// Value form a neutral key admits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueForm {
    /// A `VT_LPSTR` or `VT_LPWSTR` string.
    Text,
    /// A nonzero `VT_FILETIME`, written as an RFC 3339 UTC timestamp.
    Time,
}

/// Neutral metadata keys, in `source.attributes` serialization order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum MetadataKey {
    Title,
    Subject,
    Author,
    Keywords,
    Comments,
    Description,
    Company,
    Project,
    Revision,
    Designer,
    Material,
    Status,
    PartNumber,
    CreatedAt,
    SavedAt,
}

impl MetadataKey {
    const ALL: [Self; 15] = [
        Self::Title,
        Self::Subject,
        Self::Author,
        Self::Keywords,
        Self::Comments,
        Self::Description,
        Self::Company,
        Self::Project,
        Self::Revision,
        Self::Designer,
        Self::Material,
        Self::Status,
        Self::PartNumber,
        Self::CreatedAt,
        Self::SavedAt,
    ];

    pub(super) fn attribute(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Subject => "subject",
            Self::Author => "author",
            Self::Keywords => "keywords",
            Self::Comments => "comments",
            Self::Description => "description",
            Self::Company => "company",
            Self::Project => "project",
            Self::Revision => "revision",
            Self::Designer => "designer",
            Self::Material => "material",
            Self::Status => "status",
            Self::PartNumber => "part_number",
            Self::CreatedAt => "created_at",
            Self::SavedAt => "saved_at",
        }
    }

    fn form(self) -> ValueForm {
        match self {
            Self::CreatedAt | Self::SavedAt => ValueForm::Time,
            _ => ValueForm::Text,
        }
    }

    /// Admitted identities, highest precedence first.
    fn identities(self) -> &'static [([u8; 16], u32)] {
        match self {
            Self::Title => &[(FMTID_INVENTOR_SUMMARY, 2), (FMTID_SUMMARY_INFORMATION, 2)],
            Self::Subject => &[(FMTID_SUMMARY_INFORMATION, 3)],
            Self::Author => &[(FMTID_INVENTOR_SUMMARY, 4), (FMTID_SUMMARY_INFORMATION, 4)],
            Self::Keywords => &[(FMTID_SUMMARY_INFORMATION, 5)],
            Self::Comments => &[(FMTID_INVENTOR_SUMMARY, 6), (FMTID_SUMMARY_INFORMATION, 6)],
            Self::Description => &[(FMTID_DESIGN_TRACKING_PROPERTIES, 29)],
            Self::Company => &[
                (FMTID_INVENTOR_DOCUMENT_SUMMARY, 15),
                (FMTID_DOCUMENT_SUMMARY_INFORMATION, 15),
            ],
            Self::Project => &[(FMTID_DESIGN_TRACKING_PROPERTIES, 7)],
            Self::Revision => &[(FMTID_INVENTOR_SUMMARY, 9), (FMTID_SUMMARY_INFORMATION, 9)],
            Self::Designer => &[(FMTID_DESIGN_TRACKING_PROPERTIES, 41)],
            Self::Material => &[(FMTID_DESIGN_TRACKING_PROPERTIES, 20)],
            Self::Status => &[(FMTID_DESIGN_TRACKING_PROPERTIES, 17)],
            Self::PartNumber => &[(FMTID_DESIGN_TRACKING_PROPERTIES, 5)],
            Self::CreatedAt => &[
                (FMTID_DESIGN_TRACKING_PROPERTIES, 4),
                (FMTID_SUMMARY_INFORMATION, 12),
            ],
            Self::SavedAt => &[
                (FMTID_DESIGN_TRACKING_CONTROL, 17),
                (FMTID_SUMMARY_INFORMATION, 13),
            ],
        }
    }

    /// The key and precedence rank selected by one property identity.
    pub(super) fn select(fmtid: &[u8; 16], property_id: u32) -> Option<(Self, usize)> {
        Self::ALL.into_iter().find_map(|key| {
            key.identities()
                .iter()
                .position(|(set, id)| set == fmtid && *id == property_id)
                .map(|rank| (key, rank))
        })
    }
}

struct Selected {
    rank: usize,
    value: String,
    native_id: String,
}

#[derive(Default)]
pub(super) struct MetadataProjection {
    selected: BTreeMap<MetadataKey, Selected>,
    pub(super) document_kind: Option<DocumentKind>,
    pub(super) bom_properties: BTreeMap<String, String>,
    pub(super) unmapped: usize,
}

impl MetadataProjection {
    pub(super) fn get(&self, key: MetadataKey) -> Option<&str> {
        self.selected
            .get(&key)
            .map(|selected| selected.value.as_str())
    }

    #[cfg(test)]
    pub(super) fn with_value(key: MetadataKey, value: &str) -> Self {
        let mut projection = Self::default();
        projection.selected.insert(
            key,
            Selected {
                rank: 0,
                value: value.into(),
                native_id: String::new(),
            },
        );
        projection
    }

    /// Considers one property, identified by its section FMTID and property ID.
    pub(super) fn consider(
        &mut self,
        ctx: &DecodeContext<'_>,
        (fmtid, property_id): (&[u8; 16], u32),
        name: Option<&str>,
        value: &PropertyValue<'_>,
        scalar: Option<&str>,
        native_id: &str,
    ) -> Result<(), CodecError> {
        let Some(scalar) = scalar.filter(|scalar| !scalar.is_empty()) else {
            return Ok(());
        };
        if let Some((key, rank)) = MetadataKey::select(fmtid, property_id) {
            if let Some(neutral) = neutral_value(ctx, key.form(), value)? {
                return self.select(ctx, key, rank, neutral, native_id);
            }
        }
        let normalized = name
            .map(|name| super::normalize_property_name(ctx, name))
            .transpose()?;
        if matches!(normalized.as_deref(), Some("documentkind" | "documenttype")) {
            self.document_kind = DocumentKind::parse_property(scalar);
            if self.document_kind.is_some() {
                return Ok(());
            }
        }
        match name {
            Some(name) => self.retain_bom(ctx, name, scalar),
            None => {
                self.unmapped += 1;
                Ok(())
            }
        }
    }

    fn select(
        &mut self,
        ctx: &DecodeContext<'_>,
        key: MetadataKey,
        rank: usize,
        value: String,
        native_id: &str,
    ) -> Result<(), CodecError> {
        let displaced = match self.selected.get(&key) {
            None => None,
            Some(current) if current.rank <= rank => {
                if current.value != value {
                    return self.retain_bom(ctx, native_id, &value);
                }
                return Ok(());
            }
            Some(_) => self.selected.remove(&key),
        };
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(native_id.len()),
            "retain Inventor metadata identity",
        )?;
        ctx.charge_collection_items(1, "collect Inventor metadata value")?;
        if let Some(displaced) = displaced {
            if displaced.value != value {
                self.retain_bom(ctx, &displaced.native_id, &displaced.value)?;
            }
        }
        self.selected.insert(
            key,
            Selected {
                rank,
                value,
                native_id: native_id.into(),
            },
        );
        Ok(())
    }

    fn retain_bom(
        &mut self,
        ctx: &DecodeContext<'_>,
        key: &str,
        value: &str,
    ) -> Result<(), CodecError> {
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(key.len()),
            "retain Inventor BOM property key",
        )?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(value.len()),
            "retain Inventor BOM property value",
        )?;
        ctx.insert_btree_map(
            &mut self.bom_properties,
            key.into(),
            value.into(),
            "collect Inventor BOM property",
        )?;
        Ok(())
    }

    pub(super) fn apply_attributes(
        &self,
        ctx: &DecodeContext<'_>,
        attributes: &mut BTreeMap<String, String>,
    ) -> Result<(), CodecError> {
        for (key, selected) in &self.selected {
            let name = key.attribute();
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(name.len()),
                "retain Inventor metadata attribute key",
            )?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(selected.value.len()),
                "retain Inventor metadata attribute value",
            )?;
            ctx.insert_btree_map(
                attributes,
                name.into(),
                selected.value.clone(),
                "collect Inventor metadata attribute",
            )?;
        }
        Ok(())
    }
}

fn neutral_value(
    ctx: &DecodeContext<'_>,
    form: ValueForm,
    value: &PropertyValue<'_>,
) -> Result<Option<String>, CodecError> {
    match (form, value) {
        (ValueForm::Text, PropertyValue::String { value, .. }) if !value.is_empty() => {
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(value.len()),
                "retain Inventor metadata value",
            )?;
            Ok(Some(value.clone()))
        }
        (ValueForm::Time, PropertyValue::Filetime { value, .. }) => {
            match filetime_rfc3339(*value) {
                Some(text) => {
                    ctx.charge_retained(
                        cadmpeg_core::decode::u64_from_index(text.len()),
                        "retain Inventor metadata value",
                    )?;
                    Ok(Some(text))
                }
                None => Ok(None),
            }
        }
        _ => Ok(None),
    }
}

/// Writes a nonzero FILETIME (100 ns ticks since 1601-01-01 UTC) as RFC 3339 UTC.
///
/// Zero is the unset FILETIME and years after 9999 have no RFC 3339 form; both yield `None`.
pub(super) fn filetime_rfc3339(ticks: u64) -> Option<String> {
    if ticks == 0 {
        return None;
    }
    let fraction = ticks % 10_000_000;
    let seconds = ticks / 10_000_000;
    let days = i64::try_from(seconds / 86_400).ok()?;
    let second_of_day = seconds % 86_400;
    // Days from 1601-01-01 to 1970-01-01.
    let (year, month, day) = civil_from_days(days.checked_sub(134_774)?)?;
    if year > 9999 {
        return None;
    }
    let mut text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        second_of_day / 3600,
        second_of_day / 60 % 60,
        second_of_day % 60
    );
    if fraction != 0 {
        let digits = format!("{fraction:07}");
        text.push('.');
        text.push_str(digits.trim_end_matches('0'));
    }
    text.push('Z');
    Some(text)
}

/// Proleptic Gregorian date of a day count relative to 1970-01-01.
fn civil_from_days(days: i64) -> Option<(i64, u32, u32)> {
    let shifted = days.checked_add(719_468)?;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * month_index + 2) / 5 + 1).ok()?;
    let month = u32::try_from(if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    })
    .ok()?;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    Some((year, month, day))
}
