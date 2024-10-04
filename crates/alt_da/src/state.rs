use alloy_primitives::Bytes;

use crate::online::InputFetcherConfig;
use crate::types::{CommitmentData, AltDaError};
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use alloy_primitives::hex;
use alloy_primitives::map::HashMap;
use kona_primitives::BlockInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeStatus {
    Uninitialized,
    Active,
    Resolved,
    Expired,
}

pub struct Commitment {
    pub data: Box<dyn CommitmentData>,
    pub inclusion_block: BlockInfo,
    pub challenge_window_end: u64,
}

pub struct Challenge {
    pub commitment_data: Box<dyn CommitmentData>,
    pub commitment_inclusion_block_number: u64,
    pub resolve_window_end: u64,
    pub input: Option<Bytes>,
    pub challenge_status: ChallengeStatus,
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

pub struct State {
    pub commitments: Vec<Commitment>,
    pub expired_commitments: Vec<Commitment>,
    pub challenges: Vec<Challenge>,
    pub expired_challenges: Vec<Challenge>,
    pub challenges_map: HashMap<String, Challenge>,
    pub last_pruned_commitment: Option<BlockInfo>,
    pub cfg: InputFetcherConfig,
}

impl State {
    pub fn new(log: Logger, cfg: Config) -> Self {
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
        comm: Box<dyn CommitmentData>,
        inclusion_block: BlockID,
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
        self.challenges.push(challenge);
        self.challenges_map.insert(key, self.challenges.last().unwrap().clone());
    }

    pub fn resolve_challenge(
        &mut self,
        comm: &dyn CommitmentData,
        inclusion_block: BlockID,
        comm_block_number: u64,
        input: Bytes,
    ) -> Result<(), Box<dyn Error>> {
        let key = challenge_key(comm, comm_block_number);
        if let Some(challenge) = self.challenges_map.get_mut(&key) {
            challenge.input = Some(input);
            challenge.challenge_status = ChallengeStatus::Resolved;
            Ok(())
        } else {
            Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "challenge was not tracked",
            )))
        }
    }

    pub fn track_commitment(&mut self, comm: Box<dyn CommitmentData>, inclusion_block: L1BlockRef) {
        let commitment = Commitment {
            data: comm,
            inclusion_block,
            challenge_window_end: inclusion_block.number + self.cfg.challenge_window,
        };
        self.commitments.push(commitment);
    }

    pub fn get_challenge(
        &self,
        comm: &dyn CommitmentData,
        comm_block_number: u64,
    ) -> Option<&Challenge> {
        let key = challenge_key(comm, comm_block_number);
        self.challenges_map.get(&key)
    }

    pub fn get_challenge_status(
        &self,
        comm: &dyn CommitmentData,
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

    pub fn expire_commitments(&mut self, origin: BlockID) -> Result<(), AltDaError> {
        let mut reorg_required = false;

        while let Some(commitment) = self.commitments.first() {
            let challenge =
                self.get_challenge(&*commitment.data, commitment.inclusion_block.number);

            let expires_at =
                challenge.map_or(commitment.challenge_window_end, |c| c.resolve_window_end);

            if expires_at > origin.number {
                break;
            }

            let commitment = self.commitments.remove(0);
            self.log.info(
                "Expiring commitment",
                log::o!("comm" => format!("{:?}", commitment.data),
                                  "commInclusionBlockNumber" => commitment.inclusion_block.number,
                                  "origin" => format!("{:?}", origin),
                                  "challenged" => challenge.is_some()),
            );
            self.expired_commitments.push(commitment);

            if let Some(challenge) = challenge {
                if challenge.challenge_status != ChallengeStatus::Resolved {
                    reorg_required = true;
                }
            }
        }

        if reorg_required {
            Err(AltDaError::ReorgRequired)
        } else {
            Ok(())
        }
    }

    pub fn expire_challenges(&mut self, origin: BlockID) {
        while let Some(challenge) = self.challenges.first() {
            if challenge.resolve_window_end > origin.number {
                break;
            }

            let challenge = self.challenges.remove(0);
            self.log.info("Expiring challenge",
                          log::o!("comm" => format!("{:?}", challenge.comm_data),
                                  "commInclusionBlockNumber" => challenge.comm_inclusion_block_number,
                                  "origin" => format!("{:?}", origin)));
            self.expired_challenges.push(challenge);

            if let Some(challenge) = self.expired_challenges.last_mut() {
                if challenge.challenge_status == ChallengeStatus::Active {
                    challenge.challenge_status = ChallengeStatus::Expired;
                }
            }
        }
    }

    pub fn prune(&mut self, origin: BlockID) {
        self.prune_commitments(origin);
        self.prune_challenges(origin);
    }

    fn prune_commitments(&mut self, origin: BlockID) {
        while let Some(commitment) = self.expired_commitments.first() {
            let challenge =
                self.get_challenge(&*commitment.data, commitment.inclusion_block.number);

            let expires_at =
                challenge.map_or(commitment.challenge_window_end, |c| c.resolve_window_end);

            if expires_at > origin.number {
                break;
            }

            let commitment = self.expired_commitments.remove(0);
            self.last_pruned_commitment = Some(commitment.inclusion_block);
        }
    }

    fn prune_challenges(&mut self, origin: BlockID) {
        while let Some(challenge) = self.expired_challenges.first() {
            if challenge.resolve_window_end > origin.number {
                break;
            }

            let challenge = self.expired_challenges.remove(0);
            self.challenges_map.remove(&challenge.key());
        }
    }
}
