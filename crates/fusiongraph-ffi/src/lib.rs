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

    /// Null pointer passed to FFI function.
    #[error("FG-FFI-E002: Null pointer passed to {function}")]
    NullPointer {
        /// Function name.
        function: &'static str,
    },

    /// Invalid schema.
    #[error("FG-FFI-E003: Invalid Arrow schema")]
    InvalidSchema,
}

/// Result type for FFI operations.
pub type Result<T> = std::result::Result<T, FfiError>;

/// Imports a `RecordBatch` from Arrow C Data Interface structs.
///
/// # Errors
///
/// Returns an error when Arrow FFI import fails, the imported value is not a
/// struct array, or the struct columns cannot be assembled into a record batch.
///
/// # Safety
///
/// The caller must ensure that:
/// - `array` and `schema` are valid Arrow C Data Interface structs
/// - Ownership of `array` is transferred to this function and consumed
/// - `schema` remains valid for the duration of this call
/// - The caller retains ownership of `schema` and remains responsible for
///   releasing it according to the Arrow C Data Interface contract
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
/// Returns an error when Arrow cannot export the record batch data through the
/// C Data Interface.
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

/// Tests for FFI memory safety.
///
/// # Running with Miri
///
/// To verify memory safety, run tests with Miri:
/// ```bash
/// cargo +nightly miri test -p fusiongraph-ffi
/// ```
///
/// Miri will detect:
/// - Use-after-free errors
/// - Memory leaks
/// - Invalid pointer access
/// - Undefined behavior in unsafe code
#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Float64Array, Int64Array, StringArray, UInt64Array};
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

    fn create_edge_batch() -> RecordBatch {
        let schema = Schema::new(vec![
            Field::new("source", DataType::UInt64, false),
            Field::new("target", DataType::UInt64, false),
            Field::new("weight", DataType::Float64, true),
        ]);

        let sources = UInt64Array::from(vec![0, 0, 1, 2, 3]);
        let targets = UInt64Array::from(vec![1, 2, 2, 3, 4]);
        let weights = Float64Array::from(vec![1.0, 2.0, 1.5, 0.5, 3.0]);

        RecordBatch::try_new(
            Arc::new(schema),
            vec![Arc::new(sources), Arc::new(targets), Arc::new(weights)],
        )
        .unwrap()
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

    #[test]
    fn roundtrip_edge_batch() {
        let original = create_edge_batch();
        let (array, schema) = export_record_batch(&original).unwrap();

        let imported = unsafe { import_record_batch(array, &schema) }.unwrap();

        assert_eq!(original.num_rows(), imported.num_rows());
        assert_eq!(original.num_columns(), imported.num_columns());

        // Verify data integrity
        let orig_sources = original
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let imp_sources = imported
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();

        for i in 0..original.num_rows() {
            assert_eq!(orig_sources.value(i), imp_sources.value(i));
        }
    }

    #[test]
    fn roundtrip_empty_batch() {
        let schema = Schema::new(vec![Field::new("id", DataType::Int64, false)]);
        let ids = Int64Array::from(Vec::<i64>::new());
        let original = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(ids)]).unwrap();

        let (array, schema) = export_record_batch(&original).unwrap();
        let imported = unsafe { import_record_batch(array, &schema) }.unwrap();

        assert_eq!(imported.num_rows(), 0);
        assert_eq!(imported.num_columns(), 1);
    }

    #[test]
    fn roundtrip_large_batch() {
        let n: usize = 10_000;
        let schema = Schema::new(vec![
            Field::new("id", DataType::UInt64, false),
            Field::new("value", DataType::Float64, false),
        ]);

        let ids: Vec<u64> = (0..n as u64).collect();
        let values: Vec<f64> = (0..n).map(|i| i as f64 * 0.1).collect();

        let original = RecordBatch::try_new(
            Arc::new(schema),
            vec![
                Arc::new(UInt64Array::from(ids)),
                Arc::new(Float64Array::from(values)),
            ],
        )
        .unwrap();

        let (array, schema) = export_record_batch(&original).unwrap();
        let imported = unsafe { import_record_batch(array, &schema) }.unwrap();

        assert_eq!(imported.num_rows(), n);

        // Spot-check some values
        let imp_ids = imported
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert_eq!(imp_ids.value(0), 0);
        assert_eq!(imp_ids.value(n - 1), (n - 1) as u64);
    }

    #[test]
    fn multiple_roundtrips() {
        let original = create_test_batch();

        for _ in 0..5 {
            let (array, schema) = export_record_batch(&original).unwrap();
            let _imported = unsafe { import_record_batch(array, &schema) }.unwrap();
        }
    }

    #[test]
    fn stats_default() {
        let stats = FusionGraphStats::default();
        assert_eq!(stats.nodes_visited, 0);
        assert_eq!(stats.edges_traversed, 0);
        assert_eq!(stats.execution_time_us, 0);
    }
}
