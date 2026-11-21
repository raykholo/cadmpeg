// SPDX-License-Identifier: Apache-2.0
//! Resolve pattern-owned archive seed lists to history features.

use std::collections::{HashMap, HashSet};

use crate::carchive::PatternSeeds;
use crate::loss::SldprtLossCode;
use crate::records::{FeatureHistory, FeatureInputLane};
use cadmpeg_ir::features::{
    patterns::PatternSeed, Feature, FeatureDefinition, FeatureId, FeatureOperation,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

#[derive(Default)]
pub(crate) struct Selections(HashMap<String, Vec<PatternSeeds>>);

impl Selections {
    pub(crate) fn insert(&mut self, lane: String, seeds: Vec<PatternSeeds>) {
        self.0.insert(lane, seeds);
    }

    pub(crate) fn bind(
        &self,
        ir: &mut CadIr,
        histories: &[FeatureHistory],
        lanes: &[FeatureInputLane],
        losses: &mut Vec<LossNote>,
    ) {
        let assignments = crate::history::configuration::configuration_lane_assignments(
            &ir.model.configurations,
            lanes,
        );
        let mut active = Vec::new();
        for (configuration_index, lane_index) in assignments {
            let lane = &lanes[lane_index];
            let bindings = resolve(
                &ir.model.features,
                histories,
                self.0.get(&lane.id).map(Vec::as_slice).unwrap_or_default(),
                &lane.id,
            );
            let configuration = &mut ir.model.configurations[configuration_index];
            for (owner, seeds) in &bindings {
                if let Some(state) = configuration.feature_states.get_mut(owner) {
                    install(&mut state.definition, seeds);
                    for seed in seeds {
                        if let PatternSeed::Feature(seed) = seed {
                            state.dependencies.insert(seed.clone());
                        }
                    }
                }
            }
            if configuration.active {
                active.push(bindings);
            }
        }
        let bindings = if active.len() == 1 {
            active.pop()
        } else {
            None
        }
        .or_else(|| {
            if !ir.model.configurations.is_empty() {
                return None;
            }
            let mut sources = lanes
                .iter()
                .filter_map(|lane| self.0.get(&lane.id).map(|records| (&lane.id, records)));
            let (lane, records) = sources.next()?;
            sources
                .next()
                .is_none()
                .then(|| resolve(&ir.model.features, histories, records, lane))
        });
        if let Some(bindings) = bindings {
            for feature in &mut ir.model.features {
                if let Some(seeds) = bindings.get(&feature.id) {
                    feature
                        .evaluation
                        .edit(|definition, _| install(definition, seeds));
                    for seed in seeds {
                        if let PatternSeed::Feature(seed) = seed {
                            feature.dependencies.insert(seed.clone());
                        }
                    }
                }
            }
        }
        for feature in &ir.model.features {
            if !matches!(feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) if seeds.is_empty() || seeds.iter().any(|seed| matches!(seed, PatternSeed::Native(_))))
            {
                continue;
            }
            let source = histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|native| {
                    feature.native_ref.as_deref() == Some(native.id.as_str())
                        && matches!(
                            native.input_class.as_deref(),
                            Some("moLPattern_c" | "moMirrorPattern_c")
                        )
                })
                .and_then(crate::records::Feature::source_value);
            let record = source.and_then(|source| {
                self.0
                    .values()
                    .flatten()
                    .filter(|record| record.owner == source)
                    .min_by_key(|record| record.at)
            });
            if let Some(record) = record {
                losses.push(SldprtLossCode::FeaturePatternSeedsUnresolved.note(format!(
                    "Pattern {} retains its seed selection at byte {} in the native feature-input stream",
                    feature.id, record.at
                )));
            }
        }
    }
}

fn install(definition: &mut FeatureDefinition, sources: &[PatternSeed]) {
    if let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = definition {
        *seeds = sources.to_vec();
    }
}

fn resolve(
    features: &[Feature],
    histories: &[FeatureHistory],
    records: &[PatternSeeds],
    lane: &str,
) -> HashMap<FeatureId, Vec<PatternSeed>> {
    let mut by_native = HashMap::<&str, HashSet<&FeatureId>>::new();
    for feature in features {
        if let Some(native) = feature.native_ref.as_deref() {
            by_native.entry(native).or_default().insert(&feature.id);
        }
    }
    let mut by_source = HashMap::<u32, HashSet<&FeatureId>>::new();
    for native in histories.iter().flat_map(|history| &history.features) {
        if let (Some(source), Some(ids)) =
            (native.source_value(), by_native.get(native.id.as_str()))
        {
            by_source.entry(source).or_default().extend(ids);
        }
    }
    let unique = |source: &u32| {
        let ids = by_source.get(source)?;
        if ids.len() != 1 {
            return None;
        }
        ids.iter().next().map(|id| (*id).clone())
    };
    let mut result = HashMap::new();
    for record in records {
        if records
            .iter()
            .filter(|other| other.owner == record.owner)
            .count()
            != 1
        {
            continue;
        }
        let Some(owner) = unique(&record.owner) else {
            continue;
        };
        let Some(sources) = &record.seeds else {
            continue;
        };
        if sources.is_empty() {
            continue;
        }
        let seeds = sources
            .iter()
            .enumerate()
            .map(|(slot, source)| {
                match source
                    .as_ref()
                    .and_then(unique)
                    .filter(|seed| seed != &owner)
                {
                    Some(seed) => PatternSeed::Feature(seed),
                    None => PatternSeed::Native(match source {
                        Some(source) => {
                            format!("{lane}@seed-list:{}:{slot}:source={source}", record.at)
                        }
                        None => format!("{lane}@seed-list:{}:{slot}", record.at),
                    }),
                }
            })
            .collect();
        result.insert(owner, seeds);
    }
    result
}

#[cfg(test)]
mod tests;
