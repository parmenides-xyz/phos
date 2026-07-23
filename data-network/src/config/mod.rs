use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::time::Duration;
use std::{path::PathBuf, process::exit};

use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use serde::Deserialize;
use url::Url;

use helios_common::fork_schedule::ForkSchedule;
use phos_light_client::verifier::{options::Options, types::TrustThreshold};

use self::base::BaseConfig;
use self::cli::CliConfig;
use self::networks::Network;

pub use self::types::{ChainConfig, TrustOptions};

pub mod cli;
pub mod networks;

mod base;
mod types;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub consensus_rpc: Url,
    pub execution_rpc: Option<Url>,
    pub verifiable_api: Option<Url>,
    pub rpc_bind_ip: Option<IpAddr>,
    pub rpc_port: Option<u16>,
    pub trust_options: Option<TrustOptions>,
    pub data_dir: Option<PathBuf>,
    pub chain: ChainConfig,
    pub verifier_options: Options,
    pub execution_forks: ForkSchedule,
    pub database_type: Option<String>,
}

impl Config {
    pub fn from_file(config_path: &PathBuf, network: &str, cli_config: &CliConfig) -> Self {
        let base_config = Network::from_str(network)
            .map(|n| n.to_base_config())
            .unwrap_or(BaseConfig::default());

        let base_provider = Serialized::from(base_config, network);
        let toml_provider = Toml::file(config_path).nested();
        let cli_provider = cli_config.as_provider(network);

        let config_res = Figment::new()
            .merge(base_provider)
            .merge(toml_provider)
            .merge(cli_provider)
            .select(network)
            .extract();

        match config_res {
            Ok(config) => config,
            Err(err) => {
                match err.kind {
                    figment::error::Kind::MissingField(field) => {
                        let field = field.replace('_', "-");
                        println!("\x1b[91merror\x1b[0m: missing configuration field: {field}");
                        println!("\n\ttry supplying the proper command line argument: --{field}");
                        println!("\talternatively, you can add the field to your helios.toml file");
                        println!("\nfor more information, check the github README");
                    }
                    _ => println!("cannot parse configuration: {err}"),
                }
                exit(1);
            }
        }
    }

    pub fn to_base_config(&self) -> BaseConfig {
        BaseConfig {
            rpc_bind_ip: self.rpc_bind_ip.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            rpc_port: self.rpc_port.unwrap_or(8545),
            consensus_rpc: Some(self.consensus_rpc.clone()),
            execution_rpc: self.execution_rpc.clone(),
            chain: self.chain.clone(),
            verifier_options: self.verifier_options,
            execution_forks: self.execution_forks,
            data_dir: self.data_dir.clone(),
        }
    }
}

impl From<BaseConfig> for Config {
    fn from(base: BaseConfig) -> Self {
        Self {
            rpc_bind_ip: Some(base.rpc_bind_ip),
            rpc_port: Some(base.rpc_port),
            consensus_rpc: base
                .consensus_rpc
                .unwrap_or_else(|| Url::parse("http://localhost:26657").unwrap()),
            execution_rpc: base.execution_rpc,
            verifiable_api: None,
            trust_options: None,
            data_dir: base.data_dir,
            chain: base.chain,
            verifier_options: base.verifier_options,
            execution_forks: base.execution_forks,
            database_type: None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            consensus_rpc: Url::parse("http://localhost:26657").unwrap(),
            execution_rpc: None,
            verifiable_api: None,
            rpc_bind_ip: None,
            rpc_port: None,
            trust_options: None,
            data_dir: None,
            chain: ChainConfig::default(),
            verifier_options: Options {
                trust_threshold: TrustThreshold::ONE_THIRD,
                trusting_period: Duration::ZERO,
                clock_drift: Duration::ZERO,
            },
            execution_forks: ForkSchedule::default(),
            database_type: None,
        }
    }
}
