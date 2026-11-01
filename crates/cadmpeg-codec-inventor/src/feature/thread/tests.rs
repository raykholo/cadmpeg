// SPDX-License-Identifier: Apache-2.0
//! Sizing grammar and native operand controls.

use super::*;
use crate::test_support::thread::definition_bytes;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn parsed(
    nominal: &str,
    designation: &str,
    standard: &str,
    form: u32,
    pitch: &str,
) -> TestResult<DefinitionPayload> {
    let bytes = definition_bytes(1, 2, nominal, designation, standard, form, pitch);
    let arena = DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())?;
    Ok(parse(&ctx, view, 19)?)
}

#[test]
fn thread_definition_metric_coarse_and_fine_use_explicit_pitch() -> TestResult {
    for (standard, forms) in [
        (
            "ANSI Metric M Profile",
            &[0x00ca, 0x10ca, 0x10ce, 0x10ea, 0x10ee][..],
        ),
        ("ISO Metric profile", &[0x00ca, 0x10ee][..]),
    ] {
        for &form in forms {
            for (designation, text_pitch, pitch) in [("M6x1", "1", 1.0), ("M6x0.75", "0.75", 0.75)]
            {
                let value = parsed("6", designation, standard, form, text_pitch)?
                    .sizing()
                    .ok_or("missing metric sizing")?;
                assert_eq!(value.diameter.get(), 6.0);
                assert_eq!(value.pitch.get(), pitch);
                assert_eq!(value.standard.as_str(), standard);
                assert_eq!(value.designation.as_str(), designation);
            }
        }
    }
    Ok(())
}

#[test]
fn thread_definition_inch_coarse_and_fine_convert_tpi() -> TestResult {
    for (designation, tpi, pitch) in [
        ("1/4-20 UNC", "20", 1.27),
        ("1/4-28 UNF", "28", 25.4 / 28.0),
    ] {
        let value = parsed(
            "0.25",
            designation,
            "ANSI Unified Screw Threads",
            0x18f2,
            tpi,
        )?
        .sizing()
        .ok_or("missing inch sizing")?;
        assert_eq!(value.diameter.get(), 6.35);
        assert_eq!(value.pitch.get(), pitch);
    }
    Ok(())
}

#[test]
fn thread_definition_rejects_reversed_units_mismatches_and_unresolved_forms() -> TestResult {
    for (nominal, designation, standard, form, pitch) in [
        ("6", "M6x1", "ANSI Metric M Profile", 0x18f2, "1"),
        (
            "0.25",
            "1/4-20 UNC",
            "ANSI Unified Screw Threads",
            0x10ea,
            "20",
        ),
        ("6", "M6x1", "BSP Pipe Threads", 0x10ea, "1"),
        ("6", "M6x1", "ANSI Metric M Profile", 0x10ea, "0.75"),
        ("5", "M6x1", "ANSI Metric M Profile", 0x10ea, "1"),
        ("6", "M6", "ANSI Metric M Profile", 0x10ea, "1"),
        ("6", "M6x-1", "ANSI Metric M Profile", 0x10ea, "-1"),
        ("6", "M6x0", "ANSI Metric M Profile", 0x10ea, "0"),
        (
            "0.25",
            "1/4-20 UNC",
            "ANSI Unified Screw Threads",
            0x18f2,
            "28",
        ),
        (
            "0.25",
            "1/0-20 UNC",
            "ANSI Unified Screw Threads",
            0x18f2,
            "20",
        ),
        (
            "0.25",
            "1/4-20 OTHER",
            "ANSI Unified Screw Threads",
            0x18f2,
            "20",
        ),
        ("NaN", "M6x1", "ANSI Metric M Profile", 0x10ea, "1"),
        ("6", "M6x1", "ANSI Metric M Profile", 0x10eb, "1"),
    ] {
        assert!(parsed(nominal, designation, standard, form, pitch)?
            .sizing()
            .is_none());
    }
    for position in [0, 2, 3, 4, 5] {
        let mut value = parsed("6", "M6x1", "ANSI Metric M Profile", 0x10ea, "1")?;
        value.controls[position] = 1;
        assert!(value.sizing().is_none());
    }
    for text in ["", "-1", "+1", "1e2", "inf", "1.2.3", "."] {
        assert!(decimal(text).is_none());
    }
    Ok(())
}

#[test]
fn thread_definition_requires_exact_record_termination() -> TestResult {
    let mut bytes = definition_bytes(1, 2, "6", "M6x1", "ANSI Metric M Profile", 0x10ea, "1");
    let arena = DecodeArena::new();
    for len in 0..bytes.len() {
        let (ctx, view) =
            DecodeContext::from_root_bytes(&bytes[..len], &arena, &DecodePolicy::service())?;
        assert!(parse(&ctx, view, 19).is_err());
    }
    bytes.push(0);
    let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())?;
    assert!(parse(&ctx, view, 19).is_err());
    Ok(())
}

#[test]
fn thread_definition_partial_loss_pins_native_operands() -> TestResult {
    assert!(partial_loss(0).is_none());
    let loss = partial_loss(3).ok_or("missing partial-definition loss")?;
    assert_eq!(loss.code.local_code(), "feature.thread-definition-partial");
    assert!(loss
        .message
        .contains("handedness, and termination operands remain native"));
    Ok(())
}

#[test]
fn thread_definition_requires_unique_typed_feature_owner_and_supported_versions() -> TestResult {
    use crate::feature::tests::{test_feature, test_label};
    use crate::record_identity::RecordTypeId;
    let mut source = test_feature(0, 9, &[]);
    source.edit(|value| value.save_version_major = 19);
    let mut label = test_label(0, 1, crate::feature::THREAD_CLASS_ID, &[]);
    label.edit(|value| value.save_version_major = 19);
    let definition = Located::new(
        parsed("6", "M6x1", "ANSI Metric M Profile", 0x10ea, "1")?,
        RecordTypeId::from_bytes(TYPE),
        source.identity.segment_token.clone(),
        2,
    );
    assert!(sizing(&source, &label, std::slice::from_ref(&definition)).is_some());
    let mut unrelated_parent = definition.clone();
    unrelated_parent.edit(|value| value.header.parent = crate::pmdc::PmDcReference::from_packed(0x8000_0010));
    assert!(sizing(&source, &label, &[unrelated_parent]).is_some());
    assert!(sizing(&source, &label, &[]).is_none());
    assert!(sizing(&source, &label, &[definition.clone(), definition.clone()]).is_none());
    for change in [
        "owner",
        "owner_qualifier",
        "parent_qualifier",
        "type",
        "segment",
        "version",
    ] {
        let mut invalid = definition.clone();
        match change {
            "owner" => invalid.edit(|value| {
                value.header.owner = crate::pmdc::PmDcReference::from_packed(0x8000_0002)
            }),
            "owner_qualifier" => invalid
                .edit(|value| value.header.owner = crate::pmdc::PmDcReference::from_packed(1)),
            "parent_qualifier" => invalid
                .edit(|value| value.header.parent = crate::pmdc::PmDcReference::from_packed(2)),
            "type" => invalid.identity.type_id = RecordTypeId::from_bytes([0; 16]),
            "segment" => invalid.identity.segment_token = cadmpeg_ir::identity_key!("other"),
            "version" => invalid.edit(|value| value.save_version_major = 23),
            _ => unreachable!(),
        }
        assert!(sizing(&source, &label, &[invalid]).is_none(), "{change}");
    }
    source.edit(|value| value.save_version_major = 23);
    assert!(sizing(&source, &label, std::slice::from_ref(&definition)).is_none());
    source.edit(|value| value.save_version_major = 19);
    label.edit(|value| value.save_version_major = 23);
    assert!(sizing(&source, &label, std::slice::from_ref(&definition)).is_none());
    label.edit(|value| value.save_version_major = 19);
    let mut wrong = serde_json::to_value(&label)?;
    wrong["class_id"] = serde_json::json!("00000000000000000000000000000000");
    let wrong = serde_json::from_value(wrong)?;
    assert!(sizing(&source, &wrong, &[definition]).is_none());
    Ok(())
}
