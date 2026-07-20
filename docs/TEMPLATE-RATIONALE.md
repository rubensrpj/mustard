# Template Rationale

> The WHY behind the rules in `CLAUDE.md`, `pipeline-config.md`, the SKILLs and the refs. This file is NEVER loaded into a session — it exists for maintainers. The loaded templates carry only the law and the how; when you need to know why a rule exists (or want to change one), look here. Enforced leanness: `apps/cli/tests/template_budget.rs` holds the two limits Claude Code actually publishes — a command/skill `description` ≤ 1,536 chars (truncated mid-sentence in the skill listing past that) and an injectable ≤ 9,500 chars (the `additionalContext` truncation ceiling). It no longer gates command-BODY length by word count (Claude Code doesn't — the doctrine is progressive disclosure). The 2026-07-07 audit's leanness (42k words, emphasis inflation destroying signal) is now kept by STRUCTURE — a lean body with `refs/` opening on demand — plus review, not an arbitrary word tripwire.

## Routing (CLAUDE.md)

- Single door: users describe intent; the router classifies. Command-picking was removed because users chose ceremony-heavy flows for small work. The narration step exists so the user can interrupt a misclassification before anything runs.
- Routing economy: the pipeline's fixed ceremony cost is re-paid as harness context every turn; measured token economics are NEGATIVE for small Light runs — the full pipeline must justify itself (≥2 layers or a new entity). The single most expensive routing error is a wrong "full" on a small task.
- Auto-branch: `emit-pipeline` seeds `{base}_{slug}`; the gate cuts it on the FIRST edit so read-only requests never litter branches. The prefix records the PR target because sessions end and branch names survive. Bases derive from `git.flow` — a field case (sialia, empty flow) showed the fallback silently protects only main/master; the doctor `git-flow` check exists for that.

## Delegation + efficiency (CLAUDE.md)

- Parent-context bloat degrades hooks (forced retries), hence the ≥50% delegation health metric and the "never pay twice" rules. Re-running deterministic commands re-computes AND re-floods context — capture once, slice the file.
- The Verdict rule exists because Explore agents confabulated absence in field runs ("origin not located" vs "does not exist" — a runtime symptom is irrefutable by static reading).

## Locating code (CLAUDE.md, locating-code.md)

- Literal → grep, concept → digest. The per-prompt entry-point guesser was REMOVED 2026-07-07 after measuring 1 useful suggestion in 17 (prompt words are problem vocabulary; path tokens are code vocabulary — lexical overlap is coincidence). Location is on-demand work by a skilled operator, not an ambient guess.
- The digest is deterministic (BM25 over the mined model) and refuses to guess on weak vocabulary overlap — silence beats confident noise. The PT→EN gap is bridged by the ORCHESTRATOR's translation (the semantic/embedding layer was cut 2026-07-07: 12.5k lines whose recall stayed idle in the field and whose summary quality was the bottleneck).

## Spec language (spec-language.md)

- BCP-47 only, resolved by cascade, asked at most once: "mustard 100% agnostic" forbids textual language heuristics.
- Code stays English regardless of locale because grain mines identifiers/doc-comments into the term index the digest queries — mixed-language identifiers fragment retrieval.
- Contexto is a briefing, not agent input. Bad shape (real case): citing commit hashes, `UserTenant`, "DB-level" — assumes module knowledge, reads as compressed synthesis. Good shape (real case): explains *tenant* on first use, states the impact in user/business terms ("o mesmo email aparece em duas linhas"), no line numbers. A reader must answer "what's broken and why does it matter?" from Contexto alone.
- Component Contract exists because FE agents improvise variants/states/a11y without an explicit contract ("AI-look": literal colors, missing empty states). It is UI-only because on backend specs it is pure bloat.

## Pipeline execution

(The `pipeline-execution` SKILL was deleted 2026-07-18: `disable-model-invocation: true` kept it out of every context and nothing Read/Skill-loaded it — an unreachable file is not a law. Its surviving laws live in the render's `{role_block}` (diff-obeys-Guards/molds) and `commands/close.md` (capability authoring); the rationale stays here.)

- Guards/molds are law for the DIFF (shape, not behavior) because "works" is not the bar — parallel conventions rot a codebase into many codebases. Divergence is the owner's call: agents flag, never impose.
- The spec-memory relevance gate existed because irrelevant injected principles measurably degrade subagent reasoning (distractors compound with depth). The Haiku-pinned judge was RETIRED with the skill — the deterministic recall matcher (the former fallback) is the definitive selector; RT stays LLM-free and byte-stable.
- `wave-advance` returns rendered rounds because hand-assembled prompts and hand-picked agents caused drift; the orchestrator is a relay. The `MUSTARD-PROMPT-REF` stub exists so the full prompt never transits (and re-bills) the parent context.
- Review's 7 categories with Guards/molds as CRITICAL: field evals showed style-note demotion let violations ship.

## Rationalization tables (removed from the loaded files)

The excuse/answer tables ("it's just one line", "tests pass so quality is covered", "I'll align with the pattern later") were superpowers-style pre-emption. They are preserved here for authoring reference; the loaded files keep a single red-flags line per flow because the GATES (`scope_guard`, close-gate, checklist-gate, QA-gate) are what actually stop those moves — prose only needs to name the trap, not litigate it.

## Digest contract details (feature SKILL)

- "Read stdout once / never re-run / read the detail file sliced": each re-run recomputes the whole digest and re-floods the parent — a field-measured cost.
- Provenance pruning before reading: anchorsDetail carries the matched terms per anchor precisely so tangential anchors (a seeder matching `pagos`) can be dropped with zero reads.
- Slices lead on composition (field-confirmed repeatedly): a slice names the pattern to MIRROR, which a flat declaration-site anchor cannot.
- Existence/duplication is settled by Grep enumeration because sampled reading (digest anchors) never proves absence.

## Close chain (pipeline-config)

- The gate vector runs in-process and auto-finalizes on pass because the LLM skipping `complete-spec` (or calling it wrong) was a recurring field failure; report-only on fail keeps the human in the loop.
- Tactical fixes become linked sub-specs (never silent follow-ups or mid-EXECUTE waves) because the parent spec is frozen at approve — SDD purity; the ≤100-LOC/no-contract-change bounds keep the lane honest.
