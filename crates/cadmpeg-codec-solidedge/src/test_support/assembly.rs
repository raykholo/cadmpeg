// SPDX-License-Identifier: Apache-2.0
//! Synthetic assembly documents (`docs/formats/solidedge.md` §9).

use super::{cluster_and_map, document, put_reals, put_words, type_table, Object};

/// The class names of an assembly document, by form.
const ASSEMBLY_CLASSES: [&str; 10] = [
    "References Collection Object",
    "CLSID_JAsmReference",
    "CLSID_JSite",
    "Constraints Collection Object",
    "CLSID_JAsmGroundConstraint",
    "CLSID_JAsmPlanarConstraint",
    "CLSID_JAsmAxialConstraint",
    "CLSID_JAsmTangentConstraint",
    "CLSID_JAsmAngularConstraint",
    "CLSID_JMembassy",
];
const COLLECTION_FORM: u16 = 0;
const REFERENCE_FORM: u16 = 1;
const SITE_FORM: u16 = 2;
const RELATIONS_FORM: u16 = 3;
const GROUND_FORM: u16 = 4;
const PLANAR_FORM: u16 = 5;
const AXIAL_FORM: u16 = 6;
const TANGENT_FORM: u16 = 7;
const ANGULAR_FORM: u16 = 8;
const PICK_FORM: u16 = 9;
/// The object identifier of the relation list.
pub(crate) const RELATIONS: u32 = 2;
/// The object identifier of the references collection.
pub(crate) const COLLECTION: u32 = 1;
/// Bytes of an occurrence object, the frame included.
const OCCURRENCE_LEN: usize = 229;
/// The file moniker CLSID in stored byte order.
const MONIKER_CLSID: [u8; 16] = [0x03, 0x03, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];

/// The row-major identity rotation.
const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// One occurrence object.
pub(crate) struct OccurrenceSpec {
    pub(crate) id: u32,
    pub(crate) state: u32,
    pub(crate) site: u32,
    pub(crate) rotation: [f64; 9],
    /// The translation in metres.
    pub(crate) translation: [f64; 3],
    pub(crate) unit: f64,
    /// The object length to keep, the frame included, or `None` for all of it.
    pub(crate) keep: Option<usize>,
}

impl OccurrenceSpec {
    /// An occurrence of `site` at identity.
    pub(crate) fn at_identity(id: u32, site: u32) -> Self {
        Self {
            id,
            state: 2,
            site,
            rotation: IDENTITY,
            translation: [0.0; 3],
            unit: 1.0,
            keep: None,
        }
    }
}

/// One site object and its link stream.
pub(crate) struct SiteSpec<'a> {
    pub(crate) id: u32,
    /// The absolute and relative paths of the two monikers, or `None` for a
    /// site with no link stream.
    pub(crate) links: Option<[&'a str; 2]>,
}

/// One file moniker: the CLSID, the up-level count, the counted path and the
/// terminating words.
fn moniker(up_level: u16, path: &str) -> Vec<u8> {
    let mut bytes = MONIKER_CLSID.to_vec();
    bytes.extend_from_slice(&up_level.to_le_bytes());
    let length = u32::try_from(path.len() + 1).unwrap();
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(path.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0xFFFF_u16.to_le_bytes());
    bytes.extend_from_slice(&0xDEAD_u16.to_le_bytes());
    bytes
}

/// The link stream of a site: a 12-byte header, the absolute moniker, other
/// bytes, then the relative moniker.
fn link_stream(links: [&str; 2]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 12];
    bytes.extend_from_slice(&moniker(0, links[0]));
    bytes.extend_from_slice(&[0x11; 6]);
    bytes.extend_from_slice(&moniker(1, links[1]));
    bytes
}

fn occurrence_body(spec: &OccurrenceSpec) -> Vec<u8> {
    let mut body = vec![0_u8; OCCURRENCE_LEN - 8];
    put_words(&mut body, 16, &[spec.state]);
    put_words(&mut body, 113, &[spec.site]);
    put_reals(&mut body, 125, &spec.rotation);
    put_reals(&mut body, 197, &spec.translation);
    put_reals(&mut body, 221, &[spec.unit]);
    if let Some(keep) = spec.keep {
        body.truncate(keep.checked_sub(8).unwrap());
    }
    body
}

/// A document whose references collection lists `listed` and that holds the
/// occurrence objects `occurrences` and the sites `sites`.
pub(crate) fn assembly_document(
    listed: &[u32],
    occurrences: &[OccurrenceSpec],
    sites: &[SiteSpec<'_>],
) -> Vec<u8> {
    assembly_document_with_structure(listed, occurrences, sites, None)
}

/// The stream of an occurrence structure list: a 20-byte header with the entry
/// count at offset 16, then 20-byte entries of `(flags, object)`.
pub(crate) fn structure_stream(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 16];
    bytes.extend_from_slice(&u32::try_from(entries.len()).unwrap().to_le_bytes());
    for (flags, object) in entries {
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&object.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
    }
    bytes
}

/// As [`assembly_document`], with the `Preview/Structure/IOT` stream `structure`
/// where it is `Some`.
pub(crate) fn assembly_document_with_structure(
    listed: &[u32],
    occurrences: &[OccurrenceSpec],
    sites: &[SiteSpec<'_>],
    structure: Option<Vec<u8>>,
) -> Vec<u8> {
    assembly_document_full(listed, occurrences, sites, structure, Vec::new())
}

/// One relation object of an assembly.
pub(crate) enum RelationSpec {
    Ground {
        id: u32,
        reference: u32,
        rotation: [f64; 9],
        /// In metres.
        translation: [f64; 3],
    },
    Planar {
        id: u32,
        operands: [u32; 2],
        picks: [u32; 2],
        /// In metres.
        offset: f64,
        flags: u32,
        dimension: u32,
    },
    Axial {
        id: u32,
        operands: [u32; 2],
        picks: [u32; 2],
        flags: u32,
    },
    Tangent {
        id: u32,
        operands: [u32; 2],
        picks: [u32; 2],
        flags: u32,
        dimension: u32,
    },
    Angular {
        id: u32,
        operands: [u32; 2],
        picks: [u32; 2],
        /// In radians.
        angle: f64,
        flags: u32,
        dimension: u32,
    },
}

impl RelationSpec {
    /// The object identifier.
    pub(crate) fn id(&self) -> u32 {
        match self {
            Self::Ground { id, .. }
            | Self::Planar { id, .. }
            | Self::Axial { id, .. }
            | Self::Tangent { id, .. }
            | Self::Angular { id, .. } => *id,
        }
    }

    fn pair_body(len: usize, operands: [u32; 2], picks: [u32; 2]) -> Vec<u8> {
        let mut body = vec![0_u8; len - 8];
        put_words(
            &mut body,
            8,
            &[operands[0], operands[1], picks[0], picks[1]],
        );
        body
    }

    fn object(&self) -> Object {
        let (form, body) = match self {
            Self::Ground {
                reference,
                rotation,
                translation,
                ..
            } => {
                let mut body = vec![0_u8; 127 - 8];
                put_words(&mut body, 8, &[*reference]);
                put_reals(&mut body, 12, rotation);
                put_reals(&mut body, 84, translation);
                put_reals(&mut body, 108, &[1.0]);
                (GROUND_FORM, body)
            }
            Self::Planar {
                operands,
                picks,
                offset,
                flags,
                dimension,
                ..
            } => {
                let mut body = Self::pair_body(52, *operands, *picks);
                put_reals(&mut body, 32, &[*offset]);
                put_words(&mut body, 40, &[1, *flags, *dimension]);
                (PLANAR_FORM, body)
            }
            Self::Axial {
                operands,
                picks,
                flags,
                ..
            } => {
                let mut body = Self::pair_body(64, *operands, *picks);
                put_words(&mut body, 32, &[1, *flags]);
                put_words(&mut body, 52, &[1]);
                (AXIAL_FORM, body)
            }
            Self::Tangent {
                operands,
                picks,
                flags,
                dimension,
                ..
            } => {
                let mut body = Self::pair_body(60, *operands, *picks);
                put_words(&mut body, 32, &[1, *flags, *dimension]);
                put_words(&mut body, 52, &[4]);
                (TANGENT_FORM, body)
            }
            Self::Angular {
                operands,
                picks,
                angle,
                flags,
                dimension,
                ..
            } => {
                let mut body = vec![0_u8; 54 - 8];
                for (group, at) in [0_usize, 13].into_iter().enumerate() {
                    body[at..at + 4].copy_from_slice(&operands[group].to_le_bytes());
                    body[at + 4..at + 8].copy_from_slice(&picks[group].to_le_bytes());
                    body[at + 8] = 2;
                    body[at + 9..at + 13].copy_from_slice(&operands[group].to_le_bytes());
                }
                put_reals(&mut body, 34, &[*angle]);
                put_words(&mut body, 42, &[1, *flags, *dimension]);
                (ANGULAR_FORM, body)
            }
        };
        Object {
            id: self.id(),
            form,
            flags: 0,
            body,
        }
    }
}

/// A pick object with the identifier `id`.
fn pick_object(id: u32) -> Object {
    Object {
        id,
        form: PICK_FORM,
        flags: 0,
        body: vec![0; 64],
    }
}

/// The relation list object: `n` at offset 16, `ceil(n / 8)` bitmap bytes,
/// then an entry of an identifier and a byte for each listed relation.
fn relation_list(listed: &[u32]) -> Object {
    let mut body = vec![0_u8; 12];
    put_words(&mut body, 16, &[u32::try_from(listed.len()).unwrap()]);
    body.resize(12 + listed.len().div_ceil(8), 0);
    for id in listed {
        body.extend_from_slice(&id.to_le_bytes());
        body.push(0);
    }
    Object {
        id: RELATIONS,
        form: RELATIONS_FORM,
        flags: 0,
        body,
    }
}

/// As [`assembly_document`], with a relation list of `listed` and the objects
/// of `relations` and of the pick objects `picks`.
pub(crate) fn assembly_document_with_relations(
    occurrences: &[OccurrenceSpec],
    sites: &[SiteSpec<'_>],
    listed: &[u32],
    relations: &[RelationSpec],
    picks: &[u32],
) -> Vec<u8> {
    let mut extra = vec![relation_list(listed)];
    extra.extend(relations.iter().map(RelationSpec::object));
    extra.extend(picks.iter().map(|id| pick_object(*id)));
    let ids: Vec<u32> = occurrences.iter().map(|spec| spec.id).collect();
    assembly_document_full(&ids, occurrences, sites, None, extra)
}

fn assembly_document_full(
    listed: &[u32],
    occurrences: &[OccurrenceSpec],
    sites: &[SiteSpec<'_>],
    structure: Option<Vec<u8>>,
    extra: Vec<Object>,
) -> Vec<u8> {
    let mut collection = vec![0_u8; 12];
    put_words(&mut collection, 16, &[u32::try_from(listed.len()).unwrap()]);
    collection.resize(12 + listed.len().div_ceil(8), 0);
    for id in listed {
        collection.extend_from_slice(&id.to_le_bytes());
    }
    let mut objects = vec![Object {
        id: COLLECTION,
        form: COLLECTION_FORM,
        flags: 0,
        body: collection,
    }];
    for occurrence in occurrences {
        objects.push(Object {
            id: occurrence.id,
            form: REFERENCE_FORM,
            flags: 0,
            body: occurrence_body(occurrence),
        });
    }
    objects.extend(extra);
    let mut streams = Vec::new();
    for site in sites {
        objects.push(Object {
            id: site.id,
            form: SITE_FORM,
            flags: 0,
            body: vec![0; 16],
        });
        if let Some(links) = site.links {
            streams.push((format!("JSite{}/\u{1}Ole", site.id), link_stream(links)));
        }
    }
    objects.sort_by_key(|object| object.id);
    let (cluster, map) = cluster_and_map(&objects);
    let mut files = vec![
        ("PSMcluster0".to_owned(), cluster),
        ("PSMspacemap/0x00000000".to_owned(), map),
        (
            "PSMtypetable".to_owned(),
            type_table(&ASSEMBLY_CLASSES, &[]),
        ),
    ];
    files.extend(streams);
    if let Some(structure) = structure {
        files.push(("Preview/Structure/IOT".to_owned(), structure));
    }
    let borrowed: Vec<(&str, Vec<u8>)> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.clone()))
        .collect();
    document(&borrowed)
}
