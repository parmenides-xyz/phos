use alloy_primitives::{Bytes, U256};
use alloy_sol_types::{sol, SolCall, SolInterface};
use revm::interpreter::CallScheme;
use revm::precompile::{PrecompileError, PrecompileResult};

use super::IpGraph;
use crate::{
    error::{DataNetworkPrecompileError, IntoPrecompileResult, Result},
    storage::StorageCtx,
    Precompile,
};

const IP_GRAPH_WRITE_GAS: u64 = 100;
const IP_GRAPH_READ_GAS: u64 = 10;
const AVERAGE_ANCESTOR_IP_COUNT: u64 = 30;
const AVERAGE_PARENT_IP_COUNT: u64 = 4;
const INTRINSIC_GAS: u64 = 1_000;
const IP_GRAPH_EXTERNAL_READ_GAS: u64 = 2_100;

sol! {
    interface IIpGraph {
        function addParentIp(address ipId, address[] parentIpIds) external;
        function hasParentIp(address ipId, address parentIpId) external view returns (bool);
        function getParentIps(address ipId) external view returns (address[] memory);
        function getParentIpsCount(address ipId) external view returns (uint256);
        function getAncestorIps(address ipId) external view returns (address[] memory);
        function getAncestorIpsCount(address ipId) external view returns (uint256);
        function hasAncestorIp(address ipId, address ancestorIpId) external view returns (bool);
        function setRoyalty(
            address ipId,
            address parentIpId,
            uint256 royaltyPolicyKind,
            uint256 royalty
        ) external;
        function getRoyalty(
            address ipId,
            address ancestorIpId,
            uint256 royaltyPolicyKind
        ) external view returns (uint256);
        function getRoyaltyStack(
            address ipId,
            uint256 royaltyPolicyKind
        ) external view returns (uint256);
        function hasParentIpExt(
            address ipId,
            address parentIpId
        ) external view returns (bool);
        function getParentIpsExt(address ipId) external view returns (address[] memory);
        function getParentIpsCountExt(address ipId) external view returns (uint256);
        function getAncestorIpsExt(address ipId) external view returns (address[] memory);
        function getAncestorIpsCountExt(address ipId) external view returns (uint256);
        function hasAncestorIpExt(
            address ipId,
            address ancestorIpId
        ) external view returns (bool);
        function getRoyaltyExt(
            address ipId,
            address ancestorIpId,
            uint256 royaltyPolicyKind
        ) external view returns (uint256);
        function getRoyaltyStackExt(
            address ipId,
            uint256 royaltyPolicyKind
        ) external view returns (uint256);
    }
}

use IIpGraph::IIpGraphCalls;

impl Precompile for IpGraph {
    fn call(
        &mut self,
        calldata: &[u8],
        msg_sender: alloy_primitives::Address,
        call_scheme: CallScheme,
    ) -> PrecompileResult {
        dispatch_call(calldata, IIpGraphCalls::abi_decode, |call| match call {
            IIpGraphCalls::addParentIp(call) => {
                mutate_void(call, msg_sender, call_scheme, |sender, call| {
                    self.add_parent_ip(sender, call.ipId, call.parentIpIds)
                })
            }
            IIpGraphCalls::hasParentIp(call) => view(call, call_scheme, |call| {
                self.has_parent_ip(msg_sender, call.ipId, call.parentIpId)
            }),
            IIpGraphCalls::getParentIps(call) => view(call, call_scheme, |call| {
                self.get_parent_ips(msg_sender, call.ipId)
            }),
            IIpGraphCalls::getParentIpsCount(call) => view(call, call_scheme, |call| {
                self.get_parent_ips_count(msg_sender, call.ipId)
            }),
            IIpGraphCalls::getAncestorIps(call) => view(call, call_scheme, |call| {
                self.get_ancestor_ips(msg_sender, call.ipId)
            }),
            IIpGraphCalls::getAncestorIpsCount(call) => view(call, call_scheme, |call| {
                self.get_ancestor_ips_count(msg_sender, call.ipId)
            }),
            IIpGraphCalls::hasAncestorIp(call) => view(call, call_scheme, |call| {
                self.has_ancestor_ip(msg_sender, call.ipId, call.ancestorIpId)
            }),
            IIpGraphCalls::setRoyalty(call) => {
                mutate_void(call, msg_sender, call_scheme, |sender, call| {
                    self.set_royalty(
                        sender,
                        call.ipId,
                        call.parentIpId,
                        call.royaltyPolicyKind,
                        call.royalty,
                    )
                })
            }
            IIpGraphCalls::getRoyalty(call) => view(call, call_scheme, |call| {
                self.get_royalty(
                    msg_sender,
                    call.ipId,
                    call.ancestorIpId,
                    call.royaltyPolicyKind,
                )
            }),
            IIpGraphCalls::getRoyaltyStack(call) => view(call, call_scheme, |call| {
                self.get_royalty_stack(msg_sender, call.ipId, call.royaltyPolicyKind)
            }),
            IIpGraphCalls::hasParentIpExt(call) => view(call, call_scheme, |call| {
                self.has_parent_ip(msg_sender, call.ipId, call.parentIpId)
            }),
            IIpGraphCalls::getParentIpsExt(call) => view(call, call_scheme, |call| {
                self.get_parent_ips(msg_sender, call.ipId)
            }),
            IIpGraphCalls::getParentIpsCountExt(call) => view(call, call_scheme, |call| {
                self.get_parent_ips_count(msg_sender, call.ipId)
            }),
            IIpGraphCalls::getAncestorIpsExt(call) => view(call, call_scheme, |call| {
                self.get_ancestor_ips(msg_sender, call.ipId)
            }),
            IIpGraphCalls::getAncestorIpsCountExt(call) => view(call, call_scheme, |call| {
                self.get_ancestor_ips_count(msg_sender, call.ipId)
            }),
            IIpGraphCalls::hasAncestorIpExt(call) => view(call, call_scheme, |call| {
                self.has_ancestor_ip(msg_sender, call.ipId, call.ancestorIpId)
            }),
            IIpGraphCalls::getRoyaltyExt(call) => view(call, call_scheme, |call| {
                self.get_royalty(
                    msg_sender,
                    call.ipId,
                    call.ancestorIpId,
                    call.royaltyPolicyKind,
                )
            }),
            IIpGraphCalls::getRoyaltyStackExt(call) => view(call, call_scheme, |call| {
                self.get_royalty_stack(msg_sender, call.ipId, call.royaltyPolicyKind)
            }),
        })
    }
}

#[inline]
fn view<T: SolCall>(
    call: T,
    call_scheme: CallScheme,
    f: impl FnOnce(T) -> Result<T::Return>,
) -> PrecompileResult {
    if call_scheme == CallScheme::DelegateCall {
        return Err(DataNetworkPrecompileError::Revert(
            "DELEGATECALL is not allowed for IP Graph reads",
        )
        .into());
    }

    f(call).into_precompile_result(|value| T::abi_encode_returns(&value).into())
}

#[inline]
fn mutate_void<T: SolCall>(
    call: T,
    sender: alloy_primitives::Address,
    call_scheme: CallScheme,
    f: impl FnOnce(alloy_primitives::Address, T) -> Result<()>,
) -> PrecompileResult {
    if call_scheme != CallScheme::Call {
        return Err(DataNetworkPrecompileError::Revert("IP Graph writes require CALL").into());
    }

    if StorageCtx.is_static() {
        return Err(
            DataNetworkPrecompileError::Revert("state modification during static call").into(),
        );
    }

    f(sender, call).into_precompile_result(|()| Bytes::new())
}

#[inline]
fn dispatch_call<T: SolInterface>(
    calldata: &[u8],
    decode: impl FnOnce(&[u8]) -> core::result::Result<T, alloy_sol_types::Error>,
    f: impl FnOnce(T) -> PrecompileResult,
) -> PrecompileResult {
    if calldata.len() < 4 {
        return Err(PrecompileError::Other("input too short".into()));
    }

    match decode(calldata) {
        Ok(call) if calldata.len() == call.abi_encoded_size().saturating_add(4) => f(call),
        Ok(_) => Err(PrecompileError::Other("invalid input length".into())),
        Err(alloy_sol_types::Error::UnknownSelector { .. }) => {
            Err(PrecompileError::Other("unknown selector".into()))
        }
        Err(_) => Err(PrecompileError::Other("invalid input".into())),
    }
}

impl IpGraph {
    pub fn required_gas(&self, input: &[u8]) -> u64 {
        if input.len() < 4 {
            return INTRINSIC_GAS;
        }

        let read_word = |data: &[u8], start: usize| {
            let mut word = [0u8; 32];
            let start = start.min(data.len());
            let end = start.saturating_add(32).min(data.len());
            word[..end - start].copy_from_slice(&data[start..end]);
            U256::from_be_bytes(word)
        };

        let selector = &input[..4];

        if selector == IIpGraph::addParentIpCall::SELECTOR {
            let args = &input[4..];
            let parent_count = read_word(args, 64);
            if parent_count > U256::from(1_024) {
                return u64::MAX;
            }
            return INTRINSIC_GAS + IP_GRAPH_WRITE_GAS * parent_count.to::<u64>();
        }

        if selector == IIpGraph::hasParentIpCall::SELECTOR
            || selector == IIpGraph::getParentIpsCall::SELECTOR
        {
            return IP_GRAPH_READ_GAS * AVERAGE_PARENT_IP_COUNT;
        }

        if selector == IIpGraph::getParentIpsCountCall::SELECTOR {
            return IP_GRAPH_READ_GAS;
        }

        if selector == IIpGraph::getAncestorIpsCall::SELECTOR
            || selector == IIpGraph::hasAncestorIpCall::SELECTOR
        {
            return IP_GRAPH_READ_GAS * AVERAGE_ANCESTOR_IP_COUNT * 2;
        }

        if selector == IIpGraph::getAncestorIpsCountCall::SELECTOR {
            return IP_GRAPH_READ_GAS * AVERAGE_PARENT_IP_COUNT * 2;
        }

        if selector == IIpGraph::setRoyaltyCall::SELECTOR {
            return IP_GRAPH_WRITE_GAS;
        }

        if selector == IIpGraph::getRoyaltyCall::SELECTOR {
            let royalty_policy_kind = read_word(input, 64 + 4);
            return match royalty_policy_kind {
                U256::ZERO => IP_GRAPH_READ_GAS * AVERAGE_ANCESTOR_IP_COUNT * 3,
                U256::ONE => IP_GRAPH_READ_GAS * (AVERAGE_ANCESTOR_IP_COUNT * 2 + 2),
                _ => INTRINSIC_GAS,
            };
        }

        if selector == IIpGraph::getRoyaltyStackCall::SELECTOR {
            let royalty_policy_kind = read_word(input, 32 + 4);
            return match royalty_policy_kind {
                U256::ZERO => IP_GRAPH_READ_GAS * (AVERAGE_PARENT_IP_COUNT + 1),
                U256::ONE => IP_GRAPH_READ_GAS * AVERAGE_ANCESTOR_IP_COUNT * 2,
                _ => INTRINSIC_GAS,
            };
        }

        if selector == IIpGraph::hasParentIpExtCall::SELECTOR
            || selector == IIpGraph::getParentIpsExtCall::SELECTOR
        {
            return IP_GRAPH_EXTERNAL_READ_GAS * AVERAGE_PARENT_IP_COUNT;
        }

        if selector == IIpGraph::getParentIpsCountExtCall::SELECTOR {
            return IP_GRAPH_EXTERNAL_READ_GAS;
        }

        if selector == IIpGraph::getAncestorIpsExtCall::SELECTOR
            || selector == IIpGraph::hasAncestorIpExtCall::SELECTOR
        {
            return IP_GRAPH_EXTERNAL_READ_GAS * AVERAGE_ANCESTOR_IP_COUNT * 2;
        }

        if selector == IIpGraph::getAncestorIpsCountExtCall::SELECTOR {
            return IP_GRAPH_EXTERNAL_READ_GAS * AVERAGE_PARENT_IP_COUNT * 2;
        }

        if selector == IIpGraph::getRoyaltyExtCall::SELECTOR {
            let royalty_policy_kind = read_word(input, 64 + 4);
            return match royalty_policy_kind {
                U256::ZERO => IP_GRAPH_EXTERNAL_READ_GAS * AVERAGE_ANCESTOR_IP_COUNT * 3,
                U256::ONE => IP_GRAPH_EXTERNAL_READ_GAS * (AVERAGE_ANCESTOR_IP_COUNT * 2 + 2),
                _ => INTRINSIC_GAS,
            };
        }

        if selector == IIpGraph::getRoyaltyStackExtCall::SELECTOR {
            let royalty_policy_kind = read_word(input, 32 + 4);
            return match royalty_policy_kind {
                U256::ZERO => IP_GRAPH_EXTERNAL_READ_GAS * (AVERAGE_PARENT_IP_COUNT + 1),
                U256::ONE => IP_GRAPH_EXTERNAL_READ_GAS * AVERAGE_ANCESTOR_IP_COUNT * 2,
                _ => INTRINSIC_GAS,
            };
        }

        INTRINSIC_GAS
    }
}
