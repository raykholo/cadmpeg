// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::annotations::{
    AnnotationBuilder, AnnotationIdentityCollision, Annotations, StreamHandle,
};
use crate::provenance::Exactness;

/// Interleaved rewritten (`f3d:`) and kept (`acis:`, `zz:`) identities, with
/// provenance on every identity, exactness on every third, and four streams.
fn fixture(count: usize) -> Annotations {
    let ctx = cadmpeg_test_support::service_decode_context();
    let streams: Vec<StreamHandle> = ["a", "b/c", "d", "e"]
        .into_iter()
        .map(|name| {
            StreamHandle::new(
                &ctx,
                crate::StreamName::try_from(name.to_owned()).unwrap(),
                "fixture stream handle",
            )
            .unwrap()
        })
        .collect();
    let mut builder = AnnotationBuilder::new();
    for index in 0..count {
        let id = match index % 3 {
            0 => format!("f3d:part:face#{index:06}"),
            1 => format!("acis:body#{index:06}"),
            _ => format!("zz:tail#{index:06}"),
        };
        let stream = &streams[(index / 7) % streams.len()];
        let tag = (index % 2 == 0).then_some("face");
        builder
            .note(&ctx, &id, stream, u64::try_from(index).unwrap(), tag)
            .unwrap();
        if index % 3 == 0 || index % 5 == 0 {
            builder.exactness(&ctx, &id, Exactness::Inferred).unwrap();
        }
    }
    builder.build()
}

fn rescoped_id(id: &str, from: &str, to: &str) -> String {
    id.strip_prefix(from)
        .map_or_else(|| id.to_owned(), |rest| format!("{to}{rest}"))
}

fn assert_rescoped(before: &Annotations, after: &Annotations, from: &str, to: &str, owner: &str) {
    let expected: BTreeMap<String, (String, u64, Option<String>)> = before
        .provenance
        .iter()
        .map(|(id, value)| {
            (
                rescoped_id(id, from, to),
                (
                    format!("{owner}{}", value.stream()),
                    value.offset,
                    value.tag.clone(),
                ),
            )
        })
        .collect();
    let actual: BTreeMap<String, (String, u64, Option<String>)> = after
        .provenance
        .iter()
        .map(|(id, value)| {
            (
                id.clone(),
                (value.stream().to_owned(), value.offset, value.tag.clone()),
            )
        })
        .collect();
    assert_eq!(actual, expected);
    let expected: BTreeMap<String, _> = before
        .exactness()
        .iter()
        .map(|(id, note)| (rescoped_id(id, from, to), note.clone()))
        .collect();
    assert_eq!(after.exactness(), &expected);
}

fn identity_bytes(annotations: &Annotations) -> u64 {
    annotations
        .provenance
        .keys()
        .chain(annotations.exactness().keys())
        .map(|id| u64::try_from(id.len() + 1).unwrap())
        .sum()
}

/// Whether `apply` completes when the session allows `limit` work units.
fn fits_work(limit: u64, apply: impl FnOnce(&DecodeContext<'_>) -> bool) -> bool {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    apply(&ctx)
}

#[test]
fn many_identities_rescope_in_order_with_streams_tags_and_exactness() {
    let before = fixture(30_000);
    let mut after = before.clone();
    let ctx = cadmpeg_test_support::service_decode_context();
    after
        .rescope_prefix(
            &ctx,
            "f3d:",
            "f3d:xref/occ-1/",
            "f3d:xref/occ-1/",
            "test rescope",
        )
        .unwrap()
        .unwrap();
    assert_rescoped(
        &before,
        &after,
        "f3d:",
        "f3d:xref/occ-1/",
        "f3d:xref/occ-1/",
    );
}

#[test]
fn many_identity_rescoping_work_is_linear_in_identity_bytes() {
    // A linear bound: a fixed multiple of the identity bytes. The per-identity
    // tree remapping needs a logarithmic factor more and exceeds it.
    let annotations = fixture(30_000);
    let bound = identity_bytes(&annotations) * 16;
    let linear = annotations.clone();
    assert!(fits_work(bound, |ctx| {
        let mut annotations = linear;
        matches!(
            annotations.rescope_prefix(ctx, "f3d:", "f3d:xref/o/", "f3d:xref/o/", "test rescope"),
            Ok(Ok(()))
        )
    }));
    assert!(!fits_work(bound, |ctx| {
        let mut annotations = annotations;
        matches!(
            annotations.map_ids(
                ctx,
                |id| Ok(rescoped_id(id, "f3d:", "f3d:xref/o/")),
                "test remap",
            ),
            Ok(Ok(()))
        )
    }));
}

#[test]
fn rewritten_run_merges_ahead_of_kept_identities() {
    // `b:` moves ahead of `a:2`, so the two runs interleave in the result.
    let ctx = cadmpeg_test_support::service_decode_context();
    let stream =
        StreamHandle::new(&ctx, crate::stream_name!("s"), "fixture stream handle").unwrap();
    let mut builder = AnnotationBuilder::new();
    for (offset, id) in ["a:2", "b:1", "b:3", "c:0"].into_iter().enumerate() {
        builder
            .note(&ctx, id, &stream, u64::try_from(offset).unwrap(), None)
            .unwrap();
    }
    builder.exactness(&ctx, "b:3", Exactness::Derived).unwrap();
    let before = builder.build();
    let mut after = before.clone();
    after
        .rescope_prefix(&ctx, "b:", "a:", "owner/", "test rescope")
        .unwrap()
        .unwrap();
    assert_eq!(
        after
            .provenance
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["a:1", "a:2", "a:3", "c:0"]
    );
    assert_rescoped(&before, &after, "b:", "a:", "owner/");
}

#[test]
fn rescoping_refuses_collisions_within_and_across_tables_without_mutation() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let stream =
        StreamHandle::new(&ctx, crate::stream_name!("s"), "fixture stream handle").unwrap();
    // Within provenance: `b:1` becomes the kept `a:1`.
    let mut builder = AnnotationBuilder::new();
    builder.note(&ctx, "a:1", &stream, 0, None).unwrap();
    builder.note(&ctx, "b:1", &stream, 1, None).unwrap();
    let mut within = builder.build();
    // Across tables: provenance `b:1` becomes exactness-only `a:1`.
    let mut builder = AnnotationBuilder::new();
    builder.note(&ctx, "b:1", &stream, 1, None).unwrap();
    builder.exactness(&ctx, "a:1", Exactness::Unknown).unwrap();
    let mut across = builder.build();
    for annotations in [&mut within, &mut across] {
        let before = annotations.clone();
        let error = annotations
            .rescope_prefix(&ctx, "b:", "a:", "owner/", "test rescope")
            .unwrap()
            .unwrap_err();
        assert!(matches!(error, AnnotationIdentityCollision { id } if id == "a:1"));
        assert_eq!(*annotations, before);
    }
}

#[test]
fn rescoping_refuses_resource_limits_without_mutation() {
    let before = fixture(64);
    for limit in [0, 1, 10, 50] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut annotations = before.clone();
        let result = annotations.rescope_prefix(&ctx, "f3d:", "f3d:x/", "o/", "test rescope");
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        assert_eq!(annotations, before);
    }
}
