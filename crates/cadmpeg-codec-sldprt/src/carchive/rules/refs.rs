//! Face, edge, loop and vertex references.

use super::super::reader::{bad, EdgeReference, Reader, Res, V};
use crate::layout::identity_reference_header as header;

const MARKER: [u8; 16] = [
    0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54,
];

/// `moFaceRef_c`, `moEdgeRef_c`, `moLoopRef_c`, `moSilhEdgeRef_c`,
/// `moSilhVertexRef_c` (`ref`).
pub(crate) fn ref_(r: &mut Reader<'_>) -> Res<V> {
    let edge = r.last_class().as_deref() == Some("moEdgeRef_c")
        && r.open
            .iter()
            .rev()
            .skip(1)
            .any(|(class, _)| &**class == "moCompEdge_c");
    let owner = edge.then(|| r.holder_start()).flatten();
    let marker = r.p + header::MARKER;
    let (value, faces) = reference_body(r, edge)?;
    if edge {
        r.edge_references.push(EdgeReference {
            marker,
            owner,
            faces,
        });
    }
    Ok(value)
}

pub(crate) fn surface_hops(
    reader: &mut Reader<'_>,
) -> Res<Option<[super::idrep::SurfaceIdentity; 4]>> {
    reference_body(reader, true).map(|(_, faces)| faces)
}

fn reference_body(
    r: &mut Reader<'_>,
    edge: bool,
) -> Res<(V, Option<[super::idrep::SurfaceIdentity; 4]>)> {
    let face = r.last_class().as_deref() == Some("moFaceRef_c");
    r.st.face = face;
    let h = r.p;
    let kind2 = r.eq(h, &[2, 0, 0, 0]);
    r.skip(if kind2 { 36 } else { 38 });
    if !r.eq(h + 20, &MARKER) {
        return Err(bad("no marker"));
    }
    let cnt = r.pu32(h + 8)?;
    if cnt > 4096 {
        return Err(bad("count"));
    }
    if kind2 {
        r.ptr(Some(&["moVertexRef_c"]))?;
    }
    let mut surfaces = [None; 4];
    for index in 0..cnt {
        let trace_start = r.trace.len();
        let pointer = r.ptr(None)?;
        if edge && cnt == 4 && !kind2 {
            let index = usize::try_from(index).map_err(|_| bad("edge hop index"))?;
            let Some(slot) = surfaces.get_mut(index) else {
                return Err(bad("edge hop index"));
            };
            *slot = r
                .trace
                .get(trace_start)
                .filter(|trace| r.eq(trace.body, &[0, 0]))
                .and_then(|_| super::idrep::surface_identity(&pointer));
        }
    }
    if !r.zeros(r.p, 8) {
        return Err(bad("trailer zeros"));
    }
    r.skip(8);
    let nv = if r.p + 8 <= r.len() {
        Some(r.pu32(r.p)?)
    } else {
        None
    };
    if nv.is_some_and(|nv| (9000..=40000).contains(&nv) && nv % 100 == 0) && r.zeros(r.p + 4, 4) {
        r.skip(8);
        if r.st.face && r.zeros(r.p, 4) {
            r.skip(4);
        }
    } else if r.st.face && super::feature::ver_or(r, 99999) < 13000 && r.zeros(r.p, 4) {
        r.skip(4);
    }
    let faces = match surfaces {
        [Some(first), Some(second), Some(start), Some(end)] => Some([first, second, start, end]),
        _ => None,
    };
    Ok((V::map([("cnt", V::from(cnt))]), faces))
}

/// `moVertexRef_c`.
pub(crate) fn vertexref(r: &mut Reader<'_>) -> Res<V> {
    let h = r.p;
    r.skip(38);
    let cnt = r.pu32(h + 8)?;
    if !r.ffs(h, 8) || cnt > 4096 {
        return Err(bad("vertex ref header"));
    }
    for _ in 0..cnt {
        r.ptr(None)?;
    }
    if r.eq(r.p, &[2, 0, 0, 0]) && r.zeros(r.p + 4, 4) {
        r.skip(8);
    } else {
        r.expect_zeros(8, "vertex ref 8 zero")?;
    }
    let nv = r.pu32(r.p)?;
    if (9000..=40000).contains(&nv) && nv % 100 == 0 && r.zeros(r.p + 4, 4) {
        r.skip(8);
    }
    let k = r.u32()?;
    for _ in 0..k {
        r.f64()?;
        r.f64()?;
        r.f64()?;
    }
    Ok(V::from(cnt))
}

#[cfg(test)]
mod tests {
    use super::{header, ref_};
    use crate::carchive::reader::{Reader, Trace};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn four_face_edge() -> Vec<u8> {
        let mut bytes = vec![0; header::LEN];
        bytes[header::FORM..header::FORM + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes[header::HOP_COUNT..header::HOP_COUNT + 4].copy_from_slice(&4u32.to_le_bytes());
        bytes[header::MARKER..header::MARKER + 16].copy_from_slice(&header::MARKER_VALUE);
        bytes[header::HOP_COUNT + std::mem::size_of::<u32>() + 1] = 2;
        for (tag, fields) in [
            (5u16, vec![7u32]),
            (5, vec![9]),
            (4, vec![1, 3]),
            (4, vec![0, 3]),
        ] {
            bytes.extend((0x8000 | tag).to_le_bytes());
            bytes.extend(0u16.to_le_bytes());
            bytes.extend(0x8006u16.to_le_bytes());
            bytes.extend(3u16.to_le_bytes());
            bytes.extend(17u32.to_le_bytes());
            bytes.extend(29u32.to_le_bytes());
            for field in fields {
                bytes.extend(field.to_le_bytes());
            }
        }
        bytes.extend([0; 8]);
        bytes.extend(32000u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes
    }

    fn reader<'a>(
        ctx: &'a cadmpeg_core::decode::DecodeContext<'a>,
        bytes: &'a [u8],
        component: bool,
    ) -> Reader<'a> {
        let known = HashMap::from([
            (4, Arc::from("moEndFaceSurfIdRep_c")),
            (5, Arc::from("moFromSktEntSurfIdRep_c")),
            (6, Arc::from("moFR_c")),
        ]);
        let mut reader = Reader::new(ctx, bytes, 0, known, 10, Some(32000));
        reader.open.push((Arc::from("Fillet_c"), 0));
        reader.names.insert(0, "Blend".into());
        if component {
            reader.open.push((Arc::from("moCompEdge_c"), 1));
        }
        reader.open.push((Arc::from("moEdgeRef_c"), 2));
        reader.trace.push(Trace {
            class: Arc::from("moEdgeRef_c"),
            at: 2,
            body: 0,
        });
        reader
    }

    #[test]
    fn bounded_roster_reference_uses_the_same_complete_surface_hops() {
        let bytes = four_face_edge();
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let reader = reader(&ctx, &bytes, false);
        let rules = crate::carchive::ReferenceRules {
            known: reader.known,
            counter: reader.counter,
            version: Some(32000),
        };
        let faces = rules
            .surface_hops(&ctx, &bytes, header::MARKER)
            .expect("bounded read")
            .expect("four surfaces");
        assert_eq!(faces[0].values(), &[74, 17, 29, 0, 7]);
        assert_eq!(faces[3].values(), &[40, 17, 29, 0, 0, 3]);
        assert!(rules
            .surface_hops(&ctx, &bytes[..bytes.len() - 18], header::MARKER)
            .expect("permissive truncated reference")
            .is_none());
        assert!(rules
            .surface_hops(&ctx, &bytes, usize::MAX)
            .expect("permissive invalid location")
            .is_none());
    }

    #[test]
    fn component_edge_parent_assigns_the_operand_role() {
        let bytes = four_face_edge();
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let mut input = reader(&ctx, &bytes, true);
        let snapshot = input.snapshot();
        ref_(&mut input).expect("edge reference");
        assert_eq!(input.edge_references.len(), 1);
        let edge = &input.edge_references[0];
        assert_eq!(edge.marker, header::MARKER);
        assert_eq!(edge.owner, Some(0));
        let faces = edge.faces.expect("four complete surface hops");
        assert_eq!(faces[0].values(), &[74, 17, 29, 0, 7]);
        assert_eq!(faces[2].values(), &[40, 17, 29, 0, 1, 3]);
        input.restore(snapshot);
        assert!(input.edge_references.is_empty());
        let mut cached = reader(&ctx, &bytes, false);
        ref_(&mut cached).expect("cached identity");
        assert!(cached.edge_references.is_empty());
    }

    #[test]
    fn unknown_hop_prefix_retains_the_operand_without_geometry() {
        let mut bytes = four_face_edge();
        bytes[header::LEN + 2] = 1;
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let mut input = reader(&ctx, &bytes, true);
        ref_(&mut input).expect("retained reference");
        assert_eq!(input.edge_references.len(), 1);
        assert!(input.edge_references[0].faces.is_none());
    }

    #[test]
    fn truncated_edge_reference_emits_no_geometry_operand() {
        let mut bytes = four_face_edge();
        bytes.truncate(bytes.len() - 18);
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let mut input = reader(&ctx, &bytes, true);
        assert!(ref_(&mut input).is_err());
        assert!(input.edge_references.is_empty());
    }
}
