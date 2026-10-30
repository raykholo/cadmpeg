// SPDX-License-Identifier: Apache-2.0
//! Parse typed Design body-presentation and browser-node records.

use crate::bytes::lp_utf16_bounded;
use cadmpeg_core::decode::u64_from_index;

use std::collections::HashMap;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::bytes::{is_guid_prefix, lp_utf16_bytes, take_reference};
use crate::design::decode::meta::typed_primary_frames;
use crate::design::decode::sketch::{
    parse_genesis_entity_header, parse_settled_entity_header, NamedEntityHeader,
};

use crate::design::presentation::{
    is_physical_material_token, APPEARANCE_LIBRARY_ID, BODY_PRESENTATION_BASE_TYPE_GUID,
    BODY_PRESENTATION_MATERIAL_ENVELOPE_ID, BODY_PRESENTATION_TYPE_GUID,
    BODY_PRESENTATION_TYPE_VERSION, BODY_PRESENTATION_TYPE_VERSION_LEGACY_TAIL,
    BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION, BREP_CONTAINER_TYPE_GUID,
    BREP_CONTAINER_TYPE_VERSION, BROWSER_NODE_BASE_TYPE_GUID, BROWSER_NODE_TYPE_GUID,
    BROWSER_NODE_TYPE_VERSION, GUID_LEN, MODERN_APPEARANCE_LIBRARY_IDS,
    PHYSICAL_MATERIAL_LIBRARY_ID,
};
use crate::records::entity_header::{DESIGN_MODULE_BODY, DESIGN_MODULE_FUSION};

const MAX_ENVELOPE_GAP: usize = 8;

/// One typed browser-node record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserNodeRecord {
    pub(super) record_index: u32,
    guid: String,
    pub(super) entity_suffix: u64,
    pub(super) hidden_offset: u64,
    pub(super) hidden: bool,
}

/// Material members of one body-presentation envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PresentationMaterial {
    node_guid: String,
    pub(crate) body_name: Option<String>,
    pub(crate) physical_token: String,
    pub(crate) physical_token_offset: u64,
    pub(crate) visual_guid: crate::records::references::DesignVisualToken,
    pub(crate) visual_guid_offset: u64,
    pub(crate) visual_preset: Option<crate::records::identity::Located<String>>,
}

/// One body record, its exact owner header, and its browser-node join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BodyPresentation {
    pub(crate) byte_offset: u64,
    pub(crate) entity_suffix: u64,
    pub(crate) name: Option<String>,
    pub(crate) owner: BodyPresentationOwner,
    pub(crate) browser_node: Option<BrowserNodeRecord>,
    pub(crate) material: Option<PresentationMaterial>,
    /// Set when a legacy-tail (type version 18) record binds no material.
    pub(crate) legacy_tail_refusal: Option<LegacyTailRefusal>,
    /// Whether the record has the legacy-tail form (type version 18).
    pub(crate) legacy_tail: bool,
}

/// Why a legacy-tail body-presentation record carries no usable material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LegacyTailRefusal {
    /// The physical-material envelope or the colour and library tail does not
    /// match the legacy-tail grammar.
    EnvelopeUnparsed,
    /// The scene-node reference is absent, null, or does not name entity `E + 1`.
    SceneReferenceMissing,
    /// A reference names an entity of an unexpected registered type.
    SceneTypeMismatch,
    /// The record ends before the colour and library tail is complete.
    TailTruncated,
    /// The stream registers browser nodes and none joins the record's node GUID.
    BrowserNodeUnjoined,
}

impl LegacyTailRefusal {
    /// Stable kebab-case reason used in loss messages.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::EnvelopeUnparsed => "envelope-unparsed",
            Self::SceneReferenceMissing => "scene-reference-missing",
            Self::SceneTypeMismatch => "scene-type-mismatch",
            Self::TailTruncated => "tail-truncated",
            Self::BrowserNodeUnjoined => "browser-node-unjoined",
        }
    }
}

/// Entity identity form stored by one body-presentation owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BodyPresentationOwner {
    /// The owner stores a component-qualified entity ID after its entity
    /// suffix.
    Named {
        entity_id: crate::records::identity::DesignEntityId,
        entity_id_offset: u64,
    },
    /// The owner stores only its u64 entity suffix in the indexed head.
    Bare,
}

/// Decode every browser node whose dynamic class resolves to the registered
/// browser-node type. The record body is ten zero bytes, an LP-UTF16 GUID, the
/// hidden flag, `01 01`, and the owning Design entity suffix.
pub(super) fn browser_node_records(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BrowserNodeRecord>, CodecError> {
    let mut out = Vec::new();
    for frame in typed_primary_frames(bytes, meta, BROWSER_NODE_TYPE_GUID, "browser-node")? {
        if frame.design_type.version != BROWSER_NODE_TYPE_VERSION {
            continue;
        }
        if frame.design_type.module != DESIGN_MODULE_FUSION
            || !frame
                .design_type
                .base_type_guid
                .value()
                .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                .is_some_and(|base| base.eq_ignore_ascii_case(BROWSER_NODE_BASE_TYPE_GUID))
        {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D Design browser-node entity {} has incompatible registration metadata",
                frame.entity_id
            )));
        }
        let record = &bytes[frame.start..frame.end];
        let record_index = View::u32_le_at(record, 7).ok_or_else(|| {
            crate::design::text::malformed_design(format_args!(
                "F3D Design browser-node entity {} has a truncated record index",
                frame.entity_id
            ))
        })?;
        if u64::from(record_index) != frame.entity_id || record.get(11..21) != Some(&[0; 10]) {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D Design browser-node entity {} has an invalid header",
                frame.entity_id
            )));
        }
        let Some((guid, after_guid)) =
            lp_utf16_bounded(record, 21, GUID_LEN..=GUID_LEN).filter(|(guid, after)| {
                is_guid_prefix(guid)
                    && after
                        .checked_add(11)
                        .is_some_and(|record_end| record_end <= record.len())
                    && record.get(*after + 1..*after + 3) == Some(&[0x01, 0x01])
            })
        else {
            continue;
        };
        let hidden @ (0 | 1) = record.get(after_guid).copied().ok_or_else(|| {
            CodecError::Malformed("F3D Design browser-node flag is truncated".into())
        })?
        else {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D Design browser-node entity {} has an invalid hidden flag",
                frame.entity_id
            )));
        };
        let entity_suffix = View::u64_le_at(record, after_guid + 3).ok_or_else(|| {
            CodecError::Malformed("F3D Design browser-node suffix is truncated".into())
        })?;

        out.push(BrowserNodeRecord {
            record_index,
            guid,
            entity_suffix,
            hidden_offset: u64_from_index(frame.start + after_guid),
            hidden: hidden == 1,
        });
    }
    Ok(out)
}

/// Decode every typed body presentation in one Design stream.
pub(crate) fn body_presentations(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyPresentation>, CodecError> {
    let nodes = browser_node_records(bytes, meta)?;
    let entity_types = entity_types(meta)?;

    let mut out = Vec::new();
    for frame in typed_primary_frames(
        bytes,
        meta,
        BODY_PRESENTATION_TYPE_GUID,
        "body-presentation",
    )? {
        let legacy_tail = frame.design_type.version == BODY_PRESENTATION_TYPE_VERSION_LEGACY_TAIL;
        if frame.design_type.version != BODY_PRESENTATION_TYPE_VERSION && !legacy_tail {
            continue;
        }
        if frame.design_type.module != DESIGN_MODULE_BODY
            || !frame
                .design_type
                .base_type_guid
                .value()
                .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                .is_some_and(|base| base.eq_ignore_ascii_case(BODY_PRESENTATION_BASE_TYPE_GUID))
        {
            return Err(crate::design::text::malformed_design(format_args!(
                "F3D Design body-presentation entity {} has incompatible registration metadata",
                frame.entity_id
            )));
        }
        let framed_bytes = &bytes[..frame.end];
        if legacy_tail {
            out.push(legacy_tail_presentation(
                framed_bytes,
                &frame,
                &nodes,
                &entity_types,
            )?);
            continue;
        }
        let named_header = match parse_settled_entity_header(framed_bytes, frame.start) {
            Some(header) => Some(header),
            None => parse_genesis_entity_header(framed_bytes, frame.start),
        };
        let (entity_suffix, name, owner, material) = if let Some(NamedEntityHeader {
            entity_id,
            entity_id_offset,
            end: header_end,
            ..
        }) = named_header
        {
            let entity_suffix = entity_id.suffix();
            if entity_suffix != frame.entity_id {
                return Err(crate::design::text::malformed_design(format_args!(
                    "F3D Design body-presentation entity {} disagrees with its named header entity {entity_suffix}",
                    frame.entity_id
                )));
            }
            let material = presentation_material(
                framed_bytes,
                header_end,
                frame.end,
                entity_suffix,
                &entity_types,
            )?;
            let name = material.as_ref().and_then(|m| m.body_name.clone());
            (
                entity_suffix,
                name,
                BodyPresentationOwner::Named {
                    entity_id,
                    entity_id_offset: u64_from_index(entity_id_offset),
                },
                material,
            )
        } else {
            let entity_suffix =
                View::u64_le_at(framed_bytes, frame.start + 7).ok_or_else(|| {
                    crate::design::text::malformed_design(format_args!(
                        "F3D Design bare body-presentation entity {} has a truncated head",
                        frame.entity_id
                    ))
                })?;
            if entity_suffix == 0 || entity_suffix != frame.entity_id {
                return Err(crate::design::text::malformed_design(format_args!(
                    "F3D Design bare body-presentation entity {} has head entity {entity_suffix}",
                    frame.entity_id
                )));
            }
            let Some(material) = bare_presentation_material(
                framed_bytes,
                frame.start + 15,
                frame.end,
                entity_suffix,
            )?
            else {
                continue;
            };
            let name = material.body_name.clone();
            (
                entity_suffix,
                name,
                BodyPresentationOwner::Bare,
                Some(material),
            )
        };
        let browser_node = if let Some(material) = material.as_ref() {
            let mut matching_nodes = nodes.iter().filter(|node| {
                node.entity_suffix == entity_suffix
                    && node.guid.eq_ignore_ascii_case(&material.node_guid)
            });
            match (matching_nodes.next(), matching_nodes.next()) {
                (Some(node), None) => Some(node.clone()),
                _ => None,
            }
        } else {
            None
        };
        out.push(BodyPresentation {
            byte_offset: u64_from_index(frame.start),
            entity_suffix,
            name,
            owner,
            browser_node,
            material,
            legacy_tail_refusal: None,
            legacy_tail: false,
        });
    }
    Ok(out)
}

/// Bytes between the name field and the colour that are zero padding.
const LEGACY_TAIL_COLOR_GAP_MAX: usize = MAX_ENVELOPE_GAP;
/// Bytes of the RGBA colour carrier: four f32 components.
const LEGACY_TAIL_COLOR_BYTES: usize = 16;
/// Bytes of the `01 01`, nine-zero, selector-form carrier after the colour.
const LEGACY_TAIL_CARRIER_BYTES: usize = 12;
/// Bytes of one marked local reference.
const LOCAL_REFERENCE_BYTES: usize = 11;

/// Decode one legacy-tail (type version 18) body-presentation record.
///
/// The head is bare: a u64 entity suffix follows the class tag. A record that
/// does not satisfy the grammar is kept with its refusal.
fn legacy_tail_presentation(
    bytes: &[u8],
    frame: &crate::design::decode::meta::TypedPrimaryFrame<'_>,
    nodes: &[BrowserNodeRecord],
    entity_types: &HashMap<u64, (&str, u32)>,
) -> Result<BodyPresentation, CodecError> {
    let entity_suffix = View::u64_le_at(bytes, frame.start + 7).ok_or_else(|| {
        crate::design::text::malformed_design(format_args!(
            "F3D Design legacy-tail body-presentation entity {} has a truncated head",
            frame.entity_id
        ))
    })?;
    if entity_suffix == 0 || entity_suffix != frame.entity_id {
        return Err(crate::design::text::malformed_design(format_args!(
            "F3D Design legacy-tail body-presentation entity {} has head entity {entity_suffix}",
            frame.entity_id
        )));
    }
    let (material, mut refusal) = match legacy_tail_material(
        bytes,
        frame.start + 15,
        frame.end,
        entity_suffix,
        entity_types,
    ) {
        Ok(material) => (Some(material), None),
        Err(refusal) => (None, Some(refusal)),
    };
    let mut browser_node = None;
    if let Some(material) = material.as_ref() {
        let mut matching = nodes.iter().filter(|node| {
            node.entity_suffix == entity_suffix
                && node.guid.eq_ignore_ascii_case(&material.node_guid)
        });
        browser_node = match (matching.next(), matching.next()) {
            (Some(node), None) => Some(node.clone()),
            _ => None,
        };
        // A stream that registers no browser node joins by entity suffix.
        if browser_node.is_none() && !nodes.is_empty() {
            refusal = Some(LegacyTailRefusal::BrowserNodeUnjoined);
        }
    }
    let name = material.as_ref().and_then(|m| m.body_name.clone());
    Ok(BodyPresentation {
        byte_offset: u64_from_index(frame.start),
        entity_suffix,
        name,
        owner: BodyPresentationOwner::Bare,
        browser_node,
        material,
        legacy_tail_refusal: refusal,
        legacy_tail: true,
    })
}

/// Counted UTF-16 read inside `[at, end)` that separates a short record from
/// malformed text.
fn legacy_tail_lp(
    bytes: &[u8],
    at: usize,
    end: usize,
    bounds: std::ops::RangeInclusive<usize>,
) -> Result<(String, usize), LegacyTailRefusal> {
    let count = at
        .checked_add(4)
        .filter(|after| *after <= end)
        .and_then(|_| View::u32_le_at(bytes, at))
        .ok_or(LegacyTailRefusal::TailTruncated)?;
    let count = usize::try_from(count).map_err(|_| LegacyTailRefusal::EnvelopeUnparsed)?;
    if !bounds.contains(&count) {
        return Err(LegacyTailRefusal::EnvelopeUnparsed);
    }
    let after = count
        .checked_mul(2)
        .and_then(|width| (at + 4).checked_add(width))
        .ok_or(LegacyTailRefusal::EnvelopeUnparsed)?;
    if after > end {
        return Err(LegacyTailRefusal::TailTruncated);
    }
    lp_utf16_bounded(bytes, at, bounds).ok_or(LegacyTailRefusal::EnvelopeUnparsed)
}

/// Parse the material envelope of a legacy-tail body-presentation record.
///
/// After the physical-material token: the BREP-container reference, a null
/// reference, an optional counted-UTF-16 scene GUID, the reference to entity
/// `E + 1`, a counted body name, zero padding, the RGBA colour with alpha one,
/// the `01 01` carrier, the visual token, the appearance-library id, an
/// optional display name, the selector name, and f32 values zero and one.
fn legacy_tail_material(
    bytes: &[u8],
    start: usize,
    end: usize,
    entity_suffix: u64,
    entity_types: &HashMap<u64, (&str, u32)>,
) -> Result<PresentationMaterial, LegacyTailRefusal> {
    use LegacyTailRefusal::{
        EnvelopeUnparsed, SceneReferenceMissing, SceneTypeMismatch, TailTruncated,
    };
    let bytes = bytes.get(..end).ok_or(EnvelopeUnparsed)?;
    let physical_marker =
        lp_utf16_bytes(PHYSICAL_MATERIAL_LIBRARY_ID).map_err(|_| EnvelopeUnparsed)?;
    let library_marker = lp_utf16_bytes(APPEARANCE_LIBRARY_ID).map_err(|_| EnvelopeUnparsed)?;
    let mut markers = find_all(bytes, start, end, &physical_marker);
    let (Some(physical_at), None) = (markers.next(), markers.next()) else {
        return Err(EnvelopeUnparsed);
    };
    let (physical_guid_at, physical_guid) =
        preceding_lp_utf16(bytes, start, physical_at).ok_or(EnvelopeUnparsed)?;
    let node_tail_at = physical_guid_at.checked_sub(11).ok_or(EnvelopeUnparsed)?;
    if bytes.get(node_tail_at..node_tail_at + 11) != Some(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]) {
        return Err(EnvelopeUnparsed);
    }
    let (_, node_guid) = preceding_lp_utf16(bytes, start, node_tail_at).ok_or(EnvelopeUnparsed)?;
    if physical_guid.len() != GUID_LEN
        || node_guid.len() != GUID_LEN
        || !is_guid_prefix(&physical_guid)
        || !is_guid_prefix(&node_guid)
    {
        return Err(EnvelopeUnparsed);
    }
    // An optional counted display name of the physical material precedes the
    // `PrismMaterial-N` token.
    let mut token_at =
        skip_zeros(bytes, physical_at + physical_marker.len(), end).ok_or(EnvelopeUnparsed)?;
    let (mut physical_token, mut after_token) = legacy_tail_lp(bytes, token_at, end, 1..=256)?;
    if !is_physical_material_token(&physical_token) {
        token_at = after_token;
        (physical_token, after_token) = legacy_tail_lp(bytes, token_at, end, 1..=256)?;
        if !is_physical_material_token(&physical_token) {
            return Err(EnvelopeUnparsed);
        }
    }

    let mut at = after_token;
    let container = local_reference(bytes, &mut at).ok_or(EnvelopeUnparsed)?;
    if local_reference_value(bytes, &mut at) != Some(LocalReference::Null) {
        return Err(EnvelopeUnparsed);
    }
    if View::u32_le_at(bytes, at) == u32::try_from(GUID_LEN).ok() {
        let (scene_guid, after_guid) = legacy_tail_lp(bytes, at, end, GUID_LEN..=GUID_LEN)?;
        if !is_guid_prefix(&scene_guid) {
            return Err(EnvelopeUnparsed);
        }
        at = after_guid;
    }
    if end.saturating_sub(at) < LOCAL_REFERENCE_BYTES {
        return Err(TailTruncated);
    }
    let scene = local_reference(bytes, &mut at).ok_or(SceneReferenceMissing)?;
    if entity_suffix.checked_add(1) != Some(scene) {
        return Err(SceneReferenceMissing);
    }
    let type_is = |entity: u64, guid: &str, version: u32| {
        entity_types
            .get(&entity)
            .is_some_and(|(found, found_version)| {
                found.eq_ignore_ascii_case(guid) && *found_version == version
            })
    };
    if !type_is(
        container,
        BREP_CONTAINER_TYPE_GUID,
        BREP_CONTAINER_TYPE_VERSION,
    ) || !type_is(
        scene,
        BODY_SCENE_NODE_TYPE_GUID,
        BODY_SCENE_NODE_TYPE_VERSION,
    ) {
        return Err(SceneTypeMismatch);
    }

    // Counted body name; a zero count is an unnamed body.
    let name_count = View::u32_le_at(bytes, at).ok_or(TailTruncated)?;
    let body_name = if name_count == 0 {
        at += 4;
        None
    } else {
        let (name, after_name) = legacy_tail_lp(bytes, at, end, 1..=256)?;
        at = after_name;
        Some(name)
    };
    // Zero padding, then the colour and carrier.
    let mut color_at = None;
    for gap in 0..=LEGACY_TAIL_COLOR_GAP_MAX {
        if bytes
            .get(at..at + gap)
            .is_none_or(|pad| pad.iter().any(|byte| *byte != 0))
        {
            break;
        }
        let candidate = at + gap;
        let carrier_at = candidate + LEGACY_TAIL_COLOR_BYTES;
        if crate::materials::normalized_legacy_face_color(bytes, candidate).is_some()
            && crate::materials::legacy_face_selector_kind(
                bytes.get(carrier_at..carrier_at + LEGACY_TAIL_CARRIER_BYTES),
            )
            .is_some()
        {
            color_at = Some(candidate);
            break;
        }
    }
    let Some(color_at) = color_at else {
        return Err(
            if end.saturating_sub(at) < LEGACY_TAIL_COLOR_BYTES + LEGACY_TAIL_CARRIER_BYTES {
                TailTruncated
            } else {
                EnvelopeUnparsed
            },
        );
    };
    let carrier_at = color_at + LEGACY_TAIL_COLOR_BYTES;
    let selector_kind = crate::materials::legacy_face_selector_kind(
        bytes.get(carrier_at..carrier_at + LEGACY_TAIL_CARRIER_BYTES),
    )
    .ok_or(EnvelopeUnparsed)?;
    let visual_at = carrier_at + LEGACY_TAIL_CARRIER_BYTES;
    let (visual_text, after_visual) = legacy_tail_lp(bytes, visual_at, end, 1..=256)?;
    let visual_guid = crate::records::references::DesignVisualToken::try_from(visual_text)
        .map_err(|_| EnvelopeUnparsed)?;
    if bytes.get(after_visual..after_visual + library_marker.len())
        != Some(library_marker.as_slice())
    {
        return Err(if after_visual + library_marker.len() > end {
            TailTruncated
        } else {
            EnvelopeUnparsed
        });
    }
    let mut at = after_visual + library_marker.len();
    let display_count = View::u32_le_at(bytes, at).ok_or(TailTruncated)?;
    if display_count == 0 {
        at += 4;
    } else {
        let (display, after_display) = legacy_tail_lp(bytes, at, end, 1..=256)?;
        if display.chars().any(char::is_control) {
            return Err(EnvelopeUnparsed);
        }
        at = after_display;
    }
    let selector_at = at;
    let (selector, after_selector) = legacy_tail_lp(bytes, selector_at, end, 1..=256)?;
    if !crate::materials::legacy_face_selector_is_valid(selector_kind, &selector) {
        return Err(EnvelopeUnparsed);
    }
    let zero = View::f32_le_at(bytes, after_selector).ok_or(TailTruncated)?;
    let one = View::f32_le_at(bytes, after_selector + 4).ok_or(TailTruncated)?;
    if zero.to_le_bytes() != 0.0f32.to_le_bytes() || one.to_le_bytes() != 1.0f32.to_le_bytes() {
        return Err(EnvelopeUnparsed);
    }
    let visual_preset = selector
        .starts_with("Prism-")
        .then(|| crate::records::identity::Located {
            value: selector,
            offset: u64_from_index(selector_at + 4),
        });
    Ok(PresentationMaterial {
        node_guid,
        body_name,
        physical_token,
        physical_token_offset: u64_from_index(token_at + 4),
        visual_guid,
        visual_guid_offset: u64_from_index(visual_at + 4),
        visual_preset,
    })
}

fn entity_types(
    meta: &crate::metastream::MetaStream,
) -> Result<HashMap<u64, (&str, u32)>, CodecError> {
    let mut out = HashMap::new();
    for design_type in &meta.types {
        for &entity_id in design_type.entities.values() {
            if out.contains_key(&entity_id) {
                return Err(crate::design::text::malformed_design(format_args!(
                    "F3D Design entity {entity_id} has multiple registered types"
                )));
            }

            out.insert(
                entity_id,
                (design_type.type_guid.as_str(), design_type.version),
            );
        }
    }
    Ok(out)
}

fn presentation_material(
    bytes: &[u8],
    start: usize,
    end: usize,
    entity_suffix: u64,
    entity_types: &HashMap<u64, (&str, u32)>,
) -> Result<Option<PresentationMaterial>, CodecError> {
    let Some(bytes) = bytes.get(..end) else {
        return Ok(None);
    };
    let physical_marker = lp_utf16_bytes(PHYSICAL_MATERIAL_LIBRARY_ID)?;
    let legacy_marker = lp_utf16_bytes(APPEARANCE_LIBRARY_ID)?;
    let modern_marker = lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[0])?;
    let modern_trailer = lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[1])?;
    let mut candidate = None;
    for physical_at in find_all(bytes, start, end, &physical_marker) {
        let Some((physical_guid_at, physical_guid)) = preceding_lp_utf16(bytes, start, physical_at)
        else {
            continue;
        };
        let Some(node_tail_at) = physical_guid_at.checked_sub(11) else {
            continue;
        };
        if bytes.get(node_tail_at..node_tail_at + 11) != Some(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]) {
            continue;
        }
        let Some((_, node_guid)) = preceding_lp_utf16(bytes, start, node_tail_at) else {
            continue;
        };
        if physical_guid.len() != GUID_LEN
            || node_guid.len() != GUID_LEN
            || !is_guid_prefix(&physical_guid)
            || !is_guid_prefix(&node_guid)
        {
            continue;
        }
        let Some(token_at) = skip_zeros(bytes, physical_at + physical_marker.len(), end) else {
            continue;
        };
        let Some((physical_token, after_token)) = lp_utf16_bounded(bytes, token_at, 1..=256) else {
            continue;
        };
        if !is_physical_material_token(&physical_token) || after_token > end {
            continue;
        }
        let mut reference_at = after_token;
        let Some(brep_container_entity) = local_reference(bytes, &mut reference_at) else {
            continue;
        };
        if local_reference_value(bytes, &mut reference_at) != Some(LocalReference::Null) {
            continue;
        }
        let Some(scene_node_entity) = local_reference(bytes, &mut reference_at) else {
            continue;
        };
        if entity_types
            .get(&brep_container_entity)
            .is_none_or(|(guid, version)| {
                !guid.eq_ignore_ascii_case(BREP_CONTAINER_TYPE_GUID)
                    || *version != BREP_CONTAINER_TYPE_VERSION
            })
            || entity_suffix.checked_add(1) != Some(scene_node_entity)
            || entity_types
                .get(&scene_node_entity)
                .is_none_or(|(guid, version)| {
                    !guid.eq_ignore_ascii_case(BODY_SCENE_NODE_TYPE_GUID)
                        || *version != BODY_SCENE_NODE_TYPE_VERSION
                })
        {
            continue;
        }
        let Some((body_name, after_name)) = lp_utf16_bounded(bytes, reference_at, 0..=256) else {
            continue;
        };
        let Some(visual_at) = record_tail_visual_offset(bytes, after_name, end) else {
            continue;
        };
        let Some((visual_guid, after_visual)) = lp_utf16_bounded(bytes, visual_at, 1..=256) else {
            continue;
        };
        let Ok(visual_guid) = crate::records::references::DesignVisualToken::try_from(visual_guid)
        else {
            continue;
        };
        let Some(visual_marker_at) = skip_zeros(bytes, after_visual, end) else {
            continue;
        };
        let (after_visual_marker, legacy) = if bytes
            .get(visual_marker_at..visual_marker_at + legacy_marker.len())
            == Some(legacy_marker.as_slice())
        {
            (visual_marker_at + legacy_marker.len(), true)
        } else if bytes.get(visual_marker_at..visual_marker_at + modern_marker.len())
            == Some(modern_marker.as_slice())
        {
            let Some(trailer_at) = skip_zeros(bytes, visual_marker_at + modern_marker.len(), end)
            else {
                continue;
            };
            if bytes.get(trailer_at..trailer_at + modern_trailer.len())
                != Some(modern_trailer.as_slice())
            {
                continue;
            }
            (trailer_at + modern_trailer.len(), false)
        } else {
            continue;
        };
        let visual_preset = if legacy {
            if let Some(at) = skip_zeros(bytes, after_visual_marker, end) {
                lp_utf16_bounded(bytes, at, 1..=256)
                    .and_then(|(value, _)| value.starts_with("Prism-").then_some((at, value)))
            } else {
                None
            }
        } else {
            None
        };
        if candidate.is_some() {
            return Ok(None);
        }
        candidate = Some(PresentationMaterial {
            node_guid,
            body_name: (!body_name.is_empty()).then_some(body_name),
            physical_token,
            physical_token_offset: u64_from_index(token_at + 4),
            visual_guid,
            visual_guid_offset: u64_from_index(visual_at + 4),
            visual_preset: visual_preset.map(|(at, value)| crate::records::identity::Located {
                value,
                offset: u64_from_index(at + 4),
            }),
        });
    }
    Ok(candidate)
}

/// Parse the material envelope of a body-presentation owner whose indexed
/// head stores no component-qualified entity ID.
fn bare_presentation_material(
    bytes: &[u8],
    start: usize,
    end: usize,
    entity_suffix: u64,
) -> Result<Option<PresentationMaterial>, CodecError> {
    let Some(bytes) = bytes.get(..end) else {
        return Ok(None);
    };
    let marker = lp_utf16_bytes(BODY_PRESENTATION_MATERIAL_ENVELOPE_ID)?
        .into_iter()
        .chain(lp_utf16_bytes(PHYSICAL_MATERIAL_LIBRARY_ID)?)
        .collect::<Vec<_>>();
    let modern_marker = lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[0])?;
    let modern_trailer = lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[1])?;
    let mut candidate = None;
    for marker_at in find_all(bytes, start, end, &marker) {
        let Some(token_at) = skip_zeros(bytes, marker_at + marker.len(), end) else {
            continue;
        };
        let Some((physical_token, after_token)) = lp_utf16_bounded(bytes, token_at, 1..=256) else {
            continue;
        };
        if !is_physical_material_token(&physical_token) {
            continue;
        }

        let mut physical_reference_at = after_token;
        if local_reference(bytes, &mut physical_reference_at).is_none() {
            continue;
        }
        let Some(node_guid_at) = skip_zeros(bytes, physical_reference_at, end) else {
            continue;
        };
        let Some((node_guid, after_node_guid)) =
            lp_utf16_bounded(bytes, node_guid_at, GUID_LEN..=GUID_LEN)
        else {
            continue;
        };
        if !is_guid_prefix(&node_guid) {
            continue;
        }
        let mut node_reference_at = after_node_guid;
        let Some(node_entity) = local_reference(bytes, &mut node_reference_at) else {
            continue;
        };
        if entity_suffix.checked_add(1) != Some(node_entity) {
            continue;
        }

        let mut body_name = None;
        let mut after_name = None;
        if let Some(name_at) = skip_zeros(bytes, node_reference_at, end) {
            if let Some((name, end)) = lp_utf16_bounded(bytes, name_at, 1..=256) {
                body_name = Some(name);
                after_name = Some(end);
            }
        }
        let mut visual_offsets = [Some(node_reference_at), after_name]
            .into_iter()
            .flatten()
            .filter_map(|name_end| record_tail_visual_offset(bytes, name_end, end));
        let Some(visual_at) = visual_offsets.next() else {
            continue;
        };
        if visual_offsets.any(|offset| offset != visual_at) {
            continue;
        }
        let Some((visual_guid, after_visual)) = lp_utf16_bounded(bytes, visual_at, 1..=256) else {
            continue;
        };
        let Ok(visual_guid) = crate::records::references::DesignVisualToken::try_from(visual_guid)
        else {
            continue;
        };
        let Some(marker_at) = skip_zeros(bytes, after_visual, end) else {
            continue;
        };
        if bytes.get(marker_at..marker_at + modern_marker.len()) != Some(modern_marker.as_slice()) {
            continue;
        }
        let Some(trailer_at) = skip_zeros(bytes, marker_at + modern_marker.len(), end) else {
            continue;
        };
        if bytes.get(trailer_at..trailer_at + modern_trailer.len())
            != Some(modern_trailer.as_slice())
        {
            continue;
        }
        if candidate.is_some() {
            return Ok(None);
        }
        candidate = Some(PresentationMaterial {
            node_guid,
            body_name,
            physical_token,
            physical_token_offset: u64_from_index(token_at + 4),
            visual_guid,
            visual_guid_offset: u64_from_index(visual_at + 4),
            visual_preset: None,
        });
    }
    Ok(candidate)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalReference {
    Null,
    Target(u64),
}

fn local_reference_value(bytes: &[u8], at: &mut usize) -> Option<LocalReference> {
    let reference = take_reference(bytes, at)?;
    match reference {
        crate::bytes::Reference::Null => Some(LocalReference::Null),
        crate::bytes::Reference::Local { target, .. } => Some(LocalReference::Target(target)),
        _ => None,
    }
}

fn local_reference(bytes: &[u8], at: &mut usize) -> Option<u64> {
    match local_reference_value(bytes, at)? {
        LocalReference::Target(target) if target != 0 => Some(target),
        LocalReference::Null | LocalReference::Target(_) => None,
    }
}

fn record_tail_visual_offset(bytes: &[u8], name_end: usize, end: usize) -> Option<usize> {
    const OPACITY_ONE: [u8; 4] = 1.0f32.to_le_bytes();
    for marker_at in (name_end..end).take(40) {
        if bytes.get(marker_at..marker_at + 2) != Some(&[0x01, 0x01]) {
            continue;
        }
        let gap = bytes.get(name_end..marker_at)?;
        let zeros_only = gap.iter().all(|byte| *byte == 0);
        let opacity_tail = gap.len() >= OPACITY_ONE.len()
            && gap[gap.len() - OPACITY_ONE.len()..] == OPACITY_ONE
            && gap[..gap.len() - OPACITY_ONE.len()]
                .iter()
                .all(|byte| *byte == 0);
        if zeros_only || opacity_tail {
            return skip_zeros_capped(bytes, marker_at + 2, end, 12);
        }
    }
    None
}

fn find_all<'a>(
    bytes: &'a [u8],
    start: usize,
    end: usize,
    needle: &'a [u8],
) -> impl Iterator<Item = usize> + 'a {
    let range = bytes.get(start..end).unwrap_or_default();
    memchr::memmem::find_iter(range, needle).map(move |offset| start + offset)
}

fn skip_zeros(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    skip_zeros_capped(bytes, start, end, MAX_ENVELOPE_GAP)
}

fn skip_zeros_capped(bytes: &[u8], start: usize, end: usize, cap: usize) -> Option<usize> {
    let mut at = start;
    while at < end && at - start < cap && bytes.get(at) == Some(&0) {
        at += 1;
    }
    (at <= end && (at == end || bytes.get(at) != Some(&0))).then_some(at)
}

fn preceding_lp_utf16(bytes: &[u8], start: usize, marker_at: usize) -> Option<(usize, String)> {
    let mut candidate = None;
    for gap in 0..=MAX_ENVELOPE_GAP {
        let Some(end) = marker_at.checked_sub(gap) else {
            continue;
        };
        if end < start
            || bytes
                .get(end..marker_at)
                .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
        {
            continue;
        }
        for at in (start..end).rev().take(4 + 256 * 2).rev() {
            let Some(count) =
                View::u32_le_at(bytes, at).and_then(|count| usize::try_from(count).ok())
            else {
                continue;
            };
            if !(1..=256).contains(&count)
                || at.checked_add(4).and_then(|start| {
                    count
                        .checked_mul(2)
                        .and_then(|bytes| start.checked_add(bytes))
                }) != Some(end)
            {
                continue;
            }
            let Some((value, _)) = lp_utf16_bounded(bytes, at, 1..=256) else {
                continue;
            };
            if candidate.is_some() {
                return None;
            }
            candidate = Some((at, value));
        }
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::{body_presentations, browser_node_records, BodyPresentationOwner};
    use crate::bytes::lp_utf16_bytes;
    use crate::design::presentation::{
        APPEARANCE_LIBRARY_ID, BODY_PRESENTATION_BASE_TYPE_GUID,
        BODY_PRESENTATION_MATERIAL_ENVELOPE_ID, BODY_PRESENTATION_TYPE_GUID,
        BODY_PRESENTATION_TYPE_VERSION, BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION,
        BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION, BROWSER_NODE_BASE_TYPE_GUID,
        BROWSER_NODE_TYPE_GUID, BROWSER_NODE_TYPE_VERSION, MODERN_APPEARANCE_LIBRARY_IDS,
        PHYSICAL_MATERIAL_LIBRARY_ID,
    };
    use crate::design::test_support::{design_type, primary_record};
    use crate::records::entity_header::{DESIGN_MODULE_BODY, DESIGN_MODULE_FUSION};
    use crate::test_support::{lp_ascii, lp_utf16, push_reference_u64};

    fn bare_presentation_material(
        bytes: &[u8],
        start: usize,
        end: usize,
        entity_suffix: u64,
    ) -> Option<super::PresentationMaterial> {
        super::bare_presentation_material(bytes, start, end, entity_suffix).unwrap()
    }

    #[test]
    fn presentation_utf16_text_decodes_exact_text() {
        let text = "A雪𐍈";
        let mut bytes = Vec::new();
        lp_utf16(&mut bytes, text);
        let decoded = crate::bytes::lp_utf16_bounded(&bytes, 0, 1..=256).unwrap();
        assert_eq!(decoded.0, text);
        assert_eq!(decoded.1, bytes.len());

        let invalid = [1, 0, 0, 0, 0, 0xd8];
        assert!(crate::bytes::lp_utf16_bounded(&invalid, 0, 1..=256).is_none());
    }

    #[test]
    fn browser_node_records_decode_one_node() {
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "256");
        bytes.extend_from_slice(&43_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, "11111111-2222-8333-A444-555555555555");
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&42_u64.to_le_bytes());
        let meta = crate::metastream::MetaStream {
            types: vec![design_type(
                BROWSER_NODE_TYPE_GUID,
                Some(BROWSER_NODE_BASE_TYPE_GUID),
                BROWSER_NODE_TYPE_VERSION,
                DESIGN_MODULE_FUSION,
                vec![43],
            )],
            records: vec![primary_record(43, 0)],
            secondary_records: Vec::new(),
        };
        let nodes = browser_node_records(&bytes, &meta).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].entity_suffix, 42);
    }

    #[test]
    fn typed_presentation_joins_its_exact_browser_node() {
        let body_tag = 256u32;
        let node_tag = 257u32;
        let entity = (1u64 << 40) + 42;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, &format!("0_{entity}"));
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        lp_utf16(&mut bytes, "PrismMaterial-001");
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        lp_utf16(&mut bytes, "Body");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        lp_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            lp_utf16(&mut bytes, marker);
        }
        let node_start = bytes.len();
        lp_ascii(&mut bytes, &node_tag.to_string());
        bytes.extend_from_slice(&43u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![43],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0), primary_record(43, node_start)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("typed primary frames");
        assert_eq!(presentations.len(), 1);
        let presentation = &presentations[0];
        assert_eq!(presentation.entity_suffix, entity);
        assert_eq!(
            presentation.owner,
            BodyPresentationOwner::Named {
                entity_id: crate::records::identity::DesignEntityId::from_parts("0", entity),
                entity_id_offset: 25,
            }
        );
        assert_eq!(presentation.browser_node.as_ref().unwrap().guid, node_guid);
        let material = presentation.material.as_ref().unwrap();
        assert_eq!(material.node_guid, node_guid);
        assert_eq!(material.physical_token, "PrismMaterial-001");
        assert_eq!(&*material.visual_guid, visual_guid);
        assert_eq!(material.visual_preset, None);
    }

    #[test]
    fn linked_body_presentation_joins_its_browser_node() {
        let body_tag = 256u32;
        let node_tag = 257u32;
        let entity = (1u64 << 40) + 42;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, &format!("0_{entity}"));
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        lp_utf16(&mut bytes, "PrismMaterial-001");
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        lp_utf16(&mut bytes, "Body");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        lp_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            lp_utf16(&mut bytes, marker);
        }
        let node_start = bytes.len();
        lp_ascii(&mut bytes, &node_tag.to_string());
        bytes.extend_from_slice(&43u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![43],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0), primary_record(43, node_start)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).unwrap();
        assert_eq!(presentations.len(), 1);
        assert_eq!(
            presentations[0].browser_node.as_ref().unwrap().guid,
            node_guid
        );
    }

    #[test]
    fn presentation_refuses_an_unregistered_node_shaped_byte_run() {
        let body_tag = 256u32;
        let entity = 42u64;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, "0_42");
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        lp_utf16(&mut bytes, "PrismMaterial-001");
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        lp_utf16(&mut bytes, "Body");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        lp_utf16(&mut bytes, "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE");
        lp_utf16(&mut bytes, APPEARANCE_LIBRARY_ID);
        lp_utf16(&mut bytes, "Prism-001");
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("typed body owner");
        assert_eq!(presentations.len(), 1);
        assert!(presentations[0].browser_node.is_none());
        assert_eq!(
            presentations[0]
                .material
                .as_ref()
                .and_then(|material| material
                    .visual_preset
                    .as_ref()
                    .map(|field| field.value.as_str())),
            Some("Prism-001")
        );
    }

    #[test]
    fn presentation_does_not_consume_the_next_primary_frame() {
        let entity = 42u64;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "256");
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, "0_42");

        let next_start = bytes.len();
        lp_ascii(&mut bytes, "257");
        bytes.extend_from_slice(&99u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        lp_utf16(&mut bytes, "PrismMaterial-001");
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        lp_utf16(&mut bytes, "Body");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        lp_utf16(&mut bytes, "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE");
        lp_utf16(&mut bytes, APPEARANCE_LIBRARY_ID);

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    "00000000-0000-0000-0000-000000000000",
                    None,
                    0,
                    "",
                    vec![99],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0), primary_record(99, next_start)],
            secondary_records: Vec::new(),
        };

        let presentations = body_presentations(&bytes, &meta).expect("exact body frame");
        let [presentation] = presentations.as_slice() else {
            panic!("one body presentation expected")
        };
        assert!(presentation.material.is_none());
    }

    #[test]
    fn browser_node_records_skip_other_typed_member_variants() {
        let record_index = 43u32;
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "256");
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, "11111111-2222-8333-A444-555555555555");
        bytes.extend_from_slice(&[1, 0, 1]);
        bytes.extend_from_slice(&42u64.to_le_bytes());
        let meta = crate::metastream::MetaStream {
            types: vec![design_type(
                BROWSER_NODE_TYPE_GUID,
                Some(BROWSER_NODE_BASE_TYPE_GUID),
                BROWSER_NODE_TYPE_VERSION,
                DESIGN_MODULE_FUSION,
                vec![u64::from(record_index)],
            )],
            records: vec![primary_record(u64::from(record_index), 0)],
            secondary_records: Vec::new(),
        };

        assert!(browser_node_records(&bytes, &meta)
            .expect("typed alternate member")
            .is_empty());
    }

    #[test]
    fn bare_presentation_uses_its_typed_primary_owner_not_class_299() {
        let entity = 2_000u64;
        let node_record = 2_100u32;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
        let mut types = (0..44)
            .map(|_| {
                design_type(
                    "00000000-0000-0000-0000-000000000000",
                    None,
                    0,
                    "",
                    Vec::new(),
                )
            })
            .collect::<Vec<_>>();
        let body_tag = 300u32;
        let node_tag = 301u32;
        types.extend([
            design_type(
                BODY_PRESENTATION_TYPE_GUID,
                Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                BODY_PRESENTATION_TYPE_VERSION,
                DESIGN_MODULE_BODY,
                vec![entity],
            ),
            design_type(
                BROWSER_NODE_TYPE_GUID,
                Some(BROWSER_NODE_BASE_TYPE_GUID),
                BROWSER_NODE_TYPE_VERSION,
                DESIGN_MODULE_FUSION,
                vec![u64::from(node_record)],
            ),
        ]);

        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        lp_ascii(&mut bytes, "299");
        bytes.extend_from_slice(&999_999u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        lp_utf16(&mut bytes, BODY_PRESENTATION_MATERIAL_ENVELOPE_ID);
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        bytes.extend_from_slice(&[0; 4]);
        lp_utf16(&mut bytes, "PrismMaterial-018");
        push_reference_u64(&mut bytes, 1_900);
        bytes.push(0);
        lp_utf16(&mut bytes, node_guid);
        push_reference_u64(&mut bytes, entity + 1);
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            lp_utf16(&mut bytes, marker);
        }

        let node_start = bytes.len();
        assert!(bare_presentation_material(&bytes, 15, node_start, entity).is_some());
        let trailer_len = lp_utf16_bytes(MODERN_APPEARANCE_LIBRARY_IDS[1])
            .expect("fixture UTF-16 code-unit count fits u32")
            .len();
        assert!(
            bare_presentation_material(&bytes, 15, node_start - trailer_len, entity,).is_none()
        );
        lp_ascii(&mut bytes, &node_tag.to_string());
        bytes.extend_from_slice(&node_record.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[0, 1, 1]);
        bytes.extend_from_slice(&entity.to_le_bytes());

        let meta = crate::metastream::MetaStream {
            types,
            records: vec![
                primary_record(entity, 0),
                primary_record(u64::from(node_record), node_start),
            ],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("exact primary owner frame");
        let [presentation] = presentations.as_slice() else {
            panic!("one bare body presentation expected")
        };
        assert_eq!(presentation.entity_suffix, entity);
        assert_eq!(presentation.owner, BodyPresentationOwner::Bare);
        assert_eq!(
            presentation
                .browser_node
                .as_ref()
                .map(|node| node.guid.as_str()),
            Some(node_guid)
        );
        let material = presentation.material.as_ref().expect("material envelope");
        assert_eq!(material.physical_token, "PrismMaterial-018");
        assert_eq!(&*material.visual_guid, visual_guid);
    }

    #[test]
    fn typed_presentation_extracts_custom_body_name() {
        let body_tag = 256u32;
        let entity = 42u64;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, &format!("0_{entity}"));
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        lp_utf16(&mut bytes, "PrismMaterial-001");
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        lp_utf16(&mut bytes, "Bottom");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        lp_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            lp_utf16(&mut bytes, marker);
        }

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("named presentation frame");
        assert_eq!(presentations.len(), 1);
        assert_eq!(presentations[0].name.as_deref(), Some("Bottom"));
        assert_eq!(
            presentations[0]
                .material
                .as_ref()
                .unwrap()
                .body_name
                .as_deref(),
            Some("Bottom")
        );
    }

    #[test]
    fn bare_presentation_extracts_custom_body_name() {
        let body_tag = 256u32;
        let entity = 42u64;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        lp_utf16(&mut bytes, BODY_PRESENTATION_MATERIAL_ENVELOPE_ID);
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        bytes.extend_from_slice(&[0; 4]);
        lp_utf16(&mut bytes, "PrismMaterial-018");
        push_reference_u64(&mut bytes, 1_900);
        bytes.push(0);
        lp_utf16(&mut bytes, node_guid);
        push_reference_u64(&mut bytes, entity + 1);
        bytes.extend_from_slice(&[0; 4]);
        lp_utf16(&mut bytes, "Plate");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            lp_utf16(&mut bytes, marker);
        }

        let meta = crate::metastream::MetaStream {
            types: vec![design_type(
                BODY_PRESENTATION_TYPE_GUID,
                Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                BODY_PRESENTATION_TYPE_VERSION,
                DESIGN_MODULE_BODY,
                vec![entity],
            )],
            records: vec![primary_record(entity, 0)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("bare presentation frame");
        assert_eq!(presentations.len(), 1);
        assert_eq!(presentations[0].name.as_deref(), Some("Plate"));
        assert_eq!(
            presentations[0]
                .material
                .as_ref()
                .unwrap()
                .body_name
                .as_deref(),
            Some("Plate")
        );
    }

    #[test]
    fn typed_presentation_with_empty_name_yields_none() {
        let body_tag = 256u32;
        let entity = 42u64;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, &format!("0_{entity}"));
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        lp_utf16(&mut bytes, "PrismMaterial-001");
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        lp_utf16(&mut bytes, "");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        lp_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            lp_utf16(&mut bytes, marker);
        }

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("named presentation frame");
        assert_eq!(presentations.len(), 1);
        assert_eq!(presentations[0].name, None);
        assert_eq!(presentations[0].material.as_ref().unwrap().body_name, None);
    }

    #[test]
    fn typed_presentation_rejects_mismatched_scene_node() {
        let body_tag = 256u32;
        let entity = 42u64;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, &format!("0_{entity}"));
        lp_utf16(&mut bytes, node_guid);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
        lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
        lp_utf16(&mut bytes, "PrismMaterial-001");
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 2);
        lp_utf16(&mut bytes, "Plate");
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        lp_utf16(&mut bytes, visual_guid);
        for marker in MODERN_APPEARANCE_LIBRARY_IDS {
            lp_utf16(&mut bytes, marker);
        }

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 2],
                ),
            ],
            records: vec![primary_record(entity, 0)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("named presentation frame");
        assert_eq!(presentations.len(), 1);
        assert_eq!(presentations[0].material, None);
        assert_eq!(presentations[0].name, None);
    }

    #[test]
    fn typed_presentation_rejects_ambiguity() {
        let body_tag = 256u32;
        let entity = 42u64;
        let node_guid = "11111111-2222-8333-A444-555555555555";
        let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, &body_tag.to_string());
        bytes.extend_from_slice(&entity.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        lp_utf16(&mut bytes, &format!("0_{entity}"));

        for name in ["CandidateA", "CandidateB"] {
            lp_utf16(&mut bytes, node_guid);
            bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
            lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
            lp_utf16(&mut bytes, "PrismMaterial-001");
            push_reference_u64(&mut bytes, 7);
            bytes.push(0);
            push_reference_u64(&mut bytes, entity + 1);
            lp_utf16(&mut bytes, name);
            bytes.extend_from_slice(&1.0f32.to_le_bytes());
            bytes.extend_from_slice(&[1, 1]);
            lp_utf16(&mut bytes, visual_guid);
            for marker in MODERN_APPEARANCE_LIBRARY_IDS {
                lp_utf16(&mut bytes, marker);
            }
        }

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![primary_record(entity, 0)],
            secondary_records: Vec::new(),
        };
        let presentations = body_presentations(&bytes, &meta).expect("named presentation frame");
        assert_eq!(presentations.len(), 1);
        assert_eq!(presentations[0].material, None);
        assert_eq!(presentations[0].name, None);
    }

    /// Synthetic parts of a legacy-tail (type version 18) body record.
    struct LegacyTailRecord {
        entity: u64,
        container: u64,
        scene_reference: u64,
        scene_guid: Option<&'static str>,
        display_name: Option<&'static str>,
        color: [f32; 4],
        truncate_tail_by: usize,
    }

    impl LegacyTailRecord {
        fn new(entity: u64) -> Self {
            Self {
                entity,
                container: 7,
                scene_reference: entity + 1,
                scene_guid: None,
                display_name: None,
                color: [0.25, 0.5, 0.75, 1.0],
                truncate_tail_by: 0,
            }
        }

        fn bytes(&self) -> Vec<u8> {
            let mut bytes = Vec::new();
            lp_ascii(&mut bytes, "256");
            bytes.extend_from_slice(&self.entity.to_le_bytes());
            lp_utf16(&mut bytes, "11111111-2222-8333-a444-555555555555");
            bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            lp_utf16(&mut bytes, "99999999-8888-8777-A666-555555555555");
            lp_utf16(&mut bytes, PHYSICAL_MATERIAL_LIBRARY_ID);
            if let Some(name) = self.display_name {
                lp_utf16(&mut bytes, name);
            }
            lp_utf16(&mut bytes, "PrismMaterial-018");
            push_reference_u64(&mut bytes, self.container);
            bytes.push(0);
            if let Some(guid) = self.scene_guid {
                lp_utf16(&mut bytes, guid);
            }
            push_reference_u64(&mut bytes, self.scene_reference);
            bytes.extend_from_slice(&[0; 4]);
            for component in self.color {
                bytes.extend_from_slice(&component.to_le_bytes());
            }
            bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
            lp_utf16(&mut bytes, "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015");
            lp_utf16(&mut bytes, APPEARANCE_LIBRARY_ID);
            bytes.extend_from_slice(&[0; 4]);
            lp_utf16(&mut bytes, "PrismOpaque");
            bytes.extend_from_slice(&0.0f32.to_le_bytes());
            bytes.extend_from_slice(&1.0f32.to_le_bytes());
            bytes.truncate(bytes.len() - self.truncate_tail_by);
            bytes
        }

        fn meta(&self, scene_guid: &str) -> crate::metastream::MetaStream {
            let types = vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    crate::design::presentation::BODY_PRESENTATION_TYPE_VERSION_LEGACY_TAIL,
                    DESIGN_MODULE_BODY,
                    vec![self.entity],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![self.container],
                ),
                design_type(
                    scene_guid,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![self.entity + 1],
                ),
            ];
            crate::metastream::MetaStream {
                types,
                records: vec![primary_record(self.entity, 0)],
                secondary_records: Vec::new(),
            }
        }
    }

    fn legacy_tail_presentation(record: &LegacyTailRecord) -> super::BodyPresentation {
        let bytes = record.bytes();
        let meta = record.meta(BODY_SCENE_NODE_TYPE_GUID);
        let mut presentations = body_presentations(&bytes, &meta).expect("legacy-tail frame");
        assert_eq!(presentations.len(), 1);
        presentations.remove(0)
    }

    #[test]
    fn legacy_tail_record_binds_without_a_browser_node() {
        let presentation = legacy_tail_presentation(&LegacyTailRecord::new(42));
        assert!(presentation.legacy_tail);
        assert_eq!(presentation.legacy_tail_refusal, None);
        assert_eq!(presentation.owner, BodyPresentationOwner::Bare);
        assert_eq!(presentation.entity_suffix, 42);
        assert_eq!(presentation.browser_node, None);
        let material = presentation.material.expect("legacy-tail material");
        assert_eq!(material.physical_token, "PrismMaterial-018");
        assert_eq!(
            &*material.visual_guid,
            "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015"
        );
        assert_eq!(material.body_name, None);
        assert_eq!(material.visual_preset, None);
    }

    #[test]
    fn legacy_tail_record_with_scene_guid_and_display_name_binds() {
        let mut record = LegacyTailRecord::new(42);
        record.scene_guid = Some("33333333-4444-8555-b666-777777777777");
        record.display_name = Some("ABS Plastic");
        let presentation = legacy_tail_presentation(&record);
        assert_eq!(presentation.legacy_tail_refusal, None);
        let material = presentation.material.expect("legacy-tail material");
        assert_eq!(material.physical_token, "PrismMaterial-018");
    }

    #[test]
    fn legacy_tail_record_refuses_a_missing_scene_reference() {
        let mut record = LegacyTailRecord::new(42);
        record.scene_reference = 99;
        let presentation = legacy_tail_presentation(&record);
        assert_eq!(
            presentation.legacy_tail_refusal,
            Some(super::LegacyTailRefusal::SceneReferenceMissing)
        );
        assert_eq!(presentation.material, None);
    }

    #[test]
    fn legacy_tail_record_refuses_a_wrong_scene_type() {
        let record = LegacyTailRecord::new(42);
        let bytes = record.bytes();
        let meta = record.meta("AAAAAAAA-0000-4000-8000-000000000000");
        let presentations = body_presentations(&bytes, &meta).expect("legacy-tail frame");
        assert_eq!(
            presentations[0].legacy_tail_refusal,
            Some(super::LegacyTailRefusal::SceneTypeMismatch)
        );
        assert_eq!(presentations[0].material, None);
    }

    #[test]
    fn legacy_tail_record_refuses_a_truncated_tail() {
        let mut record = LegacyTailRecord::new(42);
        record.truncate_tail_by = 6;
        let presentation = legacy_tail_presentation(&record);
        assert_eq!(
            presentation.legacy_tail_refusal,
            Some(super::LegacyTailRefusal::TailTruncated)
        );
        assert_eq!(presentation.material, None);
    }

    #[test]
    fn legacy_tail_record_refuses_a_non_opaque_colour() {
        let mut record = LegacyTailRecord::new(42);
        record.color = [0.25, 0.5, 0.75, 0.5];
        let presentation = legacy_tail_presentation(&record);
        assert_eq!(
            presentation.legacy_tail_refusal,
            Some(super::LegacyTailRefusal::EnvelopeUnparsed)
        );
    }

    #[test]
    fn other_body_presentation_versions_are_ignored() {
        let record = LegacyTailRecord::new(42);
        let bytes = record.bytes();
        let mut meta = record.meta(BODY_SCENE_NODE_TYPE_GUID);
        meta.types[0].version = 17;
        assert!(body_presentations(&bytes, &meta).unwrap().is_empty());
    }
}
