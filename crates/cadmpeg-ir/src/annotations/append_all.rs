// SPDX-License-Identifier: Apache-2.0
//! One bulk append of many disjoint annotation tables.
//!
//! Appending tables one at a time rebuilds the growing destination on every
//! call, which is quadratic in the final size. Merging every sorted run in
//! balanced pairwise rounds reads each identity once per round, so the work
//! is the identity bytes times the logarithm of the run count.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use super::{AnnotationIdentityCollision, Annotations, ExactnessNote};
use crate::provenance::AnnotationProvenance;

impl Annotations {
    /// Append every table in `others` in one bulk rebuild.
    ///
    /// An identity named by two different tables, in either provenance or
    /// exactness, is a collision and leaves this table unchanged. All work and
    /// storage are admitted before anything moves.
    pub fn append_all(
        &mut self,
        ctx: &DecodeContext<'_>,
        others: Vec<Self>,
        operation: &'static str,
    ) -> Result<Result<(), AnnotationIdentityCollision>, CodecError> {
        ctx.charge_work(0, operation)?;
        let mut scratch = ctx.reserve_scoped(0, operation)?;
        // Each table's identities, once each, in order.
        let mut runs: Vec<Vec<&str>> = Vec::new();
        let count = others
            .len()
            .checked_add(1)
            .ok_or_else(|| overflow(ctx, operation))?;
        ctx.reserve_scoped_vec(&mut scratch, &mut runs, count, operation)?;
        for table in std::iter::once(&*self).chain(others.iter()) {
            let mut run = Vec::new();
            let len = table
                .provenance
                .len()
                .checked_add(table.exactness.len())
                .ok_or_else(|| overflow(ctx, operation))?;
            ctx.reserve_scoped_vec(&mut scratch, &mut run, len, operation)?;
            let mut provenance = table.provenance.keys().map(String::as_str).peekable();
            let mut exactness = table.exactness.keys().map(String::as_str).peekable();
            loop {
                let next = match (provenance.peek(), exactness.peek()) {
                    (Some(a), Some(b)) => {
                        charge_compare(ctx, a, b, operation)?;
                        match a.cmp(b) {
                            Ordering::Less => provenance.next(),
                            Ordering::Greater => exactness.next(),
                            Ordering::Equal => {
                                exactness.next();
                                provenance.next()
                            }
                        }
                    }
                    (Some(_), None) => provenance.next(),
                    (None, Some(_)) => exactness.next(),
                    (None, None) => break,
                };
                run.extend(next);
            }
            runs.push(run);
        }
        // Balanced pairwise rounds over the identity runs find any identity
        // shared by two tables.
        while runs.len() > 1 {
            let mut merged = Vec::new();
            ctx.reserve_scoped_vec(&mut scratch, &mut merged, runs.len().div_ceil(2), operation)?;
            let mut pairs = runs.into_iter();
            while let Some(left) = pairs.next() {
                let Some(right) = pairs.next() else {
                    merged.push(left);
                    break;
                };
                let mut run = Vec::new();
                let len = left
                    .len()
                    .checked_add(right.len())
                    .ok_or_else(|| overflow(ctx, operation))?;
                ctx.reserve_scoped_vec(&mut scratch, &mut run, len, operation)?;
                let (mut left, mut right) =
                    (left.into_iter().peekable(), right.into_iter().peekable());
                loop {
                    let next = match (left.peek(), right.peek()) {
                        (Some(a), Some(b)) => {
                            charge_compare(ctx, a, b, operation)?;
                            match a.cmp(b) {
                                Ordering::Less => left.next(),
                                Ordering::Greater => right.next(),
                                Ordering::Equal => {
                                    return Ok(Err(AnnotationIdentityCollision {
                                        id: ctx.copy_retained_text(a, operation)?,
                                    }));
                                }
                            }
                        }
                        (Some(_), None) => left.next(),
                        (None, Some(_)) => right.next(),
                        (None, None) => break,
                    };
                    run.extend(next);
                }
                merged.push(run);
            }
            runs = merged;
        }
        drop(runs);
        let provenance_total = total(
            ctx,
            self,
            &others,
            |table| table.provenance.len(),
            operation,
        )?;
        let exactness_total = total(ctx, self, &others, |table| table.exactness.len(), operation)?;
        admit_growth::<AnnotationProvenance>(
            ctx,
            self.provenance.len(),
            provenance_total,
            operation,
        )?;
        admit_growth::<ExactnessNote>(ctx, self.exactness.len(), exactness_total, operation)?;
        // Every fallible step is complete. The rebuild below only moves values.
        let (provenance, exactness): (Vec<_>, Vec<_>) = others
            .into_iter()
            .map(|table| (table.provenance, table.exactness))
            .unzip();
        self.provenance = merge_disjoint(std::mem::take(&mut self.provenance), provenance);
        self.exactness = merge_disjoint(std::mem::take(&mut self.exactness), exactness);
        Ok(Ok(()))
    }
}

fn overflow(ctx: &DecodeContext<'_>, operation: &'static str) -> CodecError {
    ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
}

fn charge_compare(
    ctx: &DecodeContext<'_>,
    a: &str,
    b: &str,
    operation: &'static str,
) -> Result<(), CodecError> {
    let work = u64_from_index(a.len().min(b.len()))
        .checked_add(1)
        .ok_or_else(|| overflow(ctx, operation))?;
    ctx.charge_work(work, operation)
}

fn total(
    ctx: &DecodeContext<'_>,
    first: &Annotations,
    others: &[Annotations],
    len: impl Fn(&Annotations) -> usize,
    operation: &'static str,
) -> Result<usize, CodecError> {
    others.iter().try_fold(len(first), |sum, table| {
        sum.checked_add(len(table))
            .ok_or_else(|| overflow(ctx, operation))
    })
}

/// Admit the destination nodes and entries the bulk append adds.
fn admit_growth<V>(
    ctx: &DecodeContext<'_>,
    from: usize,
    to: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    for len in from..to {
        ctx.admit_btree_node_storage::<String, V>(len, operation)?;
        ctx.charge_collection_items(1, operation)?;
        ctx.charge_work(1, operation)?;
    }
    Ok(())
}

/// Merge disjoint sorted tables in balanced pairwise rounds and build the
/// result in bulk from its strictly increasing entries.
fn merge_disjoint<V>(
    first: BTreeMap<String, V>,
    others: Vec<BTreeMap<String, V>>,
) -> BTreeMap<String, V> {
    let mut runs: Vec<Vec<(String, V)>> = std::iter::once(first)
        .chain(others)
        .map(|table| table.into_iter().collect())
        .collect();
    while runs.len() > 1 {
        let mut merged = Vec::with_capacity(runs.len().div_ceil(2));
        let mut pairs = runs.into_iter();
        while let Some(left) = pairs.next() {
            let Some(right) = pairs.next() else {
                merged.push(left);
                break;
            };
            let mut run = Vec::with_capacity(left.len() + right.len());
            let (mut left, mut right) = (left.into_iter().peekable(), right.into_iter().peekable());
            loop {
                let take_left = match (left.peek(), right.peek()) {
                    (Some(a), Some(b)) => a.0 < b.0,
                    (Some(_), None) => true,
                    (None, Some(_)) => false,
                    (None, None) => break,
                };
                run.extend(if take_left { left.next() } else { right.next() });
            }
            merged.push(run);
        }
        runs = merged;
    }
    runs.into_iter().flatten().collect()
}
