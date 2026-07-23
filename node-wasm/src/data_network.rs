extern crate console_error_panic_hook;
extern crate web_sys;

use std::collections::HashMap;
use std::str::FromStr;

use alloy::eips::{BlockId, BlockNumberOrTag};
use alloy::hex;
use alloy::primitives::{Address, B256, U256};
use alloy::rpc::types::{state::StateOverride, Filter, TransactionRequest};
use eyre::Result;
use url::Url;
use wasm_bindgen::prelude::*;
use web_sys::js_sys::Function;

use helios_common::types::SubscriptionType;
use phos_data_network::config::{networks, Config, TrustOptions};
use phos_data_network::database::{ConfigDB, Database};
use phos_data_network::spec::DataNetwork;
use phos_data_network::DataNetworkClientBuilder;
use phos_light_client::types::{Hash, Height};

use crate::map_err;
use crate::storage::LocalStorageDB;
use crate::subscription::Subscription;

#[derive(Clone)]
pub enum DatabaseType {
    Memory(ConfigDB),
    LocalStorage(LocalStorageDB),
}

impl Database for DatabaseType {
    fn new(config: &Config) -> Result<Self> {
        match config.database_type.as_deref() {
            Some("config") => Ok(DatabaseType::Memory(ConfigDB::new(config)?)),
            Some("localstorage") => match LocalStorageDB::new(config) {
                Ok(db) => Ok(DatabaseType::LocalStorage(db)),
                Err(_) => {
                    web_sys::console::warn_1(
                        &"Phos: localStorage unavailable, falling back to configured trust options"
                            .into(),
                    );
                    Ok(DatabaseType::Memory(ConfigDB::new(config)?))
                }
            },
            _ => Ok(DatabaseType::Memory(ConfigDB::new(config)?)),
        }
    }

    fn load_trust_options(&self) -> Result<TrustOptions> {
        match self {
            DatabaseType::Memory(db) => db.load_trust_options(),
            DatabaseType::LocalStorage(db) => db.load_trust_options(),
        }
    }

    fn save_trust_options(&self, trust_options: &TrustOptions) -> Result<()> {
        match self {
            DatabaseType::Memory(db) => db.save_trust_options(trust_options),
            DatabaseType::LocalStorage(db) => db.save_trust_options(trust_options),
        }
    }
}

#[wasm_bindgen]
pub struct DataNetworkClient {
    inner: phos_data_network::DataNetworkClient,
    chain_id: u64,
    active_subscriptions: HashMap<String, Subscription<DataNetwork>>,
}

#[wasm_bindgen]
impl DataNetworkClient {
    #[wasm_bindgen(constructor)]
    pub fn new(
        execution_rpc: Option<String>,
        verifiable_api: Option<String>,
        consensus_rpc: Option<String>,
        network: String,
        trust_height: Option<String>,
        trust_hash: Option<String>,
        db_type: String,
    ) -> Result<DataNetworkClient, JsError> {
        console_error_panic_hook::set_once();

        let base = match network.as_str() {
            "mainnet" => networks::mainnet(),
            "aeneid" => networks::aeneid(),
            other => Err(JsError::new(&format!("invalid network: {other}")))?,
        };

        let chain_id = base.chain.chain_id;

        let trust_options = match (trust_height, trust_hash) {
            (Some(height), Some(hash)) => {
                let height = height
                    .parse::<u64>()
                    .map_err(|e| JsError::new(&format!("invalid trust height: {e}")))?;
                let height = Height::try_from(height)
                    .map_err(|e| JsError::new(&format!("invalid trust height: {e}")))?;
                let hash = hash
                    .strip_prefix("0x")
                    .unwrap_or(&hash)
                    .to_ascii_uppercase();
                let hash = Hash::from_str(&hash)
                    .map_err(|e| JsError::new(&format!("invalid trust hash: {e}")))?;

                Some(TrustOptions { height, hash })
            }
            (None, None) => None,
            _ => {
                return Err(JsError::new(
                    "trust height and trust hash must be provided together",
                ))
            }
        };

        let consensus_rpc = if let Some(rpc) = consensus_rpc {
            Url::parse(&rpc)
                .map_err(|e| JsError::new(&format!("Invalid consensus RPC URL: {e}")))?
        } else {
            base.consensus_rpc
                .ok_or(JsError::new("consensus rpc not found"))?
        };

        let execution_rpc = execution_rpc
            .map(|url| Url::parse(&url))
            .transpose()
            .map_err(|e| JsError::new(&format!("Invalid execution RPC URL: {e}")))?;

        let verifiable_api = verifiable_api
            .map(|url| Url::parse(&url))
            .transpose()
            .map_err(|e| JsError::new(&format!("Invalid verifiable API URL: {e}")))?;

        let config = Config {
            execution_rpc,
            verifiable_api,
            consensus_rpc,
            rpc_bind_ip: None,
            rpc_port: None,
            trust_options,
            data_dir: None,
            chain: base.chain,
            verifier_options: base.verifier_options,
            execution_forks: base.execution_forks,
            database_type: Some(db_type),
        };

        let inner = map_err(
            DataNetworkClientBuilder::<DatabaseType>::new()
                .config(config)
                .build(),
        )?;

        Ok(Self {
            inner,
            chain_id,
            active_subscriptions: HashMap::new(),
        })
    }

    #[wasm_bindgen]
    pub async fn subscribe(
        &mut self,
        sub_type: JsValue,
        id: String,
        callback: Function,
    ) -> Result<bool, JsError> {
        let sub_type: SubscriptionType = serde_wasm_bindgen::from_value(sub_type)?;
        let rx = map_err(self.inner.subscribe(sub_type).await)?;

        let subscription = Subscription::<DataNetwork>::spawn_listener(id.clone(), rx, callback);
        self.active_subscriptions.insert(id, subscription);

        Ok(true)
    }

    #[wasm_bindgen]
    pub fn unsubscribe(&mut self, id: String) -> Result<bool, JsError> {
        Ok(self.active_subscriptions.remove(&id).is_some())
    }

    #[wasm_bindgen]
    pub async fn wait_synced(&self) -> Result<(), JsError> {
        map_err(self.inner.wait_synced().await)
    }

    #[wasm_bindgen]
    pub fn chain_id(&self) -> u32 {
        self.chain_id as u32
    }

    #[wasm_bindgen]
    pub async fn get_block_number(&self) -> Result<String, JsError> {
        let v = map_err(self.inner.get_block_number().await)?;
        Ok(format!("0x{:x}", v))
    }

    #[wasm_bindgen]
    pub async fn get_balance(&self, addr: JsValue, block: JsValue) -> Result<String, JsError> {
        let addr: Address = serde_wasm_bindgen::from_value(addr)?;
        let block: BlockId = serde_wasm_bindgen::from_value(block)?;
        let res = map_err(self.inner.get_balance(addr, block).await);
        res.map(|v| v.to_string())
    }

    #[wasm_bindgen]
    pub async fn get_transaction_by_hash(&self, hash: String) -> Result<JsValue, JsError> {
        let hash = B256::from_str(&hash)?;
        let tx = map_err(self.inner.get_transaction(hash).await)?;
        Ok(serde_wasm_bindgen::to_value(&tx)?)
    }

    #[wasm_bindgen]
    pub async fn get_transaction_by_block_hash_and_index(
        &self,
        hash: JsValue,
        index: JsValue,
    ) -> Result<JsValue, JsError> {
        let hash: B256 = serde_wasm_bindgen::from_value(hash)?;
        let index: u64 = serde_wasm_bindgen::from_value(index)?;
        let tx = map_err(
            self.inner
                .get_transaction_by_block_and_index(hash.into(), index)
                .await,
        )?;

        Ok(serde_wasm_bindgen::to_value(&tx)?)
    }

    #[wasm_bindgen]
    pub async fn get_transaction_by_block_number_and_index(
        &self,
        block: JsValue,
        index: JsValue,
    ) -> Result<JsValue, JsError> {
        let block: BlockNumberOrTag = serde_wasm_bindgen::from_value(block)?;
        let index: u64 = serde_wasm_bindgen::from_value(index)?;
        let tx = map_err(
            self.inner
                .get_transaction_by_block_and_index(block.into(), index)
                .await,
        )?;

        Ok(serde_wasm_bindgen::to_value(&tx)?)
    }

    #[wasm_bindgen]
    pub async fn get_transaction_count(
        &self,
        addr: JsValue,
        block: JsValue,
    ) -> Result<u32, JsError> {
        let addr: Address = serde_wasm_bindgen::from_value(addr)?;
        let block: BlockId = serde_wasm_bindgen::from_value(block)?;
        Ok(map_err(self.inner.get_nonce(addr, block).await)? as u32)
    }

    #[wasm_bindgen]
    pub async fn get_block_transaction_count_by_hash(
        &self,
        hash: JsValue,
    ) -> Result<Option<u32>, JsError> {
        let hash: B256 = serde_wasm_bindgen::from_value(hash)?;
        let count = map_err(self.inner.get_block_transaction_count(hash.into()).await)?;
        Ok(count.map(|v| v as u32))
    }

    #[wasm_bindgen]
    pub async fn get_block_transaction_count_by_number(
        &self,
        block: JsValue,
    ) -> Result<Option<u32>, JsError> {
        let block: BlockNumberOrTag = serde_wasm_bindgen::from_value(block)?;
        let count = map_err(self.inner.get_block_transaction_count(block.into()).await)?;

        Ok(count.map(|v| v as u32))
    }

    #[wasm_bindgen]
    pub async fn get_block_by_number(
        &self,
        block: JsValue,
        full_tx: bool,
    ) -> Result<JsValue, JsError> {
        let block: BlockNumberOrTag = serde_wasm_bindgen::from_value(block)?;
        let block = map_err(self.inner.get_block(block.into(), full_tx).await)?;
        Ok(serde_wasm_bindgen::to_value(&block)?)
    }

    #[wasm_bindgen]
    pub async fn get_block_by_hash(&self, hash: String, full_tx: bool) -> Result<JsValue, JsError> {
        let hash = B256::from_str(&hash)?;
        let block = map_err(self.inner.get_block(hash.into(), full_tx).await)?;
        Ok(serde_wasm_bindgen::to_value(&block)?)
    }

    #[wasm_bindgen]
    pub async fn get_code(&self, addr: JsValue, block: JsValue) -> Result<String, JsError> {
        let addr: Address = serde_wasm_bindgen::from_value(addr)?;
        let block: BlockId = serde_wasm_bindgen::from_value(block)?;
        let code = map_err(self.inner.get_code(addr, block).await)?;
        Ok(format!("0x{}", hex::encode(code)))
    }

    #[wasm_bindgen]
    pub async fn get_storage_at(
        &self,
        address: JsValue,
        slot: JsValue,
        block: JsValue,
    ) -> Result<JsValue, JsError> {
        let address: Address = serde_wasm_bindgen::from_value(address)?;
        let slot: U256 = serde_wasm_bindgen::from_value(slot)?;
        let block: BlockId = serde_wasm_bindgen::from_value(block)?;
        let storage = map_err(self.inner.get_storage_at(address, slot, block).await)?;
        Ok(serde_wasm_bindgen::to_value(&storage)?)
    }

    #[wasm_bindgen]
    pub async fn get_proof(
        &self,
        address: JsValue,
        storage_keys: JsValue,
        block: JsValue,
    ) -> Result<JsValue, JsError> {
        let address: Address = serde_wasm_bindgen::from_value(address)?;
        let storage_keys: Vec<U256> = serde_wasm_bindgen::from_value(storage_keys)?;
        let storage_keys = storage_keys
            .into_iter()
            .map(|k| k.into())
            .collect::<Vec<_>>();

        let block: BlockId = serde_wasm_bindgen::from_value(block)?;
        let proof = map_err(self.inner.get_proof(address, &storage_keys, block).await)?;
        Ok(serde_wasm_bindgen::to_value(&proof)?)
    }

    #[wasm_bindgen]
    pub async fn call(
        &self,
        opts: JsValue,
        block: JsValue,
        state_overrides: JsValue,
    ) -> Result<String, JsError> {
        let opts: TransactionRequest = serde_wasm_bindgen::from_value(opts)?;
        let block: BlockId = serde_wasm_bindgen::from_value(block)?;
        let state_overrides: Option<StateOverride> =
            serde_wasm_bindgen::from_value(state_overrides)?;
        let res = map_err(self.inner.call(&opts, block, state_overrides).await)?;
        Ok(format!("0x{}", hex::encode(res)))
    }

    #[wasm_bindgen]
    pub async fn estimate_gas(
        &self,
        opts: JsValue,
        block: JsValue,
        state_overrides: JsValue,
    ) -> Result<String, JsError> {
        let opts: TransactionRequest = serde_wasm_bindgen::from_value(opts)?;
        let block: Option<BlockId> = serde_wasm_bindgen::from_value(block)?;
        let state_overrides: Option<StateOverride> =
            serde_wasm_bindgen::from_value(state_overrides)?;
        let gas = map_err(self.inner.estimate_gas(&opts, block, state_overrides).await)?;
        Ok(format!("0x{gas:x}"))
    }

    #[wasm_bindgen]
    pub async fn create_access_list(
        &self,
        opts: JsValue,
        block: JsValue,
        state_overrides: JsValue,
    ) -> Result<JsValue, JsError> {
        let opts: TransactionRequest = serde_wasm_bindgen::from_value(opts)?;
        let block: BlockId = serde_wasm_bindgen::from_value(block)?;
        let state_overrides: Option<StateOverride> =
            serde_wasm_bindgen::from_value(state_overrides)?;
        let access_list_result = map_err(
            self.inner
                .create_access_list(&opts, block, state_overrides)
                .await,
        )?;
        Ok(serde_wasm_bindgen::to_value(&access_list_result)?)
    }

    #[wasm_bindgen]
    pub async fn gas_price(&self) -> Result<JsValue, JsError> {
        let price = map_err(self.inner.get_gas_price().await)?;
        Ok(serde_wasm_bindgen::to_value(&price)?)
    }

    #[wasm_bindgen]
    pub async fn max_priority_fee_per_gas(&self) -> Result<JsValue, JsError> {
        let price = map_err(self.inner.get_priority_fee().await)?;
        Ok(serde_wasm_bindgen::to_value(&price)?)
    }

    #[wasm_bindgen]
    pub async fn send_raw_transaction(&self, tx: String) -> Result<JsValue, JsError> {
        let tx = hex::decode(tx)?;
        let hash = map_err(self.inner.send_raw_transaction(&tx).await)?;
        Ok(serde_wasm_bindgen::to_value(&hash)?)
    }

    #[wasm_bindgen]
    pub async fn get_transaction_receipt(&self, tx: JsValue) -> Result<JsValue, JsError> {
        let tx: B256 = serde_wasm_bindgen::from_value(tx)?;
        let receipt = map_err(self.inner.get_transaction_receipt(tx).await)?;
        Ok(serde_wasm_bindgen::to_value(&receipt)?)
    }

    #[wasm_bindgen]
    pub async fn get_block_receipts(&self, block: JsValue) -> Result<JsValue, JsError> {
        let block: BlockId = serde_wasm_bindgen::from_value(block)?;
        let receipts = map_err(self.inner.get_block_receipts(block).await)?;
        Ok(serde_wasm_bindgen::to_value(&receipts)?)
    }

    #[wasm_bindgen]
    pub async fn get_logs(&self, filter: JsValue) -> Result<JsValue, JsError> {
        let filter: Filter = serde_wasm_bindgen::from_value(filter)?;
        let logs = map_err(self.inner.get_logs(&filter).await)?;
        Ok(serde_wasm_bindgen::to_value(&logs)?)
    }

    #[wasm_bindgen]
    pub async fn get_filter_logs(&self, filter_id: JsValue) -> Result<JsValue, JsError> {
        let filter_id: U256 = serde_wasm_bindgen::from_value(filter_id)?;
        let logs = map_err(self.inner.get_filter_logs(filter_id).await)?;
        Ok(serde_wasm_bindgen::to_value(&logs)?)
    }

    #[wasm_bindgen]
    pub async fn uninstall_filter(&self, filter_id: JsValue) -> Result<bool, JsError> {
        let filter_id: U256 = serde_wasm_bindgen::from_value(filter_id)?;
        let uninstalled = map_err(self.inner.uninstall_filter(filter_id).await)?;
        Ok(uninstalled)
    }

    #[wasm_bindgen]
    pub async fn new_filter(&self, filter: JsValue) -> Result<JsValue, JsError> {
        let filter: Filter = serde_wasm_bindgen::from_value(filter)?;
        let filter_id = map_err(self.inner.new_filter(&filter).await)?;
        Ok(serde_wasm_bindgen::to_value(&filter_id)?)
    }

    #[wasm_bindgen]
    pub async fn new_block_filter(&self) -> Result<JsValue, JsError> {
        let filter_id = map_err(self.inner.new_block_filter().await)?;
        Ok(serde_wasm_bindgen::to_value(&filter_id)?)
    }

    #[wasm_bindgen]
    pub async fn client_version(&self) -> String {
        self.inner.get_client_version().await
    }

    #[wasm_bindgen]
    pub async fn shutdown(&mut self) {
        for (_, subscription) in self.active_subscriptions.drain() {
            subscription.abort();
        }

        self.inner.shutdown().await;
    }
}
