# /mustard:spec — Resume flow (continuar pipeline)

Loaded on demand by `commands/mustard/spec/SKILL.md` Step 7 quando a spec selecionada estiver em estágio EXEC (executar), ou quando o sufixo `r` saltar do approve-only-flow para cá após aprovar uma spec PLAN. Conteúdo movido **verbatim** da antiga `commands/mustard/resume/SKILL.md` (deletada na TF `2026-05-23-tf-unify-spec-command`).

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Description

Resume a pipeline interrompida. O main context (contexto principal) VIRA o Pipeline Runner — despacha agentes diretamente via Task tool. NUNCA delega o pipeline inteiro para um único Task agent intermediário.

## Action

### Step 0: Dispatch Failure Pre-Check (pré-check de falha de dispatch)

Antes do fluxo normal detect-and-confirm, escaneie o pipeline state mais recente em busca de uma falha de dispatch recente sinalizada por `subagent-tracker` (PostToolUse on Task).

1. Rode `mustard-rt run event-projections --view pipeline-state --spec {specName}` para carregar o derived state atual. Pegue a spec com o timestamp de evento mais recente se múltiplas estiverem ativas.
2. Inspecione o campo projetado `lastDispatchFailure` do event log.
3. Se presente:
   - Calcule `ageMs = Date.now() - new Date(lastDispatchFailure.at).getTime()`.
   - **Se ageMs <= 10 * 60 * 1000** (≤10 min, fresco):
     1. Informe o usuário: `Detected failed dispatch ({agentType}) due to {reason} at {at}. Re-dispatching with same prompt.`
     2. Re-invocar a Task tool com:
        - `subagent_type`: `lastDispatchFailure.agentType` (fallback: `general-purpose`)
        - `description`: `lastDispatchFailure.description`
        - `prompt`: `lastDispatchFailure.prompt`
     3. Após o re-dispatch retornar, emita o sinal de dispatch_failure cleared:
        ```bash
        mustard-rt run emit-pipeline --kind pipeline.dispatch_failure --spec {specName} --payload "{\"cleared\":true,\"at\":\"{ISO now}\"}"
        ```
     4. Caia através para Step 1 (fluxo normal de resume continua do state atualizado).
   - **Se ageMs > 10 * 60 * 1000** (stale): emita o sinal cleared silenciosamente, então continue para Step 1.
4. Se `lastDispatchFailure` estiver ausente, pule Step 0 inteiramente e siga para Step 0.5.

### Step 0.5: Resume Mode (continuar vs reanalisar)

Antes de carregar contexto pesado (sync-registry, diff-context, Explore Gate), pergunte ao usuário qual modo usar. Isso gateia aproximadamente 2-5k tokens por resume.

1. **Skip conditions — auto `reanalyze` (sem prompt):**
   - Step 0 acabou de re-despachar um agente falhado (caminho de recovery → sempre reanalyze próximo step)
   - `pipeline-state.lastDispatchFailure` estava presente e <10min (já tratado no Step 0)
   - Wave plan com `failedWaves.length > 0` (tratado na seção wave failure abaixo — força `reanalyze`)

1b. **Auto-continue conditions — auto `continued` (sem prompt):**
    - `pipeline-state.updatedAt` nos últimos 10 min AND `status === 'in_progress'` AND sem `lastDispatchFailure`. O usuário acabou de pausar ou reabrir a sessão; confie no state como source of truth.
    - Override env: `MUSTARD_RESUME_MODE=continued|reanalyzed|ask` deixa um usuário forçar um modo para a sessão inteira. `ask` restaura o prompt legado.

2. **Caso contrário (state >10min OR forçado via env=ask):** AskUserQuestion:
   - **"Continuar de onde parou (modo leve)"** → `mode = "continued"`: pule sync-registry (Step 2 #6), pule diff-context (a menos que transição de wave force), pule Pre-EXECUTE Existence Gate (Step 12b). Confie no pipeline-state como source of truth.
   - **"Reanalisar contexto (modo completo)"** → `mode = "reanalyzed"`: rode Step 2 inteiro (comportamento default, relê tudo).

3. **Record mode via event:** emita resume_mode para que steps downstream saibam em qual caminho estão:
   ```bash
   mustard-rt run emit-pipeline --kind pipeline.resume_mode --spec {specName} --payload "{\"mode\":\"continued\"}"
   ```
   (substitua `"reanalyzed"` quando esse modo for escolhido)

4. **Stale-context fallback (rede de segurança):** se um agente despachado em modo `continued` retornar erro indicando contexto stale (ex.: referencia arquivo faltante, falha boundary check, ou retorna `BLOCKED` com motivo citando registry desatualizado), escale automaticamente:
   - Atualize pipeline state: `resumeMode: "escalated-to-reanalyze"`, anexe a `resumeEscalations` array com `{at, reason}`
   - Re-rode Step 2 inteiro (sync-registry + diff-context)
   - Re-despache o agente falhado com contexto fresco
   - Fail-open: escalação nunca bloqueia, só upgrade para o caminho mais pesado

### Step 1: Detect & Confirm

A spec já foi escolhida pelo picker do `/mustard:spec` (Step 1 e Step 3 deste fluxo do antigo `/resume` foram substituídos pelo picker). Pular detecção; usar a spec selecionada e seu `{specName}` direto.

3. **Resolve operational spec file** (arquivo que o resto do resume opera):
   - Se o root file é `spec.md` → operational spec = esse arquivo (single-spec mode).
   - Se o root file é `wave-plan.md` → wave-plan mode:
     a. Leia pipeline state derivado do log de eventos SQLite (`mustard-rt run event-projections --view pipeline-state --spec {specName}`). Se presente com `isWavePlan: true` + `currentWave: N`, use esse state — pule para (c).
     b. **State missing → reconstruct inline** (sem roundtrip para approve; usuário só quer continuar):
        1. Rode `mustard-rt run wave-tree --spec-dir .claude/spec/{specName} --format json` e parse `waves[]` (cada uma com `{label, folder, status}`).
        2. **Truly fresh plan** — toda wave tem `status === "queued"` (nunca executou) → pare e instrua: `Wave plan isn't approved yet. Run /mustard:spec and pick the letter for {specName} first.`
        3. **Plan already in progress** — pelo menos uma wave tem `status !== "queued"` (prova que foi aprovado & começou, só o state file foi perdido):
           - Reconstrua o state emitindo eventos no SQLite:
             ```bash
             mustard-rt run emit-pipeline --kind pipeline.scope --spec {specName} --payload "{\"scope\":\"full\",\"is_wave_plan\":true,\"total_waves\":{waves.length}}"
             mustard-rt run emit-pipeline --kind pipeline.stage --spec {specName} --payload "{\"stage\":\"Execute\"}"
             ```
           - A projection `pipeline_state_for_spec` derivará `completedWaves`, `currentWave`, e `totalWaves` dos eventos wave.complete já no log — sem JSON file escrito.
           - Informe o usuário inline: `Reconstruí pipeline-state do wave-plan.md (W{completed} done, W{currentWave} next).`
     c. Com o state (carregado ou reconstruído), operational spec = resultado do Glob `.claude/spec/{specName}/wave-{currentWave}-*/spec.md` (um match esperado).
   - **3d. Stub Expansion (wave-plan mode apenas).** Por design, `/feature` expande wave-1 completamente e deixa waves N≥2 como skeletons (`### Stage: Plan` + `### Outcome: Active`, Title + 1-line summary). Quando o resume pega wave N≥2, o stub deve ser expandido inline — sem roundtrip:
     1. Leia as primeiras 30 linhas da operational spec. Trate como **stub** se `### Stage: Plan` AND `### Outcome: Active` AND nem `## Files` nem `## Tasks` heading estiverem presentes.
     2. Se não for stub → continue para step 4.
     3. Se for stub → expanda inline via `Task(Plan)` (single dispatch, `model: "opus"`):
        - Prompt inputs: linha desta wave em `wave-plan.md` (role, file list, deps, Rationale), spec da wave mais recentemente completada (continuidade de entity/pattern), `entity-registry.json` Grepped para entidades mencionadas no file list.
        - Required return: conteúdo completo da spec expandido para esta wave casando o template Full-scope (Stage: Plan, Outcome: Active, Summary, Entity Info, Files, Tasks per agent, Dependencies, Boundaries, Acceptance Criteria, Checklist). Nada mais.
        - On return: **Write** o conteúdo no operational spec file (substituir o skeleton), depois atualize o header dele para `### Stage: Execute`, `### Outcome: Active`, `### Checkpoint: {ISO now}`.
        - Informe usuário inline: `Expandi wave-{N} stub via Plan agent. Avançando para EXECUTE.`
     3b. **Wave size audit (avisativo).** Logo após a spec expandida ser escrita, rode `mustard-rt run wave-size-check --spec-dir .claude/spec/{specName}`. Se `action: "audited"` e a entry para a wave atual (`wave === currentWave`) tem `oversized: true`, imprima a linha de aviso `⚠ Wave {N} ({folder}) — {fileCount} arquivos, {layerCount} camada(s) — considere dividir ({reason})`, notando que a wave recém-expandida saiu grande. Isso é **avisativo** — NÃO bloqueie, NÃO re-plan automaticamente; continue para EXECUTE normalmente.
     4. Siga para step 4 com a spec agora expandida.
4. **Read entire operational spec** (single Read) — extraia header (Status/Phase/Checkpoint) + conte `[x]` vs `[ ]` + identifique agentes/waves dos headers `### {Agent} Agent (Wave {N})`

   4a. **Phase marker on ANALYZE resume:** se a `### Stage:` extraída é `Analyze` (ou legacy `### Phase: ANALYZE`), rode `mustard-rt run emit-pipeline --kind pipeline.stage --spec {specName} --payload "{\"stage\":\"Analyze\"}"`. Idempotente e fail-open. Pule para qualquer outro stage.
5. Carregue pipeline state derivado do log de eventos SQLite (`mustard-rt run event-projections --view pipeline-state --spec {specName}`) → leia para current wave + scope + `explorationSummary` + `decisions`. Opcionalmente enriqueça com harness view (fail-open). Valide integridade (confie no header da spec em mismatch).
6. **Present Handoff Summary** — compilado de pipeline state + spec + agent memory + git context.

→ Ver `../../../refs/resume/handoff-summary.md` para formato exato e regras de validação de integridade.

   6a. **Wave Tree:** Rode `mustard-rt run wave-tree --spec-dir .claude/spec/{specName}` e imprima inline. Fail-open.

7. **Auto-continue default.** Informe usuário inline: `"Continuando da próxima ação. Se quiser revisar a spec antes, interrompa e diga 'review'."` Então siga direto para Step 2 (Bootstrap). Só pergunte quando TODOS aplicam: scope=full AND currentWave==1 AND no completedWaves yet (fresh start, expensive to redo). Override env: `MUSTARD_RESUME_CONFIRM=always|never|fresh-only(default)`.

### Step 2: Bootstrap (after confirmation)

6. **AUTO-SYNC:** `mustard-rt run sync-registry`
   - **Skip if `resumeMode === "continued"`** (Step 0.5): registry é reusado da sessão anterior.
   - Sempre rode se `resumeMode === "reanalyzed"` ou `"escalated-to-reanalyze"`.

### Diff Context (automático)
Rode `mustard-rt run diff-context --subproject {subproject_path}` por subproject para capturar o estado git atual com escopo de cada subproject. Inclua o output específico do subproject no agent prompt como `{diff_context}` para que agentes vejam só mudanças relevantes ao seu escopo.

**Skip if `resumeMode === "continued"`** a menos que uma wave tenha completado (wave transitions sempre refresh diff). O snapshot prior é reusado de `.claude/.pipeline-states/{specName}.diff-{subproject}.md`.

### Context Slice (automático, snapshot por-wave)

Junto com o refresh do diff-context, produza o slice de glossário filtrado por relevância que preenche o placeholder `{context_md}` do agent-prompt template.

1. Localize o `CONTEXT.md` do projeto (construído pelo skill `grill-with-docs`); também passe arquivos sibling `CONTEXT.md` e um `CONTEXT-MAP.md` se presentes — `context-slice` aceita repetidas flags `--context` e expande um map.
2. Rode `mustard-rt run context-slice --context {CONTEXT.md} --spec {operational_spec} > .claude/.pipeline-states/{specName}.context-md.md`.
3. Preencha `{context_md}` em cada prompt de subagent com o conteúdo desse snapshot file.
4. **Graceful degrade:** sem `CONTEXT.md` → slice vazio → deixe `{context_md}` vazio. Nunca bloqueie o dispatch.

O slice é estável para o pipeline inteiro, então fica no bloco PREFIX-STABLE e cacheia entre dispatches. **Re-rode o snapshot só em uma transição de wave** (mesma cadência que o refresh do diff). **Skip if `resumeMode === "continued"`** a menos que uma wave tenha completado.

7. **Read** `.claude/pipeline-config.md`. Para `entity-registry.json`: use Grep para extrair APENAS o bloco da entity relevante (ex.: `"Contract":`), NUNCA leia o JSON inteiro
9. **Update spec header:** `### Stage: Execute`, `### Checkpoint: {ISO now}`
10. **Emit stage transition para Execute:**
    ```bash
    mustard-rt run emit-pipeline --kind pipeline.stage --spec {specName} --payload "{\"stage\":\"Execute\"}"
    ```
    Nenhum JSON file é escrito.
11. **TaskCreate** — 1 por pending agent (skip completed)

### Step 3: Execute — Wave System

**CRITICAL: Main context IS the Pipeline Runner. NEVER delegate to intermediate Task agent.**

11b. **Pre-EXECUTE Rewave Check** (skip se `pipeline-state.isWavePlan === true`): Rode `mustard-rt run exec-rewave-check --spec .claude/spec/{specName}/spec.md`. Parse JSON output. Se `action: "decomposed"`, a spec foi split em N waves — atualize `pipeline-state.isWavePlan: true, currentWave: 1` e siga usando a wave-1 spec (`wave-1-{role}/spec.md`). Se `action: "keep-single"` ou `"skip"`, continue com a spec original. Silencioso — sem AskUserQuestion.

12. **Match recipe by name only:** Grep `{subproject}/.claude/commands/recipes.md` por título de recipe que case com o task type — NÃO leia o recipes file inteiro. Extraia apenas: recipe number, pattern refs, reference modules
12b. **Pre-EXECUTE Existence Gate**: Mesmo gate de `feature/SKILL.md § Pre-EXECUTE Existence Gate`. Invoque identicamente (Full scope only, `## Files` ≤ 8). No retry/resume, o gate naturalmente lida com idempotência.

   **Skip entirely if `resumeMode === "continued"`** (Step 0.5). O modo `continued` confia nos checkboxes do pipeline-state como estão. Se o stale-context fallback escalar para `reanalyze`, o gate roda no re-dispatch.

    **Pre-check (mesmo que `feature/SKILL.md § Pre-EXECUTE Existence Gate`):** Antes de despachar o explorer, rode `rtk git diff --stat HEAD -- <files listed in spec's ## Files>`. Pule o gate inteiramente se output for vazio (sem mudanças) ou total insertions/deletions <10. Só proceda com dispatch do explorer se ≥10 linhas mudaram.

12c. **Wave Plan Scope (condicional — só se `pipeline-state.isWavePlan === true`):**

Quando o pipeline state indica um wave plan, o orquestrador despacha só a **wave atual**, não a spec inteira:

1. Leia `pipeline-state.currentWave` e `pipeline-state.totalWaves`.
2. A spec para trabalhar nesta invocação é `.claude/spec/{specName}/wave-{currentWave}-*/spec.md`. Substitua qualquer referência prior a `spec.md` na raiz do spec dir pela spec da wave atual.
3. **Entre waves** (ver Step 17 post-dispatch):
   - Em wave completion: rode commit estilo `/mustard:git commit` com mensagem `feat(wave-{N}/{role}): {summary}`. Se `/mustard:git commit` não é apropriado para o projeto, fallback para `git add {files} && git commit -m "..."`.
   - Emita wave completion e avance current wave via eventos:
     ```bash
     mustard-rt run emit-pipeline --kind pipeline.wave.complete --spec {specName} --payload "{\"wave\":{N},\"duration_ms\":{elapsed}}"
     ```
   - A projection deriva `completedWaves` e `currentWave` desses eventos — sem JSON state update needed.
   - Após emitir, rode `mustard-rt run wave-tree --spec-dir .claude/spec/{specName}` para mostrar progresso.
   - Force `resumeMode = "reanalyzed"` para a próxima wave transition para que diff-context refresh com as mudanças recém-commitadas.
   - **Cache this wave's diff:** logo após o commit wave-{N-1}, rode `git diff HEAD~1 HEAD > .claude/.pipeline-states/{spec-name}.wave-{N-1}.diff.md` para que a próxima wave possa ser sliced contra ele.
   - **Wave Slice Injection (ver § Wave Slice Injection abaixo):** antes de despachar wave N (N≥2):
     1. Rode `mustard-rt run spec-extract --spec {spec_path} --wave {N}` para pegar o wave slice; ele popula `{task_steps}`.
     2. Prepende o cached previous wave diff de `.claude/.pipeline-states/{spec-name}.wave-{N-1}.diff.md`. Injete `{task_steps}` + o diff (NÃO a spec inteira, NÃO os arquivos touched inteiros) no agent prompt.
   - Se `currentWave > totalWaves` → pule remaining wave dispatch, siga para Step 19 REVIEW + Step 20 CLOSE no overall wave plan.
4. **Se uma wave falha (REJECTED após 2 fix-loops, ou BLOCKED)** — ver § Wave Failure Handling abaixo.

12d. **Dependency Precheck (factual gate)**: Rode `mustard-rt run dependency-precheck --spec .claude/spec/{specName}/wave-{currentWave}-*/spec.md` (single-spec mode: drop the wave-N path, use `.claude/spec/{specName}/spec.md`). Parse JSON. Se `ok: false`:
   1. Imprima summary inline: `BLOCKED — N símbolos ausentes: {comma list of missing.symbol}. Sugestão: criar tactical-fix com {suggested_tactical_fix_files}.`
   2. Emita dispatch_failure event: `mustard-rt run emit-pipeline --kind pipeline.dispatch_failure --spec {specName} --payload "{\"reason\":\"dependency-precheck-failed\",\"missing\":{N}}"`
   3. AskUserQuestion: **"Criar tactical-fix automaticamente"** / **"Investigar manualmente"** / **"Forçar dispatch mesmo assim (override)"**.
   4. Tactical-fix path: invoke `Skill(mustard:tactical-fix)` com parent=current spec, descricao derivada dos missing symbols.
   5. Override path: emita `mustard-rt run emit-pipeline --kind pipeline.precheck_override --spec {specName} --payload "{\"reason\":\"user-override\"}"`, depois continue para step 13.
   Se `ok: true` (ou env `MUSTARD_DEPENDENCY_PRECHECK_MODE=off`): silencioso, continue para step 13.
   **Skip entirely if `resumeMode === "continued"`** — cached trust da sessão prior.

#### Wave Slice Injection

Entre waves, o orquestrador NÃO reinjeta a spec inteira nem os arquivos de código completos nos prompts dos agentes — `mustard-rt run spec-extract` recorta só a seção da wave (ou, em wave-plan, a sub-spec inteira) e o `git diff` cacheado substitui os arquivos full.

A subtraction `wave-slice` é emitida automaticamente pelo hook `subagent-tracker.js` em todo despacho de Task na fase EXECUTE — o orquestrador não faz nada. Fail-open: se a omissão não aconteceu (seção da wave não medível), nenhum evento é registrado.

13. **Plan waves:** `Depends on: none` → Wave 1; dependencies → later. DB+Backend parallel. Frontend after Backend UNLESS all parallel override conditions met (ver `.claude/pipeline-config.md` Parallel Rules). Review agents: ALWAYS dispatch em single parallel message. Skip completed tasks.

**Note on wave plans:** quando `isWavePlan === true`, este step planeja a estrutura de wave de agentes **dentro** da spec da wave atual apenas — agentes internos à spec da wave atual ainda podem split entre DB/Backend/Frontend sub-waves. O outer wave (1..N) é a sequência cross-spec gerenciada pelo Step 12c.
13b. **Cross-wave memory injection (wave plans apenas):** Se `pipeline-state.isWavePlan === true` AND `currentWave > 1`, rode `mustard-rt run memory cross-wave --spec {specName} --wave {currentWave}` e capture stdout no placeholder `{cross_wave_memory}` do agent-prompt template. Se `currentWave === 1` (ou single-spec mode), deixe `{cross_wave_memory}` vazio. Fail-open: missing memories → empty placeholder, nunca bloqueia o dispatch.

13c. **Model selection from wave-plan (wave plans apenas):** Leia a coluna `Modelo` da row para a active wave em `.claude/spec/{specName}/wave-plan.md` e passe esse value como o `model` arg de cada Task tool call neste wave dispatch. **O agente NUNCA escolhe o modelo; o orquestrador (SKILL) é fonte de verdade lendo o wave-plan.** O `model_routing` module continua bloqueando upgrades em relação à routing table. Para single-spec mode (sem wave-plan.md), mantenha o behavior existente — model vem de `pipeline-state.model`.

14. **Build agent prompts using template** (`.claude/refs/agent-prompt/agent-prompt.md`):
    - Read template once, then fill placeholders per agent using `.claude/pipeline-config.md` data:
      - `{subproject}` → from Agents table (Subproject column)
      - `{reference_files}` → 2-3 files from matched recipe
      - `{guards_summary}` → key guards from `{subproject}/CLAUDE.md`
      - `{entity_info}` → `_patterns` type, refs, subs from registry
      - `{role}`, `{boundary}`, `{return_sections}` → from Role Rules table in config
      - `{validate_command}`, `{build_command}` → from Agents table in config
      - `{retry_context}` → empty on first dispatch. On retry, fill per `.claude/refs/agent-prompt/agent-prompt.md § Retry Modes`. Granular retries use Step 4 § Granular Retry Protocol. Fix-loops (after REJECTED review) use Step 19b § Fix Loop Dispatch Protocol.
      - `{task_steps}` → checkboxed steps from spec
      - `{context_md}` → relevance-filtered glossary slice from `.claude/.pipeline-states/{specName}.context-md.md` (ver § Context Slice). Vazio quando não há `CONTEXT.md`.
      - `{cross_wave_memory}` → captured stdout do Step 13b (`mustard-rt run memory cross-wave`); vazio para wave 1 ou single-spec mode.
      - `{recommended_skills}` → from Skill Recommendations em `.claude/pipeline-config.md`:
        1. **Prepend `karpathy-guidelines`** para code-editing agents (impl/backend/frontend/database/bugfix). **Skip** para read-only Explore e Review agents.
        2. Glob `{subproject}/.claude/skills/` para generated pattern skills
        3. Add foundation skills matching o role (ui→design-craft+react-best-practices, mobile→design-craft)
        4. Format como bullet list: `- {skill-name}`

16. **Wave transitions** — entre waves, execute transitions de `.claude/pipeline-config.md`:
    - Após Wave 1 (api/database/library) completar, antes de Wave 2 (ui):
      - Execute cada comando listado na matching `Wave Transitions` section
    - Espere transitions completar antes de despachar a próxima wave

17. **Dispatch:** TaskUpdate(in_progress). Emita task lifecycle events ao redor de cada Task invocation:
    ```bash
    # Antes de despachar cada agent Task:
    mustard-rt run emit-pipeline --kind pipeline.task.dispatch --spec {specName} --payload "{\"wave\":{N},\"name\":\"{task-name}\",\"agent\":\"{agent-type}\"}"
    ```
    TODOS os agentes na mesma wave → SINGLE message (múltiplas Task invocations). **Pass `model` from pipeline state** (ex.: `model: "opus"`) em cada Task tool call — isso sobrescreve o agent YAML default.
    ```bash
    # Após cada agent Task retornar:
    mustard-rt run emit-pipeline --kind pipeline.task.complete --spec {specName} --payload "{\"wave\":{N},\"name\":\"{task-name}\",\"agent\":\"{agent-type}\",\"duration_ms\":{elapsed}}"
    ```
    On return: TaskUpdate(completed), advance wave. O hook `checklist-auto-mark.js` marca Checklist items silenciosamente conforme arquivos são edited. close-gate nega CLOSE se algum `[ ]` permanecer.

17b. **Agent Memory:** Após cada wave completar, rode `memory.js agent` uma vez por agente (summary ≤300 chars, include `files_modified` + `decisions`). Pule se nenhuma downstream wave permanece.

#### Escalation Status Handling

Após cada agente retornar, cheque o return value por um escalation status antes de avançar para a próxima wave:

- **Internal error** (no parseable output, empty return, API error) — re-despache o(s) agente(s) falhado(s) **sequencialmente** (não parallel) com o mesmo prompt. Max 1 Internal retry per agent. Se ainda falhando: STOP + report
- `CONCERN` — record verbatim sob `## Concerns` na spec; continue para next wave
- `BLOCKED` — pare imediatamente; use `AskUserQuestion` para reportar o blocker exato; NÃO avance
- `PARTIAL` — aplique Granular Retry Protocol do last completed step; NÃO restart from step 1
- `DEFERRED` — note na spec com justification do agente; pergunte ao usuário se o deferred item é load-bearing antes de closing

Se dois ou mais agentes na mesma wave retornarem `CONCERN`, surface todas as concerns juntas antes de despachar a próxima wave. Ver `.claude/pipeline-config.md` Escalation Statuses e Diagnostic Failure Routing para a tabela completa de status.

### Step 4: Validate, Review, QA & Complete

18. **VALIDATE** — Parse agent results: Backend→`dotnet build`, Frontend→`pnpm build`, Mobile→`fvm flutter analyze`. All passed → next. Failed → **granular retry** (ver abaixo).
19. **REVIEW (MANDATORY — NEVER SKIP):**
    - Despache review agent para CADA subproject afetado em uma SINGLE message (multiple Task invocations) usando `subagent_type: "general-purpose"` com review prompt
    - Review agent MUST read `{subproject}/CLAUDE.md` + `{subproject}/.claude/commands/guards.md`
    - Checklist categories: **SOLID, Design System, Patterns, i18n, Integration, Build, Elegance**
    - Cada issue classificada: CRITICAL (blocks), WARNING (recommended), NOTE (suggestion)
    - APPROVED (zero CRITICAL) → CLOSE
    - REJECTED (any CRITICAL) → ver Step 19b § Fix Loop Dispatch Protocol (max 2 loops)
    - **NEVER skip review** — nem para Light scope. Light scope ganha mesmo checklist, só com fewer files para review
    - **Record o verdict (após consolidado):** para cada subproject reviewed rode `mustard-rt run review-result --spec {specName} --verdict {approved|rejected} --critical {N} --subproject {subproject}`. Isso emite a `review` metric que surface em `/stats` sob Verification. Fail-open — nunca bloqueia CLOSE.

### Step 19b: Fix Loop Dispatch Protocol

Extract CRITICAL findings verbatim from review return (ou harness view). Build retry context using Mode=fix-loop (K=1 or 2). Despache same subagent_type + model. Re-dispatch review after. Max 2 fix-loops → STOP.

→ Ver `../../../refs/resume/fix-loop-wave.md`

### Step 19c: QA Phase (Wave 10) — MANDATORY

Após REVIEW retornar APPROVED, rode QA antes de CLOSE. NEVER vá REVIEW→CLOSE directly — `close-gate.js` nega CLOSE sem um passing `qa.result` event.

1. Emita stage transition: `mustard-rt run emit-pipeline --kind pipeline.stage --spec {specName} --payload "{\"stage\":\"QaReview\"}"`.
2. Rode `mustard-rt run qa-run --spec {specName}`. Para wave plans, `{specName}` é o wave-plan directory name.
3. Branch on `overall`:
   - **pass** — atualize `## Acceptance Criteria` checkboxes na spec (`[x]` para cada passed AC), depois emita `mustard-rt run emit-pipeline --kind pipeline.stage --spec {specName} --payload "{\"stage\":\"Close\"}"` (`close-gate` verifica o `qa.result` event antes de allow CLOSE) → siga para Step 20.
   - **fail** — extraia a failing AC list e re-despache via o Step 19b Fix Loop Dispatch Protocol, depois re-rode este step. Maximum 3 QA iterations.
   - **skip** (no Acceptance Criteria section) — informe inline `QA pulado — spec sem Acceptance Criteria`, depois siga para Step 20.
4. After 3 failed QA iterations → `AskUserQuestion`: "(a) corrigir manualmente e repetir, (b) relaxar o AC na spec, (c) abortar pipeline."
5. Visual: `[v] EXECUTE  [v] REVIEW  [>] QA  [ ] CLOSE`.

20. **CLOSE:**
    - **Wave plan gate:** se `pipeline-state.isWavePlan === true`, only CLOSE quando `completedWaves.length === totalWaves`. Se waves remain (`currentWave <= totalWaves` e wave N-1 just finished), **do not** rode CLOSE — em vez disso atualize state (`currentWave++`, `completedWaves.push`), output `═══ WAVE {N-1} COMPLETE — {role} ═══`, e pare. Próximo `/mustard:spec {letra}` pega wave N.
    - `mustard-rt run sync-registry`
    - Spec: `### Stage: Close`, `### Outcome: Completed`, all `[ ]` → `[x]`. Para wave plans: marque `wave-plan.md` outcome `Completed`, e marque cada `wave-N-{role}/spec.md` completed também.
    - O spec dir fica em `.claude/spec/{specName}/` — status flips para `completed` via o emit abaixo; sem filesystem move.
    - **Emit completion:**
      ```bash
      mustard-rt run emit-pipeline --kind pipeline.stage --spec {spec-name} --payload "{\"stage\":\"Close\"}"
      mustard-rt run emit-pipeline --kind pipeline.outcome --spec {spec-name} --payload "{\"outcome\":\"Completed\"}"
      ```
      Nenhum JSON file para deletar — o harness lida com archival.
    - **Pipeline Summary (BEFORE banner):** rode `mustard-rt run pipeline-summary --spec-dir .claude/spec/{specName}` e imprima o markdown inline. Para wave plans, o mesmo spec-dir aplica (o command lê o root `spec.md`; para wave-plan-final closes, use o wave-plan dir). Fail-open: em non-zero exit, log um warning e continue com o banner — NÃO aborte CLOSE. Aplica para ambos single-spec e wave-plan-final paths.
    - **Wave Tree (before banner):** `mustard-rt run wave-tree --spec-dir .claude/spec/{specName}`. Fail-open.
    - Output com agent colors: `═══ PIPELINE COMPLETE — {name} | Agents: {n} ok | Files: {c} created, {m} modified ═══` (para wave plans: append `| Waves: {totalWaves}`).

### Wave Failure Handling

Only quando `isWavePlan === true`. Triggers: REJECTED após 2 fix-loops, BLOCKED by user, ou repeated build failures. Updates `failedWaves`, escreve `failure.md` log, depois pergunta ao usuário: fix manually / re-PLAN wave / abort. Preserva prior wave commits.

→ Ver `../../../refs/resume/fix-loop-wave.md`

### Granular Retry Protocol

Parse last completed step → retry only from that step forward. Build Mode=granular retry context (harness view ou agent memory fallback). Use Minimal Retry Template. Max 2 retries per agent.

→ Ver `../../../refs/resume/fix-loop-wave.md`

### Pause Handoff & Next Action Rule

On pause: emit pause event, depois `memory.js agent`. Handoff MUST end with exactly ONE next action (sem lists of options).
```bash
mustard-rt run emit-pipeline --kind pipeline.pause --spec {specName} --payload "{\"reason\":\"{pauseReason}\",\"next_action\":\"{nextAction}\"}"
```

→ Ver `../../../refs/resume/fix-loop-wave.md`

## INVIOLABLE RULES

- Main context IS the Pipeline Runner — NUNCA wrap em a single Task agent
- NEVER implementar código diretamente — ALL via Task agents (1 per subproject per wave)
- Wave dispatch: ALL agents na mesma wave em a SINGLE message
- Cada sub-agent lê seu próprio `{subproject}/CLAUDE.md` + auto-loads relevant skills (orquestrador NÃO os lê)
- Atualize pipeline state + spec checkboxes em cada transition
- ALWAYS read `.claude/pipeline-config.md` para agent/wave/model config — NUNCA hardcode project-specific values
- ALWAYS use agent-prompt template — NUNCA build prompts from scratch
- ALWAYS execute wave transitions entre waves
- ALWAYS rode QA (Step 19c) após REVIEW e antes de CLOSE — NUNCA vá REVIEW→CLOSE directly
- ALWAYS rode dependency-precheck antes de dispatch (Step 12d) — block em `ok: false` a menos que user override
