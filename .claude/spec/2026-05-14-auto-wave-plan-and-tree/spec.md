# Feature: auto-wave-plan-and-tree
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-14T09:00:00Z
### Lang: pt

## Contexto

O `/mustard:feature` hoje decide decomposicao em waves olhando apenas sinais quantitativos do `scope-decompose.js` (fileCount, layerCount, newEntityCount, knowledgeMatches). Quando o usuario tem um roadmap multi-wave concreto — escrito em `.claude/plans/<slug>.md` e referenciado na spec/no prompt — esse sinal qualitativo nao entra na decisao. Resultado pratico (caso `sialia/.../resumo-m-ltiplos-zany-zebra.md`): a Wave 1 do roadmap virou spec standalone (`active/.../wave1`), foi para `completed/` ao fechar, e `/resume` parou de listar nada relacionado. O usuario teve que abrir nova `/feature` manualmente para cada wave seguinte, sem visibilidade do progresso global. A expectativa do usuario é que (a) quando ha roadmap, `/feature` automaticamente crie estrutura parent (`wave-plan.md`) + filhos (`wave-N-*/spec.md`) sem perguntar, e (b) o estado dessa arvore apareca inline no fim de `/feature`, no inicio/fim de `/resume`, e antes do banner de `/complete` — visual ASCII direto no terminal.

## Summary

Estender `scope-decompose.js` com deteccao agnóstica de "roadmap signal" (referencias a `.claude/plans/*.md`, tabelas/listas com `Wave N` / `W\d+` / `Etapa N`, palavra-chave `roadmap`/`multi-wave`); quando o sinal dispara, `/feature` cria automaticamente `wave-plan.md` + N subdirs `wave-N-{role}/spec.md` derivados do roadmap referenciado. Adicionar `wave-tree.js` (script novo) que renderiza arvore ASCII a partir de um spec-dir e instrumentar `/feature`, `/resume`, `/complete` para imprimir essa arvore nos pontos certos.

## Boundaries

- `templates/scripts/scope-decompose.js`
- `templates/scripts/wave-tree.js` (create)
- `templates/commands/mustard/feature/SKILL.md`
- `templates/commands/mustard/resume/SKILL.md`
- `templates/commands/mustard/complete/SKILL.md`
- `templates/hooks/__tests__/scope-decompose.test.js`
- `templates/hooks/__tests__/wave-tree.test.js` (create)

Fora de escopo: alteração em `/mustard:bugfix`, em `pipeline-summary.js` (já entregue), em harness events/views, em `scope-decompose.js`'s decisao para casos puramente quantitativos (preserva exatamente a logica atual).

## Entity Info

Sem entidade nova de dominio. Conceitos manipulados:
- **roadmap signal**: boolean derivado de inspecao textual do spec body ou prompt original.
- **wave-plan.md**: arquivo markdown na raiz de um spec-dir com tabela `| Wave | Role | Status | Pasta |`.
- **wave-N-{role}**: subdir nomeado `wave-1-backend`, `wave-2-frontend`, etc., contendo `spec.md` proprio com o mesmo formato Full/Light.

## Files (~7)

- `templates/scripts/scope-decompose.js` — adicionar input `text` (spec body ou prompt original), funcao `detectRoadmapSignal(text)` retornando `{ hit: boolean, matches: string[] }`. Quando hit=true → retornar `{ decompose: true, reason: "roadmap-signal", roadmapMatches: matches, signals: {...} }`. Preservar 100% das decisoes existentes (single-layer/multi-layer/wide-and-new-entities/history-match) quando `text` ausente ou hit=false.
- `templates/scripts/wave-tree.js` (create) — CLI: `bun wave-tree.js --spec-dir <path> [--format ascii|json]`. Le `<path>/wave-plan.md` se existir; parseia tabela de waves; para cada wave, abre `<path>/<pasta>/spec.md` e extrai `### Status:`. Mapeia status → icone: `completed`=`[v]`, `implementing`=`[>]`, `closed-followup`=`[~]`, `draft`/`active`/`queued`=`[ ]`, `blocked`/`rejected`=`[!]`. Output ascii:
  ```
  Roadmap: <slug>
  ├─ [v] wave-1-backend       (completed)
  ├─ [>] wave-2-frontend      (implementing)
  └─ [ ] wave-3-billing       (queued)
  ```
  Fallback single-spec (sem wave-plan.md): `Spec: <slug>  [v] (completed)`. Exit 1 se `--spec-dir` ausente; fail-open (exit 0 + linha "(no spec)") se dir nao existe.
- `templates/commands/mustard/feature/SKILL.md` — em ANALYZE/PLAN, apos rodar `scope-decompose`, se `roadmapMatches[0]` aponta para um arquivo `.claude/plans/*.md`, ler esse arquivo, extrair tabela de waves (regex sobre `\| W?\d+ \|` rows) e gerar automaticamente: `wave-plan.md` (tabela copiada/adaptada) + `wave-1-{role}/spec.md` esqueleto (apenas wave 1 detalhada; waves 2..N ficam como esqueleto `Status: queued`). No fim do PLAN (single e wave-plan), chamar `bun .claude/scripts/wave-tree.js --spec-dir .claude/spec/active/{spec-name}` e imprimir output inline antes da AskUserQuestion de aprovacao.
- `templates/commands/mustard/resume/SKILL.md` — em Step 1 (Detect & Confirm), apos "Present Handoff Summary", chamar wave-tree e imprimir antes da pergunta "Continue from next action?". Em Step 20 (CLOSE), chamar wave-tree antes do banner `═══ PIPELINE COMPLETE ═══`. Para wave plans em progresso, tambem chamar entre waves (apos Step 17 dispatch+return, antes de avancar para `currentWave += 1`).
- `templates/commands/mustard/complete/SKILL.md` — em Step 7 (Output), adicionar Step `7b. **Wave Tree**` logo apos `7a. **Pipeline Summary**`: `bun .claude/scripts/wave-tree.js --spec-dir .claude/spec/active/{spec-name}` (ou `completed/` se ja movido), inline antes do banner. Fail-open (warn, nao aborta CLOSE).
- `templates/hooks/__tests__/scope-decompose.test.js` — adicionar 3+ cases: (a) text mencionando `.claude/plans/foo.md` → `decompose:true, reason:"roadmap-signal"`, (b) text com tabela `| Wave 1 | ... | Wave 5 |` → hit=true, (c) text sem sinais qualitativos → falls back para logica quantitativa atual.
- `templates/hooks/__tests__/wave-tree.test.js` (create) — fixtures cobrindo: (1) spec-dir com `wave-plan.md` + 3 wave subdirs com statuses variados, (2) spec-dir single-spec sem wave-plan.md, (3) spec-dir vazio/inexistente, (4) `--format json` retorna shape valido.

## Tasks

### Implementation Agent (Wave 1)

- [x] Modificar `templates/scripts/scope-decompose.js`: aceitar campo `text` no stdin JSON; implementar `detectRoadmapSignal(text)` com regex agnosticas (`\.claude\/plans\/[^\s"']+\.md`, `\b(?:Wave|W|Etapa|Fase|Phase)\s*\d+`, `\broadmap\b`, `\bmulti[-\s]?wave\b`); inserir esse caminho ANTES dos checks existentes (history → roadmap → multi-layer → wide). Output ganha `roadmapMatches: string[]` quando aplicavel.
- [x] Criar `templates/scripts/wave-tree.js`: CLI flags `--spec-dir <path>` (obrigatoria) e `--format ascii|json` (default ascii); parsing de `wave-plan.md` via regex `^\|\s*(W?\d+|Wave\s*\d+)\s*\|` para extrair lista; lookup de `### Status:` em cada wave subdir; mapeamento de status→icone descrito acima; fallback single-spec; fail-open.
- [x] Atualizar `templates/hooks/__tests__/scope-decompose.test.js` com cases (a)(b)(c) acima; preservar todos os testes atuais inalterados.
- [x] Criar `templates/hooks/__tests__/wave-tree.test.js` com 4 cases descritos.
- [x] Validar: `bun test templates/hooks/__tests__/scope-decompose.test.js templates/hooks/__tests__/wave-tree.test.js` verde.

### Orchestrator Agent (Wave 2)

- [x] Modificar `templates/commands/mustard/feature/SKILL.md` em ANALYZE e PLAN: descrever a regra "se `scope-decompose` retornar reason=roadmap-signal AND roadmapMatches contem path para `.claude/plans/*.md`, ler esse arquivo, extrair tabela de waves, e criar wave-plan.md + wave-1-{role}/spec.md (detalhada) + wave-N-{role}/spec.md esqueletos (Status: queued) — sem AskUserQuestion. Sempre rodar wave-tree no fim do PLAN."
- [x] Modificar `templates/commands/mustard/resume/SKILL.md` Step 1 e Step 20 e entre waves: incluir chamada a `bun .claude/scripts/wave-tree.js --spec-dir <path>` nos 3 pontos.
- [x] Modificar `templates/commands/mustard/complete/SKILL.md` Step 7: adicionar 7b apos 7a chamando wave-tree (fail-open, igual a pipeline-summary).
- [x] Validar: `bun test templates/hooks/__tests__/hooks.test.js` verde (regressao global).

## Dependencies

- Wave 2 depende de Wave 1 (precisa de `wave-tree.js` existindo para chamadas em SKILL.md fazerem sentido). Mas Wave 1 e Wave 2 podem ser implementadas no mesmo dispatch (mesmo subproject `templates`, sem cross-layer).

## Concerns

- WARN layer-gap (analyze-validation): "Backend Agent" original confundiu o validator — renomeado para "Implementation Agent" para refletir que o trabalho e em scripts/templates (JS+MD), nao backend de produto.
- WARN missing-file (analyze-validation): referencias a `wave-plan.md` e `.claude/plans/foo.md` no corpo da spec sao exemplos textuais, nao arquivos a serem criados; validator reporta como falso-positivo. Arquivos novos reais marcados com `(create)`.

## Acceptance Criteria

Critérios binários (pass/fail), executáveis e independentes.

- [x] AC-1: scope-decompose detecta referencia a `.claude/plans/*.md` em `text` e retorna `decompose:true, reason:roadmap-signal` — Command: `node -e "const {spawnSync}=require('child_process'); const r=spawnSync('bun',['templates/scripts/scope-decompose.js'],{input:JSON.stringify({fileCount:3,layerCount:1,newEntityCount:0,text:'veja .claude/plans/foo.md'}),encoding:'utf8'}); const o=JSON.parse(r.stdout); process.exit(o.decompose===true&&o.reason==='roadmap-signal'?0:1)"`
- [x] AC-2: scope-decompose preserva decisao quantitativa quando `text` ausente — Command: `node -e "const {spawnSync}=require('child_process'); const r=spawnSync('bun',['templates/scripts/scope-decompose.js'],{input:JSON.stringify({fileCount:3,layerCount:1,newEntityCount:0,knowledgeMatches:[]}),encoding:'utf8'}); const o=JSON.parse(r.stdout); process.exit(o.decompose===false&&o.reason==='single-layer'?0:1)"`
- [x] AC-3: wave-tree exit 1 sem `--spec-dir` — Command: `node -e "const {spawnSync}=require('child_process'); const r=spawnSync('bun',['templates/scripts/wave-tree.js'],{encoding:'utf8'}); process.exit(r.status===0?1:0)"`
- [x] AC-4: Suite scope-decompose passa — Command: `bun test templates/hooks/__tests__/scope-decompose.test.js`
- [x] AC-5: Suite wave-tree passa — Command: `bun test templates/hooks/__tests__/wave-tree.test.js`
- [x] AC-6: SKILL.md de feature, resume e complete referenciam `wave-tree.js` — Command: `node -e "const fs=require('fs');const ok=['templates/commands/mustard/feature/SKILL.md','templates/commands/mustard/resume/SKILL.md','templates/commands/mustard/complete/SKILL.md'].every(p=>fs.readFileSync(p,'utf8').includes('wave-tree.js'));process.exit(ok?0:1)"`
- [x] AC-7: feature/SKILL.md descreve criacao automatica de wave-plan.md a partir de roadmap signal — Command: `node -e "const c=require('fs').readFileSync('templates/commands/mustard/feature/SKILL.md','utf8');process.exit(c.includes('roadmap-signal')&&c.includes('wave-plan.md')?0:1)"`
- [x] AC-8: Suite global continua verde — Command: `bun test templates/hooks/__tests__/hooks.test.js`
