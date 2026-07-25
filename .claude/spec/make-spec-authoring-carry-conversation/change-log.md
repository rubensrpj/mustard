# Change Log — make-spec-authoring-carry-conversation

_Solicitações registradas automaticamente durante o pipeline (mid-spec). O `spec.md` (narrativa congelada) NÃO é alterado; dobre o que muda comportamento em `## Acceptance Criteria` e rode o QA de novo._

- **2026-07-25T11:47:42.002Z** _(Plan)_ — Será quebrado em spec ou ondas?
- **2026-07-25T11:50:20.790Z** _(Plan)_ — Sim
- **2026-07-25T11:56:38.398Z** _(Plan)_ — Pergunta final iremos ganhar benefícios e como está a injeção de memórias?
- **2026-07-25T12:02:01.653Z** _(Plan)_ — Mas só pode incluir o que vale a pena, durante a confecção da spec poderia ler a memória e adicionar algo relevante na spec com base de algo da memória. Memória entre ondas e agentes só de pontos que ocorreram dentro do processo isso será usado para orientar as ondas.
- **2026-07-25T12:14:50.294Z** _(Plan)_ — Sim
- **2026-07-25T12:17:17.002Z** _(Plan)_ — Sim
- **2026-07-25T13:49:41.430Z** _(Execute)_ — Concordo se for agregar

## Change request — wave 7 drift guard (2026-07-25)

**Requested:** wave 7 must also add a drift guard for the agent-prompt reference.

**Scope, deliberately narrowed:** assert that every placeholder the renderer
actually substitutes appears in the reference's placeholder table — a set
assertion. Do NOT assert the prose sentence's stated count: a reworded sentence
would break the test for a cosmetic reason, which is the false-red this spec
exists to prevent. The count becomes a consequence of the set, not the claim.

**Why:** the drift is present right now (the reference says 12 placeholders; the
renderer substitutes 13), and wave 5 flagged that no test enforces any of it, so
it cannot fail loudly. The isolation spec already shipped an equivalent guard
that locates its claim by shape rather than line number — reuse that discipline.
- **2026-07-25T13:52:00.000Z** _(Execute)_ — Wave 7 must ALSO add a drift guard for the agent-prompt reference, scoped deliberately: assert that every placeholder the renderer actually substitutes appears in the reference's placeholder table — a SET assertion. Do NOT assert the prose sentence's stated count: a reworded sentence would break the test for a cosmetic reason, which is the false-red this spec exists to prevent, so the count is a consequence of the set and never the claim. The drift is present right now — the reference says 12 placeholders while the renderer substitutes 13. Model it on the guard the isolation spec already shipped (`agent_prompt_ref_matches_subagent_map`), which locates its claim by SHAPE rather than by line number so the prose can be reworded around it.
- **2026-07-25T14:06:02.292Z** _(Execute)_ — Mas precisa deixar isso perfeito deixar é opção?
