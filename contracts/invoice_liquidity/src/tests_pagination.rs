#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    Address, Vec,
};
use crate::test::setup;

#[test]
fn test_list_invoices_by_submitter_pagination() {
    let t = setup();
    let env = &t.env;

    let freelancer = Address::generate(env);
    let payer = Address::generate(env);
    let due_date = env.ledger().timestamp() + 86400 * 30; // 30 days

    // Submit 5 invoices
    for i in 0..5 {
        t.contract.submit_invoice(
            &freelancer,
            &payer,
            &(1_000_000_000 + i as i128),
            &due_date,
            &300,
            &t.token.address,
        );
    }

    // Page 0, size 2 -> invoices 0, 1
    let page0 = t.contract.list_invoices_by_submitter(&freelancer, &0, &2);
    assert_eq!(page0.len(), 2);
    assert_eq!(page0.get(0).unwrap().id, 1);
    assert_eq!(page0.get(1).unwrap().id, 2);

    // Page 1, size 2 -> invoices 2, 3
    let page1 = t.contract.list_invoices_by_submitter(&freelancer, &1, &2);
    assert_eq!(page1.len(), 2);
    assert_eq!(page1.get(0).unwrap().id, 3);
    assert_eq!(page1.get(1).unwrap().id, 4);

    // Page 2, size 2 -> invoice 4
    let page2 = t.contract.list_invoices_by_submitter(&freelancer, &2, &2);
    assert_eq!(page2.len(), 1);
    assert_eq!(page2.get(0).unwrap().id, 5);

    // Page 3, size 2 -> empty
    let page3 = t.contract.list_invoices_by_submitter(&freelancer, &3, &2);
    assert_eq!(page3.len(), 0);
}

#[test]
fn test_list_invoices_by_submitter_empty() {
    let t = setup();
    let env = &t.env;
    let unknown_address = Address::generate(env);

    let result = t.contract.list_invoices_by_submitter(&unknown_address, &0, &10);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_list_invoices_by_submitter_max_page_size() {
    let t = setup();
    let env = &t.env;
    let freelancer = Address::generate(env);
    let payer = Address::generate(env);
    let due_date = env.ledger().timestamp() + 86400 * 30;

    // Submit 60 invoices
    for _ in 0..60 {
        t.contract.submit_invoice(
            &freelancer,
            &payer,
            &1_000_000_000,
            &due_date,
            &300,
            &t.token.address,
        );
    }

    // Request page_size 100, should be capped at 50
    let result = t.contract.list_invoices_by_submitter(&freelancer, &0, &100);
    assert_eq!(result.len(), 50);
}

#[test]
fn test_list_invoices_by_submitter_batch() {
    let t = setup();
    let env = &t.env;
    let freelancer = Address::generate(env);
    let payer = Address::generate(env);
    let due_date = env.ledger().timestamp() + 86400 * 30;

    let mut batch = Vec::new(env);
    for _ in 0..3 {
        batch.push_back(InvoiceParams {
            freelancer: freelancer.clone(),
            payer: payer.clone(),
            amount: 1_000_000_000,
            due_date,
            discount_rate: 300,
            token: t.token.address.clone(),
        });
    }

    t.contract.submit_invoices_batch(&batch);

    let result = t.contract.list_invoices_by_submitter(&freelancer, &0, &10);
    assert_eq!(result.len(), 3);
}

#[test]
fn test_list_invoices_by_submitter_after_transfer() {
    let t = setup();
    let env = &t.env;
    let freelancer1 = Address::generate(env);
    let freelancer2 = Address::generate(env);
    let payer = Address::generate(env);
    let due_date = env.ledger().timestamp() + 86400 * 30;

    let id = t.contract.submit_invoice(
        &freelancer1,
        &payer,
        &1_000_000_000,
        &due_date,
        &300,
        &t.token.address,
    );

    // freelancer1 should have 1 invoice
    assert_eq!(t.contract.list_invoices_by_submitter(&freelancer1, &0, &10).len(), 1);
    // freelancer2 should have 0
    assert_eq!(t.contract.list_invoices_by_submitter(&freelancer2, &0, &10).len(), 0);

    // Transfer to freelancer2
    t.contract.transfer_invoice(&id, &freelancer2);

    // freelancer1 should have 0
    assert_eq!(t.contract.list_invoices_by_submitter(&freelancer1, &0, &10).len(), 0);
    // freelancer2 should have 1
    assert_eq!(t.contract.list_invoices_by_submitter(&freelancer2, &0, &10).len(), 1);
    assert_eq!(t.contract.list_invoices_by_submitter(&freelancer2, &0, &10).get(0).unwrap().id, id);
}
