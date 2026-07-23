use std::default::Default;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::Duration;

use helios_common::fork_schedule::ForkSchedule;
use helios_exex_light_client::verifier::{options::Options, types::TrustThreshold};
use serde::Serialize;
use url::Url;

use crate::config::types::ChainConfig;

/// Base configuration for a network.
#[derive(Serialize)]
pub struct BaseConfig {
    pub rpc_bind_ip: IpAddr,
    pub rpc_port: u16,
    pub consensus_rpc: Option<Url>,
    pub execution_rpc: Option<Url>,
    pub chain: ChainConfig,
    pub verifier_options: Options,
    pub execution_forks: ForkSchedule,
    pub data_dir: Option<PathBuf>,
}

impl Default for BaseConfig {
    fn default() -> Self {
        Self {
            rpc_bind_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            rpc_port: 0,
            consensus_rpc: None,
            execution_rpc: None,
            chain: ChainConfig::default(),
            verifier_options: Options {
                trust_threshold: TrustThreshold::ONE_THIRD,
                trusting_period: Duration::ZERO,
                clock_drift: Duration::ZERO,
            },
            execution_forks: ForkSchedule::default(),
            data_dir: None,
        }
    }
}
