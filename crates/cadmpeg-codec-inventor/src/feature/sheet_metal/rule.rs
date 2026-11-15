// SPDX-License-Identifier: Apache-2.0
//! Neutral sheet-metal rule from the segment's sheet-metal style record.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::features::{
    sheet_metal::SheetMetalRuleConstruction, DistinctMembers, Feature, FeatureContent,
    FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
};
use cadmpeg_ir::scalar::{FiniteReal, Length, NonNegativeLength, PositiveLength};

use super::style::Style;
use crate::pmdc::PmDcReference;

/// Source tag of a rule feature.
pub(crate) const SOURCE_TAG: &str = "sheet_metal_rule";

/// The rule construction of a style, when its thickness and bend radius
/// resolve to lengths. The neutral factor is the K-factor of the active
/// unfold rule.
pub(crate) fn construction(
    style: &Style,
    length: impl Fn(PmDcReference) -> Option<Length>,
    neutral_factor: Option<FiniteReal>,
) -> Option<SheetMetalRuleConstruction> {
    let thickness = PositiveLength::try_from(length(style.thickness)?).ok()?;
    let inside_radius = NonNegativeLength::try_from(length(style.bend_radius)?).ok()?;
    Some(SheetMetalRuleConstruction {
        thickness: Some(thickness),
        inside_radius: Some(inside_radius),
        neutral_factor,
    })
}

/// The rule slot of an operation that can name a sheet-metal rule.
fn rule_slot(operation: &mut FeatureOperation) -> Option<&mut Option<FeatureId>> {
    match operation {
        FeatureOperation::SheetMetalBaseFlange { rule, .. }
        | FeatureOperation::SheetMetalAttachedPlate { rule, .. }
        | FeatureOperation::SheetMetalBend { rule, .. }
        | FeatureOperation::SheetMetalHem { rule, .. } => Some(rule),
        FeatureOperation::SheetMetalEdgeFlange(construction) => Some(&mut construction.rule),
        FeatureOperation::SheetMetalContourFlange { construction } => Some(&mut construction.rule),
        _ => None,
    }
}

/// Whether a definition is a sheet-metal operation that can name a rule.
pub(crate) fn uses_rule(definition: &FeatureDefinition) -> bool {
    matches!(
        definition,
        FeatureDefinition::Operation(
            FeatureOperation::SheetMetalBaseFlange { .. }
                | FeatureOperation::SheetMetalAttachedPlate { .. }
                | FeatureOperation::SheetMetalBend { .. }
                | FeatureOperation::SheetMetalEdgeFlange { .. }
                | FeatureOperation::SheetMetalHem { .. }
                | FeatureOperation::SheetMetalContourFlange { .. }
        )
    )
}

/// The largest free ordinal below `first`.
fn free_ordinal_before(features: &[Feature], first: u64) -> Option<u64> {
    let used: HashSet<u64> = features.iter().map(|feature| feature.ordinal).collect();
    std::iter::successors(first.checked_sub(1), |ordinal| ordinal.checked_sub(1))
        .take(features.len() + 1)
        .find(|ordinal| !used.contains(ordinal))
}

/// Insert one rule feature before the first sheet-metal operation and name
/// it from every sheet-metal operation. The features are in ordinal order.
pub(in crate::feature) fn project(
    ctx: &DecodeContext<'_>,
    features: &mut Vec<Feature>,
    style: Option<&Style>,
    length: impl Fn(PmDcReference) -> Option<Length>,
    neutral_factor: Option<FiniteReal>,
) -> Result<(), CodecError> {
    let Some(first) = features
        .iter()
        .filter(|feature| uses_rule(feature.evaluation.definition()))
        .map(|feature| feature.ordinal)
        .min()
    else {
        return Ok(());
    };
    let Some(style) = style else {
        return Ok(());
    };
    let Some(construction) = construction(style, length, neutral_factor) else {
        return Ok(());
    };
    let Some(ordinal) = free_ordinal_before(features, first) else {
        return Ok(());
    };
    let id = crate::feature::history_feature_id(&style.identity)?;
    if features.iter().any(|feature| feature.id == id) {
        return Ok(());
    }
    ctx.charge_entities(1, "project Inventor sheet-metal rule")?;
    for feature in features.iter_mut() {
        if !uses_rule(feature.evaluation.definition()) {
            continue;
        }
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(operation) = definition {
                if let Some(slot) = rule_slot(operation) {
                    *slot = Some(id.clone());
                }
            }
        });
        feature.dependencies.insert(id.clone());
    }
    let position = features.partition_point(|feature| feature.ordinal < ordinal);
    features.insert(position, rule_feature(id, ordinal, style, construction));
    Ok(())
}

fn rule_feature(
    id: FeatureId,
    ordinal: u64,
    style: &Style,
    construction: SheetMetalRuleConstruction,
) -> Feature {
    Feature {
        product_definition: None,
        id,
        ordinal,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some(SOURCE_TAG.into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::SheetMetalRule { construction },
        )),
        native_ref: Some(style.id()),
    }
}

#[cfg(test)]
mod tests;
