//! ILN Governance Contract
//!
//! Issue #59 — GovernanceProposal struct with full spec fields.
//! Issue #61 — cast_vote() with anti-double-vote protection and VoteCast event.
//! Issue #64 — delegate_votes() / undelegate_votes() with transitive delegation
//!             and cycle detection.
//! Issue #68 — veto_proposal() admin emergency block with governance-controlled
//!             disable mechanism.

#![no_std]
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype,
    token::Client as TokenClient, vec, Address, BytesN, Env, IntoVal, Symbol, Vec,
};

/// Vote receipts only need to outlive the active voting window.
const VOTE_RECEIPT_TTL_THRESHOLD_LEDGERS: u32 = 50_000;
const VOTE_RECEIPT_TTL_LEDGERS: u32 = 69_120;
/// Default minimum quorum = 10% (1000 bps).
const DEFAULT_MIN_QUORUM_BPS: u32 = 1_000;

/// Maximum transitive delegation chain depth we will traverse.
const MAX_DELEGATION_DEPTH: u32 = 10;

// ================================================================
// Governance error enum
// ================================================================

#[contracterror]
#[derive(Clone, Debug, PartialEq)]
pub enum GovernanceError {
    AlreadyInitialized = 1,
    ProposalNotFound = 2,
    VotingEnded = 3,
    ProposalNotActive = 4,
    NoVotingPower = 5,
    AlreadyVoted = 6,
    VotingOngoing = 7,
    QuorumNotReached = 8,
    ProposalRejected = 9,
    AlreadyResolved = 10,
    /// Issue #64: Delegating to self is not allowed.
    CannotDelegateToSelf = 11,
    /// Issue #64: Delegation would create a cycle.
    DelegationCyclePrevented = 12,
    /// Invalid quorum basis points (must be 1..=10_000).
    InvalidQuorumBps = 13,
    /// Issue #68: caller is not the admin.
    NotAdmin = 14,
    /// Issue #68: proposal cannot be vetoed in its current status.
    NotVetoable = 15,
    /// Issue #68: admin veto power has been disabled by governance.
    VetoPowerDisabled = 16,
}

// ================================================================
// ProposalAction
// ================================================================

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalAction {
    UpdateFeeRate(u32),
    AddToken(Address),
    RemoveToken(Address),
    UpdateMaxDiscountRate(u32),
}

// ================================================================
// ProposalStatus
// ================================================================

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    Active,
    Passed,
    Rejected,
    Executed,
    /// Issue #68: proposal was blocked by the admin via veto_proposal().
    Vetoed,
}

// ================================================================
// GovernanceProposal struct
// ================================================================

#[contracttype]
#[derive(Clone, Debug)]
pub struct GovernanceProposal {
    pub id: u64,
    pub proposer: Address,
    pub description_hash: BytesN<32>,
    pub action_type: ProposalAction,
    pub proposed_value: i128,
    pub status: ProposalStatus,
    pub votes_for: i128,
    pub votes_against: i128,
    pub created_at: u64,
    pub voting_end: u64,
}

// ================================================================
// Events
// ================================================================

#[contractevent(topics = ["vote_cast"])]
#[derive(Clone, Debug, PartialEq)]
pub struct VoteCast {
    #[topic]
    pub proposal_id: u64,
    #[topic]
    pub voter: Address,
    pub support: bool,
    pub weight: i128,
}

/// Issue #64
#[contractevent(topics = ["votes_delegated"])]
#[derive(Clone, Debug, PartialEq)]
pub struct VotesDelegated {
    #[topic]
    pub delegator: Address,
    #[topic]
    pub delegate: Address,
}

/// Issue #64
#[contractevent(topics = ["votes_undelegated"])]
#[derive(Clone, Debug, PartialEq)]
pub struct VotesUndelegated {
    #[topic]
    pub delegator: Address,
}

/// Issue #68: emitted when the admin vetoes a proposal.
#[contractevent(topics = ["proposal_vetoed"])]
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalVetoed {
    #[topic]
    pub proposal_id: u64,
    #[topic]
    pub admin: Address,
    pub reason_hash: BytesN<32>,
}

// ================================================================
// Storage keys
// ================================================================

#[contracttype]
pub enum StorageKey {
    IlnContract,
    GovToken,
    /// Configurable minimum participation required for proposal passing.
    /// Expressed in basis points (bps) of total supply, e.g. 1000 = 10%.
    MinQuorumBps,
    Proposal(u64),
    ProposalCount,
    VoteWeightSnapshot(u64, Address),
    HasVoted(u64, Address),
    /// Issue #64: forward delegation pointer — Delegation(X) = Y means X delegates to Y.
    Delegation(Address),
    /// Issue #64: running tally of total delegated weight pointing (transitively) at Address.
    DelegatedToMe(Address),
    /// Issue #68: the admin address (set at initialise time).
    Admin,
    /// Issue #68: when `true`, admin veto power is active; when `false`, it has been disabled.
    VetoPowerEnabled,
}

// ================================================================
// Contract
// ================================================================

#[contract]
pub struct GovContract;

#[contractimpl]
impl GovContract {
    // ── Initialise ────────────────────────────────────────────────

    pub fn initialize(
        env: Env,
        iln_contract: Address,
        gov_token: Address,
        admin: Address,
    ) -> Result<(), GovernanceError> {
        if env.storage().instance().has(&StorageKey::IlnContract) {
            return Err(GovernanceError::AlreadyInitialized);
        }
        env.storage()
            .instance()
            .set(&StorageKey::IlnContract, &iln_contract);
        env.storage()
            .instance()
            .set(&StorageKey::GovToken, &gov_token);
        env.storage()
            .instance()
            .set(&StorageKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&StorageKey::VetoPowerEnabled, &true);
        env.storage()
            .instance()
            .set(&StorageKey::MinQuorumBps, &DEFAULT_MIN_QUORUM_BPS);
        env.storage()
            .instance()
            .set(&StorageKey::ProposalCount, &0_u64);
        Ok(())
    }

    /// Returns the configured minimum quorum in bps (e.g. 1000 = 10%).
    pub fn get_min_quorum_bps(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&StorageKey::MinQuorumBps)
            .unwrap_or(DEFAULT_MIN_QUORUM_BPS)
    }

    /// Updates the minimum quorum configuration.
    ///
    /// Authorization: the configured ILN contract address must authorize.
    pub fn set_min_quorum_bps(env: Env, min_quorum_bps: u32) -> Result<(), GovernanceError> {
        if min_quorum_bps == 0 || min_quorum_bps > 10_000 {
            return Err(GovernanceError::InvalidQuorumBps);
        }

        let iln_contract: Address = env
            .storage()
            .instance()
            .get(&StorageKey::IlnContract)
            .unwrap();
        iln_contract.require_auth();

        env.storage()
            .instance()
            .set(&StorageKey::MinQuorumBps, &min_quorum_bps);
        Ok(())
    }

    // ── Issue #59: create_proposal ────────────────────────────────

    pub fn create_proposal(
        env: Env,
        proposer: Address,
        action_type: ProposalAction,
        description_hash: BytesN<32>,
        proposed_value: i128,
    ) -> Result<u64, GovernanceError> {
        proposer.require_auth();

        let count: u64 = env.storage().instance().get(&StorageKey::ProposalCount).unwrap_or(0);
        let id = count + 1;

        let now = env.ledger().timestamp();
        let voting_end = now + 259_200;

        let proposal = GovernanceProposal {
            id,
            proposer: proposer.clone(),
            description_hash,
            action_type,
            proposed_value,
            status: ProposalStatus::Active,
            votes_for: 0,
            votes_against: 0,
            created_at: now,
            voting_end,
        };

        let token_addr: Address = env.storage().instance().get(&StorageKey::GovToken).unwrap();
        let token = TokenClient::new(&env, &token_addr);
        let proposer_weight = token.balance(&proposer);
        env.storage().persistent().set(
            &StorageKey::VoteWeightSnapshot(id, proposer.clone()),
            &proposer_weight,
        );

        env.storage().persistent().set(&StorageKey::Proposal(id), &proposal);
        env.storage().instance().set(&StorageKey::ProposalCount, &id);

        Ok(id)
    }

    // ── Issue #64: delegate_votes ─────────────────────────────────

    /// Delegate the caller's voting weight to `delegate`.
    ///
    /// * Cannot delegate to self.
    /// * Rejects delegation if it would create a cycle in the chain.
    /// * Re-delegation overwrites the previous delegation and adjusts the
    ///   `DelegatedToMe` tally on both old and new terminal nodes.
    ///
    /// Emits `VotesDelegated`.
    pub fn delegate_votes(
        env: Env,
        delegator: Address,
        delegate: Address,
    ) -> Result<(), GovernanceError> {
        delegator.require_auth();

        if delegator == delegate {
            return Err(GovernanceError::CannotDelegateToSelf);
        }

        // ── Cycle detection ───────────────────────────────────────
        // Walk the forward chain from `delegate`.
        // If we reach `delegator` at any point, the new edge would close a cycle.
        let mut cursor: Option<Address> = Self::get_delegate_raw(&env, &delegate);
        let mut depth = 0u32;
        while let Some(ref next) = cursor.clone() {
            if depth >= MAX_DELEGATION_DEPTH {
                break;
            }
            if *next == delegator {
                return Err(GovernanceError::DelegationCyclePrevented);
            }
            cursor = Self::get_delegate_raw(&env, next);
            depth += 1;
        }

        // ── Find the terminal node for `delegate` ─────────────────
        let terminal = Self::resolve_terminal(&env, &delegate);

        // ── Remove weight from old terminal if re-delegating ──────
        if let Some(old_delegate) = Self::get_delegate_raw(&env, &delegator) {
            let old_terminal = Self::resolve_terminal(&env, &old_delegate);
            let delegator_balance = Self::get_own_balance_for_delegation(&env, &delegator);
            Self::adjust_delegated_to_me(&env, &old_terminal, -(delegator_balance as i128));
        }

        // ── Store forward pointer ─────────────────────────────────
        env.storage()
            .persistent()
            .set(&StorageKey::Delegation(delegator.clone()), &delegate);

        // ── Add weight to new terminal ────────────────────────────
        let delegator_balance = Self::get_own_balance_for_delegation(&env, &delegator);
        Self::adjust_delegated_to_me(&env, &terminal, delegator_balance as i128);

        env.events().publish_event(&VotesDelegated { delegator, delegate });

        Ok(())
    }

    // ── Issue #64: undelegate_votes ───────────────────────────────

    /// Remove the caller's delegation.
    ///
    /// Emits `VotesUndelegated`.
    pub fn undelegate_votes(env: Env, delegator: Address) -> Result<(), GovernanceError> {
        delegator.require_auth();

        if let Some(old_delegate) = Self::get_delegate_raw(&env, &delegator) {
            let old_terminal = Self::resolve_terminal(&env, &old_delegate);
            let delegator_balance = Self::get_own_balance_for_delegation(&env, &delegator);
            Self::adjust_delegated_to_me(&env, &old_terminal, -(delegator_balance as i128));

            env.storage()
                .persistent()
                .remove(&StorageKey::Delegation(delegator.clone()));
        }

        env.events().publish_event(&VotesUndelegated { delegator });

        Ok(())
    }

    // ── Issue #64: get_delegate ───────────────────────────────────

    /// Return the direct delegate for `addr`, if any.
    pub fn get_delegate(env: Env, addr: Address) -> Option<Address> {
        Self::get_delegate_raw(&env, &addr)
    }

    // ── cast_vote ─────────────────────────────────────────────────

    /// Cast a vote on an active proposal.
    ///
    /// Issue #64: weight = own snapshot balance + DelegatedToMe tally.
    pub fn cast_vote(
        env: Env,
        voter: Address,
        proposal_id: u64,
        support: bool,
    ) -> Result<(), GovernanceError> {
        voter.require_auth();

        let mut proposal: GovernanceProposal = env
            .storage()
            .persistent()
            .get(&StorageKey::Proposal(proposal_id))
            .ok_or(GovernanceError::ProposalNotFound)?;

        let now = env.ledger().timestamp();
        if now >= proposal.voting_end {
            return Err(GovernanceError::VotingEnded);
        }
        if proposal.status != ProposalStatus::Active {
            return Err(GovernanceError::ProposalNotActive);
        }

        let voted_key = StorageKey::HasVoted(proposal_id, voter.clone());
        if env.storage().temporary().has(&voted_key) {
            return Err(GovernanceError::AlreadyVoted);
        }

        let token_addr: Address = env.storage().instance().get(&StorageKey::GovToken).unwrap();
        let token = TokenClient::new(&env, &token_addr);

        // Own snapshotted (or current) balance.
        let snapshot_key = StorageKey::VoteWeightSnapshot(proposal_id, voter.clone());
        let own_balance: i128 = match env.storage().persistent().get(&snapshot_key) {
            Some(w) => w,
            None => {
                let current = token.balance(&voter);
                env.storage().persistent().set(&snapshot_key, &current);
                current
            }
        };

        // Issue #64: add delegated weight.
        let delegated: i128 = env
            .storage()
            .persistent()
            .get(&StorageKey::DelegatedToMe(voter.clone()))
            .unwrap_or(0_i128);

        let weight = own_balance + delegated;

        if weight == 0 {
            return Err(GovernanceError::NoVotingPower);
        }

        if support {
            proposal.votes_for += weight;
        } else {
            proposal.votes_against += weight;
        }

        env.storage().temporary().set(&voted_key, &true);
        env.storage().temporary().extend_ttl(
            &voted_key,
            VOTE_RECEIPT_TTL_THRESHOLD_LEDGERS,
            VOTE_RECEIPT_TTL_LEDGERS,
        );
        env.storage()
            .persistent()
            .set(&StorageKey::Proposal(proposal_id), &proposal);

        env.events().publish_event(&VoteCast { proposal_id, voter, support, weight });

        Ok(())
    }

    // ── execute_proposal ─────────────────────────────────────────

    pub fn execute_proposal(
        env: Env,
        proposal_id: u64,
        total_supply: i128,
    ) -> Result<(), GovernanceError> {
        let mut proposal: GovernanceProposal = env
            .storage()
            .persistent()
            .get(&StorageKey::Proposal(proposal_id))
            .ok_or(GovernanceError::ProposalNotFound)?;

        let now = env.ledger().timestamp();
        if now < proposal.voting_end {
            return Err(GovernanceError::VotingOngoing);
        }
        if proposal.status != ProposalStatus::Active {
            return Err(GovernanceError::AlreadyResolved);
        }

        let total_votes = proposal.votes_for + proposal.votes_against;
        let min_quorum_bps: u32 = env
            .storage()
            .instance()
            .get(&StorageKey::MinQuorumBps)
            .unwrap_or(DEFAULT_MIN_QUORUM_BPS);
        let quorum = if total_supply <= 0 {
            0_i128
        } else {
            total_supply
                .saturating_mul(min_quorum_bps as i128)
                / 10_000_i128
        };

        if total_votes < quorum {
            proposal.status = ProposalStatus::Rejected;
            env.storage().persistent().set(&StorageKey::Proposal(proposal_id), &proposal);
            return Err(GovernanceError::QuorumNotReached);
        }

        if proposal.votes_for <= proposal.votes_against {
            proposal.status = ProposalStatus::Rejected;
            env.storage().persistent().set(&StorageKey::Proposal(proposal_id), &proposal);
            return Err(GovernanceError::ProposalRejected);
        }

        proposal.status = ProposalStatus::Passed;

        let iln_contract: Address = env.storage().instance().get(&StorageKey::IlnContract).unwrap();

        match proposal.action_type.clone() {
            ProposalAction::UpdateFeeRate(rate) => {
                let args: Vec<soroban_sdk::Val> = vec![&env, rate.into_val(&env)];
                env.invoke_contract::<()>(&iln_contract, &Symbol::new(&env, "update_fee_rate"), args);
            }
            ProposalAction::AddToken(token) => {
                let args: Vec<soroban_sdk::Val> = vec![&env, token.into_val(&env)];
                env.invoke_contract::<()>(&iln_contract, &Symbol::new(&env, "add_token"), args);
            }
            ProposalAction::RemoveToken(token) => {
                let args: Vec<soroban_sdk::Val> = vec![&env, token.into_val(&env)];
                env.invoke_contract::<()>(&iln_contract, &Symbol::new(&env, "remove_token"), args);
            }
            ProposalAction::UpdateMaxDiscountRate(rate) => {
                let args: Vec<soroban_sdk::Val> = vec![&env, rate.into_val(&env)];
                env.invoke_contract::<()>(&iln_contract, &Symbol::new(&env, "update_max_discount"), args);
            }
        }

        proposal.status = ProposalStatus::Executed;
        env.storage().persistent().set(&StorageKey::Proposal(proposal_id), &proposal);

        Ok(())
    }

    // ── Issue #68: veto_proposal ──────────────────────────────────

    /// Veto an active (or passed) proposal, transitioning it to `Vetoed` status.
    ///
    /// * Only the stored admin may call this function.
    /// * The admin veto power must still be enabled; it cannot be used after
    ///   governance has called `disable_veto_power()`.
    /// * Only proposals in `Active` or `Passed` status can be vetoed — an
    ///   already-executed or already-vetoed proposal is not vetoable.
    ///
    /// Emits `ProposalVetoed { proposal_id, admin, reason_hash }`.
    pub fn veto_proposal(
        env: Env,
        proposal_id: u64,
        reason_hash: BytesN<32>,
    ) -> Result<(), GovernanceError> {
        // ── Auth: only admin ──────────────────────────────────────
        let admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .unwrap();
        admin.require_auth();

        // ── Guard: veto power must still be enabled ───────────────
        let enabled: bool = env
            .storage()
            .instance()
            .get(&StorageKey::VetoPowerEnabled)
            .unwrap_or(false);
        if !enabled {
            return Err(GovernanceError::VetoPowerDisabled);
        }

        // ── Load proposal ─────────────────────────────────────────
        let mut proposal: GovernanceProposal = env
            .storage()
            .persistent()
            .get(&StorageKey::Proposal(proposal_id))
            .ok_or(GovernanceError::ProposalNotFound)?;

        // ── Guard: only Active or Passed proposals are vetoable ───
        match proposal.status {
            ProposalStatus::Active | ProposalStatus::Passed => {}
            _ => return Err(GovernanceError::NotVetoable),
        }

        proposal.status = ProposalStatus::Vetoed;
        env.storage()
            .persistent()
            .set(&StorageKey::Proposal(proposal_id), &proposal);

        env.events().publish_event(&ProposalVetoed {
            proposal_id,
            admin,
            reason_hash,
        });

        Ok(())
    }

    // ── Issue #68: disable_veto_power ─────────────────────────────

    /// Permanently disable the admin veto power.
    ///
    /// Authorization: the configured ILN contract address must authorize
    /// (same pattern used by `set_min_quorum_bps` — governance votes trigger
    /// this via a cross-contract call from the ILN contract).
    ///
    /// Once disabled this cannot be re-enabled; it is a one-way switch
    /// intended to be called before mainnet launch.
    pub fn disable_veto_power(env: Env) -> Result<(), GovernanceError> {
        let iln_contract: Address = env
            .storage()
            .instance()
            .get(&StorageKey::IlnContract)
            .unwrap();
        iln_contract.require_auth();

        env.storage()
            .instance()
            .set(&StorageKey::VetoPowerEnabled, &false);

        Ok(())
    }

    /// Returns `true` when admin veto power is still active.
    pub fn is_veto_power_enabled(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&StorageKey::VetoPowerEnabled)
            .unwrap_or(false)
    }

    // ── Getters ──────────────────────────────────────────────────

    pub fn get_proposal(env: Env, proposal_id: u64) -> Result<GovernanceProposal, GovernanceError> {
        env.storage()
            .persistent()
            .get(&StorageKey::Proposal(proposal_id))
            .ok_or(GovernanceError::ProposalNotFound)
    }

    pub fn has_voted(env: Env, voter: Address, proposal_id: u64) -> bool {
        env.storage()
            .temporary()
            .has(&StorageKey::HasVoted(proposal_id, voter))
    }

    // ── Private helpers ──────────────────────────────────────────

    fn get_delegate_raw(env: &Env, addr: &Address) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::Delegation(addr.clone()))
    }

    /// Walk forward pointers to find the terminal node (one with no further delegate).
    fn resolve_terminal(env: &Env, start: &Address) -> Address {
        let mut current = start.clone();
        let mut depth = 0u32;
        loop {
            if depth >= MAX_DELEGATION_DEPTH {
                break;
            }
            match Self::get_delegate_raw(env, &current) {
                Some(next) => {
                    current = next;
                    depth += 1;
                }
                None => break,
            }
        }
        current
    }

    /// Return the token balance of `addr` to use as the delegation weight.
    fn get_own_balance_for_delegation(env: &Env, addr: &Address) -> i128 {
        let token_addr: Address = env.storage().instance().get(&StorageKey::GovToken).unwrap();
        let token = TokenClient::new(env, &token_addr);
        token.balance(addr)
    }

    /// Add `delta` (may be negative) to the `DelegatedToMe` tally of `addr`.
    fn adjust_delegated_to_me(env: &Env, addr: &Address, delta: i128) {
        let key = StorageKey::DelegatedToMe(addr.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0_i128);
        let updated = current + delta;
        if updated <= 0 {
            env.storage().persistent().remove(&key);
        } else {
            env.storage().persistent().set(&key, &updated);
        }
    }
}

#[cfg(test)]
mod test;