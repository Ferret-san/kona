//! alt DA client
use crate::traits::DAStorage;
use crate::types::{
    decode_commitment_data, new_commitment_data, AltDaError, CommitmentData, CommitmentType,
};
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloy_primitives::hex;
use alloy_primitives::Bytes;
use async_trait::async_trait;
use reqwest::Client;

/// AltDA client to communicate with an altda server
#[derive(Debug)]
pub struct DAClient {
    url: String,
    verify: bool,
    precompute: bool,
    client: Client,
}

impl DAClient {
    /// Instantiates DAClient with a URL
    pub fn new(url: String, verify: bool, precompute: bool) -> Self {
        DAClient { url, verify, precompute, client: Client::new() }
    }

    async fn set_input_with_commit(
        &self,
        comm: &dyn CommitmentData,
        img: &Bytes,
    ) -> Result<(), AltDaError> {
        let key = comm.encode();
        let url = format!("{}/put/0x{}", self.url, hex::encode(&key));

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(img.to_vec())
            .send()
            .await
            .map_err(|_| AltDaError::NetworkError)?;

        if response.status() != reqwest::StatusCode::OK {
            return Err(AltDaError::FailedToGetPreimage);
        }

        Ok(())
    }

    /// Sets the input for commitment from the altda server
    async fn set_input(
        &self,
        img: &Bytes,
    ) -> Result<Box<dyn CommitmentData + Send + Sync>, AltDaError> {
        if img.is_empty() {
            return Err(AltDaError::InvalidInput);
        }

        let url = format!("{}/put/", self.url);

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(img.to_vec())
            .send()
            .await
            .map_err(|_| AltDaError::NetworkError)?;

        if response.status() != reqwest::StatusCode::OK {
            return Err(AltDaError::FailedToGetPreimage);
        }

        let body = Bytes::from(
            response.bytes().await.map_err(|_| AltDaError::FailedToGetPreimage)?.to_vec(),
        );

        let commitment = if self.precompute {
            new_commitment_data(CommitmentType::Keccak256, img)
        } else {
            decode_commitment_data(&body)?
        };

        if self.precompute {
            self.set_input_with_commit(&*commitment, img).await?;
        }

        Ok(commitment)
    }
}

#[async_trait]
impl DAStorage for DAClient {
    /// Fetches input from altDA server
    async fn get_input(
        &self,
        key: Arc<Box<dyn CommitmentData + Send + Sync>>,
    ) -> Result<Bytes, AltDaError> {
        let url = format!("{}/get/0x{}", self.url, hex::encode(key.encode()));

        let response = self.client.get(&url).send().await.map_err(|_| AltDaError::NetworkError)?;

        match response.status() {
            reqwest::StatusCode::NOT_FOUND => return Err(AltDaError::NotFound),
            reqwest::StatusCode::OK => {}
            _ => return Err(AltDaError::FailedToGetPreimage),
        }

        let input = Bytes::from(
            response.bytes().await.map_err(|_| AltDaError::FailedToGetPreimage)?.to_vec(),
        );

        if self.verify {
            key.verify(&input)?;
        }

        Ok(input)
    }
}
