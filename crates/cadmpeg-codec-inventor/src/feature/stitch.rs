// SPDX-License-Identifier: Apache-2.0
//! Stitch Surface features
//! ([spec §13.12](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#1312-stitch-surface)).
//!
//! Slot 0 of the six-slot record is the object collection of the surface-body
//! wrappers that the stitch joins, and slot 2 is the wrapper of the result. The
//! history join checks the operands against the bodies that leave the model
//! between the state before the feature and the state after it.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::evaluation::{
    FeatureEvaluation, FeatureResultMembers, FeatureResultTopology,
};
use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers, NativeSelections};
use cadmpeg_ir::features::{
    Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureOperation, SewBodySelection,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::scalar::PositiveLength;

use super::history_states::{FeatureHistoryState, StateIndex};
use super::history_views::{StateModel, StateViews};
use super::{
    ClassId, PmDcFeature, PmDcFeatureLabel, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
    ProjectionIndex,
};

/// Feature-label class of a Stitch Surface.
pub(super) const CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0x60, 0x8b, 0xa3, 0x9e, 0xd6, 0x11, 0x74, 0x73, 0x27, 0x99, 0x43, 0xad, 0x7f, 0xbe, 0x0b, 0x63,
]));

/// Source tag of the projected feature.
pub(crate) const TAG: &str = "stitch";

/// Slot of the result surface-body wrapper.
pub(super) const RESULT_SLOT: usize = 2;

/// Count of property slots of the record.
const SLOT_COUNT: usize = 6;

/// Slots of the two Boolean controls.
const CONTROL_SLOTS: [usize; 2] = [3, 5];

/// Slot of the tolerance parameter.
const TOLERANCE_SLOT: usize = 4;

/// The wrappers of the operand collection, in collection order.
fn operand_wrappers(source: &PmDcFeature, index: &ProjectionIndex<'_>) -> Option<Vec<String>> {
    let slots = source.properties.references();
    if slots.len() != SLOT_COUNT || slots[1].index() != 0 || slots[TOLERANCE_SLOT].index() == 0 {
        return None;
    }
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items,
    } = &super::slot_property(source, 0, index)?.kind
    else {
        return None;
    };
    let token = source.identity.segment_token.as_str();
    let wrappers = items
        .references()
        .iter()
        .map(|reference| {
            let property = super::resolve_property(token, reference.index(), index)?;
            matches!(property.kind, PmDcFeaturePropertyKind::SurfaceBody { .. })
                .then(|| property.id())
        })
        .collect::<Option<Vec<_>>>()?;
    let distinct = wrappers.iter().collect::<std::collections::HashSet<_>>();
    (!wrappers.is_empty() && distinct.len() == wrappers.len()).then_some(wrappers)
}

/// Project a Stitch Surface with its operand wrappers and its result body.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let operands = operand_wrappers(source, index)?;
    let controls = CONTROL_SLOTS.into_iter().all(|slot| {
        matches!(
            super::slot_property(source, slot, index).map(|property| &property.kind),
            Some(PmDcFeaturePropertyKind::Boolean { .. })
        )
    });
    let wrapper = super::slot_property(source, RESULT_SLOT, index)?;
    if !controls || !matches!(wrapper.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
        return None;
    }
    let members = FeatureResultMembers::new(
        vec![cadmpeg_core::text::NonBlankString::new(wrapper.id())?],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .ok()?;
    let bodies = match operands.as_slice() {
        [one] => BodySelection::Native(one.clone()),
        _ => BodySelection::NativeSet(NativeSelections::try_from(operands).ok()?),
    };
    let bodies = SewBodySelection::try_from(bodies).ok()?;
    let gap_tolerance = super::length_parameter(source, TOLERANCE_SLOT, index)
        .and_then(|length| PositiveLength::new(length.get()));
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
                    gap_tolerance,
                },
            )),
            native_ref: Some(source.id()),
        },
        result,
    )))
}

/// Why the history states of a Stitch Surface disagree with its operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Disagreement {
    /// The number of bodies that appear is not one.
    Created,
    /// The number of bodies that leave is not the number of operands.
    Consumed,
    /// The faces of the leaving bodies are not the faces of the result.
    Faces,
}

impl Disagreement {
    const fn label(self) -> &'static str {
        match self {
            Self::Created => "without exactly one new body",
            Self::Consumed => "whose leaving bodies differ in number from the operands",
            Self::Faces => "whose faces differ from the result faces",
        }
    }
}

/// Counts of the check of the Stitch Surface features of a part against the
/// history states.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// Stitch Surface features that project to a neutral knit.
    pub(crate) projected: usize,
    /// Features whose states show the rule of section 13.12.
    pub(crate) agree: usize,
    /// Features whose states disagree, by reason.
    pub(crate) disagree: BTreeMap<Disagreement, usize>,
    /// Features whose states are absent or do not decode.
    pub(crate) unchecked: usize,
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

/// The count of operand wrappers of a projected Stitch Surface.
fn operand_count(feature: &Feature) -> Option<usize> {
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::SewBodies { bodies, .. }) => {
            match &**bodies {
                BodySelection::Native(_) => Some(1),
                BodySelection::NativeSet(names) => Some(names.len()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Check every projected Stitch Surface against the states before and after
/// it: exactly one body appears, as many bodies leave as the feature has
/// operands, and the faces of the leaving bodies are the faces of the new body.
pub(crate) fn check(
    ctx: &DecodeContext<'_>,
    views: Option<&StateViews<'_, '_>>,
    index: &StateIndex,
    records: &[FeatureHistoryState],
    features: &[Feature],
) -> Result<Outcome, CodecError> {
    let states = records
        .iter()
        .map(|record| (record.feature.as_str(), record))
        .collect::<HashMap<_, _>>();
    let mut outcome = Outcome::default();
    for feature in features {
        if feature.source_tag.as_deref() != Some(TAG) {
            continue;
        }
        outcome.projected += 1;
        let (Some(native), Some(operands)) = (feature.native_ref.as_deref(), operand_count(feature))
        else {
            outcome.unchecked += 1;
            continue;
        };
        match states_agree(ctx, views, index, states.get(native).copied(), operands)? {
            Ok(Some(())) => outcome.agree += 1,
            Ok(None) => outcome.unchecked += 1,
            Err(reason) => *outcome.disagree.entry(reason).or_default() += 1,
        }
    }
    Ok(outcome)
}

/// `Ok(None)` when the states cannot be read, and the disagreement when they
/// are read and differ.
fn states_agree(
    ctx: &DecodeContext<'_>,
    views: Option<&StateViews<'_, '_>>,
    index: &StateIndex,
    record: Option<&FeatureHistoryState>,
    operands: usize,
) -> Result<Result<Option<()>, Disagreement>, CodecError> {
    let Some((after, before)) = record.and_then(|record| Some((record.state_after?, record.state_before?)))
    else {
        return Ok(Ok(None));
    };
    let (Some(after_state), Some(before_state)) = (
        usize::try_from(after.ordinal)
            .ok()
            .and_then(|ordinal| index.states.get(ordinal)),
        usize::try_from(before)
            .ok()
            .and_then(|ordinal| index.states.get(ordinal)),
    ) else {
        return Ok(Ok(None));
    };
    let appear = |from: &[i64], other: &[i64]| {
        from.iter()
            .filter(|key| other.binary_search(key).is_err())
            .copied()
            .collect::<Vec<_>>()
    };
    let created = appear(&after_state.body_keys, &before_state.body_keys);
    let consumed = appear(&before_state.body_keys, &after_state.body_keys);
    let [created] = created.as_slice() else {
        return Ok(Err(Disagreement::Created));
    };
    if consumed.len() != operands {
        return Ok(Err(Disagreement::Consumed));
    }
    let Some(views) = views else {
        return Ok(Ok(None));
    };
    let (before_model, after_model) = (views.at(ctx, before)?, views.at(ctx, after.ordinal)?);
    let (StateModel::Model(before_model), StateModel::Model(after_model)) =
        (&*before_model, &*after_model)
    else {
        return Ok(Ok(None));
    };
    let (Some(before_faces), Some(after_faces)) =
        (faces_by_key(before_model), faces_by_key(after_model))
    else {
        return Ok(Ok(None));
    };
    let mut total = 0_usize;
    for key in &consumed {
        let faces = u64::try_from(*key)
            .ok()
            .and_then(|key| before_faces.get(&key));
        match faces {
            Some(faces) if *faces > 0 => total += faces,
            _ => return Ok(Err(Disagreement::Faces)),
        }
    }
    let created = u64::try_from(*created)
        .ok()
        .and_then(|key| after_faces.get(&key));
    Ok(if created == Some(&total) {
        Ok(Some(()))
    } else {
        Err(Disagreement::Faces)
    })
}

/// The loss of the check, if any Stitch Surface feature exists.
pub(crate) fn losses(outcome: &Outcome) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use crate::loss::InventorLossCode;
    let mut notes = Vec::new();
    if outcome.projected > 0 {
        notes.push(InventorLossCode::FeatureStitchControlsNative.note(format!(
            "Typed {} Stitch Surface feature(s) as knits of their operand collection; {} agree with their history states. The two Boolean controls stay source properties, and the role of slot 4 as the tolerance is assumed from its position.",
            outcome.projected, outcome.agree
        )));
    }
    let disagree = outcome.disagree.values().sum::<usize>();
    if disagree + outcome.unchecked > 0 {
        let reasons = outcome
            .disagree
            .iter()
            .map(|(reason, count)| format!("{count} {}", reason.label()))
            .chain((outcome.unchecked > 0).then(|| {
                format!(
                    "{} without readable history states",
                    outcome.unchecked
                )
            }))
            .collect::<Vec<_>>()
            .join(", ");
        notes.push(InventorLossCode::FeatureStitchHistoryUnconfirmed.note(format!(
            "Stitch Surface feature(s) whose operands the history states do not confirm: {reasons}."
        )));
    }
    notes
}

#[cfg(test)]
mod tests;
