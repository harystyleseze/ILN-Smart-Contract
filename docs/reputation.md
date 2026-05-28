# ILN Reputation System Documentation

**Document Version:** 1.0  
**Date:** May 2024  
**Status:** Pre-Audit  

## Executive Summary

The Invoice Liquidity Network (ILN) uses a reputation system to help liquidity providers (LPs) assess the creditworthiness of payers and the reliability of freelancers. Both payer and LP reputations are tracked on-chain and decay over time if inactive.

**Key Metrics:**
- **Reputation Range:** 0-100 (default 50 for new accounts)
- **Payer Reputation:** Tracks payment history and default behavior
- **LP Reputation:** Tracks funding success and yield generation
- **Decay Rate:** Configurable basis points per period (default: 50 bps = 0.5%)
- **Decay Period:** Configurable ledger count between decay applications (default: ~1 month)

---

## Part 1: Payer Reputation System

### 1.1 Overview

A **payer's reputation** reflects their payment reliability and trustworthiness. LPs use this score to decide whether to fund an invoice and what discount rate to require.

**Reputation = Payment History + Time Decay**

**Use Cases:**
- LPs filter invoices: "Only fund payers with reputation > 60"
- Automatic rate adjustments: High-rep payers get better discount rates
- Risk assessment: Lower-rep payers are higher risk

### 1.2 Score Calculation

#### Initial Score
- **New Accounts:** Default reputation = **50** (neutral)
- **Established Payers:** Score grows/shrinks based on payment history

#### Score Bounds
- **Minimum:** 0 (worst reputation)
- **Maximum:** 100 (best reputation)
- Scores are clamped: `score = clamp(score, 0, 100)`

### 1.3 Lifecycle Events Affecting Payer Reputation

#### Event 1: Invoice Paid On Time ✅

**Trigger:** Payer calls `mark_paid()` before invoice due date

**Impact:**
```rust
new_score = min(score + 1, 100)
last_activity_ledger = current_ledger
```

**Effect:** Increases reputation by **1 point** (capped at 100)

**Code Location:** [lib.rs - mark_paid()](contracts/invoice_liquidity/src/lib.rs#L1268)

**Example:**
```
Payer Alice (score = 45) pays invoice on time
→ Alice's new score = 46
→ Last activity recorded

Payer Bob (score = 99) pays invoice on time
→ Bob's new score = 100 (capped)
→ Last activity recorded
```

#### Event 2: Invoice Defaults ❌

**Trigger:** Invoice passes due date unpaid, LP calls `claim_default()` (or timeout)

**Impact:**
```rust
new_score = max(score - 5, 0)
last_activity_ledger = current_ledger
```

**Effect:** Decreases reputation by **5 points** (floored at 0)

**Code Location:** [lib.rs - claim_default()](contracts/invoice_liquidity/src/lib.rs#L1352)

**Important:** Default marking is harsh—one default erases five on-time payments.

**Example:**
```
Payer Charlie (score = 70) defaults on invoice
→ Charlie's new score = 65
→ Default penalty applied: -5

Payer Dave (score = 3) defaults on invoice
→ Dave's new score = 0 (floored)
→ Dave is now considered unreliable
```

#### Event 3: Appeal Upheld ⚖️

**Trigger:** Payer disputes a default with evidence, admin upholds the appeal

**Impact:**
```rust
// Restore the score to what it was before default
restored_score = pre_default_score
last_activity_ledger = current_ledger
```

**Effect:** Restores reputation to **pre-default level** (undoes the penalty)

**Code Location:** [lib.rs - resolve_appeal()](contracts/invoice_liquidity/src/lib.rs#L1445)

**Use Case:** Payer was unfairly marked as defaulter (e.g., payer sent payment but LP didn't receive it)

**Example:**
```
Payer Eve (score = 70) defaults
→ Eve's new score = 65 (after -5 penalty)
→ Eve's pre_default_score = 70 (saved)

Eve files appeal with proof of payment
Admin reviews and upholds appeal
→ Eve's new score = 70 (restored from pre_default_score)
→ Default removed from record
```

#### Event 4: Appeal Rejected or Dispute Lost ❌

**Trigger:** Payer disputes a default, admin rejects the appeal

**Impact:**
```rust
// Default penalty remains in effect
score = score  // No change
last_activity_ledger = current_ledger
```

**Effect:** Default penalty is **permanent**

**Example:**
```
Payer Frank (score = 60) defaults
→ Frank's new score = 55 (after -5 penalty)

Frank files appeal claiming "system error"
Admin reviews evidence and rejects appeal
→ Frank's new score = 55 (penalty stands)
```

### 1.4 Reputation Decay Mechanism

#### Problem Solved by Decay

Without decay, a payer's score would be permanent:

```
Year 1: Alice pays 50 invoices on time → score = 100
Year 5: Alice disappears from system (insolvent)
Year 6: Alice's score is still 100 (but she's bankrupt!)
        LPs fund her invoices at premium rates → lose money
```

**Solution:** Scores decay over time if the payer is inactive.

#### How Decay Works

**Formula:**
```
Time since last activity: T ledgers
Decay periods elapsed: P = T / decay_period_ledgers

For each period:
    decayed_score = decayed_score * (1 - decay_rate_bps / 10000)
```

**Code Location:** [invoice.rs - get_payer_score()](contracts/invoice_liquidity/src/invoice.rs#L227-L250)

**Example with Default Parameters:**

```
Default Config:
- decay_rate_bps = 50 (0.5% per period)
- decay_period_ledgers = 259200 (≈ 30 days)

Scenario:
Alice has score = 100 and paid an invoice 60 days ago
- T = 518400 ledgers (60 days)
- P = 518400 / 259200 = 2 periods

Period 1: score = 100 * (1 - 50/10000) = 100 * 0.995 = 99.5
Period 2: score = 99.5 * 0.995 = 99.0 (floored to 99)
```

#### Decay Triggers

Decay is applied **lazily** when the score is queried (not continuously):

1. Freelancer calls `submit_invoice()` (payer's score is checked)
2. LP calls `fund_invoice()` (payer's score is fetched)
3. Admin calls `resolve_appeal()` (payer's score is restored)
4. Any other query to `payer_score(payer_address)`

**No Background Process:** Decay does not happen in the background; only when the score is accessed.

#### Decay Bounds

- Scores decay towards **0** over time (never go negative)
- Decay rate is configurable: 0-500 bps (0-5% per period max)
- Decay period is configurable: any positive number of ledgers
- Admin can pause decay by setting decay_rate_bps = 0

**Example: Long Inactivity**

```
Alice (score = 100) hasn't paid or been involved for 3 years (1095 days)
- T = 1095 * 24 * 60 * 10 ≈ 157,680,000 ledgers (rough estimate)
- P = 157,680,000 / 259200 ≈ 608 periods

After 608 periods of 0.5% decay:
score ≈ 100 * (0.995)^608 ≈ 0.01 (essentially zero)

Result: Alice's score decays to nearly 0 after 3 years of inactivity
```

### 1.5 Worked Example: Full Lifecycle

**Timeline: Alice's Reputation Journey**

```
DAY 0 - Alice joins ILN
├─ Initial score = 50 (default)
└─ last_activity_ledger = 0

DAY 1 - Invoice #1 submitted by Alice
├─ Freelancer Bob submits invoice for Alice to pay
├─ Alice is checked for reputation
├─ Score retrieved: 50 (no decay yet, just 1 ledger of activity)
└─ Status: Pending

DAY 5 - Invoice #1 funded and Alice pays
├─ Alice calls mark_paid()
├─ Reputation update: 50 + 1 = 51
├─ last_activity_ledger = 5
└─ Status: Paid

DAY 10 - Invoice #2 submitted
├─ Score retrieved: 51 (minimal decay over 5 ledgers)
├─ Invoice funded by LP Carol
└─ Status: Pending

DAY 15 - Invoice #2 paid on time
├─ Reputation update: 51 + 1 = 52
├─ last_activity_ledger = 15
└─ Status: Paid

DAY 90 - Alice defaults on Invoice #3
├─ Invoice due date passed
├─ LP Carol calls claim_default()
├─ Reputation update: 52 - 5 = 47
├─ last_activity_ledger = 90
├─ pre_default_score = 52 (saved for potential appeal)
└─ Status: Defaulted

DAY 100 - Alice files an appeal
├─ Alice calls appeal_default() with evidence hash
├─ Evidence reviewed by admin
└─ Status: Appealed

DAY 105 - Admin upholds appeal
├─ resolve_appeal() called by admin
├─ Score restored: 52 (from pre_default_score)
├─ last_activity_ledger = 105
├─ Default penalty removed
└─ Status: Appeal resolved → Paid

DAY 200 - No activity (Alice is inactive)
├─ Decay applies when score is next queried
├─ T = 200 - 105 = 95 days
├─ P = (95 * 24 * 60 * 10) / 259200 ≈ 95 / 30 ≈ 3 periods
├─ Decayed score = 52 * (0.995)^3 ≈ 51.5
└─ If queried: score = 51 (floored)

DAY 500 - Alice re-enters after 300 days of inactivity
├─ T = 500 - 105 = 395 days
├─ P = 395 / 30 ≈ 13 periods
├─ Decayed score = 52 * (0.995)^13 ≈ 49.2
├─ last_activity_ledger updated to 500
└─ Alice's reputation is back to near-neutral (49.2)

DAY 501 - Alice pays an invoice
├─ Reputation update: 49 + 1 = 50
├─ last_activity_ledger = 501
└─ Status: Paid
```

---

## Part 2: LP (Liquidity Provider) Reputation System

### 2.1 Overview

An **LP's reputation** reflects their funding reliability and success in selecting profitable invoices.

**Reputation = Funding History**

**Use Cases:**
- LPs with high reputation get priority in the fund queue (tie-breaker)
- Showcases LP track record to other participants
- May factor into yield bonuses (future features)

### 2.2 Score Calculation

#### Initial Score
- **New LPs:** Default reputation = **50**

#### Score Bounds
- **Minimum:** 0
- **Maximum:** 100
- Clamped: `score = clamp(score, 0, 100)`

### 2.3 Lifecycle Events Affecting LP Reputation

#### Event: Successful Funding ✅

**Trigger:** LP successfully funds an invoice via `fund_invoice()`

**Impact:**
```rust
new_score = min(score + 1, 100)
last_activity_ledger = current_ledger
```

**Effect:** Increases reputation by **1 point**

**Code Location:** [lib.rs - fund_invoice()](contracts/invoice_liquidity/src/lib.rs#L634)

**Important:** LPs gain reputation for **attempting** to fund, not just succeeding. If funding fails (insufficient balance, token issues), reputation is **not** updated.

**Example:**
```
LP Carol (score = 50) successfully funds Invoice #1
→ Carol's LP score = 51
→ Recorded as successful funder

LP Dave (score = 50) attempts to fund but tx fails (insufficient balance)
→ Dave's LP score = 50 (unchanged)
→ No penalty, but no credit either
```

### 2.4 LP Decay

**Same as Payer Decay:**

LP scores also decay over time if inactive.

```
Default Config:
- decay_rate_bps = 50 (0.5% per period)
- decay_period_ledgers = 259200 (≈ 30 days)

Scenario:
Carol (LP score = 100) funded an invoice 90 days ago
- T = 90 * 24 * 60 * 10 ≈ 1,296,000 ledgers
- P = 1,296,000 / 259200 = 5 periods

After 5 periods: score = 100 * (0.995)^5 ≈ 97.5
```

**Access:** Decay is applied when LP score is queried (e.g., during fund queue resolution).

### 2.5 LP Queue & Reputation Snapshot

#### Fund Queue Mechanism

When multiple LPs want to fund the same invoice:

1. **LPs join queue** via `join_fund_queue(invoice_id)`
   - LP reputation is **snapshotted** at join time
   - Stored in `LpFundRequest` struct

2. **Queue resolved** via `resolve_fund_queue(invoice_id)`
   - Selected LP = highest reputation snapshot
   - Reputation at join time determines tie-breaking

**Why Snapshot?** Prevents gaming—LPs can't artificially boost reputation between join and resolution.

**Example:**
```
Invoice #1 available for funding
- Amount: 1000 USDC
- Discount: 3%

LP Carol (reputation = 75) calls join_fund_queue()
├─ Snapshot: Carol's reputation = 75 (at join time)

LP Dave (reputation = 60) calls join_fund_queue()
├─ Snapshot: Dave's reputation = 60 (at join time)

Dave pays himself to boost his reputation: score = 75
Call resolve_fund_queue(invoice_id)
├─ Decision: Compare snapshots, not current scores
├─ Carol snapshot = 75 > Dave snapshot = 60
├─ Winner: Carol is selected
├─ Dave's current score = 75 doesn't matter
```

---

## Part 3: Governance Parameters

### 3.1 Configuration Structure

```rust
pub struct Config {
    pub high_rep_threshold: u32,      // Reputation score threshold for bonus discount
    pub bonus_bps: u32,                // Discount bonus in basis points
    pub min_discount_rate_bps: u32,   // Floor on discount rate
    pub decay_rate_bps: u32,           // Decay per period (basis points)
    pub decay_period_ledgers: u64,    // Ledger count per period
    pub dispute_timeout_ledgers: u64, // Auto-resolve dispute after this many ledgers
}
```

### 3.2 Parameter Descriptions

#### Parameter 1: `high_rep_threshold` (u32)

**Range:** 0-100 (capped by init logic)

**Default:** 60

**Purpose:** If a payer's reputation is ≥ this threshold, they qualify for reputation bonus discount

**Impact:**
- **High value (e.g., 90):** Only top 10% of payers get discounts → stricter filtering
- **Low value (e.g., 30):** Most payers get discounts → more lenient
- **Recommended:** 50-70 (favors reliable payers, not too restrictive)

**Formula in Reputation Bonus Contract:**
```rust
if payer_reputation >= high_rep_threshold {
    effective_discount = base_discount - bonus_bps
} else {
    effective_discount = base_discount
}
```

#### Parameter 2: `bonus_bps` (u32)

**Range:** 0-500 (capped by max)

**Default:** 100 (1%)

**Purpose:** Additional discount given to payers above `high_rep_threshold`

**Impact:**
- **High value (e.g., 500):** High-rep payers save 5% → strong incentive
- **Low value (e.g., 10):** High-rep payers save 0.1% → minimal incentive
- **Recommended:** 50-200 (50 bps = 0.5% to 200 bps = 2%)

**Example:**
```
Config:
- high_rep_threshold = 60
- bonus_bps = 100

Invoice discount_rate = 300 bps (3%)

Payer reputation = 70 (≥ 60, qualifies)
→ Effective rate = 300 - 100 = 200 bps (2%)
→ Payer saves 100 bps (1%)

Payer reputation = 50 (< 60, doesn't qualify)
→ Effective rate = 300 bps (3%)
→ No discount
```

#### Parameter 3: `min_discount_rate_bps` (u32)

**Range:** 1-500 (must be ≥ 1, no maximum cap enforced)

**Default:** 50 (0.5%)

**Purpose:** Minimum discount rate floor to protect LP yields

**Impact:**
- **High value (e.g., 300):** LPs always get ≥ 3% discount → protects LP but deters freelancers
- **Low value (e.g., 10):** LPs can accept ≤ 0.1% discount → more invoices fundable but lower LP yield
- **Recommended:** 30-100 (0.3%-1%)

**Formula:**
```rust
effective_rate = max(calculated_rate, min_discount_rate_bps)
```

**Example:**
```
Config:
- bonus_bps = 100
- min_discount_rate_bps = 50

Invoice discount_rate = 25 bps (0.25%)

High-rep payer (reputation = 70):
→ Calculated rate = 25 - 100 = -75 (negative, not allowed)
→ Effective rate = max(-75, 50) = 50 bps (0.5%)
→ LP's minimum yield is protected
```

#### Parameter 4: `decay_rate_bps` (u32)

**Range:** 0-10000 (0-100%)

**Default:** 50 (0.5%)

**Purpose:** Percentage of reputation lost per period due to inactivity

**Impact:**
- **High value (e.g., 500):** 5% decay per period → scores drop fast → old data discounted quickly
- **Low value (e.g., 10):** 0.1% decay per period → scores drop slowly → old data still relevant
- **Recommended:** 30-100 (0.3%-1%)

**Formula:**
```rust
decayed_score = score * (1 - decay_rate_bps / 10000)
```

**Decay Examples:**
```
Score = 100, decay_rate_bps = 50 (0.5%)
After 1 period: 100 * (1 - 50/10000) = 100 * 0.995 = 99.5
After 2 periods: 99.5 * 0.995 ≈ 99.0
After 10 periods: 100 * (0.995)^10 ≈ 95.1

Score = 100, decay_rate_bps = 500 (5%)
After 1 period: 100 * (1 - 500/10000) = 100 * 0.95 = 95
After 2 periods: 95 * 0.95 = 90.25
After 10 periods: 100 * (0.95)^10 ≈ 59.9 (decays much faster!)
```

#### Parameter 5: `decay_period_ledgers` (u64)

**Range:** 1+

**Default:** 259200 (≈ 30 days at 5s blocks)

**Purpose:** Number of ledgers between decay applications

**Impact:**
- **High value (e.g., 1000000):** ~200 days per period → slow decay
- **Low value (e.g., 10000):** ~2 days per period → fast decay
- **Recommended:** 100000-500000 (~20-100 days)

**Ledger Estimation:**
```
Soroban blocks ≈ 5 seconds (typical)
Ledgers per day = (24 * 60 * 60) / 5 = 17,280
Ledgers per month ≈ 30 * 17,280 = 518,400

Default = 259,200 ledgers ≈ 15 days
```

#### Parameter 6: `dispute_timeout_ledgers` (u64)

**Range:** 1+

**Default:** 432000 (≈ 25 days)

**Purpose:** Auto-resolve disputes after this many ledgers (if unresolved)

**Impact:**
- **High value (e.g., 1000000):** Disputes can go unresolved for months → delays payer relief
- **Low value (e.g., 10000):** Disputes auto-resolve in ~2 days → fast closure but less review time
- **Recommended:** 300000-600000 (~20-40 days)

**Example:**
```
Invoice disputed on ledger 1000
dispute_timeout_ledgers = 432000
Auto-resolve triggers at ledger = 1000 + 432000 = 433000
Time passed ≈ 432000 * 5s ≈ 600 hours ≈ 25 days
```

### 3.3 Updating Configuration

**Access:** Admin only

**Function:**
```rust
pub fn update_config(
    env: Env,
    high_rep_threshold: u32,
    bonus_bps: u32,
    min_discount_rate_bps: u32,
    decay_rate_bps: u32,
    decay_period_ledgers: u64,
    dispute_timeout_ledgers: u64,
) -> Result<(), ConfigError>
```

**Validation:**
- `bonus_bps` must be ≤ 500 (max bonus is 5%)
- `min_discount_rate_bps` must be > 0
- No bounds check on other parameters (future improvement)

**Example:**
```bash
# Tighten reputation requirement to 65 (was 60)
soroban contract invoke \
  --id CONTRACT_ID \
  -- update_config \
  --high_rep_threshold 65 \
  --bonus_bps 100 \
  --min_discount_rate_bps 50 \
  --decay_rate_bps 50 \
  --decay_period_ledgers 259200 \
  --dispute_timeout_ledgers 432000
```

---

## Part 4: LP Threshold Filtering

### 4.1 Purpose

LPs want to filter invoices by payer reputation before deciding to fund.

**Example Use Case:**
```
LP says: "I only fund payers with reputation > 70"
System checks payer_reputation(payer_address)
If reputation >= 70 → Show invoice to LP
If reputation < 70 → Hide invoice from LP
```

### 4.2 How It Works

#### Client-Side Filtering (Recommended)

LPs query on-chain and filter locally:

```javascript
// Frontend code
const payer_reputation = await get_payer_score(payer_address);
const min_acceptable_reputation = 70;

if (payer_reputation >= min_acceptable_reputation) {
    // Show invoice to LP
    display_invoice(invoice);
} else {
    // Hide invoice
    mark_as_not_eligible(invoice);
}
```

#### Query Function

```rust
pub fn payer_score(env: Env, payer: Address) -> u32 {
    get_payer_score(&env, &payer)
}
```

- **Input:** Payer address
- **Output:** Current reputation score (0-100)
- **Includes:** Decay applied (if necessary)

### 4.3 Worked Example

```
Config:
- high_rep_threshold = 60
- bonus_bps = 100
- min_discount_rate_bps = 50

Invoice Details:
- Freelancer: Alice
- Payer: Bob
- Amount: 1000 USDC
- Due Date: 30 days
- Discount Rate: 300 bps (3.0%)

LP Carol's Filtering Decision:
1. Query: get_payer_score(Bob) → returns 72
2. Threshold check: 72 >= 60 ✅ Bob is high-rep
3. Effective rate calculation:
   - Base: 300 bps
   - Bonus: 100 bps (because reputation >= 60)
   - Effective: 300 - 100 = 200 bps (2.0%)
   - Min check: 200 > 50 ✅ (above minimum)
   - Final rate: 200 bps (2.0%)
4. Yield calculation:
   - LP funds 1000 USDC at 2% discount
   - LP receives 1000 + (1000 * 0.02) = 1020 USDC
   - Yield: 20 USDC
5. Carol's decision: "2% yield is acceptable, fund the invoice"
```

---

## Part 5: Events & Monitoring

### 5.1 Reputation Events

The contract emits events when reputation changes:

| Event | Triggered By | Data |
|-------|-----------|------|
| `InvoicePaid` | mark_paid() | invoice_id, payer, lp, paid_on_time |
| `InvoiceDefaulted` | claim_default() | invoice_id, payer, lp, defaulted_amount |
| `DefaultAppealed` | appeal_default() | invoice_id, payer, evidence_hash |
| `AppealResolved` | resolve_appeal() | invoice_id, payer, outcome |
| `FundQueueResolved` | resolve_fund_queue() | invoice_id, selected_lp |

### 5.2 Off-Chain Monitoring

Indexers and frontends can subscribe to reputation events:

```javascript
// Listen for payer defaults
contract.events.on('InvoiceDefaulted', (event) => {
    console.log(`Payer ${event.payer} defaulted on invoice ${event.invoice_id}`);
    // Update UI, notify LP
});

// Listen for successful payments
contract.events.on('InvoicePaid', (event) => {
    console.log(`Payer ${event.payer} paid invoice ${event.invoice_id}`);
    // Update reputation display, celebrate
});
```

---

## Part 6: Common Questions

### Q1: How long do reputation changes take effect?

**A:** Immediately. When `mark_paid()` or `claim_default()` is called, reputation is updated and the change is reflected on the next query.

### Q2: Can I dispute a default?

**A:** Yes, via `appeal_default()`. You must file within 30 days of the due date, and the admin will review your evidence.

### Q3: What happens if I've been inactive for a year?

**A:** Your reputation decays over time. With default decay settings (0.5% per month), after 12 months, your score would be approximately:

```
Score = 100 * (0.995)^12 ≈ 94.1
```

Less severe than one default penalty (-5 points).

### Q4: Can admins change my reputation?

**A:** Admins cannot directly set reputation scores. However:
- They can resolve appeals (which restores pre-default scores)
- They can change decay parameters (which affects all payers)
- They can pause the contract (which prevents reputation changes)

### Q5: How is LP reputation different from payer reputation?

**A:** 
- **Payer reputation:** Reflects **payment reliability** (on-time payments vs. defaults)
- **LP reputation:** Reflects **funding activity** (number of successful fundings)

LPs use payer reputation to decide who to fund; fund queue uses LP reputation as a tie-breaker.

### Q6: What's the reputation bonus for high-rep payers?

**A:** High-rep payers (score ≥ `high_rep_threshold`) receive a discount bonus:

```
effective_discount = base_discount - bonus_bps
```

**Default:** Base 3% discount → High-rep payers get 2% discount (1% savings).

### Q7: Can I game the system with multiple accounts?

**A:** Partially:
- New accounts start at 50 (neutral), not 0
- Defaults incur -5 penalty (harsh)
- Decay means old history fades over time
- Admin can monitor and pause suspicious patterns

Creating sybil accounts is inefficient because:
- Each account must build reputation separately
- One default erases 5 on-time payments
- Inactive accounts decay back to baseline

---

## Part 7: Implementation Checklist for Developers

### For Integrating with ILN:

- [ ] Query `payer_score()` before displaying invoices to LPs
- [ ] Query `lp_score()` for LP reputation display
- [ ] Subscribe to reputation events (`InvoicePaid`, `InvoiceDefaulted`)
- [ ] Display reputation badges in UI (e.g., "Trusted Payer 👍")
- [ ] Implement LP reputation-based sorting in fund queue UI
- [ ] Monitor decay parameters via `get_config()`
- [ ] Cache reputation scores (but invalidate after 1 hour)
- [ ] Alert LPs if payer reputation drops significantly
- [ ] Include reputation info in transaction notifications

### For ILN Governance:

- [ ] Review decay parameters annually
- [ ] Monitor default rate vs. reputation distribution
- [ ] Adjust `high_rep_threshold` if defaults increase
- [ ] Increase `bonus_bps` if LPs want stronger high-rep incentives
- [ ] Publish reputation statistics monthly (% high-rep payers, avg default rate)
- [ ] Consider reputation thresholds for other features (e.g., LP priority access)

---

## Part 8: Reputation Model Philosophy

The ILN reputation system embodies these principles:

### 1. **Trust, Verify, Decay**
- Trust new participants (default 50)
- Verify through payment history
- Decay old data (inactivity penalty)

### 2. **Asymmetric Penalties**
- Good behavior: +1 point
- Bad behavior: -5 points
- **Intuition:** Trust is hard to earn, easy to lose

### 3. **Reversibility**
- Appeals can undo defaults
- Allows correction of false positives
- Admin is fallback arbitrator

### 4. **Transparency**
- All reputation events are on-chain
- Anyone can query any address's score
- No hidden scoring algorithm

### 5. **Economic Incentives**
- High reputation → better discounts
- Better discounts → more invoice funding
- More funding → more reputation gains
- Virtuous cycle for reliable payers

---

## Appendix: Configuration Recommendations

### Conservative (Low Risk)

```
high_rep_threshold = 70        // Strict filtering
bonus_bps = 150               // 1.5% bonus for high-rep
min_discount_rate_bps = 100   // 1% minimum to protect LPs
decay_rate_bps = 100          // 1% decay per period (faster decay)
decay_period_ledgers = 150000 // ~8 days per period (fast decay)
dispute_timeout_ledgers = 259200 // 15 days dispute window
```

**Result:** Conservative filtering, fast decay of old reputations, strong incentive for high-rep payers

### Balanced (Moderate Risk)

```
high_rep_threshold = 60        // Moderate filtering
bonus_bps = 100               // 1% bonus for high-rep
min_discount_rate_bps = 50    // 0.5% minimum
decay_rate_bps = 50           // 0.5% decay per period (moderate)
decay_period_ledgers = 259200 // 15 days per period
dispute_timeout_ledgers = 432000 // 25 days dispute window
```

**Result:** Balanced approach, moderate decay, reasonable dispute resolution time

### Lenient (High Risk)

```
high_rep_threshold = 50        // Permissive filtering
bonus_bps = 50                // 0.5% bonus for high-rep
min_discount_rate_bps = 20    // 0.2% minimum (risky)
decay_rate_bps = 20           // 0.2% decay per period (slow decay)
decay_period_ledgers = 500000 // 30+ days per period (slow)
dispute_timeout_ledgers = 600000 // 30+ days dispute window
```

**Result:** Permissive filtering, slow decay, more default risk but higher LP activity

---

**Document Prepared By:** Product & Security Team  
**Last Updated:** May 2024  
**Next Review:** Q4 2024 (after mainnet launch)
