#![cfg(test)]

use crate::constants::MAX_DISCOUNT_RATE;
use crate::errors::ContractError;
use crate::test::setup;
use soroban_sdk::{testutils::Ledger, Env};

const INVOICE_AMOUNT: i128 = 1_000_000_000;
const DUE_DATE_OFFSET: u64 = 60 * 60 * 24 * 30;

#[test]
fn test_zero_discount_rejected() {
    let t = setup();
    let due_date = t.env.ledger().timestamp() + DUE_DATE_OFFSET;

    let result = t.contract.try_submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &due_date,
        &0,
        &t.token.address,
    );

    assert_eq!(result, Err(Ok(ContractError::InvalidDiscountRate)));
}

#[test]
fn test_max_discount_accepted() {
    let t = setup();
    let due_date = t.env.ledger().timestamp() + DUE_DATE_OFFSET;

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &due_date,
        &MAX_DISCOUNT_RATE,
        &t.token.address,
    );

    let invoice = t.contract.get_invoice(&id);
    assert_eq!(invoice.discount_rate, MAX_DISCOUNT_RATE);
    assert_eq!(invoice.id, id);
}

#[test]
fn test_above_max_rejected() {
    let t = setup();
    let due_date = t.env.ledger().timestamp() + DUE_DATE_OFFSET;

    let result = t.contract.try_submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &due_date,
        &(MAX_DISCOUNT_RATE + 1),
        &t.token.address,
    );

    assert_eq!(result, Err(Ok(ContractError::InvalidDiscountRate)));
}

#[test]
fn test_discount_rate_one_accepted() {
    let t = setup();
    let due_date = t.env.ledger().timestamp() + DUE_DATE_OFFSET;

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &due_date,
        &1,
        &t.token.address,
    );

    let invoice = t.contract.get_invoice(&id);
    assert_eq!(invoice.discount_rate, 1);
}

#[test]
fn test_large_invoice_with_max_discount() {
    let t = setup();
    let due_date = t.env.ledger().timestamp() + DUE_DATE_OFFSET;
    let large_amount: i128 = 100_000_000_000_000;

    let id = t.contract.submit_invoice(
        &t.freelancer,
        &t.payer,
        &large_amount,
        &due_date,
        &MAX_DISCOUNT_RATE,
        &t.token.address,
    );

    let invoice = t.contract.get_invoice(&id);
    assert_eq!(invoice.amount, large_amount);
    assert_eq!(invoice.discount_rate, MAX_DISCOUNT_RATE);
}

#[test]
fn test_discount_validation_happens_before_storage_write() {
    let t = setup();
    let due_date = t.env.ledger().timestamp() + DUE_DATE_OFFSET;

    let initial_invoice_count = t.contract.get_invoice_count();

    let result = t.contract.try_submit_invoice(
        &t.freelancer,
        &t.payer,
        &INVOICE_AMOUNT,
        &due_date,
        &(MAX_DISCOUNT_RATE + 1),
        &t.token.address,
    );

    assert_eq!(result, Err(Ok(ContractError::InvalidDiscountRate)));

    let final_invoice_count = t.contract.get_invoice_count();
    assert_eq!(
        initial_invoice_count, final_invoice_count,
        "No storage mutation should occur"
    );
}
