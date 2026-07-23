#[cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};

#[cfg(not(target_arch = "wasm32"))]
use cometbft::hash::Algorithm;
use eyre::Result;
#[cfg(not(target_arch = "wasm32"))]
use phos_light_client::types::{Hash, Height};

use crate::config::{Config, TrustOptions};

pub trait Database: Clone + Sync + Send + 'static {
    fn new(config: &Config) -> Result<Self>
    where
        Self: Sized;

    fn save_trust_options(&self, trust_options: &TrustOptions) -> Result<()>;
    fn load_trust_options(&self) -> Result<TrustOptions>;
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct FileDB {
    data_dir: PathBuf,
    default_trust_options: Option<TrustOptions>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Database for FileDB {
    fn new(config: &Config) -> Result<Self> {
        if let Some(data_dir) = &config.data_dir {
            return Ok(Self {
                data_dir: data_dir.to_path_buf(),
                default_trust_options: config.trust_options.clone(),
            });
        }

        eyre::bail!("data dir not in config")
    }

    fn save_trust_options(&self, trust_options: &TrustOptions) -> Result<()> {
        fs::create_dir_all(&self.data_dir)?;

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(self.data_dir.join("trust_options"))?;

        file.write_all(&trust_options.height.value().to_be_bytes())?;
        file.write_all(trust_options.hash.as_bytes())?;

        Ok(())
    }

    fn load_trust_options(&self) -> Result<TrustOptions> {
        let mut buf = Vec::new();

        let result = fs::OpenOptions::new()
            .read(true)
            .open(self.data_dir.join("trust_options"))
            .map(|mut file| file.read_to_end(&mut buf));

        if buf.len() == 40 && result.is_ok() {
            let height = Height::try_from(u64::from_be_bytes(buf[..8].try_into()?))?;
            let hash = Hash::from_bytes(Algorithm::Sha256, &buf[8..])?;

            Ok(TrustOptions { height, hash })
        } else {
            self.default_trust_options
                .clone()
                .ok_or_else(|| eyre::eyre!("trust options not found"))
        }
    }
}

#[derive(Clone)]
pub struct ConfigDB {
    trust_options: TrustOptions,
}

impl Database for ConfigDB {
    fn new(config: &Config) -> Result<Self> {
        let trust_options = config
            .trust_options
            .clone()
            .ok_or_else(|| eyre::eyre!("CometBFT trust height and hash are required"))?;

        Ok(Self { trust_options })
    }

    fn save_trust_options(&self, _trust_options: &TrustOptions) -> Result<()> {
        Ok(())
    }

    fn load_trust_options(&self) -> Result<TrustOptions> {
        Ok(self.trust_options.clone())
    }
}
