//! Contains the concrete implementation of the [DAStorage] trait for the client program.

use crate::{l1::OracleL1ChainProvider, HintType};
use alloc::vec;
use alloc::{boxed::Box, sync::Arc};
use alloy_eips::BlockNumHash;
use alloy_primitives::keccak256;
use alloy_primitives::Bytes;
use anyhow::Result;
use async_trait::async_trait;
use kona_altda::traits::{AltDaInputFetcher, DAStorage};
use kona_altda::types::{AltDaError, CommitmentData};
use kona_altda::AltDAProvider;
use kona_preimage::{CommsClient, PreimageKey, PreimageKeyType};
use kona_preimage::{HintWriterClient, PreimageOracleClient};
use op_alloy_genesis::SystemConfig;
use op_alloy_protocol::BlockInfo;

/// An oracle-backed da storage.
#[derive(Debug, Clone)]
pub struct OracleDaStorage<T: CommsClient> {
    oracle: Arc<T>,
}

impl<T: CommsClient + Clone> OracleDaStorage<T> {
    /// Constructs a new `OracleBlobProvider`.
    pub fn new(oracle: Arc<T>) -> Self {
        Self { oracle }
    }

    /// Retrieves data from an altDA commitment
    async fn get_commitment_data(
        &self,
        key: Arc<Box<dyn CommitmentData + Send + Sync>>,
    ) -> Result<Bytes> {
        let l2_input_req_meta = key.encode();

        // send a hint for altda commitment
        self.oracle.write(&HintType::L2Input.encode_with(&[l2_input_req_meta.as_ref()])).await?;

        // TODO (Diego) hash and fetch commitment to make sure it's stored?

        let data = &mut vec![];
        // fetch the data behind the commitment (keccak(commitment), where commitment is (altda_commitment | sender_address))
        self.oracle
            .get_exact(
                PreimageKey::new(*keccak256(l2_input_req_meta), PreimageKeyType::GlobalGeneric),
                data,
            )
            .await?;

        tracing::info!(target: "altda_oracle", "Retrieved altda data {key:?} from the oracle.");
        Ok(Bytes::copy_from_slice(data.as_ref()))
    }
}

#[async_trait]
impl<T: CommsClient + Sync + Send> DAStorage for OracleDaStorage<T> {
    async fn get_input(
        &self,
        key: Arc<Box<dyn CommitmentData + Send + Sync>>,
    ) -> Result<Bytes, AltDaError> {
        let data = match self.get_commitment_data(key).await {
            Ok(preimage) => preimage,
            Err(_) => return Err(AltDaError::FailedToGetPreimage),
        };
        Ok(data)
    }
}

#[async_trait]
impl<O, C> AltDaInputFetcher<OracleL1ChainProvider<C>> for AltDAProvider<OracleDaStorage<O>>
where
    O: CommsClient + Sync + Send + Clone + core::fmt::Debug,
    C: PreimageOracleClient + HintWriterClient + Sync + Send + Clone,
{
    // Implement the methods here, converting between ChainProviderAdapter and AlloyChainProvider as needed
    async fn get_input(
        &mut self,
        fetcher: &OracleL1ChainProvider<C>,
        commitment: Arc<Box<dyn CommitmentData + Send + Sync>>,
        block: BlockNumHash,
    ) -> Result<Bytes, AltDaError> {
        match self.get_input(fetcher, commitment, block).await {
            Ok(result) => return Ok(result),
            Err(err) => return Err(err),
        }
    }

    /// Note, challenges are ignored for this current implementation

    /// Advance the L1 origin to the given block number, syncing the DA challenge events.
    async fn advance_l1_origin(
        &mut self,
        _fetcher: &OracleL1ChainProvider<C>,
        _block: BlockNumHash,
    ) -> Result<(), AltDaError> {
        Ok(())
    }

    /// Reset the challenge origin in case of L1 reorg.
    fn reset(&mut self, _base: BlockInfo, _base_cfg: SystemConfig) {}
}
