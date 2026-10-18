// SPDX-License-Identifier: Apache-2.0
//! Browser-ordered feature history of one `PmDc` segment.
//!
//! A feature container owns one feature label. That label's participant list
//! is the ordered top level of the browser tree. Wrapper nodes own labels
//! whose participants are nested nodes. A feature label lists the sketches
//! the feature consumes; those sketches precede the feature in the history.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::{FeatureInventory, PmDcFeatureLabel, MODEL_HISTORY_SEGMENT};

/// Content-header flag bit of a suppressed feature or sketch record.
pub(super) const SUPPRESSED_FLAG: u32 = 0x0000_0004;

/// Maximum nesting depth of wrapper labels walked by the history.
const MAX_HISTORY_DEPTH: usize = 64;

/// Kind of a record that occupies a history position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HistoryNodeKind {
    Feature,
    Pattern,
    Sketch,
    WorkPlane,
}

/// Position and state of one record in the browser history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HistoryPlacement {
    pub(super) ordinal: u64,
    pub(super) kind: HistoryNodeKind,
    pub(super) below_end_of_part: bool,
    pub(super) model_history: bool,
    pub(super) origin: bool,
}

/// Ordered history placements keyed by record ordinal in one segment.
#[derive(Debug, Default)]
pub(super) struct History {
    pub(super) placements: HashMap<u32, HistoryPlacement>,
    pub(super) order: Vec<u32>,
    pub(super) next_ordinal: u64,
}

impl History {
    pub(super) fn placement(&self, ordinal: u32) -> Option<HistoryPlacement> {
        self.placements.get(&ordinal).copied()
    }
}

struct Walk<'a, 'b> {
    ctx: &'b DecodeContext<'a>,
    labels: &'b HashMap<u32, &'b PmDcFeatureLabel>,
    kinds: &'b HashMap<u32, Kind>,
    origin_groups: &'b HashSet<u32>,
    visited: HashSet<u32>,
    history: History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Node(HistoryNodeKind),
    EndOfPart,
}

/// Build the browser history of `token` from its feature containers.
///
/// The model container (`PmBRepSegment`) is walked first. Other containers
/// follow in record order. A container without exactly one owned label
/// contributes no history.
pub(super) fn build(
    ctx: &DecodeContext<'_>,
    token: &str,
    inventory: &FeatureInventory,
    sketches: &[crate::sketch::PmDcSketch],
) -> Result<History, CodecError> {
    let mut labels = HashMap::new();
    let mut duplicate_owners = HashSet::new();
    for label in inventory
        .labels
        .iter()
        .filter(|label| label.identity.segment_token.as_str() == token)
    {
        let Some(owner) = label.header.owner.record_ordinal() else {
            continue;
        };
        if ctx
            .insert_hash_map(&mut labels, owner, label, "index Inventor history labels")?
            .is_some()
        {
            ctx.insert_hash_set(
                &mut duplicate_owners,
                owner,
                "index Inventor history labels",
            )?;
        }
    }
    for owner in &duplicate_owners {
        labels.remove(owner);
    }
    let mut kinds = HashMap::new();
    let mut add_kind = |ordinal: u32, kind: Kind| -> Result<(), CodecError> {
        ctx.insert_hash_map(&mut kinds, ordinal, kind, "index Inventor history records")?;
        Ok(())
    };
    for record in inventory
        .features
        .iter()
        .filter(|record| record.identity.segment_token.as_str() == token)
    {
        add_kind(
            record.identity.record_ordinal,
            Kind::Node(HistoryNodeKind::Feature),
        )?;
    }
    for record in inventory
        .pattern_features
        .iter()
        .filter(|record| record.identity.segment_token.as_str() == token)
    {
        add_kind(
            record.identity.record_ordinal,
            Kind::Node(HistoryNodeKind::Pattern),
        )?;
    }
    for record in sketches
        .iter()
        .filter(|record| record.identity.segment_token.as_str() == token)
    {
        add_kind(
            record.identity.record_ordinal,
            Kind::Node(HistoryNodeKind::Sketch),
        )?;
    }
    for record in inventory
        .work_planes
        .iter()
        .filter(|record| record.identity.segment_token.as_str() == token)
    {
        add_kind(
            record.identity.record_ordinal,
            Kind::Node(HistoryNodeKind::WorkPlane),
        )?;
    }
    let mut origin_groups = HashSet::new();
    for record in inventory
        .origin_folders
        .iter()
        .filter(|record| record.identity.segment_token.as_str() == token)
    {
        ctx.insert_hash_set(
            &mut origin_groups,
            record.identity.record_ordinal,
            "index Inventor origin folders",
        )?;
    }
    for record in inventory
        .terminators
        .iter()
        .filter(|record| record.identity.segment_token.as_str() == token)
    {
        add_kind(record.identity.record_ordinal, Kind::EndOfPart)?;
    }
    let mut containers = inventory
        .containers
        .iter()
        .filter(|record| record.identity.segment_token.as_str() == token)
        .collect::<Vec<_>>();
    ctx.sort_unstable_by(
        &mut containers,
        |left, right| {
            (
                left.segment != MODEL_HISTORY_SEGMENT,
                left.identity.record_ordinal,
            )
                .cmp(&(
                    right.segment != MODEL_HISTORY_SEGMENT,
                    right.identity.record_ordinal,
                ))
        },
        |_| 0,
        "order Inventor feature containers",
    )?;
    let mut walk = Walk {
        ctx,
        labels: &labels,
        kinds: &kinds,
        origin_groups: &origin_groups,
        visited: HashSet::new(),
        history: History {
            next_ordinal: 1,
            ..History::default()
        },
    };
    for container in containers {
        let Some(root) = labels.get(&container.identity.record_ordinal) else {
            continue;
        };
        let model = container.segment == MODEL_HISTORY_SEGMENT;
        let members = root
            .participants
            .references()
            .iter()
            .filter_map(|reference| reference.record_ordinal())
            .collect::<Vec<_>>();
        walk.members(&members, 0, model, false, &mut false)?;
    }
    Ok(walk.history)
}

impl Walk<'_, '_> {
    fn members(
        &mut self,
        members: &[u32],
        depth: usize,
        model: bool,
        origin: bool,
        below: &mut bool,
    ) -> Result<(), CodecError> {
        if depth > MAX_HISTORY_DEPTH {
            return Err(self.ctx.refuse_codec_limit(
                "Inventor feature history depth",
                cadmpeg_core::decode::u64_from_index(MAX_HISTORY_DEPTH),
                cadmpeg_core::decode::u64_from_index(depth),
            ));
        }
        for &member in members {
            self.ctx.charge_work(1, "walk Inventor feature history")?;
            if !self.ctx.insert_hash_set(
                &mut self.visited,
                member,
                "walk Inventor feature history",
            )? {
                continue;
            }
            match self.kinds.get(&member).copied() {
                Some(Kind::EndOfPart) => {
                    if depth == 0 {
                        *below = true;
                    }
                }
                Some(Kind::Node(kind)) => {
                    if matches!(kind, HistoryNodeKind::Feature | HistoryNodeKind::Pattern) {
                        let consumed = self
                            .labels
                            .get(&member)
                            .map(|label| {
                                label
                                    .participants
                                    .references()
                                    .iter()
                                    .filter_map(|reference| reference.record_ordinal())
                                    .filter(|ordinal| {
                                        self.kinds.get(ordinal)
                                            == Some(&Kind::Node(HistoryNodeKind::Sketch))
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let mut nested_below = *below;
                        self.members(&consumed, depth + 1, model, origin, &mut nested_below)?;
                    }
                    self.place(member, kind, *below, model, origin)?;
                }
                None => {
                    let Some(label) = self.labels.get(&member) else {
                        continue;
                    };
                    let nested = label
                        .participants
                        .references()
                        .iter()
                        .filter_map(|reference| reference.record_ordinal())
                        .collect::<Vec<_>>();
                    let mut nested_below = *below;
                    let nested_origin = origin || self.origin_groups.contains(&member);
                    self.members(&nested, depth + 1, model, nested_origin, &mut nested_below)?;
                }
            }
        }
        Ok(())
    }

    fn place(
        &mut self,
        member: u32,
        kind: HistoryNodeKind,
        below_end_of_part: bool,
        model_history: bool,
        origin: bool,
    ) -> Result<(), CodecError> {
        let ordinal = self.history.next_ordinal;
        self.history.next_ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("Inventor feature history ordinal overflows"))?;
        self.ctx.insert_hash_map(
            &mut self.history.placements,
            member,
            HistoryPlacement {
                ordinal,
                kind,
                below_end_of_part,
                model_history,
                origin,
            },
            "place Inventor feature history node",
        )?;
        self.ctx.reserve_capacity(
            &mut self.history.order,
            1,
            "place Inventor feature history node",
        )?;
        self.history.order.push(member);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
