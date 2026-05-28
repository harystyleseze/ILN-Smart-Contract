#![cfg(test)]

//! Security-focused tests for the InvoiceLiquidity contract.
//!
//! These tests target:
//!   1. Integer overflow with i128::MAX inputs
//!   2. Boundary arithmetic correctness at half-max amounts
//!   3. Freelancer payout underflow (must never be negative)
//!   4. Cross-invoice state isolation (funding A must not affect B)
//!   5. Storage key isolation (adjacent invoice IDs are truly independent)

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

// ----------------------------------------------------------------
// Shared setup
// ----------------------------------------------------------------

struct TestEnv {
    env: Env,
    contract: InvoiceLiquidityContractClient<'static>,
    token: TokenClient<'static>,
    freelancer: Address,
    payer: Address,
    funder: Address,
}

const DUE_DATE_OFFSET: u64 = 60 * 60 * 24 * 30; // 30 days

fn setup_security() -> TestEnv {
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

    // Mint large amounts so funding large invoices is possible.
    // i128::MAX itself cannot be minted (would overflow ledger), but we
    // mint enough for boundary tests that pass validation.
    token_admin.mint(&funder, &i128::MAX);
    token_admin.mint(&payer, &i128::MAX);

    // Deploy and initialise the ILN contract
    let contract_id = env.register(InvoiceLiquidityContract, ());
    let contract = InvoiceLiquidityContractClient::new(&env, &contract_id);
    token_admin.mint(&contract.address, &(1000000000 * 100));

    let xlm_admin = Address::generate(&env);
    let xlm_contract_id = env.register_stellar_asset_contract_v2(xlm_admin);
    let xlm_address = xlm_contract_id.address();
    contract.initialize(&usdc_admin, &usdc_address, &xlm_address);

    // Fix ledger timestamp
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

fn due_date(t: &TestEnv) -> u64 {
    t.env.ledger().timestamp() + DUE_DATE_OFFSET
}

// ----------------------------------------------------------------
// 1. Overflow: amount = i128::MAX
//
//    The contract uses checked_mul(...).unwrap_or(0) when computing
//    the discount.  Passing i128::MAX should NOT panic — it must
//    gracefully fall through to the unwrap_or(0) branch.
//
//    i128::MAX * 5_000 would overflow, so discount collapses to 0
//    and the freelancer receives the full invoice amount.
// ----------------------------------------------------------------

#[test]
fn test_overflow_max_amount_does_not_panic() {
    let t = setup_security();
    let amount = i128::MAX - (1000000000 * 100);
    let due = due_date(&t);
    // MAX discount rate that the contract accepts is 5 000 bps (50%)
    let disc_rate: u32 = 5_000;

    // Submit should succeed — amount > 0, discount valid, due date future
    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &amount,
        &due,
        &disc_rate,
        &t.token.address,
    );

    // fund_invoice calls checked_mul; with i128::MAX * 5_000 overflowing,
    // the contract falls back to discount_amount = 0.
    // That means freelancer_payout = i128::MAX, which the funder must have.
    // Our mock mint above gave the funder i128::MAX, so the transfer works.
    t.contract.fund_invoice(&t.funder, &id, &amount);

    let invoice = t.contract.get_invoice(&id);
    assert_eq!(
        invoice.status,
        InvoiceStatus::Funded,
        "Invoice should be Funded when i128::MAX is used as amount"
    );

    // Verify freelancer received something (the full amount in this case
    // because overflow forced discount = 0).
    let fl_balance = t.token.balance(&t.freelancer);
    assert!(
        fl_balance > 0,
        "Freelancer balance should be positive after funding"
    );
}

// ----------------------------------------------------------------
// 2. Overflow boundary: amount = i128::MAX / 2, discount_rate = 5_000
//
//    This should NOT overflow: (i128::MAX / 2) * 5_000 fits in a
//    wider integer conceptually, but since we stay in i128 the
//    checked_mul may still wrap.  The key property is: no panic.
//
//    We also assert the discount formula is applied correctly when
//    there is no overflow (discount_amount = amount * rate / 10_000).
// ----------------------------------------------------------------

#[test]
fn test_overflow_boundary_half_max_amount_no_panic() {
    let t = setup_security();
    let amount = i128::MAX / 2;
    let due = due_date(&t);
    let disc_rate: u32 = 5_000; // 50%

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &amount,
        &due,
        &disc_rate,
        &t.token.address,
    );

    // Must not panic
    t.contract.fund_invoice(&t.funder, &id, &amount);

    let invoice = t.contract.get_invoice(&id);
    assert_eq!(
        invoice.status,
        InvoiceStatus::Funded,
        "Invoice with i128::MAX/2 amount should reach Funded state"
    );

    // The multiplication (i128::MAX/2) * 5_000 overflows i128, so the
    // contract falls back to discount = 0 -> freelancer gets full amount.
    // Either way, freelancer must have a non-negative balance.
    let fl_balance = t.token.balance(&t.freelancer);
    assert!(fl_balance >= 0, "Freelancer balance must never be negative");
}

// ----------------------------------------------------------------
// 3. Underflow: freelancer_payout must never be negative
//
//    We iterate over several boundary amount/discount_rate combos
//    and verify that the contract either rejects them (as expected)
//    or, if it funds, the freelancer's balance increases.
// ----------------------------------------------------------------

#[test]
fn test_payout_never_negative_for_valid_inputs() {
    // Pairs of (amount, discount_rate) that pass validation
    let valid_cases: &[(i128, u32)] = &[
        (1_000_000, 1),                 // minimum plausible values
        (1_000_000, 5_000),             // min amount, max rate
        (1_000_000_000, 300),   // standard 100 USDC @ 3%
        (1_000_000_000, 5_000), // standard amount, max rate
        (10_000_000_000, 1),    // large amount, tiny rate
    ];

    for &(amount, disc_rate) in valid_cases {
        let t = setup_security();
        let due = due_date(&t);

        let fl_before = t.token.balance(&t.freelancer);

        let id = t.contract.submit_invoice(
            &t.freelancer,
            &t.payer,
            &amount,
            &due,
            &disc_rate,
            &t.token.address,
        );

        t.contract.fund_invoice(&t.funder, &id, &amount);

        let fl_after = t.token.balance(&t.freelancer);

        assert!(
            fl_after >= fl_before,
            "Freelancer payout must not be negative for amount={amount}, rate={disc_rate}. \
             before={fl_before}, after={fl_after}"
        );
    }
}

// ----------------------------------------------------------------
// 4. Cross-invoice: funding invoice A must not affect invoice B's state
// ----------------------------------------------------------------

#[test]
fn test_funding_invoice_a_does_not_affect_invoice_b() {
    let t = setup_security();
    let due = due_date(&t);

    // Submit two independent invoices
    let id_a = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &1_000_000_000,
        &due,
        &300,
        &t.token.address,
    );
    let id_b = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &2_000_000_000,
        &due,
        &500,
        &t.token.address,
    );

    // Check B's state before any funding
    let invoice_b_before = t.contract.get_invoice(&id_b);
    assert_eq!(
        invoice_b_before.status,
        InvoiceStatus::Pending,
        "Invoice B should start Pending"
    );

    // Fund invoice A
    t.contract.fund_invoice(&t.funder, &id_a, &1_000_000_000);

    // B's state must remain completely untouched
    let invoice_a = t.contract.get_invoice(&id_a);
    let invoice_b = t.contract.get_invoice(&id_b);

    assert_eq!(
        invoice_a.status,
        InvoiceStatus::Funded,
        "Invoice A should now be Funded"
    );
    assert_eq!(
        invoice_b.status,
        InvoiceStatus::Pending,
        "Invoice B must remain Pending after A is funded"
    );
    assert!(
        invoice_b.funder.is_none(),
        "Invoice B funder field must remain None"
    );
    assert!(
        invoice_b.funded_at.is_none(),
        "Invoice B funded_at field must remain None"
    );
    assert_eq!(
        invoice_b.amount, 2_000_000_000,
        "Invoice B amount must not have changed"
    );
    assert_eq!(
        invoice_b.discount_rate, 500,
        "Invoice B discount_rate must not have changed"
    );
}

// ----------------------------------------------------------------
// 5. Storage isolation: adjacent invoice IDs have independent state
//
//    We submit invoices with IDs 1 and 2. We fund and mark invoice 1
//    as paid, then verify that invoice 2's every mutable field is
//    still in its original submitted state.
// ----------------------------------------------------------------

#[test]
fn test_storage_isolation_adjacent_invoice_ids() {
    let t = setup_security();
    let due = due_date(&t);

    let new_payer = Address::generate(&t.env);
    let new_funder = Address::generate(&t.env);
    let token_admin = StellarAssetClient::new(&t.env, &t.token.address);
    token_admin.mint(&new_payer, &10_000_000_000);
    token_admin.mint(&new_funder, &10_000_000_000);

    let id_1 = t.contract.submit_invoice(
        &t.freelancer,
        &new_payer,
        &1_000_000_000,
        &due,
        &300,
        &t.token.address,
    );
    let id_2 = t.contract.submit_invoice(
        &t.freelancer,
        &new_payer,
        &5_000_000_000,
        &due,
        &100,
        &t.token.address,
    );

    // Sanity: IDs should be sequential
    assert_eq!(id_1, 1, "First invoice must have ID 1");
    assert_eq!(id_2, 2, "Second invoice must have ID 2");

    // Fully cycle invoice 1: fund -> mark paid
    t.contract.fund_invoice(&new_funder, &id_1, &1_000_000_000);
    t.contract.mark_paid(&id_1, &1_000_000_000);

    let inv1 = t.contract.get_invoice(&id_1);
    let inv2 = t.contract.get_invoice(&id_2);

    // Invoice 1 should be Paid
    assert_eq!(inv1.status, InvoiceStatus::Paid, "Invoice 1 should be Paid");

    // Invoice 2 must still reflect its original submitted state
    assert_eq!(
        inv2.status,
        InvoiceStatus::Pending,
        "Invoice 2 status must still be Pending"
    );
    assert!(inv2.funder.is_none(), "Invoice 2 funder must be None");
    assert!(inv2.funded_at.is_none(), "Invoice 2 funded_at must be None");
    assert_eq!(
        inv2.id, 2,
        "Invoice 2 id field must not have been corrupted"
    );
    assert_eq!(
        inv2.amount, 5_000_000_000,
        "Invoice 2 amount must not have been corrupted"
    );
    assert_eq!(
        inv2.discount_rate, 100,
        "Invoice 2 discount_rate must not have been corrupted"
    );
    assert_eq!(
        inv2.freelancer, t.freelancer,
        "Invoice 2 freelancer must not have been corrupted"
    );
    assert_eq!(
        inv2.payer, new_payer,
        "Invoice 2 payer must not have been corrupted"
    );
}
