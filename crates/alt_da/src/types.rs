//! Types for the `kona-altda` crate.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloy_primitives::hex;
use alloy_primitives::utils::keccak256;
use alloy_primitives::{Bytes, U256};
use alloy_rpc_types::Log;
use anyhow::{anyhow, Error};
use core::any::Any;
use core::fmt::{Debug, Display};
use op_alloy_protocol::BlockInfo;

/// InputFetcherConfig struct
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

/// A altda error.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AltDaError {
    /// A reorg is required.
    ReorgRequired,
    /// Not enough data.
    NotEnoughData,
    /// The commitment was challenge, but the challenge period expired.
    ChallengeExpired,
    /// Missing data past the challenge period.
    MissingPastWindow,
    /// A challenge is pending for the given commitment
    ChallengePending,
    /// A challenge event is decoded but it does not relate to the atual chain commitment
    InvalidChallenge,
    /// An invalid commitment was received
    InvalidCommitment,
    /// An invalid commitment type was received
    InvalidCommitmentType,
    /// The commitment could not be found by the da server
    NotFound,
    /// the DA server failed to get the preimage
    FailedToGetPreimage,
    /// An invalid input was given
    InvalidInput,
    /// A Network error ocurred
    NetworkError,
    /// A mismatch between the commitment used to fetch the preimage and the commitment for the preimage ocurred
    CommitmentMismatch,
    /// Could not decode a challenge event
    DecodeError,
    /// Unknown challenge status
    UnknownStatus,
}

impl Display for AltDaError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ReorgRequired => write!(f, "reorg required"),
            Self::NotEnoughData => write!(f, "not enough data"),
            Self::ChallengeExpired => write!(f, "challenge expired"),
            Self::MissingPastWindow => write!(f, "missing past window"),
            Self::ChallengePending => write!(f, "challenge pending"),
            Self::InvalidChallenge => write!(f, "invalid challenge"),
            Self::InvalidCommitmentType => write!(f, "invalid commitment type"),
            Self::InvalidCommitment => write!(f, "invalid commitment"),
            Self::NotFound => write!(f, "not found"),
            Self::FailedToGetPreimage => write!(f, "failed to get preimage"),
            Self::InvalidInput => write!(f, "invalid input"),
            Self::NetworkError => write!(f, "network error"),
            Self::CommitmentMismatch => write!(f, "commitment mistmatch"),
            Self::DecodeError => write!(f, "could not decode challenge event"),
            Self::UnknownStatus => write!(f, "unknown challenge status"),
        }
    }
}

/// A callback method for the finalized head signal.

pub struct FinalizedHeadSignal(Arc<dyn Fn(BlockInfo) + Send + Sync>);

impl FinalizedHeadSignal {
    pub fn new(f: impl Fn(BlockInfo) + Send + Sync + 'static) -> Self {
        FinalizedHeadSignal(Arc::new(f))
    }

    pub fn call(&self, block_info: BlockInfo) {
        (self.0)(block_info)
    }
}

impl Clone for FinalizedHeadSignal {
    fn clone(&self) -> Self {
        FinalizedHeadSignal(Arc::clone(&self.0))
    }
}

impl core::fmt::Debug for FinalizedHeadSignal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FinalizedHeadSignal").field("function", &"<function>").finish()
    }
}

/// Max input size ensures the canonical chain cannot include input batches too large to
/// challenge in the Data Availability Challenge contract. Value in number of bytes.
/// This value can only be changed in a hard fork.
pub const MAX_INPUT_SIZE: usize = 130672;

/// TxDataVersion1 is the version number for batcher transactions containing
/// altda commitments. It should not collide with DerivationVersion which is still
/// used downstream when parsing the frames.
pub const TX_DATA_VERSION_1: u8 = 1;

pub const CHALLENGE_STATUS_EVENT_NAME: &str = "ChallengeStatusChanged";
pub const CHALLENGE_STATUS_EVENT_ABI: &str = "ChallengeStatusChanged(uint256,bytes,uint8)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitmentType {
    Keccak256 = 0,
    Generic = 1,
}

impl CommitmentType {
    pub fn from_string(s: &str) -> Result<Self, AltDaError> {
        match s {
            "KeccakCommitment" => Ok(CommitmentType::Keccak256),
            "GenericCommitment" => Ok(CommitmentType::Generic),
            _ => Err(AltDaError::InvalidCommitmentType),
        }
    }
}

pub trait CommitmentData: Send + Sync + Debug {
    fn commitment_type(&self) -> CommitmentType;
    fn encode(&self) -> Bytes;
    fn tx_data(&self) -> Bytes;
    fn verify(&self, input: &[u8]) -> Result<(), AltDaError>;
    fn to_string(&self) -> String;
    fn clone_box(&self) -> Box<dyn CommitmentData + Send + Sync>;
    fn as_any(&self) -> &dyn Any;
}

impl Clone for Box<dyn CommitmentData + Send + Sync> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keccak256Commitment(Bytes);

impl Keccak256Commitment {
    pub fn new(input: &[u8]) -> Self {
        let hash = keccak256(input);
        Keccak256Commitment(Bytes::from(hash.to_vec()))
    }

    pub fn decode(commitment: &[u8]) -> Result<Self, AltDaError> {
        if commitment.is_empty() || commitment.len() != 32 {
            return Err(AltDaError::InvalidCommitment);
        }
        Ok(Keccak256Commitment(Bytes::from(commitment.to_vec())))
    }
}

impl CommitmentData for Keccak256Commitment {
    fn commitment_type(&self) -> CommitmentType {
        CommitmentType::Keccak256
    }

    fn encode(&self) -> Bytes {
        let mut encoded = Vec::with_capacity(1 + self.0.len());
        encoded.push(CommitmentType::Keccak256 as u8);
        encoded.extend_from_slice(&self.0);
        Bytes::from(encoded)
    }

    fn tx_data(&self) -> Bytes {
        let mut data = Vec::with_capacity(2 + self.0.len());
        data.push(1); // TxDataVersion1
        data.push(CommitmentType::Keccak256 as u8);
        data.extend_from_slice(&self.0);
        Bytes::from(data)
    }

    fn verify(&self, input: &[u8]) -> Result<(), AltDaError> {
        let hash = keccak256(input);
        if hash.as_slice() == &self.0[..] {
            Ok(())
        } else {
            Err(AltDaError::CommitmentMismatch)
        }
    }

    fn to_string(&self) -> String {
        hex::encode(self.encode())
    }

    fn clone_box(&self) -> Box<dyn CommitmentData + Send + Sync> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericCommitment(Bytes);

impl GenericCommitment {
    pub fn new(input: &[u8]) -> Self {
        GenericCommitment(Bytes::from(input.to_vec()))
    }

    pub fn decode(commitment: &[u8]) -> Result<Self, AltDaError> {
        if commitment.is_empty() {
            return Err(AltDaError::InvalidCommitment);
        }
        Ok(GenericCommitment(Bytes::from(commitment.to_vec())))
    }
}

impl CommitmentData for GenericCommitment {
    fn commitment_type(&self) -> CommitmentType {
        CommitmentType::Generic
    }

    fn encode(&self) -> Bytes {
        let mut encoded = Vec::with_capacity(1 + self.0.len());
        encoded.push(CommitmentType::Generic as u8);
        encoded.extend_from_slice(&self.0);
        Bytes::from(encoded)
    }

    fn tx_data(&self) -> Bytes {
        let mut data = Vec::with_capacity(2 + self.0.len());
        data.push(1); // TxDataVersion1
        data.push(CommitmentType::Generic as u8);
        data.extend_from_slice(&self.0);
        Bytes::from(data)
    }

    fn verify(&self, _input: &[u8]) -> Result<(), AltDaError> {
        Ok(())
    }

    fn to_string(&self) -> String {
        hex::encode(self.encode())
    }

    fn clone_box(&self) -> Box<dyn CommitmentData + Send + Sync> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn new_commitment_data(
    t: CommitmentType,
    input: &[u8],
) -> Box<dyn CommitmentData + Send + Sync> {
    match t {
        CommitmentType::Keccak256 => Box::new(Keccak256Commitment::new(input)),
        CommitmentType::Generic => Box::new(GenericCommitment::new(input)),
    }
}

pub fn decode_commitment_data(
    input: &[u8],
) -> Result<Box<dyn CommitmentData + Send + Sync>, AltDaError> {
    if input.is_empty() {
        return Err(AltDaError::InvalidCommitment);
    }
    let t = input[0];
    let data = &input[1..];
    match t {
        0 => Ok(Box::new(Keccak256Commitment::decode(data)?)),
        1 => Ok(Box::new(GenericCommitment::decode(data)?)),
        _ => Err(AltDaError::InvalidCommitment),
    }
}

pub fn decode_challenge_status_event(log: &Log) -> Result<(Bytes, u64, u8), Error> {
    // Ensure we have the correct number of topics
    if log.topics().len() != 3 {
        return Err(anyhow!("Invalid number of topics for ChallengeStatusChanged event"));
    }

    // Check if the first topic matches the event signature
    let event_signature = keccak256(CHALLENGE_STATUS_EVENT_ABI.as_bytes());
    if log.topics()[0] != event_signature {
        return Err(anyhow!("Invalid event signature"));
    }

    // Parse indexed parameters from topics
    let challenged_commitment = Bytes::from(log.topics()[1]);
    let block_number_u256 = U256::from_be_bytes(log.topics()[2].into());

    let block_number =
        block_number_u256.try_into().map_err(|_| anyhow!("Block number too large")).unwrap();

    // Decode the non-indexed parameter (status) from the data field
    let status = if log.inner.data.data.len() >= 32 {
        log.inner.data.data[31] // Assuming status is the last byte of the 32-byte word
    } else {
        return Err(anyhow!("Invalid data length for status"));
    };

    Ok((challenged_commitment, block_number, status))
}

pub fn decode_resolved_input(data: &[u8]) -> Result<Vec<u8>, Error> {
    // Check if the data is long enough (4 bytes for function selector + at least 32 bytes for data)
    if data.len() < 36 {
        return Err(anyhow!("Input data too short"));
    }

    // Skip the first 4 bytes (function selector)
    let input_data = &data[4..];

    // The first 32 bytes after the selector contain the offset to the dynamic data
    let offset = u256_from_be_bytes(&input_data[0..32]);

    // Ensure the offset is valid
    if offset >= input_data.len() as u64 {
        return Err(anyhow!("Invalid data offset"));
    }

    // The next 32 bytes after the offset contain the length of the resolveData
    let length_start = offset as usize;
    let length = u256_from_be_bytes(&input_data[length_start..length_start + 32]);

    // Ensure we have enough data
    let data_start = length_start + 32;
    if data_start + length as usize > input_data.len() {
        return Err(anyhow!("Input data too short for specified length"));
    }

    // Extract the resolveData
    Ok(input_data[data_start..data_start + length as usize].to_vec())
}

// Helper function to convert big-endian bytes to u64
fn u256_from_be_bytes(bytes: &[u8]) -> u64 {
    let mut result = 0u64;
    for &byte in bytes.iter().rev().take(8) {
        result = (result << 8) | (byte as u64);
    }
    result
}
