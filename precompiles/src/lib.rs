//! DATA Network precompile implementations.

use std::fmt::Display;

use alloy_primitives::{address, Address, Bytes};
use revm::{
    context::{Cfg, ContextTr, JournalTr, LocalContextTr},
    handler::{ContextTrDbError, EthPrecompiles, PrecompileProvider},
    interpreter::{CallInputs, CallScheme, Gas, InstructionResult, InterpreterResult},
    precompile::{secp256r1, PrecompileError, PrecompileResult},
    primitives::hardfork::SpecId,
};

pub mod error;
pub mod ip_graph;
pub mod storage;

pub use error::{DataNetworkPrecompileError, Result};

use ip_graph::{IpGraph, IP_GRAPH_ADDRESS};
use storage::{evm::EvmPrecompileStorageProvider, StorageCtx};

const P256_VERIFY_ADDRESS: Address = address!("0000000000000000000000000000000000000100");

/// Trait implemented by DATA Network precompile contract types.
pub trait Precompile {
    /// ABI-decodes calldata and dispatches it to the matching precompile method.
    fn call(
        &mut self,
        calldata: &[u8],
        msg_sender: Address,
        call_scheme: CallScheme,
    ) -> PrecompileResult;
}

/// Ethereum precompiles extended with DATA Network's stateful precompiles.
#[derive(Debug, Clone, Default)]
pub struct DataNetworkPrecompiles {
    inner: EthPrecompiles,
}

impl<CTX> PrecompileProvider<CTX> for DataNetworkPrecompiles
where
    CTX: ContextTr<Cfg: Cfg<Spec = SpecId>>,
    ContextTrDbError<CTX>: Display,
{
    type Output = InterpreterResult;

    fn set_spec(&mut self, spec: <CTX::Cfg as Cfg>::Spec) -> bool {
        <EthPrecompiles as PrecompileProvider<CTX>>::set_spec(&mut self.inner, spec)
    }

    fn run(
        &mut self,
        context: &mut CTX,
        inputs: &CallInputs,
    ) -> std::result::Result<Option<Self::Output>, String> {
        let result = if inputs.bytecode_address == P256_VERIFY_ADDRESS
            && self.inner.spec >= SpecId::CANCUN
            && self.inner.spec < SpecId::OSAKA
        {
            let calldata = inputs.input.bytes(context);
            secp256r1::P256VERIFY.execute(&calldata, inputs.gas_limit)
        } else if inputs.bytecode_address == IP_GRAPH_ADDRESS && self.inner.spec >= SpecId::CANCUN {
            let calldata = inputs.input.bytes(context);
            let required_gas = IpGraph::default().required_gas(&calldata);

            if required_gas > inputs.gas_limit {
                Err(PrecompileError::OutOfGas)
            } else {
                let mut storage = EvmPrecompileStorageProvider::new(context, inputs.is_static);
                StorageCtx::enter(&mut storage, || {
                    IpGraph::default().call(&calldata, inputs.caller, inputs.scheme)
                })
                .map(|mut output| {
                    output.gas_used = required_gas;
                    output
                })
            }
        } else {
            return <EthPrecompiles as PrecompileProvider<CTX>>::run(
                &mut self.inner,
                context,
                inputs,
            );
        };

        let mut interpreter_result = InterpreterResult {
            result: InstructionResult::Return,
            gas: Gas::new(inputs.gas_limit),
            output: Bytes::new(),
        };

        match result {
            Ok(output) => {
                interpreter_result.gas.record_refund(output.gas_refunded);
                let recorded = interpreter_result.gas.record_cost(output.gas_used);
                assert!(recorded, "Gas underflow is not possible");
                interpreter_result.result = if output.reverted {
                    InstructionResult::Revert
                } else {
                    InstructionResult::Return
                };
                interpreter_result.output = output.bytes;
            }
            Err(PrecompileError::Fatal(error)) => return Err(error),
            Err(error) => {
                interpreter_result.result = if error.is_oog() {
                    InstructionResult::PrecompileOOG
                } else {
                    InstructionResult::PrecompileError
                };
                if !error.is_oog() && context.journal().depth() == 1 {
                    context
                        .local_mut()
                        .set_precompile_error_context(error.to_string());
                }
            }
        }

        Ok(Some(interpreter_result))
    }

    fn warm_addresses(&self) -> Box<impl Iterator<Item = Address>> {
        let p256_verify = (self.inner.spec >= SpecId::CANCUN && self.inner.spec < SpecId::OSAKA)
            .then_some(P256_VERIFY_ADDRESS)
            .into_iter();
        let ip_graph = (self.inner.spec >= SpecId::CANCUN)
            .then_some(IP_GRAPH_ADDRESS)
            .into_iter();

        Box::new(
            self.inner
                .warm_addresses()
                .chain(p256_verify)
                .chain(ip_graph),
        )
    }

    fn contains(&self, address: &Address) -> bool {
        (*address == IP_GRAPH_ADDRESS && self.inner.spec >= SpecId::CANCUN)
            || (*address == P256_VERIFY_ADDRESS
                && self.inner.spec >= SpecId::CANCUN
                && self.inner.spec < SpecId::OSAKA)
            || self.inner.contains(address)
    }
}
