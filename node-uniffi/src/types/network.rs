use phos_data_network::config::networks::Network as DataNetwork;

/// DATA Network chain to connect to.
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum Network {
    Mainnet,
    Aeneid,
}

impl Network {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Mainnet => "mainnet",
            Self::Aeneid => "aeneid",
        }
    }
}

impl From<Network> for DataNetwork {
    fn from(network: Network) -> Self {
        match network {
            Network::Mainnet => Self::Mainnet,
            Network::Aeneid => Self::Aeneid,
        }
    }
}
