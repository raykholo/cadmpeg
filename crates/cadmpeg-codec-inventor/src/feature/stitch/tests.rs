// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::tests::{
    reference_list, test_feature, test_label, test_projection_index, test_property,
};
use crate::test_support::test_fixtures::parse;

fn boolean(ordinal: u32, value: bool) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value,
        },
    )
}

fn properties(patch_items: &[u32]) -> Vec<PmDcFeatureProperty> {
    vec![
        boolean(1, true),
        test_property(
            2,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch,
                items: reference_list(patch_items),
            },
        ),
        boolean(3, false),
        test_property(
            4,
            PmDcFeaturePropertyKind::SurfaceBody {
                body: crate::feature::tests::reference(9),
            },
        ),
    ]
}

const SLOTS: [(usize, u32); 5] = [(0, 0), (2, 1), (3, 2), (4, 3), (5, 4)];

#[test]
fn stitch_projects_unresolved_operands_and_the_result_wrapper() {
    for items in [&[][..], &[7][..]] {
        let properties = properties(items);
        let source = test_feature(0, 6, &SLOTS);
        let label = test_label(0, 2, CLASS_ID, &[]);
        let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
        parse(&[], |ctx, _| {
            let (feature, result) = project(ctx, &source, &label, &index)
                .expect("supported Stitch Surface")
                .expect("project Stitch Surface");
            assert_eq!(
                feature.evaluation.definition(),
                &FeatureDefinition::Operation(FeatureOperation::SewBodies {
                    bodies: SewBodySelection::try_from(BodySelection::Unresolved)
                        .expect("unresolved sew operands"),
                    gap_tolerance: None,
                })
            );
            assert_eq!(feature.source_tag.as_deref(), Some(TAG));
            assert_eq!(
                feature
                    .source_properties
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str()))
                    .collect::<Vec<_>>(),
                [("property_2_boolean", "true"), ("property_4_boolean", "false")]
            );
            assert_eq!(
                result.bodies().iter().map(|body| body.as_str()).collect::<Vec<_>>(),
                [properties[3].id().as_str()]
            );
        });
    }
}

#[test]
fn stitch_refuses_other_slot_forms() {
    let label = test_label(0, 2, CLASS_ID, &[]);
    let properties = properties(&[]);
    let index = test_projection_index(&properties, &[], &[], &[], &[], &[], &[], &[]);
    parse(&[], |ctx, _| {
        // A seventh slot, a nonnull slot 1, and two patch items each refuse.
        let seven = test_feature(0, 7, &SLOTS);
        assert!(project(ctx, &seven, &label, &index).is_none());
        let slot_one = test_feature(0, 6, &[(0, 0), (1, 1), (2, 1), (3, 2), (4, 3), (5, 4)]);
        assert!(project(ctx, &slot_one, &label, &index).is_none());
        let two = properties(&[7, 8]);
        let index = test_projection_index(&two, &[], &[], &[], &[], &[], &[], &[]);
        let source = test_feature(0, 6, &SLOTS);
        assert!(project(ctx, &source, &label, &index).is_none());
    });
}

#[test]
fn stitch_losses_count_resolved_and_unresolved_features() {
    let outcome = Outcome {
        resolved: 2,
        unresolved: BTreeMap::from([(Unresolved::Faces, 1), (Unresolved::Inputs, 3)]),
    };
    let notes = losses(&outcome);
    assert_eq!(notes.len(), 2);
    assert!(notes[0].message.contains("Typed 2 Stitch Surface"));
    assert!(notes[1].message.contains("4 Stitch Surface"));
    assert!(losses(&Outcome::default()).is_empty());
}
