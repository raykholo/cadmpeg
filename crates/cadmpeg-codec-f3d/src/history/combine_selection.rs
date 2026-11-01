// SPDX-License-Identifier: Apache-2.0
//! Admit Combine selection refinements while retaining source operands on refusal.

use cadmpeg_ir::features::{BodySelection, CombineOperands, FeatureId};
use cadmpeg_ir::report::loss::LossNote;

use crate::F3dLossCode;

pub(super) fn refine(
    operands: &mut CombineOperands,
    feature: &FeatureId,
    edit: impl FnOnce(&mut BodySelection, &mut BodySelection),
) -> Option<LossNote> {
    operands.try_edit(edit).err().map(|error| {
        F3dLossCode::FeatureCombineSelectionNative.note(format!(
            "feature {feature} retains its source Combine selections: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_selection_refinement_retains_disjoint_source_operands_on_overlap() {
        let feature = FeatureId::mint("f3d:test:feature#combine").unwrap();
        let source = CombineOperands::new(
            BodySelection::Native("f3d:test:selection#target".into()),
            BodySelection::Native("f3d:test:selection#tools".into()),
        )
        .unwrap();
        let mut operands = source.clone();
        let loss = refine(&mut operands, &feature, |target, tools| {
            *tools = target.clone();
        })
        .unwrap();
        assert_eq!(operands, source);
        assert_eq!(
            loss.code,
            F3dLossCode::FeatureCombineSelectionNative.note("").code
        );

        let loss = refine(&mut operands, &feature, |_, tools| {
            *tools = BodySelection::Native("f3d:test:selection#other-tool".into());
        });
        assert!(loss.is_none());
        assert_eq!(
            operands.tools(),
            &BodySelection::Native("f3d:test:selection#other-tool".into())
        );
    }
}
