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

#[cfg(test)]
mod tests {
    use alloy_primitives::{address, hex, B256, U256};
    use revm::{
        database::{CacheDB, EmptyDB},
        interpreter::{CallInput, CallInputs, CallScheme, CallValue},
        Context, MainContext,
    };

    use super::*;

    const CALLER: Address = address!("1000000000000000000000000000000000000001");
    const VALID_P256_INPUT: &str = "4cee90eb86eaa050036147a12d49004b6b9c72bd725d39d4785011fe190f0b4da73bd4903f0ce3b639bbbf6e8e80d16931ff4bcf5993d58468e8fb19086e8cac36dbcd03009df8c59286b162af3bd7fcc0450c9aa81be5d10d312af6c66b1d604aebd3099c618202fcfe16ae7770b0c49ab5eadf74b754204a3bb6060e44eff37618b065f9832de4ca6ca971a7a1adc826d0f7c00181a5fb2ddf79ae00b4e10e";

    fn inputs(calldata: Vec<u8>, gas_limit: u64) -> CallInputs {
        CallInputs {
            input: CallInput::Bytes(calldata.into()),
            return_memory_offset: 0..0,
            gas_limit,
            bytecode_address: P256_VERIFY_ADDRESS,
            known_bytecode: None,
            target_address: P256_VERIFY_ADDRESS,
            caller: CALLER,
            value: CallValue::Transfer(U256::ZERO),
            scheme: CallScheme::Call,
            is_static: false,
        }
    }

    fn assert_registration<CTX>(provider: &DataNetworkPrecompiles, _context: &CTX, expected: bool)
    where
        CTX: ContextTr<Cfg: Cfg<Spec = SpecId>>,
        ContextTrDbError<CTX>: Display,
    {
        assert_eq!(
            <DataNetworkPrecompiles as PrecompileProvider<CTX>>::contains(
                provider,
                &P256_VERIFY_ADDRESS,
            ),
            expected,
        );
        assert_eq!(
            <DataNetworkPrecompiles as PrecompileProvider<CTX>>::warm_addresses(provider)
                .any(|address| address == P256_VERIFY_ADDRESS),
            expected,
        );
        assert_eq!(
            <DataNetworkPrecompiles as PrecompileProvider<CTX>>::contains(
                provider,
                &IP_GRAPH_ADDRESS,
            ),
            expected,
        );
        assert_eq!(
            <DataNetworkPrecompiles as PrecompileProvider<CTX>>::warm_addresses(provider)
                .any(|address| address == IP_GRAPH_ADDRESS),
            expected,
        );
    }

    #[test]
    fn registers_p256_at_data_network_forks() {
        let valid_input = hex::decode(VALID_P256_INPUT).unwrap();

        for (spec, gas_cost) in [
            (SpecId::CANCUN, 3_450),
            (SpecId::PRAGUE, 3_450),
            (SpecId::OSAKA, 6_900),
        ] {
            let mut context = Context::mainnet().with_db(CacheDB::new(EmptyDB::default()));
            let mut provider = DataNetworkPrecompiles {
                inner: EthPrecompiles::new(spec),
            };

            assert_registration(&provider, &context, true);

            let output = provider
                .run(&mut context, &inputs(valid_input.clone(), gas_cost))
                .unwrap()
                .unwrap();
            assert_eq!(output.result, InstructionResult::Return);
            assert_eq!(output.gas.spent(), gas_cost);
            assert_eq!(output.output.as_ref(), B256::with_last_byte(1).as_slice());

            let output = provider
                .run(&mut context, &inputs(valid_input.clone(), gas_cost - 1))
                .unwrap()
                .unwrap();
            assert_eq!(output.result, InstructionResult::PrecompileOOG);
        }

        let mut context = Context::mainnet().with_db(CacheDB::new(EmptyDB::default()));
        let mut provider = DataNetworkPrecompiles {
            inner: EthPrecompiles::new(SpecId::BERLIN),
        };
        assert_registration(&provider, &context, false);
        assert!(provider
            .run(&mut context, &inputs(valid_input, 3_450))
            .unwrap()
            .is_none());
    }
}
