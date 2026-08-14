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

## 5. Incremental cache on/off — the third refutation

One line appended to `apps/rt/src/shared/branch_state.rs`, rebuilt with
`cargo build -p mustard-rt`, each configuration warmed with its own full build
first so neither pays the other's cold start. The file was restored byte-identical
afterwards (verified).

| Configuration | One-line rebuild |
|---|---|
| `CARGO_INCREMENTAL=1` | **8,6s** |
| `CARGO_INCREMENTAL=0` | **29,7s** |

**The cache pays for itself, 3,5× over.** The proposal to turn it off — mine, and
the one the unit was half-expecting to adopt — is refuted. Turning it off would
have made the edit-rebuild loop three and a half times slower while everyone
congratulated themselves on the reclaimed disk.

The price is real and now quantified: after these two runs alone,
`target/debug/incremental` held **4,53 GB across 15 165 files**. For context, it had
reached 52,28 GB before being deleted by hand that morning — out of a `target/` of
85,16 GB and a checkout of 102,75 GB.

**So the policy wave 3 must declare is NOT "off".** It is "on, and pruned": keep the
3,5× on every rebuild, and bound the directory so it cannot silently reach 52 GB
again. Note this cuts against the shape the spec's own Files table anticipated —
follow the number, not the anticipation.

## 6. Link cost of the 59 test binaries — measured, and it exonerates them

Method: one line appended to a source file of the crate, then the SAME edit rebuilt
twice — once as the library alone, once as the library plus its test binaries. The
difference is the binaries' own cost, with the library's recompilation cancelled out.

(A first attempt measured warm `--no-run` no-ops and produced 0,3–0,5s across every
crate. That measured nothing: a no-op neither compiles nor links. Recorded here
because the numbers looked plausible and were meaningless.)

| Crate | test binaries | library alone | library + tests | difference | per binary |
|---|---|---|---|---|---|
| `mustard-rt` | 32 | 12,5s | 24,9s | **+12,4s** | 0,39s |
| `mustard-core` | 5 | 3,3s | 5,0s | **+1,7s** | 0,34s |

The per-binary cost is consistent across a crate with 32 and one with 5, so it
scales linearly: about **0,37s per test binary**. For all 59, roughly **22s** of link
on any local rebuild that touches a shared library.

**And that exonerates them for CI.** The Windows Test step costs 729s. Even
multiplying this link figure several times over for a cold runner, linking accounts
for well under a fifth of it. The remainder is the tests RUNNING: 3051 of them, many
spawning `git` into temporary repositories. Process creation is the expensive
operation on Windows, and it is what the 6,4× is made of.

So merging the 59 files into fewer — the repair the Non-Goals set aside and that §7's
attribution seemed to reopen — would **not** fix CI. It would buy back part of a
22s local cost and leave the 729s almost untouched. The Non-Goal stands, now for a
measured reason rather than a cautious one.

## 7. CI — measured, from the run itself

PR #155, run `31796118965`, same commit, same steps, `Swatinem/rust-cache@v2` on
all three runners:

| Runner | Duration |
|---|---|
| ubuntu-latest | 5m46s |
| macos-latest | 6m29s |
| **windows-latest** | **19m18s** |

Windows costs **3,3×** Linux for identical work. Per-step attribution, from the
run's own job API:

| Step | ubuntu | macos | **windows** | win ÷ ubuntu |
|---|---|---|---|---|
| Build | 161s | 138s | **306s** | 1,9× |
| **Test** | 114s | 181s | **729s** | **6,4×** |
| Clippy | 57s | 45s | **89s** | 1,6× |
| cache restore + post | 8s | 12s | 17s | — |

**This refutes the premise wave 2 was built on.** The unit was planned around "CI
compiles the workspace three times, so collapse the invocations". The attribution
says the redundant third compilation — Clippy, the one that cannot reuse artefacts
because it drives through `clippy-driver` — costs **89s of 1124s: 8%**. Collapsing
it is worth having and is nowhere near the money.

The money is the **Test step: 729s of 1124s, 65% of the Windows job**, and it is the
only step whose platform penalty is extreme (6,4× against Linux, where Build and
Clippy sit near 2×). Two things happen in that step and the totals cannot separate
them: compiling and linking 59 test binaries, and then RUNNING 3051 tests, many of
which spawn `git` into temporary repositories. Process creation is the classic
Windows penalty, and 6,4× is the shape of it.

Note what this does to the unit's own Non-Goals: restructuring the test files was
excluded on the grounds that the trade needed a measurement first. This is that
measurement, and it points straight at the excluded ground.

## 8. The Test step's real cost: the suite ran TWICE

Ranking the 35 test binaries of a full `cargo test -p mustard-rt` by execution
time made the answer obvious:

| Target | Tests | Execution |
|---|---|---|
| `unittests src/main.rs` | 1973 | **187,6s** |
| `unittests src/lib.rs` | 1968 | 150,8s |
| `tests/spec_invariants.rs` | 1 | 19,8s |
| everything else (32 binaries) | ~90 | ~39s |
| **total** | | **397,7s** |

The top two are the SAME assertions. `apps/rt` declares both a `[lib]` and a
`[[bin]]`, and `main.rs` declares the same seven modules `lib.rs` does instead of
consuming the library — so every `#[cfg(test)]` block under `src/` was compiled
and executed once per target. The 5-test difference (1973 against 1968) was the
binary's own: the harness-response shape for each hook event.

**Repair.** `test = false` on the `[[bin]]`, with those five tests and the pure
function they exercise (`hook_specific_output`) moved to `src/hook_output.rs` — a
module the library declares too, so they still run, once.

| | before | after |
|---|---|---|
| test binaries | 35 | 34 |
| tests counted | 4033 | 2065 |
| **unique tests** | 2065 | **2065** |
| **execution** | 397,7s | **214,7s** |

**183s saved locally, 46% of execution time**, with no test lost — the drop from
4033 to 2065 is exactly the duplicate.

### What it was actually worth in CI — the fifth correction

Measured on run `31816628632` (PR #156), against `31796118965` (PR #155):

| Runner | before | after | gain |
|---|---|---|---|
| ubuntu | 5m46s | 5m0s | −13% |
| macos | 6m29s | 5m49s | −10% |
| **windows** | **19m18s** | **15m30s** | **−20%** |

Per step on Windows: `Build` 306s + `Test` 729s = 1035s became a single `Test` of
792s — 243s off the pair. (The Test step alone rose, 729s → 792s, because it now
absorbs the compilation the removed Build step was doing. The pair is the number.)

**The local figure did not transfer, and the reason matters.** 46% was measured
WARM, where execution dominates. A CI runner is always COLD, so its Test step is
dominated by COMPILING, and the duplicate removed here was execution. Predicting
CI from a warm local measurement overstated the gain by roughly half.

Real, and worth having: 3m48s off every Windows CI round. But the honest headline
is 20%, not 46%, and the two numbers measure different things.

Worth naming: this had nothing to do with Windows, with the antivirus, with the
cache, or with the 59 binaries. Four measurements were spent on those before the
one that ranked the binaries by time — which was cheap, and should have been
first.

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
