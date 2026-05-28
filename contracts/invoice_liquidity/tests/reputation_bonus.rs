#![cfg(test)]

use invoice_liquidity::rate_logic::calculate_effective_rate;

#[test]
fn test_rate_calculation_bonus_applied() {
    let base_rate = 1000;
    let rep_score = 80;
    let threshold = 80;
    let bonus = 200;
    let min_rate = 100;
    
    let res = calculate_effective_rate(base_rate, rep_score, threshold, bonus, min_rate).unwrap();
    assert_eq!(res, 800);
}

#[test]
fn test_rate_calculation_no_bonus() {
    let base_rate = 1000;
    let rep_score = 79;
    let threshold = 80;
    let bonus = 200;
    let min_rate = 100;
    
    let res = calculate_effective_rate(base_rate, rep_score, threshold, bonus, min_rate).unwrap();
    assert_eq!(res, 1000);
}

#[test]
fn test_rate_calculation_floor_enforced() {
    let base_rate = 300;
    let rep_score = 90;
    let threshold = 80;
    let bonus = 250;
    let min_rate = 100;
    
    let res = calculate_effective_rate(base_rate, rep_score, threshold, bonus, min_rate).unwrap();
    assert_eq!(res, 100);
}

#[test]
fn test_exact_threshold_match() {
    let base_rate = 500;
    let rep_score = 50;
    let threshold = 50;
    let bonus = 100;
    let min_rate = 50;
    
    let res = calculate_effective_rate(base_rate, rep_score, threshold, bonus, min_rate).unwrap();
    assert_eq!(res, 400);
}

#[test]
fn test_zero_reputation() {
    let base_rate = 500;
    let rep_score = 0;
    let threshold = 50;
    let bonus = 100;
    let min_rate = 50;
    
    let res = calculate_effective_rate(base_rate, rep_score, threshold, bonus, min_rate).unwrap();
    assert_eq!(res, 500);
}

#[test]
fn test_maximum_bonus_application() {
    let base_rate = 600;
    let rep_score = 99;
    let threshold = 50;
    let bonus = 500;
    let min_rate = 50;
    
    let res = calculate_effective_rate(base_rate, rep_score, threshold, bonus, min_rate).unwrap();
    assert_eq!(res, 100);
}

#[test]
fn test_zero_base_rate() {
    let base_rate = 0;
    let rep_score = 90;
    let threshold = 50;
    let bonus = 200;
    let min_rate = 50;
    
    let res = calculate_effective_rate(base_rate, rep_score, threshold, bonus, min_rate).unwrap();
    assert_eq!(res, 50);
}
