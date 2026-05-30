#![cfg(test)]

use super::*;
use crate::invoice::InvoiceStatus;
use crate::storage::DataKey;
use soroban_sdk::{
    testutils::{storage::Persistent, Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

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

    token_admin.mint(&funder, &1_000_000_000_000);
    token_admin.mint(&payer, &1_000_000_000_000);

    let contract_id = env.register(InvoiceLiquidityContract, ());
    let contract = InvoiceLiquidityContractClient::new(&env, &contract_id);

    let xlm_admin = Address::generate(&env);
    let xlm_address = env.register_stellar_asset_contract_v2(xlm_admin).address();

    contract.initialize(&usdc_admin, &usdc_address, &xlm_address);

    TestEnv {
        env,
        contract,
        token,
        freelancer,
        payer,
        funder,
    }
}

#[test]
fn test_submit_invoice_sets_ttl() {
    let t = setup();
    let amount = 100_000_000;
    let due_date = t.env.ledger().timestamp() + 86400;

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &amount,
        &due_date,
        &300,
        &t.token.address,
    );

    let key = crate::storage::DataKey::Invoice(id);
    let ttl = t.env.as_contract(&t.contract.address, || {
        t.env.storage().persistent().get_ttl(&key)
    });

    // Check that TTL is set
    assert!(ttl > 0);
    // Verify aggregate counter TTL as well
    let count_key = crate::storage::DataKey::TotalInvoices;
    let count_ttl = t.env.as_contract(&t.contract.address, || {
        t.env.storage().persistent().get_ttl(&count_key)
    });
    assert!(count_ttl > 0);
}

#[test]
fn test_fund_invoice_extends_ttl() {
    let t = setup();
    let amount = 100_000_000;
    let due_date = t.env.ledger().timestamp() + 86400;

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &amount,
        &due_date,
        &300,
        &t.token.address,
    );

    let key = crate::storage::DataKey::Invoice(id);
    let initial_ttl = t.env.as_contract(&t.contract.address, || {
        t.env.storage().persistent().get_ttl(&key)
    });

    // Advance ledger state to simulate time passed (enough to cross threshold)
    let mut ledger = t.env.ledger().get();
    ledger.sequence_number += 1_500_000;
    ledger.timestamp += 43_200;
    t.env.ledger().set(ledger);

    // Call fund_invoice which should refresh the TTL on the entry
    t.contract.fund_invoice(&t.funder, &id, &amount);

    let updated_ttl = t.env.as_contract(&t.contract.address, || {
        t.env.storage().persistent().get_ttl(&key)
    });

    // TTL should be at least equal to initial, effectively extended relative to the new ledger height
    assert!(updated_ttl >= initial_ttl);
}

#[test]
fn test_data_persistence_after_advancement() {
    let t = setup();
    let amount = 100_000_000;
    let due_date = t.env.ledger().timestamp() + 86400 * 30; // 30 days

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &amount,
        &due_date,
        &300,
        &t.token.address,
    );

    // Advance ledger significantly (e.g., 10,000 ledgers)
    let mut ledger = t.env.ledger().get();
    ledger.sequence_number += 10_000;
    ledger.timestamp += 50_000;
    t.env.ledger().set(ledger);

    // Re-read and assert correctness
    let invoice = t.contract.get_invoice(&id);
    assert_eq!(invoice.amount, amount);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
}

#[test]
fn test_invoice_count_persistence_across_versions() {
    let t = setup();

    t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &100_000_000,
        &(t.env.ledger().timestamp() + 86400),
        &300,
        &t.token.address,
    );

    assert_eq!(t.contract.get_invoice_count(), 1);

    // Advance ledger 1 million sequences
    let mut ledger = t.env.ledger().get();
    ledger.sequence_number += 1_000_000;
    ledger.timestamp += 86400 * 60; // 60 days
    t.env.ledger().set(ledger);

    // Counter still correct
    assert_eq!(t.contract.get_invoice_count(), 1);

    // New submission works correctly
    let next_id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &200_000_000,
        &(t.env.ledger().timestamp() + 86400),
        &300,
        &t.token.address,
    );

    assert_eq!(next_id, 2);
    assert_eq!(t.contract.get_invoice_count(), 2);
}

#[test]
fn test_storage_ttl_near_boundary() {
    let t = setup();
    let amount = 100_000_000;
    let due_date = t.env.ledger().timestamp() + 86400;

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &amount,
        &due_date,
        &300,
        &t.token.address,
    );

    let key = crate::storage::DataKey::Invoice(id);
    let ttl = t.env.as_contract(&t.contract.address, || {
        t.env.storage().persistent().get_ttl(&key)
    });

    // Advance ledger to just before TTL expiry
    // If TTL is X ledgers, advance by X-1
    let mut ledger = t.env.ledger().get();
    ledger.sequence_number += ttl - 1;
    t.env.ledger().set(ledger);

    // Read should still work
    let invoice = t.contract.get_invoice(&id);
    assert_eq!(invoice.id, id);
}
