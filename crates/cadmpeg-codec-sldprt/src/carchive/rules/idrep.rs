//! The persistent-identity family: `moFR_c`, the `*IdRep_c` classes,
//! `moExtObject_c`, `moCStringHandle_c`.

use super::super::reader::{bad, Ptr, Reader, Res, V};

/// `moCStringHandle_c`.
pub(crate) fn cstringhandle(r: &mut Reader<'_>) -> Res<V> {
    Ok(V::Str(r.cstring()?))
}

/// `moExtObject_c`.
pub(crate) fn extobject(reader: &mut Reader<'_>) -> Res<V> {
    let path = reader.ptr(None)?;
    let name = reader.ptr(None)?;
    if reader.u16()? != 2 {
        return Err(bad("ExtObject version"));
    }
    reader.u8()?;
    reader.u32()?;
    reader.cstring()?;
    reader.cstring()?;
    reader.cstring()?;
    let at = reader.p;
    let Some(stop) = reader.find(&[0xff, 0xfe, 0xff], at, at + 64) else {
        return Err(bad("ExtObject: no configuration string"));
    };
    let x_value = stop - at;
    reader.p = stop;
    let value = super::feature::ver_or(reader, 0);
    let cfg = reader.cstring()?;
    reader.skip(if value >= 17000 { 16 } else { 12 });
    Ok(V::map([
        ("path", V::from(path)),
        ("name", V::from(name)),
        (
            "X",
            V::Int(i64::try_from(x_value).map_err(|_| bad("offset exceeds i64"))?),
        ),
        ("cfg", V::from(cfg)),
    ]))
}

/// `moFR_c`.
pub(crate) fn fr(r: &mut Reader<'_>) -> Res<V> {
    let prod = r.ptr(None)?;
    let fid = r.u32()?;
    let ts = r.u32()?;
    Ok(V::map([
        ("prod", V::from(prod)),
        ("fid", V::from(fid)),
        ("ts", V::from(ts)),
    ]))
}

/// `OWN`: the id-rep classes and the own bytes after their `moFR_c`.
pub(crate) fn own(c: &str) -> Option<usize> {
    Some(match c {
        "moSimpleSurfIdRep_c"
        | "moFromSktEntSurfIdRep_c"
        | "moFilletSurfIdRep_c"
        | "moThinOffsetSurfIdRep_c"
        | "moWireEdgeIdRep_c"
        | "moSimpleEdgeIdRep_c" => 4,
        "moFromSktEnt3IntSurfIdRep_c" | "moEndFace3IntSurfIdRep_c" => 12,
        "moEndFaceSurfIdRep_c"
        | "moThinClosingSurfIdRep_c"
        | "moUnBendSurfIdRep_c"
        | "moBendEndSurfIdRep_c"
        | "moSegmLeftTrimEdgeIdRep_c"
        | "moSegmRightTrimEdgeIdRep_c"
        | "moSweepSideSurfIdRep_c"
        | "moWzdHoleSurfIdRep_c" => 8,
        "moLPatternSurfIdRep_c"
        | "moIndirectSurfIdRep_c"
        | "moSurfaceIdRep_c"
        | "moDerivedSurfIdRep_c"
        | "moCirPatternSurfIdRep_c"
        | "moCirPatternSurfIdRep2_c"
        | "moMirPatternSurfIdRep_c" => 0,
        _ => return None,
    })
}

/// `WRAP`: id-rep classes that wrap a child, and the u32 count after it.
fn wrap(c: &str) -> Option<usize> {
    Some(match c {
        "moDerivedSurfIdRep_c" => 1,
        "moLPatternSurfIdRep_c" | "moCirPatternSurfIdRep_c" | "moCirPatternSurfIdRep2_c" => 2,
        "moMirPatternSurfIdRep_c" | "moIndirectSurfIdRep_c" => 0,
        _ => return None,
    })
}

/// The rule `make(c)` builds for each id-rep class.
pub(crate) fn rep(r: &mut Reader<'_>) -> Res<V> {
    let c = r
        .last_class()
        .ok_or_else(|| bad("id rep without a class"))?;
    r.u16()?;
    let f = r.obj(Some(&["moFR_c"]))?;
    if let Some(k) = wrap(&c) {
        let kid = r.ptr(None)?;
        let mut tail = Vec::new();
        for _ in 0..k {
            tail.push(V::from(r.u32()?));
        }
        return Ok(V::map([
            ("fr", V::from(f)),
            ("kid", V::from(kid)),
            ("tail", V::List(tail)),
        ]));
    }
    let n = own(&c).unwrap_or(0);
    let own_bytes = r.slice(r.p, n).to_vec();
    r.skip(n);
    Ok(V::map([("fr", V::from(f)), ("own", V::Bytes(own_bytes))]))
}

pub(crate) const FILLET_SURFACE_CLASS: u32 = 72;

/// One complete single-hop `FACE_ID_2001` identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SurfaceIdentity {
    words: [u32; 7],
    len: usize,
    document: u32,
}

impl SurfaceIdentity {
    pub(crate) fn values(&self) -> &[u32] {
        &self.words[..self.len]
    }

    pub(crate) fn same_document(&self, other: &Self) -> bool {
        self.document == other.document
    }
}

pub(crate) fn surface_identity(pointer: &super::super::reader::Ptr) -> Option<SurfaceIdentity> {
    let (code, fields) = match pointer.obj_class()? {
        "moFromSktEntSurfIdRep_c" => (74, 1),
        "moFromSktEnt3IntSurfIdRep_c" => (83, 3),
        "moEndFaceSurfIdRep_c" => (40, 2),
        "moEndFace3IntSurfIdRep_c" => (81, 3),
        "moFilletSurfIdRep_c" => (FILLET_SURFACE_CLASS, 1),
        "moSurfaceIdRep_c" => (36, 0),
        "moWzdHoleSurfIdRep_c" => (49, 2),
        "moSweepSideSurfIdRep_c" => (46, 2),
        "moSimpleSurfIdRep_c" => (77, 1),
        "moThinOffsetSurfIdRep_c" => (78, 1),
        _ => return None,
    };
    let value = pointer.value()?;
    let V::Ptr(feature) = value.get("fr") else {
        return None;
    };
    if feature.obj_class()? != "moFR_c" {
        return None;
    }
    let feature = feature.value()?;
    let V::Ptr(document) = feature.get("prod") else {
        return None;
    };
    let document = match document.as_ref() {
        Ptr::Null => 0,
        Ptr::Ref { index, .. } => *index,
        Ptr::Obj { .. } => return None,
    };
    let V::Bytes(own) = value.get("own") else {
        return None;
    };
    if own.len() != fields * 4 {
        return None;
    }
    let mut identity = SurfaceIdentity {
        words: [0; 7],
        len: 4 + fields,
        document,
    };
    identity.words[0] = code;
    identity.words[1] = u32::try_from(feature.get("fid").as_int()?).ok()?;
    identity.words[2] = u32::try_from(feature.get("ts").as_int()?).ok()?;
    let mut view = cadmpeg_core::decode::View::over_retained(own);
    for word in &mut identity.words[4..identity.len] {
        *word = view.u32_le()?;
    }
    Some(identity)
}

#[cfg(test)]
mod tests {
    use super::surface_identity;
    use crate::carchive::reader::{Ptr, V};
    use std::sync::Arc;

    fn identity(class: &str, fields: &[u32]) -> Ptr {
        let fr = Ptr::Obj {
            class: Arc::from("moFR_c"),
            value: V::map([
                (
                    "prod",
                    V::from(Ptr::Ref {
                        index: 3,
                        class: Some(Arc::from("moExtObject_c")),
                    }),
                ),
                ("fid", V::from(17u32)),
                ("ts", V::from(29u32)),
            ]),
        };
        Ptr::Obj {
            class: Arc::from(class),
            value: V::map([
                ("fr", V::from(fr)),
                (
                    "own",
                    V::Bytes(
                        fields
                            .iter()
                            .flat_map(|field| field.to_le_bytes())
                            .collect(),
                    ),
                ),
            ]),
        }
    }

    #[test]
    fn surface_hops_keep_class_timestamp_and_cap_region() {
        let end = surface_identity(&identity("moEndFaceSurfIdRep_c", &[1, 3])).expect("cap");
        let side = surface_identity(&identity("moFromSktEntSurfIdRep_c", &[1])).expect("side");
        assert_eq!(end.values(), &[40, 17, 29, 0, 1, 3]);
        assert_eq!(side.values(), &[74, 17, 29, 0, 1]);
        assert_ne!(end, side);
    }

    #[test]
    fn incomplete_fields_and_wrapping_hops_keep_native_identity() {
        assert!(surface_identity(&identity("moEndFaceSurfIdRep_c", &[1])).is_none());
        assert!(surface_identity(&identity("moFromSktEntSurfIdRep_c", &[1, 2])).is_none());
        assert!(surface_identity(&identity("moMirPatternSurfIdRep_c", &[])).is_none());
    }
}
