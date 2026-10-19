// SPDX-License-Identifier: Apache-2.0
//! Typed compact parameters and typed-reference owner frames.

use std::io::{Cursor, Write};

use zip::CompressionMethod;

use super::super::{decode_parameters, parse_design_parameter_record, parse_parameter_owner};
use crate::records::parameters::DesignParameterSource;
use crate::test_support::lp_utf16;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::zip_test::with_scan;

const OWNER_TYPE: &str = "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0";
const SCOPE_TYPE: &str = "11111111-2222-3333-4444-555555555555";
const PARAMETER_TYPE: &str = "66666666-7777-8888-9999-AAAAAAAAAAAA";
const COMPANION_TYPE: &str = "BBBBBBBB-CCCC-DDDD-EEEE-FFFFFFFFFFFF";
const TAIL_TYPE: &str = "B0556F58-EE09-46A7-B18F-E296F48E5B13";
const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

fn typed_reference(out: &mut Vec<u8>, target: u64, guid: &str) {
    out.push(1);
    out.extend_from_slice(&target.to_le_bytes());
    out.extend_from_slice(&36u32.to_le_bytes());
    out.extend_from_slice(guid.as_bytes());
    out.extend_from_slice(&[0, 0]);
}

/// Unit spelling of a synthetic typed parameter.
#[derive(Clone, Copy)]
enum Unit<'a> {
    Token(&'a str),
    ExplicitEmpty,
    Absent,
}

struct TypedParameter<'a> {
    record_index: u32,
    owner: Option<u32>,
    expression: &'a str,
    source_kind: &'a str,
    unit: Unit<'a>,
    name: &'a str,
    value: f64,
}

fn typed_parameter_record(parameter: &TypedParameter<'_>, tail_type: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"331");
    out.extend_from_slice(&parameter.record_index.to_le_bytes());
    out.extend_from_slice(&[0; 15]);
    out.extend_from_slice(&8u32.to_le_bytes());
    let trailer: [u8; 5] = match parameter.owner {
        Some(owner) => {
            typed_reference(&mut out, u64::from(owner), OWNER_TYPE);
            [0; 5]
        }
        None => {
            out.push(0);
            [0, 0, 0, 0, 1]
        }
    };
    lp_utf16(&mut out, parameter.expression);
    out.extend_from_slice(&trailer);
    lp_utf16(&mut out, parameter.source_kind);
    match parameter.unit {
        Unit::Token(unit) => lp_utf16(&mut out, unit),
        Unit::ExplicitEmpty => out.extend_from_slice(&0u32.to_le_bytes()),
        Unit::Absent => {}
    }
    lp_utf16(&mut out, parameter.name);
    out.extend_from_slice(&parameter.value.to_le_bytes());
    out.push(0);
    typed_reference(&mut out, 167, tail_type);
    out
}

fn owned(record_index: u32) -> TypedParameter<'static> {
    TypedParameter {
        record_index,
        owner: Some(record_index - 1),
        expression: "3 mm",
        source_kind: "AlongDistance",
        unit: Unit::Token("mm"),
        name: "d1",
        value: 0.3,
    }
}

#[test]
fn typed_owned_parameter_reads_the_inline_typed_owner_reference() {
    let bytes = typed_parameter_record(&owned(537), TAIL_TYPE);
    let parameter = parse_design_parameter_record(&bytes).expect("typed owned parameter");
    assert_eq!(parameter.class_tag.as_str(), "331");
    assert_eq!(parameter.record_index, 537);
    assert_eq!(parameter.source_ordinal, 8);
    assert_eq!(parameter.owner_record_index(), Some(536));
    assert_eq!(parameter.family_discriminator(), None);
    assert_eq!(parameter.expression(), "3 mm");
    assert_eq!(parameter.expression_offset(), 85);
    assert_eq!(parameter.source_kind(), "AlongDistance");
    assert_eq!(
        parameter.unit().map(|field| field.value.as_str()),
        Some("mm")
    );
    assert_eq!(parameter.name(), "d1");
    assert_eq!(parameter.evaluated_value().get(), 0.3);
    assert_eq!(
        parameter.evaluated_value_offset(),
        u64::try_from(bytes.len() - 60).unwrap()
    );
}

#[test]
fn typed_document_parameter_stores_a_null_owner_and_marked_trailer() {
    let bytes = typed_parameter_record(
        &TypedParameter {
            record_index: 242,
            owner: None,
            expression: "7.5 mm",
            source_kind: "User Parameter",
            unit: Unit::Token("mm"),
            name: "Offset",
            value: 0.75,
        },
        TAIL_TYPE,
    );
    let parameter = parse_design_parameter_record(&bytes).expect("typed document parameter");
    assert_eq!(parameter.owner_record_index(), None);
    assert_eq!(
        parameter.source(),
        &DesignParameterSource::User {
            family_discriminator: None
        }
    );
    assert_eq!(parameter.expression_offset(), 35);
    assert_eq!(parameter.name(), "Offset");
    assert_eq!(parameter.evaluated_value().get(), 0.75);

    // The owned trailer is not admitted after a null owner reference.
    let mut owned_trailer = bytes;
    let trailer_end = 31 + 4 + 2 * "7.5 mm".len() + 4;
    owned_trailer[trailer_end] = 0;
    assert!(parse_design_parameter_record(&owned_trailer).is_none());
}

#[test]
fn typed_parameter_unit_is_a_token_an_empty_field_or_absent() {
    for (unit, expected) in [
        (Unit::Token("deg"), Some("deg")),
        (Unit::ExplicitEmpty, None),
        (Unit::Absent, None),
    ] {
        let bytes = typed_parameter_record(
            &TypedParameter {
                unit,
                source_kind: "Count",
                name: "d9",
                ..owned(91)
            },
            TAIL_TYPE,
        );
        let parameter = parse_design_parameter_record(&bytes).expect("typed parameter unit");
        assert_eq!(parameter.unit().map(|field| field.value.as_str()), expected);
        assert_eq!(parameter.name(), "d9");
    }
}

#[test]
fn typed_parameter_requires_its_tail_type_trailer_and_exact_scalar_boundary() {
    let other_tail = typed_parameter_record(&owned(537), SCOPE_TYPE);
    assert!(parse_design_parameter_record(&other_tail).is_none());

    let mut trailer = typed_parameter_record(&owned(537), TAIL_TYPE);
    let trailer_at = 81 + 4 + 2 * "3 mm".len();
    trailer[trailer_at + 3] = 1;
    assert!(parse_design_parameter_record(&trailer).is_none());

    // An extra byte before the tail moves the scalar off its boundary.
    let mut misplaced = typed_parameter_record(&owned(537), TAIL_TYPE);
    let tail_at = misplaced.len() - 52;
    misplaced.insert(tail_at, 0);
    assert!(parse_design_parameter_record(&misplaced).is_none());
}

fn archive(bulk: &[u8]) -> Vec<u8> {
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(STREAM, stored).unwrap();
    zip.write_all(bulk).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn decode_reports_typed_parameter_frames_that_match_no_grammar() {
    let mut bulk = typed_parameter_record(&owned(537), TAIL_TYPE);
    let mut rejected = typed_parameter_record(&owned(541), TAIL_TYPE);
    let trailer_at = 81 + 4 + 2 * "3 mm".len();
    rejected[trailer_at] = 7;
    bulk.extend_from_slice(&rejected);
    let archive = archive(&bulk);
    let decoded = with_scan(&archive, |scan| {
        decode_parameters(&cadmpeg_test_support::service_decode_context(), scan)
    })
    .unwrap();
    let [parameter] = decoded.parameters.as_slice() else {
        panic!("expected the one well-formed typed parameter");
    };
    assert_eq!(parameter.record_index, 537);
    let [unparsed] = decoded.unparsed.as_slice() else {
        panic!("expected one stream with unparsed typed parameter frames");
    };
    assert_eq!(unparsed.stream, STREAM);
    assert_eq!(unparsed.count, 1);
}

#[test]
fn decode_reports_no_unparsed_frames_when_every_typed_parameter_parses() {
    let mut bulk = typed_parameter_record(&owned(537), TAIL_TYPE);
    bulk.extend_from_slice(&typed_parameter_record(&owned(541), TAIL_TYPE));
    let archive = archive(&bulk);
    let decoded = with_scan(&archive, |scan| {
        decode_parameters(&cadmpeg_test_support::service_decode_context(), scan)
    })
    .unwrap();
    assert_eq!(decoded.parameters.len(), 2);
    assert!(decoded.unparsed.is_empty());
}

/// A scalar owner whose five references all use the typed spelling.
fn typed_owner_frame(repeated_scope_tail: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&3u32.to_le_bytes());
    frame.extend_from_slice(b"378");
    frame.extend_from_slice(&536u32.to_le_bytes());
    frame.extend_from_slice(&[0; 8]);
    frame.extend_from_slice(&[1, 1, 0, 0, 0]);
    typed_reference(&mut frame, 226, SCOPE_TYPE);
    frame.extend_from_slice(&2u32.to_le_bytes());
    frame.push(0);
    frame.extend_from_slice(&0.3f64.to_le_bytes());
    typed_reference(&mut frame, 537, PARAMETER_TYPE);
    frame.extend_from_slice(&1u32.to_le_bytes());
    frame.extend_from_slice(&[0; 4]);
    typed_reference(&mut frame, 226, SCOPE_TYPE);
    frame.extend_from_slice(repeated_scope_tail);
    typed_reference(&mut frame, 538, COMPANION_TYPE);
    frame.push(0);
    typed_reference(&mut frame, 226, SCOPE_TYPE);
    frame
}

#[test]
fn typed_owner_frame_widens_every_reference_and_keeps_the_scalar_envelope() {
    let frame = typed_owner_frame(&[0, 0]);
    assert_eq!(frame.len(), 303);
    let parsed = parse_parameter_owner(&frame).expect("typed scalar owner");
    assert_eq!(parsed.frame_length, 303);
    assert_eq!(parsed.record_index, 536);
    assert_eq!(parsed.scope_record_index, 226);
    assert_eq!(parsed.local_ordinal, 2);
    assert_eq!(parsed.evaluated_value.get(), 0.3);
    assert_eq!(parsed.parameter_record_index, 537);
    assert_eq!(parsed.owned_ordinal, 1);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.companion_record_index, 538);
    let owner = parsed
        .into_record(STREAM, 1000)
        .expect("typed owner record");
    assert_eq!(owner.evaluated_value_offset(), 1080);
}

#[test]
fn typed_owner_frame_admits_the_compact_and_full_variant_slots() {
    let compact = typed_owner_frame(&[1, 1]);
    let parsed = parse_parameter_owner(&compact).expect("typed compact variant owner");
    assert_eq!(parsed.frame_length, 303);
    assert_eq!(parsed.variant, Some(1));
    assert!(parsed.into_record(STREAM, 0).is_some());

    let full = typed_owner_frame(&[1, 0, 0]);
    let parsed = parse_parameter_owner(&full).expect("typed variant owner");
    assert_eq!(parsed.frame_length, 304);
    assert_eq!(parsed.variant, Some(0));
    assert!(parsed.into_record(STREAM, 0).is_some());
}

#[test]
fn typed_owner_frame_rejects_mixed_spellings_and_mismatched_scope() {
    // One plain reference among typed ones is not an owner frame.
    let mut mixed = typed_owner_frame(&[0, 0]);
    let final_scope = mixed.len() - 51;
    mixed.truncate(final_scope);
    mixed.push(1);
    mixed.extend_from_slice(&226u64.to_le_bytes());
    mixed.extend_from_slice(&[0, 0]);
    assert!(parse_parameter_owner(&mixed).is_none());

    let mut scope = typed_owner_frame(&[0, 0]);
    let final_scope = scope.len() - 51;
    scope[final_scope + 1] = 227;
    assert!(parse_parameter_owner(&scope).is_none());
}

#[test]
fn typed_parameter_owner_reference_can_omit_its_inline_type_guid() {
    let typed = typed_parameter_record(&owned(537), TAIL_TYPE);
    let mut plain = typed[..30].to_vec();
    plain.push(1);
    plain.extend_from_slice(&536u64.to_le_bytes());
    plain.extend_from_slice(&[0, 0]);
    plain.extend_from_slice(&typed[81..]);
    let parameter = parse_design_parameter_record(&plain).expect("plain-owner typed parameter");
    assert_eq!(parameter.owner_record_index(), Some(536));
    assert_eq!(parameter.expression(), "3 mm");
    assert_eq!(parameter.expression_offset(), 45);
    assert_eq!(parameter.name(), "d1");

    // A null owner with a feature-input source kind is not a parameter.
    let mut ownerless = typed[..30].to_vec();
    ownerless.push(0);
    lp_utf16(&mut ownerless, "3 mm");
    ownerless.extend_from_slice(&[0, 0, 0, 1, 0]);
    let kind_at = 81 + 4 + 2 * "3 mm".len() + 5;
    ownerless.extend_from_slice(&typed[kind_at..]);
    assert!(parse_design_parameter_record(&ownerless).is_none());
}

#[test]
fn typed_owner_presence_must_agree_with_the_document_source_kind() {
    let ownerless_input = typed_parameter_record(
        &TypedParameter {
            owner: None,
            ..owned(537)
        },
        TAIL_TYPE,
    );
    assert!(parse_design_parameter_record(&ownerless_input).is_none());
    let owned_document = typed_parameter_record(
        &TypedParameter {
            source_kind: "User Parameter",
            ..owned(537)
        },
        TAIL_TYPE,
    );
    assert!(parse_design_parameter_record(&owned_document).is_none());
}
