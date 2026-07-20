use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use alloy::{
    consensus::{
        transaction::{SignerRecoverable, TransactionInfo},
        TxEnvelope,
    },
    eips::{eip4895::Withdrawal, eip7685::Requests},
    primitives::{Address, Bloom, Bytes, Sealable, B256, U256},
    rlp::Encodable,
    rpc::types::{
        engine::{
            CancunPayloadFields, ExecutionPayload, ExecutionPayloadSidecar, ExecutionPayloadV1,
            ExecutionPayloadV2, ExecutionPayloadV3, PraguePayloadFields,
        },
        Block, BlockTransactions, Header, Transaction,
    },
};
use cometbft::{
    block::Block as CometBlock, crypto::default::Sha256, merkle::simple_hash_from_byte_vectors,
};
use eyre::{bail, eyre, Context, Result};
use prost::Message;
use tokio::sync::{
    mpsc::{channel, Receiver, Sender},
    watch, Mutex,
};
use tracing::{error, info, warn};

use helios_core::{consensus::Consensus, time::interval};
use helios_exex_light_client::{
    builder::LightClientBuilder,
    components::{clock::SystemClock, scheduler::basic_bisecting_schedule},
    instance::Instance,
    store::memory::MemoryStore,
    verifier::{predicates::ProdPredicates, types::LightBlock, ProdVerifier},
};

use crate::{
    config::{Config, TrustOptions},
    database::Database,
    rpc::http_rpc::HttpRpc,
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
    block_recv: Option<Receiver<Block<Transaction>>>,
    finalized_block_recv: Option<watch::Receiver<Option<Block<Transaction>>>>,
    sync_status_recv: Mutex<watch::Receiver<ConsensusSyncStatus>>,
    shutdown_send: watch::Sender<bool>,
    expected_highest_block: Arc<AtomicU64>,
    chain_id: u64,
    phantom: PhantomData<DB>,
}

impl<DB: Database> ConsensusClient<DB> {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let db = Arc::new(DB::new(&config)?);
        let trust_options = db.load_trust_options()?;
        let (block_send, block_recv) = channel(256);
        let (finalized_block_send, finalized_block_recv) = watch::channel(None);
        let (sync_status_send, sync_status_recv) = watch::channel(ConsensusSyncStatus::Syncing);
        let (shutdown_send, mut shutdown_recv) = watch::channel(false);
        let expected_highest_block = Arc::new(AtomicU64::new(0));
        let expected_highest_block_task = expected_highest_block.clone();
        let consensus_rpc = config.consensus_rpc.clone();
        let verifier_options = config.verifier_options;

        #[cfg(not(target_arch = "wasm32"))]
        let run = tokio::spawn;

        #[cfg(target_arch = "wasm32")]
        let run = wasm_bindgen_futures::spawn_local;

        run(async move {
            let mut inner = match Inner::new(
                consensus_rpc,
                trust_options,
                verifier_options,
                block_send,
                finalized_block_send,
                expected_highest_block_task,
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
            expected_highest_block,
            chain_id: config.chain.chain_id,
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
        self.expected_highest_block.load(Ordering::Relaxed)
    }

    fn chain_id(&self) -> u64 {
        self.chain_id
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

struct Inner<DB: Database> {
    rpc: HttpRpc,
    light_client: Instance,
    last_comet_height: Option<cometbft::block::Height>,
    block_send: Sender<Block<Transaction>>,
    finalized_block_send: watch::Sender<Option<Block<Transaction>>>,
    expected_highest_block: Arc<AtomicU64>,
    db: Arc<DB>,
}

impl<DB: Database> Inner<DB> {
    async fn new(
        consensus_rpc: reqwest::Url,
        trust_options: TrustOptions,
        verifier_options: helios_exex_light_client::verifier::options::Options,
        block_send: Sender<Block<Transaction>>,
        finalized_block_send: watch::Sender<Option<Block<Transaction>>>,
        expected_highest_block: Arc<AtomicU64>,
        db: Arc<DB>,
    ) -> Result<Self> {
        let rpc = HttpRpc::connect(consensus_rpc).await?;
        let peer_id = rpc.peer_id();
        let light_client = LightClientBuilder::custom(
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

        Ok(Self {
            rpc,
            light_client,
            last_comet_height: None,
            block_send,
            finalized_block_send,
            expected_highest_block,
            db,
        })
    }

    async fn advance(&mut self) -> Result<()> {
        let light_block = self
            .light_client
            .light_client
            .verify_to_highest(&mut self.light_client.state)
            .await?;

        if self.last_comet_height == Some(light_block.height()) {
            return Ok(());
        }

        let block = fetch_execution_block(&self.rpc, &light_block).await?;
        self.last_comet_height = Some(light_block.height());
        self.db.save_trust_options(&TrustOptions {
            height: light_block.height(),
            hash: light_block.signed_header.header.hash_with::<Sha256>(),
        })?;

        let Some(block) = block else {
            return Ok(());
        };

        self.expected_highest_block
            .store(block.header.number, Ordering::Relaxed);
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
    rpc: &HttpRpc,
    light_block: &LightBlock,
) -> Result<Option<Block<Transaction>>> {
    let trusted_header = &light_block.signed_header.header;
    let block = rpc.fetch_block(trusted_header.height).await?;

    verify_block_data(&block, light_block)?;

    let Some(tx) = block.data.first() else {
        return Ok(None);
    };

    let payload = decode_execution_payload(tx)?;
    let parent_beacon_block_root = B256::try_from(trusted_header.app_hash.as_bytes())
        .map_err(|_| eyre!("CometBFT app hash is not 32 bytes"))?;

    payload_to_block(payload, parent_beacon_block_root).map(Some)
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
    let tx = ProtoTxRaw::decode(tx).wrap_err("failed to decode Cosmos SDK TxRaw")?;
    let body = ProtoTxBody::decode(tx.body_bytes.as_slice())
        .wrap_err("failed to decode Cosmos SDK TxBody")?;

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

    let payload = ProtoMsgExecutionPayload::decode(message.value.as_slice())
        .wrap_err("failed to decode MsgExecutionPayload")?;

    match (
        payload.execution_payload.is_empty(),
        payload.execution_payload_deneb,
    ) {
        (false, None) => serde_json::from_slice(&payload.execution_payload)
            .wrap_err("failed to decode legacy JSON execution payload"),
        (true, Some(payload)) => payload.try_into(),
        (false, Some(_)) => bail!("execution payload contains both legacy and protobuf forms"),
        (true, None) => bail!("execution payload is missing"),
    }
}

fn payload_to_block(
    payload: ExecutionPayloadV3,
    parent_beacon_block_root: B256,
) -> Result<Block<Transaction>> {
    let expected_hash = payload.payload_inner.payload_inner.block_hash;
    let cancun_fields = CancunPayloadFields::new(parent_beacon_block_root, Vec::new());
    let sidecar =
        ExecutionPayloadSidecar::v4(cancun_fields, PraguePayloadFields::new(Requests::default()));
    let block = ExecutionPayload::V3(payload)
        .try_into_block_with_sidecar::<TxEnvelope>(&sidecar)
        .wrap_err("failed to construct execution block from payload")?;
    let actual_hash = block.header.hash_slow();

    if actual_hash != expected_hash {
        bail!("execution payload block hash mismatch: expected {expected_hash}, got {actual_hash}");
    }

    let size = U256::from(block.length());
    let header = Header::from_consensus(
        block.header.clone().seal_slow(),
        Some(U256::ZERO),
        Some(size),
    );
    let block_number = block.header.number;
    let base_fee = block.header.base_fee_per_gas;
    let mut index = 0u64;
    let block = block.try_map_transactions(|tx| {
        let tx_info = TransactionInfo {
            hash: None,
            index: Some(index),
            block_hash: Some(actual_hash),
            block_number: Some(block_number),
            base_fee,
        };
        index += 1;

        tx.try_into_recovered()
            .map(|tx| Transaction::from_transaction(tx, tx_info))
    })?;
    let withdrawals = block.body.withdrawals;

    Ok(
        Block::new(header, BlockTransactions::Full(block.body.transactions))
            .with_withdrawals(withdrawals),
    )
}

impl TryFrom<ProtoExecutionPayloadDeneb> for ExecutionPayloadV3 {
    type Error = eyre::Error;

    fn try_from(payload: ProtoExecutionPayloadDeneb) -> Result<Self> {
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

        Ok(Self {
            payload_inner: ExecutionPayloadV2 {
                payload_inner: ExecutionPayloadV1 {
                    parent_hash: B256::from(fixed_bytes("parent hash", payload.parent_hash)?),
                    fee_recipient: Address::from(fixed_bytes(
                        "fee recipient",
                        payload.fee_recipient,
                    )?),
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
}

fn fixed_bytes<const N: usize>(field: &str, bytes: Vec<u8>) -> Result<[u8; N]> {
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| eyre!("{field} must be {N} bytes, found {}", bytes.len()))
}

#[derive(Clone, PartialEq, Message)]
struct ProtoTxRaw {
    #[prost(bytes = "vec", tag = "1")]
    body_bytes: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct ProtoTxBody {
    #[prost(message, repeated, tag = "1")]
    messages: Vec<ProtoAny>,
}

#[derive(Clone, PartialEq, Message)]
struct ProtoAny {
    #[prost(string, tag = "1")]
    type_url: String,
    #[prost(bytes = "vec", tag = "2")]
    value: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct ProtoMsgExecutionPayload {
    #[prost(bytes = "vec", tag = "2")]
    execution_payload: Vec<u8>,
    #[prost(message, optional, tag = "4")]
    execution_payload_deneb: Option<ProtoExecutionPayloadDeneb>,
}

#[derive(Clone, PartialEq, Message)]
struct ProtoExecutionPayloadDeneb {
    #[prost(bytes = "vec", tag = "1")]
    parent_hash: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    fee_recipient: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    state_root: Vec<u8>,
    #[prost(bytes = "vec", tag = "4")]
    receipts_root: Vec<u8>,
    #[prost(bytes = "vec", tag = "5")]
    logs_bloom: Vec<u8>,
    #[prost(bytes = "vec", tag = "6")]
    prev_randao: Vec<u8>,
    #[prost(uint64, tag = "7")]
    block_number: u64,
    #[prost(uint64, tag = "8")]
    gas_limit: u64,
    #[prost(uint64, tag = "9")]
    gas_used: u64,
    #[prost(uint64, tag = "10")]
    timestamp: u64,
    #[prost(bytes = "vec", tag = "11")]
    extra_data: Vec<u8>,
    #[prost(bytes = "vec", tag = "12")]
    base_fee_per_gas: Vec<u8>,
    #[prost(bytes = "vec", tag = "13")]
    block_hash: Vec<u8>,
    #[prost(bytes = "vec", repeated, tag = "14")]
    transactions: Vec<Vec<u8>>,
    #[prost(message, repeated, tag = "15")]
    withdrawals: Vec<ProtoWithdrawal>,
    #[prost(uint64, tag = "16")]
    blob_gas_used: u64,
    #[prost(uint64, tag = "17")]
    excess_blob_gas: u64,
}

#[derive(Clone, PartialEq, Message)]
struct ProtoWithdrawal {
    #[prost(uint64, tag = "1")]
    index: u64,
    #[prost(uint64, tag = "2")]
    validator_index: u64,
    #[prost(bytes = "vec", tag = "3")]
    address: Vec<u8>,
    #[prost(uint64, tag = "4")]
    amount: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_cometbft_transaction_ids_into_block_data_root() {
        let data_hash = block_data_hash(&[b"DATA"]);

        assert_eq!(
            data_hash,
            [
                0x18, 0x85, 0x4b, 0x00, 0x61, 0xe9, 0x49, 0x68, 0x3d, 0x59, 0xa4, 0xd6, 0xf0, 0xb8,
                0x19, 0x84, 0xa0, 0x84, 0x49, 0x93, 0xa6, 0xd8, 0x75, 0xf9, 0xd9, 0xb8, 0x2b, 0xe8,
                0x3b, 0x04, 0xa2, 0x30,
            ]
        );
    }

    #[test]
    fn decodes_protobuf_execution_payload_from_cosmos_tx_raw() {
        let payload = ProtoExecutionPayloadDeneb {
            parent_hash: vec![1; 32],
            fee_recipient: vec![2; 20],
            state_root: vec![3; 32],
            receipts_root: vec![4; 32],
            logs_bloom: vec![5; 256],
            prev_randao: vec![6; 32],
            block_number: 7,
            gas_limit: 8,
            gas_used: 9,
            timestamp: 10,
            extra_data: vec![11],
            base_fee_per_gas: vec![0; 32],
            block_hash: vec![12; 32],
            transactions: Vec::new(),
            withdrawals: vec![ProtoWithdrawal {
                index: 13,
                validator_index: 14,
                address: vec![15; 20],
                amount: 16,
            }],
            blob_gas_used: 17,
            excess_blob_gas: 18,
        };
        let message = ProtoMsgExecutionPayload {
            execution_payload: Vec::new(),
            execution_payload_deneb: Some(payload),
        };
        let body = ProtoTxBody {
            messages: vec![ProtoAny {
                type_url: EXECUTION_PAYLOAD_TYPE_URL.to_owned(),
                value: message.encode_to_vec(),
            }],
        };
        let tx = ProtoTxRaw {
            body_bytes: body.encode_to_vec(),
        };

        let payload = decode_execution_payload(&tx.encode_to_vec()).unwrap();
        let inner = payload.payload_inner.payload_inner;

        assert_eq!(inner.parent_hash, B256::repeat_byte(1));
        assert_eq!(inner.fee_recipient, Address::repeat_byte(2));
        assert_eq!(inner.block_number, 7);
        assert_eq!(payload.payload_inner.withdrawals[0].amount, 16);
        assert_eq!(payload.blob_gas_used, 17);
        assert_eq!(payload.excess_blob_gas, 18);
    }

    #[test]
    fn rejects_wrong_execution_payload_type_url() {
        let body = ProtoTxBody {
            messages: vec![ProtoAny {
                type_url: "/unexpected.Msg".to_owned(),
                value: Vec::new(),
            }],
        };
        let tx = ProtoTxRaw {
            body_bytes: body.encode_to_vec(),
        };

        let error = decode_execution_payload(&tx.encode_to_vec()).unwrap_err();
        assert!(error.to_string().contains("unexpected DATA message type"));
    }
}
