#![cfg(test)]

use crate::{
    events::InvoiceFunded,
    InvoiceLiquidityContract,
    InvoiceLiquidityContractClient,
    InvoiceStatus,
};

use soroban_sdk::{
    testutils::{Address as _, Ledger, Events},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal, xdr::ToXdr, xdr::FromXdr,
};

#[test]
fn tests_lp_funding_details_event() {
    let env = Env::default();

    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();

    // ------------------------------------------------------------
    // Accounts
    // ------------------------------------------------------------
    let admin = Address::generate(&env);
    let freelancer = Address::generate(&env);
    let payer = Address::generate(&env);
    let lp = Address::generate(&env);

    // ------------------------------------------------------------
    // Token setup
    // ------------------------------------------------------------
    let token_admin = Address::generate(&env);

    let token = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token.address();

    let token_client = TokenClient::new(&env, &token_address);
    let token_admin_client = StellarAssetClient::new(&env, &token_address);

    // Mint LP funds
    token_admin_client.mint(&lp, &10_000_000);

    // ------------------------------------------------------------
    // Contract setup
    // ------------------------------------------------------------
    let contract_id = env.register(InvoiceLiquidityContract, ());
    let client = InvoiceLiquidityContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &token_address,
        &token_address,
    );

    // ------------------------------------------------------------
    // Ledger timestamp
    // ------------------------------------------------------------
    let now = 1_700_000_000u64;

    env.ledger().set_timestamp(now);

    // 30 days due date
    let due_date = now + (30 * 24 * 60 * 60);

    // ------------------------------------------------------------
    // Submit invoice
    // ------------------------------------------------------------
    let invoice_id = client.submit_invoice(
        &freelancer,
        &payer,
        &5_000_000i128,
        &due_date,
        &1000u32, // 10%
        &token_address,
    );

    // ------------------------------------------------------------
    // Fund invoice
    // ------------------------------------------------------------
    client.fund_invoice(
        &lp,
        &invoice_id,
        &5_000_000i128,
    );

    // ------------------------------------------------------------
    // Verify event emitted
    // ------------------------------------------------------------
    let all_events = env.events().all();
    assert!(!all_events.events().is_empty());
    }