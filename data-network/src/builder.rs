use std::{marker::PhantomData, sync::Arc};
#[cfg(not(target_arch = "wasm32"))]
use std::{net::SocketAddr, path::PathBuf};

use eyre::{eyre, Result};
use reqwest::{IntoUrl, Url};

use helios_core::execution::{
    cache::CachingProvider,
    providers::{
        block::block_cache::BlockCache, historical::eip2935::Eip2935Provider,
        rpc::RpcExecutionProvider, verifiable_api::VerifiableApiExecutionProvider,
    },
};

#[cfg(not(target_arch = "wasm32"))]
use crate::database::FileDB;
use crate::{
    config::{networks::Network, Config, TrustOptions},
    consensus::ConsensusClient,
    database::{ConfigDB, Database},
    spec::DataNetwork,
    DataNetworkClient,
};

pub struct DataNetworkClientBuilder<DB: Database> {
    network: Option<Network>,
    consensus_rpc: Option<Url>,
    execution_rpc: Option<Url>,
    verifiable_api: Option<Url>,
    trust_options: Option<TrustOptions>,
    #[cfg(not(target_arch = "wasm32"))]
    rpc_address: Option<SocketAddr>,
    #[cfg(not(target_arch = "wasm32"))]
    data_dir: Option<PathBuf>,
    config: Option<Config>,
    phantom: PhantomData<DB>,
}

impl<DB: Database> Default for DataNetworkClientBuilder<DB> {
    fn default() -> Self {
        Self {
            network: None,
            consensus_rpc: None,
            execution_rpc: None,
            verifiable_api: None,
            trust_options: None,
            #[cfg(not(target_arch = "wasm32"))]
            rpc_address: None,
            #[cfg(not(target_arch = "wasm32"))]
            data_dir: None,
            config: None,
            phantom: PhantomData,
        }
    }
}

impl<DB: Database> DataNetworkClientBuilder<DB> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn network(mut self, network: Network) -> Self {
        self.network = Some(network);
        self
    }

    pub fn consensus_rpc<T: IntoUrl>(mut self, consensus_rpc: T) -> Result<Self> {
        self.consensus_rpc = Some(
            consensus_rpc
                .into_url()
                .map_err(|_| eyre!("Invalid consensus RPC URL"))?,
        );
        Ok(self)
    }

    pub fn execution_rpc<T: IntoUrl>(mut self, execution_rpc: T) -> Result<Self> {
        self.execution_rpc = Some(
            execution_rpc
                .into_url()
                .map_err(|_| eyre!("Invalid execution RPC URL"))?,
        );
        Ok(self)
    }

    pub fn verifiable_api<T: IntoUrl>(mut self, verifiable_api: T) -> Result<Self> {
        self.verifiable_api = Some(
            verifiable_api
                .into_url()
                .map_err(|_| eyre!("Invalid verifiable API URL"))?,
        );
        Ok(self)
    }

    pub fn trust_options(mut self, trust_options: TrustOptions) -> Self {
        self.trust_options = Some(trust_options);
        self
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn rpc_address(mut self, rpc_address: SocketAddr) -> Self {
        self.rpc_address = Some(rpc_address);
        self
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn data_dir(mut self, data_dir: PathBuf) -> Self {
        self.data_dir = Some(data_dir);
        self
    }

    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    pub fn build(self) -> Result<DataNetworkClient> {
        let base_config = if let Some(network) = self.network {
            network.to_base_config()
        } else {
            let config = self
                .config
                .as_ref()
                .ok_or(eyre!("missing network config"))?;
            config.to_base_config()
        };

        let consensus_rpc = self
            .consensus_rpc
            .or_else(|| {
                self.config
                    .as_ref()
                    .map(|config| config.consensus_rpc.clone())
            })
            .or_else(|| base_config.consensus_rpc.clone())
            .ok_or_else(|| eyre!("missing consensus rpc"))?;

        let execution_rpc = self
            .execution_rpc
            .or_else(|| {
                self.config
                    .as_ref()
                    .and_then(|config| config.execution_rpc.clone())
            })
            .or_else(|| base_config.execution_rpc.clone());

        let verifiable_api = self.verifiable_api.or_else(|| {
            self.config
                .as_ref()
                .and_then(|config| config.verifiable_api.clone())
        });

        let trust_options = self.trust_options.or_else(|| {
            self.config
                .as_ref()
                .and_then(|config| config.trust_options.clone())
        });

        #[cfg(not(target_arch = "wasm32"))]
        let data_dir = if self.data_dir.is_some() {
            self.data_dir
        } else if let Some(config) = &self.config {
            config.data_dir.clone()
        } else {
            base_config.data_dir.clone()
        };

        #[cfg(not(target_arch = "wasm32"))]
        let rpc_address = if let Some(address) = self.rpc_address {
            Some(address)
        } else if let Some(config) = &self.config {
            config
                .rpc_bind_ip
                .zip(config.rpc_port)
                .map(|(ip, port)| SocketAddr::new(ip, port))
        } else {
            Some(SocketAddr::new(
                base_config.rpc_bind_ip,
                base_config.rpc_port,
            ))
        };

        let database_type = self
            .config
            .as_ref()
            .and_then(|config| config.database_type.clone());

        let config = Config {
            consensus_rpc,
            execution_rpc,
            verifiable_api,
            rpc_bind_ip: None,
            rpc_port: None,
            trust_options,
            #[cfg(not(target_arch = "wasm32"))]
            data_dir,
            #[cfg(target_arch = "wasm32")]
            data_dir: None,
            chain: base_config.chain,
            verifier_options: base_config.verifier_options,
            execution_forks: base_config.execution_forks,
            database_type,
        };

        let config = Arc::new(config);
        let consensus = ConsensusClient::<DB>::new(config.clone())?;

        if let Some(verifiable_api) = &config.verifiable_api {
            let block_provider = BlockCache::<DataNetwork>::new();
            let historical_provider = Eip2935Provider::new();
            let execution = VerifiableApiExecutionProvider::with_historical_provider(
                verifiable_api,
                block_provider,
                historical_provider,
            );
            let execution = CachingProvider::new(execution);

            Ok(DataNetworkClient::new(
                consensus,
                execution,
                config.execution_forks,
                #[cfg(not(target_arch = "wasm32"))]
                rpc_address,
            ))
        } else {
            let block_provider = BlockCache::<DataNetwork>::new();
            let rpc_url = config
                .execution_rpc
                .as_ref()
                .ok_or_else(|| eyre!("missing execution rpc"))?
                .clone();
            let historical_provider = Eip2935Provider::new();
            let execution = RpcExecutionProvider::with_historical_provider(
                rpc_url,
                block_provider,
                historical_provider,
            );
            let execution = CachingProvider::new(execution);

            Ok(DataNetworkClient::new(
                consensus,
                execution,
                config.execution_forks,
                #[cfg(not(target_arch = "wasm32"))]
                rpc_address,
            ))
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DataNetworkClientBuilder<FileDB> {
    pub fn with_file_db(self) -> Self {
        self
    }
}

impl DataNetworkClientBuilder<ConfigDB> {
    pub fn with_config_db(self) -> Self {
        self
    }
}
