use std::fmt::Display;

use alloy_primitives::{Address, U256};
use revm::{
    context::{ContextTr, JournalTr},
    handler::ContextTrDbError,
};

use crate::{
    error::{DataNetworkPrecompileError, Result},
    storage::PrecompileStorageProvider,
};

pub struct EvmPrecompileStorageProvider<'a, CTX> {
    context: &'a mut CTX,
    is_static: bool,
}

impl<'a, CTX> EvmPrecompileStorageProvider<'a, CTX> {
    pub fn new(context: &'a mut CTX, is_static: bool) -> Self {
        Self { context, is_static }
    }
}

impl<CTX> PrecompileStorageProvider for EvmPrecompileStorageProvider<'_, CTX>
where
    CTX: ContextTr,
    ContextTrDbError<CTX>: Display,
{
    fn sload(&mut self, address: Address, key: U256) -> Result<U256> {
        let journal = self.context.journal_mut();
        journal
            .load_account(address)
            .map_err(|error| DataNetworkPrecompileError::Fatal(error.to_string()))?;
        journal
            .sload(address, key)
            .map(|loaded| loaded.data)
            .map_err(|error| DataNetworkPrecompileError::Fatal(error.to_string()))
    }

    fn sstore(&mut self, address: Address, key: U256, value: U256) -> Result<()> {
        if self.is_static {
            return Err(DataNetworkPrecompileError::Revert(
                "state modification during static call",
            ));
        }

        let journal = self.context.journal_mut();
        journal
            .load_account(address)
            .map_err(|error| DataNetworkPrecompileError::Fatal(error.to_string()))?;
        journal
            .sstore(address, key, value)
            .map(|_| ())
            .map_err(|error| DataNetworkPrecompileError::Fatal(error.to_string()))
    }

    fn is_static(&self) -> bool {
        self.is_static
    }
}
