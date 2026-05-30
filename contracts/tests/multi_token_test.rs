#![cfg(test)]

use invoice_liquidity::{
    InvoiceLiquidityContract, InvoiceLiquidityContractClient, InvoiceStatus, ContractError,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

const DUE_DATE_OFFSET: u64 = 60 * 60 * 24 * 30; // 30 days
const DISCOUNT_RATE: u32 = 300; // 3.00%
const INVOICE_AMOUNT: i128 = 1_000_000_000;

struct MockToken {
    address: Address,
    client: TokenClient<'static>,
    admin_client: StellarAssetClient<'static>,
}

struct MultiTokenTestEnv {
    env: Env,
    contract: InvoiceLiquidityContractClient<'static>,
    admin: Address,
    freelancer: Address,
    payer: Address,
    lp: Address,
    usdc: MockToken,
    eurc: MockToken,
    xlm: MockToken,
}

fn register_mock_token(env: &Env) -> MockToken {
    let token_admin = Address::generate(env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = token_contract.address();

    MockToken {
        address: token_address.clone(),
        client: TokenClient::new(env, &token_address),
        admin_client: StellarAssetClient::new(env, &token_address),
    }
}

fn setup() -> MultiTokenTestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let freelancer = Address::generate(&env);
    let payer = Address::generate(&env);
    let lp = Address::generate(&env);

    let usdc = register_mock_token(&env);
    let eurc = register_mock_token(&env);
    let xlm = register_mock_token(&env);

    // Mint tokens
    usdc.admin_client.mint(&payer, &10_000_000_000);
    usdc.admin_client.mint(&lp, &10_000_000_000);
    eurc.admin_client.mint(&payer, &10_000_000_000);
    eurc.admin_client.mint(&lp, &10_000_000_000);
    xlm.admin_client.mint(&payer, &100_000_000_000);
    xlm.admin_client.mint(&lp, &100_000_000_000);

    let contract_id = env.register(InvoiceLiquidityContract, ());
    let contract = InvoiceLiquidityContractClient::new(&env, &contract_id);
    contract.initialize(&admin, &usdc.address, &xlm.address);
    contract.add_token(&eurc.address);

    let mut ledger_info = env.ledger().get();
    ledger_info.timestamp = 1_700_000_000;
    env.ledger().set(ledger_info);

    MultiTokenTestEnv {
        env,
        contract,
        admin,
        freelancer,
        payer,
        lp,
        usdc,
        eurc,
        xlm,
    }
}

fn due_date(env: &MultiTokenTestEnv) -> u64 {
    env.env.ledger().timestamp() + DUE_DATE_OFFSET
}

fn expected_discount(amount: i128) -> i128 {
    amount * DISCOUNT_RATE as i128 / 10_000
}

fn assert_lifecycle_for_token(
    token_name: &str,
    token: &MockToken,
    env: &MultiTokenTestEnv,
    amount: i128,
) {
    // 1. Submit
    let invoice_id = env.contract.submit_invoice(
        &env.freelancer,
        &env.payer,
        &amount,
        &due_date(env),
        &DISCOUNT_RATE,
        &token.address,
    );

    let invoice = env.contract.get_invoice(&invoice_id);
    assert_eq!(
        invoice.token, token.address,
        "{token_name} invoice should persist its token address"
    );
    assert_eq!(invoice.status, InvoiceStatus::Pending);

    let freelancer_before = token.client.balance(&env.freelancer);
    let lp_before = token.client.balance(&env.lp);
    let payer_before = token.client.balance(&env.payer);

    // 2. Fund
    env.contract.fund_invoice(&env.lp, &invoice_id, &amount);

    let discount = expected_discount(amount);
    let expected_payout = amount - discount;

    assert_eq!(
        token.client.balance(&env.freelancer) - freelancer_before,
        expected_payout,
        "{token_name} freelancer should receive amount minus discount"
    );

    assert_eq!(
        lp_before - token.client.balance(&env.lp),
        expected_payout,
        "{token_name} LP should pay the payout amount"
    );

    let invoice_funded = env.contract.get_invoice(&invoice_id);
    assert_eq!(invoice_funded.status, InvoiceStatus::Funded);

    // 3. Paid
    env.contract.mark_paid(&invoice_id, &amount);

    assert_eq!(
        token.client.balance(&env.lp) - lp_before,
        discount,
        "{token_name} LP should earn yield"
    );

    assert_eq!(
        payer_before - token.client.balance(&env.payer),
        amount,
        "{token_name} payer should pay full amount"
    );

    let invoice_paid = env.contract.get_invoice(&invoice_id);
    assert_eq!(invoice_paid.status, InvoiceStatus::Paid);
}

#[test]
fn test_integration_lifecycle_usdc() {
    let env = setup();
    assert_lifecycle_for_token("USDC", &env.usdc, &env, INVOICE_AMOUNT);
}

#[test]
fn test_integration_lifecycle_eurc() {
    let env = setup();
    assert_lifecycle_for_token("EURC", &env.eurc, &env, INVOICE_AMOUNT);
}

#[test]
fn test_integration_lifecycle_xlm() {
    let env = setup();
    assert_lifecycle_for_token("XLM", &env.xlm, &env, INVOICE_AMOUNT);
}

#[test]
fn test_integration_submit_unapproved_token_fails() {
    let env = setup();
    let unapproved_token = register_mock_token(&env.env);

    let result = env.contract.try_submit_invoice(
        &env.freelancer,
        &env.payer,
        &INVOICE_AMOUNT,
        &due_date(&env),
        &DISCOUNT_RATE,
        &unapproved_token.address,
    );

    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn test_integration_fund_removed_token_fails() {
    let env = setup();
    
    // Submit invoice with EURC (currently approved)
    let invoice_id = env.contract.submit_invoice(
        &env.freelancer,
        &env.payer,
        &INVOICE_AMOUNT,
        &due_date(&env),
        &DISCOUNT_RATE,
        &env.eurc.address,
    );

    // Admin removes EURC from approved list
    env.contract.remove_token(&env.eurc.address);

    // LP tries to fund it - should fail with Unauthorized
    let result = env.contract.try_fund_invoice(&env.lp, &invoice_id, &INVOICE_AMOUNT);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}
