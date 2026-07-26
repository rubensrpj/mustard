# Re-review — isolate-each-wave-s-implementer (post-fix)

## Verdict: rejected — 1 critical

The three previous findings are fixed and verified. All nine ACs pass as written; full crate green; clippy clean; all four registrations present; `run` output ordered and relative. `isolation: worktree` confirmed a real frontmatter key against the official docs, not taken on trust.

## CRITICAL — the whole way OUT keys on an unverified name prefix
Both the reclaim candidate sweep and the clean-tree precondition gate on the literal worktree-name prefix `agent-`. That prefix could not be confirmed anywhere:

- The official `WorktreeCreate` input schema describes `name` as a slug identifier, user-specified or auto-generated, with an example of the `bold-oak-a3f2` shape. Five doc routes were fetched and none contains an `agent-` prefix.
- The only auto-generated worktree name this project's own telemetry ever recorded is of that slug shape. The single `agent-` name on disk is this wave's own test fixture.
- The spec presents the prefix as a fact verified in code; what exists in code is an older doc comment asserting the same thing.

Consequence under a slug name — the documented shape: the cut still comes from the unit HEAD, so isolation turns ON, but the candidate sweep finds nothing, reclaim answers `ok:true nothing-to-reclaim`, and the wave is reported complete with its commit stranded in an unreclaimed checkout. That is a fail-open on the spec's own zero-metric, and the dirty-tree refusal never fires either. Every test proving those criteria hard-codes the fixture name, so nothing exercises the real naming.

The prefix is also asymmetric with the cut's own cascade, which correctly keys on the ABSENCE of an underscore rather than on a prefix.

## MAJOR — the switch silently retires the prompt-size gate
Routing writing roles to the plugin implementer makes the budget gate's role classifier return Unknown, because it matches the literal built-in name. The hard prompt-size block therefore no longer applies to any pipeline dispatch, and both general budgets became unreachable. The output budget deliberately folds Unknown into the impl budget; the prompt budget does not — the drift is asymmetric and unmentioned.

## MINOR
- The worktree parser emits an entry only when a branch line is present, so a DETACHED agent checkout is invisible to the sweep and also answers `ok:true`.
- The budget metric label for writing dispatches changes, and the dashboard aggregates on those strings.
