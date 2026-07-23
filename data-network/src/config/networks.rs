use std::fmt::Display;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use dirs::home_dir;
use eyre::Result;
use serde::{Deserialize, Serialize};
use strum::EnumIter;
use url::Url;

use helios_common::fork_schedule::ForkSchedule;
use phos_light_client::verifier::{options::Options, types::TrustThreshold};

use crate::config::base::BaseConfig;
use crate::config::types::ChainConfig;

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, EnumIter, Hash, Eq, PartialEq, PartialOrd, Ord,
)]
pub enum Network {
    Mainnet,
    Aeneid,
}

impl FromStr for Network {
    type Err = eyre::Report;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "mainnet" => Ok(Self::Mainnet),
            "aeneid" => Ok(Self::Aeneid),
            _ => Err(eyre::eyre!("network not recognized")),
        }
    }
}

impl Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            Self::Mainnet => "mainnet",
            Self::Aeneid => "aeneid",
        };

        f.write_str(str)
    }
}

impl Network {
    pub fn to_base_config(&self) -> BaseConfig {
        match self {
            Self::Mainnet => mainnet(),
            Self::Aeneid => aeneid(),
        }
    }

    pub fn from_chain_id(id: u64) -> Result<Self> {
        match id {
            1514 => Ok(Network::Mainnet),
            1315 => Ok(Network::Aeneid),
            _ => Err(eyre::eyre!("chain id not known")),
        }
    }
}

pub fn mainnet() -> BaseConfig {
    BaseConfig {
        rpc_port: 8545,
        consensus_rpc: Some(Url::parse("https://story-consensus-rpc.publicnode.com").unwrap()),
        execution_rpc: Some(Url::parse("https://story-rpc.publicnode.com").unwrap()),
        chain: ChainConfig { chain_id: 1514 },
        verifier_options: Options {
            trust_threshold: TrustThreshold::ONE_THIRD,
            trusting_period: Duration::from_secs(806_400),
            clock_drift: Duration::from_secs(5),
        },
        execution_forks: DataNetworkForkSchedule::mainnet(),
        #[cfg(not(target_arch = "wasm32"))]
        data_dir: Some(data_dir(Network::Mainnet)),
        ..std::default::Default::default()
    }
}

pub fn aeneid() -> BaseConfig {
    BaseConfig {
        rpc_port: 8545,
        consensus_rpc: None,
        chain: ChainConfig { chain_id: 1315 },
        verifier_options: Options {
            trust_threshold: TrustThreshold::ONE_THIRD,
            trusting_period: Duration::from_secs(806_400),
            clock_drift: Duration::from_secs(5),
        },
        execution_forks: DataNetworkForkSchedule::aeneid(),
        #[cfg(not(target_arch = "wasm32"))]
        data_dir: Some(data_dir(Network::Aeneid)),
        ..std::default::Default::default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn data_dir(network: Network) -> PathBuf {
    match home_dir() {
        Some(home) => home.join(format!(".helios/data/{network}")),
        None => std::env::temp_dir().join(format!("helios/data/{network}")),
    }
}

pub struct DataNetworkForkSchedule;

impl DataNetworkForkSchedule {
    fn mainnet() -> ForkSchedule {
        ForkSchedule {
            frontier_timestamp: 0,
            homestead_timestamp: 0,
            dao_timestamp: u64::MAX,
            tangerine_timestamp: 0,
            spurious_dragon_timestamp: 0,
            byzantium_timestamp: 0,
            constantinople_timestamp: 0,
            petersburg_timestamp: 0,
            istanbul_timestamp: 0,
            muir_glacier_timestamp: u64::MAX,
            berlin_timestamp: 0,
            london_timestamp: 0,
            arrow_glacier_timestamp: 0,
            gray_glacier_timestamp: 0,
            paris_timestamp: 0,
            shanghai_timestamp: 0,
            cancun_timestamp: 0,
            prague_timestamp: 1_751_934_608,
            osaka_timestamp: 1_768_435_200,

            ..Default::default()
        }
    }

    fn aeneid() -> ForkSchedule {
        ForkSchedule {
            frontier_timestamp: 0,
            homestead_timestamp: 0,
            dao_timestamp: u64::MAX,
            tangerine_timestamp: 0,
            spurious_dragon_timestamp: 0,
            byzantium_timestamp: 0,
            constantinople_timestamp: 0,
            petersburg_timestamp: 0,
            istanbul_timestamp: 0,
            muir_glacier_timestamp: u64::MAX,
            berlin_timestamp: 0,
            london_timestamp: 0,
            arrow_glacier_timestamp: 0,
            gray_glacier_timestamp: 0,
            paris_timestamp: 0,
            shanghai_timestamp: 0,
            cancun_timestamp: 0,
            prague_timestamp: 1_748_305_808,
            osaka_timestamp: 1_767_830_400,

            ..Default::default()
        }
    }
}
