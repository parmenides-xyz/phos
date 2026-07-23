extern crate console_error_panic_hook;
extern crate web_sys;

use alloy::hex;
use eyre::Result;
use wasm_bindgen::prelude::*;

use phos_data_network::{
    config::{Config, TrustOptions},
    database::Database,
};
use phos_light_client::types::{Hash, Height};

#[derive(Clone)]
pub struct LocalStorageDB {
    key: String,
    default_trust_options: Option<TrustOptions>,
}

impl Database for LocalStorageDB {
    fn new(config: &Config) -> Result<Self> {
        console_error_panic_hook::set_once();
        let window = web_sys::window().ok_or_else(|| eyre::eyre!("window not available"))?;
        if let Ok(Some(_local_storage)) = window.local_storage() {
            return Ok(Self {
                key: format!("phos-{}-trust-options", config.chain.chain_id),
                default_trust_options: config.trust_options.clone(),
            });
        }

        eyre::bail!("local_storage not available")
    }

    fn load_trust_options(&self) -> Result<TrustOptions> {
        let window = web_sys::window().ok_or_else(|| eyre::eyre!("window not available"))?;
        if let Ok(Some(local_storage)) = window.local_storage() {
            let encoded = local_storage.get_item(&self.key);
            if let Ok(Some(encoded)) = encoded {
                let bytes = hex::decode(encoded.strip_prefix("0x").unwrap_or(&encoded))?;
                if bytes.len() != 40 {
                    eyre::bail!("invalid stored trust options")
                }

                let height = Height::try_from(u64::from_be_bytes(bytes[..8].try_into()?))?;
                let hash = Hash::try_from(bytes[8..].to_vec())?;
                return Ok(TrustOptions { height, hash });
            }

            return self
                .default_trust_options
                .clone()
                .ok_or_else(|| eyre::eyre!("trust options not found"));
        }

        eyre::bail!("local_storage not available")
    }

    fn save_trust_options(&self, trust_options: &TrustOptions) -> Result<()> {
        let window = web_sys::window().ok_or_else(|| eyre::eyre!("window not available"))?;
        if let Ok(Some(local_storage)) = window.local_storage() {
            let mut bytes = trust_options.height.value().to_be_bytes().to_vec();
            bytes.extend_from_slice(trust_options.hash.as_bytes());
            local_storage
                .set_item(&self.key, &hex::encode(bytes))
                .unwrap_throw();
            return Ok(());
        }

        eyre::bail!("local_storage not available")
    }
}
