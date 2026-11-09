// SPDX-License-Identifier: Apache-2.0

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::topology::BodyKind;

use crate::design::scope::{PmDcModelRootPayload, MODEL_ROOT_TYPE};
use crate::pmdc::{PmDcReference, PmDcReferenceList};
use crate::record_identity::{Located, RecordTypeId};
use crate::rse::{RseInventory, SegmentKind, SegmentMetaState};
use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream, primary_envelope_fixture_with_kernel, EnvelopeDeclarations,
};

fn with_flat_pattern(
    test: impl FnOnce(&DecodeContext<'_>, RseInventory<'_>, crate::design::DesignInventory),
) {
    let bytes = primary_envelope_fixture_with_kernel(
        EnvelopeDeclarations::default(),
        &acis_sphere_kernel_stream(21_800),
    );
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("context");
    let compound = CompoundSnapshot::new(&ctx, view).expect("compound");
    let mut rse = RseInventory::build(&ctx, &compound).expect("RSe");
    let mut design = crate::design::inventory(&ctx, &rse).expect("design");
    let segment = &mut rse.segments[0];
    segment.kind = SegmentKind::MbBRep;
    let SegmentMetaState::Parsed(meta) = &segment.meta else {
        panic!("metadata")
    };
    design.model_roots.push(Located::new(
        PmDcModelRootPayload {
            save_version_major: 19,
            header_value: 0,
            header_id: 0,
            next: PmDcReference::from_packed(0),
            flags: 0x40,
            flags_extension: None,
            identifier: "0".repeat(32),
            name: String::new(),
            members: PmDcReferenceList::new(2, None, Vec::new()).expect("empty members"),
            values: [0; 15],
            parameter_table: PmDcReference::from_packed(0),
            brep_segment: meta.display_name.clone(),
            scale: cadmpeg_ir::scalar::FiniteReal::ONE,
            terminal_value: 0,
        },
        RecordTypeId::from_bytes(MODEL_ROOT_TYPE),
        cadmpeg_ir::identity_key!("design"),
        0,
    ));
    test(&ctx, rse, design);
}

#[test]
fn flat_pattern_root_reads_its_stored_solid() {
    with_flat_pattern(|ctx, rse, design| {
        let bodies = crate::flat_pattern::read(ctx, &rse, &design).expect("read");
        assert_eq!(bodies.len(), 1);
        let body = bodies.into_iter().next().expect("model").expect("kernel");
        assert_eq!(body.brep.bodies.len(), 1);
        assert_eq!(body.brep.bodies[0].kind, BodyKind::Solid);
        assert_eq!(body.brep.faces.len(), 1);
    });
}

#[test]
fn flat_pattern_requires_a_unique_named_mb_segment() {
    with_flat_pattern(|ctx, mut rse, mut design| {
        let name = design.model_roots[0].brep_segment.clone();
        design.model_roots[0].edit(|root| root.brep_segment = "absent".into());
        assert!(crate::flat_pattern::read(ctx, &rse, &design)
            .expect("missing")
            .is_empty());
        design.model_roots[0].edit(|root| root.brep_segment = name);
        rse.segments[0].kind = SegmentKind::PmBRep;
        assert!(crate::flat_pattern::read(ctx, &rse, &design)
            .expect("folded")
            .is_empty());
        rse.segments[0].kind = SegmentKind::MbBRep;
        design.model_roots.clear();
        assert!(crate::flat_pattern::read(ctx, &rse, &design)
            .expect("no root")
            .is_empty());
    });
}

#[test]
fn flat_pattern_resource_limit_propagates() {
    with_flat_pattern(|_, rse, design| {
        for max_entities in [0, 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_entities = max_entities;
            let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).expect("context");
            assert!(matches!(
                crate::flat_pattern::read(&ctx, &rse, &design),
                Err(CodecError::ResourceLimit(_))
            ));
        }
    });
}

#[test]
fn flat_pattern_rejects_ambiguous_segment_names() {
    with_flat_pattern(|ctx, rse, design| {
        with_flat_pattern(|_, mut duplicate, _| {
            let mut rse = rse;
            rse.segments.push(duplicate.segments.remove(0));
            assert!(crate::flat_pattern::read(ctx, &rse, &design)
                .expect("ambiguous")
                .is_empty());
        });
    });
}

#[test]
fn flat_pattern_retains_carrier_failure_internally() {
    with_flat_pattern(|ctx, mut rse, design| {
        rse.segments[0].registry = None;
        let bodies = crate::flat_pattern::read(ctx, &rse, &design).expect("read");
        assert_eq!(bodies.len(), 1);
        let error = bodies
            .into_iter()
            .next()
            .expect("model")
            .err()
            .expect("failure");
        assert!(error.contains("segment version is unavailable"));
    });
}
