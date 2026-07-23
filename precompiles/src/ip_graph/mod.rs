//! DATA Network IP Graph precompile.

pub mod dispatch;

use std::collections::{HashMap, HashSet};

use alloy_primitives::{address, b256, keccak256, Address, Keccak256, B256, U256};
use alloy_sol_types::SolValue;
use num_bigint::BigUint;

use crate::{storage::StorageCtx, DataNetworkPrecompileError, Result};

pub const IP_GRAPH_ADDRESS: Address = address!("0000000000000000000000000000000000000101");

const ACL_ADDRESS: Address = address!("1640A22a8A086747cD377b73954545e2Dfcc9Cad");
const ACL_SLOT: B256 = b256!("af99b37fdaacca72ee7240cb1435cc9e498aee6ef4edc19c8cc0cd787f4e6800");
const HUNDRED_PERCENT: u64 = 100_000_000;

#[derive(Debug, Default)]
pub struct IpGraph {
    pub(crate) storage: StorageCtx,
}

impl IpGraph {
    fn is_allowed(&self, caller: Address) -> Result<bool> {
        let key = U256::from_be_bytes(keccak256((caller, ACL_SLOT).abi_encode_packed()).0);
        Ok(self.storage.sload(ACL_ADDRESS, key)? == U256::ONE)
    }

    pub fn add_parent_ip(
        &mut self,
        msg_sender: Address,
        ip_id: Address,
        parent_ip_ids: Vec<Address>,
    ) -> Result<()> {
        if !self.is_allowed(msg_sender)? {
            return Err(DataNetworkPrecompileError::Unauthorized(
                "caller not allowed to add parent IP",
            ));
        }

        let data_slot = U256::from_be_bytes(keccak256(ip_id).0);

        for (index, parent_ip_id) in parent_ip_ids.iter().enumerate() {
            self.storage.sstore(
                IP_GRAPH_ADDRESS,
                data_slot + U256::from(index),
                U256::from_be_slice(parent_ip_id.as_slice()),
            )?;
        }

        self.storage.sstore(
            IP_GRAPH_ADDRESS,
            U256::from_be_slice(ip_id.as_slice()),
            U256::from(parent_ip_ids.len()),
        )
    }

    pub fn has_parent_ip(
        &self,
        msg_sender: Address,
        ip_id: Address,
        parent_ip_id: Address,
    ) -> Result<bool> {
        if !self.is_allowed(msg_sender)? {
            return Err(DataNetworkPrecompileError::Unauthorized(
                "caller not allowed to query hasParentIp",
            ));
        }

        let length_slot = U256::from_be_slice(ip_id.as_slice());
        let current_length = self.storage.sload(IP_GRAPH_ADDRESS, length_slot)?;
        let data_slot = U256::from_be_bytes(keccak256(ip_id).0);

        for index in 0..current_length.to::<u64>() {
            let stored_parent = self
                .storage
                .sload(IP_GRAPH_ADDRESS, data_slot + U256::from(index))?;

            if Address::from_word(B256::from(stored_parent)) == parent_ip_id {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub fn get_parent_ips(&self, msg_sender: Address, ip_id: Address) -> Result<Vec<Address>> {
        if !self.is_allowed(msg_sender)? {
            return Err(DataNetworkPrecompileError::Unauthorized(
                "caller not allowed to query getParentIps",
            ));
        }

        let length_slot = U256::from_be_slice(ip_id.as_slice());
        let current_length = self.storage.sload(IP_GRAPH_ADDRESS, length_slot)?;
        let data_slot = U256::from_be_bytes(keccak256(ip_id).0);
        let mut parent_ip_ids = Vec::new();

        for index in 0..current_length.to::<u64>() {
            let stored_parent = self
                .storage
                .sload(IP_GRAPH_ADDRESS, data_slot + U256::from(index))?;

            parent_ip_ids.push(Address::from_word(B256::from(stored_parent)));
        }

        Ok(parent_ip_ids)
    }

    pub fn get_parent_ips_count(&self, msg_sender: Address, ip_id: Address) -> Result<U256> {
        if !self.is_allowed(msg_sender)? {
            return Err(DataNetworkPrecompileError::Unauthorized(
                "caller not allowed to query parent Ips count",
            ));
        }

        self.storage
            .sload(IP_GRAPH_ADDRESS, U256::from_be_slice(ip_id.as_slice()))
    }

    pub fn get_ancestor_ips(&self, msg_sender: Address, ip_id: Address) -> Result<Vec<Address>> {
        if !self.is_allowed(msg_sender)? {
            return Err(DataNetworkPrecompileError::Unauthorized(
                "caller not allowed to query getAncestorIps",
            ));
        }

        let mut ancestors: Vec<_> = self.find_ancestors(ip_id)?.into_iter().collect();
        ancestors.sort_unstable();

        Ok(ancestors)
    }

    pub fn get_ancestor_ips_count(&self, msg_sender: Address, ip_id: Address) -> Result<U256> {
        if !self.is_allowed(msg_sender)? {
            return Err(DataNetworkPrecompileError::Unauthorized(
                "caller not allowed to query getAncestorIpsCount",
            ));
        }

        Ok(U256::from(self.find_ancestors(ip_id)?.len()))
    }

    pub fn has_ancestor_ip(
        &self,
        msg_sender: Address,
        ip_id: Address,
        ancestor_ip_id: Address,
    ) -> Result<bool> {
        if !self.is_allowed(msg_sender)? {
            return Err(DataNetworkPrecompileError::Unauthorized(
                "caller not allowed to query hasAncestorIp",
            ));
        }

        Ok(self.find_ancestors(ip_id)?.contains(&ancestor_ip_id))
    }

    fn find_ancestors(&self, ip_id: Address) -> Result<HashSet<Address>> {
        let mut ancestors = HashSet::new();
        let mut stack = vec![ip_id];

        while let Some(node) = stack.pop() {
            let current_length = self
                .storage
                .sload(IP_GRAPH_ADDRESS, U256::from_be_slice(node.as_slice()))?;
            let data_slot = U256::from_be_bytes(keccak256(node).0);

            for index in 0..current_length.to::<u64>() {
                let stored_parent = self
                    .storage
                    .sload(IP_GRAPH_ADDRESS, data_slot + U256::from(index))?;
                let parent_ip_id = Address::from_word(B256::from(stored_parent));

                if ancestors.insert(parent_ip_id) {
                    stack.push(parent_ip_id);
                }
            }
        }

        Ok(ancestors)
    }

    pub fn set_royalty(
        &mut self,
        msg_sender: Address,
        ip_id: Address,
        parent_ip_id: Address,
        royalty_policy_kind: U256,
        royalty: U256,
    ) -> Result<()> {
        if !self.is_allowed(msg_sender)? {
            return Err(DataNetworkPrecompileError::Unauthorized(
                "caller not allowed to set Royalty",
            ));
        }

        if royalty > U256::from(u32::MAX) {
            return Err(DataNetworkPrecompileError::Revert(
                "royalty value exceeds uint32 range",
            ));
        }

        let policy_bytes = royalty_policy_kind.to_be_bytes_trimmed_vec();
        let mut hasher = Keccak256::new();
        hasher.update(ip_id.as_slice());
        hasher.update(parent_ip_id.as_slice());
        hasher.update(&policy_bytes);
        let slot = U256::from_be_bytes(hasher.finalize().0);
        self.storage.sstore(IP_GRAPH_ADDRESS, slot, royalty)?;

        if royalty_policy_kind == U256::ZERO {
            let mut hasher = Keccak256::new();
            hasher.update(parent_ip_id.as_slice());
            hasher.update(&policy_bytes);
            hasher.update(b"royaltyStack");
            let parent_slot = U256::from_be_bytes(hasher.finalize().0);
            let parent_royalty_stack = self.storage.sload(IP_GRAPH_ADDRESS, parent_slot)?;

            let mut hasher = Keccak256::new();
            hasher.update(ip_id.as_slice());
            hasher.update(&policy_bytes);
            hasher.update(b"royaltyStack");
            let royalty_stack_slot = U256::from_be_bytes(hasher.finalize().0);
            let royalty_stack = self.storage.sload(IP_GRAPH_ADDRESS, royalty_stack_slot)?;
            let royalty_stack = royalty_stack
                .wrapping_add(parent_royalty_stack)
                .wrapping_add(royalty);

            self.storage
                .sstore(IP_GRAPH_ADDRESS, royalty_stack_slot, royalty_stack)?;
        }

        Ok(())
    }

    pub fn get_royalty(
        &self,
        msg_sender: Address,
        ip_id: Address,
        ancestor_ip_id: Address,
        royalty_policy_kind: U256,
    ) -> Result<U256> {
        if !self.is_allowed(msg_sender)? {
            return Err(DataNetworkPrecompileError::Unauthorized(
                "caller not allowed to query getRoyalty",
            ));
        }

        let total_royalty = match royalty_policy_kind {
            U256::ZERO => self.get_royalty_lap(ip_id, ancestor_ip_id)?,
            U256::ONE => self.get_royalty_lrp(ip_id, ancestor_ip_id)?,
            _ => {
                return Err(DataNetworkPrecompileError::Revert(
                    "unknown royalty policy kind",
                ));
            }
        };

        if total_royalty > BigUint::from(u32::MAX) {
            return Err(DataNetworkPrecompileError::Revert(
                "royalty value exceeds uint32 range",
            ));
        }

        Ok(U256::from_be_slice(&total_royalty.to_bytes_be()))
    }

    fn get_royalty_lap(&self, ip_id: Address, ancestor_ip_id: Address) -> Result<BigUint> {
        let mut royalties = HashMap::new();
        let mut path_counts = HashMap::new();
        royalties.insert(ip_id, BigUint::from(HUNDRED_PERCENT));
        path_counts.insert(ip_id, BigUint::from(1u8));

        let (topo_order, all_parents) = self.topological_sort(ip_id, ancestor_ip_id)?;
        let policy_bytes = U256::ZERO.to_be_bytes_trimmed_vec();

        for node in topo_order.into_iter().rev() {
            if node == ancestor_ip_id {
                break;
            }

            let Some(parents) = all_parents.get(&node) else {
                continue;
            };
            let contribution = path_counts.get(&node).cloned().ok_or_else(|| {
                DataNetworkPrecompileError::Fatal(
                    "missing path count while calculating LAP royalty".into(),
                )
            })?;

            for parent_ip_id in parents {
                let mut hasher = Keccak256::new();
                hasher.update(node.as_slice());
                hasher.update(parent_ip_id.as_slice());
                hasher.update(&policy_bytes);
                let royalty_slot = U256::from_be_bytes(hasher.finalize().0);
                let parent_royalty = self.storage.sload(IP_GRAPH_ADDRESS, royalty_slot)?;
                let parent_royalty = BigUint::from_bytes_be(&parent_royalty.to_be_bytes::<32>());

                *path_counts.entry(*parent_ip_id).or_default() += &contribution;
                *royalties.entry(*parent_ip_id).or_default() += &contribution * parent_royalty;
            }
        }

        Ok(royalties.remove(&ancestor_ip_id).unwrap_or_default())
    }

    fn get_royalty_lrp(&self, ip_id: Address, ancestor_ip_id: Address) -> Result<BigUint> {
        let mut royalties = HashMap::new();
        royalties.insert(ip_id, BigUint::from(HUNDRED_PERCENT));

        let (topo_order, all_parents) = self.topological_sort(ip_id, ancestor_ip_id)?;
        let policy_bytes = U256::ONE.to_be_bytes_trimmed_vec();

        for node in topo_order.into_iter().rev() {
            if node == ancestor_ip_id {
                break;
            }

            let current_royalty = royalties.get(&node).cloned().unwrap_or_default();
            if current_royalty == BigUint::default() {
                continue;
            }

            let Some(parents) = all_parents.get(&node) else {
                continue;
            };

            for parent_ip_id in parents {
                let mut hasher = Keccak256::new();
                hasher.update(node.as_slice());
                hasher.update(parent_ip_id.as_slice());
                hasher.update(&policy_bytes);
                let royalty_slot = U256::from_be_bytes(hasher.finalize().0);
                let parent_royalty = self.storage.sload(IP_GRAPH_ADDRESS, royalty_slot)?;
                let parent_royalty = BigUint::from_bytes_be(&parent_royalty.to_be_bytes::<32>());
                let contribution =
                    &current_royalty * parent_royalty / BigUint::from(HUNDRED_PERCENT);

                *royalties.entry(*parent_ip_id).or_default() += contribution;
            }
        }

        Ok(royalties.remove(&ancestor_ip_id).unwrap_or_default())
    }

    #[allow(clippy::type_complexity)]
    fn topological_sort(
        &self,
        ip_id: Address,
        ancestor_ip_id: Address,
    ) -> Result<(Vec<Address>, HashMap<Address, Vec<Address>>)> {
        let mut all_parents = HashMap::<Address, Vec<Address>>::new();
        let mut visited = HashSet::new();
        let mut in_topo_order = HashSet::new();
        let mut topo_order = Vec::new();
        let mut stack = vec![ip_id];

        while let Some(current) = stack.pop() {
            if visited.contains(&current) {
                if in_topo_order.insert(current) {
                    topo_order.push(current);
                }
                continue;
            }

            visited.insert(current);
            stack.push(current);

            let current_length = self
                .storage
                .sload(IP_GRAPH_ADDRESS, U256::from_be_slice(current.as_slice()))?;
            let data_slot = U256::from_be_bytes(keccak256(current).0);

            for index in 0..current_length.to::<u64>() {
                let stored_parent = self
                    .storage
                    .sload(IP_GRAPH_ADDRESS, data_slot + U256::from(index))?;
                let parent_ip_id = Address::from_word(B256::from(stored_parent));
                all_parents.entry(current).or_default().push(parent_ip_id);

                if !visited.contains(&parent_ip_id) {
                    stack.push(parent_ip_id);
                }
            }
        }

        if !visited.contains(&ancestor_ip_id) {
            return Ok((Vec::new(), HashMap::new()));
        }

        Ok((topo_order, all_parents))
    }

    pub fn get_royalty_stack(
        &self,
        msg_sender: Address,
        ip_id: Address,
        royalty_policy_kind: U256,
    ) -> Result<U256> {
        if !self.is_allowed(msg_sender)? {
            return Err(DataNetworkPrecompileError::Unauthorized(
                "caller not allowed to query getRoyaltyStack",
            ));
        }

        match royalty_policy_kind {
            U256::ZERO => self.get_royalty_stack_lap(ip_id),
            U256::ONE => self.get_royalty_stack_lrp(ip_id),
            _ => Err(DataNetworkPrecompileError::Revert(
                "unknown royalty policy kind",
            )),
        }
    }

    fn get_royalty_stack_lap(&self, ip_id: Address) -> Result<U256> {
        let policy_bytes = U256::ZERO.to_be_bytes_trimmed_vec();
        let mut hasher = Keccak256::new();
        hasher.update(ip_id.as_slice());
        hasher.update(&policy_bytes);
        hasher.update(b"royaltyStack");
        let slot = U256::from_be_bytes(hasher.finalize().0);

        self.storage.sload(IP_GRAPH_ADDRESS, slot)
    }

    fn get_royalty_stack_lrp(&self, ip_id: Address) -> Result<U256> {
        let current_length = self
            .storage
            .sload(IP_GRAPH_ADDRESS, U256::from_be_slice(ip_id.as_slice()))?;
        let data_slot = U256::from_be_bytes(keccak256(ip_id).0);
        let policy_bytes = U256::ONE.to_be_bytes_trimmed_vec();
        let mut total_royalty = U256::ZERO;

        for index in 0..current_length.to::<u64>() {
            let stored_parent = self
                .storage
                .sload(IP_GRAPH_ADDRESS, data_slot + U256::from(index))?;
            let parent_ip_id = Address::from_word(B256::from(stored_parent));

            let mut hasher = Keccak256::new();
            hasher.update(ip_id.as_slice());
            hasher.update(parent_ip_id.as_slice());
            hasher.update(&policy_bytes);
            let royalty_slot = U256::from_be_bytes(hasher.finalize().0);
            let royalty = self.storage.sload(IP_GRAPH_ADDRESS, royalty_slot)?;

            total_royalty = total_royalty.wrapping_add(royalty);
        }

        Ok(total_royalty)
    }
}
