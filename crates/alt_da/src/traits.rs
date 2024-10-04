//! This module contains traits for the altda extension of the derivation pipeline.

use crate::types::{AltDaError, CommitmentData, FinalizedHeadSignal};
use alloc::boxed::Box;
use alloy_primitives::Bytes;
use async_trait::async_trait;
use kona_derive::traits::ChainProvider;
use kona_primitives::{BlockID, BlockInfo, SystemConfig};

/// A altda input fetcher.
#[async_trait]
pub trait AltDaInputFetcher<CP: ChainProvider + Send> {
    /// Get the input for the given commitment at the given block number from the DA storage
    /// service.
    async fn get_input(
        &mut self,
        fetcher: &CP,
        commitment: CommitmentData,
        block: BlockID,
    ) -> Result<Bytes, AltDaError>;

    /// Advance the L1 origin to the given block number, syncing the DA challenge events.
    async fn advance_l1_origin(&mut self, fetcher: &CP, block: BlockID) -> Result<(), AltDaError>;

    /// Reset the challenge origin in case of L1 reorg.
    pub fn reset(&mut self, base: BlockInfo, _base_cfg: SystemConfig);

    /// Notify L1 finalized head so altda finality is always behind L1.
    async fn finalize(&mut self, block_number: BlockInfo) -> Result<(), AltDaError>;

    /// Set the engine finalization signal callback.
    fn on_finalized_head_signal(&mut self, callback: FinalizedHeadSignal);
}
