use std::collections::HashMap;

use alloy_primitives::{Address, U256};

use crate::{
    error::{DataNetworkPrecompileError, Result},
    storage::PrecompileStorageProvider,
};

/// In-memory [`PrecompileStorageProvider`] for tests and isolated execution.
#[derive(Debug, Default)]
pub struct HashMapStorageProvider {
    internals: HashMap<(Address, U256), U256>,
    fail_on_sload: Option<(Address, U256)>,
    is_static: bool,
    counter_sload: u64,
    counter_sstore: u64,
}

impl HashMapStorageProvider {
    /// Creates an empty provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns this provider with the static-call flag set.
    pub fn with_static(mut self, is_static: bool) -> Self {
        self.is_static = is_static;
        self
    }
}

impl PrecompileStorageProvider for HashMapStorageProvider {
    fn sload(&mut self, address: Address, key: U256) -> Result<U256> {
        if self.fail_on_sload == Some((address, key)) {
            return Err(DataNetworkPrecompileError::Fatal(
                "injected sload failure".into(),
            ));
        }

        self.counter_sload += 1;
        Ok(self
            .internals
            .get(&(address, key))
            .copied()
            .unwrap_or(U256::ZERO))
    }

    fn sstore(&mut self, address: Address, key: U256, value: U256) -> Result<()> {
        self.counter_sstore += 1;
        self.internals.insert((address, key), value);
        Ok(())
    }

    fn is_static(&self) -> bool {
        self.is_static
    }
}

impl HashMapStorageProvider {
    /// Makes the next load at `address` and `slot` fail.
    pub fn fail_next_sload_at(&mut self, address: Address, slot: U256) {
        self.fail_on_sload = Some((address, slot));
    }

    /// Returns the number of SLOAD operations.
    pub fn counter_sload(&self) -> u64 {
        self.counter_sload
    }

    /// Returns the number of SSTORE operations.
    pub fn counter_sstore(&self) -> u64 {
        self.counter_sstore
    }

    /// Resets the storage-operation counters.
    pub fn reset_counters(&mut self) {
        self.counter_sload = 0;
        self.counter_sstore = 0;
    }

    /// Returns all storage entries as `(address, slot, value)` tuples.
    pub fn into_storage(self) -> impl Iterator<Item = (Address, U256, U256)> {
        self.internals
            .into_iter()
            .map(|((address, slot), value)| (address, slot, value))
    }
}
