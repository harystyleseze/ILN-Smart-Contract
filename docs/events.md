# ILN Smart Contract Event Schema

## Overview
This document outlines the event schema for the ILN Smart Contract ecosystem. These events are intended for consumption by indexers, blockchain explorers, and off-chain backend integrations. Soroban events provide a reliable audit trail for critical state transitions. Events are categorized by topics (indexed fields) and data payloads. Indexers should rely on these emitted events to reconstruct state rather than scraping the ledger directly.

## Event Index

| Event | Description |
| ----- | ----------- |
| [InvoiceSubmitted](#invoicesubmitted) | Emitted when a new invoice is created |
| [InvoiceUpdated](#invoiceupdated) | Emitted when an invoice is updated |
| [InvoiceFunded](#invoicefunded) | Emitted when an invoice receives funding |
| [InvoicePaid](#invoicepaid) | Emitted when an invoice is settled by the payer |
| [InvoiceCancelled](#invoicecancelled) | Emitted when an invoice is cancelled |
| [InvoiceDefaulted](#invoicedefaulted) | Emitted when an invoice goes into default |
| [InvoiceTransferred](#invoicetransferred) | Emitted when invoice ownership is transferred |
| [DefaultAppealed](#defaultappealed) | Emitted when a payer appeals a default marking |
| [AppealResolved](#appealresolved) | Emitted when an appeal is resolved |
| [FundRequested](#fundrequested) | Emitted when an LP registers to fund via priority queue |
| [FundQueueResolved](#fundqueueresolved) | Emitted when priority funding queue is resolved |
| [AdminChanged](#adminchanged) | Emitted when the contract admin is updated |
| [VoteCast](#votecast) | Emitted when a governance vote is cast |
| [ProposalVetoed](#proposalvetoed) | Emitted when the admin vetoes a governance proposal |
| [ContractPaused](#contractpaused) | Partially implemented |
| [ContractUnpaused](#contractunpaused) | Partially implemented |
| [InvoiceExpired](#invoiceexpired) | Referenced in requirements but missing |
| [InvoiceDisputed](#invoicedisputed) | Referenced in requirements but missing |
| [ReputationUpdated](#reputationupdated) | Referenced in requirements but missing |
| [TokenAdded](#tokenadded) | Referenced in requirements but missing |
| [TokenRemoved](#tokenremoved) | Referenced in requirements but missing |
| [LPPositionTransferred](#lppositiontransferred) | Referenced in requirements but missing |

---

## InvoiceSubmitted

### Description
Emitted when a freelancer successfully submits a new invoice.

### Trigger Condition
Triggered during the initial invoice submission transaction after successful validation.

### Event Topics
Topics:
`["submitted", invoice_id, freelancer, payer]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `invoice_id` | `u64` | The unique identifier of the invoice (Topic) |
| `freelancer` | `Address` | The address of the freelancer who submitted the invoice (Topic) |
| `payer` | `Address` | The address of the payer responsible for settlement (Topic) |
| `token` | `Address` | The contract address of the accepted payment token |
| `amount` | `i128` | Total invoice amount |
| `due_date` | `u64` | Expiration/due timestamp |
| `discount_rate` | `u32` | Discount rate offered for early liquidity |
| `status` | `InvoiceStatus` | Current state of the invoice |
| `timestamp` | `u64` | Ledger timestamp when the invoice was submitted |

### Example Payload
```json
{
  "invoice_id": 42,
  "freelancer": "GBRPYHIL2C...",
  "payer": "GCFX...",
  "token": "CCW...",
  "amount": "500000000",
  "due_date": 1735603200,
  "discount_rate": 500,
  "status": "Pending",
  "timestamp": 1700000000
}
```

---

## InvoiceUpdated

### Description
Emitted when an invoice's parameters are updated prior to funding.

### Trigger Condition
Triggered when the freelancer modifies an unfunded invoice.

### Event Topics
Topics:
`["updated", invoice_id, freelancer, payer]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `invoice_id` | `u64` | The unique identifier of the invoice (Topic) |
| `freelancer` | `Address` | The address of the freelancer (Topic) |
| `payer` | `Address` | The address of the payer (Topic) |
| `token` | `Address` | The contract address of the token |
| `amount` | `i128` | Total invoice amount |
| `due_date` | `u64` | Expiration/due timestamp |
| `discount_rate` | `u32` | Discount rate offered |
| `status` | `InvoiceStatus` | Current state of the invoice |

### Example Payload
```json
{
  "invoice_id": 42,
  "freelancer": "GBRPYHIL2C...",
  "payer": "GCFX...",
  "token": "CCW...",
  "amount": "600000000",
  "due_date": 1735603200,
  "discount_rate": 600,
  "status": "Pending"
}
```

---

## InvoiceFunded

### Description
Emitted when an invoice is fully or partially funded by a Liquidity Provider (LP).

### Trigger Condition
Triggered when an LP supplies liquidity to the invoice.

### Event Topics
Topics:
`["funded", invoice_id, funder]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `invoice_id` | `u64` | The unique identifier of the invoice (Topic) |
| `funder` | `Address` | The address of the LP providing funds (Topic) |
| `freelancer` | `Address` | The freelancer address receiving the funds |
| `payer` | `Address` | The payer address |
| `token` | `Address` | The payment token address |
| `fund_amount` | `i128` | The newly provided funding amount |
| `amount_funded` | `i128` | Total amount funded so far |
| `invoice_amount` | `i128` | The total invoice amount |
| `due_date` | `u64` | The invoice due date |
| `discount_rate` | `u32` | Applied discount rate |
| `funded_at` | `Option<u64>` | Ledger timestamp when funding occurred |
| `status` | `InvoiceStatus` | Current state of the invoice |

### Example Payload
```json
{
  "invoice_id": 42,
  "funder": "GBLP...",
  "freelancer": "GBRPYHIL2C...",
  "payer": "GCFX...",
  "token": "CCW...",
  "fund_amount": "450000000",
  "amount_funded": "450000000",
  "invoice_amount": "500000000",
  "due_date": 1735603200,
  "discount_rate": 500,
  "funded_at": 1700050000,
  "status": "Funded"
}
```

---

## InvoicePaid

### Description
Emitted when a payer settles the invoice.

### Trigger Condition
Triggered when the payer transfers the required settlement amount to the contract.

### Event Topics
Topics:
`["paid", invoice_id, payer, lp]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `invoice_id` | `u64` | The unique identifier of the invoice (Topic) |
| `payer` | `Address` | The address of the payer who settled (Topic) |
| `lp` | `Address` | The LP address receiving the payout (Topic) |
| `freelancer` | `Address` | The freelancer address |
| `token` | `Address` | The payment token address |
| `amount_paid` | `i128` | Full amount settled by payer |
| `lp_earned` | `i128` | LP earnings (`amount_paid` - `amount_funded`) |
| `lp_payout` | `i128` | Total amount distributed to LP |
| `settlement_timestamp` | `u64` | Settlement ledger timestamp |
| `paid_on_time` | `bool` | Whether the settlement occurred before the due date |
| `status` | `InvoiceStatus` | The resulting state (`Paid`) |

### Example Payload
```json
{
  "invoice_id": 42,
  "payer": "GCFX...",
  "lp": "GBLP...",
  "freelancer": "GBRPYHIL2C...",
  "token": "CCW...",
  "amount_paid": "500000000",
  "lp_earned": "50000000",
  "lp_payout": "500000000",
  "settlement_timestamp": 1700100000,
  "paid_on_time": true,
  "status": "Paid"
}
```

---

## InvoiceCancelled

### Description
Emitted when an invoice is cancelled.

### Trigger Condition
Triggered when an unfunded invoice is cancelled by the freelancer.

### Event Topics
Topics:
`["cancelled", invoice_id]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `invoice_id` | `u64` | The unique identifier of the invoice (Topic) |
| `freelancer` | `Address` | The address of the freelancer who cancelled it |
| `status` | `InvoiceStatus` | The resulting state (`Cancelled`) |

### Example Payload
```json
{
  "invoice_id": 42,
  "freelancer": "GBRPYHIL2C...",
  "status": "Cancelled"
}
```

---

## InvoiceDefaulted

### Description
Emitted when an invoice surpasses its due date without settlement.

### Trigger Condition
Triggered when an authorized party marks an overdue invoice as defaulted.

### Event Topics
Topics:
`["defaulted", invoice_id, funder]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `invoice_id` | `u64` | The unique identifier of the invoice (Topic) |
| `funder` | `Address` | The address of the funder (Topic) |
| `freelancer` | `Address` | The freelancer address |
| `payer` | `Address` | The defaulting payer address |
| `token` | `Address` | The payment token address |
| `amount` | `i128` | The original invoice amount |
| `due_date` | `u64` | The missed due date |
| `defaulted_at` | `u64` | Timestamp of the default marking |
| `discount_amount` | `i128` | The applied discount amount |
| `status` | `InvoiceStatus` | The resulting state (`Defaulted`) |

### Example Payload
```json
{
  "invoice_id": 42,
  "funder": "GBLP...",
  "freelancer": "GBRPYHIL2C...",
  "payer": "GCFX...",
  "token": "CCW...",
  "amount": "500000000",
  "due_date": 1735603200,
  "defaulted_at": 1735605000,
  "discount_amount": "50000000",
  "status": "Defaulted"
}
```

---

## InvoiceTransferred

### Description
Emitted when the ownership of an invoice changes.

### Trigger Condition
Triggered when the original freelancer transfers the invoice to a new owner.

### Event Topics
Topics:
`["transferred", invoice_id]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `invoice_id` | `u64` | The unique identifier of the invoice (Topic) |
| `old_freelancer` | `Address` | The previous freelancer address |
| `new_freelancer` | `Address` | The new freelancer address |
| `status` | `InvoiceStatus` | The current invoice state |

### Example Payload
```json
{
  "invoice_id": 42,
  "old_freelancer": "GBRPYHIL2C...",
  "new_freelancer": "GZNEW...",
  "status": "Pending"
}
```

---

## DefaultAppealed

### Description
Emitted when a payer files an appeal against an unfair default marking.

### Trigger Condition
Triggered during the 30-day appeal window when a payer submits evidence.

### Event Topics
Topics:
`["default_appealed", invoice_id, payer]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `invoice_id` | `u64` | The unique identifier of the invoice (Topic) |
| `payer` | `Address` | The payer filing the appeal (Topic) |
| `evidence_hash` | `BytesN<32>` | SHA-256 hash of off-chain evidence provided |
| `appealed_at` | `u64` | Timestamp when the appeal was filed |

### Example Payload
```json
{
  "invoice_id": 42,
  "payer": "GCFX...",
  "evidence_hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  "appealed_at": 1735700000
}
```

---

## AppealResolved

### Description
Emitted when governance resolves a payer's appeal.

### Trigger Condition
Triggered by governance after reviewing evidence.

### Event Topics
Topics:
`["appeal_resolved", invoice_id, payer]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `invoice_id` | `u64` | The unique identifier of the invoice (Topic) |
| `payer` | `Address` | The payer whose appeal was resolved (Topic) |
| `upheld` | `bool` | True if the default is reversed, false if rejected |
| `resolved_at` | `u64` | Timestamp of the resolution |

### Example Payload
```json
{
  "invoice_id": 42,
  "payer": "GCFX...",
  "upheld": true,
  "resolved_at": 1736000000
}
```

---

## FundRequested

### Description
Emitted when an LP registers intent to fund via the priority queue.

### Trigger Condition
Triggered when an LP queues up to supply liquidity based on their reputation.

### Event Topics
Topics:
`["fund_requested", invoice_id, lp]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `invoice_id` | `u64` | The target invoice ID (Topic) |
| `lp` | `Address` | The LP address requesting to fund (Topic) |
| `score` | `u32` | LP's reputation score at registration time |

### Example Payload
```json
{
  "invoice_id": 42,
  "lp": "GBLP...",
  "score": 850
}
```

---

## FundQueueResolved

### Description
Emitted when the priority queue resolves, selecting a winning LP.

### Trigger Condition
Triggered at the end of the priority queue window to finalize funding rights.

### Event Topics
Topics:
`["fund_queue_resolved", invoice_id, approved_lp]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `invoice_id` | `u64` | The target invoice ID (Topic) |
| `approved_lp` | `Address` | The winning LP address (Topic) |
| `score` | `u32` | Winning score that secured priority |

### Example Payload
```json
{
  "invoice_id": 42,
  "approved_lp": "GBLP...",
  "score": 850
}
```

---

## AdminChanged

### Description
Emitted whenever the contract admin address is updated. Provides an on-chain audit trail.

### Trigger Condition
Triggered by the current admin transferring power.

### Event Topics
Topics:
`["admin_changed"]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `old_admin` | `Address` | The previous admin address |
| `new_admin` | `Address` | The new admin address |
| `timestamp` | `u64` | Ledger timestamp of the transition |

### Example Payload
```json
{
  "old_admin": "GBOLD...",
  "new_admin": "GCNEW...",
  "timestamp": 1700000000
}
```

---

## VoteCast

### Description
Emitted when a governance vote is cast.

### Trigger Condition
Triggered in the governance contract when a voter successfully records their vote.

### Event Topics
Topics:
`["vote_cast", proposal_id, voter]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `proposal_id` | `u64` | The target governance proposal ID (Topic) |
| `voter` | `Address` | The address casting the vote (Topic) |
| `support` | `bool` | True if voted for, false if voted against |
| `weight` | `i128` | Voting weight derived from token balance |

### Example Payload
```json
{
  "proposal_id": 1,
  "voter": "GCGOV...",
  "support": true,
  "weight": "10000000000"
}
```

---

## ProposalVetoed

### Description
Emitted when the admin exercises their emergency veto power to block a governance proposal.

### Trigger Condition
Triggered when the admin calls `veto_proposal(proposal_id, reason_hash)` on the governance contract while veto power is still enabled. The proposal must be in `Active` or `Passed` status.

### Event Topics
Topics:
`["proposal_vetoed", proposal_id, admin]`

### Field Schema Table

| Field | Type | Description |
| ----- | ---- | ----------- |
| `proposal_id` | `u64` | The ID of the vetoed proposal (Topic) |
| `admin` | `Address` | The admin address that issued the veto (Topic) |
| `reason_hash` | `BytesN<32>` | SHA-256 hash of an off-chain document explaining the veto reason |

### Example Payload
```json
{
  "proposal_id": 7,
  "admin": "GBADMIN...",
  "reason_hash": "dedededededededededededededededededededededededededededededededed"
}
```

---

## Partially Implemented Events

### ContractPaused
The contract code references `env.events().publish_event(&ContractPaused { timestamp: env.ledger().timestamp() })` inside the `pause` function, but the actual `ContractPaused` struct is currently missing from the codebase. Note: This implementation is incomplete.

### ContractUnpaused
Similar to `ContractPaused`, the `unpause` function attempts to emit `ContractUnpaused`, but the struct definition is currently missing from the codebase. Note: This implementation is incomplete.

---

## Missing Events
The following events were referenced by issue requirements but are **not currently emitted** anywhere in the contract logic:

* **InvoiceExpired**: Not emitted (reverting logic may be handled implicitly, but no explicit event exists).
* **InvoiceDisputed**: Partially substituted by `InvoiceDefaulted` and `DefaultAppealed`, but there is no explicit `InvoiceDisputed` event.
* **ReputationUpdated**: Not emitted by the protocol.
* **TokenAdded**: Not emitted when supported tokens are added to the whitelist.
* **TokenRemoved**: Not emitted when supported tokens are removed.
* **LPPositionTransferred**: Missing. (The protocol supports `InvoiceTransferred` for the freelancer, but lacks LP position transfer events).

---

## Indexer Notes

* **Event Ordering Assumptions**: `InvoiceSubmitted` always precedes `InvoiceFunded`, which precedes `InvoicePaid` or `InvoiceDefaulted`.
* **State Reconstruction**: The `InvoiceSubmitted` event contains the `timestamp` field specifically to help indexers reconstruct the exact creation ledger time without querying the ledger headers directly.
* **Timestamps**: All timestamps are provided in `u64` format representing seconds since the Unix epoch. Indexers should safely cast these to date objects.
* **Amounts**: All numerical amounts (`amount`, `fund_amount`, `lp_payout`, etc.) are represented as `i128` units (including decimals). Depending on the token (e.g., USDC), these must be formatted to display the correct decimal values on the frontend.