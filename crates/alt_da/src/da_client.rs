//! alt DA client
use crate::types::{
    decode_commitment_data, new_commitment_data, CommitmentData, CommitmentType, AltDaError,
};
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloy_primitives::hex;
use alloy_primitives::Bytes;
use reqwest::Client;

/// TODO (Diego): Add code to read config from a file / cli
#[derive(Debug)]
pub struct DAClient {
    url: String,
    verify: bool,
    precompute: bool,
    client: Client,
}

impl DAClient {
    pub fn new(url: String, verify: bool, precompute: bool) -> Self {
        DAClient { url, verify, precompute, client: Client::new() }
    }

    pub async fn get_input(&self, comm: &dyn CommitmentData) -> Result<Bytes, AltDaError> {
        let url = format!("{}/get/0x{}", self.url, hex::encode(comm.encode()));

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
            comm.verify(&input)?;
        }

        Ok(input)
    }

    pub async fn set_input(&self, img: &Bytes) -> Result<Box<dyn CommitmentData>, AltDaError> {
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
}
