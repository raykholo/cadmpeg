// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of rectangular patterns. Fusion reported each copy's
//! translation.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};
use serde_json::json;

use crate::F3dCodec;

const EPS_HOLE_POSITION_MM: f64 = 1.0e-8;

/// The decoded IR of a corpus file as JSON, after checking that the neutral
/// model validates.
fn decoded(bytes: &[u8]) -> serde_json::Value {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    serde_json::to_value(result.ir()).expect("serialize the IR")
}

/// The definition of the feature named `name`.
fn definition<'a>(ir: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    let mut features = ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .filter(|feature| feature["name"] == name);
    let feature = features.next().unwrap_or_else(|| panic!("feature {name}"));
    assert!(features.next().is_none(), "one feature named {name}");
    &feature["definition"]
}

/// The symmetric byte of every rectangular-pattern distribution block.
fn distribution_symmetry(ir: &serde_json::Value) -> Vec<bool> {
    ir["native"]["f3d"]["design_parameter_scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .filter_map(|scope| scope["rectangular_pattern_construction"]["distribution"].as_object())
        .map(|distribution| distribution["symmetric"]["value"] == true)
        .collect()
}

#[test]
fn a_symmetric_pattern_with_a_count_one_second_direction_reads_its_spacing() {
    // Fusion places the four copies at x = -5, 0, +5 and +10 mm. Direction two
    // has quantity 1 and keeps its 5 mm distance.
    let files: [&[u8]; 2] = [
        include_bytes!("../../../../corpus/f3d/symmetric_pattern.f3d"),
        include_bytes!("../../../../corpus/f3d/symmetric_pattern_body.f3d"),
    ];
    for bytes in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        assert_eq!(pattern["direction"], json!({"x": 1.0, "y": 0.0, "z": 0.0}));
        assert_eq!(pattern["spacing"], 5.0);
        assert_eq!(pattern["count"], 4);
        assert_eq!(pattern["symmetric"], true);
        assert_eq!(distribution_symmetry(&ir), [true]);
    }
}

/// The Compute option of every rectangular-pattern distribution block.
fn distribution_compute(ir: &serde_json::Value) -> Vec<u64> {
    ir["native"]["f3d"]["design_parameter_scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .filter_map(|scope| scope["rectangular_pattern_construction"]["distribution"].as_object())
        .filter_map(|distribution| distribution["compute_option"]["value"].as_u64())
        .collect()
}

#[test]
fn the_compute_option_leaves_a_five_copy_pattern_unchanged() {
    // Fusion translates the copies by 0, 2, 4, 6 and 8 mm in all three saves.
    let files: [(&[u8], u64); 3] = [
        (
            include_bytes!("../../../../corpus/f3d/compute_adjust.f3d"),
            2,
        ),
        (
            include_bytes!("../../../../corpus/f3d/compute_optimized.f3d"),
            0,
        ),
        (
            include_bytes!("../../../../corpus/f3d/compute_identical.f3d"),
            1,
        ),
    ];
    for (bytes, compute) in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "Groove pattern")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        assert_eq!(pattern["spacing"], 2.0);
        assert_eq!(pattern["count"], 5);
        assert_eq!(distribution_compute(&ir), [compute]);
    }
}

#[test]
fn an_optimized_pattern_reads_its_extent_and_symmetric_distribution() {
    // Fusion translates the Extent copies by 0, 10 and 20 mm, and the
    // symmetric copies by -10, 0 and +10 mm.
    let extent = decoded(include_bytes!(
        "../../../../corpus/f3d/optimized_extent_pattern.f3d"
    ));
    let pattern = &definition(&extent, "R-Pattern 1")["value"]["value"]["pattern"];
    assert_eq!(pattern["kind"], "linear", "{pattern}");
    assert_eq!(pattern["direction"], json!({"x": 1.0, "y": 0.0, "z": 0.0}));
    assert_eq!(pattern["spacing"], 10.0);
    assert_eq!(pattern["count"], 3);
    assert_eq!(pattern["symmetric"], false);
    assert_eq!(distribution_compute(&extent), [0]);

    let symmetric = decoded(include_bytes!(
        "../../../../corpus/f3d/optimized_symmetric_pattern.f3d"
    ));
    let pattern = &definition(&symmetric, "R-Pattern 1")["value"]["value"]["pattern"];
    assert_eq!(pattern["spacing"], 10.0);
    assert_eq!(pattern["count"], 3);
    assert_eq!(pattern["symmetric"], true);
    assert_eq!(distribution_symmetry(&symmetric), [true]);
    assert_eq!(distribution_compute(&symmetric), [0]);
}

#[test]
fn a_pattern_of_a_cut_through_two_bodies_reads_its_distribution() {
    // Fusion translates the copies by 0, 10 and 20 mm along Y.
    let ir = decoded(include_bytes!(
        "../../../../corpus/f3d/two_target_pattern.f3d"
    ));
    let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
    assert_eq!(pattern["kind"], "linear", "{pattern}");
    assert_eq!(pattern["direction"], json!({"x": 0.0, "y": 1.0, "z": 0.0}));
    assert_eq!(pattern["spacing"], 10.0);
    assert_eq!(pattern["count"], 3);
    assert_eq!(distribution_compute(&ir), [2]);
}

#[test]
fn a_rectangular_feature_pattern_keeps_its_direction_in_the_v11_record() {
    let ir = decoded(include_bytes!(
        "../../../../corpus/f3d/optimized_extent_pattern.f3d"
    ));
    let features = ir["design"]["features"].as_array().expect("v11 features");
    let feature = features
        .iter()
        .find(|feature| feature["name"] == "R-Pattern 1")
        .expect("rectangular pattern");
    let operation = &feature["definition"]["operation"];
    assert_eq!(operation["kind"], "pattern");
    let layout = &operation["value"]["layout"];
    assert_eq!(layout["kind"], "linear");
    let run = &layout["value"]["first"];
    assert_eq!(
        run["direction"]["resolved"],
        json!({"x": 1.0, "y": 0.0, "z": 0.0})
    );
    assert_eq!(run["run"]["spacing"]["kind"], "span");
    assert_eq!(
        run["run"]["spacing"]["value"]["count"],
        json!({"kind": "literal", "value": 3})
    );
    assert_eq!(
        run["run"]["spacing"]["value"]["span"],
        json!({"kind": "literal", "value": 20.0})
    );
    let mut centers = ir["geometry"]["surfaces"]
        .as_array()
        .expect("final surfaces")
        .iter()
        .filter_map(|surface| {
            let geometry = &surface["geometry"]["value"];
            (geometry["kind"] == "cylinder").then(|| {
                let origin = &geometry["origin"];
                [origin["x"].as_f64().unwrap(), origin["y"].as_f64().unwrap()]
            })
        })
        .collect::<Vec<_>>();
    centers.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(centers.len(), 3);
    for (center, expected_x) in centers.iter().zip([10.0, 20.0, 30.0]) {
        assert!((center[0] - expected_x).abs() < EPS_HOLE_POSITION_MM);
        assert!((center[1] - 20.0).abs() < EPS_HOLE_POSITION_MM);
    }
}

#[test]
fn an_unproved_body_seed_keeps_its_native_record_and_final_geometry() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../../corpus/f3d/symmetric_pattern_body.f3d"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode the body pattern");
    assert_eq!(result.ir().geometry.bodies.len(), 5);
    let feature = result
        .ir()
        .design
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("R-Pattern 1"))
        .expect("body pattern feature");
    let cadmpeg_ir::design::feature::FeatureOperation::Pattern(pattern) =
        &feature.definition.operation
    else {
        panic!("typed body pattern");
    };
    let cadmpeg_ir::design::ops::body::PatternSeeds::Bodies { bodies, join } = &pattern.seeds
    else {
        panic!("body seeds");
    };
    assert_eq!(*join, Some(false));
    let cadmpeg_ir::design::selection::Picks::Named(picks) = &bodies.picks else {
        panic!("native body seed");
    };
    assert!(picks.as_slice()[0].native.is_some());
    assert!(feature.native_ref.is_some());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::FeatureRectangularPatternDesignNative.kind()
    }));
}
