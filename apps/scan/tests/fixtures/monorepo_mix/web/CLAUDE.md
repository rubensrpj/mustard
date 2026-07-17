@.claude/scan-map.md

# Web

> Parent: [../../../../../../CLAUDE.md](../../../../../../CLAUDE.md) | Orchestrator: [../../../../../../.claude/CLAUDE.md](../../../../../../.claude/CLAUDE.md)

## Guards

<!-- mustard:guards -->
<!-- facts: kind=npm; frameworks=(none) -->
[critical] never `import` in `src/**/*.ts`
This directory is scan test data, not an app: `monorepo_strata_each_keep_a_sample_slot` in `apps/scan/tests/stratified_samples.rs` asserts the exact sample order (`invoice_card.ts` → api's `InvoiceService.cs` → `invoice_list.ts`) — adding, removing, or renaming a file here breaks a byte-stable assertion.
Keep each module to exactly one empty `export class Invoice<X> {}` — the three "focused" single-declaration modules are what lets the web stratum win the global BM25 slots while api's longer `InvoiceService.cs` needs the stratification guarantee; any extra declaration, method, or import retunes that balance.
The `invoice` vocabulary overlap with `api/Billing/InvoiceService.cs` is the point of the fixture (two strata sharing one term) — do not "improve" the naming to something more distinctive.
`package.json` stays `"private": true` with zero dependencies: it exists only so scan detects the `web` npm stratum (`projects[].dir == "web"`); a real dependency would change the deterministic scan facts the test premise relies on.
<!-- /mustard:guards -->
