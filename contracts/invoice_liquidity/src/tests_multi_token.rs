#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

const DUE_DATE_OFFSET: u64 = 60 * 60 * 24 * 30;
const DISCOUNT_RATE: u32 = 300;

struct MockToken {
    address: Address,
    client: TokenClient<'static>,
    admin_client: StellarAssetClient<'static>,
}

struct MultiTokenTestEnv {
    env: Env,
    contract: InvoiceLiquidityContractClient<'static>,
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

fn submit_invoice(env: &MultiTokenTestEnv, token: &MockToken, amount: i128) -> u64 {
    env.contract.submit_invoice(
        &env.freelancer,
        &env.payer,
        &amount,
        &due_date(env),
        &DISCOUNT_RATE,
        &token.address,
    )
}

fn expected_discount(amount: i128) -> i128 {
    amount * DISCOUNT_RATE as i128 / 10_000
}

fn assert_full_lifecycle_for_token(
    token_name: &str,
    token: &MockToken,
    env: &MultiTokenTestEnv,
    amount: i128,
) {
    let invoice_id = submit_invoice(env, token, amount);
    let invoice = env.contract.get_invoice(&invoice_id);
    assert_eq!(
        invoice.token, token.address,
        "{token_name} invoice should persist its token"
    );

    let freelancer_before = token.client.balance(&env.freelancer);
    let lp_before = token.client.balance(&env.lp);
    let payer_before = token.client.balance(&env.payer);

    env.contract.fund_invoice(&env.lp, &invoice_id, &amount);

    let discount = expected_discount(amount);
    assert_eq!(
        token.client.balance(&env.freelancer) - freelancer_before,
        amount - discount,
        "{token_name} should pay the freelancer in the same token path",
    );

    env.contract.mark_paid(&invoice_id, &INVOICE_AMOUNT);

    assert_eq!(
        token.client.balance(&env.lp) - lp_before,
        discount,
        "{token_name} LP should earn yield in the same token path",
    );
    assert_eq!(
        payer_before - token.client.balance(&env.payer),
        amount,
        "{token_name} payer should settle the invoice amount in the same token path",
    );
    assert_eq!(
        env.contract.get_invoice(&invoice_id).status,
        InvoiceStatus::Paid,
        "{token_name} invoice should finish the lifecycle as Paid",
    );
}

#[test]
fn test_full_lifecycle_usdc_token_path() {
    let env = setup();
    assert_full_lifecycle_for_token("USDC", &env.usdc, &env, 1_000_000_000);
}

#[test]
fn test_full_lifecycle_eurc_token_path() {
    let env = setup();
    assert_full_lifecycle_for_token("EURC", &env.eurc, &env, 25_000_000);
}

#[test]
fn test_full_lifecycle_xlm_sac_token_path() {
    let env = setup();
    assert_full_lifecycle_for_token("XLM SAC", &env.xlm, &env, 70_000_000);
}

#[test]
fn test_submit_with_unapproved_token_is_rejected() {
    let env = setup();
    let rogue = register_mock_token(&env.env);

    let result = env.contract.try_submit_invoice(
        &env.freelancer,
        &env.payer,
        &1_000_000,
        &due_date(&env),
        &DISCOUNT_RATE,
        &rogue.address,
    );

    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn test_admin_removing_token_mid_flight_does_not_break_existing_invoice_settlement() {
    let env = setup();
    let amount = 42_500_000;
    let invoice_id = submit_invoice(&env, &env.eurc, amount);

    env.contract.remove_token(&env.eurc.address);

    env.contract.fund_invoice(&env.lp, &invoice_id, &amount);
    env.contract.mark_paid(&invoice_id, &INVOICE_AMOUNT);

    let invoice = env.contract.get_invoice(&invoice_id);
    assert_eq!(invoice.status, InvoiceStatus::Paid);
    assert_eq!(invoice.token, env.eurc.address);
}

#[test]
fn test_same_lp_can_settle_invoices_independently_across_different_tokens() {
    let env = setup();
    let usdc_amount = 15_000_000;
    let eurc_amount = 9_500_000;

    let usdc_invoice = submit_invoice(&env, &env.usdc, usdc_amount);
    let eurc_invoice = submit_invoice(&env, &env.eurc, eurc_amount);

    let usdc_lp_before = env.usdc.client.balance(&env.lp);
    let eurc_lp_before = env.eurc.client.balance(&env.lp);

    env.contract
        .fund_invoice(&env.lp, &usdc_invoice, &usdc_amount);
    env.contract
        .fund_invoice(&env.lp, &eurc_invoice, &eurc_amount);

    env.contract.mark_paid(&usdc_invoice, &INVOICE_AMOUNT);

    assert_eq!(
        env.contract.get_invoice(&usdc_invoice).status,
        InvoiceStatus::Paid
    );
    assert_eq!(
        env.contract.get_invoice(&eurc_invoice).status,
        InvoiceStatus::Funded
    );
    assert_eq!(
        env.usdc.client.balance(&env.lp) - usdc_lp_before,
        expected_discount(usdc_amount),
    );
    assert_eq!(
        env.eurc.client.balance(&env.lp),
        eurc_lp_before - eurc_amount
    );

    env.contract.mark_paid(&eurc_invoice, &INVOICE_AMOUNT);

    assert_eq!(
        env.contract.get_invoice(&eurc_invoice).status,
        InvoiceStatus::Paid
    );
    assert_eq!(
        env.eurc.client.balance(&env.lp) - eurc_lp_before,
        expected_discount(eurc_amount),
    );
}

#[test]
fn test_amounts_preserve_precision_for_6_and_7_decimal_token_paths() {
    let env = setup();
    let eurc_amount = 12_345_678;
    let xlm_amount = 123_456_789;

    let eurc_invoice = submit_invoice(&env, &env.eurc, eurc_amount);
    let xlm_invoice = submit_invoice(&env, &env.xlm, xlm_amount);

    let eurc_freelancer_before = env.eurc.client.balance(&env.freelancer);
    let xlm_freelancer_before = env.xlm.client.balance(&env.freelancer);
    let eurc_lp_before = env.eurc.client.balance(&env.lp);
    let xlm_lp_before = env.xlm.client.balance(&env.lp);

    env.contract
        .fund_invoice(&env.lp, &eurc_invoice, &eurc_amount);
    env.contract
        .fund_invoice(&env.lp, &xlm_invoice, &xlm_amount);

    assert_eq!(
        env.eurc.client.balance(&env.freelancer) - eurc_freelancer_before,
        eurc_amount - expected_discount(eurc_amount),
    );
    assert_eq!(
        env.xlm.client.balance(&env.freelancer) - xlm_freelancer_before,
        xlm_amount - expected_discount(xlm_amount),
    );

    env.contract.mark_paid(&eurc_invoice, &INVOICE_AMOUNT);
    env.contract.mark_paid(&xlm_invoice, &INVOICE_AMOUNT);

    assert_eq!(
        env.eurc.client.balance(&env.lp) - eurc_lp_before,
        expected_discount(eurc_amount),
    );
    assert_eq!(
        env.xlm.client.balance(&env.lp) - xlm_lp_before,
        expected_discount(xlm_amount),
    );
}
