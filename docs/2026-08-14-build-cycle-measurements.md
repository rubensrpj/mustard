# Build and test cycle — measured

Wave 1 of `build-test-cycle-is-too`. Every number here was timed on this machine
with `Measure-Command`, or read from a GitHub Actions run. Nothing is estimated.

**Machine**: Windows 11, the operator's daily checkout (`C:\Atiz\mustard`).
**Date**: 2026-08-14. **Toolchain**: the workspace's pinned stable.

> STATUS: in progress. The target-switching experiment (§4) was still running when
> this file was first written; §5 and §6 are not measured yet and say so. No row
> below is a placeholder — a row exists only if it was timed.

## 1. The warm cycle — what a no-change round trip costs

Measured back to back, cache warm, nothing edited between them:

| Step | Duration |
|---|---|
| `cargo build --workspace` | 23,6s |
| `cargo test --workspace --no-run` (compiles the 59 test binaries) | 55,9s |
| `cargo clippy --workspace` | 33,5s |
| **total** | **1m53s** |

That is the cost of confirming that *nothing changed*.

## 2. The no-op, repeated — the first refutation

The 23,6s above invited the obvious explanation: NTFS plus antivirus, thousands of
files stat-ed one by one. **That explanation is wrong**, and the repeat run is what
killed it:

| Run | Duration |
|---|---|
| `cargo build --workspace` — 1st repeat | 0,6s |
| — 2nd repeat | 0,4s |
| — 3rd repeat | 0,3s |

A warm no-op costs **three tenths of a second**. The filesystem is not the problem.
Whatever cost 23,6s was not the walk over the tree.

## 3. Per-crate no-op — the cost is not proportional to the code

| Target | Files in crate | Duration |
|---|---|---|
| `cargo build -p mustard-mcp` | 2 | **19,9s** |
| `cargo build -p mustard-rt` | 285 | 12,8s |

The **smaller** crate costs **more**. Compilation volume does not explain this.
What both have in common is that each was invoked right after a *different* target
selection.

## 4. Hypothesis under test: switching targets invalidates the cache

`cargo build --workspace` resolves feature flags across every workspace member at
once; `cargo build -p mustard-mcp` resolves them for that crate alone. Different
feature sets produce different artefacts, so each invocation can discard what the
previous one built. The 23,6s in §1 followed a `cargo install --path apps/rt`,
which is a third selection (and a release profile besides).

The falsifiable prediction: `--workspace` right after a `-p` run is SLOW, an
immediate repeat is FAST, and going back to `-p` is slow again.

| Step | Prediction | Measured |
|---|---|---|
| A. `--workspace` right after the `-p` runs | slow | **5,5s** |
| B. `--workspace` immediately again | fast | 0,3s |
| C. back to `-p mustard-mcp` | slow (≈20s) | **1,2s** |
| D. `-p mustard-mcp` immediately again | fast | 0,2s |
| E. the CI's exact 4-crate selection | — | 0,4s |
| F. the same selection repeated | fast | 0,5s |

**Largely refuted.** Target switching is real but CHEAP: 5,5s at worst (A), and the
return to `-p` cost 1,2s where the hypothesis predicted ~20s (C). It does not
explain §1 or §3.

**The corrected explanation.** The 23,6s and 19,9s were not invalidation — they were
crates being compiled for the FIRST time in this tree. `apps/mcp` and `apps/cli` had
not been built in debug since the incremental cache was deleted that morning
(`cargo test -p mustard-rt` does not reach them, and the two `cargo install` runs
before it were release-profile). Once each target has been built once, every
selection is sub-second, including the CI's exact four-crate one (E/F).

## 5. Incremental cache on/off — NOT MEASURED YET

The datum that decides wave 3's policy: a one-line-change rebuild with
`CARGO_INCREMENTAL=1` against `CARGO_INCREMENTAL=0`, recording both wall clock and
the bytes `target/debug/incremental` gains each way.

Context for it: `target/` measured 85,16 GB on this machine, of which 52,28 GB were
`target/debug/incremental` and 24,99 GB `target/debug/deps`. The whole checkout was
102,75 GB. The 52,28 GB were deleted by hand on 2026-08-14; nothing in the
repository prevents them from accumulating again.

## 6. Link cost of the 59 test binaries — NOT MEASURED YET

`cargo test --no-run` per crate, to separate compiling the crates from linking the
integration-test binaries: `mustard-core` has 5 test files, `apps/cli` 1,
`apps/scan` 21 and `apps/rt` 32 — 59 executables, each linked in full.

The §1 figure (55,9s warm) bounds the total but does not attribute it.

## 7. CI — measured, from the run itself

PR #155, run `31796118965`, same commit, same steps, `Swatinem/rust-cache@v2` on
all three runners:

| Runner | Duration |
|---|---|
| ubuntu-latest | 5m46s |
| macos-latest | 6m29s |
| **windows-latest** | **19m18s** |

Windows costs **3,3×** Linux for identical work. Per-step attribution is still to be
pulled; the job totals above are what the run reports.

The workflow invokes cargo three times per runner — `build`, then `test`, then
`clippy` (`.github/workflows/ci.yml:48,56,62`). Clippy drives compilation through
`clippy-driver` rather than `rustc`, so it cannot reuse the earlier artefacts. The
§1 warm figures put a floor under what that costs, but the CI runs cold.

## What this record rules out, and where the cost actually is

Three explanations were proposed and measured. Two are dead:

- **The filesystem is not the bottleneck.** §2 refutes it: a warm no-op is 0,3s.
- **Target switching is not the bottleneck.** §4 refutes it: 5,5s at worst, 1,2s in
  the case predicted to cost twenty.

Both were plausible, both were mine, and both are wrong. Recording them is the point:
the next reader who proposes "it's the antivirus" can read §2 instead of spending an
afternoon on it.

What the measurements leave standing:

1. **The warm local loop is NOT slow.** Once every target has been built once, a
   no-op is sub-second and even the CI's selection is 0,4s. Any repair aimed at the
   warm loop would be optimising something that already costs nothing.
2. **The local cost that IS real is the test step**: 55,9s warm for
   `cargo test --no-run`, against 0,3s for the build alone. Compiling and linking 59
   integration-test binaries is where local time goes — §6 still has to attribute how
   much of it is link.
3. **CI is the expensive half, and it runs cold every time.** 19m18s on Windows
   against 5m46s on Linux, with the workspace compiled three separate times per
   runner. None of the warm-loop findings transfer there: a CI runner never gets to
   be warm, which is exactly why the three-invocation waste is paid in full.

**Consequence for the plan.** Wave 2 (CI) keeps its target and gains a sharper one —
the waste is cold compilation repeated three times, not anything the local numbers
touch. Wave 3 (the incremental policy) has NOT yet been given its datum: §5 is the
measurement that decides it, and until it exists the policy would be a guess. The
59-binary link cost (§6) was declared a non-goal for this unit, but item 2 above is
the evidence a future unit would need to reopen it.
