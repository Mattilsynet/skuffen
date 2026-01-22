# Risk Analysis Protocol

This document defines the rules for analyzing the project's roadmap and plan to identify potential risks.

## Analysis Checklist

When reviewing `roadmap.md` and `plan.md`, or proposing new architectural changes, the following questions MUST be answered:

### 1. Security & Authentication
- [ ] **Is the auth situation properly addressed?**
    - Are strict authentication and authorization boundaries maintained?
    - Are secrets handled securely (e.g., using `secrecy` crate, not logged)?

### 2. Data Integrity
- [ ] **Is there a potential for important data loss somewhere?**
    - Do schema changes preserve existing data?
    - Are there race conditions or lack of transactions in critical paths?
    - Is the idempotency of operations considered?

### 3. Architecture & Maintainability
- [ ] **Are there architecture architecture decisions that will cause problems down the line?**
    - Does this introduce tight coupling between independent modules?
    - Does this violate the "Norwenglish" separation (domain logic vs technical details)?
    - Is it consistent with the existing architecture guidelines?

### 4. Supply Chain & Dependencies
- [ ] **Does this minimalize the attack surface?**
    - Are we avoiding unnecessary heavyweight dependencies?

## Risk Log
*(Record identified risks here)*
