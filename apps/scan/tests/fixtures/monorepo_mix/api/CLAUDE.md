@.claude/scan-map.md

# Api

> Parent: [../../../../../../CLAUDE.md](../../../../../../CLAUDE.md) | Orchestrator: [../../../../../../.claude/CLAUDE.md](../../../../../../.claude/CLAUDE.md)

## Guards

<!-- mustard:guards -->
<!-- facts: kind=dotnet; frameworks=(none) -->
[critical] never Invoice in apps/scan/tests/fixtures/monorepo_mix/api/**
This directory is a frozen scan test fixture, not an application: `monorepo_strata_each_keep_a_sample_slot` in `apps/scan/tests/stratified_samples.rs` asserts the byte-exact digest sample order including `api/Billing/InvoiceService.cs` — any rename, move, or new `.cs` file here must update that test in the same change.
`InvoiceService.cs` is calibrated to LOSE on pure relevance (a single "invoice" subtoken diluted by `LoadTotals`/`ComputeBalance`/`SyncBook`) so only the stratum guarantee hands it a sample slot — do not add, rename, or "clean up" its declarations, and never add a second invoice-bearing name to this stratum.
Keep `Api.csproj` a bare `Microsoft.NET.Sdk` stub with no `PackageReference` and no extra properties — its only job is to make scan classify `api/` as its own dotnet stratum; real dependencies would change the deterministic facts this fixture exists to pin.
Never run `dotnet build`/`dotnet restore` in this directory: generated `obj/` sources (e.g. `*.AssemblyAttributes.cs`) would be swept up by scan and silently change the stratum's code-file count and ranking.
<!-- /mustard:guards -->
