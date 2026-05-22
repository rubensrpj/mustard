# Feature: pre-execute-existence-gate
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-04-09T00:00:00Z

## Summary
Adicionar Gate de verificação de existência entre PLAN→EXECUTE no `/mustard:feature` e `/mustard:resume`: ANTES de dispatchar impl agents, rodar 1 explorer Haiku (`subagent_type: "Explore"`, `model: "haiku"`, prompt ≤2000 chars) que analisa CADA task do checklist (task-level, não file-level) e verifica se os identifiers correspondentes já existem nos arquivos alvo. 3 desfechos:

1. **Nenhuma task já feita** → proceder EXECUTE normal (gate transparente)
2. **Algumas feitas, outras não** → marca `[x]` nas feitas, re-dispatch EXECUTE só nas restantes (mantém scope original, NÃO inventa "PARTIAL" state)
3. **Todas já feitas** → surface obrigatório via `AskUserQuestion` (nunca skip silencioso); user escolhe: close/force-execute/abort

**Skip conditions**: Light scope (overhead não se paga) OR `## Files` count > 8 (Haiku 10-tool-use cap não cobre).

Evita desperdício do tipo `zelia-tone-config` (~108s Opus "do nothing" observado em sessão anterior).

## Why
Memory `reference_mustard_token_efficiency.md` lista "Despachar impl quando código já existe" como top-3 ineficiência. Um Haiku pré-execute custa ~2.5K tokens (budget explorer) e pode economizar wave inteiro de impl (50-95K tokens). ROI: ~20:1 no pior caso, 0 no melhor. Sempre positivo.

## Why Full scope
Modifica fluxo da pipeline (feature.md + resume.md), adiciona nova fase obrigatória em Full scope features, interage com estado do spec (mark tasks done, re-dispatch seletivo). 3+ arquivos afetados, mudança estrutural.

## Boundaries
- `templates/commands/mustard/feature/SKILL.md` (modify — adicionar Pre-EXECUTE Gate section)
- `templates/commands/mustard/resume/SKILL.md` (modify — referência DRY ao gate)
- `.claude/pipeline-config.md` (modify — documentar novo fluxo)
- `.claude/commands/mustard/feature/SKILL.md` (mirror)
- `.claude/commands/mustard/resume/SKILL.md` (mirror)

## Tasks

### templates-impl Agent (Wave 1)

#### Gate insertion in feature.md

- [x] Em `templates/commands/mustard/feature/SKILL.md`, após a transição PLAN → EXECUTE (e DEPOIS de approval do user), inserir nova seção **"Pre-EXECUTE Existence Gate"**. O conteúdo a copy-paste está abaixo entre os delimitadores `~~~` (outer `~~~` evita conflito com o bloco ` ```javascript ` aninhado no dispatch):

~~~markdown
### Pre-EXECUTE Existence Gate (Full scope only)

**Skip conditions**: Light scope OR `## Files` section lists more than 8 files (cost-benefit inverts — Haiku 10-tool-use cap will not cover).

Before dispatching implementation agents, run 1 Haiku explorer to verify the work is still needed.

**Dispatch:**

```javascript
Task({
  subagent_type: "Explore",
  model: "haiku",
  description: "Pre-EXECUTE existence check",
  prompt: `# EXISTENCE CHECK
Read .claude/spec/active/{specName}/spec.md sections: "## Files" and "## Checklist".

For EACH checklist task (task-level, NOT file-level):
  1. Extract 1-3 concrete identifiers from the task text — function names, component names, file path fragments, string literals.
     Example: task "Add LogoutButton component with handleLogout handler" → identifiers: ["LogoutButton", "handleLogout"].
  2. Identify target files for the task from "## Files" (match by extension, name hint, or task context).
  3. Grep each target file for the identifiers.
  4. Verdict for this task:
     - ALL target files contain a MAJORITY of identifiers → all_present=yes
     - SOME do, SOME do not → all_present=partial
     - NONE do → all_present=no

Return a markdown table:
| task | target_files | all_present | evidence |
|------|--------------|-------------|----------|
| <task text> | <comma-sep files> | yes/partial/no | <identifier:line or "none"> |

Return ≤20 lines total. Self-cap: ≤10 tool uses (the tool-use budget is the true limit, not the task count).`
})
```

**Decision after return (orchestrator inspects the returned table):**

- **All tasks `all_present=no`** → Gate is transparent. Proceed to EXECUTE normally.
- **Mixed** (any combination that is NOT all-no AND NOT all-yes — includes all-partial, yes+no, partial+no, yes+partial, yes+partial+no) → Edit the spec: mark `[x]` on tasks where `all_present=yes`. Leave `[ ]` on `partial` and `no` (both require re-dispatch). Re-dispatch EXECUTE only for tasks still `[ ]`. Keep the original scope (Light/Full). Do NOT invent a new "PARTIAL" state.
- **All tasks `all_present=yes`** → **MANDATORY user surface** via `AskUserQuestion`: _"Pre-EXECUTE Existence Gate detected all N tasks already implemented. Evidence: {inline table}. Choose: (a) Close as already-implemented, (b) Force EXECUTE anyway (the gate may be wrong), (c) Abort pipeline."_ Never silently skip EXECUTE.
~~~

- [x] Ensure this gate is **only invoked for Full scope AND `## Files` count ≤ 8** (Light + very large specs skip it)

#### Gate insertion in resume.md

- [x] Em `templates/commands/mustard/resume/SKILL.md`, inserir a referência DRY ANTES do step `13. **Plan waves:**` dentro da seção `### Step 3: Execute — Wave System` (localizar via `Grep "13\. \*\*Plan waves"`). A referência é um sub-step que roda antes do planejamento de waves. Também Full only:

~~~markdown
### Pre-EXECUTE Existence Gate

Same gate as `feature/SKILL.md § Pre-EXECUTE Existence Gate`. Invoke identically (Full scope only, `## Files` ≤ 8). On retry/resume, the gate naturally handles idempotence: tasks already `[x]` from a prior run are treated as Mixed — the Haiku confirms they stay done and the orchestrator only re-dispatches what remains `[ ]`.
~~~

### templates-impl Agent (Wave 2, depends on Wave 1)

#### Documentation + Mirror + Validation

- [x] Atualizar `.claude/pipeline-config.md` seção "Pipeline Phases" para incluir o gate no fluxo Full:

~~~
ANALYZE → [analyze-validation.js] → PLAN → /approve → [Pre-EXECUTE Existence Gate (Full + Files ≤ 8)] → EXECUTE → REVIEW → CLOSE
~~~

- [x] Mirror `templates/commands/mustard/feature/SKILL.md` → `.claude/commands/mustard/feature/SKILL.md`
- [x] Mirror `templates/commands/mustard/resume/SKILL.md` → `.claude/commands/mustard/resume/SKILL.md`
- [x] Build: `rtk npm run build` → PASS
- [x] Hook tests: `rtk bun test templates/hooks/__tests__/hooks.test.js` → 26/26
- [x] **Walkthrough mental obrigatório**: ler o texto inserido em feature.md, simular orchestrator respondendo aos 3 cenários (all-no / mixed / all-yes) — confirmar que as decisões são determinísticas e que o path "all-yes" realmente aciona `AskUserQuestion`. Documentar a simulação na seção `## Result` do spec.

## Files (~5)
- `templates/commands/mustard/feature/SKILL.md` (modify — main gate)
- `templates/commands/mustard/resume/SKILL.md` (modify — DRY reference)
- `.claude/pipeline-config.md` (modify — flow doc)
- `.claude/commands/mustard/feature/SKILL.md` (mirror)
- `.claude/commands/mustard/resume/SKILL.md` (mirror)

## Dependencies
- Wave 1 sequencial: gate em feature.md primeiro, depois referência em resume.md.
- Wave 2 após Wave 1: docs em pipeline-config.md + mirrors + testes + walkthrough.

## Acceptance
- Seção "Pre-EXECUTE Existence Gate" presente em `feature/SKILL.md`, Full only, com skip condition `Files > 8` explícita
- Referência DRY em `resume/SKILL.md` (não duplica descrição)
- `pipeline-config.md` documenta novo fluxo incluindo `Files ≤ 8`
- Mirrors sync
- Build PASS
- Hook tests 26/26 PASS
- Prompt do Haiku dispatch é ≤2000 chars (verificar character count literal)
- Retorno do Haiku é task-level (não file-level): tabela `| task | target_files | all_present | evidence |`
- Decisão "all-yes" aciona `AskUserQuestion` obrigatório, nunca skip silencioso
- Walkthrough mental documentado na seção `## Result` da spec final
- Markdown do template renderiza sem conflito (outer `~~~`, inner ` ``` `)

## Guards
- Gate SÓ roda em Full scope — Light skip (já é barato)
- Gate também SKIP se `## Files` count > 8 (cost-benefit inverte, Haiku cap 10 tool uses não cobre)
- Haiku prompt ≤2000 chars (dentro do budget explorer)
- Return cap: ≤20 lines + ≤10 tool uses (dentro de scan budget)
- Tool use cap é soft (instrução no prompt), sem hook enforcement — Haiku geralmente respeita
- NÃO altera lógica existente de Light scope EXECUTE
- NÃO remove nada de feature.md, só adiciona seção
- NÃO inventa novo scope "PARTIAL" — decisão Mixed usa mark `[x]` + re-dispatch seletivo no scope original
- Idempotent / safe-to-retry: 2o run vê tasks já `[x]` e trata como Mixed, validando o restante. Seguro após crash parcial.
- DRY: resume.md referencia feature.md, não duplica
- Skip-EXECUTE case (all-yes) requer `AskUserQuestion` obrigatório — nunca silenciosamente skipar
- Retorno do Haiku deve ser task-level (não file-level) para evitar o mapping ambíguo arquivo→task

## Elegance Check
Pergunta: "Existe abordagem mais elegante?"

Alternativas consideradas:
1. **Hook PreToolUse(Task)** — não funciona: hook fires por-tool-call, não por phase-transition. Não tem contexto de "estou entre PLAN e EXECUTE".
2. **Script dedicado (existence-check.js)** — possível, mas adiciona artefato extra. A decisão (Haiku call) é pipeline-level, pertence ao prompt do orchestrator.
3. **Impl agent faz own check primeiro** — ruim: já gastou Opus tokens para chegar lá. O ponto é evitar o gasto.
4. **Retorno file-level em vez de task-level** — rejeitado: mapear arquivo→task é ambíguo (task N pode tocar vários arquivos, arquivo N pode ser tocado por várias tasks). Task-level elimina a ambiguidade.
5. **Delimitador ` ``` ` no outer em vez de `~~~`** — rejeitado: cria conflito com o bloco ` ```javascript ` aninhado. `~~~` é a escolha CommonMark para aninhamento.

Conclusão: gate no prompt do orchestrator (SKILL.md) com retorno task-level e delimitador outer `~~~` é a decomposição correta. Mantendo.

## Open Questions (decididas)
1. Gate em Light scope? → **NO**. Light é ≤5 files, overhead do Haiku explorer (2.5K tokens) não se paga.
2. Gate no `bugfix` pipeline também? → **NO por ora**. Bugfix é mais dinâmico, contexto muda rápido, existence check faz menos sentido. Possível iteração futura.
3. Gate dispatch em background? → **NO**. Sequencial — precisa do resultado para decidir próximo passo.
4. Limite de `## Files` para ativar o gate? → **≤8 files**. Haiku self-cap é 10 tool uses; precisa margem para múltiplos greps por arquivo. >8 files → overhead inverte o ROI.
5. Decisão "Mixed" deveria criar novo scope "PARTIAL"? → **NO**. Reusar `[x]`/`[ ]` existente + re-dispatch seletivo. Evita inflação de estados.
6. Decisão "all-yes" pode skipar silenciosamente? → **NO**. `AskUserQuestion` é obrigatório porque user aprovou o spec esperando EXECUTE — skip silencioso é surpresa ruim.
7. Retorno do Haiku: file-level ou task-level? → **task-level**. Mapping arquivo→task é ambíguo. Task-level é a unidade correta.

## Result

### Files Modified
- `templates/commands/mustard/feature/SKILL.md` lines 170-209 — Pre-EXECUTE Existence Gate section inserted before Light scope EXECUTE
- `.claude/commands/mustard/feature/SKILL.md` — mirror of above
- `templates/commands/mustard/resume/SKILL.md` line 100 — step 12b inserted before step 13
- `.claude/commands/mustard/resume/SKILL.md` — mirror of above
- `.claude/pipeline-config.md` lines 5-11 — new `## Pipeline Phases` section added with flow diagram

### Validation
- Haiku prompt char count: **1067 / 2000** ✓
- Markdown delimiters: 0 `~~~` pairs in feature.md (outer delimiters were spec-only, not inserted). 4 ` ``` ` pairs, including `\`\`\`javascript` at line 178 closing at line 203. No conflicts.
- Build: PASS (tsc clean)
- Hook tests: 26/26 PASS

## Walkthrough Simulation

### Scenario 1 — All tasks `all_present=no`
Spec: "Add new UserProfile component". No file contains "UserProfile" yet. Haiku greps 4 target files, finds 0 matches across all tasks. Returns table with all rows `all_present=no`. Orchestrator reads the table, matches the "All tasks all_present=no" decision branch, and proceeds immediately to EXECUTE dispatching all impl agents normally. Gate is fully transparent — zero user friction, zero scope change.

### Scenario 2 — Mixed (1 yes, rest no)
Same spec but a stub `UserProfile.tsx` was created earlier (contains "UserProfile" export but no "handleLogout"). Haiku returns: task 1 (create component) → `all_present=yes` (identifier found), task 2 (add handler) → `all_present=no`. Orchestrator matches "Mixed" branch. Edits spec: marks task 1 as `[x]`, leaves task 2 as `[ ]`. Re-dispatches EXECUTE only for task 2. Original Full scope is preserved — no new state invented. Result: saves one impl agent dispatch, handles the partial-done case safely.

### Scenario 3 — All tasks `all_present=yes`
Spec was already fully implemented in a prior session that crashed before CLOSE. Haiku finds all identifiers present in all target files. Returns all rows `all_present=yes`. Orchestrator hits the "All tasks all_present=yes" branch and **MUST** call `AskUserQuestion` with options: (a) Close as already-implemented, (b) Force EXECUTE anyway, (c) Abort. The word "Never silently skip EXECUTE" in feature.md makes this non-skippable — any orchestrator reading the decision table will find no path to proceed without surfacing the question. User can safely choose (a) to close the pipeline, recovering from the prior crash gracefully.
