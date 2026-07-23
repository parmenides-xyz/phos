//! Native library providing Rust to mobile language bindings for the DATA Network light client.
//!
//! This crate uses Mozilla's UniFFI to generate Swift bindings for the light client,
//! allowing it to be used from iOS applications.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use alloy::{
    eips::{BlockId, BlockNumberOrTag},
    hex,
    primitives::{Address, B256, U256},
    rpc::types::{state::StateOverride, Filter, TransactionRequest},
};
use helios_common::types::SubscriptionType;
use phos_data_network::DataNetworkClient;
use serde::de::DeserializeOwned;
use tokio::sync::RwLock;
use tokio_stream::wrappers::BroadcastStream;
use uniffi::Object;

use crate::error::{DataNetworkError, Result};
use crate::types::{NewHeadsStream, NodeConfig};

mod error;
mod types;

uniffi::setup_scaffolding!();

/// The DATA Network light client exposed to Swift.
#[derive(Object)]
pub struct DataNetworkNode {
    node: RwLock<Option<DataNetworkClient>>,
    config: NodeConfig,
}

#[uniffi::export(async_runtime = "tokio")]
impl DataNetworkNode {
    /// Sets a new connection to the DATA Network light client.
    #[uniffi::constructor]
    pub fn new(config: NodeConfig) -> Result<Self> {
        Ok(Self {
            node: RwLock::new(None),
            config,
        })
    }

    /// Starts the light client and connects to the network.
    pub async fn start(&self) -> Result<bool> {
        let mut node_lock = self.node.write().await;
        if node_lock.is_some() {
            return Err(DataNetworkError::AlreadyRunning);
        }

        let new_node = self.config.clone().into_client().await?;
        *node_lock = Some(new_node);

        Ok(true)
    }

    /// Stops the running light client and closes its network connections.
    pub async fn stop(&self) -> Result<()> {
        let mut node = self.node.write().await;
        match node.take() {
            Some(node) => {
                node.shutdown().await;
                Ok(())
            }
            None => Err(DataNetworkError::NodeNotRunning),
        }
    }

    /// Checks if the light client is currently running.
    pub async fn is_running(&self) -> bool {
        self.node.read().await.is_some()
    }

    /// Waits until the light client is synced and ready to serve requests.
    pub async fn wait_synced(&self) -> Result<()> {
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        node.wait_synced()
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))
    }

    /// Gets the DATA Network EVM chain ID.
    pub async fn chain_id(&self) -> Result<u64> {
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        Ok(node.get_chain_id().await)
    }

    /// Gets the latest verified EVM block number as a JSON-RPC quantity.
    pub async fn get_block_number(&self) -> Result<String> {
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let value = node
            .get_block_number()
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(format!("0x{value:x}"))
    }

    /// Gets an account balance.
    pub async fn get_balance(&self, address: String, block: String) -> Result<String> {
        let address = parse_string_value::<Address>(&address, "address")?;
        let block = parse_string_value::<BlockId>(&block, "block")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let value = node
            .get_balance(address, block)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(value.to_string())
    }

    /// Gets an account transaction count.
    pub async fn get_transaction_count(&self, address: String, block: String) -> Result<u64> {
        let address = parse_string_value::<Address>(&address, "address")?;
        let block = parse_string_value::<BlockId>(&block, "block")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        node.get_nonce(address, block)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))
    }

    /// Gets a transaction by hash as serialized JSON.
    pub async fn get_transaction_by_hash(&self, hash: String) -> Result<String> {
        let hash = parse_string_value::<B256>(&hash, "transaction hash")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let transaction = node
            .get_transaction(hash)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&transaction)
    }

    /// Gets a transaction by block hash and index as serialized JSON.
    pub async fn get_transaction_by_block_hash_and_index(
        &self,
        hash: String,
        index: u64,
    ) -> Result<String> {
        let hash = parse_string_value::<B256>(&hash, "block hash")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let transaction = node
            .get_transaction_by_block_and_index(hash.into(), index)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&transaction)
    }

    /// Gets a transaction by block number or tag and index as serialized JSON.
    pub async fn get_transaction_by_block_number_and_index(
        &self,
        block: String,
        index: u64,
    ) -> Result<String> {
        let block = parse_string_value::<BlockNumberOrTag>(&block, "block")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let transaction = node
            .get_transaction_by_block_and_index(block.into(), index)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&transaction)
    }

    /// Gets the transaction count for a block hash.
    pub async fn get_block_transaction_count_by_hash(&self, hash: String) -> Result<Option<u64>> {
        let hash = parse_string_value::<B256>(&hash, "block hash")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        node.get_block_transaction_count(hash.into())
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))
    }

    /// Gets the transaction count for a block number or tag.
    pub async fn get_block_transaction_count_by_number(
        &self,
        block: String,
    ) -> Result<Option<u64>> {
        let block = parse_string_value::<BlockNumberOrTag>(&block, "block")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        node.get_block_transaction_count(block.into())
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))
    }

    /// Gets a block by number or tag as serialized JSON.
    pub async fn get_block_by_number(&self, block: String, full_tx: bool) -> Result<String> {
        let block = parse_string_value::<BlockNumberOrTag>(&block, "block")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let block = node
            .get_block(block.into(), full_tx)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&block)
    }

    /// Gets a block by hash as serialized JSON.
    pub async fn get_block_by_hash(&self, hash: String, full_tx: bool) -> Result<String> {
        let hash = parse_string_value::<B256>(&hash, "block hash")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let block = node
            .get_block(hash.into(), full_tx)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&block)
    }

    /// Gets deployed bytecode at an address.
    pub async fn get_code(&self, address: String, block: String) -> Result<String> {
        let address = parse_string_value::<Address>(&address, "address")?;
        let block = parse_string_value::<BlockId>(&block, "block")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let code = node
            .get_code(address, block)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(format!("0x{}", hex::encode(code)))
    }

    /// Gets a storage value at an address and slot.
    pub async fn get_storage_at(
        &self,
        address: String,
        slot: String,
        block: String,
    ) -> Result<String> {
        let address = parse_string_value::<Address>(&address, "address")?;
        let slot = parse_string_value::<U256>(&slot, "storage slot")?;
        let block = parse_string_value::<BlockId>(&block, "block")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let storage = node
            .get_storage_at(address, slot, block)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(format!("{storage:#x}"))
    }

    /// Gets an EIP-1186 account proof as serialized JSON.
    pub async fn get_proof(
        &self,
        address: String,
        storage_keys: Vec<String>,
        block: String,
    ) -> Result<String> {
        let address = parse_string_value::<Address>(&address, "address")?;
        let storage_keys = storage_keys
            .iter()
            .map(|key| parse_string_value::<U256>(key, "storage key").map(Into::into))
            .collect::<Result<Vec<B256>>>()?;
        let block = parse_string_value::<BlockId>(&block, "block")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let proof = node
            .get_proof(address, &storage_keys, block)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&proof)
    }

    /// Executes an EVM call and returns its output bytes.
    pub async fn call(
        &self,
        transaction_json: String,
        block: String,
        state_overrides_json: Option<String>,
    ) -> Result<String> {
        let transaction = parse_json::<TransactionRequest>(&transaction_json, "transaction")?;
        let block = parse_string_value::<BlockId>(&block, "block")?;
        let state_overrides =
            parse_optional_json::<StateOverride>(state_overrides_json, "state overrides")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let output = node
            .call(&transaction, block, state_overrides)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(format!("0x{}", hex::encode(output)))
    }

    /// Estimates gas for an EVM transaction.
    pub async fn estimate_gas(
        &self,
        transaction_json: String,
        block: Option<String>,
        state_overrides_json: Option<String>,
    ) -> Result<String> {
        let transaction = parse_json::<TransactionRequest>(&transaction_json, "transaction")?;
        let block = block
            .map(|block| parse_string_value::<BlockId>(&block, "block"))
            .transpose()?;
        let state_overrides =
            parse_optional_json::<StateOverride>(state_overrides_json, "state overrides")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let gas = node
            .estimate_gas(&transaction, block, state_overrides)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(format!("0x{gas:x}"))
    }

    /// Creates an EIP-2930 access list as serialized JSON.
    pub async fn create_access_list(
        &self,
        transaction_json: String,
        block: String,
        state_overrides_json: Option<String>,
    ) -> Result<String> {
        let transaction = parse_json::<TransactionRequest>(&transaction_json, "transaction")?;
        let block = parse_string_value::<BlockId>(&block, "block")?;
        let state_overrides =
            parse_optional_json::<StateOverride>(state_overrides_json, "state overrides")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let access_list = node
            .create_access_list(&transaction, block, state_overrides)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&access_list)
    }

    /// Gets the current gas price as a JSON-RPC quantity.
    pub async fn gas_price(&self) -> Result<String> {
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let price = node
            .get_gas_price()
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(format!("0x{price:x}"))
    }

    /// Gets the current priority fee as a JSON-RPC quantity.
    pub async fn max_priority_fee_per_gas(&self) -> Result<String> {
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let price = node
            .get_priority_fee()
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(format!("0x{price:x}"))
    }

    /// Broadcasts a raw signed transaction and returns its hash.
    pub async fn send_raw_transaction(&self, transaction: String) -> Result<String> {
        let transaction = hex::decode(transaction)
            .map_err(|error| DataNetworkError::invalid_request(error.to_string()))?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let hash = node
            .send_raw_transaction(&transaction)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(format!("{hash:#x}"))
    }

    /// Gets a transaction receipt as serialized JSON.
    pub async fn get_transaction_receipt(&self, hash: String) -> Result<String> {
        let hash = parse_string_value::<B256>(&hash, "transaction hash")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let receipt = node
            .get_transaction_receipt(hash)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&receipt)
    }

    /// Gets all transaction receipts for a block as serialized JSON.
    pub async fn get_block_receipts(&self, block: String) -> Result<String> {
        let block = parse_string_value::<BlockId>(&block, "block")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let receipts = node
            .get_block_receipts(block)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&receipts)
    }

    /// Gets logs matching a serialized JSON filter.
    pub async fn get_logs(&self, filter_json: String) -> Result<String> {
        let filter = parse_json::<Filter>(&filter_json, "filter")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let logs = node
            .get_logs(&filter)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&logs)
    }

    /// Creates a log filter and returns its ID.
    pub async fn new_filter(&self, filter_json: String) -> Result<String> {
        let filter = parse_json::<Filter>(&filter_json, "filter")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let filter_id = node
            .new_filter(&filter)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(format!("0x{filter_id:x}"))
    }

    /// Creates a block filter and returns its ID.
    pub async fn new_block_filter(&self) -> Result<String> {
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let filter_id = node
            .new_block_filter()
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(format!("0x{filter_id:x}"))
    }

    /// Gets logs for an existing filter as serialized JSON.
    pub async fn get_filter_logs(&self, filter_id: String) -> Result<String> {
        let filter_id = parse_string_value::<U256>(&filter_id, "filter ID")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let logs = node
            .get_filter_logs(filter_id)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&logs)
    }

    /// Uninstalls an existing filter.
    pub async fn uninstall_filter(&self, filter_id: String) -> Result<bool> {
        let filter_id = parse_string_value::<U256>(&filter_id, "filter ID")?;
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        node.uninstall_filter(filter_id)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))
    }

    /// Gets the current synchronization status as serialized JSON.
    pub async fn syncing(&self) -> Result<String> {
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let status = node
            .syncing()
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        to_json(&status)
    }

    /// Gets the light client version string.
    pub async fn client_version(&self) -> Result<String> {
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        Ok(node.get_client_version().await)
    }

    /// Subscribes to newly verified EVM blocks.
    pub async fn subscribe_new_heads(&self) -> Result<Arc<NewHeadsStream>> {
        let node = self.node.read().await;
        let node = node.as_ref().ok_or(DataNetworkError::NodeNotRunning)?;
        let receiver = node
            .subscribe(SubscriptionType::NewHeads)
            .await
            .map_err(|error| DataNetworkError::client(error.to_string()))?;
        Ok(Arc::new(NewHeadsStream::new(BroadcastStream::new(
            receiver,
        ))))
    }
}

fn parse_string_value<T: DeserializeOwned>(value: &str, name: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|error| DataNetworkError::invalid_request(format!("invalid {name}: {error}")))
}

fn parse_json<T: DeserializeOwned>(value: &str, name: &str) -> Result<T> {
    serde_json::from_str(value)
        .map_err(|error| DataNetworkError::invalid_request(format!("invalid {name} JSON: {error}")))
}

fn parse_optional_json<T: DeserializeOwned>(
    value: Option<String>,
    name: &str,
) -> Result<Option<T>> {
    value.map(|value| parse_json(&value, name)).transpose()
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|error| DataNetworkError::serialization(error.to_string()))
}
