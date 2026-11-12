// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{ExtrudeDirection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::Vector3;

use crate::loss::SldprtLossCode;
use crate::test_support::container::{
    add_solidworks_version, make_block, native_envelope_fixture, sldprt_with_body,
};
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::parasolid::owned_triangle;
use crate::SldprtCodec;

fn extrusion(native: bool, direction: Option<&str>) -> Vec<u8> {
    let direction = direction
        .map(|value| format!(" Direction=\"{value}\""))
        .unwrap_or_default();
    let keywords = format!(
        "<Keywords><Extrusion Name=\"Extrude\" Type=\"BossExtrude\" id=\"31\" EndCondition=\"Blind\"{direction}><Dimension Name=\"Depth\">10mm</Dimension></Extrusion></Keywords>"
    );
    let mut source = sldprt_with_body(&owned_triangle(0, 700, 0.0));
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    add_solidworks_version(&mut source, 18000);
    if native {
        source.extend(make_block(
            0x43,
            "Contents/Config-0-ResolvedFeatures",
            &resolved_feature_classes_with_ids(&[("moExtrusion_c", "Extrude", 31)]),
        ));
    }
    native_envelope_fixture(&source)
}

#[test]
fn native_travel_retention_preserves_explicit_and_mirrored_controls() {
    for (native, vector) in [
        (false, None),
        (true, None),
        (true, Some(("0,0,1", 1.0))),
        (true, Some(("0,0,-1", -1.0))),
    ] {
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(extrusion(native, vector.map(|(literal, _)| literal))),
                &DecodeOptions::default(),
            )
            .expect("synthetic extrusion decodes");
        let direction = decoded
            .ir()
            .model
            .features
            .iter()
            .find_map(|feature| match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) => {
                    Some(direction)
                }
                _ => None,
            })
            .expect("extrusion definition");
        match vector {
            Some((_, z)) => assert!(matches!(direction,
                ExtrudeDirection::Explicit { vector, .. }
                    if vector.get() == Vector3::new(0.0, 0.0, z))),
            None if native => assert!(matches!(direction, ExtrudeDirection::Unresolved {})),
            None => assert!(matches!(direction, ExtrudeDirection::ProfileNormal {})),
        }
        assert_eq!(
            decoded.report().losses.iter().any(|loss| {
                loss.code == SldprtLossCode::FeatureExtrudeDirectionUnresolved.kind()
            }),
            native && vector.is_none(),
        );
    }
}
