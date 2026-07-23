//! Provides an interface and default implementation for the `VotingPower` operation

use alloc::vec::Vec;
use core::{convert::TryFrom, fmt, marker::PhantomData};

use cometbft::{
    account,
    block::CommitSig,
    chain,
    crypto::signature,
    trust_threshold::TrustThreshold as _,
    validator,
    vote::{SignedVote, ValidatorIndex, Vote},
};
use serde::{Deserialize, Serialize};

use crate::{
    errors::VerificationError,
    prelude::*,
    types::{Commit, SignedHeader, TrustThreshold, ValidatorSet},
};

/// Tally for the voting power computed by the `VotingPowerCalculator`
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, Eq)]
pub struct VotingPowerTally {
    /// Total voting power
    pub total: u64,
    /// Tallied voting power
    pub tallied: u64,
    /// Trust threshold for voting power
    pub trust_threshold: TrustThreshold,
}

impl VotingPowerTally {
    fn new(total: u64, trust_threshold: TrustThreshold) -> Self {
        Self {
            total,
            tallied: 0,
            trust_threshold,
        }
    }

    /// Adds given amount of power to tallied voting power amount.
    fn tally(&mut self, power: u64) {
        self.tallied += power;
        debug_assert!(self.tallied <= self.total);
    }

    /// Checks whether tallied amount meets trust threshold.
    fn check(&self) -> Result<(), Self> {
        if self
            .trust_threshold
            .is_enough_power(self.tallied, self.total)
        {
            Ok(())
        } else {
            Err(*self)
        }
    }
}

impl fmt::Display for VotingPowerTally {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "VotingPower(total={} tallied={} trust_threshold={})",
            self.total, self.tallied, self.trust_threshold
        )
    }
}

/// Computes the voting power in a commit against a validator set.
///
/// This trait provides default implementation of some helper functions.
pub trait VotingPowerCalculator: Send + Sync {
    /// Compute the total voting power in a validator set
    fn total_power_of(&self, validator_set: &ValidatorSet) -> u64 {
        validator_set
            .validators()
            .iter()
            .fold(0u64, |total, val_info| total + val_info.power.value())
    }

    /// Check that there is enough trust between an untrusted header and given
    /// trusted and untrusted validator sets.
    fn check_enough_trust_and_signers(
        &self,
        untrusted_header: &SignedHeader,
        trusted_validators: &ValidatorSet,
        trust_threshold: TrustThreshold,
        untrusted_validators: &ValidatorSet,
    ) -> Result<(), VerificationError> {
        let (trusted_power, untrusted_power) = self.voting_power_in_sets(
            untrusted_header,
            (trusted_validators, trust_threshold),
            (untrusted_validators, TrustThreshold::TWO_THIRDS),
        )?;
        trusted_power
            .check()
            .map_err(VerificationError::not_enough_trust)?;
        untrusted_power
            .check()
            .map_err(VerificationError::insufficient_signers_overlap)?;
        Ok(())
    }

    /// Check if there is 2/3rd overlap between an untrusted header and untrusted validator set
    fn check_signers_overlap(
        &self,
        untrusted_header: &SignedHeader,
        untrusted_validators: &ValidatorSet,
    ) -> Result<(), VerificationError> {
        let trust_threshold = TrustThreshold::TWO_THIRDS;
        self.voting_power_in(untrusted_header, untrusted_validators, trust_threshold)?
            .check()
            .map_err(VerificationError::insufficient_signers_overlap)
    }

    /// Compute the voting power in a header and its commit against a validator
    /// set.
    fn voting_power_in(
        &self,
        signed_header: &SignedHeader,
        validator_set: &ValidatorSet,
        trust_threshold: TrustThreshold,
    ) -> Result<VotingPowerTally, VerificationError>;

    /// Compute the voting power in a header and its commit against two separate
    /// validator sets.
    ///
    /// This is equivalent to calling [`Self::voting_power_in`] on each set
    /// separately but may be more optimised.  Implementators are encouraged to
    /// write a properly optimised method which avoids checking the same
    /// signature twice but for a simple unoptimised implementation the
    /// following works:
    ///
    /// ```ignore
    ///     fn voting_power_in_sets(
    ///         &self,
    ///         signed_header: &SignedHeader,
    ///         first_set: (&ValidatorSet, TrustThreshold),
    ///         second_set: (&ValidatorSet, TrustThreshold),
    ///     ) -> Result<(VotingPowerTally, VotingPowerTally), VerificationError> {
    ///         let first_tally = self.voting_power_in(
    ///             signed_header,
    ///             first_set.0,
    ///             first_set.1,
    ///         )?;
    ///         let second_tally = self.voting_power_in(
    ///             signed_header,
    ///             first_set.0,
    ///             first_set.1,
    ///         )?;
    ///         Ok((first_tally, second_tally))
    ///     }
    ///
    /// ```
    fn voting_power_in_sets(
        &self,
        signed_header: &SignedHeader,
        first_set: (&ValidatorSet, TrustThreshold),
        second_set: (&ValidatorSet, TrustThreshold),
    ) -> Result<(VotingPowerTally, VotingPowerTally), VerificationError>;
}

/// A signed non-nil vote.
struct NonAbsentCommitVote {
    signed_vote: SignedVote,
    /// Flag indicating whether the signature has already been verified.
    verified: bool,
}

impl NonAbsentCommitVote {
    /// Returns a signed non-nil vote for given commit.
    pub fn new(
        commit_sig: &CommitSig,
        validator_index: ValidatorIndex,
        commit: &Commit,
        chain_id: &chain::Id,
    ) -> Option<Result<Self, VerificationError>> {
        let (validator_address, timestamp, signature) = match commit_sig {
            CommitSig::BlockIdFlagAbsent => return None,
            CommitSig::BlockIdFlagCommit {
                validator_address,
                timestamp,
                signature,
            } => (*validator_address, *timestamp, signature),
            CommitSig::BlockIdFlagNil { .. } => return None,
        };

        let vote = Vote {
            vote_type: cometbft::vote::Type::Precommit,
            height: commit.height,
            round: commit.round,
            block_id: Some(commit.block_id),
            timestamp: Some(timestamp),
            validator_address,
            validator_index,
            signature: signature.clone(),
            extension: Default::default(),
            extension_signature: None,
        };
        Some(
            SignedVote::from_vote(vote, chain_id.clone())
                .ok_or_else(VerificationError::missing_signature)
                .map(|signed_vote| Self {
                    signed_vote,
                    verified: false,
                }),
        )
    }

    /// Returns address of the validator making the vote.
    pub fn validator_id(&self) -> account::Id {
        self.signed_vote.validator_id()
    }
}

/// Collection of non-absent commit votes.
struct NonAbsentCommitVotes {
    /// Votes sorted by validator address.
    votes: Vec<NonAbsentCommitVote>,
    /// Internal buffer for storing sign_bytes.
    ///
    /// The buffer is reused for each canonical vote so that we allocate it
    /// once.
    sign_bytes: Vec<u8>,
}

impl NonAbsentCommitVotes {
    /// Initial capacity of the `sign_bytes` buffer.
    const SIGN_BYTES_INITIAL_CAPACITY: usize = 166;

    pub fn new(signed_header: &SignedHeader) -> Result<Self, VerificationError> {
        let mut votes = signed_header
            .commit
            .signatures
            .iter()
            .enumerate()
            .flat_map(|(idx, signature)| {
                // We never have more than 2³¹ signatures so this always
                // succeeds.
                let idx = ValidatorIndex::try_from(idx).unwrap();
                NonAbsentCommitVote::new(
                    signature,
                    idx,
                    &signed_header.commit,
                    &signed_header.header.chain_id,
                )
            })
            .collect::<Result<Vec<_>, VerificationError>>()?;
        votes.sort_unstable_by_key(NonAbsentCommitVote::validator_id);

        // Check if there are duplicate signatures.  If at least one duplicate
        // is found, report it as an error.
        let duplicate = votes
            .windows(2)
            .find(|pair| pair[0].validator_id() == pair[1].validator_id());
        if let Some(pair) = duplicate {
            Err(VerificationError::duplicate_validator(
                pair[0].validator_id(),
            ))
        } else {
            Ok(Self {
                votes,
                sign_bytes: Vec::with_capacity(Self::SIGN_BYTES_INITIAL_CAPACITY),
            })
        }
    }

    /// Looks up a vote cast by given validator.
    pub fn has_voted<V: signature::Verifier>(
        &mut self,
        validator: &validator::Info,
    ) -> Result<Option<usize>, VerificationError> {
        if let Ok(idx) = self
            .votes
            .binary_search_by_key(&validator.address, NonAbsentCommitVote::validator_id)
        {
            let vote = &mut self.votes[idx];

            if !vote.verified {
                self.sign_bytes = vote.signed_vote.sign_bytes();

                let sign_bytes = self.sign_bytes.as_slice();
                validator
                    .verify_signature::<V>(sign_bytes, vote.signed_vote.signature())
                    .map_err(|_| {
                        VerificationError::invalid_signature(
                            vote.signed_vote.signature().as_bytes().to_vec(),
                            Box::new(validator.clone()),
                            sign_bytes.to_vec(),
                        )
                    })?;
                vote.verified = true;
            }
            Ok(Some(idx))
        } else {
            Ok(None)
        }
    }
}

/// Default implementation of a `VotingPowerCalculator`, parameterized with
/// the signature verification trait.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProvidedVotingPowerCalculator<V> {
    _verifier: PhantomData<V>,
}

// Safety: the only member is phantom data
unsafe impl<V> Send for ProvidedVotingPowerCalculator<V> {}
unsafe impl<V> Sync for ProvidedVotingPowerCalculator<V> {}

impl<V> Default for ProvidedVotingPowerCalculator<V> {
    fn default() -> Self {
        Self {
            _verifier: PhantomData,
        }
    }
}

/// Dictionary of validators sorted by address.
struct ValidatorMap<'a> {
    validators: Vec<(&'a validator::Info, bool)>,
}

/// Error during validator lookup.
enum LookupError {
    NotFound,
    AlreadySeen,
}

impl<'a> ValidatorMap<'a> {
    /// Constructs a new map from given list of validators.
    pub fn new(validators: &'a [validator::Info]) -> Self {
        let mut validators = validators.iter().map(|v| (v, false)).collect::<Vec<_>>();
        validators.sort_unstable_by_key(|item| &item.0.address);
        Self { validators }
    }

    /// Finds entry for validator with given address; returns error if validator
    /// has been returned already by previous call to `find`.
    pub fn find(&mut self, address: &account::Id) -> Result<&'a validator::Info, LookupError> {
        let index = self
            .validators
            .binary_search_by_key(&address, |item| &item.0.address)
            .map_err(|_| LookupError::NotFound)?;

        let (validator, seen) = &mut self.validators[index];
        if *seen {
            Err(LookupError::AlreadySeen)
        } else {
            *seen = true;
            Ok(validator)
        }
    }
}

/// Default implementation of a `VotingPowerCalculator`.
#[cfg(feature = "rust-crypto")]
pub type ProdVotingPowerCalculator =
    ProvidedVotingPowerCalculator<cometbft::crypto::default::signature::Verifier>;

impl<V: signature::Verifier> VotingPowerCalculator for ProvidedVotingPowerCalculator<V> {
    fn voting_power_in(
        &self,
        signed_header: &SignedHeader,
        validator_set: &ValidatorSet,
        trust_threshold: TrustThreshold,
    ) -> Result<VotingPowerTally, VerificationError> {
        let mut votes = NonAbsentCommitVotes::new(signed_header)?;
        voting_power_in_impl::<V>(
            &mut votes,
            validator_set,
            trust_threshold,
            self.total_power_of(validator_set),
        )
    }

    fn voting_power_in_sets(
        &self,
        signed_header: &SignedHeader,
        first_set: (&ValidatorSet, TrustThreshold),
        second_set: (&ValidatorSet, TrustThreshold),
    ) -> Result<(VotingPowerTally, VotingPowerTally), VerificationError> {
        let mut votes = NonAbsentCommitVotes::new(signed_header)?;
        let first_tally = voting_power_in_impl::<V>(
            &mut votes,
            first_set.0,
            first_set.1,
            self.total_power_of(first_set.0),
        )?;
        let second_tally = voting_power_in_impl::<V>(
            &mut votes,
            second_set.0,
            second_set.1,
            self.total_power_of(second_set.0),
        )?;
        Ok((first_tally, second_tally))
    }
}

fn voting_power_in_impl<V: signature::Verifier>(
    votes: &mut NonAbsentCommitVotes,
    validator_set: &ValidatorSet,
    trust_threshold: TrustThreshold,
    total_voting_power: u64,
) -> Result<VotingPowerTally, VerificationError> {
    let mut power = VotingPowerTally::new(total_voting_power, trust_threshold);
    let mut seen_vals = Vec::new();

    for validator in validator_set.validators() {
        if let Some(idx) = votes.has_voted::<V>(validator)? {
            // Check if this validator has already voted.
            //
            // O(n) complexity.
            if seen_vals.contains(&idx) {
                return Err(VerificationError::duplicate_validator(validator.address));
            }
            seen_vals.push(idx);

            power.tally(validator.power());

            // Break early if sufficient voting power is reached.
            if power.check().is_ok() {
                break;
            }
        }
    }
    Ok(power)
}

// The below unit tests replaces the static voting power test files
// see https://github.com/informalsystems/tendermint-rs/pull/383
// This is essentially to remove the heavy dependency on MBT
// TODO: We plan to add Lightweight MBT for `voting_power_in` in the near future
