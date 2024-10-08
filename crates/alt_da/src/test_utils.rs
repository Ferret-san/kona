//! Test utilities for the `kona-altda` crate.

use crate::{
    traits::AltDaInputFetcher,
    types::{AltDaError, CommitmentData, FinalizedHeadSignal},
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use alloy_eips::BlockNumHash;
use alloy_primitives::Bytes;
use async_trait::async_trait;
use kona_derive::traits::test_utils::TestChainProvider;
use op_alloy_genesis::SystemConfig;
use op_alloy_protocol::BlockInfo;

/// A mock altda input fetcher for testing.
#[derive(Debug, Clone, Default)]
pub struct TestAltDAInputFetcher {
    /// Inputs to return.
    pub inputs: Vec<Result<Bytes, AltDaError>>,
    /// Advance L1 origin results.
    pub advances: Vec<Result<(), AltDaError>>,
    /// Reset results.
    pub resets: Vec<Result<(), AltDaError>>,
}

#[async_trait]
impl AltDaInputFetcher<TestChainProvider> for TestAltDAInputFetcher {
    async fn get_input(
        &mut self,
        _fetcher: &TestChainProvider,
        _commitment: Arc<Box<dyn CommitmentData + Send + Sync>>,
        _block: BlockNumHash,
    ) -> Result<Bytes, AltDaError> {
        self.inputs.pop().unwrap()
    }

    async fn advance_l1_origin(
        &mut self,
        _fetcher: &TestChainProvider,
        _block: BlockNumHash,
    ) -> Result<(), AltDaError> {
        self.advances.pop().unwrap()
    }

    fn reset(&mut self, _block_number: BlockInfo, _cfg: SystemConfig) {}

    async fn finalize(&mut self, _block_number: BlockInfo) {}

    fn on_finalized_head_signal(&mut self, _block_number: FinalizedHeadSignal) {}
}
