//! Test utilities for the `kona-altda` crate.

use crate::{
    traits::AltDaInputFetcher,
    types::{AltDaError, FinalizedHeadSignal},
};
use alloc::{boxed::Box, vec::Vec};
use alloy_primitives::Bytes;
use async_trait::async_trait;
use kona_derive::traits::test_utils::TestChainProvider;
use kona_primitives::{BlockID, BlockInfo, SystemConfig};

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
        _commitment: Bytes,
        _block: BlockID,
    ) -> Option<Result<Bytes, AltDaError>> {
        self.inputs.pop()
    }

    async fn advance_l1_origin(
        &mut self,
        _fetcher: &TestChainProvider,
        _block: BlockID,
    ) -> Option<Result<(), AltDaError>> {
        self.advances.pop()
    }

    async fn reset(
        &mut self,
        _block_number: BlockInfo,
        _cfg: SystemConfig,
    ) -> Option<Result<(), AltDaError>> {
        self.resets.pop()
    }

    async fn finalize(&mut self, _block_number: BlockInfo) -> Option<Result<(), AltDaError>> {
        None
    }

    fn on_finalized_head_signal(&mut self, _block_number: FinalizedHeadSignal) {}
}
