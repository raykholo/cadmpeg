//! An exact reader of the MFC `CArchive` object graph in a configuration's
//! `Contents/Config-N-ResolvedFeatures` stream (`docs/formats/sldprt.md` § 2.3).
//!
//! The stream continues the class map of `Contents/Config-N`: its leading u32
//! is the map count at which that stream ended, and its class tags name that
//! stream's indices ([`crate::archive::class_index_map`]). From there every
//! object is read by its class's rule, so the map counter stays exact and a
//! class tag inside the stream names one class.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use cadmpeg_core::decode::{DecodeContext, View};

#[cfg(test)]
mod tests;

pub(crate) mod reader;
pub(crate) mod rules;

/// How a walk ended.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Status {
    /// Every top-level object was read to the end of the stream.
    End,
    /// A rule stopped the walk.
    Stop(reader::Stop),
}

/// A walk of one `ResolvedFeatures` stream.
#[derive(Debug, Clone)]
pub(crate) struct Walk {
    pub(crate) status: Status,
    /// Every object opened, in order.
    #[cfg(test)]
    pub(crate) trace: Vec<reader::Trace>,
    /// Parameter objects read.
    pub(crate) parameters: Vec<reader::ParameterRecord>,
    /// The sketch profile bodies read, each with the extent of the
    /// profile-family object that holds it.
    pub(crate) sketch_profiles: Vec<SketchProfile>,
    /// The `moPMarkRecord_c` objects read, each with the named object that
    /// holds it.
    pub(crate) marks: Vec<reader::MarkOwner>,
    pub(crate) edge_references: Vec<EdgeOperand>,
    pub(crate) reference_rules: ReferenceRules,
    /// Top-level pointers read.
    #[cfg(test)]
    pub(crate) tops: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ReferenceRules {
    known: HashMap<u32, Arc<str>>,
    counter: u32,
    version: Option<u32>,
}

impl ReferenceRules {
    pub(crate) fn surface_hops(
        &self,
        ctx: &DecodeContext<'_>,
        payload: &[u8],
        marker: usize,
    ) -> Result<Option<[rules::idrep::SurfaceIdentity; 4]>, cadmpeg_core::CodecError> {
        use crate::layout::identity_reference_header as header;
        let Some(start) = marker.checked_sub(header::MARKER) else {
            return Ok(None);
        };
        if start
            .checked_add(header::LEN)
            .is_none_or(|end| end > payload.len())
        {
            return Ok(None);
        }
        if View::u32_le_at(payload, start + header::FORM) != Some(1)
            || View::u32_le_at(payload, start + header::ZERO_WORD) != Some(0)
            || View::u32_le_at(payload, start + header::HOP_COUNT) != Some(4)
            || View::u32_le_at(
                payload,
                start + header::HOP_COUNT + std::mem::size_of::<u32>(),
            )
            .is_none_or(|selector| selector & 0xffff_ff00 != 0x0000_0200)
            || View::u16_le_at(payload, start + header::ZERO_TAIL) != Some(0)
        {
            return Ok(None);
        }
        let mut reader = reader::Reader::new(
            ctx,
            payload,
            start,
            self.known.clone(),
            self.counter,
            self.version,
        );
        match rules::refs::surface_hops(&mut reader) {
            Ok(faces) => Ok(faces),
            Err(reader::Stop::Refused(limit)) => Err(limit.into()),
            Err(_) => Ok(None),
        }
    }
}

/// A component-edge operand in a completed named object.
#[derive(Debug, Clone)]
pub(crate) struct EdgeOperand {
    pub(crate) owner: std::ops::Range<usize>,
    pub(crate) marker: usize,
    pub(crate) faces: Option<[rules::idrep::SurfaceIdentity; 4]>,
}

/// A sketch's profile body and the stream extent of the profile-family object
/// that holds it (§2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SketchProfile {
    /// Where the body's nested stream starts.
    pub(crate) stream: usize,
    /// Where the holding object's tag starts.
    pub(crate) owner_start: usize,
    /// Where the holding object ends.
    pub(crate) owner_end: usize,
}

/// The profile bodies of `bodies` whose holding object was read to its end.
fn sketch_profiles(
    bodies: &[reader::ProfileBody],
    ends: &HashMap<usize, usize>,
) -> Vec<SketchProfile> {
    bodies
        .iter()
        .filter_map(|body| {
            Some(SketchProfile {
                stream: body.stream,
                owner_start: body.owner,
                owner_end: *ends.get(&body.owner)?,
            })
        })
        .collect()
}

fn edge_operands(
    references: &[reader::EdgeReference],
    ends: &HashMap<usize, usize>,
) -> Vec<EdgeOperand> {
    references
        .iter()
        .filter_map(|edge| {
            let owner = edge.owner?;
            Some(EdgeOperand {
                owner: owner..*ends.get(&owner)?,
                marker: edge.marker,
                faces: edge.faces,
            })
        })
        .collect()
}

/// Walk `resolved` (a `Config-N-ResolvedFeatures` stream) with the class map
/// `known` its `Config-N` stream ended with (`ftop.run`): the map count u32,
/// a u16, then top-level objects to the end.
pub(crate) fn walk<'b>(
    ctx: &'b DecodeContext<'b>,
    known: &BTreeMap<u32, String>,
    resolved: &'b [u8],
    ver: Option<u32>,
) -> Option<Walk> {
    let counter = View::u32_le_at(resolved, 0)?;
    let known = known
        .iter()
        .map(|(&index, name)| (index, Arc::<str>::from(name.as_str())))
        .collect::<HashMap<_, _>>();
    let mut r = reader::Reader::new(ctx, resolved, 6, known, counter, ver);
    #[cfg(test)]
    let mut tops = 0;
    let status = loop {
        if r.p + 1 >= resolved.len() {
            break Status::End;
        }
        match r.ptr(None) {
            Ok(_) => {
                #[cfg(test)]
                {
                    tops += 1;
                }
            }
            Err(stop) => break Status::Stop(stop),
        }
    };
    let edge_references = edge_operands(&r.edge_references, &r.ends);
    Some(Walk {
        sketch_profiles: sketch_profiles(&r.profile_bodies, &r.ends),
        status,
        parameters: r.parameters,
        marks: r.marks,
        edge_references,
        reference_rules: ReferenceRules {
            known: r.known,
            counter: r.counter,
            version: ver,
        },
        #[cfg(test)]
        trace: r.trace,
        #[cfg(test)]
        tops,
    })
}

/// Walk a configuration's `ResolvedFeatures` stream from its `Config-N`
/// stream's class map: `None` when that map does not reconcile with the
/// stream's count, or the stream carries no count. A decode-context refusal
/// is an error, not a stop.
pub(crate) fn walk_configuration(
    ctx: &DecodeContext<'_>,
    config: &[u8],
    resolved: &[u8],
    ver: Option<u32>,
) -> Result<Option<Walk>, cadmpeg_core::CodecError> {
    let Some(count) = crate::archive::resolved_features_count(resolved) else {
        return Ok(None);
    };
    let Some(map) = crate::archive::class_index_map(config, count, ctx)? else {
        return Ok(None);
    };
    let walk = walk(ctx, &map, resolved, ver);
    if let Some(Status::Stop(reader::Stop::Refused(limit))) = walk.as_ref().map(|walk| &walk.status)
    {
        return Err((*limit).into());
    }
    Ok(walk)
}

/// The version the walk reads a configuration with: the declared `swVersion`
/// when it is a number, else the u32 at offset 22 of the `Config-N` stream
/// when it lies in 9000..=40000. `None` when the file declares neither: the
/// walk does not substitute a grammar for an undeclared dialect.
pub(crate) fn walk_version(declared: Option<&str>, config: &[u8]) -> Option<u32> {
    if let Some(version) = declared.and_then(|value| value.trim().parse::<u32>().ok()) {
        return Some(version);
    }
    View::u32_le_at(config, 22).filter(|value| (9000..=40000).contains(value))
}
