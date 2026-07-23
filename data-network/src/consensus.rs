use std::{marker::PhantomData, sync::Arc, time::Duration};

use alloy::{
    consensus::{
        transaction::{SignerRecoverable, TransactionInfo},
        TxEnvelope,
    },
    eips::{eip4895::Withdrawal, eip7685::Requests},
    primitives::{Address, Bloom, Bytes, B256, U256},
    rpc::types::{
        engine::{
            CancunPayloadFields, ExecutionPayload, ExecutionPayloadSidecar, ExecutionPayloadV1,
            ExecutionPayloadV2, ExecutionPayloadV3, PraguePayloadFields,
        },
        Block, Transaction,
    },
};
use cometbft::{
    block::Block as CometBlock, crypto::default::Sha256, merkle::simple_hash_from_byte_vectors,
};
use cometbft_rpc::client::Client;
use eyre::{bail, eyre, Context, Result};
use prost::Message;
use tokio::sync::{
    mpsc::{channel, Receiver, Sender},
    watch, Mutex,
};
use tracing::{error, info, warn};

use helios_common::fork_schedule::ForkSchedule;
use helios_core::{consensus::Consensus, time::interval};
use phos_light_client::{
    builder::LightClientBuilder,
    components::{clock::SystemClock, io::IoError, scheduler::basic_bisecting_schedule},
    errors::Error as LightClientError,
    instance::Instance,
    store::memory::MemoryStore,
    verifier::{
        predicates::ProdPredicates,
        types::{Height, LightBlock},
        ProdVerifier,
    },
};
use phos_proto::{
    cosmos::tx::v1beta1::{TxBody, TxRaw},
    story::evmengine::v1::types::{ExecutionPayloadDeneb, MsgExecutionPayload},
};

use crate::{
    config::{Config, TrustOptions},
    database::Database,
    rpc::http_rpc::{HttpClient, ProdIo},
};

// DATA blocks retain the legacy protobuf package name on the wire.
const EXECUTION_PAYLOAD_TYPE_URL: &str = "/story.evmengine.v1.types.MsgExecutionPayload";

#[derive(Debug, Clone)]
enum ConsensusSyncStatus {
    Syncing,
    Synced,
    Error(String),
}

pub struct ConsensusClient<DB: Database> {
    pub block_recv: Option<Receiver<Block<Transaction>>>,
    pub finalized_block_recv: Option<watch::Receiver<Option<Block<Transaction>>>>,
    sync_status_recv: Mutex<watch::Receiver<ConsensusSyncStatus>>,
    shutdown_send: watch::Sender<bool>,
    config: Arc<Config>,
    phantom: PhantomData<DB>,
}

#[derive(Debug)]
pub(crate) struct Provider {
    instance: Instance,
    rpc: ProdIo,
}

impl Provider {
    pub fn new(instance: Instance, rpc: ProdIo) -> Self {
        Self { instance, rpc }
    }

    pub async fn verify_to_highest(&mut self) -> Result<LightBlock, LightClientError> {
        self.instance
            .light_client
            .verify_to_highest(&mut self.instance.state)
            .await
    }

    pub async fn fetch_block(&self, height: Height) -> Result<CometBlock, IoError> {
        self.rpc.fetch_block(height).await
    }
}

struct Inner<DB: Database> {
    provider: Provider,
    last_comet_height: Option<Height>,
    execution_forks: ForkSchedule,
    block_send: Sender<Block<Transaction>>,
    finalized_block_send: watch::Sender<Option<Block<Transaction>>>,
    db: Arc<DB>,
}

impl<DB: Database> ConsensusClient<DB> {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let (block_send, block_recv) = channel(256);
        let (finalized_block_send, finalized_block_recv) = watch::channel(None);
        let (sync_status_send, sync_status_recv) = watch::channel(ConsensusSyncStatus::Syncing);
        let (shutdown_send, mut shutdown_recv) = watch::channel(false);
        let config_clone = config.clone();
        let db = Arc::new(DB::new(&config)?);
        let trust_options = db.load_trust_options()?;
        let consensus_rpc = config.consensus_rpc.clone();
        let verifier_options = config.verifier_options;
        let execution_forks = config.execution_forks;

        #[cfg(not(target_arch = "wasm32"))]
        let run = tokio::spawn;

        #[cfg(target_arch = "wasm32")]
        let run = wasm_bindgen_futures::spawn_local;

        run(async move {
            let mut inner = match Inner::new(
                consensus_rpc,
                trust_options,
                verifier_options,
                execution_forks,
                block_send,
                finalized_block_send,
                db,
            )
            .await
            {
                Ok(inner) => inner,
                Err(err) => {
                    error!(target: "helios::data_network", err = %err, "sync failed");
                    _ = sync_status_send.send(ConsensusSyncStatus::Error(err.to_string()));
                    return;
                }
            };

            if let Err(err) = inner.advance().await {
                error!(target: "helios::data_network", err = %err, "sync failed");
                _ = sync_status_send.send(ConsensusSyncStatus::Error(err.to_string()));
                return;
            }
            _ = sync_status_send.send(ConsensusSyncStatus::Synced);

            let mut interval = interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    result = shutdown_recv.changed() => {
                        if result.is_err() || *shutdown_recv.borrow_and_update() {
                            info!(target: "helios::data_network", "shutting down consensus client");
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        if let Err(err) = inner.advance().await {
                            warn!(target: "helios::data_network", err = %err, "advance failed");
                        }
                    }
                }
            }
        });

        Ok(Self {
            block_recv: Some(block_recv),
            finalized_block_recv: Some(finalized_block_recv),
            sync_status_recv: Mutex::new(sync_status_recv),
            shutdown_send,
            config: config_clone,
            phantom: PhantomData,
        })
    }
}

#[async_trait::async_trait]
impl<DB: Database> Consensus<Block<Transaction>> for ConsensusClient<DB> {
    fn block_recv(&mut self) -> Option<Receiver<Block<Transaction>>> {
        self.block_recv.take()
    }

    fn finalized_block_recv(&mut self) -> Option<watch::Receiver<Option<Block<Transaction>>>> {
        self.finalized_block_recv.take()
    }

    fn checkpoint_recv(&self) -> Option<watch::Receiver<Option<B256>>> {
        None
    }

    fn expected_highest_block(&self) -> u64 {
        u64::MAX
    }

    fn chain_id(&self) -> u64 {
        self.config.chain.chain_id
    }

    fn shutdown(&self) -> Result<()> {
        self.shutdown_send.send(true)?;
        Ok(())
    }

    async fn wait_synced(&self) -> Result<()> {
        let mut sync_status_recv = self.sync_status_recv.lock().await;

        loop {
            let status = sync_status_recv.borrow().clone();
            match status {
                ConsensusSyncStatus::Synced => return Ok(()),
                ConsensusSyncStatus::Error(err) => return Err(eyre!("sync failed: {err}")),
                ConsensusSyncStatus::Syncing => sync_status_recv.changed().await?,
            }
        }
    }
}

impl<DB: Database> Inner<DB> {
    async fn new(
        consensus_rpc: reqwest::Url,
        trust_options: TrustOptions,
        verifier_options: phos_light_client::verifier::options::Options,
        execution_forks: ForkSchedule,
        block_send: Sender<Block<Transaction>>,
        finalized_block_send: watch::Sender<Option<Block<Transaction>>>,
        db: Arc<DB>,
    ) -> Result<Self> {
        let rpc_client = HttpClient::new(consensus_rpc);
        let peer_id = rpc_client.status().await?.node_info.id;
        let rpc = ProdIo::new(peer_id, rpc_client);
        let instance = LightClientBuilder::custom(
            peer_id,
            verifier_options,
            Box::new(MemoryStore::new()),
            Box::new(rpc.clone()),
            Box::new(SystemClock),
            Box::new(ProdVerifier::default()),
            Box::new(basic_bisecting_schedule),
            Box::new(ProdPredicates),
        )
        .trust_primary_at(trust_options.height, trust_options.hash)
        .await?
        .build();

        let provider = Provider::new(instance, rpc);

        Ok(Self {
            provider,
            last_comet_height: None,
            execution_forks,
            block_send,
            finalized_block_send,
            db,
        })
    }

    async fn advance(&mut self) -> Result<()> {
        let light_block = self.provider.verify_to_highest().await?;

        if self.last_comet_height == Some(light_block.height()) {
            return Ok(());
        }

        let block =
            fetch_execution_block(&self.provider, &light_block, &self.execution_forks).await?;
        self.last_comet_height = Some(light_block.height());
        self.db.save_trust_options(&TrustOptions {
            height: light_block.height(),
            hash: light_block.signed_header.header.hash_with::<Sha256>(),
        })?;

        let Some(block) = block else {
            return Ok(());
        };

        self.block_send
            .send(block.clone())
            .await
            .map_err(|_| eyre!("block receiver closed"))?;
        self.finalized_block_send
            .send(Some(block))
            .map_err(|_| eyre!("finalized block receiver closed"))?;

        Ok(())
    }
}

pub(crate) async fn fetch_execution_block(
    provider: &Provider,
    light_block: &LightBlock,
    execution_forks: &ForkSchedule,
) -> Result<Option<Block<Transaction>>> {
    let trusted_header = &light_block.signed_header.header;
    let block = provider.fetch_block(trusted_header.height).await?;

    verify_block_data(&block, light_block)?;

    let Some(tx) = block.data.first() else {
        return Ok(None);
    };

    let payload = decode_execution_payload(tx)?;
    let app_hash = B256::try_from(trusted_header.app_hash.as_bytes())
        .map_err(|_| eyre!("CometBFT app hash is not 32 bytes"))?;

    payload_to_block(payload, app_hash, execution_forks).map(Some)
}

fn verify_block_data(block: &CometBlock, light_block: &LightBlock) -> Result<()> {
    let trusted_header = &light_block.signed_header.header;

    if block.header.height != trusted_header.height {
        bail!(
            "fetched block height {} does not match verified height {}",
            block.header.height,
            trusted_header.height
        );
    }

    if block.header.hash_with::<Sha256>() != trusted_header.hash_with::<Sha256>() {
        bail!("fetched block header does not match verified header");
    }

    let data_hash = block_data_hash(&block.data);
    if data_hash.as_slice() != block.header.data_hash.unwrap_or_default().as_ref() {
        bail!("fetched block data does not match verified data hash");
    }

    let expected_transactions = if block.header.height.value() == 1 {
        0
    } else {
        1
    };

    if block.data.len() != expected_transactions {
        bail!(
            "DATA block at height {} must contain exactly {} execution payload transactions, found {}",
            block.header.height,
            expected_transactions,
            block.data.len()
        );
    }

    Ok(())
}

fn block_data_hash(data: &[impl AsRef<[u8]>]) -> [u8; 32] {
    let transaction_hashes = data
        .iter()
        .map(|tx| <Sha256 as cometbft::crypto::Sha256>::digest(tx))
        .collect::<Vec<_>>();

    simple_hash_from_byte_vectors::<Sha256>(&transaction_hashes)
}

fn decode_execution_payload(tx: &[u8]) -> Result<ExecutionPayloadV3> {
    let tx = TxRaw::decode(tx).wrap_err("failed to decode Cosmos SDK TxRaw")?;
    let body =
        TxBody::decode(tx.body_bytes.as_slice()).wrap_err("failed to decode Cosmos SDK TxBody")?;

    if body.messages.len() != 1 {
        bail!(
            "DATA transaction must contain exactly one message, found {}",
            body.messages.len()
        );
    }

    let message = &body.messages[0];
    if message.type_url != EXECUTION_PAYLOAD_TYPE_URL {
        bail!("unexpected DATA message type: {}", message.type_url);
    }

    let payload = MsgExecutionPayload::decode(message.value.as_slice())
        .wrap_err("failed to decode MsgExecutionPayload")?;

    match (
        payload.execution_payload.is_empty(),
        payload.execution_payload_deneb,
    ) {
        (false, None) => serde_json::from_slice(&payload.execution_payload)
            .wrap_err("failed to decode legacy JSON execution payload"),
        (true, Some(payload)) => payload_from_proto(payload),
        (false, Some(_)) => bail!("execution payload contains both legacy and protobuf forms"),
        (true, None) => bail!("execution payload is missing"),
    }
}

fn payload_to_block(
    execution_payload: ExecutionPayloadV3,
    app_hash: B256,
    execution_forks: &ForkSchedule,
) -> Result<Block<Transaction>> {
    let payload = ExecutionPayload::V3(execution_payload);
    let expected_block_hash = payload.block_hash();

    let versioned_hashes = Vec::new();
    let parent_beacon_block_root = app_hash;
    let cancun = CancunPayloadFields::new(parent_beacon_block_root, versioned_hashes);

    let sidecar = if payload.timestamp() >= execution_forks.prague_timestamp {
        let requests = Requests::default();
        let prague = PraguePayloadFields::new(requests);
        ExecutionPayloadSidecar::v4(cancun, prague)
    } else {
        ExecutionPayloadSidecar::v3(cancun)
    };

    let consensus_block = payload
        .try_into_block_with_sidecar::<TxEnvelope>(&sidecar)
        .wrap_err("failed to construct execution block from payload")?;

    let block_hash = consensus_block.header.hash_slow();
    if block_hash != expected_block_hash {
        bail!(
            "execution payload block hash mismatch: expected {expected_block_hash}, got {block_hash}"
        );
    }

    let block_number = consensus_block.header.number;
    let base_fee = consensus_block.header.base_fee_per_gas;
    let mut transaction_index = 0;

    let rpc_block = Block::from_consensus(consensus_block, Some(U256::ZERO));
    let rpc_block = rpc_block.try_map_transactions(|transaction| {
        let transaction_info = TransactionInfo {
            hash: Some(*transaction.tx_hash()),
            index: Some(transaction_index),
            block_hash: Some(block_hash),
            block_number: Some(block_number),
            base_fee,
        };
        transaction_index += 1;

        transaction
            .try_into_recovered()
            .map(|transaction| Transaction::from_transaction(transaction, transaction_info))
    })?;

    Ok(rpc_block)
}

fn payload_from_proto(payload: ExecutionPayloadDeneb) -> Result<ExecutionPayloadV3> {
    let withdrawals = payload
        .withdrawals
        .into_iter()
        .map(|withdrawal| {
            Ok(Withdrawal {
                index: withdrawal.index,
                validator_index: withdrawal.validator_index,
                address: Address::from(fixed_bytes("withdrawal address", withdrawal.address)?),
                amount: withdrawal.amount,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ExecutionPayloadV3 {
        payload_inner: ExecutionPayloadV2 {
            payload_inner: ExecutionPayloadV1 {
                parent_hash: B256::from(fixed_bytes("parent hash", payload.parent_hash)?),
                fee_recipient: Address::from(fixed_bytes("fee recipient", payload.fee_recipient)?),
                state_root: B256::from(fixed_bytes("state root", payload.state_root)?),
                receipts_root: B256::from(fixed_bytes("receipts root", payload.receipts_root)?),
                logs_bloom: Bloom::from(fixed_bytes("logs bloom", payload.logs_bloom)?),
                prev_randao: B256::from(fixed_bytes("prev randao", payload.prev_randao)?),
                block_number: payload.block_number,
                gas_limit: payload.gas_limit,
                gas_used: payload.gas_used,
                timestamp: payload.timestamp,
                extra_data: Bytes::from(payload.extra_data),
                base_fee_per_gas: U256::from_be_bytes(fixed_bytes::<32>(
                    "base fee per gas",
                    payload.base_fee_per_gas,
                )?),
                block_hash: B256::from(fixed_bytes("block hash", payload.block_hash)?),
                transactions: payload.transactions.into_iter().map(Bytes::from).collect(),
            },
            withdrawals,
        },
        blob_gas_used: payload.blob_gas_used,
        excess_blob_gas: payload.excess_blob_gas,
    })
}

fn fixed_bytes<const N: usize>(field: &str, bytes: Vec<u8>) -> Result<[u8; N]> {
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| eyre!("{field} must be {N} bytes, found {}", bytes.len()))
}
