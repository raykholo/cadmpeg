// SPDX-License-Identifier: Apache-2.0
use super::*;

fn push_f64s(bytes: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

#[test]
fn parses_generated_container_link_plane_and_surface() {
    let mut container = Vec::new();
    container.extend_from_slice(&0u32.to_le_bytes());
    container.extend_from_slice(&2u16.to_le_bytes());
    container.extend_from_slice(&0x8000_0002u32.to_le_bytes());
    container.extend_from_slice(&0x40u32.to_le_bytes());
    container.extend_from_slice(&[7; 16]);
    crate::test_support::test_fixtures::push_utf16(&mut container, "Part");
    container.extend_from_slice(&references(&[]));
    for value in 0..15u32 {
        container.extend_from_slice(&value.to_le_bytes());
    }
    crate::test_support::test_fixtures::push_utf16(&mut container, "PmBRepSegment");
    container.extend_from_slice(&2.7f64.to_le_bytes());
    container.extend_from_slice(&0u32.to_le_bytes());
    let parsed = parse(&container, |ctx, source| {
        crate::feature::parse_container(ctx, source, 19).expect("container")
    });
    assert_eq!(parsed.name, "Part");
    assert_eq!(parsed.segment, crate::feature::MODEL_HISTORY_SEGMENT);
    assert_eq!(parsed.next.index(), 2);
    assert_eq!(parsed.values[14], 14);
    container.push(0);
    assert!(
        parse(&container, |ctx, source| crate::feature::parse_container(
            ctx, source, 19
        ))
        .is_err()
    );

    let mut link = Vec::new();
    link.extend_from_slice(&0u32.to_le_bytes());
    link.extend_from_slice(&0x1a6u16.to_le_bytes());
    link.extend_from_slice(&107u32.to_le_bytes());
    link.push(4);
    link.extend_from_slice(&0x8000_013du32.to_le_bytes());
    link.extend_from_slice(&3u32.to_le_bytes());
    link.extend_from_slice(&38u32.to_le_bytes());
    let parsed = parse(&link, |_, source| {
        crate::feature::parse_kernel_entity_link(source, 19).expect("kernel entity link")
    });
    assert_eq!(parsed.selection_source, 107);
    assert_eq!(parsed.body.index(), 0x13d);
    assert_eq!(
        (parsed.entity_id, parsed.entity_type, parsed.state),
        (3, 38, 4)
    );

    let frame = [1.5, 1.2, 0.3, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    for (version, marker, extended) in [(19, false, false), (19, true, false), (22, true, true)] {
        let mut plane = content(9);
        plane.extend_from_slice(&0x80_8000u32.to_le_bytes());
        plane.extend_from_slice(&[0; 8]);
        if extended {
            plane.extend_from_slice(&0u32.to_le_bytes());
        }
        if marker {
            plane.extend_from_slice(&crate::feature::WORK_PLANE_FRAME_MARKER.to_le_bytes());
        }
        plane.extend_from_slice(&0u32.to_le_bytes());
        push_f64s(&mut plane, &frame);
        let parsed = parse(&plane, |_, source| {
            crate::feature::parse_work_plane(source, version).expect("work plane")
        });
        assert_eq!(parsed.marker, marker);
        assert_eq!(parsed.values.len(), if extended { 4 } else { 3 });
        assert_eq!(parsed.origin[0].get(), 1.5);
        assert_eq!(parsed.u_axis[0].get(), -1.0);
        assert_eq!(parsed.v_axis[2].get(), 1.0);
    }

    let mut flag = content(10);
    flag.push(1);
    let parsed = parse(&flag, |ctx, source| {
        crate::feature::parse_termination_flag(ctx, source, 19).expect("termination flag")
    });
    assert_eq!(
        parsed.kind,
        PmDcFeaturePropertyKind::TerminationFlag { value: 1 }
    );

    let mut surface = content(11);
    surface.extend_from_slice(&[0; 12]);
    surface.push(1);
    surface.extend_from_slice(&crate::feature::PLANE_SURFACE_FORM.to_le_bytes());
    surface.extend_from_slice(&0u32.to_le_bytes());
    push_f64s(&mut surface, &frame);
    let parsed = parse(&surface, |_, source| {
        crate::feature::parse_surface(source, 19).expect("surface")
    })
    .expect("plane surface");
    assert!(matches!(
        parsed.kind,
        PmDcFeaturePropertyKind::PlaneSurface {
            state: 1,
            surface_form: 25,
            ..
        }
    ));
    let mut cylinder = content(12);
    cylinder.extend_from_slice(&[0; 12]);
    cylinder.push(1);
    cylinder.extend_from_slice(&29u32.to_le_bytes());
    cylinder.extend_from_slice(&1u32.to_le_bytes());
    push_f64s(&mut cylinder, &frame);
    assert!(parse(&cylinder, |_, source| crate::feature::parse_surface(
        source, 19
    ))
    .expect("surface form")
    .is_none());
}

/// At segment major versions 23 and 25 the feature and terminator keep their content header,
/// feature properties extend it by one and two values, and a fillet edge set ends with a u32.
#[test]
fn version_23_and_25_feature_records_follow_their_class_header() {
    let mut feature = content(7);
    feature.extend_from_slice(&(-1i32).to_le_bytes());
    feature.extend_from_slice(&42u32.to_le_bytes());
    feature.extend_from_slice(&references(&[]));
    feature.extend_from_slice(&9u32.to_le_bytes());
    for version in [22, 23, 25] {
        let parsed = parse(&feature, |ctx, source| {
            crate::feature::parse_feature(ctx, source, version).expect("feature")
        });
        assert!(parsed.header.extension.is_empty());
        assert_eq!(parsed.value, 9);
    }
    let mut terminator = content(8);
    terminator.extend_from_slice(&(-1i32).to_le_bytes());
    for version in [23, 25] {
        let parsed = parse(&terminator, |_, source| {
            crate::feature::parse_terminator(source, version).expect("terminator")
        });
        assert_eq!(parsed.state, -1);
    }

    for (version, extension) in [(22, Vec::<i32>::new()), (23, vec![-1]), (25, vec![-1, 7])] {
        let mut fillet_set = content(18);
        for value in &extension {
            fillet_set.extend_from_slice(&value.to_le_bytes());
        }
        for reference in 0x8000_0010u32..0x8000_0014 {
            fillet_set.extend_from_slice(&reference.to_le_bytes());
        }
        if version >= 23 {
            fillet_set.extend_from_slice(&0u32.to_le_bytes());
        }
        let parsed = parse(&fillet_set, |ctx, source| {
            crate::feature::parse_fillet_edge_set(ctx, source, version).expect("fillet edge set")
        });
        assert_eq!(parsed.header.extension, extension);
        assert_eq!(parsed.tail, (version >= 23).then_some(0));
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::FilletEdgeSet { radius, .. } if radius.index() == 17
        ));
    }
}

/// Segment major versions 23 and later store a fourth reference before the surface state.
#[test]
fn version_23_surfaces_store_four_references() {
    let frame = [1.5, 1.2, 0.3, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    for (version, extension, references) in [(22, 0, 3), (23, 1, 4), (25, 2, 4)] {
        let mut plane = content(12);
        for _ in 0..extension {
            plane.extend_from_slice(&(-1i32).to_le_bytes());
        }
        for _ in 0..references {
            plane.extend_from_slice(&0u32.to_le_bytes());
        }
        plane.push(1);
        plane.extend_from_slice(&crate::feature::PLANE_SURFACE_FORM.to_le_bytes());
        plane.extend_from_slice(&0u32.to_le_bytes());
        push_f64s(&mut plane, &frame);
        let parsed = parse(&plane, |_, source| {
            crate::feature::parse_surface(source, version).expect("surface")
        })
        .expect("plane surface");
        assert!(matches!(
            parsed.kind,
            PmDcFeaturePropertyKind::PlaneSurface { state: 1, surface_form: 25, fourth_reference, .. }
                if fourth_reference.is_some() == (version >= 23)
        ));
    }
}
