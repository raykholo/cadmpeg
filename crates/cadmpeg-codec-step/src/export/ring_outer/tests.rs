// SPDX-License-Identifier: Apache-2.0
//! The outer bound of an unclassified cone band.

use crate::export::write_step;
use crate::test_support::cone_band::unclassified_cone_band;
use crate::{StepSchema, StepWriteOptions};

fn written() -> String {
    let mut output = Vec::new();
    write_step(
        &unclassified_cone_band(),
        &mut output,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes the document");
    String::from_utf8(output).expect("utf8")
}

fn records(text: &str, entity: &str) -> usize {
    text.matches(&format!("= {entity}(")).count()
}

/// The larger ring of an unclassified cone band is its outer bound.
#[test]
fn the_larger_ring_of_an_unclassified_cone_band_is_the_outer_bound() {
    let text = written();
    assert_eq!(records(&text, "ADVANCED_FACE"), 1);
    assert_eq!(records(&text, "FACE_OUTER_BOUND"), 1);
    assert_eq!(records(&text, "FACE_BOUND"), 1);
}
