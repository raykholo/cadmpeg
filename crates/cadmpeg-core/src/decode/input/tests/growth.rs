// SPDX-License-Identifier: Apache-2.0

use super::super::{reserve_input, reserve_input_hint, INPUT_CHUNK};
use crate::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceDimension,
};
use crate::CodecError;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

fn read_with_growth(length: usize, hint: Option<u64>, limit: usize) -> (Vec<u8>, usize) {
    let mut reader = io::repeat(0x5a).take(u64_from_index(length));
    let mut bytes = Vec::new();
    let mut changes = 0;
    if let Some(hint) = hint {
        reserve_input_hint(&mut bytes, hint, u64_from_index(limit));
        changes += usize::from(bytes.capacity() != 0);
    }
    let mut chunk = [0; INPUT_CHUNK];
    loop {
        let read = reader.read(&mut chunk).expect("read");
        if read == 0 {
            break;
        }
        let previous = bytes.capacity();
        reserve_input(&mut bytes, read, Some(limit)).expect("reserve");
        changes += usize::from(previous != bytes.capacity());
        assert!(bytes.capacity() <= limit);
        bytes.extend_from_slice(&chunk[..read]);
    }
    (bytes, changes)
}

#[test]
fn unhinted_input_growth_is_logarithmic() {
    let length = 8 * 1024 * 1024;
    let (bytes, changes) = read_with_growth(length, None, length);
    assert_eq!(bytes.len(), length);
    assert!(bytes.iter().all(|&byte| byte == 0x5a));
    assert!(changes <= 11);
}

#[test]
fn hinted_input_needs_one_reservation() {
    let length = 8 * 1024 * 1024;
    let (bytes, changes) = read_with_growth(length, Some(u64_from_index(length)), length);
    assert_eq!(bytes.len(), length);
    assert_eq!(changes, 1);
}

#[test]
fn growth_stops_at_small_capacity_limit() {
    let limit = INPUT_CHUNK * 2 + 13;
    let (bytes, changes) = read_with_growth(limit, None, limit);
    assert_eq!(bytes.capacity(), limit);
    assert_eq!(changes, 3);
}

#[test]
fn prefix_capacity_stops_at_requested_length() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let length = INPUT_CHUNK * 2 + 13;
    let bytes = ctx
        .read_input_prefix(&mut io::repeat(0x5a), length)
        .expect("prefix");
    assert_eq!(bytes.len(), length);
    assert_eq!(bytes.capacity(), length);
}

#[test]
fn complete_input_caps_capacity_and_charges_read_bytes() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    let limit = INPUT_CHUNK * 2 + 13;
    policy.limits.max_input_bytes = u64_from_index(limit);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut bytes = Vec::new();
    ctx.complete_input(
        &mut io::repeat(0x5a).take(u64_from_index(limit)),
        &mut bytes,
    )
    .expect("complete");
    assert_eq!(bytes.len(), limit);
    assert_eq!(bytes.capacity(), limit);
    assert_eq!(ctx.budget.input_bytes(), u64_from_index(limit));
}

#[test]
fn hint_over_the_limit_reserves_nothing() {
    let mut bytes = Vec::new();
    reserve_input_hint(&mut bytes, u64::MAX, 17);
    assert_eq!(bytes.capacity(), 0);
    reserve_input_hint(&mut bytes, 18, 17);
    assert_eq!(bytes.capacity(), 0);
}

#[test]
fn hint_within_the_limit_reserves_it_exactly() {
    let mut bytes = Vec::new();
    reserve_input_hint(&mut bytes, 17, 17);
    assert_eq!(bytes.capacity(), 17);
    assert!(bytes.is_empty());
}

struct RootReader {
    bytes: Cursor<Vec<u8>>,
    hint: Option<u64>,
}

impl Read for RootReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.bytes.read(bytes)
    }
}

impl Seek for RootReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        match (self.hint, position) {
            (Some(hint), SeekFrom::End(0)) => Ok(hint),
            (Some(_), _) => self.bytes.seek(position),
            (None, _) => Err(io::Error::from(io::ErrorKind::Unsupported)),
        }
    }
}

#[test]
fn root_readers_preserve_large_input_with_and_without_seek() {
    let length = 8 * 1024 * 1024;
    for hint in [None, Some(u64_from_index(length))] {
        let arena = DecodeArena::new();
        let mut reader = RootReader {
            bytes: Cursor::new(vec![0x5a; length]),
            hint,
        };
        let (ctx, root) = DecodeContext::read_root(
            &mut reader,
            &arena,
            &DecodePolicy::default(),
            DecodeDepth::Full,
        )
        .expect("root");
        assert_eq!(root.window(), reader.bytes.get_ref());
        assert_eq!(ctx.budget.input_bytes(), u64_from_index(length));
    }
}

#[test]
fn failed_hint_allocation_falls_back_to_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_input_bytes = u64::MAX;
    let mut reader = RootReader {
        bytes: Cursor::new(b"abc".to_vec()),
        hint: Some(u64::MAX),
    };
    let (ctx, root) =
        DecodeContext::read_root(&mut reader, &arena, &policy, DecodeDepth::Full).expect("root");
    assert_eq!(root.window(), b"abc");
    assert_eq!(ctx.budget.input_bytes(), 3);
}

#[test]
fn seekable_input_above_limit_preserves_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_input_bytes = 2;
    assert!(matches!(
        DecodeContext::read_root(
            &mut Cursor::new(b"abc"), &arena, &policy, DecodeDepth::Full
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::InputBytes
                && limit.used == 2 && limit.additional == 1
                && limit.operation == "complete input"
                && limit.reason == crate::decode::ResourceFailure::BudgetExceeded
                && limit.limit == 2
    ));
}

#[test]
#[ignore = "manual 256 MiB input growth timing"]
fn input_growth_timing() {
    use std::time::Instant;

    let length = 256 * 1024 * 1024;
    for geometric in [false, true] {
        let start = Instant::now();
        let mut reader = io::repeat(0x5a).take(u64_from_index(length));
        let mut bytes = Vec::new();
        let mut chunk = [0; INPUT_CHUNK];
        let mut changes = 0;
        loop {
            let read = reader.read(&mut chunk).expect("read");
            if read == 0 {
                break;
            }
            let previous = bytes.capacity();
            if geometric {
                reserve_input(&mut bytes, read, Some(length)).expect("reserve");
            } else {
                bytes.try_reserve_exact(read).expect("reserve");
            }
            changes += usize::from(bytes.capacity() != previous);
            bytes.extend_from_slice(&chunk[..read]);
        }
        assert_eq!(bytes.len(), length);
        eprintln!(
            "geometric={geometric} elapsed={:?} capacity_changes={changes}",
            start.elapsed()
        );
    }
}

#[test]
fn underestimated_hint_allows_geometric_growth() {
    let length = INPUT_CHUNK * 4;
    let (bytes, changes) = read_with_growth(length, Some(1), length);
    assert_eq!(bytes.len(), length);
    assert!(changes <= 4);
}

#[test]
fn zero_hint_allows_growth() {
    let (bytes, changes) = read_with_growth(17, Some(0), 17);
    assert_eq!(bytes.capacity(), 17);
    assert_eq!(changes, 1);
}

#[test]
fn input_storage_allocation_is_fallible() {
    let mut bytes = Vec::new();
    assert!(reserve_input(&mut bytes, usize::MAX, Some(usize::MAX)).is_err());
    assert!(bytes.is_empty());
    assert_eq!(bytes.capacity(), 0);
}

#[test]
fn spare_capacity_does_not_charge_input_budget() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_input_bytes = u64_from_index(INPUT_CHUNK * 4);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let length = INPUT_CHUNK + 1;
    let mut bytes = Vec::new();
    ctx.complete_input(
        &mut io::repeat(0x5a).take(u64_from_index(length)),
        &mut bytes,
    )
    .expect("complete");
    assert!(bytes.capacity() > bytes.len());
    assert_eq!(ctx.budget.input_bytes(), u64_from_index(length));
}

#[test]
fn unaddressable_input_growth_is_fallible() {
    let mut bytes = vec![0_u8];
    assert!(reserve_input(&mut bytes, usize::MAX, None).is_err());
    assert_eq!(bytes, [0]);
}

#[test]
fn input_growth_accepts_an_unaddressable_policy_bound() {
    let mut bytes = Vec::new();
    reserve_input(&mut bytes, 1, None).expect("reserve");
    bytes.push(0);
    reserve_input(&mut bytes, 1, None).expect("grow");
    assert_eq!(bytes.capacity(), 2);
}
