//! Module contains an online implementation of the AltDA Input Fetcher.

use core::fmt::Debug;

use crate::{
    state::{ChallengeStatus, State},
    traits::{AltDaInputFetcher, DAStorage},
    types::{
        decode_challenge_status_event, decode_commitment_data, decode_resolved_input, AltDaError,
        CommitmentData, CommitmentType, FinalizedHeadSignal, InputFetcherConfig,
        CHALLENGE_STATUS_EVENT_ABI,
    },
};
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::{boxed::Box, string::ToString};
use alloy_consensus::Transaction;
use alloy_eips::BlockNumHash;
use alloy_primitives::{keccak256, Bytes};
use alloy_rpc_types::Log;
use async_trait::async_trait;
use kona_common::io;
use kona_derive::{errors::PipelineError, pipeline::ChainProvider};
use kona_providers_alloy::AlloyChainProvider;
use op_alloy_genesis::SystemConfig;
use op_alloy_protocol::BlockInfo;
use tokio::sync::Mutex;

/// An AltDa Provider.
#[derive(Clone, Debug)]
pub struct AltDAProvider<D>
where
    D: DAStorage + Clone + Debug,
{
    cfg: InputFetcherConfig,
    storage: D,
    state: State,

    challenge_origin: BlockNumHash,
    commitment_origin: BlockNumHash,
    finalized_head: BlockInfo,
    l1_finalized_head: BlockInfo,

    resetting: bool,

    finalized_head_signal_handler: Option<FinalizedHeadSignal>,
}

impl<D> AltDAProvider<D>
where
    D: DAStorage + Clone + Debug,
{
    pub fn new_alt_da(cfg: InputFetcherConfig, storage: D) -> Self {
        AltDAProvider {
            cfg: cfg.clone(),
            storage,
            state: State::new(cfg.clone()),
            challenge_origin: BlockNumHash::default(),
            commitment_origin: BlockNumHash::default(),
            finalized_head: BlockInfo::default(),
            l1_finalized_head: BlockInfo::default(),
            resetting: false,
            finalized_head_signal_handler: None,
        }
    }

    pub fn new_alt_da_with_state(cfg: InputFetcherConfig, storage: D, state: State) -> Self {
        AltDAProvider {
            cfg,
            storage,
            state,
            challenge_origin: BlockNumHash::default(),
            commitment_origin: BlockNumHash::default(),
            finalized_head: BlockInfo::default(),
            l1_finalized_head: BlockInfo::default(),
            resetting: false,
            finalized_head_signal_handler: None,
        }
    }

    pub fn on_finalized_head_signal(&mut self, f: FinalizedHeadSignal) {
        self.finalized_head_signal_handler = Some(f);
    }

    pub fn update_finalized_head(&mut self, l1_finalized: BlockInfo) {
        self.l1_finalized_head = l1_finalized;
        self.state.prune(l1_finalized.id());
        self.finalized_head = self.state.last_pruned_commitment.unwrap_or_default();
    }

    pub async fn update_finalized_from_l1(
        &mut self,
        l1: &AlloyChainProvider,
    ) -> Result<(), AltDaError> {
        if self.l1_finalized_head.number < self.cfg.challenge_window {
            return Ok(());
        }
        let ref_block = match l1
            .clone()
            .block_info_by_number(self.l1_finalized_head.number - self.cfg.challenge_window)
            .await
        {
            Ok(b) => b,
            Err(_) => return Err(AltDaError::NetworkError),
        };
        self.finalized_head = ref_block;
        Ok(())
    }

    pub async fn look_ahead(&mut self, l1: &AlloyChainProvider) -> Result<(), AltDaError> {
        let blk_ref = match l1.clone().block_info_by_number(self.challenge_origin.number + 1).await
        {
            Ok(b) => b,
            Err(_) => return Err(AltDaError::NetworkError),
        };
        self.advance_challenge_origin(l1, blk_ref.id()).await
    }

    pub async fn advance_challenge_origin(
        &mut self,
        l1: &AlloyChainProvider,
        block: BlockNumHash,
    ) -> Result<(), AltDaError> {
        if block.number <= self.challenge_origin.number {
            return Ok(());
        }

        self.load_challenge_events(l1, block).await?;
        self.state.expire_challenges(block);
        self.challenge_origin = block;
        Ok(())
    }

    pub async fn advance_commitment_origin(
        &mut self,
        block: BlockNumHash,
    ) -> Result<(), AltDaError> {
        if block.number <= self.commitment_origin.number {
            return Ok(());
        }

        if let Err(e) = self.state.expire_commitments(block) {
            self.resetting = true;
            return Err(e);
        }

        self.commitment_origin = block;
        Ok(())
    }

    async fn load_challenge_events(
        &mut self,
        l1: &AlloyChainProvider,
        block: BlockNumHash,
    ) -> Result<(), AltDaError> {
        let logs = self.fetch_challenge_logs(l1, block).await?;

        for log in logs {
            let (status, comm, bn) = match self.decode_challenge_status(&log) {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!("failed to decode challenge event");
                    continue;
                }
            };

            match status {
                ChallengeStatus::Resolved => {
                    let (_, txs) =
                        l1.clone().block_info_and_transactions_by_hash(block.hash).await.unwrap();
                    let tx_index = match log.transaction_index {
                        Some(index) => index,
                        None => {
                            tracing::error!("could not find transaction in log");
                            continue;
                        }
                    };
                    let tx = &txs[tx_index as usize];
                    let tx_hash = match log.transaction_hash {
                        Some(hash) => hash,
                        None => {
                            tracing::error!("could not get transaction hash from log");
                            continue;
                        }
                    };
                    if tx.tx_hash() != &tx_hash {
                        tracing::error!("tx hash mismatch");
                        continue;
                    }

                    // TODO (Diego): decode generic commitments

                    let input = if self.cfg.commitment_type == CommitmentType::Keccak256 {
                        match decode_resolved_input(tx.input()) {
                            Ok(input) => {
                                if let Err(e) = comm.verify(&input) {
                                    tracing::error!(
                                        "failed to verify commitmen: {0}",
                                        comm.to_string()
                                    );
                                    continue;
                                }
                                Some(Bytes::from(input))
                            }
                            Err(e) => {
                                tracing::error!("failed to decode resolved input");
                                continue;
                            }
                        }
                    } else {
                        None
                    };

                    tracing::info!("challenge resolved for block: {0}", block.hash);
                    if let Err(e) = self.state.resolve_challenge(Arc::new(comm), bn, input.unwrap())
                    {
                        tracing::error!("failed to resolve challenge for block: {0}", block.number);
                    }
                }
                ChallengeStatus::Active => {
                    tracing::info!(
                        "detected new active challenge for block {0} and commitment {1}",
                        block.number,
                        comm.to_string()
                    );
                    self.state.create_challenge(Arc::new(comm), block, bn);
                }
                _ => {
                    tracing::warn!(
                        "skipping unknown challengeblock {0} and commitment {1}",
                        block.number,
                        comm.to_string()
                    );
                }
            }
        }
        Ok(())
    }

    async fn fetch_challenge_logs(
        &self,
        l1: &AlloyChainProvider,
        block: BlockNumHash,
    ) -> Result<Vec<Log>, AltDaError> {
        if self.cfg.commitment_type == CommitmentType::Generic {
            return Ok(Vec::new());
        }
        let receipts = match l1.clone().receipts_by_hash(block.hash).await {
            Ok(r) => r,
            Err(_) => return Err(AltDaError::NetworkError),
        };

        let (block_info, transactions) =
            match l1.clone().block_info_and_transactions_by_hash(block.hash).await {
                Ok((info, txs)) => (info, txs),
                Err(_) => return Err(AltDaError::NetworkError),
            };

        let mut logs = Vec::new();
        for (tx_index, (receipt, tx_envelope)) in
            receipts.into_iter().zip(transactions.into_iter()).enumerate()
        {
            for (log_index, log) in receipt.logs.into_iter().enumerate() {
                if log.address == self.cfg.da_challenge_contract
                    && !log.topics().is_empty()
                    && log.topics()[0] == keccak256(CHALLENGE_STATUS_EVENT_ABI.as_bytes())
                {
                    logs.push(Log {
                        block_hash: Some(block.hash),
                        block_number: Some(block_info.number),
                        transaction_hash: Some(*tx_envelope.tx_hash()),
                        transaction_index: Some(tx_index as u64),
                        log_index: Some(log_index as u64),
                        removed: false,
                        block_timestamp: Some(block_info.timestamp), // Removed Some()
                        inner: log,
                    })
                }
            }
        }
        Ok(logs)
    }

    fn decode_challenge_status(
        &self,
        log: &Log,
    ) -> Result<(ChallengeStatus, Box<dyn CommitmentData>, u64), AltDaError> {
        let (challenged_commitment, block_number, status) =
            decode_challenge_status_event(log).map_err(|e| AltDaError::DecodeError)?;

        // Decode the commitment data
        let commitment = decode_commitment_data(&challenged_commitment)
            .map_err(|e| AltDaError::InvalidCommitment)?;

        let challenge_status = match status {
            0 => ChallengeStatus::Uninitialized,
            1 => ChallengeStatus::Active,
            2 => ChallengeStatus::Resolved,
            3 => ChallengeStatus::Expired,
            _ => return Err(AltDaError::UnknownStatus),
        };
        Ok((challenge_status, commitment, block_number))
    }

    async fn finalize(&mut self, block_number: BlockInfo) {
        self.update_finalized_head(block_number);

        tracing::info!("received l1 finalized signal, forwarding altda finalization to finalizedHeadSignalHandler");

        if let Some(handler) = &self.finalized_head_signal_handler {
            handler.call(self.finalized_head);
        } else {
            tracing::warn!("finalized head signal handler not set");
        }
    }
}

/// Move code from the impl to the
#[async_trait]
impl<D> AltDaInputFetcher<AlloyChainProvider> for AltDAProvider<D>
where
    D: DAStorage + Clone + Debug,
{
    async fn get_input(
        &mut self,
        fetcher: &AlloyChainProvider,
        commitment: Arc<Box<dyn CommitmentData + Send + Sync>>,
        block: BlockNumHash,
    ) -> Result<Bytes, AltDaError> {
        if self.cfg.commitment_type != commitment.commitment_type() {
            return Err(AltDaError::ChallengeExpired);
        }
        let status = self.state.get_challenge_status(commitment.clone(), block.number);
        if status == ChallengeStatus::Expired {
            return Err(AltDaError::ChallengeExpired);
        }

        let block_info = match fetcher.clone().block_info_by_number(block.number).await {
            Ok(b) => b,
            Err(_) => return Err(AltDaError::NetworkError),
        };

        // Use Arc and Mutex to make the closure Send
        self.state.track_commitment(commitment.clone(), block_info);

        match self.storage.get_input(commitment.clone()).await {
            Ok(data) => Ok(data),
            Err(AltDaError::NotFound) => match status {
                ChallengeStatus::Uninitialized => {
                    if self.challenge_origin.number > block.number + self.cfg.challenge_window {
                        Err(AltDaError::MissingPastWindow)
                    } else {
                        self.look_ahead(fetcher).await?;
                        Err(AltDaError::ChallengePending)
                    }
                }
                ChallengeStatus::Active => {
                    self.look_ahead(fetcher).await?;
                    Err(AltDaError::ChallengePending)
                }
                ChallengeStatus::Resolved => {
                    if commitment.commitment_type() == CommitmentType::Generic {
                        Err(AltDaError::MissingPastWindow)
                    } else {
                        let ch = self.state.get_challenge(commitment, block.number).unwrap();
                        Ok(ch.input.clone().unwrap())
                    }
                }
                _ => Err(AltDaError::UnknownStatus),
            },
            Err(e) => Err(e),
        }
    }

    async fn advance_l1_origin(
        &mut self,
        l1: &AlloyChainProvider,
        block: BlockNumHash,
    ) -> Result<(), AltDaError> {
        self.advance_challenge_origin(l1, block).await?;
        self.advance_commitment_origin(block).await?;

        if self.state.no_commitments() {
            self.update_finalized_from_l1(l1).await?;
        }
        Ok(())
    }

    fn reset(&mut self, base: BlockInfo, _base_cfg: SystemConfig) {
        if self.resetting {
            self.resetting = false;
            self.commitment_origin = base.id();
            self.state.clear_commitments();
        } else {
            self.challenge_origin = base.id();
            self.commitment_origin = base.id();
            self.state.reset();
        }
        tracing::error!("{}", PipelineError::Eof);
    }
}
