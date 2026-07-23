//! Error handling for DATA Network precompiles.

use alloy_primitives::Bytes;
use revm::precompile::{PrecompileError, PrecompileOutput, PrecompileResult};

/// Errors produced while executing a DATA Network precompile.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DataNetworkPrecompileError {
    /// Gas limit exceeded during precompile execution.
    #[error("gas limit exceeded")]
    OutOfGas,

    /// The caller is not authorized to perform the requested operation.
    #[error("{0}")]
    Unauthorized(&'static str),

    /// The call is validly formed but cannot be executed with the supplied values.
    #[error("{0}")]
    Revert(&'static str),

    /// An unrecoverable internal error occurred.
    #[error("fatal precompile error: {0}")]
    Fatal(String),
}

/// Result type used by DATA Network precompile operations.
pub type Result<T> = std::result::Result<T, DataNetworkPrecompileError>;

impl From<DataNetworkPrecompileError> for PrecompileError {
    fn from(value: DataNetworkPrecompileError) -> Self {
        match value {
            DataNetworkPrecompileError::OutOfGas => Self::OutOfGas,
            DataNetworkPrecompileError::Fatal(message) => Self::Fatal(message),
            error @ (DataNetworkPrecompileError::Unauthorized(_)
            | DataNetworkPrecompileError::Revert(_)) => Self::Other(error.to_string().into()),
        }
    }
}

/// Converts a DATA Network precompile result into REVM's precompile result.
pub(crate) trait IntoPrecompileResult<T> {
    fn into_precompile_result(self, encode_ok: impl FnOnce(T) -> Bytes) -> PrecompileResult;
}

impl<T> IntoPrecompileResult<T> for Result<T> {
    fn into_precompile_result(self, encode_ok: impl FnOnce(T) -> Bytes) -> PrecompileResult {
        match self {
            Ok(value) => Ok(PrecompileOutput::new(0, encode_ok(value))),
            Err(error) => Err(error.into()),
        }
    }
}
