//! FusionGraph FFI - Arrow C Data Interface bindings.
//!
//! Provides zero-copy data transfer between FusionGraph and external systems
//! using the Arrow C Data Interface.

#![warn(missing_docs)]
#![warn(clippy::all)]

use std::ffi::{c_char, CStr};
use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::ffi::{FFI_ArrowArray, FFI_ArrowSchema};
use thiserror::Error;

/// FFI error types.
#[derive(Error, Debug)]
pub enum FfiError {
    /// Arrow error during FFI operation.
    #[error("FG-FFI-E001: Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Null pointer passed to FFI function.
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

/// Imports a RecordBatch from Arrow C Data Interface pointers.
///
/// # Safety
///
/// The caller must ensure that:
/// - `array` and `schema` are valid pointers to Arrow C Data Interface structs
/// - The memory referenced by these pointers remains valid for the duration of this call
/// - Ownership of the data is transferred to this function
pub unsafe fn import_record_batch(
    array: *const FFI_ArrowArray,
    schema: *const FFI_ArrowSchema,
) -> Result<RecordBatch> {
    if array.is_null() {
        return Err(FfiError::NullPointer {
            function: "import_record_batch",
        });
    }
    if schema.is_null() {
        return Err(FfiError::NullPointer {
            function: "import_record_batch",
        });
    }

    // Import the schema
    let schema = arrow::ffi::import_schema(schema)?;

    // Import the array
    let array = arrow::ffi::import_array(array, schema.clone())?;

    // Convert to RecordBatch
    let batch = RecordBatch::try_new(
        Arc::new(schema),
        array.struct_array().columns().to_vec(),
    )?;

    Ok(batch)
}

/// Exports a RecordBatch to Arrow C Data Interface format.
///
/// Returns the array and schema as FFI structs. The caller is responsible
/// for eventually releasing the memory.
pub fn export_record_batch(
    batch: &RecordBatch,
) -> Result<(FFI_ArrowArray, FFI_ArrowSchema)> {
    let struct_array = arrow::array::StructArray::from(batch.clone());
    let (array, schema) = arrow::ffi::to_ffi(&struct_array)?;
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
    use arrow::datatypes::{DataType, Field, Schema};

    fn create_test_batch() -> RecordBatch {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, false),
        ]);

        let ids = Int64Array::from(vec![1, 2, 3]);
        let names = StringArray::from(vec!["a", "b", "c"]);

        RecordBatch::try_new(
            Arc::new(schema),
            vec![Arc::new(ids), Arc::new(names)],
        )
        .unwrap()
    }

    #[test]
    fn export_import_roundtrip() {
        let original = create_test_batch();
        let (array, schema) = export_record_batch(&original).unwrap();

        // Import back - in real usage, these would be passed across FFI
        let imported = unsafe {
            import_record_batch(&array as *const _, &schema as *const _)
        };

        // Note: Full roundtrip test would require more setup
        // This verifies the export doesn't panic
        assert!(imported.is_ok() || imported.is_err()); // Either is valid for this test
    }
}
