#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    token::StellarAssetClient,
    Address, Vec,
};
use crate::test::setup;

#[test]
fn test_list_invoices_by_lp_pagination() {
    let t = setup();
    let env = &t.env;
    let token_admin = StellarAssetClient::new(env, &t.token.address);

    let lp = Address::generate(env);
    token_admin.mint(&lp, &10_000_000_000);

    let freelancer = Address::generate(env);
    let payer = Address::generate(env);
    let due_date = env.ledger().timestamp() + 86400 * 30;

    // Submit 5 invoices and fund them all with the same LP
    for i in 0..5 {
        let id = t.contract.submit_invoice(
            &freelancer,
            &payer,
            &(1_000_000_000 + i as i128),
            &due_date,
            &300,
            &t.token.address,
        );
        t.contract.fund_invoice(&lp, &id, &(1_000_000_000 + i as i128));
    }

    // Page 0, size 2
    let page0 = t.contract.list_invoices_by_lp(&lp, &0, &2);
    assert_eq!(page0.len(), 2);
    assert_eq!(page0.get(0).unwrap().id, 1);
    assert_eq!(page0.get(1).unwrap().id, 2);

    // Page 2, size 2 -> invoice 4
    let page2 = t.contract.list_invoices_by_lp(&lp, &2, &2);
    assert_eq!(page2.len(), 1);
    assert_eq!(page2.get(0).unwrap().id, 5);
}

#[test]
fn test_list_invoices_by_lp_empty() {
    let t = setup();
    let env = &t.env;
    let unknown_lp = Address::generate(env);

    let result = t.contract.list_invoices_by_lp(&unknown_lp, &0, &10);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_list_invoices_by_lp_no_duplicates_on_partial_funding() {
    let t = setup();
    let env = &t.env;
    let token_admin = StellarAssetClient::new(env, &t.token.address);

    let lp = Address::generate(env);
    token_admin.mint(&lp, &2_000_000_000);
    
    let freelancer = Address::generate(env);
    let payer = Address::generate(env);
    let due_date = env.ledger().timestamp() + 86400 * 30;

    let id = t.contract.submit_invoice(
        &freelancer,
        &payer,
        &1_000_000_000,
        &due_date,
        &300,
        &t.token.address,
    );

    // Fund partially twice
    t.contract.fund_invoice(&lp, &id, &500_000_000);
    t.contract.fund_invoice(&lp, &id, &500_000_000);

    // Should only appear once in LP index
    let result = t.contract.list_invoices_by_lp(&lp, &0, &10);
    assert_eq!(result.len(), 1);
    assert_eq!(result.get(0).unwrap().id, id);
}

#[test]
fn test_list_invoices_by_lp_multiple_lps() {
    let t = setup();
    let env = &t.env;
    let token_admin = StellarAssetClient::new(env, &t.token.address);

    let lp1 = Address::generate(env);
    let lp2 = Address::generate(env);
    token_admin.mint(&lp1, &1_000_000_000);
    token_admin.mint(&lp2, &1_000_000_000);

    let freelancer = Address::generate(env);
    let payer = Address::generate(env);
    let due_date = env.ledger().timestamp() + 86400 * 30;

    let id = t.contract.submit_invoice(
        &freelancer,
        &payer,
        &1_000_000_000,
        &due_date,
        &300,
        &t.token.address,
    );

    // lp1 funds half, lp2 funds half
    t.contract.fund_invoice(&lp1, &id, &500_000_000);
    t.contract.fund_invoice(&lp2, &id, &500_000_000);

    // Both should see the invoice
    assert_eq!(t.contract.list_invoices_by_lp(&lp1, &0, &10).len(), 1);
    assert_eq!(t.contract.list_invoices_by_lp(&lp2, &0, &10).len(), 1);
}
