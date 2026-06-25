# Purpose enrich — give each logic declaration a one-sentence meaning so the digest can find it

Loaded on demand when a scan runs `enrich-purpose`. **Single source of truth — reference this file, never copy the prose into a SKILL.**

## Why
The scan indexes declaration NAMES; it cannot know what a method DOES. When the user's query vocabulary diverges from the identifier (PT "efetivar" vs EN `EffectivateAsync`, or a synonym within one language), name-match never surfaces the right method — and no judge can re-rank what was never retrieved. Measured on real code: name-match recall **0/10** on cross-lingual holes; a one-sentence `purpose` summary derived from the BODY lifts lexical recall to **7-8/10**. The meaning lives only in the body, so a model must read it.

## Invariant — AI lives in the orchestration, never in the binary
`enrich-purpose --render` and `enrich-purpose --apply` make **zero** model/network calls. The binary RENDERS a deterministic batch prompt and APPLIES the summaries fed back to it; the orchestrator dispatches the model. Mirrors `lexicon-enrich` (render→apply) and `digest-validate-render`.

## The language is the project's, not hardcoded
The `purpose` is written in the language from `mustard.json` root `language` (`ProjectConfig`, single source). A pt-BR project gets pt-BR summaries (bridges pt queries to en code); an en project gets en summaries (bridges en synonyms). The render prompt is parameterized by `config.language` — never a hardcoded language. Carve-out: code/logs/schema are untouched.

## Steps (run after the scan builds the grain model)
1. **Render the batch (deterministic).** `mustard-rt run enrich-purpose --render --model .claude/grain.model.json`. It iterates declarations with `kind ∈ {method, function}` and a non-trivial body (skips property/const/type/field/enum and trivial accessors), RE-READS each source file and slices the body by `line` + brace-balance, and emits a byte-stable prompt asking for ONE sentence per declaration in the config language. Each item carries a stable id `path#name#line`. Empty stdout (nothing to summarize) → skip.
2. **Dispatch the model (cheap, batched).** Send the rendered prompt to a cheap model (default Haiku). It returns a JSON array `[{"id": "path#name#line", "purpose": "<one sentence>"}]`. It documents what the body DOES — it must not translate the English name.
3. **Apply (deterministic, incremental).** Write the summaries to a temp file and run `mustard-rt run enrich-purpose --apply <file>`. It writes `purpose` + a `body_hash` into the model's declarations via atomic write. A declaration whose current body hash equals the stored one is SKIPPED — re-runs only re-summarize changed bodies (incremental, cheap).

## What it changes at query time — a DECOUPLED fallback, not the anchor ranking
The digest's name-based anchor pipeline is left UNTOUCHED (threading purpose into its BM25F/cap ranking proved fragile — weak name matches evict the purpose anchor). Instead, purpose recall is a separate deterministic command the orchestrator runs ONLY on a miss — when the digest-validate judge reports `centralFound=false` (the central concept was not found by name):

`mustard-rt run purpose-search --intent "<missed terms>" --model .claude/grain.model.json`

It does an UNCAPPED lexical lookup: every query token is matched against every declaration's `purpose` through the scan match ladder WITH the trigram rescue rung (the PT Snowball stemmer has gaps on verb forms — `efetivar`↔`efetiva`, `baixa`↔`baixado` need trigram, not just stem), and returns the files ranked by how many distinct query concepts each one's purpose bridges. No model, no embedding — query-time stays 100% deterministic; the orchestrator reads the returned files. Because it is uncapped and outside the term ranking, a rare method name (exactly the recall target) is never lost to the digest's MAX_TERMS / anchor caps.

## Cost (one-time + incremental)
Only logic methods need summaries (≈¼ of all declarations). A repo the size of sialia (~6.8k logic methods) costs **~$5 of Haiku once**, **~$0.10/commit** incremental, **+~1 MB** in the grain model. The deterministic scan and the binary stay AI-free.
