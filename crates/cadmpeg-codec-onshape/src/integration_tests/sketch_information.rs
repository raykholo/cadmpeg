// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
//! `sketches.json` supplies the attachment frame of a sketch on a model face.
use super::projection::face_sketch_bundle;
use crate::reader::id;
use crate::test_support::review::{decode, members, validate, wire};
use serde_json::{json, Value};

const PATH: &str = "partstudios/studio/sketches.json";
type MatrixEdit = fn(&mut Value);
const UNRESOLVED: &str = "attachment translation and rotation unresolved";

fn with_matrix(matrix: &Value) -> Vec<u8> {
    members(&face_sketch_bundle([0.0, 0.0, 1.0], true), |members| {
        members.insert(
            PATH.into(),
            serde_json::to_vec(&json!({
                "sourceMicroversion": "synthetic-microversion",
                "sketches": [{"featureId": "side-sketch", "sketchMatrix": matrix}]
            }))
            .unwrap(),
        );
    })
}

fn identity_at_face() -> Value {
    json!([1.0, 0.0, 0.0, 0.04, 0.0, 1.0, 0.0, 0.07, 0.0, 0.0, 1.0, 0.005, 0.0, 0.0, 0.0, 1.0])
}

fn kinds(wire: &Value) -> Vec<&str> {
    wire["model"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["definition"]["value"]["kind"].as_str().unwrap())
        .collect()
}

fn has_loss(result: &cadmpeg_ir::codec::DecodeResult, code: &str, detail: &str) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == code && loss.message.contains(detail))
}

#[test]
fn a_face_sketch_takes_its_frame_from_the_stored_matrix() {
    let result = decode(&with_matrix(&identity_at_face()));
    let wire = wire(&result);
    assert_eq!(
        kinds(&wire),
        ["sketch", "extrude", "sketch"],
        "{:?}",
        result.report().losses
    );
    let sketch = wire["model"]["sketches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sketch| sketch["id"] == id("sketch", "side-sketch"))
        .unwrap();
    assert_eq!(
        sketch["placement"]["frame"],
        json!({"origin":{"x":40.0,"y":70.0,"z":5.0},
            "normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}})
    );
    validate(result);
}

#[test]
fn a_rotated_stored_matrix_sets_the_x_axis() {
    // x = +y, normal = +z, y = normal x x = -x.
    let matrix = json!([
        0.0, -1.0, 0.0, 0.04, 1.0, 0.0, 0.0, 0.07, 0.0, 0.0, 1.0, 0.005, 0.0, 0.0, 0.0, 1.0
    ]);
    let result = decode(&with_matrix(&matrix));
    let wire = wire(&result);
    assert_eq!(kinds(&wire)[2], "sketch", "{:?}", result.report().losses);
    let sketch = wire["model"]["sketches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sketch| sketch["id"] == id("sketch", "side-sketch"))
        .unwrap();
    assert_eq!(
        sketch["placement"]["frame"]["u_axis"],
        json!({"x":0.0,"y":1.0,"z":0.0})
    );
    validate(result);
}

#[test]
fn a_bundle_without_the_member_keeps_the_unresolved_frame_refusal() {
    let result = decode(&face_sketch_bundle([0.0, 0.0, 1.0], true));
    assert_eq!(kinds(&wire(&result))[2], "native");
    assert!(has_loss(
        &result,
        "onshape/history.not-supported",
        UNRESOLVED
    ));
    assert!(!has_loss(&result, "onshape/sketch.frame-invalid", ""));
    validate(result);
}

#[test]
fn an_invalid_stored_matrix_refuses_with_the_frame_loss() {
    let edits: [(&str, MatrixEdit); 6] = [
        ("orthonormal", |m| m[0] = json!(0.5)),
        ("orthonormal", |m| {
            for i in [1, 5, 9] {
                m[i] = json!(-m[i].as_f64().unwrap());
            }
        }),
        ("orthonormal", |m| m[4] = json!(2.0)),
        ("affine", |m| m[12] = json!(0.1)),
        ("sixteen", |m| {
            m.as_array_mut().unwrap().pop();
        }),
        ("non-finite", |m| m[3] = json!("0.04")),
    ];
    for (detail, edit) in edits {
        let mut matrix = identity_at_face();
        edit(&mut matrix);
        let result = decode(&with_matrix(&matrix));
        assert_eq!(kinds(&wire(&result))[2], "native");
        assert!(
            has_loss(&result, "onshape/sketch.frame-invalid", detail),
            "{detail}: {:?}",
            result.report().losses
        );
        validate(result);
    }
}

#[test]
fn a_stored_matrix_off_the_face_plane_refuses_the_sketch() {
    let mut matrix = identity_at_face();
    matrix[11] = json!(0.05);
    let result = decode(&with_matrix(&matrix));
    assert_eq!(kinds(&wire(&result))[2], "native");
    assert!(has_loss(
        &result,
        "onshape/history.not-supported",
        "contradicts its planar face"
    ));
    validate(result);
}
