#![cfg(test)]

use super::*;
use soroban_sdk::xdr::{ScErrorCode, ScErrorType};
use soroban_sdk::{
    testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke},
    token::StellarAssetClient,
    vec, Address, Env, Error, IntoVal, InvokeError, Symbol, TryFromVal, Val,
};

const INVOICE_AMOUNT: i128 = 1_000_000_000;
const DISCOUNT_RATE: u32 = 300;
const DUE_DATE_OFFSET: u64 = 60 * 60 * 24 * 30;

type HostResult<T> =
    Result<Result<T, <T as TryFromVal<Env, Val>>::Error>, Result<Error, InvokeError>>;

struct AuthTestEnv {
    env: Env,
    contract_id: Address,
    token_address: Address,
    freelancer: Address,
    payer: Address,
    funder: Address,
}

fn auth_error() -> Error {
    Error::from_type_and_code(ScErrorType::Context, ScErrorCode::InvalidAction)
}

fn expect_success<T: core::fmt::Debug + TryFromVal<Env, Val>>(result: HostResult<T>) -> T {
    match result {
        Ok(Ok(value)) => value,
        other => panic!("expected successful invocation, got {:?}", other),
    }
}

fn setup() -> AuthTestEnv {
    let env = Env::default();

    let token_admin = Address::generate(&env);
    let usdc_contract = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = usdc_contract.address();
    let token_admin_client = StellarAssetClient::new(&env, &token_address);

    let freelancer = Address::generate(&env);
    let payer = Address::generate(&env);
    let funder = Address::generate(&env);

    mint_with_auth(
        &env,
        &token_address,
        &token_admin_client,
        &token_admin,
        &payer,
    );
    mint_with_auth(
        &env,
        &token_address,
        &token_admin_client,
        &token_admin,
        &funder,
    );

    let contract_id = env.register(InvoiceLiquidityContract, ());
    let client = InvoiceLiquidityContractClient::new(&env, &contract_id);

    mint_with_auth(
        &env,
        &token_address,
        &token_admin_client,
        &token_admin,
        &client.address,
    );

    let xlm_admin = Address::generate(&env);
    let xlm_contract = env.register_stellar_asset_contract_v2(xlm_admin);
    client.initialize(&token_admin, &token_address, &xlm_contract.address());

    let mut ledger = env.ledger().get();
    ledger.timestamp = 1_700_000_000;
    env.ledger().set(ledger);

    AuthTestEnv {
        env,
        contract_id,
        token_address,
        freelancer,
        payer,
        funder,
    }
}

fn mint_with_auth(
    env: &Env,
    token_address: &Address,
    token_admin_client: &StellarAssetClient<'_>,
    admin: &Address,
    to: &Address,
) {
    env.mock_auths(&[MockAuth {
        address: admin,
        invoke: &MockAuthInvoke {
            contract: token_address,
            fn_name: "mint",
            args: (to.clone(), INVOICE_AMOUNT * 10).into_val(env),
            sub_invokes: &[],
        },
    }]);

    token_admin_client.mint(to, &(INVOICE_AMOUNT * 10));
}

fn due_date(t: &AuthTestEnv) -> u64 {
    t.env.ledger().timestamp() + DUE_DATE_OFFSET
}

fn submit_invoice_args(t: &AuthTestEnv) -> soroban_sdk::Vec<Val> {
    vec![
        &t.env,
        t.freelancer.clone().into_val(&t.env),
        t.payer.clone().into_val(&t.env),
        INVOICE_AMOUNT.into_val(&t.env),
        due_date(t).into_val(&t.env),
        DISCOUNT_RATE.into_val(&t.env),
        t.token_address.clone().into_val(&t.env),
    ]
}

fn fund_invoice_args(t: &AuthTestEnv, invoice_id: u64) -> soroban_sdk::Vec<Val> {
    vec![
        &t.env,
        t.funder.clone().into_val(&t.env),
        invoice_id.into_val(&t.env),
        INVOICE_AMOUNT.into_val(&t.env),
    ]
}

fn update_invoice_args(t: &AuthTestEnv, invoice_id: u64) -> soroban_sdk::Vec<Val> {
    update_invoice_args_for_signer(t, invoice_id, &t.freelancer)
}

fn update_invoice_args_for_signer(
    t: &AuthTestEnv,
    invoice_id: u64,
    signer: &Address,
) -> soroban_sdk::Vec<Val> {
    vec![
        &t.env,
        signer.clone().into_val(&t.env),
        invoice_id.into_val(&t.env),
        INVOICE_AMOUNT.into_val(&t.env),
        (due_date(t) + 1).into_val(&t.env),
        DISCOUNT_RATE.into_val(&t.env),
    ]
}

fn claim_default_args(t: &AuthTestEnv, invoice_id: u64) -> soroban_sdk::Vec<Val> {
    vec![
        &t.env,
        t.funder.clone().into_val(&t.env),
        invoice_id.into_val(&t.env),
    ]
}

fn invoke_submit_invoice(t: &AuthTestEnv) -> HostResult<u64> {
    t.env.try_invoke_contract(
        &t.contract_id,
        &Symbol::new(&t.env, "submit_invoice"),
        submit_invoice_args(t),
    )
}

fn invoke_fund_invoice(t: &AuthTestEnv, invoice_id: u64) -> HostResult<()> {
    t.env.try_invoke_contract(
        &t.contract_id,
        &Symbol::new(&t.env, "fund_invoice"),
        fund_invoice_args(t, invoice_id),
    )
}

fn invoke_update_invoice(t: &AuthTestEnv, invoice_id: u64) -> HostResult<()> {
    t.env.try_invoke_contract(
        &t.contract_id,
        &Symbol::new(&t.env, "update_invoice"),
        update_invoice_args(t, invoice_id),
    )
}

fn invoke_mark_paid(t: &AuthTestEnv, invoice_id: u64) -> HostResult<()> {
    t.env.try_invoke_contract(
        &t.contract_id,
        &Symbol::new(&t.env, "mark_paid"),
        vec![&t.env, invoice_id.into_val(&t.env), INVOICE_AMOUNT.into_val(&t.env)],
    )
}

fn invoke_claim_default(t: &AuthTestEnv, invoice_id: u64) -> HostResult<()> {
    t.env.try_invoke_contract(
        &t.contract_id,
        &Symbol::new(&t.env, "claim_default"),
        claim_default_args(t, invoice_id),
    )
}

fn invoke_cancel_invoice(t: &AuthTestEnv, invoice_id: u64) -> HostResult<()> {
    t.env.try_invoke_contract(
        &t.contract_id,
        &Symbol::new(&t.env, "cancel_invoice"),
        vec![&t.env, invoice_id.into_val(&t.env)],
    )
}

fn set_submit_invoice_auth(t: &AuthTestEnv, signer: &Address) {
    t.env.mock_auths(&[MockAuth {
        address: signer,
        invoke: &MockAuthInvoke {
            contract: &t.contract_id,
            fn_name: "submit_invoice",
            args: (
                t.freelancer.clone(),
                t.payer.clone(),
                INVOICE_AMOUNT,
                due_date(t),
                DISCOUNT_RATE,
                t.token_address.clone(),
            )
                .into_val(&t.env),
            sub_invokes: &[],
        },
    }]);
}

fn set_fund_invoice_auth(
    t: &AuthTestEnv,
    signer: &Address,
    invoice_id: u64,
    include_transfer: bool,
) {
    let cost = INVOICE_AMOUNT - (INVOICE_AMOUNT * DISCOUNT_RATE as i128 / 10000);
    let transfer_sub_invokes = [MockAuthInvoke {
        contract: &t.token_address,
        fn_name: "transfer",
        args: (t.funder.clone(), t.contract_id.clone(), cost).into_val(&t.env),
        sub_invokes: &[],
    }];

    let sub_invokes = if include_transfer {
        &transfer_sub_invokes[..]
    } else {
        &[]
    };

    t.env.mock_auths(&[MockAuth {
        address: signer,
        invoke: &MockAuthInvoke {
            contract: &t.contract_id,
            fn_name: "fund_invoice",
            args: (t.funder.clone(), invoice_id, INVOICE_AMOUNT).into_val(&t.env),
            sub_invokes,
        },
    }]);
}

fn set_update_invoice_auth(t: &AuthTestEnv, signer: &Address, invoice_id: u64) {
    t.env.mock_auths(&[MockAuth {
        address: signer,
        invoke: &MockAuthInvoke {
            contract: &t.contract_id,
            fn_name: "update_invoice",
            args: (
                signer.clone(),
                invoice_id,
                INVOICE_AMOUNT,
                due_date(t) + 1,
                DISCOUNT_RATE,
            )
                .into_val(&t.env),
            sub_invokes: &[],
        },
    }]);
}

fn set_mark_paid_auth(t: &AuthTestEnv, signer: &Address, invoice_id: u64, include_transfer: bool) {
    let transfer_sub_invokes = [MockAuthInvoke {
        contract: &t.token_address,
        fn_name: "transfer",
        args: (t.payer.clone(), t.contract_id.clone(), INVOICE_AMOUNT).into_val(&t.env),
        sub_invokes: &[],
    }];

    let sub_invokes = if include_transfer {
        &transfer_sub_invokes[..]
    } else {
        &[]
    };

    t.env.mock_auths(&[MockAuth {
        address: signer,
        invoke: &MockAuthInvoke {
            contract: &t.contract_id,
            fn_name: "mark_paid",
            args: (invoice_id, INVOICE_AMOUNT).into_val(&t.env),
            sub_invokes,
        },
    }]);
}

fn set_claim_default_auth(t: &AuthTestEnv, signer: &Address, invoice_id: u64) {
    t.env.mock_auths(&[MockAuth {
        address: signer,
        invoke: &MockAuthInvoke {
            contract: &t.contract_id,
            fn_name: "claim_default",
            args: (t.funder.clone(), invoice_id).into_val(&t.env),
            sub_invokes: &[],
        },
    }]);
}

fn set_cancel_invoice_auth(t: &AuthTestEnv, signer: &Address, invoice_id: u64) {
    t.env.mock_auths(&[MockAuth {
        address: signer,
        invoke: &MockAuthInvoke {
            contract: &t.contract_id,
            fn_name: "cancel_invoice",
            args: (invoice_id,).into_val(&t.env),
            sub_invokes: &[],
        },
    }]);
}

fn submit_invoice_authorized(t: &AuthTestEnv) -> u64 {
    set_submit_invoice_auth(t, &t.freelancer);
    expect_success(invoke_submit_invoice(t))
}

fn fund_invoice_authorized(t: &AuthTestEnv, invoice_id: u64) {
    set_fund_invoice_auth(t, &t.funder, invoice_id, true);
    expect_success(invoke_fund_invoice(t, invoice_id));
}

#[test]
fn submit_invoice_rejects_wrong_signer() {
    let t = setup();
    let impostor = Address::generate(&t.env);

    set_submit_invoice_auth(&t, &impostor);
    let result = invoke_submit_invoice(&t);

    assert_eq!(result, Err(Ok(auth_error())));
}

#[test]
fn submit_invoice_accepts_freelancer_signature() {
    let t = setup();

    set_submit_invoice_auth(&t, &t.freelancer);
    let result = invoke_submit_invoice(&t);

    assert_eq!(expect_success(result), 1);
}

#[test]
fn fund_invoice_rejects_wrong_signer() {
    let t = setup();
    let invoice_id = submit_invoice_authorized(&t);
    let impostor = Address::generate(&t.env);

    set_fund_invoice_auth(&t, &impostor, invoice_id, false);
    let result = invoke_fund_invoice(&t, invoice_id);

    assert_eq!(result, Err(Ok(auth_error())));
}

#[test]
fn fund_invoice_accepts_funder_signature() {
    let t = setup();
    let invoice_id = submit_invoice_authorized(&t);

    set_fund_invoice_auth(&t, &t.funder, invoice_id, true);
    let result = invoke_fund_invoice(&t, invoice_id);

    assert_eq!(expect_success(result), ());
}

#[test]
fn update_invoice_rejects_wrong_signer() {
    let t = setup();
    let invoice_id = submit_invoice_authorized(&t);
    let impostor = Address::generate(&t.env);

    set_update_invoice_auth(&t, &impostor, invoice_id);
    let result = t
        .env
        .try_invoke_contract::<Result<(), ContractError>, Error>(
            &t.contract_id,
            &Symbol::new(&t.env, "update_invoice"),
            update_invoice_args_for_signer(&t, invoice_id, &impostor),
        );
    let expected_error: Error = ContractError::Unauthorized.into();

    assert_eq!(result, Err(Ok(expected_error)));
}

#[test]
fn update_invoice_accepts_freelancer_signature() {
    let t = setup();
    let invoice_id = submit_invoice_authorized(&t);

    set_update_invoice_auth(&t, &t.freelancer, invoice_id);
    let result = invoke_update_invoice(&t, invoice_id);

    assert_eq!(expect_success(result), ());
}

#[test]
fn mark_paid_rejects_wrong_signer() {
    let t = setup();
    let invoice_id = submit_invoice_authorized(&t);
    fund_invoice_authorized(&t, invoice_id);
    let impostor = Address::generate(&t.env);

    set_mark_paid_auth(&t, &impostor, invoice_id, false);
    let result = invoke_mark_paid(&t, invoice_id);

    assert_eq!(result, Err(Ok(auth_error())));
}

#[test]
fn mark_paid_accepts_payer_signature() {
    let t = setup();
    let invoice_id = submit_invoice_authorized(&t);
    fund_invoice_authorized(&t, invoice_id);

    set_mark_paid_auth(&t, &t.payer, invoice_id, true);
    let result = invoke_mark_paid(&t, invoice_id);

    assert_eq!(expect_success(result), ());
}

#[test]
fn claim_default_rejects_wrong_signer() {
    let t = setup();
    let invoice_id = submit_invoice_authorized(&t);
    fund_invoice_authorized(&t, invoice_id);
    let impostor = Address::generate(&t.env);

    let mut ledger = t.env.ledger().get();
    ledger.timestamp = due_date(&t) + 1;
    t.env.ledger().set(ledger);

    set_claim_default_auth(&t, &impostor, invoice_id);
    let result = invoke_claim_default(&t, invoice_id);

    assert_eq!(result, Err(Ok(auth_error())));
}

#[test]
fn claim_default_accepts_funder_signature() {
    let t = setup();
    let invoice_id = submit_invoice_authorized(&t);
    fund_invoice_authorized(&t, invoice_id);

    let mut ledger = t.env.ledger().get();
    ledger.timestamp = due_date(&t) + 1;
    t.env.ledger().set(ledger);

    set_claim_default_auth(&t, &t.funder, invoice_id);
    let result = invoke_claim_default(&t, invoice_id);

    assert_eq!(expect_success(result), ());
}

#[test]
fn cancel_invoice_rejects_wrong_signer() {
    let t = setup();
    let invoice_id = submit_invoice_authorized(&t);
    let impostor = Address::generate(&t.env);

    set_cancel_invoice_auth(&t, &impostor, invoice_id);
    let result = invoke_cancel_invoice(&t, invoice_id);

    assert_eq!(result, Err(Ok(auth_error())));
}

#[test]
fn cancel_invoice_accepts_freelancer_signature() {
    let t = setup();
    let invoice_id = submit_invoice_authorized(&t);

    set_cancel_invoice_auth(&t, &t.freelancer, invoice_id);
    let result = invoke_cancel_invoice(&t, invoice_id);

    assert_eq!(expect_success(result), ());
}
