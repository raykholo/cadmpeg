//! Synthetic revolution input records from §2.3.

use super::*;
use crate::records::{Feature, FeatureInputClass, ObjectId};
use cadmpeg_ir::features::{
    AngularTermination, BooleanOp, FeatureDefinition, FeatureOperation, PlanarProfileRef,
    RevolveConstruction, RevolveExtent,
};
use std::collections::BTreeMap;

const ANGLE_TOLERANCE: f64 = 1e-12;

fn cstring(bytes: &mut Vec<u8>, text: &str) {
    bytes.extend_from_slice(&[
        0xff,
        0xfe,
        0xff,
        u8::try_from(text.encode_utf16().count()).unwrap(),
    ]);
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
}

fn object(lane: &mut FeatureInputLane, class: &str, name: &str, source: u32) {
    let mut bytes = lane.native_payload.to_vec();
    if class == "moRevolution_c" {
        bytes.extend_from_slice(&[0; 12]);
    }
    let offset = u64_from_index(bytes.len());
    bytes.extend_from_slice(&[0xff, 0xff, 1, 0]);
    bytes.extend_from_slice(&u16::try_from(class.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(class.as_bytes());
    lane.classes.push(FeatureInputClass {
        id: format!("class{offset}"),
        parent: lane.id.clone(),
        ordinal: 0,
        offset,
        name: class.into(),
    });
    lane.names.push(FeatureInputName {
        id: format!("name{offset}"),
        parent: lane.id.clone(),
        ordinal: 0,
        offset: u64_from_index(bytes.len()),
        object_id: ObjectId::from_value(source),
        value: name.into(),
    });
    bytes.extend_from_slice(&[4, 0x80]);
    cstring(&mut bytes, name);
    bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x40]);
    bytes.extend_from_slice(&source.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    cstring(&mut bytes, "");
    lane.native_payload = bytes.into();
}

fn declaration(lane: &mut FeatureInputLane, class: &str) {
    let offset = u64_from_index(lane.native_payload.len());
    let mut bytes = lane.native_payload.to_vec();
    bytes.extend_from_slice(&[0xff, 0xff, 1, 0]);
    bytes.extend_from_slice(&u16::try_from(class.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(class.as_bytes());
    lane.classes.push(FeatureInputClass {
        id: format!("class{offset}"),
        parent: lane.id.clone(),
        ordinal: 0,
        offset,
        name: class.into(),
    });
    lane.native_payload = bytes.into();
}

fn feature(name: &str, source: u32) -> Feature {
    Feature {
        id: format!("sldprt:history:feature#1:{source}"),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: crate::records::FeatureSource::from_value(source),
        ordinal: source,
        name: name.into(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

fn lane() -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn frame() -> Vec<u8> {
    let mut bytes = vec![0; 135];
    for (at, value) in [(0, 13u32), (4, 42), (10, 1), (18, 91), (50, 16000)] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    for at in [34, 38, 42] {
        bytes[at..at + 4].copy_from_slice(&(-12345i32).to_le_bytes());
    }
    bytes[66..70].fill(0xff);
    for (at, value) in [
        (78, 0.4f64),
        (86, 0.001),
        (94, 0.002),
        (102, 0.003),
        (126, -1.0),
    ] {
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn datum_axis_uses_the_point_after_the_auxiliary_scalar() {
    let (point, direction) = datum_axis_frame(&frame(), &HashSet::from([13])).unwrap();
    assert_eq!(point.get(), Point3::new(1.0, 2.0, 3.0));
    assert_eq!(*direction.as_raw(), Vector3::new(0.0, 0.0, -1.0));
}

#[test]
fn truncated_and_invalid_axis_records_stay_unresolved() {
    let bytes = frame();
    for len in 0..bytes.len() {
        assert!(datum_axis_frame(&bytes[..len], &HashSet::from([13])).is_none());
    }
    assert!(datum_axis_frame(&bytes, &HashSet::from([14])).is_none());
    for at in [8, 14, 22, 34, 46, 54, 66, 70, 134] {
        let mut invalid = bytes.clone();
        invalid[at] ^= 1;
        assert!(
            datum_axis_frame(&invalid, &HashSet::from([13])).is_none(),
            "{at}"
        );
    }
    for at in [86, 110] {
        let mut invalid = bytes.clone();
        invalid[at..at + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(datum_axis_frame(&invalid, &HashSet::from([13])).is_none());
    }
}

#[test]
fn archive_reference_selects_a_profile_across_an_intervening_axis() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    for long in [false, true] {
        let mut lane = lane();
        object(&mut lane, "moProfileFeature_c", "section", 11);
        object(&mut lane, "moRefAxis_c", "axis", 13);
        object(&mut lane, "moRevolution_c", "turn", 17);
        let mut bytes = lane.native_payload.to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 0]);
        if long {
            bytes.extend_from_slice(&[0xff, 0x7f]);
            bytes.extend_from_slice(&31u32.to_le_bytes());
        } else {
            bytes.extend_from_slice(&31u16.to_le_bytes());
        }
        lane.native_payload = bytes.into();
        declaration(&mut lane, "moLineRef_w");
        declaration(&mut lane, "moCompRefAxis_c");
        let mut bytes = lane.native_payload.to_vec();
        bytes.extend(frame());
        lane.native_payload = bytes.into();
        declaration(&mut lane, "moRevEndSpec_c");
        let mut histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![
                feature("section", 11),
                feature("axis", 13),
                feature("turn", 17),
            ],
        }];
        histories[0].features[1].properties.insert(
            cadmpeg_core::nonblank_literal!("Origin"),
            "1mm,2mm,0mm".into(),
        );
        histories[0].features[1].properties.insert(
            cadmpeg_core::nonblank_literal!("Direction"),
            "0,0,-1".into(),
        );
        histories[0].features[2]
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D1"), "270°".into());
        let objects = [NamedObject {
            index: 31,
            class: "moProfileFeature_c".into(),
            source: 11,
        }];
        enrich_archive_inputs(
            &ctx,
            &mut histories,
            std::slice::from_ref(&lane),
            &HashMap::from([(lane.id.clone(), objects.to_vec())]),
        )
        .unwrap();
        let properties = &histories[0].features[2].properties;
        assert_eq!(properties.get("Profile").map(String::as_str), Some("11"));
        crate::resolved_features::classes::bind_history_classes(
            &mut histories,
            std::slice::from_ref(&lane),
        );
        let mut projected = crate::history::project::project_features(&histories).unwrap();
        crate::resolved_features::operations::bind_revolution_operations(
            &mut projected,
            &histories,
            std::slice::from_ref(&lane),
            Some(8),
        );
        let FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction:
                RevolveConstruction::Resolved {
                    profile: PlanarProfileRef::Feature(profile),
                    axis,
                    extent:
                        RevolveExtent::OneSided {
                            termination: AngularTermination::Angle { angle },
                        },
                    ..
                },
            op,
        }) = projected[2].evaluation.definition()
        else {
            panic!("resolved revolution");
        };
        assert_eq!(profile, &projected[0].id);
        assert_eq!(*op, BooleanOp::Unresolved);
        assert_eq!(axis.origin.get(), Point3::new(1.0, 2.0, 3.0));
        assert_eq!(axis.direction.get(), Vector3::new(0.0, 0.0, -1.0));
        assert!((angle.get() - std::f64::consts::PI * 1.5).abs() < ANGLE_TOLERANCE);
        assert_initial_operation_requires_one_resolved_producer(&projected, &histories, &lane);
        histories[0].features[2].properties.clear();
        enrich_archive_inputs(
            &ctx,
            &mut histories,
            std::slice::from_ref(&lane),
            &HashMap::new(),
        )
        .unwrap();
        assert!(!histories[0].features[2].properties.contains_key("Profile"));
    }
}

#[test]
fn an_unreadable_axis_keeps_the_profile_and_the_next_revolution() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut lane = lane();
    object(&mut lane, "moProfileFeature_c", "section", 11);
    object(&mut lane, "moRefAxis_c", "axis", 13);
    for (name, source, valid) in [("first", 17, false), ("second", 19, true)] {
        object(&mut lane, "moRevolution_c", name, source);
        let mut bytes = lane.native_payload.to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 0, 31, 0]);
        lane.native_payload = bytes.into();
        declaration(&mut lane, "moLineRef_w");
        declaration(&mut lane, "moCompRefAxis_c");
        let mut record = frame();
        if !valid {
            record[126..134].copy_from_slice(&f64::NAN.to_le_bytes());
        }
        let mut bytes = lane.native_payload.to_vec();
        bytes.extend(record);
        lane.native_payload = bytes.into();
        declaration(&mut lane, "moRevEndSpec_c");
    }
    let mut histories = [FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            feature("section", 11),
            feature("axis", 13),
            feature("first", 17),
            feature("second", 19),
        ],
    }];
    let names = HashMap::from([(
        lane.id.clone(),
        vec![NamedObject {
            index: 31,
            class: "moProfileFeature_c".into(),
            source: 11,
        }],
    )]);
    enrich_archive_inputs(&ctx, &mut histories, &[lane], &names).unwrap();
    for feature in &histories[0].features[2..] {
        assert_eq!(
            feature.properties.get("Profile").map(String::as_str),
            Some("11")
        );
    }
    assert!(!histories[0].features[2]
        .properties
        .contains_key("AxisDirection"));
    assert_eq!(
        histories[0].features[3]
            .properties
            .get("AxisDirection")
            .map(String::as_str),
        Some("0,0,-1")
    );
}

#[test]
fn unresolved_archive_cells_keep_the_profile_unresolved() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    for cell in [
        vec![0, 0],
        vec![1, 0x80],
        vec![0xff, 0x7f, 0, 0, 0, 0x80],
        vec![0xff],
    ] {
        let mut lane = lane();
        object(&mut lane, "moRevolution_c", "turn", 17);
        let mut bytes = lane.native_payload.to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 0]);
        bytes.extend(cell);
        lane.native_payload = bytes.into();
        assert_eq!(profile_index(&ctx, &lane, &lane.names[0]).unwrap(), None);
    }
}

#[test]
fn opaque_axis_tail_values_preserve_the_placed_line() {
    let mut bytes = frame();
    bytes[4..8].fill(0);
    bytes[18..22].fill(0);
    bytes[78..86].copy_from_slice(&f64::NAN.to_le_bytes());
    let (point, direction) = datum_axis_frame(&bytes, &HashSet::from([13])).unwrap();
    assert_eq!(point.get(), Point3::new(1.0, 2.0, 3.0));
    assert_eq!(*direction.as_raw(), Vector3::new(0.0, 0.0, -1.0));
}

fn assert_initial_operation_requires_one_resolved_producer(
    projected: &[cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    lane: &FeatureInputLane,
) {
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::Body;
    let body: Body = serde_json::from_value(serde_json::json!({
        "id": "synthetic:test:body#result", "kind": "solid"
    }))
    .unwrap();
    let mut features = projected.to_vec();
    features[2]
        .evaluation
        .set_outputs(vec![body.id.clone()].try_into().unwrap());
    let run = |features: &mut [cadmpeg_ir::features::Feature],
               bodies: &[Body],
               lanes: &[FeatureInputLane]| {
        let mut losses = Vec::new();
        bind_initial_operation(features, histories, lanes, bodies, Some(8), &mut losses);
        losses
    };
    let assert_unresolved = |features: &[cadmpeg_ir::features::Feature]| {
        assert!(matches!(
            features[2].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Revolve {
                op: BooleanOp::Unresolved,
                ..
            })
        ));
    };
    let mut resolved = features.clone();
    let losses = run(
        &mut resolved,
        std::slice::from_ref(&body),
        std::slice::from_ref(lane),
    );
    assert!(matches!(
        resolved[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            op: BooleanOp::NewBody,
            ..
        })
    ));
    assert_active_configuration_tracks_initial_operation(&resolved, &features[2], &body);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::SldprtLossCode::FeatureRevolutionInitialBodyAssumed.kind()
    );
    assert_eq!(
        losses[0].code.local_code(),
        "feature.revolution-initial-body-assumed"
    );
    assert_eq!(
        losses[0].code.taxonomy(),
        cadmpeg_ir::report::loss::LossTaxonomy::DecodeDiagnostic
    );
    for bodies in [Vec::new(), vec![body.clone(), body.clone()]] {
        let mut refused = features.clone();
        assert!(run(&mut refused, &bodies, std::slice::from_ref(lane)).is_empty());
        assert_unresolved(&refused);
    }
    let mut sheet = body.clone();
    sheet.kind = cadmpeg_ir::topology::BodyKind::Sheet;
    let mut refused = features.clone();
    assert!(run(&mut refused, &[sheet], std::slice::from_ref(lane)).is_empty());
    assert_unresolved(&refused);
    for outputs in [
        Vec::new(),
        vec![BodyId::mint("synthetic:test:body#other").unwrap()],
    ] {
        let mut refused = features.clone();
        refused[2]
            .evaluation
            .set_outputs(outputs.try_into().unwrap());
        assert!(run(
            &mut refused,
            std::slice::from_ref(&body),
            std::slice::from_ref(lane)
        )
        .is_empty());
        assert_unresolved(&refused);
    }
    let mut refused = features.clone();
    refused.push(features[2].clone());
    assert!(run(
        &mut refused,
        std::slice::from_ref(&body),
        std::slice::from_ref(lane)
    )
    .is_empty());
    assert_unresolved(&refused);
    let mut wrong = lane.clone();
    let name = &wrong.names[2];
    let trailer = usize::try_from(name.offset).unwrap() + 6 + 2 * name.value.encode_utf16().count();
    wrong.native_payload[trailer + 4] = 1;
    for lanes in [Vec::new(), vec![wrong.clone()], vec![lane.clone(), wrong]] {
        let mut refused = features.clone();
        assert!(run(&mut refused, std::slice::from_ref(&body), &lanes).is_empty());
        assert_unresolved(&refused);
    }
}

fn assert_active_configuration_tracks_initial_operation(
    resolved: &[cadmpeg_ir::features::Feature],
    unresolved: &cadmpeg_ir::features::Feature,
    body: &cadmpeg_ir::topology::Body,
) {
    let id = unresolved.id.as_str();
    let configuration = serde_json::json!({
        "id": "synthetic:test:configuration#base", "ordinal": 0, "active": true,
        "properties": {}, "parameter_overrides": {}, "parameter_values": {},
        "bodies": [body.id.as_str()],
        "feature_states": {id: {
            "evaluation": {"kind": "active", "outputs": []}, "dependencies": [],
            "definition": unresolved.evaluation.definition()
        }}
    });
    for case in 0..6 {
        let mut configuration = configuration.clone();
        match case {
            1 => configuration["active"] = false.into(),
            2 => configuration["bodies"] = serde_json::json!(["synthetic:test:body#other"]),
            3 => {
                configuration["feature_states"][id]["evaluation"] =
                    serde_json::json!({"kind": "suppressed"});
            }
            4 => {
                configuration["feature_states"][id]["definition"]["value"]["value"]
                    ["construction"]["value"]["axis"]["origin"]["z"] = 9.0.into();
            }
            5 => {
                configuration["feature_states"][id]["definition"]["value"]["value"]["op"] =
                    "cut".into();
            }
            _ => {}
        }
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.features = resolved.to_vec();
        ir.model
            .configurations
            .push(serde_json::from_value(configuration).unwrap());
        sync_initial_operation(&mut ir);
        let definition = &ir.model.configurations[0].feature_states[&unresolved.id].definition;
        let FeatureDefinition::Operation(FeatureOperation::Revolve { op, .. }) = definition else {
            panic!("revolution")
        };
        let expected = match case {
            0 => BooleanOp::NewBody,
            5 => BooleanOp::Cut,
            _ => BooleanOp::Unresolved,
        };
        assert_eq!(*op, expected, "configuration case {case}");
    }
}
