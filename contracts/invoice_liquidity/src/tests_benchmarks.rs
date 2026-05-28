#![cfg(test)]

//! Benchmarks for Invoice Liquidity contract core functions

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};

// ----------------------------------------------------------------
// Cost Conversion Constants
// ----------------------------------------------------------------
/// 1 XLM = 10_000_000 stroops. We will define an approximate conversion rate for instructions and memory
/// Since Soroban fees depend on a complex cost model, we use a mock linear conversion rule for testing.
const CPU_FEE_RATE: f64 = 0.00000001; // Example: 1 stroop per 100 instructions
const MEM_FEE_RATE: f64 = 0.0000001;  // Example: 1 stroop per 10 bytes

// ----------------------------------------------------------------
// Threshold Limits
// ----------------------------------------------------------------
const MAX_CPU_SUBMIT: u64 = 5_000_000;
const MAX_MEM_SUBMIT: u64 = 3_000_000;
const MAX_CPU_FUND: u64 = 5_000_000;
const MAX_MEM_FUND: u64 = 3_000_000;
const MAX_CPU_MARK_PAID: u64 = 5_000_000;
const MAX_MEM_MARK_PAID: u64 = 3_000_000;
const MAX_CPU_CLAIM: u64 = 5_000_000;
const MAX_MEM_CLAIM: u64 = 3_000_000;

struct BaseBenchEnv {
    env: Env,
    contract: InvoiceLiquidityContractClient<'static>,
    token: Address,
    freelancer: Address,
    payer: Address,
    lp: Address,
}

fn setup_benchmark_env() -> BaseBenchEnv {
    let env = Env::default();
    env.mock_all_auths();
    
    // Enable cost tracking
    env.cost_estimate().budget().reset_unlimited();

    let mut ledger = env.ledger().get();
    ledger.timestamp = 1_700_000_000;
    env.ledger().set(ledger);

    let usdc_admin = Address::generate(&env);
    let usdc = env.register_stellar_asset_contract_v2(usdc_admin.clone());
    let xlm_admin = Address::generate(&env);
    let xlm = env.register_stellar_asset_contract_v2(xlm_admin);

    let contract_id = env.register(InvoiceLiquidityContract, ());
    let contract = InvoiceLiquidityContractClient::new(&env, &contract_id);
    contract.initialize(&usdc_admin, &usdc.address(), &xlm.address());

    let freelancer = Address::generate(&env);
    let payer = Address::generate(&env);
    let lp = Address::generate(&env);

    let usdc_client = StellarAssetClient::new(&env, &usdc.address());
    usdc_client.mint(&lp, &1_000_000_000_000);
    usdc_client.mint(&payer, &1_000_000_000_000);

    BaseBenchEnv {
        env,
        contract,
        token: usdc.address(),
        freelancer,
        payer,
        lp,
    }
}

fn compute_fee(cpu: u64, mem: u64) -> f64 {
    let fee_stroops = (cpu as f64 * CPU_FEE_RATE) + (mem as f64 * MEM_FEE_RATE);
    fee_stroops / 10_000_000.0
}

#[test]
fn test_benchmark_all_functions() {
    let mut results = std::vec::Vec::new();
    
    // ----------------------------------------------------------------
    // 1. Benchmark: submit_invoice
    // ----------------------------------------------------------------
    let b1 = setup_benchmark_env();
    let due_date = b1.env.ledger().timestamp() + 86_400;
    let amount = 1_000_000_000;
    
    b1.env.cost_estimate().budget().reset_unlimited();
    let id = b1.contract.submit_invoice(
        &b1.freelancer,
        &b1.payer,
        &amount,
        &due_date,
        &300,
        &b1.token,
    );
    let cpu_submit = b1.env.cost_estimate().budget().cpu_instruction_cost();
    let mem_submit = b1.env.cost_estimate().budget().memory_bytes_cost();
    
    assert!(cpu_submit <= MAX_CPU_SUBMIT, "submit_invoice CPU usage exceeded threshold: {} > {}", cpu_submit, MAX_CPU_SUBMIT);
    assert!(mem_submit <= MAX_MEM_SUBMIT, "submit_invoice Mem usage exceeded threshold: {} > {}", mem_submit, MAX_MEM_SUBMIT);
    
    results.push(("submit_invoice", cpu_submit, mem_submit, compute_fee(cpu_submit, mem_submit)));

    // ----------------------------------------------------------------
    // 2. Benchmark: fund_invoice
    // ----------------------------------------------------------------
    b1.env.cost_estimate().budget().reset_unlimited();
    b1.contract.fund_invoice(&b1.lp, &id, &amount);
    
    let cpu_fund = b1.env.cost_estimate().budget().cpu_instruction_cost();
    let mem_fund = b1.env.cost_estimate().budget().memory_bytes_cost();
    
    assert!(cpu_fund <= MAX_CPU_FUND, "fund_invoice CPU usage exceeded threshold: {} > {}", cpu_fund, MAX_CPU_FUND);
    assert!(mem_fund <= MAX_MEM_FUND, "fund_invoice Mem usage exceeded threshold: {} > {}", mem_fund, MAX_MEM_FUND);
    
    results.push(("fund_invoice", cpu_fund, mem_fund, compute_fee(cpu_fund, mem_fund)));

    // ----------------------------------------------------------------
    // 3. Benchmark: mark_paid
    // ----------------------------------------------------------------
    b1.env.cost_estimate().budget().reset_unlimited();
    b1.contract.mark_paid(&id, &INVOICE_AMOUNT);
    
    let cpu_mark = b1.env.cost_estimate().budget().cpu_instruction_cost();
    let mem_mark = b1.env.cost_estimate().budget().memory_bytes_cost();
    
    assert!(cpu_mark <= MAX_CPU_MARK_PAID, "mark_paid CPU usage exceeded threshold: {} > {}", cpu_mark, MAX_CPU_MARK_PAID);
    assert!(mem_mark <= MAX_MEM_MARK_PAID, "mark_paid Mem usage exceeded threshold: {} > {}", mem_mark, MAX_MEM_MARK_PAID);
    
    results.push(("mark_paid", cpu_mark, mem_mark, compute_fee(cpu_mark, mem_mark)));

    // ----------------------------------------------------------------
    // 4. Benchmark: claim_default
    // ----------------------------------------------------------------
    let b2 = setup_benchmark_env();
    let due_date2 = b2.env.ledger().timestamp() + 86_400; // Future date
    let id2 = b2.contract.submit_invoice(
        &b2.freelancer,
        &b2.payer,
        &amount,
        &due_date2,
        &300,
        &b2.token,
    );
    b2.contract.fund_invoice(&b2.lp, &id2, &amount);
    
    // Simulate past due date
    let mut ledger = b2.env.ledger().get();
    ledger.timestamp = due_date2 + 86_400; // 1 day late
    b2.env.ledger().set(ledger);
    
    b2.env.cost_estimate().budget().reset_unlimited();
    let _ = b2.contract.try_claim_default(&b2.lp, &id2);
    
    let cpu_claim = b2.env.cost_estimate().budget().cpu_instruction_cost();
    let mem_claim = b2.env.cost_estimate().budget().memory_bytes_cost();
    
    assert!(cpu_claim <= MAX_CPU_CLAIM, "claim_default CPU usage exceeded threshold: {} > {}", cpu_claim, MAX_CPU_CLAIM);
    assert!(mem_claim <= MAX_MEM_CLAIM, "claim_default Mem usage exceeded threshold: {} > {}", mem_claim, MAX_MEM_CLAIM);
    
    results.push(("claim_default", cpu_claim, mem_claim, compute_fee(cpu_claim, mem_claim)));

    // ----------------------------------------------------------------
    // Print Table Result
    // ----------------------------------------------------------------
    println!("\n| Function       | CPU Instructions | Memory (bytes) | Estimated Fee (XLM) |");
    println!("| -------------- | ---------------- | -------------- | ------------------- |");
    for (name, cpu, mem, fee) in results {
        println!("| {:<14} | {:>16} | {:>14} | {:>19.7} |", name, cpu, mem, fee);
    }
    println!();
}
