use alloy_primitives::Bytes;
use anyhow::Error;
use async_trait::async_trait;
use celestia_rpc::{BlobClient, Client};
use celestia_types::{nmt::Namespace, Commitment};
use kona_derive::traits::CelestiaProvider;
use std::sync::Arc;

/// Online client to fetch data from a Celestia network
#[derive(Clone)]
pub struct CelestiaClient {
    /// The node client
    pub client: Arc<Client>,
    /// The namespace to fetch data from
    pub namespace: Namespace,
}

impl CelestiaClient {
    pub fn new(client: Client, namespace: Namespace) -> Self {
        CelestiaClient { client: Arc::new(client), namespace }
    }
}

// will need a trait for the "CelestiaClient"

impl core::fmt::Debug for CelestiaClient {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CelestiaClient")
            .field("namespace", &self.namespace)
            // Skip debugging the client field since it doesn't implement Debug
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl CelestiaProvider for CelestiaClient {
    type Error = anyhow::Error;

    async fn blob_get(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> Result<Bytes, Error> {
        match self.client.blob_get(height, namespace, commitment).await {
            Ok(blob) => {
                tracing::info!("Retrieved blob data for commitment {:?}.", blob.commitment);
                return Ok(Bytes::from(blob.data));
            }
            Err(e) => return Err(e.into()),
        }
    }
}
