// SPDX-License-Identifier: Apache-2.0
//! Bind the tools of an operand-list `Combine` that build in another component.
//!
//! A tool body that a component builds in its own BREP history is not a body
//! of the history that holds the `Combine` state. Its browser node still names
//! the body-map entity of the tool and the scope that creates it, and the
//! body-map pair of that entity is a history-only pair.

use std::collections::HashMap;

use cadmpeg_ir::features::{
    BodySelection, Feature, FeatureDefinition, FeatureId, FeatureOperation, GeneratedBodyRef,
};
use cadmpeg_ir::report::loss::LossNote;

use crate::design::feature_project::ScopeHistoryGraph;
use crate::loss::F3dLossCode;
use crate::records::bodies::DesignBodyBinding;
use crate::records::feature::combine::{DesignCombineBodySelection, DesignCombineForm};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::recipes::DesignComponentNamingSpace;

/// The scope that creates a derived component's body, as a Design record kind.
fn is_derived_base(scope: &DesignParameterScope) -> bool {
    matches!(
        scope.kind_name(),
        "Derived Base Feature" | "DerivedBaseFeature"
    )
}

/// Feature that regenerates the history-only body of one tool.
fn generating_feature<'a>(
    creator: &DesignParameterScope,
    scopes: &'a [DesignParameterScope],
    naming_spaces: &[DesignComponentNamingSpace],
    by_native: &'a HashMap<&str, &Feature>,
) -> Result<&'a Feature, &'static str> {
    if let Some(feature) = by_native.get(creator.id.as_str()) {
        return Ok(feature);
    }
    if !is_derived_base(creator) {
        return Err("the creating scope is not a projected feature");
    }
    let space = ScopeHistoryGraph::component_namespace(creator, naming_spaces)
        .ok_or("the creating scope has no component naming space")?;
    let stream = crate::ids::native_stream(&creator.id);
    let mut instances = scopes.iter().filter(|scope| {
        scope.kind() == DesignFeatureKind::DerivedInstance
            && crate::ids::native_stream(&scope.id) == stream
            && ScopeHistoryGraph::component_namespace(scope, naming_spaces) == Some(space)
    });
    let (Some(instance), None) = (instances.next(), instances.next()) else {
        return Err("the component naming space of the creating scope has no single DerivedInstance scope");
    };
    by_native
        .get(instance.id.as_str())
        .copied()
        .ok_or("the DerivedInstance scope of the creating component is not a projected feature")
}

/// Resolve every tool of `scope` to a generated body, or name the reason.
fn resolve_tools(
    scope: &DesignParameterScope,
    combine_ordinal: u64,
    inputs: &Inputs<'_>,
) -> Result<Vec<(FeatureId, String)>, &'static str> {
    let operation = scope
        .combine_operation()
        .ok_or("the scope stores no Combine operation")?;
    if operation.form != DesignCombineForm::OperandLists {
        return Err("the Combine is not in the operand-list form");
    }
    let stream = crate::ids::native_stream(&scope.id);
    let mut resolved = Vec::<(FeatureId, String)>::new();
    for tool in operation.tools.iter() {
        let DesignCombineBodySelection {
            body_node: Some(node),
            ..
        } = tool
        else {
            return Err("a tool stores no browser node");
        };
        let mut pairs = inputs.bindings.iter().filter(|binding| {
            crate::ids::native_stream(binding.id()) == stream
                && binding.entity_suffix == u64::from(node.body_entity_suffix)
        });
        let (Some(pair), None) = (pairs.next(), pairs.next()) else {
            return Err("the body entity of a tool has no single body-map pair");
        };
        if !pair.history_only {
            return Err("the body-map pair of a tool is not history-only");
        }
        let mut creators = inputs.scopes.iter().filter(|candidate| {
            candidate.record_index == node.creating_scope_record_index
                && crate::ids::native_stream(&candidate.id) == stream
        });
        let (Some(creator), None) = (creators.next(), creators.next()) else {
            return Err("the creating scope of a tool is not a single scope");
        };
        let feature = generating_feature(
            creator,
            inputs.scopes,
            inputs.naming_spaces,
            &inputs.by_native,
        )?;
        if feature.ordinal >= combine_ordinal {
            return Err("the feature that generates a tool does not precede the Combine");
        }
        let local = pair.id().clone();
        if resolved.iter().any(|(_, existing)| existing == &local) {
            return Err("two tools name the same body");
        }
        resolved.push((feature.id.clone(), local));
    }
    Ok(resolved)
}

struct Inputs<'a> {
    scopes: &'a [DesignParameterScope],
    bindings: &'a [DesignBodyBinding],
    naming_spaces: &'a [DesignComponentNamingSpace],
    by_native: HashMap<&'a str, &'a Feature>,
}

/// Replace the unresolved tools of each operand-list `Combine` with the bodies
/// that other components generate, and record the generating features as
/// dependencies of the `Combine`.
pub(crate) fn bind(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    bindings: &[DesignBodyBinding],
    naming_spaces: &[DesignComponentNamingSpace],
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    let mut updates = Vec::<(usize, Vec<(FeatureId, String)>)>::new();
    {
        let inputs = Inputs {
            scopes,
            bindings,
            naming_spaces,
            by_native: features
                .iter()
                .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature)))
                .collect(),
        };
        for (index, feature) in features.iter().enumerate() {
            let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                feature.evaluation.definition()
            else {
                continue;
            };
            if !matches!(
                operands.tools(),
                BodySelection::Native(_) | BodySelection::NativeSet(_)
            ) {
                continue;
            }
            let Some(native_ref) = feature.native_ref.as_deref() else {
                continue;
            };
            let mut matching = scopes.iter().filter(|scope| scope.id == native_ref);
            let (Some(scope), None) = (matching.next(), matching.next()) else {
                continue;
            };
            let Some(operation) = scope.combine_operation() else {
                continue;
            };
            if operation.form != DesignCombineForm::OperandLists {
                continue;
            }
            match resolve_tools(scope, feature.ordinal, &inputs) {
                Ok(tools) => updates.push((index, tools)),
                Err(reason) => losses.push(F3dLossCode::FeatureCombineToolBodyUnbound.note(
                    format!(
                        "feature {} retains its native Combine tools: {reason}",
                        feature.id
                    ),
                )),
            }
        }
    }
    for (index, tools) in updates {
        let feature = &mut features[index];
        let Some(native) = feature.native_ref.clone() else {
            continue;
        };
        let mut refs = Vec::new();
        for (generator, local) in &tools {
            let Ok(reference) = GeneratedBodyRef::new(generator.clone(), local.clone()) else {
                refs.clear();
                break;
            };
            refs.push(reference);
        }
        let Ok(selection) = BodySelection::generated(refs, native) else {
            losses.push(F3dLossCode::FeatureCombineToolBodyUnbound.note(format!(
                "feature {} retains its native Combine tools: the generated-body selection is not admitted",
                feature.id
            )));
            continue;
        };
        let mut refusal = None;
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                definition
            else {
                return;
            };
            refusal = operands
                .try_edit(|_, operand_tools| *operand_tools = selection)
                .err();
        });
        if let Some(error) = refusal {
            losses.push(F3dLossCode::FeatureCombineToolBodyUnbound.note(format!(
                "feature {} retains its native Combine tools: {error}",
                feature.id
            )));
            continue;
        }
        for (generator, _) in tools {
            if generator != feature.id {
                feature.dependencies.insert(generator);
            }
        }
    }
    losses
}

#[cfg(test)]
mod tests;
