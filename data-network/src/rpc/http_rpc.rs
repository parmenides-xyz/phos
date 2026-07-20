#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use async_trait::async_trait;
use cometbft_rpc::{
    self as rpc, client::Client, request::RequestMessage, response::Response, SimpleRequest,
};
use reqwest::header;

use helios_exex_light_client::{
    components::io::{AtHeight, Io, IoError},
    verifier::types::{Height, LightBlock, PeerId, SignedHeader, ValidatorAddress, ValidatorSet},
};

#[derive(Clone, Debug)]
struct HttpTransport {
    inner: reqwest::Client,
    url: reqwest::Url,
}

impl HttpTransport {
    fn new(url: reqwest::Url) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let inner = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        #[cfg(target_arch = "wasm32")]
        let inner = reqwest::Client::new();

        Self { inner, url }
    }

    fn build_request<R>(&self, request: R) -> Result<reqwest::Request, rpc::Error>
    where
        R: RequestMessage,
    {
        let request_body = request.into_json();

        self.inner
            .post(self.url.clone())
            .header(header::CONTENT_TYPE, "application/json")
            .body(request_body.into_bytes())
            .build()
            .map_err(rpc::Error::http)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Client for HttpTransport {
    async fn perform<R>(&self, request: R) -> Result<R::Output, rpc::Error>
    where
        R: SimpleRequest,
    {
        let request = self.build_request(request)?;
        let response = self
            .inner
            .execute(request)
            .await
            .map_err(rpc::Error::http)?;
        let response_status = response.status();
        let response_body = response.bytes().await.map_err(rpc::Error::http)?;

        if response_status != reqwest::StatusCode::OK {
            return Err(rpc::Error::http_request_failed(response_status));
        }

        R::Response::from_string(&response_body).map(Into::into)
    }
}

#[derive(Clone, Debug)]
pub struct HttpRpc {
    transport: HttpTransport,
    peer_id: PeerId,
}

impl HttpRpc {
    pub async fn connect(url: reqwest::Url) -> Result<Self, rpc::Error> {
        let transport = HttpTransport::new(url);
        let peer_id = transport.status().await?.node_info.id;

        Ok(Self { transport, peer_id })
    }

    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    pub async fn fetch_signed_header(&self, height: AtHeight) -> Result<SignedHeader, IoError> {
        let response = match height {
            AtHeight::Highest => self.transport.latest_commit().await,
            AtHeight::At(height) => self.transport.commit(height).await,
        }
        .map_err(IoError::from_rpc)?;

        Ok(response.signed_header)
    }

    pub async fn fetch_validator_set(
        &self,
        height: AtHeight,
        proposer_address: Option<ValidatorAddress>,
    ) -> Result<ValidatorSet, IoError> {
        let height = match height {
            AtHeight::Highest => return Err(IoError::invalid_height()),
            AtHeight::At(height) => height,
        };

        let response = self
            .transport
            .validators(height, rpc::Paging::All)
            .await
            .map_err(IoError::rpc)?;

        match proposer_address {
            Some(proposer_address) => {
                ValidatorSet::with_proposer(response.validators, proposer_address)
                    .map_err(IoError::invalid_validator_set)
            }
            None => Ok(ValidatorSet::without_proposer(response.validators)),
        }
    }

    pub async fn fetch_block(&self, height: Height) -> Result<cometbft::block::Block, IoError> {
        self.transport
            .block(height)
            .await
            .map(|response| response.block)
            .map_err(IoError::from_rpc)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Io for HttpRpc {
    async fn fetch_light_block(&self, height: AtHeight) -> Result<LightBlock, IoError> {
        let signed_header = self.fetch_signed_header(height).await?;
        let height = signed_header.header.height;
        let proposer_address = signed_header.header.proposer_address;

        let validator_set = self
            .fetch_validator_set(height.into(), Some(proposer_address))
            .await?;
        let next_validator_set = self
            .fetch_validator_set(height.increment().into(), None)
            .await?;

        Ok(LightBlock::new(
            signed_header,
            validator_set,
            next_validator_set,
            self.peer_id,
        ))
    }
}
