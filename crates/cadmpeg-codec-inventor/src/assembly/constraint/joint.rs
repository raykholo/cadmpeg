// SPDX-License-Identifier: Apache-2.0
//! Neutral assembly joints from typed assembly constraints (specification section 9.4).
//!
//! A constraint becomes a joint when its solution state is 0, it stores no
//! limits, both operands carry stored geometry and a selection key, and its
//! family and geometry pair have a neutral joint family. Every other
//! constraint stays a native record.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
use cadmpeg_ir::ids::{IdentityKey, OccurrenceId};
use cadmpeg_ir::products::{
    AssemblyJoint, JointConnector, JointId, JointOperand, Occurrence, PairedJointKind, PlaneSense,
};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;

use super::geometry::GeometryShape;
use super::{AssemblyConstraintRecord, ConstraintKind, ConstraintOperandRecord, OperandSelection};

/// Inventor stores operand geometry in centimetres.
const CENTIMETRES_TO_MILLIMETRES: f64 = 10.0;
/// Largest deviation from unit length that a stored axis may have.
const EPS_UNIT_AXIS: f64 = 1.0e-6;
/// Largest cosine between the two stored plane axes.
const EPS_ORTHOGONAL_AXES: f64 = 1.0e-6;
/// Largest magnitude of a line-mate offset that states coincident axes.
const EPS_ZERO_OFFSET: f64 = 1.0e-9;
/// Solution state of a constraint that the stored placements satisfy.
const SOLVED: u8 = 0;
/// Parameter role of a length value.
const ROLE_LENGTH: u16 = 3;
/// Parameter role of an angle value.
const ROLE_ANGLE: u16 = 4;

/// Neutral joints and the count of constraints that stay native.
#[derive(Debug, Default)]
pub(crate) struct JointProjection {
    pub(crate) joints: Vec<AssemblyJoint>,
    pub(crate) native: usize,
}

/// Projects every constraint that has a neutral joint family.
pub(crate) fn project_joints(
    ctx: &DecodeContext<'_>,
    constraints: &[AssemblyConstraintRecord],
    occurrences: &[Occurrence],
    parameters: &[DesignParameter],
) -> Result<JointProjection, CodecError> {
    let mut occurrence_ids = HashSet::new();
    for occurrence in occurrences {
        ctx.charge_work(1, "index Inventor joint occurrences")?;
        ctx.insert_hash_set(
            &mut occurrence_ids,
            occurrence.id.as_str(),
            "index Inventor joint occurrences",
        )?;
    }
    let mut parameter_values = HashMap::new();
    for parameter in parameters {
        ctx.charge_work(1, "index Inventor joint parameters")?;
        if let Some(value) = &parameter.value {
            ctx.insert_hash_map(
                &mut parameter_values,
                parameter.id.as_str(),
                value,
                "index Inventor joint parameters",
            )?;
        }
    }
    let mut projection = JointProjection::default();
    for constraint in constraints {
        ctx.charge_work(1, "project Inventor assembly constraint")?;
        match project_joint(ctx, constraint, &occurrence_ids, &parameter_values)? {
            Some(joint) => {
                ctx.charge_collection_items(1, "project Inventor assembly joint")?;
                ctx.charge_entities(1, "project Inventor assembly joint")?;
                projection.joints.push(joint);
            }
            None => projection.native += 1,
        }
    }
    Ok(projection)
}

fn project_joint(
    ctx: &DecodeContext<'_>,
    constraint: &AssemblyConstraintRecord,
    occurrence_ids: &HashSet<&str>,
    parameter_values: &HashMap<&str, &ParameterValue>,
) -> Result<Option<AssemblyJoint>, CodecError> {
    if constraint.solution != SOLVED || constraint.limit_state != 0 {
        return Ok(None);
    }
    let [first, second] = constraint.operands.as_slice() else {
        return Ok(None);
    };
    let [parameter] = constraint.parameters.as_slice() else {
        return Ok(None);
    };
    let (Some(role), Some(reference)) = (parameter.role, parameter.parameter_reference) else {
        return Ok(None);
    };
    let Some(ordinal) = reference.record_ordinal() else {
        return Ok(None);
    };
    let (Some(first_shape), Some(second_shape)) = (
        first
            .stored_geometry
            .as_ref()
            .map(|geometry| &geometry.shape),
        second
            .stored_geometry
            .as_ref()
            .map(|geometry| &geometry.shape),
    ) else {
        return Ok(None);
    };
    let Some(key) = record_key(ctx, &constraint.segment_token, ordinal)? else {
        return Ok(None);
    };
    let parameter_id = ParameterId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "parameter"),
        key,
    );
    let Some(value) = parameter_values.get(parameter_id.as_str()) else {
        return Ok(None);
    };
    let Some(kind) = joint_kind(constraint.kind, first_shape, second_shape, role, value) else {
        return Ok(None);
    };
    let Some(first) = connector(ctx, constraint, first, first_shape, occurrence_ids)? else {
        return Ok(None);
    };
    let Some(second) = connector(ctx, constraint, second, second_shape, occurrence_ids)? else {
        return Ok(None);
    };
    let Some(key) = record_key(ctx, &constraint.segment_token, constraint.record_ordinal)? else {
        return Ok(None);
    };
    let id = JointId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "assembly", "joint"),
        key,
    );
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(constraint.id.len()),
        "retain Inventor joint native reference",
    )?;
    let mut joint = AssemblyJoint::paired(id, kind, [first, second], None);
    joint.driving_parameter = Some(parameter_id);
    joint.native_ref = Some(constraint.id.clone());
    Ok(Some(joint))
}

/// The identity key `<token>-<ordinal>` of one `AmDc` record.
fn record_key(
    ctx: &DecodeContext<'_>,
    token: &str,
    ordinal: u32,
) -> Result<Option<IdentityKey>, CodecError> {
    let key = ctx.format_retained(
        format_args!("{token}-{ordinal}"),
        "Inventor joint record identity key",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index("inventor:assembly:joint#".len() + key.len()),
        "retain Inventor joint record identity",
    )?;
    Ok(IdentityKey::try_new(key).ok())
}

/// The neutral family of a constraint family, geometry pair, and value.
fn joint_kind(
    kind: ConstraintKind,
    first: &GeometryShape,
    second: &GeometryShape,
    role: u16,
    value: &ParameterValue,
) -> Option<PairedJointKind> {
    use GeometryShape::{Direction, Line, Plane, Point};
    match (kind, first, second, role, value) {
        (
            ConstraintKind::Mate,
            Plane { .. },
            Plane { .. },
            ROLE_LENGTH,
            ParameterValue::Length(offset),
        ) => Some(PairedJointKind::PlaneCoincidence {
            sense: PlaneSense::Opposed,
            offset: Some(FiniteReal::new(offset.get())?),
        }),
        (
            ConstraintKind::Flush,
            Plane { .. },
            Plane { .. },
            ROLE_LENGTH,
            ParameterValue::Length(offset),
        ) => Some(PairedJointKind::PlaneCoincidence {
            sense: PlaneSense::Aligned,
            offset: Some(FiniteReal::new(offset.get())?),
        }),
        (
            ConstraintKind::Mate,
            Line { .. },
            Line { .. },
            ROLE_LENGTH,
            ParameterValue::Length(offset),
        ) if offset.get().abs() <= EPS_ZERO_OFFSET => Some(PairedJointKind::AxisCoincidence {}),
        (
            ConstraintKind::Mate,
            Point { .. },
            Line { .. },
            ROLE_LENGTH,
            ParameterValue::Length(distance),
        ) => Some(PairedJointKind::PointOnAxis {
            distance: Some(FiniteReal::new(distance.get())?),
        }),
        (
            ConstraintKind::Angle,
            Direction { .. },
            Direction { .. },
            ROLE_ANGLE,
            ParameterValue::Angle(angle),
        ) => Some(PairedJointKind::Angle {
            angle: Some(FiniteReal::new(angle.get())?),
        }),
        _ => None,
    }
}

/// The connector of one operand: its owning occurrence path, its selection, and its frame.
fn connector(
    ctx: &DecodeContext<'_>,
    constraint: &AssemblyConstraintRecord,
    operand: &ConstraintOperandRecord,
    shape: &GeometryShape,
    occurrence_ids: &HashSet<&str>,
) -> Result<Option<JointConnector>, CodecError> {
    let Some(frame) = frame(shape) else {
        return Ok(None);
    };
    let Some(selection) = &operand.selection else {
        return Ok(None);
    };
    let operand = match (selection, operand.occurrence_id) {
        (OperandSelection::Face { part, name_key, .. }, Some(occurrence)) => {
            let subelement =
                ctx.format_retained(format_args!("face:{name_key}"), "Inventor joint face key")?;
            nested_operand(
                ctx,
                occurrence,
                &part.occurrence_key,
                &part.key,
                subelement,
                occurrence_ids,
            )?
        }
        (
            OperandSelection::WorkFeature {
                part, source_index, ..
            },
            Some(occurrence),
        ) => {
            let subelement = ctx.format_retained(
                format_args!("work_feature:{source_index}"),
                "Inventor joint work-feature key",
            )?;
            nested_operand(
                ctx,
                occurrence,
                &part.occurrence_key,
                &part.key,
                subelement,
                occurrence_ids,
            )?
        }
        (
            OperandSelection::AssemblyGeometry {
                target_source_index,
                ..
            },
            None,
        ) => {
            let object = ctx.format_retained(
                format_args!("AmDcSegment::{}", constraint.segment_token),
                "Inventor joint assembly object",
            )?;
            let subelement = ctx.format_retained(
                format_args!("assembly_geometry:{target_source_index}"),
                "Inventor joint assembly geometry key",
            )?;
            Some(JointOperand::root(object, vec![subelement]))
        }
        _ => None,
    };
    Ok(operand.map(|operand| JointConnector {
        operand,
        frame,
        detached: false,
    }))
}

/// The operand owned by a document below the occurrence `occurrence`.
///
/// `occurrence_key` lists the owning occurrence path, outermost first, as
/// colon-separated decimal keys. Its first key is the operand occurrence.
fn nested_operand(
    ctx: &DecodeContext<'_>,
    occurrence: u32,
    occurrence_key: &str,
    object: &str,
    subelement: String,
    occurrence_ids: &HashSet<&str>,
) -> Result<Option<JointOperand>, CodecError> {
    let mut keys = occurrence_key.split(':');
    if keys.next().and_then(|key| key.parse::<u32>().ok()) != Some(occurrence) {
        return Ok(None);
    }
    let id = OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "assembly", "instance"),
        occurrence,
    );
    if !occurrence_ids.contains(id.as_str()) {
        return Ok(None);
    }
    let mut path = Vec::new();
    for key in keys {
        ctx.charge_collection_items(1, "collect Inventor joint occurrence path")?;
        if key.is_empty() || !key.bytes().all(|byte| byte.is_ascii_digit()) {
            return Ok(None);
        }
        path.push(ctx.copy_retained_text(key, "retain Inventor joint occurrence path key")?);
    }
    let object = ctx.copy_retained_text(object, "retain Inventor joint operand object")?;
    Ok(Some(JointOperand::nested_occurrence(
        id,
        path,
        object,
        vec![subelement],
    )))
}

/// The connector frame of a stored shape, in millimetres. The z axis is the
/// plane normal, line direction, or direction vector.
fn frame(shape: &GeometryShape) -> Option<Transform> {
    let (origin, x, z) = match shape {
        GeometryShape::Plane {
            origin,
            u_axis,
            v_axis,
        } => {
            let u = unit(*u_axis)?;
            let v = unit(*v_axis)?;
            if dot(u, v).abs() > EPS_ORTHOGONAL_AXES {
                return None;
            }
            (*origin, u, unit(cross(u, v))?)
        }
        GeometryShape::Line { origin, direction } => {
            let z = unit(*direction)?;
            (*origin, perpendicular(z)?, z)
        }
        GeometryShape::Point { position } => (*position, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        GeometryShape::Direction { vector } => {
            let z = unit(*vector)?;
            ([0.0; 3], perpendicular(z)?, z)
        }
    };
    let y = cross(z, x);
    let mut rows = [[FiniteReal::ZERO; 4]; 3];
    for (axis, row) in rows.iter_mut().enumerate() {
        *row = [
            FiniteReal::new(x[axis])?,
            FiniteReal::new(y[axis])?,
            FiniteReal::new(z[axis])?,
            FiniteReal::new(origin[axis] * CENTIMETRES_TO_MILLIMETRES)?,
        ];
    }
    Some(Transform::from_finite_rows(rows))
}

/// The unit vector perpendicular to `z` in the plane of `z` and the
/// coordinate axis least aligned with `z`.
fn perpendicular(z: [f64; 3]) -> Option<[f64; 3]> {
    let mut axis = [0.0; 3];
    let least = (0..3).min_by(|&left, &right| z[left].abs().total_cmp(&z[right].abs()))?;
    axis[least] = 1.0;
    let along = dot(axis, z);
    unit([
        axis[0] - along * z[0],
        axis[1] - along * z[1],
        axis[2] - along * z[2],
    ])
}

fn unit(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(vector, vector).sqrt();
    if !length.is_finite() || (length - 1.0).abs() > EPS_UNIT_AXIS {
        return None;
    }
    Some(vector.map(|component| component / length))
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

#[cfg(test)]
mod tests;
