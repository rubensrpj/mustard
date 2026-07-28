# Change Log — make-harness-stop-asserting-what

_Solicitações registradas automaticamente durante o pipeline (mid-spec). O `spec.md` (narrativa congelada) NÃO é alterado; dobre o que muda comportamento em `## Acceptance Criteria` e rode o QA de novo._

- **2026-07-28T00:00:19.878Z** _(Plan)_ — execute agora mustard:spec
- **2026-07-28T00:03:42.326Z** _(Plan)_ — segue
- **2026-07-28T00:29:05.850Z** _(Execute)_ — **Instruction:** Wave 1 published the confirmation half of the criterion proof as 'ac-negative-check --confirm', but no prose instructs it: plugin/refs/spec/resume-loop.md and plugin/pipeline-config.md still describe only the red half. Teach the confirm pass where the EXECUTE and CLOSE prose already teaches the red one, so a wave closes by taking the confirmation instead of leaving the flag undiscovered.
- **2026-07-28T00:29:24.609Z** _(Execute)_ — **Instruction:** Wave 2 added an 'Onde' column to the active-specs table (where each spec lives: current checkout or the branch that carries it), but plugin/commands/spec.md section 2 spells the picker's Siglas out literally and still describes the old columns. Add the Onde legend line there so the picker prose matches what the command now prints.
- **2026-07-28T00:32:42.962Z** _(Execute)_ — b, registra e segue com a rodada 2
- **2026-07-28T01:02:05.826Z** _(Execute)_ — **Instruction:** Wave 4 emits a new 'neverDispatched' field on resume-bootstrap, but plugin/refs/spec/resume-loop.md still instructs the orchestrator to read currentWave and never mentions it. The field is emitted and nothing tells anyone to read it — same gap as the --confirm flag from wave 1. Teach both in the same pass over the plugin prose.
