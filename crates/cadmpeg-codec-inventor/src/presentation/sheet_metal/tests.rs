// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};

use super::{parse_sheet_metal_rule, parse_unfold_rule, supports_version};
use crate::pmdc::PmDcReference;

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    push_u32(
        bytes,
        u32::try_from(units.len()).expect("fixture string fits u32"),
    );
    for unit in units {
        push_u16(bytes, unit);
    }
}

fn style_header(bytes: &mut Vec<u8>, category: u32, name: &str) {
    push_u32(bytes, 0);
    push_u16(bytes, 0x0789);
    bytes.push(0);
    push_u16(bytes, 1);
    push_u16(bytes, 3);
    push_u16(bytes, 0);
    push_u32(bytes, 1);
    push_u32(bytes, 0);
    push_u32(bytes, category);
    push_utf16(bytes, name);
    push_utf16(bytes, "");
    push_u16(bytes, 0);
    push_utf16(bytes, &format!("1:{name}\n"));
}

fn rule_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    style_header(&mut bytes, 0x8000_001e, "Rule");
    for text in ["a", "b", "c", "d", "e"] {
        push_utf16(&mut bytes, text);
    }
    for value in [2, 1, 4] {
        push_u32(&mut bytes, value);
    }
    push_utf16(&mut bytes, "f");
    push_u32(&mut bytes, 4);
    push_utf16(&mut bytes, "g");
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0x8000_0037);
    push_u32(&mut bytes, 0x8000_0034);
    push_utf16(&mut bytes, "3 mm");
    push_utf16(&mut bytes, "i");
    push_u32(&mut bytes, 1);
    bytes
}

fn unfold_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    style_header(&mut bytes, 0x8000_001f, "Unfold");
    push_u32(&mut bytes, 1);
    push_utf16(&mut bytes, "0.440 ul");
    push_utf16(&mut bytes, "0.440 ul");
    push_u16(&mut bytes, 2);
    push_u16(&mut bytes, 0x3000);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 3);
    for value in [4e-5_f64, 0.004, 0.004] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 1]);
    push_u32(&mut bytes, 3);
    push_u16(&mut bytes, 2);
    push_u16(&mut bytes, 0x3000);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0x8000_0086);
    push_utf16(&mut bytes, "0.500 ul");
    bytes
}

fn with_view<T>(bytes: &[u8], test: impl FnOnce(&DecodeContext<'_>, View<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("synthetic payload fits policy");
    test(&ctx, view)
}

#[test]
fn sheet_metal_rule_frames_its_texts_values_and_references() {
    let rule = with_view(&rule_bytes(), |ctx, view| {
        parse_sheet_metal_rule(ctx, view, 19).expect("rule parses")
    });
    assert_eq!(rule.header.name, "Rule");
    assert_eq!(rule.header.label, "1:Rule\n");
    assert_eq!(
        rule.header.category,
        PmDcReference::from_packed(0x8000_001e)
    );
    assert_eq!(
        rule.texts,
        ["a", "b", "c", "d", "e", "f", "g", "3 mm", "i"].map(String::from)
    );
    assert_eq!(rule.integers, [2, 1, 4, 4, 1, 1]);
    assert_eq!(rule.unfold_rule.record_ordinal(), Some(0x36));
    assert_eq!(rule.material_style.record_ordinal(), Some(0x33));
}

#[test]
fn sheet_metal_rule_refuses_trailing_bytes() {
    let mut bytes = rule_bytes();
    bytes.push(0);
    with_view(&bytes, |ctx, view| {
        assert!(parse_sheet_metal_rule(ctx, view, 19)
            .expect_err("trailing byte")
            .to_string()
            .contains("trailing bytes"));
    });
}

#[test]
fn unfold_rule_frames_its_method_and_factors() {
    let rule = with_view(&unfold_bytes(), |ctx, view| {
        parse_unfold_rule(ctx, view, 19).expect("unfold rule parses")
    });
    assert_eq!(rule.header.name, "Unfold");
    assert_eq!(rule.method, 1);
    assert_eq!(rule.factor, "0.440 ul");
    assert_eq!(rule.secondary_factor, "0.440 ul");
    assert!(rule.members.references().is_empty());
    assert_eq!(rule.value_count, 3);
    assert_eq!(
        rule.scalars.map(cadmpeg_ir::scalar::FiniteReal::get),
        [4e-5, 0.004, 0.004]
    );
    assert_eq!((rule.first_state, rule.second_state), (0, 1));
    assert_eq!(rule.mode, 3);
    assert_eq!(
        rule.references.references(),
        &[PmDcReference::from_packed(0x8000_0086)]
    );
    assert_eq!(rule.tertiary_factor, "0.500 ul");
}

#[test]
fn unfold_rule_refuses_a_truncated_payload() {
    let mut bytes = unfold_bytes();
    bytes.truncate(bytes.len() - 2);
    with_view(&bytes, |ctx, view| {
        assert!(parse_unfold_rule(ctx, view, 19).is_err());
    });
}

#[test]
fn sheet_metal_style_grammar_covers_versions_17_through_23() {
    assert!(!supports_version(16));
    assert!(supports_version(17));
    assert!(supports_version(23));
    assert!(!supports_version(24));
}

#[test]
fn sheet_metal_style_records_round_trip_through_their_native_wire() {
    use crate::record_identity::{Located, RecordTypeId};
    let rule = with_view(&rule_bytes(), |ctx, view| {
        parse_sheet_metal_rule(ctx, view, 19).expect("rule parses")
    });
    let unfold = with_view(&unfold_bytes(), |ctx, view| {
        parse_unfold_rule(ctx, view, 19).expect("unfold rule parses")
    });
    let rule = Located::new(
        rule,
        RecordTypeId::from_bytes(super::SHEET_METAL_RULE_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        53,
    );
    let unfold = Located::new(
        unfold,
        RecordTypeId::from_bytes(super::UNFOLD_RULE_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        54,
    );
    let rule_wire = serde_json::to_value(&rule).expect("rule serializes");
    assert_eq!(
        rule_wire["id"],
        "inventor:presentation:sheet-metal-rule#segment-53"
    );
    let rule_back: Located<super::PmAppSheetMetalRule> =
        serde_json::from_value(rule_wire).expect("rule deserializes");
    assert_eq!(rule_back, rule);
    let unfold_wire = serde_json::to_value(&unfold).expect("unfold rule serializes");
    let unfold_back: Located<super::PmAppUnfoldRule> =
        serde_json::from_value(unfold_wire).expect("unfold rule deserializes");
    assert_eq!(unfold_back, unfold);
}
