use std::cell::RefCell;

use alloy_primitives::{Address, U256};
use scoped_tls::scoped_thread_local;

use crate::{
    error::{DataNetworkPrecompileError, Result},
    storage::PrecompileStorageProvider,
};

scoped_thread_local!(static STORAGE: RefCell<&mut dyn PrecompileStorageProvider>);

/// Thread-local storage accessor that exposes the active precompile storage provider.
#[derive(Debug, Default, Clone, Copy)]
pub struct StorageCtx;

impl StorageCtx {
    /// Enters a storage context for the duration of the supplied closure.
    pub fn enter<S, R>(storage: &mut S, f: impl FnOnce() -> R) -> R
    where
        S: PrecompileStorageProvider,
    {
        let storage: &mut dyn PrecompileStorageProvider = storage;
        let storage_static: &mut (dyn PrecompileStorageProvider + 'static) =
            unsafe { std::mem::transmute(storage) };
        let cell = RefCell::new(storage_static);
        STORAGE.set(&cell, f)
    }

    /// Executes an infallible function with the active storage provider.
    fn with_storage<F, R>(f: F) -> R
    where
        F: FnOnce(&mut dyn PrecompileStorageProvider) -> R,
    {
        assert!(
            STORAGE.is_set(),
            "No storage context. 'StorageCtx::enter' must be called first"
        );
        STORAGE.with(|cell| {
            let mut guard = cell.borrow_mut();
            f(&mut **guard)
        })
    }

    /// Executes a fallible function with the active storage provider.
    fn try_with_storage<F, R>(f: F) -> Result<R>
    where
        F: FnOnce(&mut dyn PrecompileStorageProvider) -> Result<R>,
    {
        if !STORAGE.is_set() {
            return Err(DataNetworkPrecompileError::Fatal(
                "No storage context. 'StorageCtx::enter' must be called first".to_string(),
            ));
        }
        STORAGE.with(|cell| {
            // Holding the guard prevents re-entrant mutable borrows.
            let mut guard = cell.borrow_mut();
            f(&mut **guard)
        })
    }

    /// Performs an SLOAD operation.
    pub fn sload(&self, address: Address, key: U256) -> Result<U256> {
        Self::try_with_storage(|storage| storage.sload(address, key))
    }

    /// Performs an SSTORE operation.
    pub fn sstore(&mut self, address: Address, key: U256, value: U256) -> Result<()> {
        Self::try_with_storage(|storage| storage.sstore(address, key, value))
    }

    /// Returns whether the current call context is static.
    pub fn is_static(&self) -> bool {
        Self::with_storage(|storage| storage.is_static())
    }
}
