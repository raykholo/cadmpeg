// SPDX-License-Identifier: Apache-2.0
//! Rectangular-pattern operands and bounded neutral projection.

use super::{
    f64_triple, history, history_feature_id, length_reference, native_feature, resolve_property,
    result_from_collection, PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcPatternFamily, PmDcPatternFeature, ProjectionIndex, WORK_AXIS_TYPE,
};
use crate::pmdc::{content_header, state_content_header, Cursor};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::patterns::{
    LinearPatternDirection, PatternKind, PatternSeed, PatternTransform,
};
use cadmpeg_ir::features::FeatureDirection3;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, FeatureResultTopology};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::PositiveLength;
use std::collections::{HashMap, HashSet};

const SUPPRESSION_TYPE: [u8; 16] = [
    0xbe, 0xf0, 0x6e, 0x8d, 0x35, 0x4c, 0xc7, 0xaa, 0xd7, 0xc8, 0x8d, 0xb7, 0xea, 0x9b, 0x0f, 0xfb,
];
const SELECTION_CONTROL_TYPE: [u8; 16] = [
    0x5a, 0x4a, 0x68, 0x99, 0xd2, 0x11, 0x7f, 0xec, 0x60, 0x00, 0xb0, 0xab, 0xc8, 0x43, 0x3c, 0xb0,
];
const EDGE_DIRECTION_TYPE: [u8; 16] = [
    0x7b, 0x45, 0x44, 0xa2, 0xab, 0x4d, 0x9e, 0x16, 0x85, 0x0c, 0x48, 0xa2, 0x1e, 0xdf, 0x9e, 0x3a,
];
const LINE_DIRECTION_TYPE: [u8; 16] = [
    0xaa, 0x7b, 0x97, 0xa5, 0x70, 0x45, 0x79, 0x58, 0x97, 0xb5, 0x13, 0x96, 0xff, 0xc5, 0xd6, 0x31,
];
const IDENTITY_PLACEMENT: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

#[derive(Debug, Default)]
pub(super) struct Operands {
    records: HashMap<(String, u32), Option<Operand>>,
    raw: HashMap<(String, u32), Option<[u8; 16]>>,
}

#[derive(Debug)]
enum Operand {
    Unresolved,
    Suppression(Vec<[u32; 2]>),
    SelectionControl {
        references: [crate::pmdc::PmDcReference; 2],
        value: u8,
    },
    Line {
        reverse: crate::pmdc::PmDcReference,
        geometry: crate::pmdc::PmDcReference,
        links: Vec<crate::pmdc::PmDcReference>,
        span: Vector3,
    },
}

pub(super) fn is_operand(type_id: [u8; 16]) -> bool {
    matches!(
        type_id,
        SUPPRESSION_TYPE | SELECTION_CONTROL_TYPE | EDGE_DIRECTION_TYPE | LINE_DIRECTION_TYPE
    )
}

impl Operands {
    pub(super) fn note_record(&mut self, token: &str, ordinal: u32, type_id: [u8; 16]) {
        self.raw
            .entry((token.to_owned(), ordinal))
            .and_modify(|value| *value = None)
            .or_insert(Some(type_id));
    }

    fn resolves(&self, token: &str, reference: crate::pmdc::PmDcReference) -> bool {
        reference.record_ordinal().is_some_and(|ordinal| {
            self.raw
                .get(&(token.to_owned(), ordinal))
                .is_some_and(Option::is_some)
        })
    }

    pub(super) fn read(
        &mut self,
        ctx: &DecodeContext<'_>,
        token: &str,
        ordinal: u32,
        type_id: [u8; 16],
        source: View<'_>,
        version: u8,
    ) -> Result<(), CodecError> {
        let operand = parse(ctx, type_id, source, version)?;
        ctx.charge_entities(1, "admit Inventor rectangular-pattern operand")?;
        match self.records.entry((token.to_owned(), ordinal)) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(operand));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.insert(None);
            }
        }
        Ok(())
    }

    pub(super) fn validate_suppression(
        &self,
        source: &PmDcPatternFeature,
        properties: &[PmDcFeatureProperty],
    ) -> Result<(), CodecError> {
        let Some(reference) = slot(source, 1) else {
            return Ok(());
        };
        let Some(Operand::Suppression(pairs)) =
            self.get(source.identity.segment_token.as_str(), reference)
        else {
            return Ok(());
        };
        let count = |number| {
            let ordinal = slot(source, number)?.record_ordinal()?;
            let mut records = properties.iter().filter(|record| {
                record.identity.segment_token == source.identity.segment_token
                    && record.identity.record_ordinal == ordinal
            });
            let record = records.next()?;
            if records.next().is_some() {
                return None;
            }
            match record.kind {
                PmDcFeaturePropertyKind::RdxVariable { model_value, .. } if model_value > 0 => {
                    Some(model_value)
                }
                _ => None,
            }
        };
        let base = if source.save_version_major >= 21 {
            18
        } else {
            12
        };
        let first = count(base).ok_or_else(|| {
            CodecError::malformed("Inventor pattern first instance count is unresolved or zero")
        })?;
        let second = if slot(source, base + 1).is_some_and(|reference| reference.index() == 0) {
            1
        } else {
            count(base + 1).ok_or_else(|| {
                CodecError::malformed(
                    "Inventor pattern second instance count is unresolved or zero",
                )
            })?
        };
        if pairs.iter().any(|pair| pair[0] > first || pair[1] > second) {
            return Err(CodecError::malformed(
                "Inventor pattern suppression coordinate exceeds its instance count",
            ));
        }
        Ok(())
    }

    fn get(&self, token: &str, reference: crate::pmdc::PmDcReference) -> Option<&Operand> {
        if !self.resolves(token, reference) {
            return None;
        }
        self.records
            .get(&(token.to_owned(), reference.record_ordinal()?))?
            .as_ref()
    }
}

fn parse(
    ctx: &DecodeContext<'_>,
    type_id: [u8; 16],
    source: View<'_>,
    version: u8,
) -> Result<Operand, CodecError> {
    let mut cursor = Cursor::new(source);
    if matches!(type_id, SUPPRESSION_TYPE | SELECTION_CONTROL_TYPE) {
        content_header(&mut cursor, version)?;
    } else {
        state_content_header(&mut cursor, version)?;
    }
    let operand = match type_id {
        SUPPRESSION_TYPE => {
            if [
                cursor.u16("suppression marker")?,
                cursor.u16("suppression marker")?,
            ] != [4, 0x3000]
            {
                return Err(CodecError::malformed(
                    "Inventor pattern suppression marker is invalid",
                ));
            }
            let count = cursor.u32("suppression count")?;
            if count != 0 && cursor.u32_array::<2>("suppression metadata")? != [0, 0] {
                return Err(CodecError::malformed(
                    "Inventor pattern suppression metadata is invalid",
                ));
            }
            let count = cadmpeg_core::decode::bounded_len(u64::from(count), 8, cursor.remaining())
                .ok_or_else(|| {
                    CodecError::malformed("Inventor pattern suppression count exceeds payload")
                })?;
            let mut pairs =
                ctx.alloc_filled(count, [0; 2], "read Inventor pattern suppressions")?;
            for pair in &mut pairs {
                *pair = cursor.u32_array("suppressed instance coordinates")?;
            }
            cursor.finish("pattern suppressions")?;
            let mut distinct = HashSet::new();
            if pairs
                .iter()
                .any(|pair| pair.contains(&0) || *pair == [1, 1] || !distinct.insert(*pair))
            {
                return Err(CodecError::malformed(
                    "Inventor pattern suppression coordinates are invalid",
                ));
            }
            return Ok(Operand::Suppression(pairs));
        }
        SELECTION_CONTROL_TYPE => Operand::SelectionControl {
            references: [
                cursor.reference("pattern selection control reference")?,
                cursor.reference("pattern selection control reference")?,
            ],
            value: cursor.u8("pattern selection control value")?,
        },
        EDGE_DIRECTION_TYPE | LINE_DIRECTION_TYPE => {
            let state = cursor.i32("pattern selection state")?;
            let reverse = cursor.reference("pattern selection reversal")?;
            let geometry = cursor.reference("pattern selection geometry")?;
            let selection_form = cursor.u32("pattern selection form")?;
            let zero = cursor.u16("pattern selection value")?;
            let curve_form = cursor.u32("pattern selection curve form")?;
            if selection_form != 1 || curve_form != 23 {
                return Ok(Operand::Unresolved);
            }
            if (state != 1 && state != -1) || zero != 0 {
                return Err(CodecError::malformed(
                    "Inventor pattern line selection prefix is invalid",
                ));
            }
            f64_triple(&mut cursor, "pattern line origin")?;
            let span = f64_triple(&mut cursor, "pattern line span")?;
            f64_triple(&mut cursor, "pattern line selection point")?;
            if cursor.u8("pattern line state")? != 1 {
                return Err(CodecError::malformed(
                    "Inventor pattern line state is invalid",
                ));
            }
            let mut links = vec![cursor.reference("pattern line source")?];
            if type_id == EDGE_DIRECTION_TYPE {
                links.push(cursor.reference("pattern line source")?);
            }
            let span = Vector3::new(span[0].get(), span[1].get(), span[2].get());
            if span == Vector3::new(0.0, 0.0, 0.0) {
                return Err(CodecError::malformed("Inventor pattern line span is zero"));
            }
            Operand::Line {
                reverse,
                geometry,
                links,
                span,
            }
        }
        _ => {
            return Err(CodecError::malformed(
                "Inventor pattern operand type is invalid",
            ))
        }
    };
    cursor.finish("pattern operand")?;
    Ok(operand)
}

fn slot(source: &PmDcPatternFeature, slot: usize) -> Option<crate::pmdc::PmDcReference> {
    source.property_slots.get(slot).copied()
}

fn property_at<'a>(
    source: &PmDcPatternFeature,
    slot_number: usize,
    index: &'a ProjectionIndex<'a>,
) -> Option<&'a PmDcFeatureProperty> {
    resolve_property(
        source.identity.segment_token.as_str(),
        slot(source, slot_number)?.index(),
        index,
    )
}

fn bool_at(
    source: &PmDcPatternFeature,
    slot_number: usize,
    index: &ProjectionIndex<'_>,
) -> Option<bool> {
    match property_at(source, slot_number, index)?.kind {
        PmDcFeaturePropertyKind::Boolean { value, .. } => Some(value),
        _ => None,
    }
}

fn count_at(
    source: &PmDcPatternFeature,
    slot_number: usize,
    index: &ProjectionIndex<'_>,
) -> Option<u32> {
    match property_at(source, slot_number, index)?.kind {
        PmDcFeaturePropertyKind::RdxVariable { model_value, .. } if model_value >= 1 => {
            Some(model_value)
        }
        _ => None,
    }
}

fn direction(
    source: &PmDcPatternFeature,
    slot_number: usize,
    index: &ProjectionIndex<'_>,
    operands: &Operands,
) -> Option<FeatureDirection3> {
    let token = source.identity.segment_token.as_str();
    let reference = slot(source, slot_number)?;
    if !operands.resolves(token, reference) {
        return None;
    }
    if let Some(record) = index
        .directions
        .get(&(token, reference.record_ordinal()?))
        .and_then(Option::as_ref)
    {
        return cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(
            record.direction[0].get(),
            record.direction[1].get(),
            record.direction[2].get(),
        ))
        .map(FeatureDirection3::from);
    }
    if let Some(record) = resolve_property(token, reference.index(), index) {
        if let PmDcFeaturePropertyKind::WorkAxis { span, .. } = record.kind {
            return cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(
                span[0].get(),
                span[1].get(),
                span[2].get(),
            ))
            .map(FeatureDirection3::from);
        }
    }
    let Operand::Line {
        reverse,
        geometry,
        links,
        span,
    } = operands.get(token, reference)?
    else {
        return None;
    };
    if !operands.resolves(token, *geometry)
        || !links.iter().all(|link| operands.resolves(token, *link))
    {
        return None;
    }
    let vector_span = if operands
        .raw
        .get(&(token.to_owned(), geometry.record_ordinal()?))
        == Some(&Some(WORK_AXIS_TYPE))
    {
        let PmDcFeaturePropertyKind::WorkAxis { span, .. } =
            resolve_property(token, geometry.index(), index)?.kind
        else {
            return None;
        };
        Vector3::new(span[0].get(), span[1].get(), span[2].get())
    } else {
        *span
    };
    let mut vector = cadmpeg_ir::units::UnitVector3::normalized(vector_span)?;
    match resolve_property(token, reverse.index(), index)?.kind {
        PmDcFeaturePropertyKind::Boolean { value: true, .. } => vector = vector.reversed(),
        PmDcFeaturePropertyKind::Boolean { value: false, .. } => {}
        _ => return None,
    }
    Some(vector.into())
}

fn axis(
    source: &PmDcPatternFeature,
    axis: usize,
    index: &ProjectionIndex<'_>,
    operands: &Operands,
) -> Option<LinearPatternDirection> {
    let base = if source.save_version_major >= 21 {
        18
    } else {
        12
    };
    let count = count_at(source, base + axis, index)?;
    let spacing = PositiveLength::try_from(length_reference(
        source.identity.segment_token.as_str(),
        slot(source, base + 2 + axis)?.index(),
        index,
    )?)
    .ok()?;
    let direction = direction(source, base + 4 + axis, index, operands)?;
    if [6, 8, 10, 12]
        .into_iter()
        .any(|offset| bool_at(source, base + offset + axis, index) != Some(false))
    {
        return None;
    }
    Some(LinearPatternDirection {
        direction,
        spacing,
        count,
    })
}

fn fixed_feature_header(source: &PmDcPatternFeature) -> bool {
    source.family == PmDcPatternFamily::Rectangular
        && source.control == 0
        && source.properties.references().is_empty()
        && source.extension_values.iter().all(|value| *value == 0)
        && [3, 7, 8, 10]
            .into_iter()
            .all(|number| slot(source, number).is_some_and(|reference| reference.index() == 0))
        && (source.save_version_major < 21
            || (12..18)
                .all(|number| slot(source, number).is_some_and(|reference| reference.index() == 0)))
}

fn fixed_feature_properties<'a>(
    property: impl Fn(usize) -> Option<&'a PmDcFeatureProperty>,
) -> bool {
    [6, 11].into_iter().all(|number| {
        matches!(
            property(number).map(|record| &record.kind),
            Some(PmDcFeaturePropertyKind::Boolean { value: false, .. })
        )
    }) && matches!(
        property(2).map(|record| &record.kind),
        Some(PmDcFeaturePropertyKind::RdxVariable { model_value: 0, .. })
    ) && matches!(
        property(5).map(|record| &record.kind),
        Some(PmDcFeaturePropertyKind::References {
            family: super::PmDcFeatureReferenceFamily::FeatureDimensions,
            ..
        })
    )
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    source: &PmDcPatternFeature,
    label: Option<&PmDcFeatureLabel>,
    index: &ProjectionIndex<'_>,
    operands: &Operands,
    history: &history::History,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    if !fixed_feature_header(source)
        || !fixed_feature_properties(|number| property_at(source, number, index))
    {
        return None;
    }
    let token = source.identity.segment_token.as_str();
    let Operand::Suppression(suppressed) = operands.get(token, slot(source, 1)?)? else {
        return None;
    };
    if !suppressed.is_empty() {
        return None;
    }
    let Operand::SelectionControl { references, value } = operands.get(token, slot(source, 4)?)?
    else {
        return None;
    };
    if *value != 0 || references.iter().any(|reference| reference.index() != 0) {
        return None;
    }
    let placement_transform = index
        .transforms
        .get(&(token, slot(source, 0)?.record_ordinal()?))?
        .as_ref()?;
    if placement_transform.matrix.rows() != IDENTITY_PLACEMENT {
        return None;
    }
    let first = axis(source, 0, index, operands)?;
    let base = if source.save_version_major >= 21 {
        18
    } else {
        12
    };
    let second = if slot(source, base + 1)?.index() == 0 {
        None
    } else {
        Some(axis(source, 1, index, operands)?)
    };
    let count = first
        .count
        .checked_mul(second.as_ref().map_or(1, |axis| axis.count))?;
    if count < 2 {
        return None;
    }
    let placement = history.placement(source.identity.record_ordinal)?;
    if !placement.model_history {
        return None;
    }
    let mut seeds = Vec::new();
    let mut distinct = HashSet::new();
    for reference in source.participants.references() {
        if !operands.resolves(token, *reference) {
            return None;
        }
        let ordinal = reference.record_ordinal()?;
        let seed = history.placement(ordinal)?;
        if !distinct.insert(ordinal)
            || seed.kind == history::HistoryNodeKind::Sketch
            || seed.ordinal >= placement.ordinal
            || seed.model_history != placement.model_history
        {
            return None;
        }
        let mut identity = source.identity.clone();
        identity.record_ordinal = ordinal;
        let id = match history_feature_id(&identity) {
            Ok(id) => id,
            Err(error) => return Some(Err(error)),
        };
        seeds.push(PatternSeed::Feature(id));
    }
    if seeds.is_empty() {
        return None;
    }
    let pattern = PatternKind::new(PatternTransform::Linear {
        direction: Some(first.direction),
        spacing: first.spacing,
        count: first.count,
        second,
    })
    .ok()?;
    let collection = property_at(source, 9, index)?;
    let (id, result) = match result_from_collection(ctx, &source.identity, collection, index)? {
        Ok(result) => result,
        Err(error) => return Some(Err(error)),
    };
    let mut feature = match native_feature(ctx, &source.identity, label, source.id()) {
        Ok(feature) => feature,
        Err(error) => return Some(Err(error)),
    };
    feature.id = id;
    feature.source_tag = Some("rectangular_pattern".into());
    feature.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }),
    );
    Some(Ok((feature, result)))
}

pub(crate) fn is_linear_pattern(feature: &Feature) -> bool {
    matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. })
            if matches!(pattern.definition(), PatternTransform::Linear { .. })
    )
}

pub(crate) fn matches_source(
    source: &PmDcPatternFeature,
    feature: &Feature,
    properties: &[PmDcFeatureProperty],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    transforms: &[crate::sketch::PmDcTransform],
    results: &[FeatureResultTopology],
) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
        feature.evaluation.definition()
    else {
        return false;
    };
    let PatternTransform::Linear {
        direction,
        spacing,
        count,
        second,
    } = pattern.definition()
    else {
        return false;
    };
    if !fixed_feature_header(source) || direction.is_none() {
        return false;
    }
    let by_record = cadmpeg_core::decode::unique_index(properties.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let property = |number| {
        let ordinal = slot(source, number)?.record_ordinal()?;
        by_record
            .get(&(source.identity.segment_token.as_str(), ordinal))?
            .as_ref()
            .copied()
    };
    if !fixed_feature_properties(property) {
        return false;
    }
    let Some(placement) = slot(source, 0).and_then(crate::pmdc::PmDcReference::record_ordinal)
    else {
        return false;
    };
    let mut placements = transforms.iter().filter(|record| {
        record.identity.segment_token == source.identity.segment_token
            && record.identity.record_ordinal == placement
    });
    if placements
        .next()
        .is_none_or(|record| record.matrix.rows() != IDENTITY_PLACEMENT)
        || placements.next().is_some()
    {
        return false;
    }
    let base = if source.save_version_major >= 21 {
        18
    } else {
        12
    };
    let expected_count = |number| match property(number)?.kind {
        PmDcFeaturePropertyKind::RdxVariable { model_value, .. } => Some(model_value),
        _ => None,
    };
    let expected_spacing = |number| {
        let ordinal = slot(source, number)?.record_ordinal()?;
        let key = format!(
            "inventor:pmdc:parameter#{}-{ordinal}",
            source.identity.segment_token
        );
        let mut values = parameters
            .iter()
            .filter(|parameter| parameter.native_ref.as_deref() == Some(key.as_str()));
        let parameter = values.next()?;
        if values.next().is_some() {
            return None;
        }
        match parameter.value {
            Some(cadmpeg_ir::features::ParameterValue::Length(value)) => Some(value.get()),
            _ => None,
        }
    };
    if expected_count(base) != Some(*count) || expected_spacing(base + 2) != Some(spacing.get()) {
        return false;
    }
    match (
        slot(source, base + 1).map(crate::pmdc::PmDcReference::index),
        second,
    ) {
        (Some(0), None) => {}
        (Some(reference), Some(axis))
            if reference != 0
                && expected_count(base + 1) == Some(axis.count)
                && expected_spacing(base + 3) == Some(axis.spacing.get()) => {}
        _ => return false,
    }
    let axes = if second.is_some() { 2 } else { 1 };
    if (0..axes).any(|axis| {
        [6, 8, 10, 12].into_iter().any(|offset| {
            !matches!(
                property(base + offset + axis).map(|record| &record.kind),
                Some(PmDcFeaturePropertyKind::Boolean { value: false, .. })
            )
        })
    }) || count
        .checked_mul(second.as_ref().map_or(1, |axis| axis.count))
        .is_none_or(|total| total < 2)
        || seeds.is_empty()
        || seeds.len() != source.participants.references().len()
    {
        return false;
    }
    let mut distinct = HashSet::new();
    for (seed, reference) in seeds.iter().zip(source.participants.references()) {
        let Some(ordinal) = reference.record_ordinal() else {
            return false;
        };
        if !distinct.insert(ordinal) {
            return false;
        }
        let mut identity = source.identity.clone();
        identity.record_ordinal = ordinal;
        if history_feature_id(&identity)
            .ok()
            .as_ref()
            .is_none_or(|id| !matches!(seed, PatternSeed::Feature(actual) if actual == id))
        {
            return false;
        }
    }
    let Some(collection) = property(9) else {
        return false;
    };
    if !matches!(
        collection.kind,
        PmDcFeaturePropertyKind::References {
            family: super::PmDcFeatureReferenceFamily::ObjectCollection,
            ..
        }
    ) {
        return false;
    }
    let mut feature_results = results
        .iter()
        .filter(|result| result.output_of == feature.id);
    let Some(result) = feature_results.next() else {
        return false;
    };
    feature_results.next().is_none()
        && result.native_ref.as_deref() == Some(collection.id().as_str())
}

#[cfg(test)]
mod tests;
