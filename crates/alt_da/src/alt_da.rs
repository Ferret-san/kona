//! Contains the [AltDADataSource], which is a concrete implementation of the
//! [DataAvailabilityProvider] trait for the AltDA source. Used as an adapter to the
//! [kona_derive] crate's derivation pipeline construction.

use crate::{source::AltDaSource, traits::AltDaInputFetcher};
use alloc::{boxed::Box, fmt::Debug};
use alloy_primitives::Bytes;
use anyhow::Result;
use async_trait::async_trait;
use kona_derive::traits::{ChainProvider, DataAvailabilityProvider};
use kona_primitives::BlockInfo;
use tracing_subscriber::field::debug::Alt;

/// The AltDA data source implements the [DataAvailabilityProvider] trait for the AltDA source.
#[derive(Debug, Clone, Copy)]
pub struct AltDaDataSource<C, F, I>
where
    C: ChainProvider + Send + Clone,
    F: AltDaInputFetcher<C> + Clone,
    I: Iterator<Item = Bytes> + Send + Clone,
{
    /// The chain provider.
    pub chain_provider: C,
    /// The altda input fetcher.
    pub altda_input_fetcher: F,
    /// The altda iterator.
    pub altda_source: I,
}

impl<C, F, I> AltDaDataSource<C, F, I>
where
    C: ChainProvider + Send + Clone + Debug,
    F: AltDaInputFetcher<C> + Clone,
    I: Iterator<Item = Bytes> + Send + Clone,
{
    /// Creates a new [AltDaSource] from the given providers.
    pub fn new(chain_provider: C, altda_input_fetcher: F, altda_source: I) -> Self {
        Self { chain_provider, altda_input_fetcher, altda_source }
    }
}

#[async_trait]
impl<C, F, I> DataAvailabilityProvider for AltDADataSource<C, F, I>
where
    C: ChainProvider + Send + Clone + Debug + Sync,
    F: AltDaInputFetcher<C> + Clone + Debug + Send + Sync,
    I: Iterator<Item = Bytes> + Send + Clone + Debug + Sync,
{
    type Item = Bytes;
    type DataIter = AltDaSource<C, F, I>;

    async fn open_data(&self, block_ref: &BlockInfo) -> Result<Self::DataIter> {
        Ok(AltDaSource::new(
            self.chain_provider.clone(),
            self.altda_input_fetcher.clone(),
            self.altda_source.clone(),
            block_ref.id(),
        ))
    }
}
