// SPDX-License-Identifier: Apache-2.0
//! Stitch Surface features
//! ([spec §13.12](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#1312-stitch-surface)).
//!
//! The six-slot record stores no operand. The operands are the surface bodies
//! that leave the model between the state before the feature and the state
//! after it, and the result is the one body that appears. The projection
//! types the operation with unresolved operands. The history join resolves
//! them when the two states and their geometry show the rule.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::evaluation::{
    FeatureEvaluation, FeatureResultMembers, FeatureResultTopology,
};
use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers};
use cadmpeg_ir::features::{
    Feature, FeatureContent, FeatureDefinition, FeatureOperation, SewBodySelection,
};
use cadmpeg_ir::ids::{FeatureId, FeatureResultTopologyId};
use cadmpeg_ir::native::bytes::NativeBytes;

use super::body_selection::WrapperLink;
use super::history_states::{FeatureHistoryState, StateIndex};
use super::history_views::{StateModel, StateViews};
use super::{
    ClassId, PmDcFeature, PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily, PmDcKernelEntityLink, ProjectionIndex,
};

/// Feature-label class of a Stitch Surface.
pub(super) const CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x60, 0x8b, 0xa3, 0x9e, 0xd6, 0x11, 0x74, 0x73, 0x27, 0x99, 0x43, 0xad, 0x7f, 0xbe, 0x0b, 0x63,
]));

/// Source tag of the projected feature.
pub(crate) const TAG: &str = "stitch";

/// Slot of the result surface-body wrapper.
pub(super) const RESULT_SLOT: usize = 5;

/// Count of property slots of the record.
const SLOT_COUNT: usize = 6;

/// Slots of the two Boolean controls.
const CONTROL_SLOTS: [usize; 2] = [2, 4];

/// Slot of the boundary-patch list.
const PATCH_SLOT: usize = 3;

/// Whether the record has the six-slot form.
fn form(source: &PmDcFeature, index: &ProjectionIndex<'_>) -> bool {
    let slots = source.properties.references();
    if slots.len() != SLOT_COUNT || slots[0].index() == 0 || slots[1].index() != 0 {
        return false;
    }
    let boolean = |slot| {
        matches!(
            super::slot_property(source, slot, index).map(|property| &property.kind),
            Some(PmDcFeaturePropertyKind::Boolean { .. })
        )
    };
    let patch = matches!(
        super::slot_property(source, PATCH_SLOT, index).map(|property| &property.kind),
        Some(PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BoundaryPatch,
            items,
        }) if items.references().len() <= 1
    );
    CONTROL_SLOTS.into_iter().all(boolean) && patch
}

/// Project a Stitch Surface with unresolved operands and its result body.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    if !form(source, index) {
        return None;
    }
    let wrapper = super::slot_property(source, RESULT_SLOT, index)?;
    if !matches!(wrapper.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
        return None;
    }
    let members = FeatureResultMembers::new(
        vec![cadmpeg_core::text::NonBlankString::new(wrapper.id())?],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .ok()?;
    let bodies = SewBodySelection::try_from(BodySelection::Unresolved).ok()?;
    if let Err(error) = ctx.charge_entities(2, "project Inventor Stitch Surface") {
        return Some(Err(error));
    }
    let key = match source.identity.key() {
        Ok(key) => key,
        Err(error) => return Some(Err(error)),
    };
    let id = FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
        key.clone(),
    );
    let result = FeatureResultTopology::new(
        FeatureResultTopologyId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
            key,
        ),
        id.clone(),
        members,
        Some(wrapper.id()),
    );
    Some(Ok((
        Feature {
            product_definition: None,
            id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: super::boolean_properties(source, &CONTROL_SLOTS, index),
            source_tag: Some(TAG.into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::SewBodies {
                    bodies,
                    gap_tolerance: None,
                },
            )),
            native_ref: Some(source.id()),
        },
        result,
    )))
}

/// Why the operands of a Stitch Surface stay unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Unresolved {
    /// No naming tag lists the feature, or no state precedes its state.
    NoStates,
    /// The result wrapper has no body key.
    NoResultKey,
    /// The result key is not a body of the state after and of no body of the
    /// state before, or another body appears.
    ResultKey,
    /// Fewer than two bodies leave the model.
    Inputs,
    /// A state does not decode.
    State,
    /// The faces of the leaving bodies are not the faces of the result.
    Faces,
}

impl Unresolved {
    const fn label(self) -> &'static str {
        match self {
            Self::NoStates => "without a state before and after",
            Self::NoResultKey => "without a result body key",
            Self::ResultKey => "whose result key does not appear alone",
            Self::Inputs => "with fewer than two bodies that leave",
            Self::State => "whose history state does not decode",
            Self::Faces => "whose faces differ from the result faces",
        }
    }
}

/// Counts of the resolution of the Stitch Surface features of a part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// Features whose operands are resolved.
    pub(crate) resolved: usize,
    /// Features whose operands stay unresolved, by reason.
    pub(crate) unresolved: BTreeMap<Unresolved, usize>,
}

/// Records that the resolution reads.
pub(crate) struct Inputs<'a> {
    /// Kernel-entity links.
    pub(crate) kernel_links: &'a [PmDcKernelEntityLink],
    /// Joins of wrappers to kernel-entity links.
    pub(crate) wrapper_links: &'a [WrapperLink],
    /// Feature property records.
    pub(crate) properties: &'a [PmDcFeatureProperty],
    /// `PmDc` feature records.
    pub(crate) features: &'a [PmDcFeature],
}

/// The faces of each body of a state model, by kernel body key.
fn faces_by_key(model: &CadIr) -> Option<HashMap<u64, usize>> {
    let keys: Vec<BodyNativeKey> = model
        .native
        .namespace("inventor")?
        .arena_as("body_native_keys")
        .ok()?;
    let shell_region = model
        .geometry
        .shells
        .iter()
        .map(|shell| (&shell.id, &shell.region))
        .collect::<HashMap<_, _>>();
    let region_body = model
        .geometry
        .regions
        .iter()
        .map(|region| (&region.id, &region.body))
        .collect::<HashMap<_, _>>();
    let mut per_body = HashMap::<&cadmpeg_ir::ids::BodyId, usize>::new();
    for face in &model.geometry.faces {
        let body = region_body.get(shell_region.get(&face.shell)?)?;
        *per_body.entry(body).or_default() += 1;
    }
    let mut result = HashMap::new();
    for key in &keys {
        let faces = per_body.get(&key.body).copied().unwrap_or(0);
        if result.insert(key.asm_body_key?, faces).is_some() {
            return None;
        }
    }
    Some(result)
}

/// The body key of the result wrapper of `source`.
fn result_key(source: &PmDcFeature, inputs: &Inputs<'_>) -> Option<i64> {
    let slot = source.properties.references().get(RESULT_SLOT)?;
    let ordinal = slot.index().checked_sub(1)?;
    let wrapper = inputs.properties.iter().find(|property| {
        property.identity.segment_token == source.identity.segment_token
            && property.identity.record_ordinal == ordinal
    })?;
    let wrapper = wrapper.id();
    let join = inputs
        .wrapper_links
        .iter()
        .find(|join| join.wrapper == wrapper)?;
    let link = inputs.kernel_links.iter().find(|link| link.id() == join.link)?;
    Some(i64::from(link.entity_id))
}

/// Resolve the operands of every Stitch Surface feature of `features` whose
/// history states show the rule of section 13.12. A feature that fails a check
/// keeps its unresolved operands.
pub(crate) fn resolve(
    ctx: &DecodeContext<'_>,
    views: Option<&StateViews<'_, '_>>,
    index: &StateIndex,
    records: &[FeatureHistoryState],
    inputs: &Inputs<'_>,
    features: &mut [Feature],
) -> Result<Outcome, CodecError> {
    let states = records
        .iter()
        .map(|record| (record.feature.as_str(), record))
        .collect::<HashMap<_, _>>();
    let sources = inputs
        .features
        .iter()
        .map(|feature| (feature.id(), feature))
        .collect::<HashMap<_, _>>();
    let mut outcome = Outcome::default();
    for feature in features {
        if feature.source_tag.as_deref() != Some(TAG) {
            continue;
        }
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        let (Some(record), Some(source)) = (states.get(native), sources.get(native)) else {
            *outcome.unresolved.entry(Unresolved::NoStates).or_default() += 1;
            continue;
        };
        match operands(ctx, views, index, record, source, inputs)? {
            Ok(bodies) => {
                let before = record.state_before.unwrap_or_default();
                let selection = BodySelection::local(
                    bodies.iter().map(i64::to_string).collect(),
                    format!("inventor:history-state:{before}"),
                )?;
                let Ok(bodies) = SewBodySelection::try_from(selection) else {
                    *outcome.unresolved.entry(Unresolved::Inputs).or_default() += 1;
                    continue;
                };
                feature
                    .evaluation
                    .set_definition(FeatureDefinition::Operation(FeatureOperation::SewBodies {
                        bodies,
                        gap_tolerance: None,
                    }));
                outcome.resolved += 1;
            }
            Err(reason) => *outcome.unresolved.entry(reason).or_default() += 1,
        }
    }
    Ok(outcome)
}

/// The body keys that a Stitch Surface consumes, or the reason they are not
/// proved.
fn operands(
    ctx: &DecodeContext<'_>,
    views: Option<&StateViews<'_, '_>>,
    index: &StateIndex,
    record: &FeatureHistoryState,
    source: &PmDcFeature,
    inputs: &Inputs<'_>,
) -> Result<Result<Vec<i64>, Unresolved>, CodecError> {
    let (Some(after), Some(before)) = (record.state_after, record.state_before) else {
        return Ok(Err(Unresolved::NoStates));
    };
    let (Some(after_state), Some(before_state)) = (
        usize::try_from(after.ordinal)
            .ok()
            .and_then(|ordinal| index.states.get(ordinal)),
        usize::try_from(before)
            .ok()
            .and_then(|ordinal| index.states.get(ordinal)),
    ) else {
        return Ok(Err(Unresolved::NoStates));
    };
    let Some(result) = result_key(source, inputs) else {
        return Ok(Err(Unresolved::NoResultKey));
    };
    let created = after_state
        .body_keys
        .iter()
        .filter(|key| before_state.body_keys.binary_search(key).is_err())
        .copied()
        .collect::<Vec<_>>();
    if created != [result] {
        return Ok(Err(Unresolved::ResultKey));
    }
    let consumed = before_state
        .body_keys
        .iter()
        .filter(|key| after_state.body_keys.binary_search(key).is_err())
        .copied()
        .collect::<Vec<_>>();
    if consumed.len() < 2 {
        return Ok(Err(Unresolved::Inputs));
    }
    let Some(views) = views else {
        return Ok(Err(Unresolved::State));
    };
    let (before_model, after_model) = (views.at(ctx, before)?, views.at(ctx, after.ordinal)?);
    let (StateModel::Model(before_model), StateModel::Model(after_model)) =
        (&*before_model, &*after_model)
    else {
        return Ok(Err(Unresolved::State));
    };
    let (Some(before_faces), Some(after_faces)) =
        (faces_by_key(before_model), faces_by_key(after_model))
    else {
        return Ok(Err(Unresolved::State));
    };
    let Ok(result) = u64::try_from(result) else {
        return Ok(Err(Unresolved::ResultKey));
    };
    let mut total = 0_usize;
    for key in &consumed {
        let faces = u64::try_from(*key)
            .ok()
            .and_then(|key| before_faces.get(&key));
        match faces {
            Some(faces) if *faces > 0 => total += faces,
            _ => return Ok(Err(Unresolved::Faces)),
        }
    }
    if after_faces.get(&result) != Some(&total) {
        return Ok(Err(Unresolved::Faces));
    }
    Ok(Ok(consumed))
}

/// The loss of the resolution, if any Stitch Surface feature exists.
pub(crate) fn losses(outcome: &Outcome) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use crate::loss::InventorLossCode;
    let mut notes = Vec::new();
    if outcome.resolved > 0 {
        notes.push(InventorLossCode::FeatureStitchControlsNative.note(format!(
            "Typed {} Stitch Surface feature(s) as knits of the bodies that leave their history state; the two Boolean controls stay source properties, and the stitch tolerance and the boundary-patch item are not represented.",
            outcome.resolved
        )));
    }
    let open = outcome.unresolved.values().sum::<usize>();
    if open > 0 {
        let reasons = outcome
            .unresolved
            .iter()
            .map(|(reason, count)| format!("{count} {}", reason.label()))
            .collect::<Vec<_>>()
            .join(", ");
        notes.push(InventorLossCode::FeatureStitchOperandsUnresolved.note(format!(
            "Retained {open} Stitch Surface feature(s) with unresolved operands: {reasons}."
        )));
    }
    notes
}

#[cfg(test)]
mod tests;
