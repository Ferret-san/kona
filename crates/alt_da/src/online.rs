//! Module contains an online implementation of the AltDA Input Fetcher.

use crate::{
    state::{ChallengeStatus, State},
    traits::AltDaInputFetcher,
    types::{
        decode_challenge_status_event, decode_commitment_data, decode_resolved_input, AltDaError,
        CommitmentData, CommitmentType, FinalizedHeadSignal, CHALLENGE_STATUS_EVENT_ABI_HASH,
    },
};
use alloc::boxed::Box;
use alloc::vec::Vec;
use alloy_primitives::{keccak256, Bytes, Log, B256, U256};
use async_trait::async_trait;
use kona_common::io;
use kona_derive::{online::AlloyChainProvider, traits::ChainProvider};
use kona_primitives::{Block, BlockID, BlockInfo, SystemConfig};

/// Trait for calling the DA storage server
#[async_trait]
pub trait DAStorage: Send + Sync {
    async fn get_input(&self, key: &dyn CommitmentData) -> Result<Bytes, AltDaError>;
    async fn set_input(&self, img: &Bytes) -> Result<Box<dyn CommitmentData>, AltDaError>;
}

/// Config struct
#[derive(Debug, Clone)]
pub struct InputFetcherConfig {
    // Used to filtercontract events
    pub da_challenge_contract: alloy_primitives::Address,
    // Allowed commitment type for the input fetcher
    pub commitment_type: CommitmentType,
    // The number of l1 blocks after the input is committed during which one can challenge.
    pub challenge_window: u64,
    // The number of l1 blocksafter a commitmnet s challenged during which one can resolve.
    pub resolve_window: u64,
}
/// An Online AltDa Input Fetcher.
#[derive(Debug, Clone)]
pub struct OnlineAltDAInputFetcher {
    cfg: InputFetcherConfig,
    storage: Arc<dyn DAStorage>,
    state: State,

    challenge_origin: BlockID,
    commitment_origin: BlockID,
    finalized_head: BlockInfo,
    l1_finalized_head: BlockInfo,

    resetting: bool,

    finalized_head_signal_handler: FinalizedHeadSignal,
}

impl OnlineAltDAInputFetcher {
    pub fn new_alt_da(log: Logger, cfg: Config, storage: Arc<dyn DAStorage>) -> Self {
        OnlineAltDAInputFetcher {
            cfg,
            storage,
            state: State::new(log.clone(), cfg.clone()),
            challenge_origin: BlockID::default(),
            commitment_origin: BlockID::default(),
            finalized_head: L1BlockRef::default(),
            l1_finalized_head: L1BlockRef::default(),
            resetting: false,
            finalized_head_signal_handler: None,
        }
    }

    pub fn new_alt_da_with_state(
        log: Logger,
        cfg: Config,
        storage: Arc<dyn DAStorage>,
        state: State,
    ) -> Self {
        OnlineAltDAInputFetcher {
            cfg,
            storage,
            state,
            challenge_origin: BlockID::default(),
            commitment_origin: BlockID::default(),
            finalized_head: L1BlockRef::default(),
            l1_finalized_head: L1BlockRef::default(),
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
        let ref_block = l1
            .block_info_by_number(self.l1_finalized_head.number - self.cfg.challenge_window)
            .await?;
        self.finalized_head = ref_block;
        Ok(())
    }

    pub async fn look_ahead(&mut self, l1: &AlloyChainProvider) -> Result<(), AltDaError> {
        let blk_ref = l1.block_info_by_number(self.challenge_origin.number + 1).await?;
        self.advance_challenge_origin(l1, blk_ref.id()).await
    }

    pub async fn advance_challenge_origin(
        &mut self,
        l1: &AlloyChainProvider,
        block: BlockID,
    ) -> Result<(), AltDaError> {
        if block.number <= self.challenge_origin.number {
            return Ok(());
        }

        self.load_challenge_events(l1, block).await?;
        self.state.expire_challenges(block);
        self.challenge_origin = block;
        tracing::info!(self.log, "processed altda challenge origin"; "origin" => ?block);
        Ok(())
    }

    pub async fn advance_commitment_origin(
        &mut self,
        l1: &AlloyChainProvider,
        block: BlockID,
    ) -> Result<(), AltDaError> {
        if block.number <= self.commitment_origin.number {
            return Ok(());
        }

        if let Err(e) = self.state.expire_commitments(block) {
            self.resetting = true;
            return Err(e);
        }

        self.commitment_origin = block;
        tracing::info!(self.log, "processed altda l1 origin";
            "origin" => ?block,
            "finalized" => ?self.finalized_head.id(),
            "l1-finalize" => ?self.l1_finalized_head.id()
        );
        Ok(())
    }

    async fn load_challenge_events(
        &mut self,
        l1: &AlloyChainProvider,
        block: BlockID,
    ) -> Result<(), AltDaError> {
        let logs = self.fetch_challenge_logs(l1, block).await?;

        for log in logs {
            let (status, comm, bn) = match self.decode_challenge_status(&log) {
                Ok(result) => result,
                Err(e) => {
                    error!(self.log, "failed to decode challenge event";
                        "block" => block.number,
                        "tx" => log.transaction_index,
                        "log" => log.log_index,
                        "err" => ?e
                    );
                    continue;
                }
            };

            match status {
                ChallengeStatus::Resolved => {
                    let (_, txs) = l1.info_and_txs_by_hash(block.hash).await?;
                    let tx = &txs[log.transaction_index as usize];
                    if tx.hash() != log.transaction_hash {
                        error!(self.log, "tx hash mismatch";
                            "block" => block.number,
                            "txIdx" => log.transaction_index,
                            "log" => log.log_index,
                            "txHash" => ?tx.hash(),
                            "receiptTxHash" => ?log.transaction_hash
                        );
                        continue;
                    }

                    let input = if self.cfg.commitment_type == CommitmentType::Keccak256 {
                        match decode_resolved_input(tx.input.as_ref()) {
                            Ok(input) => {
                                if let Err(e) = comm.verify(&input) {
                                    error!(self.log, "failed to verify commitment";
                                        "block" => block.number,
                                        "txIdx" => log.transaction_index,
                                        "err" => ?e
                                    );
                                    continue;
                                }
                                Some(input)
                            }
                            Err(e) => {
                                error!(self.log, "failed to decode resolved input";
                                    "block" => block.number,
                                    "txIdx" => log.transaction_index,
                                    "err" => ?e
                                );
                                continue;
                            }
                        }
                    } else {
                        None
                    };

                    tracing::info!(self.log, "challenge resolved"; "block" => ?block, "txIdx" => log.transaction_index);
                    if let Err(e) = self.state.resolve_challenge(&*comm, block, bn, input) {
                        error!(self.log, "failed to resolve challenge";
                            "block" => block.number,
                            "txIdx" => log.transaction_index,
                            "err" => ?e
                        );
                    }
                }
                ChallengeStatus::Active => {
                    tracing::info!(self.log, "detected new active challenge"; "block" => ?block, "comm" => ?comm);
                    self.state.create_challenge(comm, block, bn);
                }
                _ => {
                    tracing::warn!(self.log, "skipping unknown challenge status";
                        "block" => block.number,
                        "tx" => log.transaction_index,
                        "log" => log.log_index,
                        "status" => ?status,
                        "comm" => ?comm
                    );
                }
            }
        }
        Ok(())
    }

    async fn fetch_challenge_logs(
        &self,
        l1: &AlloyChainProvider,
        block: BlockID,
    ) -> Result<Vec<Log>, AltDaError> {
        if self.cfg.commitment_type == CommitmentType::Generic {
            return Ok(Vec::new());
        }
        let receipts = l1.receipts_by_hash(block.hash).await?;
        let mut logs = Vec::new();
        for receipt in receipts {
            for log in receipt.logs {
                if log.address == self.cfg.da_challenge_contract
                    && !log.topics().is_empty()
                    && log.topics()[0] == *CHALLENGE_STATUS_EVENT_ABI_HASH
                {
                    logs.push(log);
                }
            }
        }
        Ok(logs)
    }

    fn decode_challenge_status(
        &self,
        log: &Log,
    ) -> Result<(ChallengeStatus, Box<dyn CommitmentData>, u64), AltDaError> {
        let (challenged_commitment, block_number, status) = decode_challenge_status_event(log)
            .map_err(|e| AltDaError::DecodeError(e.to_string()))?;

        // Decode the commitment data
        let commitment = decode_commitment_data(&challenged_commitment)
            .map_err(|e| AltDaError::InvalidCommitment)?;

        Ok((challenge_status, commitment, block_number))
    }
}

/// Move code from the impl to the
#[async_trait]
impl AltDaInputFetcher<AlloyChainProvider> for OnlineAltDAInputFetcher {
    pub async fn get_input(
        &self,
        l1: &AlloyChainProvider,
        comm: &dyn CommitmentData,
        block_id: BlockID,
    ) -> Result<Bytes, AltDaError> {
        if self.cfg.commitment_type != comm.commitment_type() {
            return AltDaError::ChallengeExpired;
        }
        let status = self.state.get_challenge_status(comm, block_id.number);
        if status == ChallengeStatus::Expired {
            return AltDaError::ChallengeExpired;
        }
        self.state.track_commitment(comm.clone(), block_id);
        tracing::info!(self.log, "getting input"; "comm" => ?comm, "status" => ?status);

        match self.storage.get_input(comm).await {
            Ok(data) => Ok(data),
            Err(AltDaError::NotFound) => {
                tracing::warn!(self.log, "data not found for the given commitment"; "comm" => ?comm, "status" => ?status, "block" => block_id.number);
                match status {
                    ChallengeStatus::Uninitialized => {
                        if self.challenge_origin.number
                            > block_id.number + self.cfg.challenge_window
                        {
                            Err(Box::new(ErrMissingPastWindow))
                        } else {
                            self.look_ahead(l1).await?;
                            Err(Box::new(ErrPendingChallenge))
                        }
                    }
                    ChallengeStatus::Active => {
                        self.look_ahead(l1).await?;
                        Err(Box::new(ErrPendingChallenge))
                    }
                    ChallengeStatus::Resolved => {
                        if comm.commitment_type() == CommitmentType::Generic {
                            Err(Box::new(ErrMissingPastWindow))
                        } else {
                            let ch = self.state.get_challenge(comm, block_id.number).unwrap();
                            Ok(ch.input.clone().unwrap())
                        }
                    }
                    _ => Err(Box::new(io::Error::new(
                        io::ErrorKind::Other,
                        format!("unknown challenge status: {:?}", status),
                    ))),
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn advance_l1_origin(
        &mut self,
        l1: &AlloyChainProvider,
        block: BlockID,
    ) -> Result<(), AltDaError> {
        self.advance_challenge_origin(l1, block).await?;
        self.advance_commitment_origin(l1, block).await?;

        if self.state.no_commitments() {
            self.update_finalized_from_l1(l1).await?;
        }
        Ok(())
    }

    pub fn reset(&mut self, base: BlockInfo, _base_cfg: SystemConfig) {
        if self.resetting {
            self.resetting = false;
            self.commitment_origin = base.id();
            self.state.clear_commitments();
        } else {
            self.challenge_origin = base.id();
            self.commitment_origin = base.id();
            self.state.reset();
        }
        io::print_err(EOF);
    }

    pub fn finalize(&mut self, l1_finalized: BlockInfo) {
        self.update_finalized_head(l1_finalized);

        tracing::info!(self.log, "received l1 finalized signal, forwarding altda finalization to finalizedHeadSignalHandler";
            "l1" => ?l1_finalized,
            "altda" => ?self.finalized_head
        );

        if let Some(handler) = &self.finalized_head_signal_handler {
            handler(self.finalized_head);
        } else {
            tracing::warn!(self.log, "finalized head signal handler not set");
        }
    }

    pub fn on_finalized_head_signal(&mut self, _callback: FinalizedHeadSignal) {
        self.finalized_head_signal_handler = Some(f);
    }
}
