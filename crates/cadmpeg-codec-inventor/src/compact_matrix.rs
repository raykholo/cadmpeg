// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

/// A finite matrix whose implicit cells agree with its compact encoding masks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CompactMatrixWire", into = "CompactMatrixWire")]
pub(crate) struct CompactMatrix {
    value_mask: u16,
    zero_mask: u16,
    matrix: [[FiniteReal; 4]; 4],
}

#[derive(Serialize, Deserialize)]
struct CompactMatrixWire {
    value_mask: u16,
    zero_mask: u16,
    matrix: [[f64; 4]; 4],
}

impl CompactMatrix {
    pub(crate) fn masks(&self) -> (u16, u16) {
        (self.value_mask, self.zero_mask)
    }

    /// Constructs the matrix from masks and row-major explicit values.
    pub(crate) fn try_new(
        value_mask: u16,
        zero_mask: u16,
        mut explicit: impl FnMut(usize) -> Result<FiniteReal, CodecError>,
    ) -> Result<Self, CodecError> {
        let mut matrix = [[FiniteReal::ZERO; 4]; 4];
        for (index, value) in matrix.iter_mut().flatten().enumerate() {
            let bit = 1u16 << index;
            *value = match (value_mask & bit != 0, zero_mask & bit != 0) {
                (false, false) => explicit(index)?,
                (true, false) => FiniteReal::ONE,
                (false, true) => FiniteReal::ZERO,
                (true, true) => FiniteReal::ONE.negated(),
            };
        }
        Ok(Self {
            value_mask,
            zero_mask,
            matrix,
        })
    }

    /// Admits finite rows that agree with the compact encoding masks.
    pub(crate) fn try_from_rows(
        value_mask: u16,
        zero_mask: u16,
        matrix: [[f64; 4]; 4],
    ) -> Result<Self, CodecError> {
        let expected = Self::try_new(value_mask, zero_mask, |index| {
            FiniteReal::new(matrix[index / 4][index % 4]).ok_or_else(|| {
                CodecError::malformed(format_args!("compact matrix[{index}] is not finite"))
            })
        })?;
        if expected.rows() != matrix {
            return Err(CodecError::Malformed(
                "compact matrix disagrees with value_mask or zero_mask".into(),
            ));
        }
        Ok(expected)
    }

    /// The admitted matrix rows.
    pub(crate) fn rows(&self) -> [[f64; 4]; 4] {
        self.matrix.map(|row| row.map(FiniteReal::get))
    }

    /// The admitted matrix cells for transfer into a checked transform.
    pub(crate) fn checked_rows(&self) -> [[FiniteReal; 4]; 4] {
        self.matrix
    }
}

impl TryFrom<CompactMatrixWire> for CompactMatrix {
    type Error = CodecError;

    fn try_from(wire: CompactMatrixWire) -> Result<Self, Self::Error> {
        Self::try_from_rows(wire.value_mask, wire.zero_mask, wire.matrix)
    }
}

impl From<CompactMatrix> for CompactMatrixWire {
    fn from(value: CompactMatrix) -> Self {
        Self {
            value_mask: value.value_mask,
            zero_mask: value.zero_mask,
            matrix: value.rows(),
        }
    }
}

/// A finite 3-by-3 planar matrix whose implicit cells agree with its compact
/// encoding masks. Mask bits 0 through 8 select the row-major cells.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CompactMatrix3Wire", into = "CompactMatrix3Wire")]
pub(crate) struct CompactMatrix3 {
    value_mask: u16,
    zero_mask: u16,
    matrix: [[FiniteReal; 3]; 3],
}

#[derive(Serialize, Deserialize)]
struct CompactMatrix3Wire {
    value_mask: u16,
    zero_mask: u16,
    matrix: [[f64; 3]; 3],
}

/// Mask bits that name no cell of a 3-by-3 matrix.
const MATRIX3_UNUSED_BITS: u16 = !0x01ff;

impl CompactMatrix3 {
    /// Constructs the matrix from masks and row-major explicit values.
    pub(crate) fn try_new(
        value_mask: u16,
        zero_mask: u16,
        mut explicit: impl FnMut(usize) -> Result<FiniteReal, CodecError>,
    ) -> Result<Self, CodecError> {
        if (value_mask | zero_mask) & MATRIX3_UNUSED_BITS != 0 {
            return Err(CodecError::Malformed(
                "compact 3-by-3 matrix mask names a cell outside the matrix".into(),
            ));
        }
        let mut matrix = [[FiniteReal::ZERO; 3]; 3];
        for (index, value) in matrix.iter_mut().flatten().enumerate() {
            let bit = 1u16 << index;
            *value = match (value_mask & bit != 0, zero_mask & bit != 0) {
                (false, false) => explicit(index)?,
                (true, false) => FiniteReal::ONE,
                (false, true) => FiniteReal::ZERO,
                (true, true) => FiniteReal::ONE.negated(),
            };
        }
        Ok(Self {
            value_mask,
            zero_mask,
            matrix,
        })
    }

    /// Admits finite rows that agree with the compact encoding masks.
    pub(crate) fn try_from_rows(
        value_mask: u16,
        zero_mask: u16,
        matrix: [[f64; 3]; 3],
    ) -> Result<Self, CodecError> {
        let expected = Self::try_new(value_mask, zero_mask, |index| {
            FiniteReal::new(matrix[index / 3][index % 3]).ok_or_else(|| {
                CodecError::malformed(format_args!("compact matrix[{index}] is not finite"))
            })
        })?;
        if expected.rows() != matrix {
            return Err(CodecError::Malformed(
                "compact matrix disagrees with value_mask or zero_mask".into(),
            ));
        }
        Ok(expected)
    }

    /// The admitted matrix rows.
    pub(crate) fn rows(&self) -> [[f64; 3]; 3] {
        self.matrix.map(|row| row.map(FiniteReal::get))
    }
}

impl TryFrom<CompactMatrix3Wire> for CompactMatrix3 {
    type Error = CodecError;

    fn try_from(wire: CompactMatrix3Wire) -> Result<Self, Self::Error> {
        Self::try_from_rows(wire.value_mask, wire.zero_mask, wire.matrix)
    }
}

impl From<CompactMatrix3> for CompactMatrix3Wire {
    fn from(value: CompactMatrix3) -> Self {
        Self {
            value_mask: value.value_mask,
            zero_mask: value.zero_mask,
            matrix: value.rows(),
        }
    }
}

/// The assembly placement transform wire fields.
pub(crate) mod assembly_wire {
    use super::CompactMatrix;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        transform_encoding: [u16; 2],
        transform: [[f64; 4]; 4],
    }

    /// Serializes an assembly compact matrix.
    pub(crate) fn serialize<S: Serializer>(
        matrix: &CompactMatrix,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Wire {
            transform_encoding: [matrix.value_mask, matrix.zero_mask],
            transform: matrix.rows(),
        }
        .serialize(serializer)
    }

    /// Deserializes an assembly compact matrix.
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<CompactMatrix, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        CompactMatrix::try_from_rows(
            wire.transform_encoding[0],
            wire.transform_encoding[1],
            wire.transform,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{CompactMatrix, CompactMatrix3};

    #[test]
    fn planar_masks_select_identity_cells_and_translation() {
        let mut values = [0.5, -2.0].into_iter();
        let matrix = CompactMatrix3::try_new(0x0111, 0x00ca, |_| {
            Ok(
                cadmpeg_ir::scalar::FiniteReal::new(values.next().unwrap_or(0.0))
                    .expect("finite matrix cell"),
            )
        })
        .expect("planar matrix fixture agrees with its masks");
        assert_eq!(
            matrix.rows(),
            [[1.0, 0.0, 0.5], [0.0, 1.0, -2.0], [0.0, 0.0, 1.0]]
        );
        let wire = serde_json::to_value(matrix).expect("planar matrix wire");
        assert_eq!(
            serde_json::from_value::<CompactMatrix3>(wire).expect("planar matrix round trip"),
            matrix
        );
        assert!(
            CompactMatrix3::try_new(0x0200, 0, |_| Ok(cadmpeg_ir::scalar::FiniteReal::ZERO))
                .is_err()
        );
    }

    #[test]
    fn masks_select_explicit_positive_zero_and_negative_cells() {
        let mut indices = Vec::new();
        let matrix = CompactMatrix::try_new(0x000a, 0xfffc, |index| {
            indices.push(index);
            Ok(cadmpeg_ir::scalar::FiniteReal::new(2.5).expect("finite matrix cell"))
        })
        .expect("matrix fixture agrees with its masks");
        assert_eq!(indices, [0]);
        assert_eq!(matrix.rows()[0], [2.5, 1.0, 0.0, -1.0]);
        let wire = serde_json::to_value(matrix).expect("matrix fixture agrees with its masks");
        assert_eq!(
            serde_json::from_value::<CompactMatrix>(wire.clone())
                .expect("matrix fixture agrees with its masks"),
            matrix
        );
        for column in 1..4 {
            let mut invalid = wire.clone();
            invalid["matrix"][0][column] = serde_json::json!(3.0);
            assert!(serde_json::from_value::<CompactMatrix>(invalid).is_err());
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(CompactMatrix::try_from_rows(0, 0, [[value; 4]; 4]).is_err());
            let mut rows = matrix.rows();
            rows[0][1] = value;
            assert!(CompactMatrix::try_from_rows(0x000a, 0xfffc, rows).is_err());
        }
    }
}
