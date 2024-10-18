//! Contains the [AltDADataSource], which is a concrete implementation of the
//! [DataAvailabilityProvider] trait for the AltDA source. Used as an adapter to the
//! [kona_derive] crate's derivation pipeline construction.

use crate::{source::AltDaSource, traits::AltDaInputFetcher};
use alloc::{boxed::Box, fmt::Debug};
use alloy_primitives::Bytes;
use anyhow::Result;
use async_trait::async_trait;
use kona_derive::errors::PipelineErrorKind;
use kona_derive::pipeline::ChainProvider;
use kona_derive::sources::{
    BlobSource, CalldataSource, EthereumDataSource, EthereumDataSourceVariant,
};
use kona_derive::traits::BlobProvider;
use kona_derive::traits::DataAvailabilityProvider;
use op_alloy_protocol::BlockInfo;

/// The AltDA data source implements the [DataAvailabilityProvider] trait for the AltDA source.
#[derive(Debug, Clone, Copy)]
pub struct AltDaDataSource<C, B, F>
where
    C: ChainProvider + Send + Clone,
    B: BlobProvider + Clone,
    F: AltDaInputFetcher<C>,
{
    /// The chain provider.
    pub chain_provider: C,
    /// The altda input fetcher.
    pub altda_input_fetcher: F,
    /// The ethereum source.
    pub ethereum_source: EthereumDataSource<C, B>,
}

impl<C, B, F> AltDaDataSource<C, B, F>
where
    C: ChainProvider + Send + Clone,
    B: BlobProvider + Clone,
    F: AltDaInputFetcher<C>,
{
    /// Creates a new [AltDaSource] from the given providers.
    pub fn new(
        chain_provider: C,
        altda_input_fetcher: F,
        ethereum_source: EthereumDataSource<C, B>,
    ) -> Self {
        Self { chain_provider, altda_input_fetcher, ethereum_source }
    }
}

#[async_trait]
impl<C, B, F> DataAvailabilityProvider for AltDaDataSource<C, B, F>
where
    C: ChainProvider + Send + Clone + Debug + Sync,
    B: BlobProvider + Send + Clone + Debug + Sync,
    F: AltDaInputFetcher<C> + Clone + Debug + Send + Sync,
{
    type Item = Bytes;
    type DataIter = AltDaSource<C, B, F>;

    async fn open_data(&self, block_ref: &BlockInfo) -> Result<Self::DataIter, PipelineErrorKind> {
        let ecotone_enabled = self
            .ethereum_source
            .ecotone_timestamp
            .map(|e| block_ref.timestamp >= e)
            .unwrap_or(false);
        if ecotone_enabled {
            Ok(AltDaSource::new(
                self.chain_provider.clone(),
                self.altda_input_fetcher.clone(),
                EthereumDataSourceVariant::Blob(BlobSource::new(
                    self.chain_provider.clone(),
                    self.ethereum_source.blob_provider.clone(),
                    self.ethereum_source.batch_inbox_address,
                    *block_ref,
                    self.ethereum_source.signer,
                )),
                block_ref.id(),
            ))
        } else {
            Ok(AltDaSource::new(
                self.chain_provider.clone(),
                self.altda_input_fetcher.clone(),
                EthereumDataSourceVariant::Calldata(CalldataSource::new(
                    self.chain_provider.clone(),
                    self.ethereum_source.batch_inbox_address,
                    *block_ref,
                    self.ethereum_source.signer,
                )),
                block_ref.id(),
            ))
        }
    }
}
