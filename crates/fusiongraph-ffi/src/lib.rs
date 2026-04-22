//! `FusionGraph` FFI - Arrow C Data Interface bindings.
//!
//! Provides zero-copy data transfer between `FusionGraph` and external systems
//! using the Arrow C Data Interface.

#![warn(missing_docs)]
#![warn(clippy::all)]
#![allow(unsafe_code)] // FFI requires unsafe

use std::ffi::c_char;
use std::sync::Arc;

use arrow::array::{make_array, Array, RecordBatch, StructArray};
use arrow::datatypes::{Field, Schema};
use arrow::ffi::{from_ffi, to_ffi, FFI_ArrowArray, FFI_ArrowSchema};
use thiserror::Error;

/// FFI error types.
#[derive(Error, Debug)]
pub enum FfiError {
    /// Arrow error during FFI operation.
    #[error("FG-FFI-E001: Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Null pointer passed to an FFI function.
    #[error("FG-FFI-E002: Null pointer passed to {function}")]
    NullPointer {
        /// Function name.
        function: &'static str,
    },

    /// Invalid schema.
    #[error("FG-FFI-E001: Invalid Arrow schema")]
    InvalidSchema,
}

/// Result type for FFI operations.
pub type Result<T> = std::result::Result<T, FfiError>;

/// Imports a `RecordBatch` from Arrow C Data Interface structs.
///
/// # Safety
///
/// The caller must ensure that:
/// - `array` and `schema` are valid Arrow C Data Interface structs
/// - Ownership is transferred to this function (structs will be consumed)
///
/// # Errors
///
/// Returns [`FfiError::Arrow`] when Arrow fails to import the FFI data and
/// [`FfiError::InvalidSchema`] when the imported payload is not a struct-backed
/// record batch.
pub unsafe fn import_record_batch(
    array: FFI_ArrowArray,
    schema: &FFI_ArrowSchema,
) -> Result<RecordBatch> {
    // Import using Arrow's FFI - returns ArrayData
    let array_data = from_ffi(array, schema)?;

    // Convert ArrayData to Array
    let array = make_array(array_data);

    // The imported data should be a struct array for RecordBatch
    let struct_array = array
        .as_any()
        .downcast_ref::<StructArray>()
        .ok_or(FfiError::InvalidSchema)?;

    // Build schema from struct fields
    let fields: Vec<Field> = struct_array
        .fields()
        .iter()
        .map(|f| f.as_ref().clone())
        .collect();
    let schema = Arc::new(Schema::new(fields));

    // Build RecordBatch
    let batch = RecordBatch::try_new(schema, struct_array.columns().to_vec())?;

    Ok(batch)
}

/// Exports a `RecordBatch` to Arrow C Data Interface format.
///
/// Returns the array and schema as FFI structs. The caller is responsible
/// for eventually releasing the memory.
///
/// # Errors
///
/// Returns [`FfiError::Arrow`] when Arrow fails to materialize the FFI view for
/// the provided batch.
pub fn export_record_batch(batch: &RecordBatch) -> Result<(FFI_ArrowArray, FFI_ArrowSchema)> {
    let struct_array = StructArray::from(batch.clone());
    let data = struct_array.to_data();
    let (array, schema) = to_ffi(&data)?;
    Ok((array, schema))
}

/// C-compatible result structure for graph operations.
#[repr(C)]
pub struct FusionGraphResult {
    /// Arrow array containing result data.
    pub array: FFI_ArrowArray,
    /// Arrow schema for the result.
    pub schema: FFI_ArrowSchema,
    /// Error message (null if success).
    pub error: *const c_char,
    /// Statistics about the operation.
    pub stats: FusionGraphStats,
}

/// C-compatible statistics structure.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FusionGraphStats {
    /// Number of nodes visited.
    pub nodes_visited: u64,
    /// Number of edges traversed.
    pub edges_traversed: u64,
    /// Execution time in microseconds.
    pub execution_time_us: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Int64Array, StringArray};
    use arrow::datatypes::DataType;

    fn create_test_batch() -> RecordBatch {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, false),
        ]);

        let ids = Int64Array::from(vec![1, 2, 3]);
        let names = StringArray::from(vec!["a", "b", "c"]);

        RecordBatch::try_new(Arc::new(schema), vec![Arc::new(ids), Arc::new(names)]).unwrap()
    }

    #[test]
    fn export_succeeds() {
        let batch = create_test_batch();
        let result = export_record_batch(&batch);
        assert!(result.is_ok());
    }

    #[test]
    fn export_import_roundtrip() {
        let original = create_test_batch();
        let (array, schema) = export_record_batch(&original).unwrap();

        let imported = unsafe { import_record_batch(array, &schema) }.unwrap();

        assert_eq!(original.num_rows(), imported.num_rows());
        assert_eq!(original.num_columns(), imported.num_columns());
    }
}
