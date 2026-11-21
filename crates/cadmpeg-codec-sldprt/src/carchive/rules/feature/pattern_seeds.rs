// SPDX-License-Identifier: Apache-2.0
//! Pattern-owned archive seed lists.

use crate::carchive::reader::{Ptr, Reader, Res, V};
use crate::carchive::PatternSeeds;

pub(super) fn read(reader: &mut Reader<'_>, owner: &V) -> Res<()> {
    let at = reader.p;
    let record_index = owner
        .get("oid")
        .as_int()
        .and_then(|id| u32::try_from(id).ok())
        .map(|owner| {
            let index = reader.pattern_seeds.len();
            reader.pattern_seeds.push(PatternSeeds {
                owner,
                at,
                seeds: None,
            });
            index
        });
    let list = reader.ptr(Some(&["suObList"]))?;
    let seeds = list.value().and_then(V::as_list).map(|items| {
        items
            .iter()
            .map(|item| {
                let V::Ptr(pointer) = item else {
                    return None;
                };
                match &**pointer {
                    Ptr::Obj { value, .. } => u32::try_from(value.get("oid").as_int()?).ok(),
                    Ptr::Ref { index, .. } => reader
                        .object_starts
                        .get(index)
                        .and_then(|start| reader.sources.get(start))
                        .copied(),
                    Ptr::Null => None,
                }
            })
            .collect()
    });
    if let Some(index) = record_index {
        reader.pattern_seeds[index].seeds = seeds;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
