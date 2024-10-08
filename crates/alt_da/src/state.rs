use alloy_eips::BlockNumHash;
use alloy_primitives::Bytes;

use crate::types::{AltDaError, CommitmentData, InputFetcherConfig};
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloy_primitives::hex;
use alloy_primitives::map::HashMap;
use op_alloy_protocol::BlockInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeStatus {
    Uninitialized,
    Active,
    Resolved,
    Expired,
}

#[derive(Clone)]
pub struct Commitment {
    pub data: Arc<Box<dyn CommitmentData + Send + Sync>>,
    pub inclusion_block: BlockInfo,
    pub challenge_window_end: u64,
}

pub struct Challenge {
    pub commitment_data: Arc<Box<dyn CommitmentData + Send + Sync>>,
    pub commitment_inclusion_block_number: u64,
    pub resolve_window_end: u64,
    pub input: Option<Bytes>,
    pub challenge_status: ChallengeStatus,
}

impl Clone for Challenge {
    fn clone(&self) -> Self {
        Challenge {
            commitment_data: self.commitment_data.clone(),
            commitment_inclusion_block_number: self.commitment_inclusion_block_number,
            resolve_window_end: self.resolve_window_end,
            input: self.input.clone(),
            challenge_status: self.challenge_status.clone(),
        }
    }
}

impl Challenge {
    fn key(&self) -> String {
        format!(
            "{}{}",
            self.commitment_inclusion_block_number,
            hex::encode(self.commitment_data.encode())
        )
    }
}

fn challenge_key(
    comm: Arc<Box<dyn CommitmentData + Send + Sync>>,
    inclusion_block_number: u64,
) -> String {
    let encoded = comm.encode();
    format!("{:016x}{}", inclusion_block_number, hex::encode(&encoded))
}

pub struct State {
    pub commitments: Vec<Commitment>,
    pub expired_commitments: Vec<Commitment>,
    pub challenges: Vec<Challenge>,
    pub expired_challenges: Vec<Challenge>,
    pub challenges_map: HashMap<String, Challenge>,
    pub last_pruned_commitment: Option<BlockInfo>,
    pub cfg: InputFetcherConfig,
}

impl Clone for State {
    fn clone(&self) -> Self {
        State {
            commitments: self.commitments.clone(),
            expired_commitments: self.expired_commitments.clone(),
            challenges: self.challenges.clone(),
            expired_challenges: self.expired_challenges.clone(),
            challenges_map: self.challenges_map.clone(),
            last_pruned_commitment: self.last_pruned_commitment.clone(),
            cfg: self.cfg.clone(),
        }
    }
}

impl State {
    pub fn new(cfg: InputFetcherConfig) -> Self {
        State {
            commitments: Vec::new(),
            expired_commitments: Vec::new(),
            challenges: Vec::new(),
            expired_challenges: Vec::new(),
            challenges_map: HashMap::new(),
            last_pruned_commitment: None,
            cfg,
        }
    }

    pub fn clear_commitments(&mut self) {
        self.commitments.clear();
        self.expired_commitments.clear();
    }

    pub fn reset(&mut self) {
        self.commitments.clear();
        self.expired_commitments.clear();
        self.challenges.clear();
        self.expired_challenges.clear();
        self.challenges_map.clear();
    }

    pub fn create_challenge(
        &mut self,
        comm: Arc<Box<dyn CommitmentData + Send + Sync>>,
        inclusion_block: BlockNumHash,
        comm_block_number: u64,
    ) {
        let challenge = Challenge {
            commitment_data: comm,
            commitment_inclusion_block_number: comm_block_number,
            resolve_window_end: inclusion_block.number + self.cfg.resolve_window,
            input: None,
            challenge_status: ChallengeStatus::Active,
        };

        let key = challenge.key();
        self.challenges.push(challenge.clone());
        self.challenges_map.insert(key, challenge);
    }

    pub fn resolve_challenge(
        &mut self,
        comm: Arc<Box<dyn CommitmentData + Send + Sync>>,
        comm_block_number: u64,
        input: Bytes,
    ) -> Result<(), AltDaError> {
        let key = challenge_key(comm, comm_block_number);
        if let Some(challenge) = self.challenges_map.get_mut(&key) {
            challenge.input = Some(input);
            challenge.challenge_status = ChallengeStatus::Resolved;
            Ok(())
        } else {
            Err(AltDaError::NotFound)
        }
    }

    pub fn track_commitment(
        &mut self,
        comm: Arc<Box<dyn CommitmentData + Send + Sync>>,
        inclusion_block: BlockInfo,
    ) {
        let commitment = Commitment {
            data: comm,
            inclusion_block,
            challenge_window_end: inclusion_block.number + self.cfg.challenge_window,
        };
        self.commitments.push(commitment);
    }

    pub fn get_challenge(
        &self,
        comm: Arc<Box<dyn CommitmentData + Send + Sync>>,
        comm_block_number: u64,
    ) -> Option<&Challenge> {
        let key = challenge_key(comm, comm_block_number);
        self.challenges_map.get(&key)
    }

    pub fn get_challenge_status(
        &self,
        comm: Arc<Box<dyn CommitmentData + Send + Sync>>,
        comm_block_number: u64,
    ) -> ChallengeStatus {
        self.get_challenge(comm, comm_block_number)
            .map_or(ChallengeStatus::Uninitialized, |c| c.challenge_status)
    }

    pub fn no_commitments(&self) -> bool {
        self.challenges.is_empty()
            && self.expired_challenges.is_empty()
            && self.commitments.is_empty()
            && self.expired_commitments.is_empty()
    }

    pub fn expire_commitments(&mut self, origin: BlockNumHash) -> Result<(), AltDaError> {
        let mut reorg_required = false;
        let mut expired_indices = Vec::new();

        for (index, commitment) in self.commitments.iter().enumerate() {
            let challenge_key =
                challenge_key(commitment.clone().data, commitment.inclusion_block.number);
            let expires_at = self
                .challenges_map
                .get(&challenge_key)
                .map_or(commitment.challenge_window_end, |c| c.resolve_window_end);

            if expires_at > origin.number {
                break;
            }

            expired_indices.push(index);

            if let Some(challenge) = self.challenges_map.get(&challenge_key) {
                if challenge.challenge_status != ChallengeStatus::Resolved {
                    reorg_required = true;
                }
            }
        }

        // Remove expired commitments in reverse order to maintain correct indices
        for &index in expired_indices.iter().rev() {
            let commitment = self.commitments.remove(index);
            self.expired_commitments.push(commitment);
        }

        if reorg_required {
            Err(AltDaError::ReorgRequired)
        } else {
            Ok(())
        }
    }

    pub fn expire_challenges(&mut self, origin: BlockNumHash) {
        while let Some(challenge) = self.challenges.first() {
            if challenge.resolve_window_end > origin.number {
                break;
            }

            let challenge = self.challenges.remove(0);
            self.expired_challenges.push(challenge);

            if let Some(challenge) = self.expired_challenges.last_mut() {
                if challenge.challenge_status == ChallengeStatus::Active {
                    challenge.challenge_status = ChallengeStatus::Expired;
                }
            }
        }
    }

    pub fn prune(&mut self, origin: BlockNumHash) {
        self.prune_commitments(origin);
        self.prune_challenges(origin);
    }

    fn prune_commitments(&mut self, origin: BlockNumHash) {
        while let Some(commitment) = self.expired_commitments.first() {
            let challenge =
                self.get_challenge(commitment.clone().data, commitment.inclusion_block.number);

            let expires_at =
                challenge.map_or(commitment.challenge_window_end, |c| c.resolve_window_end);

            if expires_at > origin.number {
                break;
            }

            let commitment = self.expired_commitments.remove(0);
            self.last_pruned_commitment = Some(commitment.inclusion_block);
        }
    }

    fn prune_challenges(&mut self, origin: BlockNumHash) {
        while let Some(challenge) = self.expired_challenges.first() {
            if challenge.resolve_window_end > origin.number {
                break;
            }

            let challenge = self.expired_challenges.remove(0);
            self.challenges_map.remove(&challenge.key());
        }
    }
}
