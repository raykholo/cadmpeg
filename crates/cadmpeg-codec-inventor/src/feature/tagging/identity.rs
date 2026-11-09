// SPDX-License-Identifier: Apache-2.0
//! Internal comparison of the materialized states adjacent to Tagging.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use cadmpeg_asm::history::{
    decode_record_archive, frame_history_chain, walk_entity_versions, ChainFraming, EntityChange,
};
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_asm::nurbs::reader::LEN_TO_MM;
use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

use crate::feature::history_states::{FeatureHistoryState, StateIndex};

const EPS_TAGGING_SURFACE_PARAMETER_E10: f64 = 1.0e-10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Same,
    Changed,
    Undecided(Refusal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    NoStateJoin,
    NoPredecessor,
    SharedState,
    StateNotMaterialized,
    IncompleteOwnership,
    UnpairedEntities,
    UnsupportedSurface,
    OtherGeometry,
}

fn charge(ctx: &DecodeContext<'_>, count: usize) -> Result<(), CodecError> {
    ctx.charge_entities(u64_from_index(count), "compare Tagging history geometry")
}

/// Compare stable body and face slots. Complete analytic carriers supply the
/// surface parameters. An exact remainder check protects trims and placement.
pub(crate) fn tagging_identity(
    ctx: &DecodeContext<'_>,
    before: &[Record],
    after: &[Record],
) -> Result<Outcome, CodecError> {
    charge(ctx, before.len())?;
    charge(ctx, after.len())?;
    let before_map: Records<'_> = before.iter().map(|r| (r.index, r)).collect();
    let after_map: Records<'_> = after.iter().map(|r| (r.index, r)).collect();
    for (records, map) in [(before, &before_map), (after, &after_map)] {
        if map.len() != records.len() || records.is_empty() {
            return Ok(Outcome::Undecided(Refusal::StateNotMaterialized));
        }
        for record in records {
            charge(ctx, record.tokens.len())?;
            if record.tokens.iter().any(|t| matches!(t, Token::Ref(r) if *r >= 0 && usize::try_from(*r).ok().is_none_or(|r| !map.contains_key(&r)))) {
                return Ok(Outcome::Undecided(Refusal::StateNotMaterialized));
            }
        }
    }
    let (Some(left), Some(right)) = (body_faces(ctx, &before_map)?, body_faces(ctx, &after_map)?)
    else {
        return Ok(Outcome::Undecided(Refusal::IncompleteOwnership));
    };
    if left.len() != right.len() {
        return Ok(Outcome::Changed);
    }
    if left.keys().ne(right.keys()) {
        return Ok(Outcome::Undecided(Refusal::UnpairedEntities));
    }
    let mut surfaces = BTreeSet::new();
    for (body, faces) in &left {
        let other = &right[body];
        if faces.len() != other.len() {
            return Ok(Outcome::Changed);
        }
        if faces != other {
            return Ok(Outcome::Undecided(Refusal::UnpairedEntities));
        }
        for face in faces {
            let (Some(a), Some(b)) = (surface(&before_map, *face), surface(&after_map, *face))
            else {
                return Ok(Outcome::Undecided(Refusal::UnsupportedSurface));
            };
            charge(ctx, a.tokens.len())?;
            charge(ctx, b.tokens.len())?;
            let (Some(left), Some(right)) = (analytic(a), analytic(b)) else {
                return Ok(Outcome::Undecided(Refusal::UnsupportedSurface));
            };
            if a.index != b.index {
                return Ok(Outcome::Undecided(Refusal::UnpairedEntities));
            }
            if left.kind != right.kind
                || left.reversed != right.reversed
                || !left
                    .scalars
                    .iter()
                    .zip(right.scalars)
                    .all(|(a, b)| close(*a, b))
            {
                return Ok(Outcome::Changed);
            }
            if a.name != b.name || !parameters_equal(a, b) {
                return Ok(Outcome::Undecided(Refusal::OtherGeometry));
            }
            surfaces.insert(a.index);
            surfaces.insert(b.index);
        }
    }
    let a = geometry(before);
    let b = geometry(after);
    if a.keys().ne(b.keys()) {
        return Ok(Outcome::Undecided(Refusal::OtherGeometry));
    }
    for (slot, record) in a {
        let other = b[&slot];
        charge(ctx, record.tokens.len())?;
        charge(ctx, other.tokens.len())?;
        if surfaces.contains(&slot) {
            continue;
        }
        if record.name != other.name || entity_parameters(record).ne(entity_parameters(other)) {
            return Ok(Outcome::Undecided(Refusal::OtherGeometry));
        }
    }
    Ok(Outcome::Same)
}

fn geometry(records: &[Record]) -> Records<'_> {
    records
        .iter()
        .filter(|r| !r.name.ends_with("attrib"))
        .map(|r| (r.index, r))
        .collect()
}

type Records<'a> = BTreeMap<usize, &'a Record>;
type BodyFaces = BTreeMap<usize, BTreeSet<usize>>;

fn reference(record: &Record, slot: usize) -> Option<usize> {
    usize::try_from(record.ref_at(slot)?).ok()
}

fn body_faces(
    ctx: &DecodeContext<'_>,
    records: &Records<'_>,
) -> Result<Option<BodyFaces>, CodecError> {
    let mut bodies = records
        .values()
        .filter(|r| r.head() == "body")
        .map(|r| (r.index, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for face in records.values().filter(|r| r.head() == "face") {
        let Some(mut owner) = reference(face, 5) else {
            return Ok(None);
        };
        let mut visited = BTreeSet::new();
        loop {
            charge(ctx, 1)?;
            if !visited.insert(owner) {
                return Ok(None);
            }
            let Some(record) = records.get(&owner) else {
                return Ok(None);
            };
            let slot = match record.head() {
                "subshell" => 3,
                "shell" => 7,
                "lump" => 5,
                "body" => {
                    let Some(faces) = bodies.get_mut(&owner) else {
                        return Ok(None);
                    };
                    faces.insert(face.index);
                    break;
                }
                _ => return Ok(None),
            };
            let Some(next) = reference(record, slot) else {
                return Ok(None);
            };
            owner = next;
        }
    }
    Ok(Some(bodies))
}

fn surface<'a>(records: &Records<'a>, face: usize) -> Option<&'a Record> {
    records.get(&reference(records.get(&face)?, 7)?).copied()
}

struct AnalyticParameters {
    kind: std::mem::Discriminant<SolvedSurfaceGeometry>,
    scalars: [f64; 12],
    reversed: bool,
}

fn analytic(record: &Record) -> Option<AnalyticParameters> {
    if !record.tokens.iter().all(|t| token_equal(t, t)) {
        return None;
    }
    let (geometry, reversed) = cadmpeg_asm::brep::geometry::decode_surface(record)?;
    let kind = std::mem::discriminant(&geometry);
    let (origin, frame, dimensions) = match geometry {
        SolvedSurfaceGeometry::Plane(s) => (s.origin(), *s.frame(), [0.0, 0.0, 0.0]),
        SolvedSurfaceGeometry::Cylinder(s) => (
            s.origin(),
            *s.frame(),
            [s.radius().get() / LEN_TO_MM, 0.0, 0.0],
        ),
        SolvedSurfaceGeometry::Cone(s) => (
            s.origin(),
            *s.frame(),
            [
                s.radius().get() / LEN_TO_MM,
                s.ratio().get(),
                s.half_angle().get(),
            ],
        ),
        SolvedSurfaceGeometry::Sphere(s) => (
            s.center(),
            *s.frame(),
            [s.radius().get() / LEN_TO_MM, 0.0, 0.0],
        ),
        SolvedSurfaceGeometry::Torus(s) => (
            s.center(),
            *s.frame(),
            [
                s.major_radius().get() / LEN_TO_MM,
                s.minor_radius().get() / LEN_TO_MM,
                0.0,
            ],
        ),
        _ => return None,
    };
    let point = origin.get();
    let axis = frame.axis().as_raw();
    let reference = frame.reference().as_raw();
    Some(AnalyticParameters {
        kind,
        reversed,
        scalars: [
            point.x / LEN_TO_MM,
            point.y / LEN_TO_MM,
            point.z / LEN_TO_MM,
            axis.x,
            axis.y,
            axis.z,
            reference.x,
            reference.y,
            reference.z,
            dimensions[0],
            dimensions[1],
            dimensions[2],
        ],
    })
}

fn close(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= EPS_TAGGING_SURFACE_PARAMETER_E10
}

fn token_equal(a: &Token, b: &Token) -> bool {
    match (a, b) {
        (Token::Double(a), Token::Double(b)) => close(*a, *b),
        (Token::Float(a), Token::Float(b)) => close(f64::from(*a), f64::from(*b)),
        (Token::Position(a), Token::Position(b)) | (Token::Vector3(a), Token::Vector3(b)) => {
            a.iter().zip(b).all(|(a, b)| close(*a, *b))
        }
        (Token::Vector2(a), Token::Vector2(b)) => a.iter().zip(b).all(|(a, b)| close(*a, *b)),
        _ => a == b,
    }
}

fn entity_parameters(record: &Record) -> impl Iterator<Item = &Token> {
    let skip = usize::from(
        matches!(record.tokens.first(), Some(Token::Ref(_)))
            && matches!(
                record.head(),
                "body"
                    | "lump"
                    | "shell"
                    | "subshell"
                    | "face"
                    | "loop"
                    | "coedge"
                    | "edge"
                    | "tedge"
                    | "vertex"
                    | "tvertex"
                    | "wire"
                    | "point"
                    | "straight"
                    | "ellipse"
                    | "intcurve"
                    | "pcurve"
            ),
    );
    record.tokens.iter().skip(skip)
}

fn parameters_equal(a: &Record, b: &Record) -> bool {
    a.tokens.len() == b.tokens.len()
        && a.tokens
            .iter()
            .skip(3)
            .zip(b.tokens.iter().skip(3))
            .all(|(a, b)| token_equal(a, b))
}

/// Materialize the requested state pairs with revision references normalized
/// to stable slots. The results remain internal to the reader.
pub(crate) fn probe(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    width: RefWidth,
    index: &StateIndex,
    joins: &[FeatureHistoryState],
) -> Result<Vec<Outcome>, CodecError> {
    let tags = joins
        .iter()
        .filter(|r| r.kind == "native:Tagging")
        .collect::<Vec<_>>();
    if tags.is_empty() {
        return Ok(Vec::new());
    }
    let shared = shared_states(joins);
    let compare = |states: &BTreeMap<u32, Vec<Record>>, complete| {
        tags.iter()
            .map(|tag| compare_tag(ctx, tag, &shared, states, complete))
            .collect()
    };
    let wanted = tags
        .iter()
        .filter(|r| r.state_after.is_none_or(|s| !shared.contains(&s.ordinal)))
        .filter_map(|r| {
            r.state_before
                .zip(r.state_after)
                .map(|(before, after)| [before, after.ordinal])
        })
        .flatten()
        .collect::<BTreeSet<_>>();
    if wanted.is_empty() {
        return compare(&BTreeMap::new(), true);
    }
    let ChainFraming::Framed(chain) = frame_history_chain(ctx, bytes, width)? else {
        return compare(&BTreeMap::new(), false);
    };
    let views = chain.views(bytes);
    let Some(frames) = decode_record_archive(ctx, &views, bytes, width)? else {
        return compare(&BTreeMap::new(), false);
    };
    let Ok(active_count) = i64::try_from(frames.active.len()) else {
        return compare(&BTreeMap::new(), false);
    };
    charge(ctx, frames.active.len())?;
    let mut slots = (0..active_count).map(|n| (n, n)).collect::<HashMap<_, _>>();
    let mut consistent = true;
    for state in &views {
        charge(ctx, state.changes.len())?;
        for change in &state.changes {
            let (revision, slot) = match *change {
                EntityChange::Update { old, new } => (old, new),
                EntityChange::Delete { old } => (old, old),
                EntityChange::Insert { .. } => continue,
            };
            if slots.insert(revision, slot).is_some_and(|old| old != slot) {
                consistent = false;
            }
        }
    }
    let mut archived_ids = frames.archived.keys().copied().collect::<Vec<_>>();
    archived_ids.sort_unstable();
    let mut states = BTreeMap::new();
    let mut failure = None;
    let mut visit = index.states.len();
    let closed = walk_entity_versions(&views, active_count, &archived_ids, |_, versions| {
        let Some(next) = visit.checked_sub(1) else {
            consistent = false;
            return;
        };
        visit = next;
        let Ok(ordinal) = u32::try_from(visit) else {
            consistent = false;
            return;
        };
        if failure.is_some() {
            return;
        }
        if let Err(error) = charge(ctx, versions.len()) {
            failure = Some(error);
            return;
        }
        if !wanted.contains(&ordinal) {
            return;
        }
        let materialized = materialize(ctx, versions, &slots, |revision| {
            frames.archived.get(&revision).or_else(|| {
                usize::try_from(revision)
                    .ok()
                    .and_then(|n| frames.active.get(n))
            })
        });
        match materialized {
            Ok(Some(records)) => {
                states.insert(ordinal, records);
            }
            Ok(None) => {}
            Err(error) => failure = Some(error),
        }
    });
    if let Some(error) = failure {
        return Err(error);
    }
    compare(&states, closed && consistent && visit == 0)
}

fn shared_states(joins: &[FeatureHistoryState]) -> BTreeSet<u32> {
    let mut seen = BTreeSet::new();
    joins
        .iter()
        .filter_map(|join| join.state_after.map(|s| s.ordinal))
        .filter(|ordinal| !seen.insert(*ordinal))
        .collect()
}

fn compare_tag(
    ctx: &DecodeContext<'_>,
    tag: &FeatureHistoryState,
    shared: &BTreeSet<u32>,
    states: &BTreeMap<u32, Vec<Record>>,
    complete: bool,
) -> Result<Outcome, CodecError> {
    if tag.state_after.is_some_and(|s| shared.contains(&s.ordinal)) {
        return Ok(Outcome::Undecided(Refusal::SharedState));
    }
    compare_pair(
        ctx,
        tag.state_before,
        tag.state_after.map(|s| s.ordinal),
        states,
        complete,
    )
}

fn compare_pair(
    ctx: &DecodeContext<'_>,
    before: Option<u32>,
    after: Option<u32>,
    states: &BTreeMap<u32, Vec<Record>>,
    complete: bool,
) -> Result<Outcome, CodecError> {
    match (before, after) {
        (_, None) => Ok(Outcome::Undecided(Refusal::NoStateJoin)),
        (None, _) => Ok(Outcome::Undecided(Refusal::NoPredecessor)),
        (Some(before), Some(after)) => match (states.get(&before), states.get(&after)) {
            (Some(a), Some(b)) if complete => tagging_identity(ctx, a, b),
            _ => Ok(Outcome::Undecided(Refusal::StateNotMaterialized)),
        },
    }
}

fn materialize<'a>(
    ctx: &DecodeContext<'_>,
    versions: &BTreeMap<i64, i64>,
    slots: &HashMap<i64, i64>,
    lookup: impl Fn(i64) -> Option<&'a Record>,
) -> Result<Option<Vec<Record>>, CodecError> {
    charge(ctx, versions.len())?;
    let mut result = Vec::new();
    for (&slot, &revision) in versions {
        if slots.get(&revision) != Some(&slot) {
            return Ok(None);
        }
        let Some(record) = lookup(revision) else {
            return Ok(None);
        };
        charge(ctx, record.tokens.len())?;
        for token in record.tokens.iter() {
            if let Token::Str(value) | Token::Ident(value) | Token::SubIdent(value) = token {
                charge(ctx, value.len())?;
            }
        }
        let mut tokens = ctx.alloc_filled(
            record.tokens.len(),
            Token::Ref(-1),
            "materialize Tagging state",
        )?;
        for (output, input) in tokens.iter_mut().zip(record.tokens.iter()) {
            *output = input.clone();
            if let Token::Ref(reference) = output {
                if *reference >= 0 {
                    let Some(&normalized) = slots
                        .get(reference)
                        .filter(|slot| versions.contains_key(slot))
                    else {
                        return Ok(None);
                    };
                    *reference = normalized;
                }
            }
        }
        let Ok(index) = usize::try_from(slot) else {
            return Ok(None);
        };
        result.push(Record {
            index,
            tokens: Arc::from(tokens),
            ..record.clone()
        });
    }
    Ok(Some(result))
}

#[cfg(test)]
mod tests;
