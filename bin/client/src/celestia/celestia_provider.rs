//! Contains the concrete implementation of the [CelestiaProvider] trait for the client program.

use core::fmt::Display;

use crate::{l1::OracleL1ChainProvider, HintType};
use alloc::{boxed::Box, sync::Arc};
use alloc::{vec, vec::Vec};
use alloy_eips::BlockNumHash;
use alloy_primitives::keccak256;
use alloy_primitives::Bytes;
use anyhow::{Error, Result};
use async_trait::async_trait;
use celestia_types::{nmt::Namespace, Commitment};
use kona_derive::traits::CelestiaProvider;
use kona_preimage::{CommsClient, PreimageKey, PreimageKeyType};
use kona_preimage::{HintWriterClient, PreimageOracleClient};
use op_alloy_genesis::SystemConfig;
use op_alloy_protocol::BlockInfo;

/// An oracle-backed da storage.
#[derive(Debug, Clone)]
pub struct OracleCelestiaProvider<T: CommsClient> {
    oracle: Arc<T>,
}

impl<T: CommsClient + Clone> OracleCelestiaProvider<T> {
    /// Constructs a new `OracleBlobProvider`.
    pub fn new(oracle: Arc<T>) -> Self {
        Self { oracle }
    }

    /// Retrieves data from an altDA commitment
    async fn blob_get(
        &self,
        height: u64,
        _namespace: Namespace,
        commitment: Commitment,
    ) -> Result<Bytes, Error> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&height.to_be_bytes());
        encoded.extend_from_slice(&commitment.0);

        // send a hint for altda commitment
        self.oracle.write(&HintType::L2CelestiaInput.encode_with(&[encoded.as_ref()])).await?;

        let data = &mut vec![];
        // fetch the data behind the keccak256(height, commitment) key
        self.oracle
            .get_exact(PreimageKey::new(*keccak256(encoded), PreimageKeyType::GlobalGeneric), data)
            .await?;

        tracing::info!(target: "celestia_oracle", "Retrieved celestia data {:?} from the oracle.", commitment);
        Ok(Bytes::copy_from_slice(data.as_ref()))
    }
}

#[async_trait]
impl<T: CommsClient + Sync + Send> CelestiaProvider for OracleCelestiaProvider<T> {
    type Error = anyhow::Error;

    async fn blob_get(
        &self,
        height: u64,
        namespace: Namespace,
        commitment: Commitment,
    ) -> Result<Bytes, Self::Error> {
        self.blob_get(height, namespace, commitment).await
    }
}
