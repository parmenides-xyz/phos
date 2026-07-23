use helios_common::types::SubscriptionEvent;
use phos_data_network::spec::DataNetwork;
use tokio::sync::Mutex;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_stream::StreamExt;
use uniffi::Object;

#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum SubscriptionError {
    /// Receiver lagged too far behind and skipped subscription items.
    #[error("Subscription receiver lagged and skipped {0} items")]
    Lagged(u64),
    /// Unexpected subscription stream close.
    #[error("Subscription stream ended")]
    StreamEnded,
    /// Error serializing a block.
    #[error("Unable to serialize block: {0}")]
    Serialization(String),
}

#[derive(Object)]
pub struct NewHeadsStream(Mutex<BroadcastStream<SubscriptionEvent<DataNetwork>>>);

impl NewHeadsStream {
    pub(crate) fn new(stream: BroadcastStream<SubscriptionEvent<DataNetwork>>) -> Self {
        Self(Mutex::new(stream))
    }
}

#[uniffi::export]
impl NewHeadsStream {
    /// Returns the next verified EVM block serialized as JSON.
    pub async fn next(&self) -> std::result::Result<String, SubscriptionError> {
        let event = self
            .0
            .lock()
            .await
            .next()
            .await
            .ok_or(SubscriptionError::StreamEnded)?
            .map_err(|error| match error {
                BroadcastStreamRecvError::Lagged(skipped) => SubscriptionError::Lagged(skipped),
            })?;

        match event {
            SubscriptionEvent::NewHeads(block) => serde_json::to_string(&block)
                .map_err(|error| SubscriptionError::Serialization(error.to_string())),
        }
    }
}
