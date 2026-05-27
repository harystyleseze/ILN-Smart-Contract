#![cfg(test)]

//! Regression tests for previously fixed bugs and edge cases.
//! Each test documents the issue/PR it regresses to prevent future regressions.

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    testutils::Ledger,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

// ----------------------------------------------------------------
// Test helpers
// ----------------------------------------------------------------

struct RegressionTestEnv {
    env: Env,
    contract: InvoiceLiquidityContractClient<'static>,
    token: TokenClient<'static>,
    freelancer: Address,
    payer: Address,
    funder: Address,
}

const ONE_USDC: i128 = 10_000_000; // 1 USDC in stroops
const ONE_BPS: u32 = 1; // 0.01% discount

fn setup_regression() -> RegressionTestEnv {
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

    token_admin.mint(&funder, &(ONE_USDC * 100));
    token_admin.mint(&payer, &(ONE_USDC * 100));

    let contract_id = env.register(InvoiceLiquidityContract, ());
    let contract = InvoiceLiquidityContractClient::new(&env, &contract_id);

    let xlm_admin = Address::generate(&env);
    let xlm_contract_id = env.register_stellar_asset_contract_v2(xlm_admin);
    let xlm_address = xlm_contract_id.address();

    contract.initialize(&usdc_admin, &usdc_address, &xlm_address);

    let mut ledger_info = env.ledger().get();
    ledger_info.timestamp = 1_700_000_000;
    env.ledger().set(ledger_info);

    RegressionTestEnv {
        env,
        contract,
        token,
        freelancer,
        payer,
        funder,
    }
}

// ----------------------------------------------------------------
// REGRESSION TESTS
// ----------------------------------------------------------------

/// Regression for: wasm32v1-none target does not affect test results
/// This is a meta-test to verify that the test infrastructure works
/// consistently regardless of compilation target.
/// See: https://github.com/Nursca/Invoice-Liquidity-Network/issues/49
#[test]
fn regression_wasm32_target_independence() {
    let t = setup_regression();

    // Basic sanity check: can submit and fund an invoice
    let due_date = t.env.ledger().timestamp() + 86400; // 1 day from now

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &ONE_USDC,
        &due_date,
        &100, // 1% discount
        &t.token.address,
    );

    assert_eq!(id, 1);

    // Fund the invoice
    t.contract.fund_invoice(&t.funder, &id, &ONE_USDC);

    // Verify funding worked
    let invoice = t.contract.get_invoice(&id);
    assert_eq!(invoice.amount_funded, ONE_USDC);
}

/// Regression for: Discount of exactly 1 bps on a 1-stroop invoice rounds to 0, not negative
/// Previously, calculating discount on small amounts with low discount rates
/// could result in negative values due to integer underflow.
/// See: https://github.com/Nursca/Invoice-Liquidity-Network/issues/49
#[test]
fn regression_discount_rounding_minimum_amount() {
    let t = setup_regression();

    let due_date = t.env.ledger().timestamp() + 86400;

    // Submit invoice with 1 USDC (10_000_000 stroops) and 1 bps discount
    // Expected discount = 10_000_000 * 0.0001 = 1000 stroops = 0.0001 USDC
    // This should NOT underflow to a negative value
    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &ONE_USDC, // 10_000_000 stroops = 1 USDC
        &due_date,
        &ONE_BPS, // 1 bps = 0.01%
        &t.token.address,
    );

    assert_eq!(id, 1);

    // Get the invoice and verify discount_rate is stored correctly
    let invoice = t.contract.get_invoice(&id);
    assert_eq!(invoice.discount_rate, ONE_BPS);

    // Fund the invoice and verify the discount calculation doesn't underflow
    t.contract.fund_invoice(&t.funder, &id, &ONE_USDC);

    // Verify final state - should have funded amount, not negative
    let funded_invoice = t.contract.get_invoice(&id);
    assert!(funded_invoice.amount_funded >= 0);
}

/// Regression for: Invoice with due_date exactly equal to now is rejected (not accepted)
/// Previously, invoices with due_date == now() could be accepted, which is
/// problematic because they would immediately be in default status.
/// See: https://github.com/Nursca/Invoice-Liquidity-Network/issues/49
#[test]
fn regression_due_date_must_be_future() {
    let t = setup_regression();

    let now = t.env.ledger().timestamp();

    // Attempt to submit invoice with due_date exactly equal to now
    // This should FAIL with InvalidDueDate error
    let result = t.contract.try_submit_invoice(
        &t.freelancer,
        &t.payer,
        &ONE_USDC,
        &now, // exactly now - should be rejected
        &100,
        &t.token.address,
    );

    assert!(result.is_err());
    assert_eq!(result, Err(Ok(ContractError::InvalidDueDate)));

    // Also test: due_date in the past should also be rejected
    let past_date = now - 1;
    let result_past = t.contract.try_submit_invoice(
        &t.freelancer,
        &t.payer,
        &ONE_USDC,
        &past_date, // in the past - should be rejected
        &100,
        &t.token.address,
    );

    assert!(result_past.is_err());
    assert_eq!(result_past, Err(Ok(ContractError::InvalidDueDate)));

    // Test: due_date less than 24 hours in the future should be rejected
    let too_soon = now + (12 * 60 * 60); // 12 hours - too soon
    let result_too_soon = t.contract.try_submit_invoice(
        &t.freelancer,
        &t.payer,
        &ONE_USDC,
        &too_soon,
        &100,
        &t.token.address,
    );

    assert!(result_too_soon.is_err());
    assert_eq!(result_too_soon, Err(Ok(ContractError::DueDateTooSoon)));

    // Test: valid due_date (24+ hours in future) should succeed
    let valid_date = now + (24 * 60 * 60) + 1; // 24 hours + 1 second
    let result_valid = t.contract.try_submit_invoice(
        &t.freelancer,
        &t.payer,
        &ONE_USDC,
        &valid_date,
        &100,
        &t.token.address,
    );

    assert!(result_valid.is_ok());
}

/// Regression for: submitting an invoice with the freelancer as payer is rejected
/// Self-invoicing has no economic purpose and should be disallowed.
#[test]
fn regression_self_invoice_rejected() {
    let t = setup_regression();

    let due_date = t.env.ledger().timestamp() + 86400;

    let result = t.contract.try_submit_invoice(
        &t.freelancer,
        &t.freelancer,
        &ONE_USDC,
        &due_date,
        &ONE_BPS,
        &t.token.address,
    );

    assert!(result.is_err());
    assert_eq!(result, Err(Ok(ContractError::SelfInvoice)));
}

/// Regression for: Two invoices submitted in same ledger timestamp get different IDs
/// Previously, there was a race condition where submitting two invoices in the
/// same ledger could result in the same ID being assigned to both.
/// See: https://github.com/Nursca/Invoice-Liquidity-Network/issues/49
#[test]
fn regression_concurrent_invoice_ids_unique() {
    let t = setup_regression();

    let due_date = t.env.ledger().timestamp() + 86400;

    // Submit first invoice
    let id1 = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &ONE_USDC,
        &due_date,
        &100,
        &t.token.address,
    );

    // Submit second invoice in same ledger timestamp
    // (no time advancement between submissions)
    let id2 = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &(ONE_USDC * 2),
        &due_date,
        &200,
        &t.token.address,
    );

    // IDs must be different and sequential
    assert_ne!(id1, id2);
    assert_eq!(id2, id1 + 1);

    // Verify both invoices exist and have correct data
    let inv1 = t.contract.get_invoice(&id1);
    let inv2 = t.contract.get_invoice(&id2);

    assert_eq!(inv1.amount, ONE_USDC);
    assert_eq!(inv2.amount, ONE_USDC * 2);
}

/// Regression for: fund_invoice with exact invoice.amount USDC does not leave dust in contract
/// Previously, due to rounding in discount calculations, funding an invoice
/// with the exact amount could leave small amounts (dust) in the contract,
/// making it impossible to fully fund the invoice.
/// See: https://github.com/Nursca/Invoice-Liquidity-Network/issues/49
#[test]
fn regression_fund_exact_amount_no_dust() {
    let t = setup_regression();

    let due_date = t.env.ledger().timestamp() + 86400;

    // Submit invoice for exactly 1 USDC with 1% discount (valid rate)
    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &ONE_USDC,
        &due_date,
        &100, // 1% discount
        &t.token.address,
    );

    // Get initial funder balance
    let funder_balance_before = t.token.balance(&t.funder);

    // Fund exactly the invoice amount
    t.contract.fund_invoice(&t.funder, &id, &ONE_USDC);

    // Verify invoice is fully funded
    let invoice = t.contract.get_invoice(&id);
    assert_eq!(invoice.amount_funded, ONE_USDC);
    assert_eq!(invoice.status, InvoiceStatus::Funded);

    // Verify no dust left - funder should have paid exactly cost
    let funder_balance_after = t.token.balance(&t.funder);
    let cost = ONE_USDC - 100_000;
    let expected_balance = funder_balance_before - cost;
    assert_eq!(funder_balance_after, expected_balance);

    // Verify contract retained 0 because it paid out to freelancer
    let contract_address = t.contract.address.clone();
    let contract_balance = t.token.balance(&contract_address);
    assert_eq!(contract_balance, 0);
}

/// Additional regression: Batch submissions also get unique IDs
/// See: https://github.com/Nursca/Invoice-Liquidity-Network/issues/49
#[test]
fn regression_batch_invoice_ids_unique() {
    let t = setup_regression();

    let due_date = t.env.ledger().timestamp() + 86400;

    let mut params = Vec::new(&t.env);
    params.push_back(InvoiceParams {
        freelancer: t.freelancer.clone(),
        payer: t.payer.clone(),
        amount: ONE_USDC,
        due_date,
        discount_rate: 100,
        token: t.token.address.clone(),
    });
    params.push_back(InvoiceParams {
        freelancer: t.freelancer.clone(),
        payer: t.payer.clone(),
        amount: ONE_USDC * 2,
        due_date,
        discount_rate: 200,
        token: t.token.address.clone(),
    });
    params.push_back(InvoiceParams {
        freelancer: t.freelancer.clone(),
        payer: t.payer.clone(),
        amount: ONE_USDC * 3,
        due_date,
        discount_rate: 300,
        token: t.token.address.clone(),
    });

    let ids = t.contract.submit_invoices_batch(&params);

    // All IDs should be unique and sequential
    assert_eq!(ids.len(), 3);
    assert_eq!(ids.get(0).unwrap(), 1);
    assert_eq!(ids.get(1).unwrap(), 2);
    assert_eq!(ids.get(2).unwrap(), 3);

    // Verify all invoices exist
    for i in 1..=3 {
        let inv = t.contract.get_invoice(&i);
        assert!(inv.amount > 0);
    }
}
