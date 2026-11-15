// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::sheet_metal::Fixture;
use cadmpeg_ir::{
    features::{NativeFeatureKind, PlanarProfileRef, SheetMetalThicknessSide},
    ids::IdentityKey,
    sketches::SketchId,
};

fn feature(key: &str, ordinal: u64, operation: FeatureOperation) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("test", "design", "feature"),
            IdentityKey::try_new(key).expect("key"),
        ),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: None,
    }
}

fn history() -> Vec<Feature> {
    vec![
        feature(
            "sketch",
            1,
            FeatureOperation::Native {
                kind: NativeFeatureKind::Other("Sketch".into()),
                parameters: BTreeMap::new(),
            },
        ),
        feature(
            "face",
            2,
            FeatureOperation::SheetMetalBaseFlange {
                profile: PlanarProfileRef::Sketch(SketchId::compose(
                    &cadmpeg_ir::identity_namespace!("test", "design", "sketch"),
                    IdentityKey::try_new("profile").expect("key"),
                )),
                thickness: PositiveLength::new(7.0).expect("thickness"),
                side: SheetMetalThicknessSide::Forward,
                rule: None,
            },
        ),
    ]
}

#[test]
fn style_lengths_become_one_preceding_rule_named_by_each_sheet_metal_operation() {
    let fixture = Fixture::new();
    let mut features = history();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service())
            .expect("context");
    project(
        &ctx,
        &mut features,
        Some(&fixture.style),
        Fixture::length,
        None,
    )
    .expect("project");
    assert_eq!(features.len(), 3);
    let rule = &features[0];
    let id = crate::feature::history_feature_id(&fixture.style.identity).expect("identity");
    assert_eq!(rule.id, id);
    assert_eq!(rule.ordinal, 0);
    assert_eq!(rule.native_ref, Some(fixture.style.id()));
    assert!(rule.evaluation.outputs().is_empty());
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalRule { construction }) =
        rule.evaluation.definition()
    else {
        panic!("rule");
    };
    assert_eq!(
        construction.thickness,
        Some(PositiveLength::new(7.0).expect("thickness"))
    );
    assert_eq!(
        construction.inside_radius,
        Some(NonNegativeLength::new(11.0).expect("radius"))
    );
    assert_eq!(construction.neutral_factor, None);
    let face = &features[2];
    assert_eq!(
        face.evaluation.definition().operation().sheet_metal_rule(),
        Some(&id)
    );
    assert!(face.dependencies.contains(&id));
    assert_eq!(features[1].dependencies.len(), 0);
}

#[test]
fn unresolved_style_lengths_or_no_style_leave_the_history_unchanged() {
    let fixture = Fixture::new();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service())
            .expect("context");
    let mut features = history();
    project(&ctx, &mut features, Some(&fixture.style), |_| None, None).expect("project");
    assert_eq!(features, history());
    project(&ctx, &mut features, None, Fixture::length, None).expect("project");
    assert_eq!(features, history());
    let mut plain = history();
    plain.truncate(1);
    project(
        &ctx,
        &mut plain,
        Some(&fixture.style),
        Fixture::length,
        None,
    )
    .expect("project");
    assert_eq!(plain.len(), 1);
}

#[test]
fn active_unfold_factor_is_the_rule_neutral_factor() {
    let fixture = Fixture::new();
    let factor = FiniteReal::new(0.37).expect("factor");
    let construction =
        construction(&fixture.style, Fixture::length, Some(factor)).expect("construction");
    assert_eq!(construction.neutral_factor, Some(factor));
}
