#![cfg(test)]

use super::*;
use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

#[contracttype]
enum DistDataKey {
    Lp(Address),
    Freelancer(Address),
    PayerOnTime(Address),
}

#[contract]
struct MockDistribution;

#[contractimpl]
impl MockDistribution {
    pub fn accrue_lp(env: Env, lp: Address, amount_usdc_equivalent: i128) {
        let key = DistDataKey::Lp(lp);
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&key, &(current + amount_usdc_equivalent));
    }

    pub fn accrue_settlement(env: Env, freelancer: Address, payer: Address, on_time: bool) {
        let freelancer_key = DistDataKey::Freelancer(freelancer);
        let freelancer_count: u64 = env.storage().persistent().get(&freelancer_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&freelancer_key, &(freelancer_count + 1));

        if on_time {
            let payer_key = DistDataKey::PayerOnTime(payer);
            let payer_count: u64 = env.storage().persistent().get(&payer_key).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&payer_key, &(payer_count + 1));
        }
    }

    pub fn lp_volume(env: Env, lp: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DistDataKey::Lp(lp))
            .unwrap_or(0)
    }

    pub fn freelancer_settled(env: Env, freelancer: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DistDataKey::Freelancer(freelancer))
            .unwrap_or(0)
    }

    pub fn payer_on_time(env: Env, payer: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DistDataKey::PayerOnTime(payer))
            .unwrap_or(0)
    }
}

#[test]
fn distribution_hooks_track_lp_freelancer_and_payer() {
    let env = Env::default();
    env.mock_all_auths();

    let usdc_admin = Address::generate(&env);
    let usdc_id = env.register_stellar_asset_contract_v2(usdc_admin.clone());
    let usdc = TokenClient::new(&env, &usdc_id.address());
    let usdc_admin_client = StellarAssetClient::new(&env, &usdc_id.address());

    let freelancer = Address::generate(&env);
    let payer = Address::generate(&env);
    let funder = Address::generate(&env);

    let invoice_amount: i128 = 1_000_000_000;
    usdc_admin_client.mint(&funder, &(invoice_amount * 10));
    usdc_admin_client.mint(&payer, &(invoice_amount * 10));

    let invoice_id = env.register(InvoiceLiquidityContract, ());
    let invoice = InvoiceLiquidityContractClient::new(&env, &invoice_id);

    let xlm_admin = Address::generate(&env);
    let xlm_id = env.register_stellar_asset_contract_v2(xlm_admin);

    invoice.initialize(&usdc_admin, &usdc_id.address(), &xlm_id.address());

    let dist_id = env.register(MockDistribution, ());
    let dist = MockDistributionClient::new(&env, &dist_id);
    invoice.set_distribution_contract(&dist_id);

    let mut ledger = env.ledger().get();
    ledger.timestamp = 1_700_000_000;
    env.ledger().set(ledger.clone());

    let due_date = ledger.timestamp + (24 * 60 * 60) + 3600; // 25 hours in the future
    let submitted = invoice.submit_invoice(
        &freelancer,
        &payer,
        &invoice_amount,
        &due_date,
        &300,
        &usdc.address,
    );

    invoice.fund_invoice(&funder, &submitted, &invoice_amount);
    assert_eq!(dist.lp_volume(&funder), invoice_amount);

    invoice.mark_paid(&submitted, &invoice_amount);

    assert_eq!(dist.freelancer_settled(&freelancer), 1);
    assert_eq!(dist.payer_on_time(&payer), 1);
}
