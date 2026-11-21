//! Archive input references and datum-axis records of solid revolutions.

use super::scalars::FeatureObjectNames;
use crate::carchive::{
    reader::{Reader, Stop},
    NamedObject,
};
use crate::layout::revolution_datum_axis_tail as axis_tail;
use crate::records::{FeatureHistory, FeatureInputLane, FeatureInputName, ObjectId};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::{SumSquaresUnitVector3, UnitVector3};
use std::collections::{BTreeMap, HashMap, HashSet};

type Properties = BTreeMap<cadmpeg_core::text::NonBlankString, String>;

const NATIVE_TO_MM: f64 = 1000.0;

fn profile_index(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    name: &FeatureInputName,
) -> Result<Option<u32>, cadmpeg_core::CodecError> {
    let Some(offset) = usize::try_from(name.offset)
        .ok()
        .and_then(|offset| offset.checked_add(2))
    else {
        return Ok(None);
    };
    let Some(source) = name.object_id.and_then(ObjectId::value) else {
        return Ok(None);
    };
    let mut reader = Reader::new(ctx, &lane.native_payload, offset, HashMap::new(), 0, None);
    let mut read = || -> Result<Option<u32>, Stop> {
        if reader.cstring()? != name.value || reader.u32()? != 0 {
            return Ok(None);
        }
        reader.u16()?;
        reader.u8()?;
        reader.u8()?;
        if reader.u32()? != source || reader.u32()? != 0 {
            return Ok(None);
        }
        reader.cstring()?;
        if reader.u32()? != 0 || reader.u16()? != 0 || reader.u16()? != 1 {
            return Ok(None);
        }
        Ok(match reader.u16()? {
            index @ 1..0x7fff => Some(u32::from(index)),
            0x7fff => Some(reader.u32()?).filter(|index| *index > 0 && *index < 0x8000_0000),
            _ => None,
        })
    };
    match read() {
        Ok(index) => Ok(index),
        Err(Stop::Refused(limit)) => Err(limit.into()),
        Err(_) => Ok(None),
    }
}

fn datum_axis_frame(frame: &[u8], axes: &HashSet<u32>) -> Option<(FinitePoint3, UnitVector3)> {
    if frame.len() != axis_tail::LEN
        || !axes.contains(&View::u32_le_at(frame, axis_tail::SOURCE)?)
        || View::u16_le_at(frame, axis_tail::ZERO_WORD)? != 0
        || View::u32_le_at(frame, axis_tail::KIND)? != 1
        || frame.get(axis_tail::ZERO_KIND..axis_tail::HANDLE_WORD)? != [0; 4]
        || frame.get(axis_tail::ZERO_HANDLE_WORD..axis_tail::HANDLES)? != [0; 12]
        || ![0, 4, 8]
            .into_iter()
            .all(|offset| View::i32_le_at(frame, axis_tail::HANDLES + offset) == Some(-12345))
        || frame.get(axis_tail::ZERO_HANDLES..axis_tail::VERSION)? != [0; 4]
        || View::u32_le_at(frame, axis_tail::VERSION)? < 16000
        || frame.get(axis_tail::ZERO_VERSION..axis_tail::SENTINEL)? != [0; 12]
        || View::i32_le_at(frame, axis_tail::SENTINEL)? != -1
        || frame.get(axis_tail::ZERO_GEOMETRY..axis_tail::AUXILIARY)? != [0; 8]
        || frame[axis_tail::ZERO_TAIL] != 0
    {
        return None;
    }
    let origin = FinitePoint3::new(Point3::new(
        View::f64_le_at(frame, axis_tail::ORIGIN)? * NATIVE_TO_MM,
        View::f64_le_at(frame, axis_tail::ORIGIN + 8)? * NATIVE_TO_MM,
        View::f64_le_at(frame, axis_tail::ORIGIN + 16)? * NATIVE_TO_MM,
    ))?;
    let direction = SumSquaresUnitVector3::new(Vector3::new(
        View::f64_le_at(frame, axis_tail::DIRECTION)?,
        View::f64_le_at(frame, axis_tail::DIRECTION + 8)?,
        View::f64_le_at(frame, axis_tail::DIRECTION + 16)?,
    ))?
    .normalized();
    Some((origin, direction))
}

fn datum_axis(
    lane: &FeatureInputLane,
    start: u64,
    end: u64,
    axes: &HashSet<u32>,
) -> Option<(FinitePoint3, UnitVector3)> {
    let classes = lane
        .classes
        .iter()
        .filter(|class| class.offset >= start && class.offset < end)
        .collect::<Vec<_>>();
    let mut candidates = classes.windows(3).filter_map(|classes| {
        let [line, component, termination] = classes else {
            return None;
        };
        if line.name != "moLineRef_w"
            || component.name != "moCompRefAxis_c"
            || termination.name != "moRevEndSpec_c"
        {
            return None;
        }
        let finish = usize::try_from(termination.offset).ok()?;
        let begin = finish.checked_sub(axis_tail::LEN)?;
        if u64_from_index(begin)
            < component
                .offset
                .checked_add(6 + u64_from_index(component.name.len()))?
        {
            return None;
        }
        datum_axis_frame(lane.native_payload.get(begin..finish)?, axes)
    });
    let first = candidates.next()?;
    candidates
        .all(|candidate| candidate == first)
        .then_some(first)
}

fn lane_inputs(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
    lane: &FeatureInputLane,
    objects: &[NamedObject],
) -> Result<HashMap<String, Properties>, cadmpeg_core::CodecError> {
    let is_revolution = |class: &str| matches!(class, "moRevolution_c" | "moRevCut_c");
    if !lane.classes.iter().any(|class| is_revolution(&class.name))
        && !objects.iter().any(|object| is_revolution(&object.class))
    {
        return Ok(HashMap::new());
    }
    let mut result = HashMap::new();
    let names = FeatureObjectNames::new(&lane.names);
    let class_of = |name: &FeatureInputName| {
        lane.classes
            .iter()
            .find(|class| {
                class
                    .offset
                    .checked_add(6 + u64_from_index(class.name.len()))
                    == Some(name.offset)
            })
            .map(|class| class.name.as_str())
            .or_else(|| {
                let source = name.object_id?.value()?;
                let mut matches = objects.iter().filter(|object| object.source == source);
                let first = matches.next()?.class.as_ref();
                matches
                    .all(|object| object.class.as_ref() == first)
                    .then_some(first)
            })
    };
    for history in histories {
        let axes = history
            .features
            .iter()
            .filter_map(|feature| {
                let name = names.of(feature)?;
                (class_of(name) == Some("moRefAxis_c"))
                    .then_some(name.object_id?.value()?)
                    .filter(|source| *source > 0)
            })
            .collect::<HashSet<_>>();
        let profiles = history
            .features
            .iter()
            .filter_map(|feature| {
                let name = names.of(feature)?;
                (class_of(name) == Some("moProfileFeature_c")).then_some(name.object_id?.value()?)
            })
            .collect::<HashSet<_>>();
        let starts = history
            .features
            .iter()
            .filter_map(|feature| names.of(feature).map(|name| name.offset))
            .collect::<Vec<_>>();
        for feature in &history.features {
            let Some(name) = names.of(feature) else {
                continue;
            };
            let Some("moRevolution_c" | "moRevCut_c") = class_of(name) else {
                continue;
            };
            let mut properties = Properties::new();
            if let Some(profile) = profile_index(ctx, lane, name)?.and_then(|index| {
                let object = objects.iter().find(|object| object.index == index)?;
                (object.class.as_ref() == "moProfileFeature_c" && profiles.contains(&object.source))
                    .then_some(object.source)
            }) {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("Profile"),
                    profile.to_string(),
                );
            }
            let end = starts
                .iter()
                .copied()
                .filter(|offset| *offset > name.offset)
                .min()
                .unwrap_or_else(|| u64_from_index(lane.native_payload.len()));
            if let Some((origin, direction)) = datum_axis(lane, name.offset, end, &axes) {
                let Point3 { x, y, z } = origin.get();
                properties.insert(
                    cadmpeg_core::nonblank_literal!("AxisOrigin"),
                    format!("{x}mm,{y}mm,{z}mm"),
                );
                let Vector3 { x, y, z } = *direction.as_raw();
                properties.insert(
                    cadmpeg_core::nonblank_literal!("AxisDirection"),
                    format!("{x},{y},{z}"),
                );
            }
            result.insert(feature.id.clone(), properties);
        }
    }
    Ok(result)
}

pub(super) fn enrich_archive_inputs(
    ctx: &DecodeContext<'_>,
    histories: &mut [FeatureHistory],
    lanes: &[FeatureInputLane],
    archive_names: &HashMap<String, Vec<NamedObject>>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut votes = HashMap::<String, Vec<_>>::new();
    for lane in lanes {
        let objects = archive_names.get(&lane.id).map_or(&[][..], Vec::as_slice);
        for (feature, properties) in lane_inputs(ctx, histories, lane, objects)? {
            votes.entry(feature).or_default().push(properties);
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let Some(votes) = votes.get(&feature.id) else {
            continue;
        };
        let Some(first) = votes.first() else {
            continue;
        };
        let has_axis = feature.properties.contains_key("AxisOrigin")
            || feature.properties.contains_key("AxisDirection");
        for (key, value) in first {
            if has_axis && matches!(key.as_str(), "AxisOrigin" | "AxisDirection") {
                continue;
            }
            if votes.iter().all(|vote| vote.get(key) == Some(value)) {
                feature
                    .properties
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
        }
    }
    Ok(())
}

pub(crate) fn bind_initial_operation(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    bodies: &[cadmpeg_ir::topology::Body],
    form_padding: Option<usize>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> bool {
    use cadmpeg_ir::features::{
        BooleanOp, FeatureDefinition, FeatureOperation, RevolveConstruction,
    };
    let [body] = bodies else { return false };
    if body.kind != cadmpeg_ir::topology::BodyKind::Solid {
        return false;
    }
    let mut candidate = None;
    for (index, feature) in features.iter().enumerate() {
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(
                FeatureOperation::TreeNode { .. }
                | FeatureOperation::Sketch { .. }
                | FeatureOperation::DatumPrincipalPlane { .. }
                | FeatureOperation::DatumAxis { .. }
                | FeatureOperation::DatumPoint { .. }
                | FeatureOperation::DatumPlane { .. }
                | FeatureOperation::DatumCoordinateSystem { .. },
            ) => {}
            FeatureDefinition::Operation(FeatureOperation::Revolve {
                construction:
                    RevolveConstruction::Resolved {
                        solid: Some(true), ..
                    },
                op: BooleanOp::Unresolved,
            }) if candidate.is_none() => candidate = Some(index),
            _ => return false,
        }
    }
    let Some(index) = candidate else { return false };
    let feature = &mut features[index];
    let outputs = feature.evaluation.outputs();
    if outputs.len() != 1 || !outputs.contains(&body.id) {
        return false;
    }
    let history_features = crate::records::HistoryFeatures::new(histories);
    let Some(history) = feature
        .native_ref
        .as_deref()
        .and_then(|id| history_features.get(id))
    else {
        return false;
    };
    if history.input_class.as_deref() != Some("moRevolution_c") {
        return false;
    }
    let mut forms = lanes.iter().filter_map(|lane| {
        let names = FeatureObjectNames::new(&lane.names);
        if !names.has_candidate(history) {
            return None;
        }
        Some(names.of(history).is_some_and(|name| {
            super::operations::has_zero_revolution_form(lane, name, form_padding)
        }))
    });
    if forms.next() != Some(true) || !forms.all(|form| form) {
        return false;
    }
    feature.evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Revolve { op, .. }) = definition {
            *op = BooleanOp::NewBody;
        }
    });
    losses.push(crate::loss::SldprtLossCode::FeatureRevolutionInitialBodyAssumed.note(format!(
        "Revolution {} uses NewBody for its zero operation form: it is the sole solid-producing feature and owns the sole result body; the native Boolean flag remains retained.", feature.id
    )));
    true
}

pub(crate) fn sync_initial_operation(ir: &mut cadmpeg_ir::CadIr) {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, FeatureOperation};
    let Some(feature) = ir.model.features.iter().find(|feature| {
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Revolve {
                op: BooleanOp::NewBody,
                ..
            })
        )
    }) else {
        return;
    };
    let FeatureDefinition::Operation(FeatureOperation::Revolve {
        construction: resolved,
        ..
    }) = feature.evaluation.definition()
    else {
        return;
    };
    for configuration in ir
        .model
        .configurations
        .iter_mut()
        .filter(|configuration| configuration.active)
    {
        if configuration.bodies.as_deref() != Some(feature.evaluation.outputs().as_slice()) {
            continue;
        }
        let Some(state) = configuration.feature_states.get_mut(&feature.id) else {
            continue;
        };
        if state.evaluation.is_suppressed() {
            continue;
        }
        if let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, op }) =
            &mut state.definition
        {
            if construction == resolved && *op == BooleanOp::Unresolved {
                *op = BooleanOp::NewBody;
            }
        }
    }
}

#[cfg(test)]
mod tests;
