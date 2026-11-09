// SPDX-License-Identifier: Apache-2.0
//! Internal bodies selected by flat-pattern model roots.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::design::DesignInventory;
use crate::kernel::DecodedKernelCarrier;
use crate::rse::{RseInventory, SegmentKind, SegmentMetaState};

pub(crate) type FlatPatternBody = Result<DecodedKernelCarrier, String>;

pub(crate) fn read(
    ctx: &DecodeContext<'_>,
    rse: &RseInventory<'_>,
    design: &DesignInventory,
) -> Result<Vec<FlatPatternBody>, CodecError> {
    let mut bodies = Vec::new();
    for root in &design.model_roots {
        let mut segments = rse.segments.iter().filter(|segment| {
            matches!(&segment.meta, SegmentMetaState::Parsed(meta)
                if meta.display_name == root.brep_segment)
        });
        let Some(segment) = segments.next() else {
            continue;
        };
        if segments.next().is_some() || segment.kind != SegmentKind::MbBRep {
            continue;
        }
        ctx.charge_entities(1, "read Inventor flat-pattern body")?;
        let body = match crate::kernel::select_segment_carrier(segment) {
            Ok(carrier) => match &carrier.header {
                Ok(header) => crate::kernel::decode_kernel_carrier(ctx, &carrier, header)
                    .map_err(crate::issue_detail),
                Err(detail) => Err(Ok(detail.clone())),
            },
            Err(detail) => Err(Ok(detail)),
        };
        bodies.push(match body {
            Ok(body) => Ok(body),
            Err(detail) => Err(detail?),
        });
    }
    Ok(bodies)
}
