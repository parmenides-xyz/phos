use std::net::IpAddr;
use std::{
    path::PathBuf,
    process::exit,
    str::FromStr,
    sync::{Arc, Mutex},
};

use clap::{Args, Parser, Subcommand};
use dirs::home_dir;
use eyre::Result;
use futures::executor::block_on;
use tracing::{error, info};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use tracing_subscriber::FmtSubscriber;
use url::Url;

use helios_common::network_spec::NetworkSpec;
use helios_core::client::HeliosClient;
use helios_exex_data_network::{
    config::{cli::CliConfig, Config, TrustOptions},
    DataNetworkClient, DataNetworkClientBuilder,
};
use helios_exex_light_client::types::{Hash, Height};

mod tui;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if !cli.tui {
        enable_tracer();
    }

    match cli.command {
        Command::Node(data_network) => {
            let client = data_network.make_client();
            if cli.tui {
                if let Err(e) = tui::run(client).await {
                    error!(target: "helios::tui", "TUI error: {}", e);
                    exit(1);
                }
            } else {
                register_shutdown_handler(client);
                std::future::pending().await
            }
        }
    }

    Ok(())
}

fn enable_tracer() {
    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env()
        .expect("invalid env filter");

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(env_filter)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("subscriber set failed");
}

fn register_shutdown_handler<N: NetworkSpec>(client: HeliosClient<N>) {
    let shutdown_counter = Arc::new(Mutex::new(0));

    ctrlc::set_handler(move || {
        let mut counter = shutdown_counter.lock().unwrap();
        *counter += 1;

        let counter_value = *counter;

        if counter_value == 3 {
            info!(target: "helios::runner", "forced shutdown");
            exit(0);
        }

        info!(
            target: "helios::runner",
            "shutting down... press ctrl-c {} more times to force quit",
            3 - counter_value
        );

        if counter_value == 1 {
            let client = client.clone();
            std::thread::spawn(move || {
                block_on(client.shutdown());
                exit(0);
            });
        }
    })
    .expect("could not register shutdown handler");
}

#[derive(Parser)]
#[command(version, about)]
/// Phos is a fast, secure, and portable DATA Network light client.
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, global = true, help = "Enable terminal user interface")]
    tui: bool,
}

#[derive(Subcommand)]
enum Command {
    #[command(name = "node")]
    Node(DataNetworkArgs),
}

#[derive(Args)]
struct DataNetworkArgs {
    #[arg(short, long, default_value = "mainnet")]
    network: String,
    #[arg(short = 'b', long, env)]
    rpc_bind_ip: Option<IpAddr>,
    #[arg(short = 'p', long, env)]
    rpc_port: Option<u16>,
    #[arg(long, env, value_parser = parse_height, requires = "trust_hash")]
    trust_height: Option<Height>,
    #[arg(long, env, requires = "trust_height")]
    trust_hash: Option<Hash>,
    #[arg(short, long, env, value_parser = parse_url)]
    execution_rpc: Option<Url>,
    #[arg(short, long, env, value_parser = parse_url)]
    verifiable_api: Option<Url>,
    #[arg(short, long, env, value_parser = parse_url)]
    consensus_rpc: Option<Url>,
    #[arg(short, long, env)]
    data_dir: Option<String>,
}

impl DataNetworkArgs {
    fn make_client(&self) -> DataNetworkClient {
        let config_path = home_dir().unwrap().join(".helios-exex/helios.toml");
        let cli_config = self.as_cli_config();
        let config = Config::from_file(&config_path, &self.network, &cli_config);

        match DataNetworkClientBuilder::new()
            .with_file_db()
            .config(config)
            .build()
        {
            Ok(client) => client,
            Err(err) => {
                error!(target: "helios::runner", error = %err);
                exit(1);
            }
        }
    }

    fn as_cli_config(&self) -> CliConfig {
        CliConfig {
            execution_rpc: self.execution_rpc.clone(),
            verifiable_api: self.verifiable_api.clone(),
            consensus_rpc: self.consensus_rpc.clone(),
            trust_options: self
                .trust_height
                .zip(self.trust_hash)
                .map(|(height, hash)| TrustOptions { height, hash }),
            rpc_bind_ip: self.rpc_bind_ip,
            rpc_port: self.rpc_port,
            data_dir: self
                .data_dir
                .as_ref()
                .map(|path| PathBuf::from_str(path).expect("cannot find data dir")),
        }
    }
}

fn parse_height(s: &str) -> Result<Height, String> {
    let height = s
        .parse::<u64>()
        .map_err(|err| format!("invalid trust height: {err}"))?;

    Height::try_from(height).map_err(|err| format!("invalid trust height: {err}"))
}

fn parse_url(s: &str) -> Result<Url, url::ParseError> {
    Url::parse(s)
}
