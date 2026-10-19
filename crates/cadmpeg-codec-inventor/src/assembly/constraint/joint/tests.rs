// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId, ParameterValue};
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, OperandContainer, PairedJointKind, PlaneSense, PrototypeReference,
};
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
use cadmpeg_ir::transform::Transform;

use super::super::geometry::{GeometryShape, OperandGeometryRecord};
use super::super::selection::{KeySegmentKind, PartKey};
use super::super::{
    AssemblyConstraintRecord, ConstraintKind, ConstraintOperandRecord, ConstraintParameterRecord,
    OperandSelection,
};
use super::project_joints;
use crate::pmdc::PmDcReference;

const TOKEN: &str = "segment";
const PARAMETER_ORDINAL: u32 = 54;
const EPS_FRAME: f64 = 1.0e-12;

fn context() -> cadmpeg_core::decode::DecodeContext<'static> {
    cadmpeg_test_support::service_decode_context()
}

fn reference(index: u32) -> PmDcReference {
    PmDcReference::new(index, true).unwrap()
}

fn occurrence(id: u32) -> Occurrence {
    Occurrence {
        id: OccurrenceId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "assembly", "instance"),
            id,
        ),
        prototype: PrototypeReference::Unresolved {},
        parent: OccurrenceParent::Root {},
        ordinal: id,
        transform: Transform::identity(),
        linked_prototype: None,
        scale: [FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    }
}

fn parameter(value: ParameterValue) -> DesignParameter {
    DesignParameter {
        id: ParameterId::mint(format!(
            "inventor:design:parameter#{TOKEN}-{PARAMETER_ORDINAL}"
        ))
        .unwrap(),
        owner: None,
        ordinal: 0,
        name: "d1".into(),
        expression: "0 mm".into(),
        display: None,
        value: Some(value),
        dependencies: DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

fn length(millimetres: f64) -> ParameterValue {
    ParameterValue::Length(Length::new(millimetres).unwrap())
}

fn face_operand(
    occurrence: u32,
    occurrence_key: &str,
    shape: GeometryShape,
) -> ConstraintOperandRecord {
    ConstraintOperandRecord {
        reference: reference(100),
        occurrence_reference: Some(reference(10)),
        geometry_reference: Some(reference(56)),
        geometry_type: None,
        geometry: None,
        stored_geometry: Some(OperandGeometryRecord {
            entity_flags: 0,
            document_reference: reference(115),
            frame_marker: false,
            shape,
        }),
        occurrence_id: Some(occurrence),
        projection_hint: None,
        selection: Some(OperandSelection::Face {
            key_record: 56,
            part: PartKey {
                key: "PmDCSegment::part:revision:6".into(),
                segment_kind: KeySegmentKind::Part,
                part_segment: "part".into(),
                revision_key: "revision".into(),
                key_number: "6".into(),
                occurrence_key: occurrence_key.into(),
            },
            values: [2, 2],
            name_map_reference: reference(4),
            name_key: 7,
            name_record: None,
            name_type: None,
            imported_name: None,
        }),
    }
}

fn plane(origin: [f64; 3]) -> GeometryShape {
    GeometryShape::Plane {
        origin,
        u_axis: [-1.0, 0.0, 0.0],
        v_axis: [0.0, 1.0, 0.0],
    }
}

fn line(origin: [f64; 3]) -> GeometryShape {
    GeometryShape::Line {
        origin,
        direction: [0.0, 0.0, 1.0],
    }
}

fn constraint(
    kind: ConstraintKind,
    role: u16,
    operands: Vec<ConstraintOperandRecord>,
) -> AssemblyConstraintRecord {
    AssemblyConstraintRecord {
        id: format!("inventor:assembly:constraint#{TOKEN}-30"),
        segment_token: TOKEN.into(),
        record_ordinal: 30,
        header_value: 0,
        header_id: 0,
        next_reference: reference(0),
        flags: 512,
        context_reference: reference(3),
        source_index: 57,
        state: -1,
        owner_reference: reference(43),
        kind_code: 1,
        kind,
        operands,
        parameters: vec![ConstraintParameterRecord {
            reference: reference(166),
            role: Some(role),
            parameter_reference: Some(reference(PARAMETER_ORDINAL + 1)),
        }],
        solution: 0,
        name_index: 2,
        limit_state: 0,
    }
}

fn mate() -> AssemblyConstraintRecord {
    constraint(
        ConstraintKind::Mate,
        3,
        vec![
            face_operand(1, "1", plane([0.0, 0.0, 0.0])),
            face_operand(4, "4:2:9", plane([1.0, 2.0, 0.5])),
        ],
    )
}

fn occurrences() -> Vec<Occurrence> {
    vec![occurrence(1), occurrence(4)]
}

#[test]
fn a_plane_mate_projects_opposed_planes_with_the_parameter_offset() {
    let projection = project_joints(
        &context(),
        &[mate()],
        &occurrences(),
        &[parameter(length(5.0))],
    )
    .unwrap();
    assert_eq!(projection.native, 0);
    let [joint] = projection.joints.as_slice() else {
        panic!("one joint: {:?}", projection.joints);
    };
    assert_eq!(joint.id.as_str(), "inventor:assembly:joint#segment-30");
    assert_eq!(
        joint.native_ref.as_deref(),
        Some("inventor:assembly:constraint#segment-30")
    );
    assert_eq!(
        joint.driving_parameter.as_ref().map(ParameterId::as_str),
        Some("inventor:design:parameter#segment-54")
    );
    let wire = serde_json::to_value(joint).unwrap();
    assert_eq!(
        serde_json::from_value::<PairedJointKind>(wire["operands"]["kind"].clone()).unwrap(),
        PairedJointKind::PlaneCoincidence {
            sense: PlaneSense::Opposed,
            offset: Some(FiniteReal::new(5.0).unwrap()),
        }
    );
    let connectors = joint.connectors().collect::<Vec<_>>();
    let OperandContainer::Occurrence {
        occurrence,
        occurrence_path,
    } = &connectors[1].operand.container
    else {
        panic!("occurrence container");
    };
    assert_eq!(occurrence.as_str(), "inventor:assembly:instance#4");
    assert_eq!(occurrence_path, &["2".to_owned(), "9".to_owned()]);
    assert_eq!(connectors[1].operand.object, "PmDCSegment::part:revision:6");
    assert_eq!(connectors[1].operand.subelements, ["face:7"]);
    let OperandContainer::Occurrence {
        occurrence_path, ..
    } = &connectors[0].operand.container
    else {
        panic!("occurrence container");
    };
    assert!(occurrence_path.is_empty());

    // u = -x and v = y, so the normal u x v is -z and the frame y axis z x u is
    // +y; the origin is in millimetres.
    let rows = connectors[1].frame.affine_rows();
    let expected = [
        [-1.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, 20.0],
        [0.0, 0.0, -1.0, 5.0],
    ];
    for (row, expected) in rows.iter().zip(expected) {
        for (value, expected) in row.iter().zip(expected) {
            assert!((value - expected).abs() <= EPS_FRAME, "{rows:?}");
        }
    }
    assert!(connectors[1].frame.is_proper_rigid());
}

#[test]
fn flush_and_coaxial_mates_take_their_families() {
    let mut flush = mate();
    flush.kind = ConstraintKind::Flush;
    let mut axis = constraint(
        ConstraintKind::Mate,
        3,
        vec![
            face_operand(1, "1", line([0.0, 0.0, 0.0])),
            face_operand(4, "4", line([0.0, 0.0, 3.0])),
        ],
    );
    axis.record_ordinal = 31;
    let projection = project_joints(
        &context(),
        &[flush, axis],
        &occurrences(),
        &[parameter(length(0.0))],
    )
    .unwrap();
    assert_eq!(projection.native, 0);
    let kinds = projection
        .joints
        .iter()
        .map(|joint| serde_json::to_value(joint).unwrap()["operands"]["kind"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            serde_json::json!({"kind": "plane_coincidence", "sense": "aligned", "offset": 0.0}),
            serde_json::json!({"kind": "axis_coincidence"}),
        ]
    );
    let line_frame = projection.joints[1].connectors().next().unwrap().frame;
    assert!(line_frame.is_proper_rigid());
    assert_eq!(line_frame.affine_rows()[2][2], 1.0);
}

#[test]
fn an_angle_between_directions_takes_the_angle_family() {
    let direction = |vector| GeometryShape::Direction { vector };
    let angle = constraint(
        ConstraintKind::Angle,
        4,
        vec![
            face_operand(1, "1", direction([0.0, 1.0, 0.0])),
            face_operand(4, "4", direction([0.0, -1.0, 0.0])),
        ],
    );
    let half_turn = ParameterValue::Angle(Angle::from_assigned_real(
        FiniteReal::new(std::f64::consts::PI).unwrap(),
    ));
    let projection = project_joints(
        &context(),
        &[angle],
        &occurrences(),
        &[parameter(half_turn)],
    )
    .unwrap();
    let [joint] = projection.joints.as_slice() else {
        panic!("one joint");
    };
    assert_eq!(
        serde_json::to_value(joint).unwrap()["operands"]["kind"],
        serde_json::json!({"kind": "angle", "angle": std::f64::consts::PI})
    );
}

#[test]
fn unproven_states_and_unresolved_operands_stay_native() {
    let parameters = [parameter(length(1.0))];
    let mut cases = Vec::new();
    let mut unsolved = mate();
    unsolved.solution = 4;
    cases.push(unsolved);
    let mut limited = mate();
    limited.limit_state = 1;
    cases.push(limited);
    let mut limits = mate();
    limits.parameters.push(limits.parameters[0].clone());
    cases.push(limits);
    let mut wrong_role = mate();
    wrong_role.parameters[0].role = Some(4);
    cases.push(wrong_role);
    let mut absent_parameter = mate();
    absent_parameter.parameters[0].parameter_reference = Some(reference(9));
    cases.push(absent_parameter);
    let mut absent_occurrence = mate();
    absent_occurrence.operands[1] = face_operand(8, "8", plane([0.0; 3]));
    cases.push(absent_occurrence);
    let mut wrong_path = mate();
    wrong_path.operands[1] = face_operand(4, "5:2", plane([0.0; 3]));
    cases.push(wrong_path);
    let mut bad_path = mate();
    bad_path.operands[1] = face_operand(4, "4::2", plane([0.0; 3]));
    cases.push(bad_path);
    let mut no_selection = mate();
    no_selection.operands[1].selection = None;
    cases.push(no_selection);
    let mut no_geometry = mate();
    no_geometry.operands[0].stored_geometry = None;
    cases.push(no_geometry);
    let mut skewed = mate();
    skewed.operands[0] = face_operand(
        1,
        "1",
        GeometryShape::Plane {
            origin: [0.0; 3],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [1.0, 1.0, 0.0],
        },
    );
    cases.push(skewed);
    let mut offset_axes = constraint(
        ConstraintKind::Mate,
        3,
        vec![
            face_operand(1, "1", line([0.0; 3])),
            face_operand(4, "4", line([0.0; 3])),
        ],
    );
    offset_axes.record_ordinal = 32;
    cases.push(offset_axes);
    let mut tangent = mate();
    tangent.kind = ConstraintKind::Tangent;
    cases.push(tangent);

    let count = cases.len();
    let projection = project_joints(&context(), &cases, &occurrences(), &parameters).unwrap();
    assert!(projection.joints.is_empty(), "{:?}", projection.joints);
    assert_eq!(projection.native, count);
}

#[test]
fn assembly_geometry_operands_belong_to_the_root() {
    let mut joint = mate();
    joint.operands[0].occurrence_id = None;
    joint.operands[0].selection = Some(OperandSelection::AssemblyGeometry {
        key_record: 60,
        target_reference: reference(70),
        target_source_index: 2,
    });
    let projection = project_joints(
        &context(),
        &[joint],
        &occurrences(),
        &[parameter(length(0.0))],
    )
    .unwrap();
    let [joint] = projection.joints.as_slice() else {
        panic!("one joint");
    };
    let first = joint.connectors().next().unwrap();
    assert_eq!(first.operand.container, OperandContainer::Root {});
    assert_eq!(first.operand.object, "AmDcSegment::segment");
    assert_eq!(first.operand.subelements, ["assembly_geometry:2"]);
}
