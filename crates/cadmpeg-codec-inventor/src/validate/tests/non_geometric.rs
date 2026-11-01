// SPDX-License-Identifier: Apache-2.0

use super::{empty_data, sketch_point, sketch_records, TestResult};
use crate::pmdc::{PmDcReference, PmDcReferenceList};
use crate::sketch::PmDcSketchEntityKind;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId};

#[test]
fn non_geometric_sources_are_rejected_as_sketch_entities() -> TestResult {
    let reference = PmDcReference::from_packed(0);
    let list = PmDcReferenceList::new(2, None, Vec::new())?;
    let mut kinds = vec![
        PmDcSketchEntityKind::Polygon {
            reference,
            value: 0,
        },
        PmDcSketchEntityKind::BlockMember {
            instances: list.clone(),
            member: reference,
        },
        PmDcSketchEntityKind::BlockInstance {
            definition: reference,
            group: reference,
            masks: [0x111, 0xee],
            placement: [
                [FiniteReal::ONE, FiniteReal::ZERO, FiniteReal::ZERO],
                [FiniteReal::ZERO, FiniteReal::ONE, FiniteReal::ZERO],
                [FiniteReal::ZERO, FiniteReal::ZERO, FiniteReal::ONE],
            ],
            value: 1,
        },
    ];
    kinds.push(PmDcSketchEntityKind::Point {
        position: [FiniteReal::ZERO; 2],
        endpoint_of: list.clone(),
        center_of: list,
        tail: crate::sketch::PointTail::Absent,
    });
    for (index, kind) in kinds.into_iter().enumerate() {
        let mut data = empty_data();
        let mut source = sketch_point(
            22,
            0,
            serde_json::json!({"state": null, "associations": null}),
        )?;
        crate::test_support::sketch_curves::edit_entity(&mut source, |payload| payload.kind = kind);
        source.identity.type_id =
            crate::record_identity::RecordTypeId::from_bytes(source.kind.record_type_id());
        let expected = index < 3;
        let native = source.id();
        data.pm_dc_sketch_entities.push(source);
        sketch_records(&mut data)?;
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.sketch_entities.push(
            SketchEntity::new(
                SketchEntityId::mint("inventor:design:sketch-entity#generated-0")?,
                SketchId::mint("inventor:design:sketch#generated-2")?,
                SketchGeometry::native(cadmpeg_core::nonblank_literal!("source")),
            )
            .with_native_ref(Some(native)),
        );
        let mut findings = Vec::new();
        super::super::validate_sketches(&data, &ir, &mut findings);
        assert_eq!(findings.len(), usize::from(expected), "{findings:?}");
        for finding in findings {
            assert!(finding.message.contains("non-geometric sketch record"));
        }
    }
    Ok(())
}

#[test]
fn non_geometric_relations_are_rejected_as_sketch_constraints() -> TestResult {
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId,
    };
    let null = PmDcReference::from_packed(0);
    for (kind, record_type, expected) in [
        (
            serde_json::json!({"form": "polygon_relation", "first": null, "second": null,
            "reference": null, "polygon": [null, null], "values": [0, 0]}),
            "1f89dbfbd84778b6b0967285c7ec8bb9",
            true,
        ),
        (
            serde_json::json!({"form": "block_group", "members": {"marker": 2, "metadata": null, "references": []}}),
            "cf8d56f2324a27ddb666f1894a9d1617",
            true,
        ),
        (
            serde_json::json!({"form": "fix", "entity": null}),
            "b07b170fd11196e680006fb1e13554c7",
            false,
        ),
    ] {
        let mut wire = serde_json::to_value(crate::test_support::sketch_curves::spline_midpoint())?;
        wire["kind"] = kind;
        wire["type_id"] = serde_json::json!(record_type);
        wire["header"]["parameter"] = serde_json::Value::Null;
        let source: crate::sketch::PmDcSketchConstraint = serde_json::from_value(wire)?;
        let mut data = empty_data();
        let native = source.id();
        data.records.push(serde_json::from_value(serde_json::json!({
            "id": "inventor:rse:record#generated-9", "token": "generated", "ordinal": 9,
            "selector": 0, "type_index": 0, "type_id": record_type,
            "payload_offset": 0, "payload_len": 0, "payload_sha256": "",
            "trailing_payload_len": 0, "trailer_len": 0, "trailer_sha256": ""
        }))?);
        data.pm_dc_sketch_constraints.push(source);
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId::mint("inventor:design:sketch-constraint#generated-9")?,
            sketch: SketchId::mint("inventor:design:sketch#generated-2")?,
            definition: SketchConstraintDefinitionInput::Native {
                native_kind: cadmpeg_core::nonblank_literal!("relation"),
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
                entities: Vec::new(),
                parameter: None,
                operands: Vec::new(),
            }
            .try_into()?,
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: Some(native),
        });
        let mut findings = Vec::new();
        super::super::validate_sketches(&data, &ir, &mut findings);
        assert_eq!(findings.len(), usize::from(expected), "{findings:?}");
        for finding in findings {
            assert!(finding.message.contains("non-geometric sketch relation"));
        }
    }
    Ok(())
}
