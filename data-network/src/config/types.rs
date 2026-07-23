use phos_light_client::types::{Hash, Height};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ChainConfig {
    pub chain_id: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrustOptions {
    pub height: Height,
    pub hash: Hash,
}
