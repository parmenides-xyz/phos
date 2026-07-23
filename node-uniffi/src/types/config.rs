use std::path::Path;
use std::str::FromStr;

use phos_data_network::config::{Config, TrustOptions};
use phos_data_network::database::{ConfigDB, FileDB};
use phos_data_network::{DataNetworkClient, DataNetworkClientBuilder};
use phos_light_client::types::{Hash, Height};
use uniffi::Record;
use url::Url;

use crate::error::{DataNetworkError, Result};
use crate::types::Network;

/// Configuration options for the DATA Network light client.
#[derive(Debug, Clone, Record)]
pub struct NodeConfig {
    /// Base path for storing the latest verified trust options.
    /// If this is not set, trust options are kept in memory.
    pub base_path: Option<String>,
    /// Network to connect to.
    pub network: Network,
    /// EVM execution JSON-RPC endpoint.
    pub execution_rpc: Option<String>,
    /// Optional verifiable execution API endpoint.
    pub verifiable_api: Option<String>,
    /// CometBFT consensus JSON-RPC endpoint.
    pub consensus_rpc: Option<String>,
    /// Initially trusted CometBFT height.
    pub trust_height: Option<u64>,
    /// Initially trusted CometBFT block hash.
    pub trust_hash: Option<String>,
}

impl NodeConfig {
    /// Convert into a DATA Network client for the implementation.
    pub(crate) async fn into_client(self) -> Result<DataNetworkClient> {
        let network = phos_data_network::config::networks::Network::from(self.network);
        let base = network.to_base_config();

        let consensus_rpc = match self.consensus_rpc {
            Some(rpc) => parse_url(&rpc, "consensus RPC")?,
            None => base
                .consensus_rpc
                .ok_or_else(|| DataNetworkError::invalid_config("consensus RPC is required"))?,
        };
        let execution_rpc = self
            .execution_rpc
            .map(|rpc| parse_url(&rpc, "execution RPC"))
            .transpose()?;
        let verifiable_api = self
            .verifiable_api
            .map(|rpc| parse_url(&rpc, "verifiable API"))
            .transpose()?;
        let trust_options = parse_trust_options(self.trust_height, self.trust_hash)?;
        let data_dir = self
            .base_path
            .map(|path| Path::new(&path).join(format!("store-{}", self.network.as_str())));

        let config = Config {
            consensus_rpc,
            execution_rpc,
            verifiable_api,
            rpc_bind_ip: None,
            rpc_port: None,
            trust_options,
            data_dir: data_dir.clone(),
            chain: base.chain,
            verifier_options: base.verifier_options,
            execution_forks: base.execution_forks,
            database_type: None,
        };

        if data_dir.is_some() {
            DataNetworkClientBuilder::<FileDB>::new()
                .config(config)
                .build()
                .map_err(|error| DataNetworkError::client(error.to_string()))
        } else {
            DataNetworkClientBuilder::<ConfigDB>::new()
                .config(config)
                .build()
                .map_err(|error| DataNetworkError::client(error.to_string()))
        }
    }
}

fn parse_url(value: &str, name: &str) -> Result<Url> {
    Url::parse(value)
        .map_err(|error| DataNetworkError::invalid_config(format!("invalid {name}: {error}")))
}

fn parse_trust_options(
    trust_height: Option<u64>,
    trust_hash: Option<String>,
) -> Result<Option<TrustOptions>> {
    match (trust_height, trust_hash) {
        (Some(height), Some(hash)) => {
            let height = Height::try_from(height).map_err(|error| {
                DataNetworkError::invalid_config(format!("invalid trust height: {error}"))
            })?;
            let hash = hash
                .strip_prefix("0x")
                .unwrap_or(&hash)
                .to_ascii_uppercase();
            let hash = Hash::from_str(&hash).map_err(|error| {
                DataNetworkError::invalid_config(format!("invalid trust hash: {error}"))
            })?;
            Ok(Some(TrustOptions { height, hash }))
        }
        (None, None) => Ok(None),
        _ => Err(DataNetworkError::invalid_config(
            "trust height and trust hash must be provided together",
        )),
    }
}
