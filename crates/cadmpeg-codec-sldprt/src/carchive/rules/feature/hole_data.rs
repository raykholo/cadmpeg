// SPDX-License-Identifier: Apache-2.0
//! Fixed headers in a Hole Wizard catalogue record.

use crate::carchive::reader::{bad, Reader, Res};
use crate::layout::{hole_wizard_catalog_tail as tail, hole_wizard_data_header as header};

const CATALOG_CELL_OFFSET: usize = tail::SENTINEL + std::mem::size_of::<u32>();
const CATALOG_CELL_LEN: usize = tail::LEN - CATALOG_CELL_OFFSET;

#[derive(Debug, PartialEq)]
pub(super) struct Header {
    pub(super) native: [u8; header::DIAMETER],
    pub(super) diameter: f64,
}

pub(super) fn read_header(reader: &mut Reader<'_>) -> Res<Header> {
    let start = reader.p;
    let diameter = reader.pf64(start + header::DIAMETER)?;
    if reader.pu32(start + header::ONE)? != header::ONE_VALUE {
        return Err(bad("hole data header"));
    }
    let mut native = [0; header::DIAMETER];
    for (index, byte) in native.iter_mut().enumerate() {
        *byte = reader.byte(start + index)?;
    }
    reader.skip(header::LEN);
    Ok(Header { native, diameter })
}

pub(super) fn read_catalog_tail(reader: &mut Reader<'_>) -> Res<[u8; CATALOG_CELL_LEN]> {
    let start = reader.p;
    if reader.pu32(start + tail::FIRST_ONE)? != tail::FIRST_ONE_VALUE
        || reader.pu32(start + tail::SECOND_ONE)? != tail::SECOND_ONE_VALUE
        || reader.pu32(start + tail::SENTINEL)? != tail::SENTINEL_VALUE
    {
        return Err(bad("hole catalogue tail"));
    }
    let mut native = [0; CATALOG_CELL_LEN];
    for (index, byte) in native.iter_mut().enumerate() {
        *byte = reader.byte(start + CATALOG_CELL_OFFSET + index)?;
    }
    reader.skip(tail::LEN);
    Ok(native)
}

pub(super) fn finish(reader: &mut Reader<'_>) -> Res<()> {
    while reader.p < reader.len() && !super::objat(reader, reader.p) {
        if reader.byte(reader.p)? != 0 {
            return Err(bad("hole suffix"));
        }
        reader.skip(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
