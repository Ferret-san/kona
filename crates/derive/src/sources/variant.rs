//! Data source

use alloc::boxed::Box;
use alloy_primitives::Bytes;
use async_trait::async_trait;
use kona_providers::ChainProvider;

use crate::{
    errors::PipelineResult,
    sources::{BlobSource, CalldataSource},
    traits::{AsyncIterator, BlobProvider, CelestiaProvider},
};

/// An enum over the various data sources.
#[derive(Debug, Clone)]
pub enum EthereumDataSourceVariant<CP, B, CE>
where
    CP: ChainProvider + Send + Sync,
    B: BlobProvider + Send + Sync,
    CE: CelestiaProvider + Send + Sync,
{
    /// A calldata source.
    Calldata(CalldataSource<CP, CE>),
    /// A blob source.
    Blob(BlobSource<CP, B, CE>),
}

#[async_trait]
impl<CP, B, CE> AsyncIterator for EthereumDataSourceVariant<CP, B, CE>
where
    CP: ChainProvider + Send + Sync,
    B: BlobProvider + Send + Sync,
    CE: CelestiaProvider + Send + Sync,
{
    type Item = Bytes;

    async fn next(&mut self) -> PipelineResult<Self::Item> {
        match self {
            Self::Calldata(c) => c.next().await,
            Self::Blob(b) => b.next().await,
        }
    }
}
