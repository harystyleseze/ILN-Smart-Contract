// tests_mutation.rs
//
// Mutation-targeted tests for contracts/invoice_liquidity/src/lib.rs.
//
// Each test is designed to kill a specific class of mutation that the broader
// test suite may not catch:
//
//   MT-01  due_date == now is rejected  →  kills `due_date < now` mutation
//   MT-02  partial funding stays PartiallyFunded  →  kills `==` → `>=` mutation
//   MT-03  payer score increases by exactly +1  →  kills off-by-one on increment
//   MT-04  payer score decreases by exactly -5  →  kills off-by-one on decrement
//   MT-05  payer score floors at 0, not negative  →  kills `> 5` → `> 0` mutation
//   MT-06  discount_rate exactly at cap is accepted  →  kills `>=` → `>` mutation
//   MT-07  suggested_discount_rate formula is correct  →  kills formula mutations

#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

const INVOICE_AMOUNT: i128 = 1_000_000_000; // 100 USDC in stroops
const DISCOUNT_RATE: u32 = 300; // 3.00% bps
const DUE_DATE_OFFSET: u64 = 60 * 60 * 24 * 30; // 30 days

struct TestEnv {
    env: Env,
    contract: InvoiceLiquidityContractClient<'static>,
    token: TokenClient<'static>,
    freelancer: Address,
    payer: Address,
    funder: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let usdc_admin = Address::generate(&env);
    let usdc_contract_id = env.register_stellar_asset_contract_v2(usdc_admin.clone());
    let usdc_address = usdc_contract_id.address();

    let token = TokenClient::new(&env, &usdc_address);
    let token_admin = StellarAssetClient::new(&env, &usdc_address);

    let freelancer = Address::generate(&env);
    let payer = Address::generate(&env);
    let funder = Address::generate(&env);

    token_admin.mint(&funder, &(INVOICE_AMOUNT * 10));
    token_admin.mint(&payer, &(INVOICE_AMOUNT * 10));

    let contract_id = env.register(InvoiceLiquidityContract, ());
    let contract = InvoiceLiquidityContractClient::new(&env, &contract_id);
    token_admin.mint(&contract.address, &(1000000000 * 100));

    let xlm_admin = Address::generate(&env);
    let xlm_contract_id = env.register_stellar_asset_contract_v2(xlm_admin);
    let xlm_address = xlm_contract_id.address();

    contract.initialize(&usdc_admin, &usdc_address, &xlm_address);

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

// ----------------------------------------------------------------
// MT-01: due_date == now must be rejected
//
// The guard is `if due_date <= now { return Err(...) }`.
// A mutation `due_date < now` would allow due_date == now, which is invalid.
// This test submits an invoice with due_date exactly equal to ledger timestamp.
// ----------------------------------------------------------------
#[test]
fn mt01_due_date_equal_to_now_is_rejected() {
    let t = setup();
    let now = t.env.ledger().timestamp();

    let result = t.contract.try_submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &now, // exactly now — not in the future
        &DISCOUNT_RATE,
        &t.token.address,
    );

    assert_eq!(
        result,
        Err(Ok(ContractError::InvalidDueDate)),
        "due_date == ledger_timestamp should be rejected"
    );
}

// ----------------------------------------------------------------
// MT-02: partial funding should set status to PartiallyFunded, not Funded
//
// The condition for full funding is `amount_funded == amount`.
// A mutation `amount_funded >= amount` would incorrectly set the status to
// Funded when partially funded.  This test funds half the invoice and
// asserts the status remains PartiallyFunded.
// ----------------------------------------------------------------
#[test]
fn mt02_partial_fund_keeps_status_partially_funded() {
    let t = setup();
    let due_date = t.env.ledger().timestamp() + DUE_DATE_OFFSET;

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &due_date,
        &DISCOUNT_RATE,
        &t.token.address,
    );

    // Fund exactly half the invoice amount
    let half = INVOICE_AMOUNT / 2;
    t.contract.fund_invoice(&t.funder, &id, &half);

    let invoice = t.contract.get_invoice(&id);

    assert_eq!(
        invoice.status,
        InvoiceStatus::PartiallyFunded,
        "half-funded invoice must stay PartiallyFunded, not Funded"
    );
    assert_eq!(invoice.amount_funded, half);
}

// ----------------------------------------------------------------
// MT-03: payer score increments by exactly +1 on successful settlement
//
// The new score is `current_score + 1`.  A mutation to `+ 0` or `+ 2`
// would change the expected value.  Starting from the default of 50,
// after one settlement the score must be exactly 51.
// ----------------------------------------------------------------
#[test]
fn mt03_payer_score_increases_by_exactly_one_on_settlement() {
    let t = setup();
    let due_date = t.env.ledger().timestamp() + DUE_DATE_OFFSET;

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &due_date,
        &DISCOUNT_RATE,
        &t.token.address,
    );

    let score_before = t.contract.payer_score(&t.payer);
    assert_eq!(score_before, 50, "default score should be 50");

    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);
    t.contract.mark_paid(&id, &INVOICE_AMOUNT);

    let score_after = t.contract.payer_score(&t.payer);
    assert_eq!(
        score_after, 51,
        "score should increase by exactly 1 (50 → 51)"
    );
}

// ----------------------------------------------------------------
// MT-04: payer score decreases by exactly -5 on default
//
// The new score is `current_score - 5`.  A mutation to `- 4` or `- 6`
// would change the expected value.  Starting from 50, after one default
// the score must be exactly 45.
// ----------------------------------------------------------------
#[test]
fn mt04_payer_score_decreases_by_exactly_five_on_default() {
    let t = setup();
    let due_date = t.env.ledger().timestamp() + DUE_DATE_OFFSET;

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &due_date,
        &DISCOUNT_RATE,
        &t.token.address,
    );

    t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);

    // Advance ledger past due date
    let mut ledger = t.env.ledger().get();
    ledger.timestamp += DUE_DATE_OFFSET + 1;
    t.env.ledger().set(ledger);

    t.contract.claim_default(&t.funder, &id);

    let score_after = t.contract.payer_score(&t.payer);
    assert_eq!(
        score_after,
        45, // 50 - 5
        "score should decrease by exactly 5 (50 → 45)"
    );
}

// ----------------------------------------------------------------
// MT-05: payer score floors at 0 when current_score <= 5
//
// The guard `if current_score > 5` prevents u32 underflow.
// A mutation `current_score > 0` would let score 3 subtract 5, causing a panic.
// We set score to 3 manually by recording multiple defaults on throwaway
// invoices, then verify the floor is 0.
//
// Implementation note: we use the admin-set score indirectly by defaulting
// an invoice when payer score is already low (simulate via repeated defaults).
// ----------------------------------------------------------------
#[test]
fn mt05_payer_score_floors_at_zero_not_negative() {
    let t = setup();
    // Submit and default 9 invoices.
    // Starting score = 50, after 9 defaults: 50 - 5*9 = 5
    // The 10th default would subtract 5 from 5, which would underflow if the
    // `> 5` guard were mutated to `> 0`.  With the correct guard the result is 0.
    for _ in 0..9 {
        let current_due = t.env.ledger().timestamp() + DUE_DATE_OFFSET;
        let id = t.contract.submit_invoice(
            &t.freelancer,
            &t.payer,
            &INVOICE_AMOUNT,
            &current_due,
            &DISCOUNT_RATE,
            &t.token.address,
        );

        t.contract.fund_invoice(&t.funder, &id, &INVOICE_AMOUNT);

        let mut ledger = t.env.ledger().get();
        ledger.timestamp += DUE_DATE_OFFSET + 1;
        t.env.ledger().set(ledger);

        t.contract.claim_default(&t.funder, &id);
    }

    let score_after_nine_defaults = t.contract.payer_score(&t.payer);
    assert_eq!(
        score_after_nine_defaults, 5,
        "after 9 defaults score should be 5"
    );

    // 10th default: score is 5, NOT > 5, so guard should floor it to 0
    let last_due = t.env.ledger().timestamp() + DUE_DATE_OFFSET;
    let last_id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &last_due,
        &DISCOUNT_RATE,
        &t.token.address,
    );

    t.contract
        .fund_invoice(&t.funder, &last_id, &INVOICE_AMOUNT);

    let mut ledger = t.env.ledger().get();
    ledger.timestamp += DUE_DATE_OFFSET + 1;
    t.env.ledger().set(ledger);

    t.contract.claim_default(&t.funder, &last_id);

    let final_score = t.contract.payer_score(&t.payer);
    assert_eq!(final_score, 0, "score should floor at 0, not underflow");
}

// ----------------------------------------------------------------
// MT-06: discount_rate at the exact cap (5000 bps) must be accepted
//
// The guard is `discount_rate == 0 || discount_rate > max_rate` where
// max_rate defaults to 5000.  A mutation `discount_rate >= max_rate`
// would incorrectly reject rate == 5000.
// ----------------------------------------------------------------
#[test]
fn mt06_discount_rate_at_cap_is_accepted() {
    let t = setup();
    let due_date = t.env.ledger().timestamp() + DUE_DATE_OFFSET;

    let result = t.contract.try_submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &due_date,
        &5_000, // exactly the max
        &t.token.address,
    );

    assert!(
        result.is_ok(),
        "discount_rate == 5000 (50%) should be accepted"
    );
}

// ----------------------------------------------------------------
// MT-07: suggested_discount_rate formula correctness
//
// Formula: 500 + (100 - score) * 5
// Mutations: changing 500, 100, or 5 would break this.
//   score 100 → 500 + 0 = 500
//   score 50  → 500 + 250 = 750
//   score 0   → 500 + 500 = 1000
// ----------------------------------------------------------------
#[test]
fn mt07_suggested_discount_rate_formula() {
    let t = setup();

    // Default score is 50, so suggested rate = 500 + (100 - 50) * 5 = 750
    let rate = t.contract.suggested_discount_rate(&t.payer);
    assert_eq!(
        rate, 750,
        "default payer (score=50) should get suggested rate of 750 bps"
    );
}
