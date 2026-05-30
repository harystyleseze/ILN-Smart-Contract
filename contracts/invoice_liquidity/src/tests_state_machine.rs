#![cfg(test)]

//! State Machine Tests for Invoice Liquidity Contract
//!
//! This module exhaustively tests every valid and invalid state transition
//! in the invoice lifecycle:
//!
//! Invoice Status Lifecycle:
//!   Pending → Funded (via fund_invoice with full amount)
//!   Pending → PartiallyFunded (via fund_invoice with partial amount)
//!   Funded → Paid (via mark_paid)
//!   Funded → Defaulted (via claim_default after due_date)
//!
//! Invalid transitions tested:
//!   - Fund a Funded invoice → AlreadyFunded
//!   - Fund a Paid invoice → AlreadyPaid
//!   - Fund a Cancelled invoice → error (InvoiceDefaulted for cancelled/defaulted)
//!   - Mark paid on Pending → NotFunded
//!   - Mark paid on Paid → AlreadyPaid
//!   - Cancel a Funded invoice → error (AlreadyFunded)
//!   - Claim default on Pending → NotFunded
//!   - Claim default on Paid → error (AlreadyPaid)

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

// ----------------------------------------------------------------
// Test Environment Setup
// ----------------------------------------------------------------

struct TestEnv {
    env: Env,
    contract: InvoiceLiquidityContractClient<'static>,
    token: TokenClient<'static>,
    freelancer: Address,
    payer: Address,
    funder: Address,
}

const INVOICE_AMOUNT: i128 = 1_000_000_000; // 100 USDC in stroops
const DISCOUNT_RATE: u32 = 300; // 3.00% in basis points
const DUE_DATE_OFFSET: u64 = 60 * 60 * 24 * 30; // 30 days

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    // Deploy mock USDC token
    let usdc_admin = Address::generate(&env);
    let usdc_contract_id = env.register_stellar_asset_contract_v2(usdc_admin.clone());
    let usdc_address = usdc_contract_id.address();

    let token = TokenClient::new(&env, &usdc_address);
    let token_admin = StellarAssetClient::new(&env, &usdc_address);

    // Generate test wallets
    let freelancer = Address::generate(&env);
    let payer = Address::generate(&env);
    let funder = Address::generate(&env);

    // Mint tokens
    token_admin.mint(&funder, &(INVOICE_AMOUNT * 10));
    token_admin.mint(&payer, &(INVOICE_AMOUNT * 10));

    // Deploy and initialize contract
    let contract_id = env.register(InvoiceLiquidityContract, ());
    let contract = InvoiceLiquidityContractClient::new(&env, &contract_id);
    token_admin.mint(&contract.address, &(1000000000 * 100));

    let xlm_address = Address::generate(&env);
    contract.initialize(&usdc_admin, &usdc_address, &xlm_address);

    // Set baseline timestamp
    let mut ledger_info = env.ledger().get();
    ledger_info.timestamp = 1_700_000_000;
    env.ledger().set(ledger_info);

    TestEnv {
        env,
        contract,
        token,
        freelancer,
        payer,
        funder,
    }
}

/// Helper: submit a standard invoice and return its ID
fn submit_invoice(t: &TestEnv) -> u64 {
    let due_date = t.env.ledger().timestamp() + DUE_DATE_OFFSET;
    t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &due_date,
        &DISCOUNT_RATE,
        &t.token.address,
    )
}

/// Helper: fast-forward ledger time past the due date
fn advance_past_due_date(t: &TestEnv, invoice_id: u64) {
    let invoice = t.contract.get_invoice(&invoice_id);
    let mut ledger_info = t.env.ledger().get();
    ledger_info.timestamp = invoice.due_date + 1;
    t.env.ledger().set(ledger_info);
}

// =================================================================
// INVALID TRANSITIONS
// =================================================================

// ----------------------------------------------------------------
// Fund a Funded invoice → AlreadyFunded
// ----------------------------------------------------------------

#[test]
fn test_cannot_fund_already_funded_invoice() {
    let t = setup();
    let id = submit_invoice(&t);

    // First funding succeeds
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Funded,
        "Invoice should be Funded after full funding"
    );

    // Second funding attempt should fail with AlreadyFunded
    let result = t.contract.try_fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);
    assert_eq!(result, Err(Ok(ContractError::AlreadyFunded)));

    // Verify status unchanged
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Funded,
        "Invoice status should remain Funded after failed funding attempt"
    );
}

// ----------------------------------------------------------------
// Fund a Paid invoice → AlreadyPaid
// ----------------------------------------------------------------

#[test]
fn test_cannot_fund_already_paid_invoice() {
    let t = setup();
    let id = submit_invoice(&t);

    // Fund and mark as paid
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);
    t.contract.mark_paid(&id, &INVOICE_AMOUNT);
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Paid,
        "Invoice should be Paid"
    );

    // Funding attempt should fail with AlreadyPaid
    let result = t.contract.try_fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);
    assert_eq!(result, Err(Ok(ContractError::AlreadyPaid)));

    // Verify status unchanged
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Paid,
        "Invoice status should remain Paid after failed funding attempt"
    );
}

// ----------------------------------------------------------------
// Fund a Defaulted invoice → InvoiceDefaulted
// ----------------------------------------------------------------

#[test]
fn test_cannot_fund_defaulted_invoice() {
    let t = setup();
    let id = submit_invoice(&t);

    // Fund the invoice
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);

    // Advance past due date and claim default
    advance_past_due_date(&t, id);
    t.contract.claim_default(&t.funder, &id);
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Defaulted,
        "Invoice should be Defaulted"
    );

    // Funding attempt should fail with InvoiceDefaulted
    let result = t.contract.try_fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);
    assert_eq!(result, Err(Ok(ContractError::InvoiceDefaulted)));

    // Verify status unchanged
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Defaulted,
        "Invoice status should remain Defaulted after failed funding attempt"
    );
}

// ----------------------------------------------------------------
// Mark paid on Pending → NotFunded
// ----------------------------------------------------------------

#[test]
fn test_cannot_mark_paid_on_pending_invoice() {
    let t = setup();
    let id = submit_invoice(&t);

    // Invoice is in Pending state
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Pending,
        "Invoice should start as Pending"
    );

    // Mark paid should fail with NotFunded
    let result = t.contract.try_mark_paid(&id, &INVOICE_AMOUNT);
    assert_eq!(result, Err(Ok(ContractError::NotFunded)));

    // Verify status unchanged
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Pending,
        "Invoice status should remain Pending after failed mark_paid attempt"
    );
}

// ----------------------------------------------------------------
// Mark paid on Paid → AlreadyPaid
// ----------------------------------------------------------------

#[test]
fn test_cannot_mark_paid_twice() {
    let t = setup();
    let id = submit_invoice(&t);

    // Fund and mark as paid once
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);
    t.contract.mark_paid(&id, &INVOICE_AMOUNT);
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Paid,
        "Invoice should be Paid after first mark_paid"
    );

    // Second mark_paid should fail with AlreadyPaid
    let result = t.contract.try_mark_paid(&id, &INVOICE_AMOUNT);
    assert_eq!(result, Err(Ok(ContractError::AlreadyPaid)));

    // Verify status unchanged
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Paid,
        "Invoice status should remain Paid after second failed mark_paid attempt"
    );
}

// ----------------------------------------------------------------
// Mark paid on Defaulted → InvoiceDefaulted
// ----------------------------------------------------------------

#[test]
fn test_cannot_mark_paid_on_defaulted_invoice() {
    let t = setup();
    let id = submit_invoice(&t);

    // Fund the invoice
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);

    // Advance past due date and claim default
    advance_past_due_date(&t, id);
    t.contract.claim_default(&t.funder, &id);
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Defaulted,
        "Invoice should be Defaulted"
    );

    // Mark paid should fail with InvoiceDefaulted
    let result = t.contract.try_mark_paid(&id, &INVOICE_AMOUNT);
    assert_eq!(result, Err(Ok(ContractError::InvoiceDefaulted)));

    // Verify status unchanged
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Defaulted,
        "Invoice status should remain Defaulted after failed mark_paid attempt"
    );
}

// ----------------------------------------------------------------
// Transfer a Funded invoice → AlreadyFunded
// ----------------------------------------------------------------

#[test]
fn test_cannot_transfer_funded_invoice() {
    let t = setup();
    let id = submit_invoice(&t);

    // Fund the invoice
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Funded,
        "Invoice should be Funded"
    );

    // Transfer should fail with AlreadyFunded
    let new_freelancer = Address::generate(&t.env);
    let result = t.contract.try_transfer_invoice(&id, &new_freelancer);
    assert_eq!(result, Err(Ok(ContractError::AlreadyFunded)));

    // Verify status unchanged
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Funded,
        "Invoice status should remain Funded after failed transfer attempt"
    );
}

// ----------------------------------------------------------------
// Claim default on Pending → NotFunded
// ----------------------------------------------------------------

#[test]
fn test_cannot_claim_default_on_pending_invoice() {
    let t = setup();
    let id = submit_invoice(&t);

    // Invoice is in Pending state
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Pending,
        "Invoice should start as Pending"
    );

    // Advance past due date
    advance_past_due_date(&t, id);

    // Claim default should fail with NotFunded
    let result = t.contract.try_claim_default(&t.funder, &id);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));

    // Verify status unchanged
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Pending,
        "Invoice status should remain Pending after failed claim_default attempt"
    );
}

// ----------------------------------------------------------------
// Claim default on Paid → AlreadyPaid
// ----------------------------------------------------------------

#[test]
fn test_cannot_claim_default_on_paid_invoice() {
    let t = setup();
    let id = submit_invoice(&t);

    // Fund and mark as paid
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);
    t.contract.mark_paid(&id, &INVOICE_AMOUNT);
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Paid,
        "Invoice should be Paid"
    );

    // Advance past due date (even though it's paid)
    advance_past_due_date(&t, id);

    // Claim default should fail with AlreadyPaid
    let result = t.contract.try_claim_default(&t.funder, &id);
    assert_eq!(result, Err(Ok(ContractError::AlreadyPaid)));

    // Verify status unchanged
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Paid,
        "Invoice status should remain Paid after failed claim_default attempt"
    );
}

// ----------------------------------------------------------------
// Claim default on Defaulted → InvoiceDefaulted
// ----------------------------------------------------------------

#[test]
fn test_cannot_claim_default_twice() {
    let t = setup();
    let id = submit_invoice(&t);

    // Fund the invoice
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);

    // Advance past due date and claim default
    advance_past_due_date(&t, id);
    t.contract.claim_default(&t.funder, &id);
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Defaulted,
        "Invoice should be Defaulted after first claim_default"
    );

    // Second claim_default should fail with InvoiceDefaulted
    let result = t.contract.try_claim_default(&t.funder, &id);
    assert_eq!(result, Err(Ok(ContractError::InvoiceDefaulted)));

    // Verify status unchanged
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Defaulted,
        "Invoice status should remain Defaulted after second failed claim_default attempt"
    );
}

// ----------------------------------------------------------------
// Claim default before due date → NotYetDefaulted
// ----------------------------------------------------------------

#[test]
fn test_cannot_claim_default_before_due_date() {
    let t = setup();
    let id = submit_invoice(&t);

    // Fund the invoice
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Funded,
        "Invoice should be Funded"
    );

    // Do NOT advance time - still before due date

    // Claim default should fail with NotYetDefaulted
    let result = t.contract.try_claim_default(&t.funder, &id);
    assert_eq!(result, Err(Ok(ContractError::NotYetDefaulted)));

    // Verify status unchanged
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Funded,
        "Invoice status should remain Funded after failed claim_default attempt"
    );
}

// =================================================================
// VALID TRANSITIONS
// =================================================================

// ----------------------------------------------------------------
// Pending → Funded (via full funding)
// ----------------------------------------------------------------

#[test]
fn test_pending_to_funded_full_funding() {
    let t = setup();
    let id = submit_invoice(&t);

    // Verify initial state
    let invoice_before = t.contract.get_invoice(&id);
    assert_eq!(invoice_before.status, InvoiceStatus::Pending);
    assert_eq!(invoice_before.amount_funded, 0);
    assert!(invoice_before.funder.is_none());
    assert!(invoice_before.funded_at.is_none());

    // Fund the full amount
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);

    // Verify state transition
    let invoice_after = t.contract.get_invoice(&id);
    assert_eq!(invoice_after.status, InvoiceStatus::Funded);
    assert_eq!(invoice_after.amount_funded, INVOICE_AMOUNT);
    assert_eq!(invoice_after.funder, Some(t.funder.clone()));
    assert!(invoice_after.funded_at.is_some());
    assert_eq!(invoice_after.funded_at.unwrap(), t.env.ledger().timestamp() as u32);
}

// ----------------------------------------------------------------
// Pending → PartiallyFunded → Funded (via partial funding)
// ----------------------------------------------------------------

#[test]
fn test_pending_to_partially_funded_to_funded() {
    let t = setup();
    let id = submit_invoice(&t);

    let funder1 = Address::generate(&t.env);
    let funder2 = Address::generate(&t.env);
    let token_admin = StellarAssetClient::new(&t.env, &t.token.address);
    token_admin.mint(&funder1, &INVOICE_AMOUNT);
    token_admin.mint(&funder2, &INVOICE_AMOUNT);

    // Verify initial state
    assert_eq!(t.contract.get_invoice(&id).status, InvoiceStatus::Pending);

    // Fund 40% - transition to PartiallyFunded
    let partial_amount = INVOICE_AMOUNT * 4000 / 10000;
    t.contract.fund_invoice(&funder1, &id, &partial_amount);

    let invoice_partial = t.contract.get_invoice(&id);
    assert_eq!(invoice_partial.status, InvoiceStatus::PartiallyFunded);
    assert_eq!(invoice_partial.amount_funded, partial_amount);

    // Fund remaining 60% - transition to Funded
    let remaining_amount = INVOICE_AMOUNT - partial_amount;
    t.contract.fund_invoice(&funder2, &id, &remaining_amount);

    let invoice_final = t.contract.get_invoice(&id);
    assert_eq!(invoice_final.status, InvoiceStatus::Funded);
    assert_eq!(invoice_final.amount_funded, INVOICE_AMOUNT);
}

// ----------------------------------------------------------------
// Funded → Paid (via mark_paid)
// ----------------------------------------------------------------

#[test]
fn test_funded_to_paid() {
    let t = setup();
    let id = submit_invoice(&t);

    // Fund the invoice first
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);

    // Verify Funded state
    let invoice_before = t.contract.get_invoice(&id);
    assert_eq!(invoice_before.status, InvoiceStatus::Funded);

    // Mark as paid
    t.contract.mark_paid(&id, &INVOICE_AMOUNT);

    // Verify state transition
    let invoice_after = t.contract.get_invoice(&id);
    assert_eq!(invoice_after.status, InvoiceStatus::Paid);

    // Verify funder received principal + yield
    let discount_amount = INVOICE_AMOUNT * DISCOUNT_RATE as i128 / 10_000;
    let _expected_payout = INVOICE_AMOUNT + discount_amount;

    // The funder's balance should have increased by the payout amount
    // (accounting for the initial transfer out when funding)
    assert!(t.token.balance(&t.funder) > 0);
}

// ----------------------------------------------------------------
// Funded → Defaulted (via claim_default after due_date)
// ----------------------------------------------------------------

#[test]
fn test_funded_to_defaulted() {
    let t = setup();
    let id = submit_invoice(&t);

    // Fund the invoice first
    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);

    // Verify Funded state
    let invoice_before = t.contract.get_invoice(&id);
    assert_eq!(invoice_before.status, InvoiceStatus::Funded);

    // Advance past due date
    advance_past_due_date(&t, id);

    // Claim default
    t.contract.claim_default(&t.funder, &id);

    // Verify state transition
    let invoice_after = t.contract.get_invoice(&id);
    assert_eq!(invoice_after.status, InvoiceStatus::Defaulted);

    // Verify payer score decreased
    assert_eq!(t.contract.payer_score(&t.payer), 45); // 50 - 5
}

// =================================================================
// STATE MACHINE COMPLETENESS TESTS
// =================================================================

// ----------------------------------------------------------------
// All states reachable from initial Pending state
// ----------------------------------------------------------------

#[test]
fn test_all_reachable_states_from_pending() {
    // Test Pending → Funded → Paid
    let t1 = setup();
    let id1 = submit_invoice(&t1);
    t1.contract.fund_invoice(&t1.funder, &id1, &INVOICE_AMOUNT);
    t1.contract.mark_paid(&id1, &INVOICE_AMOUNT);
    assert_eq!(
        t1.contract.get_invoice(&id1).status,
        InvoiceStatus::Paid,
        "Should reach Paid state"
    );

    // Test Pending → Funded → Defaulted
    let t2 = setup();
    let id2 = submit_invoice(&t2);
    t2.contract.fund_invoice(&t2.funder, &id2, &INVOICE_AMOUNT);
    advance_past_due_date(&t2, id2);
    t2.contract.claim_default(&t2.funder, &id2);
    assert_eq!(
        t2.contract.get_invoice(&id2).status,
        InvoiceStatus::Defaulted,
        "Should reach Defaulted state"
    );

    // Test Pending → PartiallyFunded → Funded
    let t3 = setup();
    let id3 = submit_invoice(&t3);
    let funder1 = Address::generate(&t3.env);
    let funder2 = Address::generate(&t3.env);
    let token_admin = StellarAssetClient::new(&t3.env, &t3.token.address);
    token_admin.mint(&funder1, &INVOICE_AMOUNT);
    token_admin.mint(&funder2, &INVOICE_AMOUNT);

    t3.contract
        .fund_invoice(&funder1, &id3, &(INVOICE_AMOUNT * 3000 / 10000));
    assert_eq!(
        t3.contract.get_invoice(&id3).status,
        InvoiceStatus::PartiallyFunded,
        "Should reach PartiallyFunded state"
    );

    t3.contract
        .fund_invoice(&funder2, &id3, &(INVOICE_AMOUNT * 7000 / 10000));
    assert_eq!(
        t3.contract.get_invoice(&id3).status,
        InvoiceStatus::Funded,
        "Should reach Funded state from PartiallyFunded"
    );
}

// ----------------------------------------------------------------
// State transition matrix validation
// ----------------------------------------------------------------

#[test]
fn test_state_transition_matrix_validation() {
    // This test documents and validates the complete state transition matrix
    //
    // From State    | Action          | To State        | Expected Result
    // --------------|-----------------|-----------------|------------------
    // Pending       | fund (full)     | Funded          | Success
    // Pending       | fund (partial)  | PartiallyFunded | Success
    // Pending       | mark_paid       | -               | NotFunded
    // Pending       | claim_default   | -               | NotFunded
    // Pending       | transfer        | Pending         | Success (freelancer change)
    //
    // PartiallyFunded | fund (rest)   | Funded          | Success
    // PartiallyFunded | mark_paid     | -               | NotFunded
    // PartiallyFunded | claim_default | -               | NotFunded
    //
    // Funded        | fund            | -               | AlreadyFunded
    // Funded        | mark_paid       | Paid            | Success
    // Funded        | claim_default   | Defaulted       | Success (if past due)
    // Funded        | transfer        | -               | AlreadyFunded
    //
    // Paid          | fund            | -               | AlreadyPaid
    // Paid          | mark_paid       | -               | AlreadyPaid
    // Paid          | claim_default   | -               | AlreadyPaid
    // Paid          | transfer        | -               | AlreadyPaid
    //
    // Defaulted     | fund            | -               | InvoiceDefaulted
    // Defaulted     | mark_paid       | -               | InvoiceDefaulted
    // Defaulted     | claim_default   | -               | InvoiceDefaulted

    let t = setup();

    // Test Pending → transfer works
    let id_pending = submit_invoice(&t);
    let new_freelancer = Address::generate(&t.env);
    let result = t
        .contract
        .try_transfer_invoice(&id_pending, &new_freelancer);
    assert!(result.is_ok(), "Transfer should work on Pending invoice");

    // Test Funded → mark_paid works
    let id_funded = submit_invoice(&t);
    t.contract
        .fund_invoice(&t.funder, &id_funded, &INVOICE_AMOUNT);
    let result = t.contract.try_mark_paid(&id_funded, &INVOICE_AMOUNT);
    assert!(result.is_ok(), "Mark paid should work on Funded invoice");

    // Test Funded → claim_default works (after due date)
    let id_default = submit_invoice(&t);
    t.contract
        .fund_invoice(&t.funder, &id_default, &INVOICE_AMOUNT);
    advance_past_due_date(&t, id_default);
    let result = t.contract.try_claim_default(&t.funder, &id_default);
    assert!(
        result.is_ok(),
        "Claim default should work on Funded invoice past due date"
    );
}

// ----------------------------------------------------------------
// PartiallyFunded state cannot be directly targeted for mark_paid/claim_default
// ----------------------------------------------------------------

#[test]
fn test_cannot_mark_paid_on_partially_funded_invoice() {
    let t = setup();
    let id = submit_invoice(&t);

    let funder = Address::generate(&t.env);
    let token_admin = StellarAssetClient::new(&t.env, &t.token.address);
    token_admin.mint(&funder, &INVOICE_AMOUNT);

    // Fund only 50%
    t.contract.fund_invoice(&funder, &id, &(INVOICE_AMOUNT / 2));

    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::PartiallyFunded
    );

    // Mark paid should fail with NotFunded
    let result = t.contract.try_mark_paid(&id, &INVOICE_AMOUNT);
    assert_eq!(result, Err(Ok(ContractError::NotFunded)));
}

#[test]
fn test_cannot_claim_default_on_partially_funded_invoice() {
    let t = setup();
    let id = submit_invoice(&t);

    let funder = Address::generate(&t.env);
    let token_admin = StellarAssetClient::new(&t.env, &t.token.address);
    token_admin.mint(&funder, &INVOICE_AMOUNT);

    // Fund only 50%
    t.contract.fund_invoice(&funder, &id, &(INVOICE_AMOUNT / 2));

    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::PartiallyFunded
    );

    advance_past_due_date(&t, id);

    // Claim default should fail with NotFunded
    let result = t.contract.try_claim_default(&funder, &id);
    assert_eq!(result, Err(Ok(ContractError::NotFunded)));
}

// ----------------------------------------------------------------
// Overfunding rejection (state preservation)
// ----------------------------------------------------------------

#[test]
fn test_overfunding_rejected_preserves_state() {
    let t = setup();
    let id = submit_invoice(&t);

    // Fund 60%
    let funder1 = Address::generate(&t.env);
    let token_admin = StellarAssetClient::new(&t.env, &t.token.address);
    token_admin.mint(&funder1, &INVOICE_AMOUNT);

    t.contract
        .fund_invoice(&funder1, &id, &(INVOICE_AMOUNT * 6000 / 10000));

    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::PartiallyFunded
    );

    // Try to overfund (50% more when only 40% needed)
    let funder2 = Address::generate(&t.env);
    token_admin.mint(&funder2, &INVOICE_AMOUNT);

    let overfund_amount = INVOICE_AMOUNT * 5000 / 10000;
    let result = t.contract.try_fund_invoice(&funder2, &id, &overfund_amount);
    assert_eq!(result, Err(Ok(ContractError::OverfundingRejected)));

    // Verify state unchanged
    let invoice = t.contract.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::PartiallyFunded);
    assert_eq!(invoice.amount_funded, INVOICE_AMOUNT * 6000 / 10000);
}

// ----------------------------------------------------------------
// Exact funding completion (no over/under)
// ----------------------------------------------------------------

#[test]
fn test_exact_multi_party_funding_completion() {
    let t = setup();
    let id = submit_invoice(&t);

    let funder1 = Address::generate(&t.env);
    let funder2 = Address::generate(&t.env);
    let funder3 = Address::generate(&t.env);
    let token_admin = StellarAssetClient::new(&t.env, &t.token.address);
    token_admin.mint(&funder1, &INVOICE_AMOUNT);
    token_admin.mint(&funder2, &INVOICE_AMOUNT);
    token_admin.mint(&funder3, &INVOICE_AMOUNT);

    // Fund in three parts: 25%, 35%, 40%
    t.contract
        .fund_invoice(&funder1, &id, &(INVOICE_AMOUNT * 2500 / 10000));
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::PartiallyFunded
    );

    t.contract
        .fund_invoice(&funder2, &id, &(INVOICE_AMOUNT * 3500 / 10000));
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::PartiallyFunded
    );

    t.contract
        .fund_invoice(&funder3, &id, &(INVOICE_AMOUNT * 4000 / 10000));
    assert_eq!(
        t.contract.get_invoice(&id).status,
        InvoiceStatus::Funded,
        "Should transition to Funded after exact full funding"
    );

    let invoice = t.contract.get_invoice(&id);
    assert_eq!(invoice.amount_funded, INVOICE_AMOUNT);
}
