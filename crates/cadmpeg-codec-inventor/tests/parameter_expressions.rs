// SPDX-License-Identifier: Apache-2.0
//! Reads the CC0 `inventor_parameter_expressions.ipt` (Inventor 2016, purpose-made; see
//! `tests/fixtures/README.txt`) and checks its user and model parameters, their expressions
//! and dependencies, and the parameter that drives the extrude distance.

use cadmpeg_codec_inventor::InventorCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use serde_json::Value;

fn decoded() -> Value {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/inventor_parameter_expressions.ipt"
    ))
    .expect("fixture");
    let ir = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode")
        .into_parts()
        .0;
    serde_json::to_value(&ir).expect("ir json")
}

fn parameter<'a>(ir: &'a Value, name: &str) -> &'a Value {
    ir["model"]["parameters"]
        .as_array()
        .expect("parameters")
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("parameter {name}"))
}

fn id_of<'a>(ir: &'a Value, name: &str) -> &'a str {
    parameter(ir, name)["id"].as_str().unwrap()
}

fn depends_on(ir: &Value, name: &str) -> Vec<String> {
    let mut names: Vec<String> = parameter(ir, name)["dependencies"]
        .as_array()
        .map(|ids| {
            ids.iter()
                .map(|id| {
                    ir["model"]["parameters"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|p| p["id"] == *id)
                        .map(|p| p["name"].as_str().unwrap().to_owned())
                        .unwrap_or_else(|| id.as_str().unwrap().to_owned())
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn all_ten_parameters_keep_their_expressions_and_dependencies() {
    let ir = decoded();
    let parameters = ir["model"]["parameters"].as_array().unwrap();
    for name in ["Width", "Height", "Hole", "d0", "d1", "d2", "d3", "d4", "d5", "d6"] {
        assert!(parameters.iter().any(|p| p["name"] == name), "{name} is missing");
    }
    // Expression text is in metres.
    assert_eq!(parameter(&ir, "Width")["expression"], "0.04 m");
    assert!(depends_on(&ir, "Width").is_empty());
    assert!(depends_on(&ir, "Hole").is_empty());
    let expression = |name: &str| parameter(&ir, name)["expression"].as_str().unwrap().to_owned();
    assert!(expression("Height").contains("Width"), "{}", expression("Height"));
    assert_eq!(depends_on(&ir, "Height"), ["Width"]);
    assert_eq!(depends_on(&ir, "d0"), ["Width"]);
    assert!(expression("d1").contains("Width"), "{}", expression("d1"));
    assert_eq!(depends_on(&ir, "d1"), ["Width"]);
    assert_eq!(depends_on(&ir, "d2"), ["Height"]);
    assert_eq!(depends_on(&ir, "d3"), ["Hole"]);
    // A model parameter that references another model parameter.
    assert_eq!(depends_on(&ir, "d4"), ["d3"]);
    assert!(expression("d4").contains("d3"), "{}", expression("d4"));
    assert_eq!(depends_on(&ir, "d5"), ["Height"]);
}

#[test]
fn user_parameters_have_no_owner_and_model_parameters_are_named_d_n() {
    let ir = decoded();
    for name in ["Width", "Height", "Hole"] {
        assert!(parameter(&ir, name).get("owner").is_none(), "{name} is a user parameter");
    }
    for name in ["d0", "d1", "d2", "d3", "d4", "d5", "d6"] {
        let id = id_of(&ir, name);
        assert!(!id.is_empty(), "{name}");
    }
}

#[test]
fn the_extrude_distance_points_at_d5() {
    let ir = decoded();
    let d5 = id_of(&ir, "d5");
    let bound: Vec<&str> = ir["model"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["source_properties"]["extent_distance_parameter"].as_str())
        .collect();
    assert_eq!(bound, [d5], "{bound:?}");
}
