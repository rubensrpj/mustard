# Feature: pipeline-gates-bundle
### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: full
### Checkpoint: 2026-04-09T00:00:00Z

## Summary
Adicionar dois gates defensivos na pipeline do Mustard que evitam os 2 maiores desperdícios de tokens observados em sessões recentes:

1. **Pre-EXECUTE existence gate** — entre PLAN→EXECUTE, despacha 1 explorer Haiku (≤2.5K) que verifica "os arquivos do spec existem? a feature já foi implementada?". Evita caso observado (~108s Opus "do nothing" em `zelia-tone-config`).

2. **ANALYZE completeness validator** — novo script `analyze-validation.js` que lê output do ANALYZE e valida: todas entities declaradas estão no registry, todos layers declarados (DB/Backend/Frontend) têm file refs, zero sinais ambíguos. Bloqueia PLAN se falhar. Reduz fix loops pós-review (caso observado: 170K+266K tokens extras).

Juntos, atacam o principal vilão de custo: dispatches que gastam muito e entregam pouco.

## Why
Memory `reference_mustard_token_efficiency.md` lista como top-3 ineficiências:
- "Despachar impl quando código já existe" (~108s Opus wasted)
- "Fix loops pós-review custam 1 wave inteira extra" (170K+266K tokens)

Ambos derivam da mesma causa raiz: **a pipeline confia que o ANALYZE foi completo e que o código alvo precisa ser escrito**, sem verificação dupla. Os 2 gates são barreiras baratas (Haiku + script local) que interceptam esses casos antes de gastar Opus.

## Boundaries
- `templates/commands/mustard/feature/SKILL.md` — adicionar gates
- `templates/commands/mustard/resume/SKILL.md` — adicionar pre-EXECUTE gate (resume usa o mesmo flow)
- `templates/scripts/analyze-validation.js` (create)
- `.claude/commands/mustard/feature/SKILL.md` — mirror
- `.claude/commands/mustard/resume/SKILL.md` — mirror
- `.claude/scripts/analyze-validation.js` — mirror
- `.claude/pipeline-config.md` — documentar novos gates na Pipeline Phases

## Tasks

### templates-impl Agent (Wave 1)

#### Gate 1: Pre-EXECUTE Existence Check
- [ ] Em `templates/commands/mustard/feature/SKILL.md`, entre PLAN (após approval) e EXECUTE (início do dispatch de impl), inserir nova seção **"Pre-EXECUTE Existence Gate"**:
  ```
  Before dispatching implementation agents, run a Haiku explorer to verify the work still needs to be done.
  Prompt cap: ≤2000 chars. Subagent: Explore. Model: haiku.
  Task: "Given this spec checklist, check each target file. For each: does the file exist? If it exists, does it already contain the described change (scan for key symbols/patterns)?"
  Return format: table rows (file, exists, already_done, evidence).
  Gate decision:
    - All files missing or no symbols matching → proceed to EXECUTE normally
    - Some files already have the change → downgrade to PARTIAL mode, update spec (mark done items), re-dispatch only for missing items
    - All files already have the change → close spec as "already implemented", skip EXECUTE
  ```
- [ ] Em `templates/commands/mustard/resume/SKILL.md`, adicionar o mesmo gate ANTES do step que despacha agents. Se spec reentra via /resume, o gate também roda.

#### Gate 2: ANALYZE Completeness Validator
- [ ] Criar `templates/scripts/analyze-validation.js`:
  - Input: `--spec {path/to/spec.md}` (CLI arg) OU stdin JSON `{specPath}`
  - Output: JSON `{ok, issues}` onde `issues` é array de `{severity, type, message}`
  - Validations:
    1. **Layer coverage**: se spec menciona "Backend" mas não tem `## Files` refs com `.ts|.cs|.py` backend — WARN
    2. **Entity registry**: se spec referencia entity que NÃO existe em `.claude/entity-registry.json` — ERROR (could be new entity, but should be explicit)
    3. **Ambiguous signals**: se spec tem keywords ambíguas ("maybe", "possibly", "TBD") em Summary ou Tasks — WARN
    4. **File refs resolvable**: cada entrada em `## Files` aponta para path que existe ou é marcado `(create)` — WARN se não
    5. **Task decomposition**: cada agent section tem 3-8 tasks — WARN se <3 ou >10
  - Exit code 0 sempre (validator, não blocker do shell); JSON output é a API
  - Built-ins only: `fs`, `path`, process.argv
- [ ] Em `templates/commands/mustard/feature/SKILL.md`, no final do ANALYZE phase (antes do PLAN phase gate), invocar:
  ```
  Run `rtk node .claude/scripts/analyze-validation.js --spec .claude/spec/active/{name}/spec.md`
  If output has `issues` with severity `ERROR`, STOP and surface to user via AskUserQuestion: "ANALYZE found N issues. Review and fix or override?"
  WARN-level issues are surfaced but non-blocking (proceed after noting them in spec under `## Concerns`).
  ```
- [ ] Mirror: `templates/scripts/analyze-validation.js` → `.claude/scripts/analyze-validation.js`

### templates-impl Agent (Wave 2, depends on Wave 1)

#### Documentation & Integration
- [ ] Atualizar `.claude/pipeline-config.md` (e se houver outro pipeline-config) seção "Pipeline Phases" para incluir os 2 gates no fluxo:
  ```
  ANALYZE → [analyze-validation.js] → PLAN → /approve → [pre-execute existence gate] → EXECUTE → REVIEW → CLOSE
  ```
- [ ] Mirror feature.md + resume.md para `.claude/commands/mustard/`
- [ ] Rodar `rtk npm run build` → PASS
- [ ] Rodar `rtk bun test hooks/__tests__/hooks.test.js` → 26/26
- [ ] Smoke test do validator: criar spec mínimo sintético e rodar o script; confirmar `{ok:true, issues:[]}` para spec limpo e issues para spec com problemas
- [ ] Smoke test do pre-EXECUTE gate: descrever manualmente o dispatch mental (sem rodar Haiku real) — garantir que o SKILL.md deixa o passo claro e copy-pasteável

## Files (~8)
- `templates/commands/mustard/feature/SKILL.md` (modify)
- `templates/commands/mustard/resume/SKILL.md` (modify)
- `templates/scripts/analyze-validation.js` (create)
- `.claude/commands/mustard/feature/SKILL.md` (mirror)
- `.claude/commands/mustard/resume/SKILL.md` (mirror)
- `.claude/scripts/analyze-validation.js` (mirror)
- `.claude/pipeline-config.md` (modify)
- (eventualmente) `templates/pipeline-config.md` se existir

## Dependencies
- Wave 1 paralelizável: Gate 1 (feature.md/resume.md edits) e Gate 2 (script novo + feature.md edit) tocam o MESMO arquivo feature.md → **sequenciar**: primeiro cria o script + edita feature.md ANALYZE section; depois edita feature.md PLAN→EXECUTE section + resume.md.
- Wave 2 após Wave 1: docs em pipeline-config.md + mirror + testes.

## Acceptance
- `templates/scripts/analyze-validation.js` existe, é built-ins only, retorna JSON
- Validator roda standalone: `rtk node templates/scripts/analyze-validation.js --spec test.md` → JSON válido
- `feature.md` ANALYZE phase invoca o validator antes de passar para PLAN
- `feature.md` PLAN→EXECUTE tem o Pre-EXECUTE Existence Gate documentado
- `resume.md` espelha o gate antes do dispatch
- `pipeline-config.md` atualizado com o novo fluxo ANALYZE → [validator] → PLAN → /approve → [existence gate] → EXECUTE
- Espelhado em `.claude/`
- Build PASS
- Hook tests 26/26 PASS

## Guards
- Gate 1 NÃO deve ser rodado para Light scope (já é minimal overhead) — só Full
- Gate 2 NÃO deve bloquear por WARNs, só por ERRORs (e com opção override)
- Validator é fail-safe: erro interno → exit 1 com JSON `{ok:false,issues:[{severity:"ERROR",type:"validator-crash",message:"..."}]}` → user decide
- NÃO introduzir npm deps
- NÃO mudar lógica existente de Light scope
- NÃO remover nada do feature.md, só adicionar seções

## Elegance Check
Pergunta obrigatória antes de aprovar: "Existe uma abordagem mais elegante que faça os dois trabalhos com menos arquivos?"
Resposta: O validator é 1 script novo pequeno (~150 linhas), e os gates são seções em 3 SKILL.md (feature, resume, pipeline-config). Tentativa de "unificar" tudo em 1 hook JS seria pior: hooks rodam em momentos fixos do lifecycle, mas estes gates são conceituais/pipeline-level — pertencem ao orchestrator prompt, não a código executado pelo harness. Decisão: manter 2 artefatos separados, pois respondem a perguntas diferentes (validator = "o spec está completo?", existence gate = "a implementação ainda é necessária?").

## Open Questions Before EXECUTE
1. Gate 1 deve sempre rodar ou só em Full scope? → **Only Full** (Light é barato, overhead do Haiku explorer não vale)
2. Gate 2 deve bloquear em WARN ou só ERROR? → **Só ERROR bloqueia**, WARNs são anotadas em `## Concerns` e seguem
3. O validator deve ler pipeline-config.md para descobrir layers/stacks esperados, ou assumir um conjunto fixo? → **Ler pipeline-config.md** se existir seção "Stacks"; senão fallback para conjunto fixo (DB/Backend/Frontend)
