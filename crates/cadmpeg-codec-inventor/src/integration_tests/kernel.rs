// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::InventorLossCode;
use crate::test_support::test_fixtures::{
    empty_asm_history_kernel_stream, primary_envelope_fixture_with_kernel, EnvelopeDeclarations,
};
use crate::InventorCodec;

#[test]
fn empty_solved_partition_retains_the_carrier_and_reports_the_blocking_loss() {
    let kernel = empty_asm_history_kernel_stream();
    let bytes = primary_envelope_fixture_with_kernel(EnvelopeDeclarations::default(), &kernel);
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("empty solved partition decodes");
    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.ir().model.faces.is_empty());
    assert!(decoded.ir().model.features.is_empty());
    assert!(!decoded.report().geometry_transferred());
    let losses: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == InventorLossCode::GeometryKernelCarrierNotTransferred.kind())
        .collect();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].message,
        "the part has no bodies: its active kernel carrier holds no model records"
    );
    assert_eq!(
        losses[0].severity,
        cadmpeg_ir::report::Severity::Blocking
    );
    assert!(!decoded.source_fidelity().retained_records().is_empty());
    let native = decoded
        .ir()
        .native
        .namespace("inventor")
        .expect("native carrier");
    let carriers = native
        .arena_as::<crate::native::ActiveCarrierRecord>("active_carrier")
        .expect("carrier arena");
    assert!(matches!(
        carriers.as_slice(),
        [crate::native::ActiveCarrierRecord::Selected { .. }]
    ));
}
