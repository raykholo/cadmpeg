// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{is_mirror_construction, matches_history};
use crate::feature::history::{HistoryNodeKind, HistoryPlacement, SUPPRESSED_FLAG};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureDirection3,
    FeatureEvaluation, FeatureId, FeatureOperation, FinitePoint3, NativeFeatureKind,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;

fn feature() -> Feature {
    Feature {
        id: FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
            cadmpeg_ir::ids::IdentityKey::try_new("test-1").expect("feature key"),
        ),
        ordinal: 3,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: NativeFeatureKind::Other("Mirror".into()),
                parameters: BTreeMap::default(),
            },
        )),
        native_ref: None,
    }
}

#[test]
fn mirror_construction_requires_mirror_pattern_transform() {
    let mut feature = feature();
    assert!(!is_mirror_construction(&feature));
    let seeds = vec![PatternSeed::Feature(feature.id.clone())];
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Pattern {
            seeds: seeds.clone(),
            pattern: PatternKind::new(PatternTransform::Mirror {
                plane_origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("origin"),
                plane_normal: FeatureDirection3::new(Vector3::new(1.0, 0.0, 0.0)).expect("normal"),
            })
            .expect("mirror pattern"),
        },
    ));
    assert!(is_mirror_construction(&feature));
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Pattern {
            seeds,
            pattern: PatternKind::new(PatternTransform::Linear {
                direction: None,
                spacing: PositiveLength::new(1.0).expect("spacing"),
                count: 2,
                second: None,
            })
            .expect("linear pattern"),
        },
    ));
    assert!(!is_mirror_construction(&feature));
}

#[test]
fn mirror_history_uses_native_order_scope_and_suppression() {
    let placement = HistoryPlacement {
        ordinal: 3,
        kind: HistoryNodeKind::Pattern,
        below_end_of_part: false,
        model_history: true,
        model_root: 0,
    };
    let feature = feature();
    assert!(matches_history(&feature, placement, 0));
    let mut changed = feature.clone();
    changed.ordinal = 4;
    assert!(!matches_history(&changed, placement, 0));
    let mut changed = feature.clone();
    changed.suppressed = Some(true);
    assert!(!matches_history(&changed, placement, 0));
    assert!(!matches_history(&feature, placement, SUPPRESSED_FLAG));
    assert!(!matches_history(
        &feature,
        HistoryPlacement {
            model_history: false,
            ..placement
        },
        0
    ));
    assert!(!matches_history(
        &feature,
        HistoryPlacement {
            below_end_of_part: true,
            ..placement
        },
        0
    ));
    let mut changed = feature.clone();
    changed.source_properties.insert(
        NonBlankString::new("history".to_owned()).expect("property key"),
        "flat_pattern".into(),
    );
    assert!(!matches_history(&changed, placement, 0));
    assert!(matches_history(
        &changed,
        HistoryPlacement {
            model_history: false,
            ..placement
        },
        0
    ));
    changed.suppressed = Some(true);
    changed.source_properties.insert(
        NonBlankString::new("suppressed".to_owned()).expect("property key"),
        "true".into(),
    );
    assert!(matches_history(
        &changed,
        HistoryPlacement {
            model_history: false,
            ..placement
        },
        SUPPRESSED_FLAG
    ));
}
