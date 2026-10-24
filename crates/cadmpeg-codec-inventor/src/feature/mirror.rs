// SPDX-License-Identifier: Apache-2.0
//! Mirror operands and reflection in the part coordinate system.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};
use cadmpeg_ir::features::{
    BodySelection, Feature, FeatureDefinition, FeatureDirection3, FeatureId, FeatureOperation,
    FeatureResultMembers, FeatureResultTopology, FinitePoint3,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;
use cadmpeg_ir::math::{Point3, Vector3};

use super::{
    history_feature_id, native_feature, ClassId, PmDcFeature, PmDcFeatureLabel,
    PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, PmDcPatternFamily,
    PmDcPatternFeature, PmDcWorkPlane,
};
use crate::pmdc::PmDcReference;
use crate::sketch::PmDcTransform;

const BODY_INSTANCE_CLASS: ClassId = ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
    0x5b, 0xb8, 0x7e, 0x87, 0xac, 0x4a, 0x0f, 0x14, 0xd3, 0x14, 0xef, 0x89, 0xad, 0x8b, 0x5a, 0x7f,
]));

/// Admission tolerance for the unit length and orthogonality of mirror-plane axes.
const EPS_MIRROR_FRAME: f64 = 1.0e-10;
const MIRROR_CLASS: ClassId = ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
    0xa3, 0x26, 0xc2, 0xb6, 0xd2, 0x11, 0x26, 0x36, 0x60, 0x00, 0x2c, 0xab, 0x01, 0xf3, 0x1b, 0xb0,
]));
pub(crate) const BODY_MAP_TYPE: [u8; 16] = [
    0x54, 0xff, 0x26, 0x1a, 0x4a, 0x42, 0x57, 0x67, 0x64, 0x92, 0x95, 0xab, 0xae, 0x7c, 0x3d, 0xb8,
];
const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

pub(crate) struct MirrorIndex<'a> {
    properties: HashMap<(&'a str, u32), Option<&'a PmDcFeatureProperty>>,
    transforms: HashMap<(&'a str, u32), Option<&'a PmDcTransform>>,
    planes: HashMap<(&'a str, u32), Option<&'a PmDcWorkPlane>>,
    features: HashMap<(&'a str, u32), Option<&'a PmDcFeature>>,
    labels: HashMap<(&'a str, u32), Option<&'a PmDcFeatureLabel>>,
    body_maps: HashMap<(&'a str, u32), Option<()>>,
}

struct ResolvedMirror {
    definition: FeatureDefinition,
    result_native: String,
    members: FeatureResultMembers,
}

impl<'a> MirrorIndex<'a> {
    pub(crate) fn new(
        properties: &'a [PmDcFeatureProperty],
        transforms: &'a [PmDcTransform],
        planes: &'a [PmDcWorkPlane],
        features: &'a [PmDcFeature],
        labels: &'a [PmDcFeatureLabel],
        body_maps: impl Iterator<Item = (&'a str, u32)>,
    ) -> Self {
        Self {
            properties: unique_index(properties.iter().map(|r| {
                (
                    (r.identity.segment_token.as_str(), r.identity.record_ordinal),
                    r,
                )
            })),
            transforms: unique_index(transforms.iter().map(|r| {
                (
                    (r.identity.segment_token.as_str(), r.identity.record_ordinal),
                    r,
                )
            })),
            planes: unique_index(planes.iter().map(|r| {
                (
                    (r.identity.segment_token.as_str(), r.identity.record_ordinal),
                    r,
                )
            })),
            features: unique_index(features.iter().map(|r| {
                (
                    (r.identity.segment_token.as_str(), r.identity.record_ordinal),
                    r,
                )
            })),
            labels: unique_index(labels.iter().filter_map(|r| {
                Some((
                    (
                        r.identity.segment_token.as_str(),
                        r.header.owner.record_ordinal()?,
                    ),
                    r,
                ))
            })),
            body_maps: unique_index(body_maps.map(|key| (key, ()))),
        }
    }

    fn property(&self, token: &str, reference: PmDcReference) -> Option<&'a PmDcFeatureProperty> {
        let property = self
            .properties
            .get(&(token, reference.record_ordinal()?))?
            .as_ref()
            .copied()?;
        let type_id = match &property.kind {
            PmDcFeaturePropertyKind::Boolean { .. } => super::BOOLEAN_TYPE,
            PmDcFeaturePropertyKind::RdxVariable { .. } => super::RDX_VARIABLE_TYPE,
            PmDcFeaturePropertyKind::SurfaceBody { .. } => super::SURFACE_BODY_TYPE,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                ..
            } => super::OBJECT_COLLECTION_TYPE,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FeatureDimensions,
                ..
            } => super::FEATURE_DIMENSIONS_TYPE,
            _ => return None,
        };
        (property.identity.type_id == crate::record_identity::RecordTypeId::from_bytes(type_id))
            .then_some(property)
    }

    fn resolve(
        &self,
        source: &PmDcPatternFeature,
        label: &PmDcFeatureLabel,
        mut seed_feature: impl FnMut(u32) -> Option<FeatureId>,
    ) -> Option<ResolvedMirror> {
        let extension_count = match source.save_version_major {
            0..=20 => 0,
            21..=28 => 6,
            _ => 14,
        };
        if !crate::pmdc::record_version_supported(source.save_version_major)
            || source.family != PmDcPatternFamily::Mirror
            || source.identity.type_id
                != crate::record_identity::RecordTypeId::from_bytes(super::MIRROR_FEATURE_TYPE)
            || source.property_slots.len() != 13
            || source.extension_values.len() != extension_count
            || source.control != 0
            || label.header.owner.record_ordinal() != Some(source.identity.record_ordinal)
            || label.identity.segment_token != source.identity.segment_token
            || label.identity.type_id
                != crate::record_identity::RecordTypeId::from_bytes(super::FEATURE_LABEL_TYPE)
            || label.class_id() != MIRROR_CLASS
        {
            return None;
        }
        let token = source.identity.segment_token.as_str();
        let slot = |n: usize| source.property_slots.get(n).copied();
        if !matches!(
            &self.property(token, slot(5)?)?.kind,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FeatureDimensions,
                ..
            }
        ) {
            return None;
        }
        let boolean = |n| match &self.property(token, slot(n)?)?.kind {
            PmDcFeaturePropertyKind::Boolean { value, .. } => Some(*value),
            _ => None,
        };
        let bodies_mode = boolean(6)?;
        if boolean(7)?
            || slot(3)?.index() != 0
            || (slot(11)?.index() != 0 && boolean(11)?)
            || !matches!(
                self.property(token, slot(2)?)?.kind,
                PmDcFeaturePropertyKind::RdxVariable {
                    nominal_value: 0,
                    model_value: 0,
                    ..
                }
            )
        {
            return None;
        }
        let placement = self
            .transforms
            .get(&(token, slot(0)?.record_ordinal()?))?
            .as_ref()?;
        if placement.identity.type_id
            != crate::record_identity::RecordTypeId::from_bytes(crate::sketch::TRANSFORM_TYPE)
            || placement.matrix.rows() != IDENTITY
        {
            return None;
        }
        let plane = self
            .planes
            .get(&(token, slot(12)?.record_ordinal()?))?
            .as_ref()?;
        if plane.identity.type_id
            != crate::record_identity::RecordTypeId::from_bytes(super::WORK_PLANE_TYPE)
        {
            return None;
        }
        let u = Vector3::from(plane.u_axis.map(cadmpeg_ir::scalar::FiniteReal::get));
        let v = Vector3::from(plane.v_axis.map(cadmpeg_ir::scalar::FiniteReal::get));
        if (u.norm() - 1.0).abs() > EPS_MIRROR_FRAME
            || (v.norm() - 1.0).abs() > EPS_MIRROR_FRAME
            || u.dot(v).abs() > EPS_MIRROR_FRAME
        {
            return None;
        }
        let normal =
            FeatureDirection3::from(cadmpeg_ir::units::UnitVector3::normalized(u.cross(v))?);
        let origin = plane.origin.map(cadmpeg_ir::scalar::FiniteReal::get);
        let origin = FinitePoint3::new(Point3::new(
            origin[0] * 10.0,
            origin[1] * 10.0,
            origin[2] * 10.0,
        ))?;
        let collection = self.property(token, slot(if bodies_mode { 10 } else { 9 })?)?;
        let PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::ObjectCollection,
            items,
        } = &collection.kind
        else {
            return None;
        };
        let mut bodies = Vec::new();
        for reference in items.references() {
            let body = self.property(token, *reference)?;
            if !matches!(body.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
                return None;
            }
            bodies.push(NonBlankString::new(body.id())?);
        }
        if bodies.is_empty() {
            return None;
        }
        let result_native = if slot(8)?.index() != 0 {
            if self.body_maps.get(&(token, slot(8)?.record_ordinal()?)) != Some(&Some(()))
                || !bodies_mode
                || label.participants.references().is_empty()
            {
                return None;
            }
            bodies.clear();
            for reference in label.participants.references() {
                let ordinal = reference.record_ordinal()?;
                let child = self.features.get(&(token, ordinal))?.as_ref()?;
                let child_label = self.labels.get(&(token, ordinal))?.as_ref()?;
                if child.identity.type_id
                    != crate::record_identity::RecordTypeId::from_bytes(super::FEATURE_TYPE)
                    || child_label.identity.type_id
                        != crate::record_identity::RecordTypeId::from_bytes(
                            super::FEATURE_LABEL_TYPE,
                        )
                    || child_label.class_id() != BODY_INSTANCE_CLASS
                {
                    return None;
                }
                let body = self.property(token, *child.properties.references().get(3)?)?;
                if !matches!(body.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
                    return None;
                }
                bodies.push(NonBlankString::new(body.id())?);
            }
            label.id()
        } else {
            collection.id()
        };
        let members = FeatureResultMembers::new(bodies, Vec::new(), Vec::new(), Vec::new()).ok()?;
        let seeds = if bodies_mode {
            if !source.participants.references().is_empty() {
                return None;
            }
            vec![PatternSeed::Bodies(BodySelection::Native(collection.id()))]
        } else {
            if source.participants.references().is_empty() {
                return None;
            }
            let mut seen = HashSet::new();
            let mut seeds = Vec::new();
            for reference in source.participants.references() {
                let ordinal = reference.record_ordinal()?;
                if !seen.insert(ordinal) {
                    return None;
                }
                seeds.push(PatternSeed::Feature(seed_feature(ordinal)?));
            }
            seeds
        };
        let pattern = PatternKind::new(PatternTransform::Mirror {
            plane_origin: origin,
            plane_normal: normal,
        })
        .ok()?;
        Some(ResolvedMirror {
            definition: FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }),
            result_native,
            members,
        })
    }

    pub(super) fn project(
        &self,
        ctx: &DecodeContext<'_>,
        source: &PmDcPatternFeature,
        label: &PmDcFeatureLabel,
        seed_feature: impl FnMut(u32) -> Option<FeatureId>,
    ) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
        self.resolve(source, label, seed_feature).map(|resolved| {
            let mut feature = native_feature(ctx, &source.identity, Some(label), source.id())?;
            feature.source_tag = Some("mirror".into());
            feature.evaluation =
                cadmpeg_ir::features::FeatureEvaluation::from_definition(resolved.definition);
            ctx.charge_entities(1, "project Inventor mirror result topology")?;
            let result = FeatureResultTopology::new(
                FeatureResultTopologyId::compose(
                    &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
                    source.identity.key()?,
                ),
                history_feature_id(&source.identity)?,
                resolved.members,
                Some(resolved.result_native),
            );
            Ok((feature, result))
        })
    }

    pub(crate) fn matches_projection(
        &self,
        source: &PmDcPatternFeature,
        label: &PmDcFeatureLabel,
        feature: &Feature,
        result: &FeatureResultTopology,
        seed_feature: impl FnMut(u32) -> Option<FeatureId>,
    ) -> bool {
        let Some(resolved) = self.resolve(source, label, seed_feature) else {
            return false;
        };
        let Ok(feature_id) = history_feature_id(&source.identity) else {
            return false;
        };
        let Ok(key) = source.identity.key() else {
            return false;
        };
        if feature.id != feature_id
            || feature.native_ref.as_deref() != Some(source.id().as_str())
            || feature.evaluation.definition() != &resolved.definition
        {
            return false;
        }
        let expected = FeatureResultTopology::new(
            FeatureResultTopologyId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
                key,
            ),
            feature_id,
            resolved.members,
            Some(resolved.result_native),
        );
        result == &expected
    }
}

pub(crate) fn seed_feature_id(
    history: &super::history::History,
    mirror_ordinal: u32,
    seed: &crate::record_identity::RecordIdentity,
) -> Option<FeatureId> {
    let before = history.placement(seed.record_ordinal)?;
    let current = history.placement(mirror_ordinal)?;
    if before.ordinal >= current.ordinal || before.model_root != current.model_root {
        return None;
    }
    let seed_type = match before.kind {
        super::history::HistoryNodeKind::Feature => super::FEATURE_TYPE,
        super::history::HistoryNodeKind::Pattern => {
            if seed.type_id
                == crate::record_identity::RecordTypeId::from_bytes(super::MIRROR_FEATURE_TYPE)
            {
                super::MIRROR_FEATURE_TYPE
            } else {
                super::RECTANGULAR_PATTERN_FEATURE_TYPE
            }
        }
        super::history::HistoryNodeKind::Sketch => return None,
    };
    if seed.type_id != crate::record_identity::RecordTypeId::from_bytes(seed_type) {
        return None;
    }
    history_feature_id(seed).ok()
}

#[cfg(test)]
mod tests;
