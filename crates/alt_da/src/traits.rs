//! This module contains traits for the altda extension of the derivation pipeline.

use core::fmt::Debug;

use crate::types::{AltDaError, CommitmentData, FinalizedHeadSignal};
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloy_eips::BlockNumHash;
use alloy_primitives::Bytes;
use async_trait::async_trait;
use kona_derive::pipeline::ChainProvider;
use op_alloy_genesis::SystemConfig;
use op_alloy_protocol::BlockInfo;

/// A altda input fetcher.
#[async_trait]
pub trait AltDaInputFetcher<CP: ChainProvider + Send> {
    /// Get the input for the given commitment at the given block number from the DA storage
    /// service.
    async fn get_input(
        &mut self,
        fetcher: &CP,
        commitment: Arc<Box<dyn CommitmentData + Send + Sync>>,
        block: BlockNumHash,
    ) -> Result<Bytes, AltDaError>;

    /// Advance the L1 origin to the given block number, syncing the DA challenge events.
    async fn advance_l1_origin(
        &mut self,
        fetcher: &CP,
        block: BlockNumHash,
    ) -> Result<(), AltDaError>;

    /// Reset the challenge origin in case of L1 reorg.
    fn reset(&mut self, base: BlockInfo, _base_cfg: SystemConfig);
}

/// Trait for calling the DA storage server
#[async_trait]
pub trait DAStorage: Send + Sync {
    /// gets inputs for a commitment/key from the altda storage
    async fn get_input(
        &self,
        key: Arc<Box<dyn CommitmentData + Send + Sync>>,
    ) -> Result<Bytes, AltDaError>;
}
