//! CallData Source

use crate::{
    errors::{PipelineError, PipelineResult},
    traits::{AsyncIterator, CelestiaProvider},
};
use alloc::{boxed::Box, collections::VecDeque, format, string::String, sync::Arc, vec};
use alloy_consensus::{Transaction, TxEnvelope};
use alloy_primitives::{Address, Bytes, TxKind};
use alloy_transport_http::reqwest::dns::Name;
use async_trait::async_trait;
use celestia_rpc::{BlobClient, Client};
use celestia_types::{nmt::Namespace, Blob, Commitment};
use kona_providers::ChainProvider;
use op_alloy_protocol::BlockInfo;

/// A data iterator that reads from calldata.
#[derive(Debug, Clone)]
pub struct CalldataSource<CP, CE>
where
    CP: ChainProvider + Send,
    CE: CelestiaProvider + Send,
{
    /// The chain provider to use for the calldata source.
    chain_provider: CP,
    /// The batch inbox address.
    batch_inbox_address: Address,
    /// Block Ref
    block_ref: BlockInfo,
    /// The L1 Signer.
    signer: Address,
    /// Current calldata.
    calldata: VecDeque<Bytes>,
    /// Whether the calldata source is open.
    open: bool,
    /// Celestia client used to perform celestia-node rpc requests
    celestia: CE,
    /// Namespace used to write and read data from
    namespace: Namespace,
}

impl<CP: ChainProvider + Send, CE: CelestiaProvider + Send> CalldataSource<CP, CE> {
    /// Creates a new calldata source.
    pub const fn new(
        chain_provider: CP,
        batch_inbox_address: Address,
        block_ref: BlockInfo,
        signer: Address,
        celestia: CE,
        namespace: Namespace,
    ) -> Self {
        Self {
            chain_provider,
            batch_inbox_address,
            block_ref,
            signer,
            calldata: VecDeque::new(),
            open: false,
            celestia,
            namespace,
        }
    }

    /// Loads the calldata into the source if it is not open.
    async fn load_calldata(&mut self) -> Result<(), CP::Error> {
        if self.open {
            return Ok(());
        }

        let (_, txs) =
            self.chain_provider.block_info_and_transactions_by_hash(self.block_ref.hash).await?;

        let mut results = VecDeque::new();
        for tx in txs.iter() {
            let (tx_kind, data) = match tx {
                TxEnvelope::Legacy(tx) => (tx.tx().to(), tx.tx().input()),
                TxEnvelope::Eip2930(tx) => (tx.tx().to(), tx.tx().input()),
                TxEnvelope::Eip1559(tx) => (tx.tx().to(), tx.tx().input()),
                _ => continue,
            };
            let TxKind::Call(to) = tx_kind else { continue };

            if to != self.batch_inbox_address {
                continue;
            }
            if tx.recover_signer().ok().is_none() || tx.recover_signer().unwrap() != self.signer {
                continue;
            }

            let result = match data[0] {
                0xce => {
                    let height_bytes = &data[1..65];
                    let height = u64::from_be_bytes(height_bytes.try_into().unwrap());
                    let commitment = Commitment(data[65..97].try_into().unwrap());

                    match self.celestia.blob_get(height, self.namespace, commitment).await {
                        Ok(blob) => blob,
                        Err(_) => continue,
                    }
                }
                _ => Bytes::from(data.to_vec()),
            };

            results.push_back(result);
        }

        self.calldata = results;
        self.open = true;

        Ok(())
    }
}

#[async_trait]
impl<CP: ChainProvider + Send, CE: CelestiaProvider + Send> AsyncIterator
    for CalldataSource<CP, CE>
{
    type Item = Bytes;

    async fn next(&mut self) -> PipelineResult<Self::Item> {
        if self.load_calldata().await.is_err() {
            return Err(PipelineError::Provider(format!(
                "Failed to load calldata for block {}",
                self.block_ref.hash
            ))
            .temp());
        }
        self.calldata.pop_front().ok_or(PipelineError::Eof.temp())
    }
}
