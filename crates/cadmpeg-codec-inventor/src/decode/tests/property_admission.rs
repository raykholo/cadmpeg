// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;

use crate::container::InventorContainer;
use crate::decode::decode_container;
use crate::property_set::{
    Property, PropertySection, PropertySetDescriptor, PropertySetState, PropertySetStream,
    PropertyValue,
};
use crate::test_support::test_fixtures::primary_envelope_fixture;

#[test]
fn parsed_property_records_refuse_each_copy_before_materialization() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture context");
    let mut container = InventorContainer::open(&setup_ctx, root).expect("fixture container");
    container.property_sets.push(PropertySetDescriptor {
        stream: container.rse.segments[0].pair.metadata,
        path: "test-property-set".into(),
        state: PropertySetState::Parsed(PropertySetStream {
            version: 1,
            system_identifier: 0,
            clsid: [1; 16],
            sections: vec![PropertySection {
                fmtid: [2; 16],
                code_page: None,
                offsets_ordered: true,
                dictionary_entries: 0,
                properties: vec![Property {
                    id: 2,
                    name: Some("Name".into()),
                    value: PropertyValue::String {
                        type_code: 31,
                        value: "Value".into(),
                    },
                    raw: View::over_retained(b"raw"),
                }],
            }],
        }),
    });

    let mut cap = 0;
    let mut operations = Vec::new();
    let mut decoded = None;
    for _ in 0..1024 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        match decode_container(&ctx, &container) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert!(limit.used + limit.additional > cap);
                operations.push(limit.operation);
                cap = limit.used + limit.additional;
            }
            Ok(value) => {
                decoded = Some(value);
                break;
            }
            Err(error) => panic!("unexpected property projection error: {error}"),
        }
    }
    let decoded = decoded.expect("property projection reaches service success");
    let namespace = decoded
        .ir
        .native
        .namespace("inventor")
        .expect("native namespace");
    assert_eq!(
        namespace
            .arena_as::<serde_json::Value>("property_sets")
            .expect("set arena")
            .len(),
        1
    );
    assert_eq!(
        namespace
            .arena_as::<serde_json::Value>("property_sections")
            .expect("section arena")
            .len(),
        1
    );
    assert_eq!(
        namespace
            .arena_as::<serde_json::Value>("properties")
            .expect("value arena")
            .len(),
        1
    );
    for operation in [
        "retain Inventor property-set id",
        "retain Inventor property-set path",
        "retain Inventor property-set CLSID",
        "retain Inventor property section id",
        "retain Inventor property section path",
        "retain Inventor property section FMTID",
        "retain Inventor property name",
        "retain Inventor property value id",
        "retain Inventor property raw digest",
        "retain Inventor property value path",
    ] {
        assert!(
            operations.contains(&operation),
            "no refusal for {operation}"
        );
    }
}

#[test]
fn standard_document_properties_reach_source_attributes_and_root_product() {
    use crate::decode::metadata::{
        FMTID_DESIGN_TRACKING_CONTROL, FMTID_DESIGN_TRACKING_PROPERTIES,
        FMTID_INVENTOR_DOCUMENT_SUMMARY, FMTID_INVENTOR_SUMMARY, FMTID_SUMMARY_INFORMATION,
    };
    fn property(id: u32, value: PropertyValue<'static>) -> Property<'static> {
        Property {
            id,
            name: None,
            value,
            raw: View::over_retained(b"raw"),
        }
    }
    fn text(value: &str) -> PropertyValue<'static> {
        PropertyValue::String {
            type_code: 31,
            value: value.into(),
        }
    }
    fn section(fmtid: [u8; 16], properties: Vec<Property<'static>>) -> PropertySection<'static> {
        PropertySection {
            fmtid,
            code_page: Some(1200),
            offsets_ordered: true,
            dictionary_entries: 0,
            properties,
        }
    }
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture context");
    let mut container = InventorContainer::open(&setup_ctx, root).expect("fixture container");
    let sections = vec![
        section(
            FMTID_SUMMARY_INFORMATION,
            vec![
                property(2, text("Public title")),
                property(5, text("bracket, plate")),
            ],
        ),
        section(
            FMTID_INVENTOR_SUMMARY,
            vec![
                property(2, text("Bracket")),
                property(4, text("Designer A")),
                property(6, text("Bent plate")),
                property(9, text("C")),
            ],
        ),
        section(
            FMTID_INVENTOR_DOCUMENT_SUMMARY,
            vec![property(15, text("Example Co"))],
        ),
        section(
            FMTID_DESIGN_TRACKING_PROPERTIES,
            vec![
                property(5, text("BR-100")),
                property(7, text("Rig")),
                property(17, text("Released")),
                property(20, text("Steel")),
                property(29, text("")),
                property(41, text("Designer B")),
                property(
                    4,
                    PropertyValue::Filetime {
                        type_code: 64,
                        value: 116_444_736_000_000_000,
                    },
                ),
            ],
        ),
        section(
            FMTID_DESIGN_TRACKING_CONTROL,
            vec![property(
                17,
                PropertyValue::Filetime {
                    type_code: 64,
                    value: 116_444_736_010_000_000,
                },
            )],
        ),
    ];
    container.property_sets.push(PropertySetDescriptor {
        stream: container.rse.segments[0].pair.metadata,
        path: "test-property-set".into(),
        state: PropertySetState::Parsed(PropertySetStream {
            version: 1,
            system_identifier: 0,
            clsid: [0; 16],
            sections,
        }),
    });
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("service context");
    let decoded = decode_container(&ctx, &container).expect("decode");
    let source =
        serde_json::to_value(decoded.ir.source.as_ref().expect("source")).expect("source json");
    let attributes = &source["attributes"];
    for (key, value) in [
        ("title", "Bracket"),
        ("keywords", "bracket, plate"),
        ("author", "Designer A"),
        ("comments", "Bent plate"),
        ("revision", "C"),
        ("company", "Example Co"),
        ("part_number", "BR-100"),
        ("project", "Rig"),
        ("status", "Released"),
        ("material", "Steel"),
        ("designer", "Designer B"),
        ("created_at", "1970-01-01T00:00:00Z"),
        ("saved_at", "1970-01-01T00:00:01Z"),
    ] {
        assert_eq!(attributes[key], value, "{key}");
    }
    assert!(attributes.get("description").is_none());
    assert!(attributes.get("subject").is_none());
    let product = decoded
        .ir
        .model
        .product_definitions
        .first()
        .expect("root product");
    assert_eq!(product.label.as_deref(), Some("Bracket"));
    assert_eq!(product.part_number.as_deref(), Some("BR-100"));
    assert_eq!(product.description.as_deref(), Some("Bent plate"));
    let bom = serde_json::to_value(&product.bom_properties).expect("bom json");
    assert_eq!(
        bom["inventor:property:value#{}-0-2".replace(
            "{}",
            &container.rse.segments[0]
                .pair
                .metadata
                .directory_id()
                .to_string()
        )],
        "Public title"
    );
}
