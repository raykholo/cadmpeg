// SPDX-License-Identifier: Apache-2.0
//! Ordered identity-prefix rescoping of annotation tables.
//!
//! Replacing one common identity prefix by another keeps the rewritten
//! identities in their stored order, and the identities left alone keep
//! theirs. Each rescoped table is therefore the merge of two sorted runs, so
//! the rewrite needs no per-identity tree search: its work is linear in the
//! identity bytes.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::Arc;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

use super::{AnnotationIdentityCollision, Annotations, ExactnessNote, StreamHandle};
use crate::provenance::{AnnotationProvenance, StreamName};

impl Annotations {
    /// Rewrite every identity that starts with `from` to start with `to`,
    /// keep every other identity, and prefix every provenance stream with
    /// `stream_owner`.
    ///
    /// All work, storage and the collision check are admitted before either
    /// table changes. A rewritten identity equal to a different kept identity
    /// in either table is a collision and leaves both tables unchanged.
    pub fn rescope_prefix(
        &mut self,
        ctx: &DecodeContext<'_>,
        from: &str,
        to: &str,
        stream_owner: &str,
        operation: &'static str,
    ) -> Result<Result<(), AnnotationIdentityCollision>, CodecError> {
        let mut scratch = ctx.reserve_scoped(0, operation)?;
        let provenance = plan_table(ctx, &mut scratch, &self.provenance, from, to, operation)?;
        let exactness = plan_table(ctx, &mut scratch, &self.exactness, from, to, operation)?;
        // Within one table a collision is an equal key across its two runs.
        // Across tables a rewritten identity can equal a kept identity of the
        // other table. Equal identities of the same run name one entity.
        for (rewritten, kept) in [
            (&provenance, &provenance),
            (&exactness, &exactness),
            (&provenance, &exactness),
            (&exactness, &provenance),
        ] {
            if let Some(id) = first_equal(ctx, &rewritten.order.rewritten, &kept.kept, operation)? {
                return Ok(Err(AnnotationIdentityCollision {
                    id: ctx.copy_retained_text(id, operation)?,
                }));
            }
        }
        let streams = plan_streams(ctx, &mut scratch, &self.provenance, stream_owner, operation)?;
        admit_nodes::<AnnotationProvenance>(ctx, self.provenance.len(), operation)?;
        admit_nodes::<ExactnessNote>(ctx, self.exactness.len(), operation)?;
        let provenance = provenance.into_order();
        let exactness = exactness.into_order();
        // Every fallible step is complete. The rebuild below only moves values.
        let mut streams = streams.into_iter();
        self.provenance = apply_order(std::mem::take(&mut self.provenance), provenance)
            .into_iter()
            .map(|(id, mut value)| {
                if let Some(stream) = streams.next() {
                    value.set_stream_handle(stream);
                }
                (id, value)
            })
            .collect();
        self.exactness = apply_order(std::mem::take(&mut self.exactness), exactness)
            .into_iter()
            .collect();
        Ok(Ok(()))
    }
}

/// New identities for one table, split into its rewritten and kept runs.
struct TablePlan<'t> {
    /// Kept identities in stored order.
    kept: Vec<&'t str>,
    order: MergeOrder,
}

/// How stored entries map onto the rescoped table, without borrowing it.
struct MergeOrder {
    /// Rewritten identities in stored order.
    rewritten: Vec<String>,
    /// For each stored entry, whether it belongs to the rewritten run.
    is_rewritten: Vec<bool>,
    /// For each rescoped position, whether it takes the next rewritten entry.
    take_rewritten: Vec<bool>,
}

impl TablePlan<'_> {
    fn into_order(self) -> MergeOrder {
        self.order
    }
}

fn plan_table<'t, V>(
    ctx: &DecodeContext<'_>,
    scratch: &mut ScopedReservation<'_>,
    table: &'t BTreeMap<String, V>,
    from: &str,
    to: &str,
    operation: &'static str,
) -> Result<TablePlan<'t>, CodecError> {
    let mut rewritten = Vec::new();
    let mut kept = Vec::new();
    let mut is_rewritten = Vec::new();
    let mut take_rewritten = Vec::new();
    ctx.reserve_scoped_vec(scratch, &mut is_rewritten, table.len(), operation)?;
    ctx.reserve_scoped_vec(scratch, &mut take_rewritten, table.len(), operation)?;
    for id in table.keys() {
        ctx.charge_work(text_work(ctx, from.len(), operation)?, operation)?;
        is_rewritten.push(id.starts_with(from));
    }
    let rewritten_count = is_rewritten.iter().filter(|value| **value).count();
    ctx.reserve_scoped_vec(scratch, &mut rewritten, rewritten_count, operation)?;
    ctx.reserve_scoped_vec(
        scratch,
        &mut kept,
        is_rewritten.iter().filter(|value| !**value).count(),
        operation,
    )?;
    for id in table.keys() {
        if let Some(rest) = id.strip_prefix(from) {
            rewritten.push(ctx.format_retained(format_args!("{to}{rest}"), operation)?);
        } else {
            kept.push(id.as_str());
        }
    }
    // Merge the two sorted runs. Each step reads at most the shorter key and
    // advances one run, so the merge is linear in identity bytes.
    let (mut left, mut right) = (0usize, 0usize);
    loop {
        let take = match (rewritten.get(left), kept.get(right)) {
            (Some(a), Some(b)) => {
                charge_compare(ctx, a, b, operation)?;
                a.as_str() < *b
            }
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if take {
            left += 1;
        } else {
            right += 1;
        }
        take_rewritten.push(take);
    }
    Ok(TablePlan {
        kept,
        order: MergeOrder {
            rewritten,
            is_rewritten,
            take_rewritten,
        },
    })
}

/// The first identity present in both sorted runs.
fn first_equal<'p>(
    ctx: &DecodeContext<'_>,
    rewritten: &'p [String],
    kept: &[&str],
    operation: &'static str,
) -> Result<Option<&'p str>, CodecError> {
    let (mut left, mut right) = (0usize, 0usize);
    while let (Some(a), Some(b)) = (rewritten.get(left), kept.get(right)) {
        charge_compare(ctx, a, b, operation)?;
        match a.as_str().cmp(b) {
            Ordering::Less => left += 1,
            Ordering::Greater => right += 1,
            Ordering::Equal => return Ok(Some(a)),
        }
    }
    Ok(None)
}

/// One unit per byte read plus one for the step.
fn text_work(
    ctx: &DecodeContext<'_>,
    bytes: usize,
    operation: &'static str,
) -> Result<u64, CodecError> {
    u64_from_index(bytes)
        .checked_add(1)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))
}

fn charge_compare(
    ctx: &DecodeContext<'_>,
    a: &str,
    b: &str,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_work(text_work(ctx, a.len().min(b.len()), operation)?, operation)
}

/// One owner-prefixed stream per provenance entry, in stored order.
/// Consecutive entries naming the same stream share one admitted handle.
fn plan_streams(
    ctx: &DecodeContext<'_>,
    scratch: &mut ScopedReservation<'_>,
    provenance: &BTreeMap<String, AnnotationProvenance>,
    stream_owner: &str,
    operation: &'static str,
) -> Result<Vec<Arc<StreamName>>, CodecError> {
    let mut streams: Vec<Arc<StreamName>> = Vec::new();
    ctx.reserve_scoped_vec(scratch, &mut streams, provenance.len(), operation)?;
    let mut previous: Option<(&StreamName, Arc<StreamName>)> = None;
    for value in provenance.values() {
        let source = value.stream_handle().as_ref();
        ctx.charge_work(text_work(ctx, source.as_str().len(), operation)?, operation)?;
        let shared = previous
            .as_ref()
            .filter(|(last, _)| *last == source)
            .map(|(_, handle)| Arc::clone(handle));
        let handle = if let Some(handle) = shared {
            handle
        } else {
            let name =
                ctx.format_retained(format_args!("{stream_owner}{}", source.as_str()), operation)?;
            let name = StreamName::try_from(name).map_err(CodecError::malformed)?;
            let handle = StreamHandle::new(ctx, name, "allocate annotation stream handle")?.0;
            previous = Some((source, Arc::clone(&handle)));
            handle
        };
        streams.push(handle);
    }
    Ok(streams)
}

/// Admit the rebuilt table's nodes and entries.
fn admit_nodes<V>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    for len in 0..count {
        ctx.admit_btree_node_storage::<String, V>(len, operation)?;
        ctx.charge_collection_items(1, operation)?;
        ctx.charge_work(1, operation)?;
    }
    Ok(())
}

/// Move each stored value to its new identity in rescoped order. The result
/// is strictly increasing, so collecting it builds the map in bulk.
fn apply_order<V>(table: BTreeMap<String, V>, order: MergeOrder) -> Vec<(String, V)> {
    let mut rewritten = Vec::with_capacity(order.rewritten.len());
    let mut kept = Vec::with_capacity(order.is_rewritten.iter().filter(|value| !**value).count());
    let mut rewritten_ids = order.rewritten.into_iter();
    for ((id, value), is_rewritten) in table.into_iter().zip(order.is_rewritten) {
        if !is_rewritten {
            kept.push((id, value));
        } else if let Some(id) = rewritten_ids.next() {
            rewritten.push((id, value));
        }
    }
    let mut merged = Vec::with_capacity(rewritten.len() + kept.len());
    let mut rewritten = rewritten.into_iter();
    let mut kept = kept.into_iter();
    for take_rewritten in order.take_rewritten {
        let next = if take_rewritten {
            rewritten.next()
        } else {
            kept.next()
        };
        merged.extend(next);
    }
    merged
}
