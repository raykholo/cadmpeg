// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::HashMap;

fn header_bytes(native: [u8; 6], diameter: f64) -> Vec<u8> {
    let mut bytes = native.to_vec();
    bytes.extend_from_slice(&diameter.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes
}

fn header_from(bytes: &[u8]) -> Res<Header> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::desktop()).unwrap();
    let mut reader = Reader::new(&ctx, bytes, 0, HashMap::new(), 0, None);
    let value = read_header(&mut reader)?;
    assert_eq!(reader.p, 18);
    Ok(value)
}

fn tail_from(words: [u32; 3], native: [u8; 8]) -> Res<[u8; 8]> {
    let mut bytes = words
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    bytes.extend_from_slice(&native);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).unwrap();
    let mut reader = Reader::new(&ctx, &bytes, 0, HashMap::new(), 0, None);
    let value = read_catalog_tail(&mut reader)?;
    assert_eq!(reader.p, 20);
    Ok(value)
}

#[test]
fn header_keeps_every_native_cell_byte_and_the_diameter_slot() {
    for (native, diameter) in [([0; 6], 0.005), ([1; 6], 0.0), ([9, 8, 7, 6, 5, 4], 0.007)] {
        assert_eq!(
            header_from(&header_bytes(native, diameter)).unwrap(),
            Header { native, diameter }
        );
    }
}

#[test]
fn header_checks_guard_and_truncation() {
    let bytes = header_bytes([1; 6], 0.0);
    for end in 0..bytes.len() {
        assert!(header_from(&bytes[..end]).is_err());
    }
    let mut bad = bytes;
    bad[14] = 2;
    assert!(header_from(&bad).is_err());
}

#[test]
fn catalogue_tail_keeps_every_native_cell_byte() {
    for native in [[0; 8], [1; 8], [255; 8], [1, 2, 3, 4, 5, 6, 7, 8]] {
        assert_eq!(tail_from([1, 1, u32::MAX], native).unwrap(), native);
    }
}

#[test]
fn catalogue_tail_checks_its_fixed_words() {
    for index in 0..3 {
        let mut words = [1, 1, u32::MAX];
        words[index] = 2;
        assert!(tail_from(words, [1; 8]).is_err());
    }
}

#[test]
fn suffix_padding_ends_at_the_next_archive_object_or_stream_end() {
    for count in [0, 12, 24, 40, 128] {
        for next in [&[][..], &[1, 128][..], &[0, 129][..], &[255, 255, 1, 0][..]] {
            let mut bytes = vec![0; count];
            bytes.extend_from_slice(next);
            let arena = DecodeArena::new();
            let (ctx, _) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).unwrap();
            let known = HashMap::from([
                (1, std::sync::Arc::from("moNodeName_c")),
                (256, std::sync::Arc::from("moNodeName_c")),
            ]);
            let mut reader = Reader::new(&ctx, &bytes, 0, known, 1000, None);
            finish(&mut reader).unwrap();
            assert_eq!(reader.p, count);
        }
    }
}

#[test]
fn suffix_preserves_nonzero_unframed_data_as_a_stopped_record() {
    let bytes = [0, 0, 2, 0];
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).unwrap();
    let mut reader = Reader::new(&ctx, &bytes, 0, HashMap::new(), 1000, None);
    assert!(finish(&mut reader).is_err());
    assert_eq!(reader.p, 2);
}
