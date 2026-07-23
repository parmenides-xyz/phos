//! EVM storage abstraction layer for DATA Network precompiles.

pub mod evm;
pub mod hashmap;
pub mod thread_local;

pub use thread_local::StorageCtx;

use alloy_primitives::{Address, U256};

use crate::Result;

/// Low-level storage provider for interacting with EVM state.
pub trait PrecompileStorageProvider {
    /// Performs an SLOAD operation.
    fn sload(&mut self, address: Address, key: U256) -> Result<U256>;

    /// Performs an SSTORE operation.
    fn sstore(&mut self, address: Address, key: U256, value: U256) -> Result<()>;

    /// Returns whether the current call context is static.
    fn is_static(&self) -> bool;
}
