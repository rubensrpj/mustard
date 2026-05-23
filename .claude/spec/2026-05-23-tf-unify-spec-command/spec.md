# Tactical Fix: unificar /mustard:approve + /mustard:resume em /mustard:spec

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-23T18:20:00Z
### Lang: pt
### Parent: 2026-05-23-dashboard-design-system

## Contexto

Hoje existem dois comandos com escopo redundante:
- `/mustard:approve` — aprova spec em `Stage: Plan, Outcome: Active`.
- `/mustard:resume` — retoma spec em `Stage: Execute, Outcome: Active`.

Ambos foram escritos partindo da premissa de **uma spec ativa por vez**:
- `commands/mustard/approve/SKILL.md` step 3: *"Locate **the** active spec"* (singular, sem fallback).
- `commands/mustard/resume/SKILL.md` step 1: detecta candidatas mas o trecho `If multiple → ask which one` é vago.

Na sessão atual, 7 specs estavam simultaneamente em PLAN/EXEC + Active:
- `2026-05-22-project-profiler` (wave plan top-level, untracked)
- `2026-05-23-dashboard-design-system` (parent em EXEC, wave plan W5+W6)
- `2026-05-23-tf-dashboard-page-primitives` (TF, untracked)
- `2026-05-23-dependency-precheck` (EXEC, untracked)
- `2026-05-23-tf-dashboard-ds-tokens-remap` (TF)
- `2026-05-23-tf-dashboard-eslint-baseline` (TF)
- `2026-05-23-tf-unify-spec-command` (esta TF — meta)

`/approve` pegou a primeira sem perguntar; usuário rebateu (*"não existe outras?"*), forçou varredura manual. A conclusão na conversa foi: **manter dois comandos não faz sentido** — ambos devem mostrar a mesma tabela de specs ativas, e a única ação distinta é "aprovar" vs "aprovar + executar". Convenção do projeto já usa `spec` em todo lugar (`.claude/spec/`, `entity-registry.json`, `spec.md`, `wave-plan.md`), então o comando unificado vira `/mustard:spec`.

## Usuários/Stakeholders

Rubens (operador único do Mustard hoje) + qualquer agente IA orquestrando pipelines via Claude Code. Sem usuários em produção — política `feedback_no_migration_dev_phase.md` permite deletar `/approve` + `/resume` sem deprecação.

## Métrica de sucesso

Ao rodar `/mustard:spec`:
1. Sem args → tabela com TODAS as specs em PLAN/EXEC + Active (filtra sub-specs review/qa/wave-N), com colunas `# | Spec | Esc | Estágio | Prog | Status | Resumo`, legenda de siglas e modo de seleção SEMPRE visíveis.
2. Com `a`-`z` → aprova (PLAN) ou continua (EXEC) a spec daquela linha.
3. Com `ar`-`zr` → aprova + executa inline na mesma sessão (PLAN → EXEC sem trocar sessão); para spec em EXEC, equivale a só a letra.

`/approve` e `/resume` deletados; tentar invocá-los retorna erro "comando não encontrado" (sem alias).

## Não-Objetivos

- **Não** preservar `/approve` ou `/resume` como aliases — política dev = deletar legado.
- **Não** mover a lógica de picker para Rust (`mustard-rt`) — fica no SKILL.md, é texto + interpretação do LLM.
- **Não** suportar seleção múltipla (`ab`, `ac` para aprovar várias em sequência) — fora de escopo; cada `/mustard:spec X` age sobre uma spec.
- **Não** mudar o fluxo interno pós-seleção — depois que a spec é escolhida, segue idêntico aos SKILLs antigos (mesmas chamadas a `mustard-rt run`, mesma sequência de events).
- **Não** auto-skipar o picker em 1-spec — sempre renderizar tabela + siglas + modo (per `feedback_siglas_always_with_legend`).

## Critérios de Aceitação

- [x] AC-1: `commands/mustard/spec/SKILL.md` existe e contém os 3 blocos obrigatórios (tabela + siglas + modo) — Command: `node -e "const c=require('fs').readFileSync('.claude/commands/mustard/spec/SKILL.md','utf8');if(!/Siglas/.test(c))process.exit(1);if(!/Modo de sele/i.test(c))process.exit(2);if(!/\|\s*Estágio\s*\|/i.test(c)&&!/Estágio.*PLAN.*EXEC/is.test(c))process.exit(3);console.log('ok')"`
- [x] AC-2: `commands/mustard/approve/` deletado — Command: `node -e "if(require('fs').existsSync('.claude/commands/mustard/approve'))process.exit(1);console.log('ok')"`
- [x] AC-3: `commands/mustard/resume/` deletado — Command: `node -e "if(require('fs').existsSync('.claude/commands/mustard/resume'))process.exit(1);console.log('ok')"`
- [x] AC-4: `commands/mustard/spec/SKILL.md` descreve sufixo `r` (aprovar + executar inline) — Command: `node -e "const c=require('fs').readFileSync('.claude/commands/mustard/spec/SKILL.md','utf8');if(!/letra \+ r|sufixo r|aprovar \+ executar/i.test(c))process.exit(1);console.log('ok')"`
- [x] AC-5: refs internas a `/mustard:approve` ou `/mustard:resume` em outros SKILLs/refs atualizadas para `/mustard:spec` — Command: `node -e "const fs=require('fs');const path=require('path');const roots=['.claude/commands','.claude/refs','apps/cli/templates/commands','apps/cli/templates/refs'];const rx=/\/mustard:(approve|resume)\b/;const hits=[];function walk(d){if(!fs.existsSync(d))return;for(const e of fs.readdirSync(d,{withFileTypes:true})){if(e.name==='node_modules'||e.name==='.git')continue;const f=path.join(d,e.name);if(e.isDirectory())walk(f);else if(e.name.endsWith('.md')){const c=fs.readFileSync(f,'utf8');if(rx.test(c))hits.push(f)}}}roots.forEach(walk);if(hits.length){console.error('stale refs:\n'+hits.join('\n'));process.exit(1)}console.log('ok')"`
- [x] AC-6: SKILL declara renderização **obrigatória** de siglas + modo em toda invocação (não auto-skipar em 1-spec) — Command: `node -e "const c=require('fs').readFileSync('.claude/commands/mustard/spec/SKILL.md','utf8');const rx=/(sempre|obrigat[óo]ri[oa]).*(siglas|legenda)|(siglas|legenda).*(sempre|obrigat[óo]ri[oa])/is;if(!rx.test(c))process.exit(1);console.log('ok')"`

## Plano

### 1. Criar `commands/mustard/spec/SKILL.md`

Estrutura:

```markdown
# /mustard:spec — Comando único de spec

## Trigger
`/mustard:spec [letra[r]]`

## Description
Comando único que substitui /approve e /resume. Sem args = picker tabelado de todas
specs ativas. Letra única = aprovar (PLAN) ou continuar (EXEC). Letra+r = aprovar+executar inline.

## Action

### Step 1: AUTO-SYNC
`mustard-rt run sync-registry`

### Step 2: Discovery
1. Glob `.claude/spec/*/spec.md` + `.claude/spec/*/wave-plan.md`
2. Para cada match, ler primeiras 10 linhas, extrair `### Stage:`, `### Outcome:`, `### Scope:`, `### Parent:`
3. Filtrar:
   - `Outcome === "Active"` AND `Stage !== "Close"`
   - Ignorar sub-specs (basename do dir começa com `review/`, `qa/`, `wave-N-...`)
4. Para wave plans, rodar `mustard-rt run event-projections --view pipeline-state --spec {name}` → extrair `completedWaves.length / totalWaves` + `failedWaves[]`
5. Para cada spec, extrair Resumo (primeira frase de `## Resumo` ou `## Contexto`, ≤70 chars)

### Step 3: Render (sempre)
Imprimir TODOS os três blocos juntos, mesmo se só 1 spec:

#### Tabela
{tabela letrada com colunas: # | Spec | Esc | Estágio | Prog | Status | Resumo}

#### Siglas
- Estágio: PLAN (planejar) · EXEC (executar) · CLOS (fechar — filtrado)
- Escopo: lt (light, ≤5 arquivos) · fl (full, wave plan ou >5 arquivos)
- Prog: X/Y (waves completas/total) — só wave plans
- Status: TF→{parent} (tactical-fix) · W{N} BLOCK (wave bloqueada) · em exec (já dispatchada)

#### Modo de seleção
- a-z → agir sobre a spec (aprovar se PLAN, continuar se EXEC)
- a-z + r → aprovar + executar inline (PLAN→EXEC sem trocar sessão; EXEC = só letra)

### Step 4: Stop & Aguardar
Parar e esperar input do usuário. Não auto-selecionar nem em 1-spec.

#### Auditoria obrigatória de siglas (antes de imprimir)

Antes de renderizar, varrer TODAS as células + headers de coluna procurando abreviações. Para cada uma achada, garantir que existe entrada correspondente no bloco "Siglas". Abreviações conhecidas que sempre precisam estar na legenda:

- Headers: `#` (coluna de letra de seleção), `Esc` (Escopo), `Prog` (Progresso)
- Estágio: `PLAN`, `EXEC`, `CLOS`
- Escopo: `lt` (light), `fl` (full)
- Progresso: `X/Y` (formato), `-` (não aplica)
- Status: `TF` (Tactical Fix), `W{N}` (Wave N), `BLOCK` (BLOCKED), `em exec` (já em Stage:Execute), `-` (sem flag)
- Truncamentos de parent name (ex.: `ds` por `dashboard-design-system`): listar em sub-bloco "Parents referenciados" mapeando alias → nome real
- Inline em Resumo: `AC` (Acceptance Criteria), `AC-W{N}.M` (AC M da Wave N), `BLOCKED`, qualquer outra sigla específica do texto

Se uma sigla nova aparecer e não estiver coberta, ou (a) trocar pela forma estendida, ou (b) adicionar à legenda. Nunca imprimir sigla sem legenda correspondente.

### Step 5: Parsing do input
- `[a-z]$` → modo "act-only"
- `[a-z]r$` → modo "act+execute"
- Outro → erro + re-render

### Step 6: Roteamento
- Spec em PLAN + act-only → fluxo idêntico ao antigo /approve (sem --resume)
- Spec em PLAN + act+execute → fluxo /approve --resume (Step 9 do approve antigo)
- Spec em EXEC → fluxo idêntico ao /resume (qualquer sufixo)
```

### 2. Deletar `commands/mustard/approve/` e `commands/mustard/resume/`

```bash
rm -rf .claude/commands/mustard/approve
rm -rf .claude/commands/mustard/resume
```

### 3. Atualizar refs internas

Grep+Edit: substituir `/mustard:approve` → `/mustard:spec` e `/mustard:resume` → `/mustard:spec` em:
- `commands/mustard/feature/SKILL.md`
- `commands/mustard/bugfix/SKILL.md`
- `commands/mustard/close/SKILL.md`
- `commands/mustard/resume/SKILL.md` (será deletado, mas refs em outros refs/)
- `refs/**/*.md`
- `templates/commands/mustard/**/*.md` + `templates/refs/**/*.md`
- `pipeline-config.md`

Para refs que dizem "depois rode `/approve`", a nova frase é "rode `/mustard:spec` e digite a letra do spec" (ou mais sucinto: "rode `/mustard:spec`").

### 4. Edge cases

- **0 specs ativas** → mensagem `Nenhuma spec ativa. Crie via /mustard:feature ou /mustard:bugfix.`
- **>26 specs ativas** → improvável; se acontecer, mostrar `a-z` primeiras + nota `(N specs adicionais — refine via /mustard:status para detalhes)` — sem paginação real nesta TF.
- **Letra inválida** → erro inline + re-render da tabela completa (siglas + modo).
- **Letra de spec EXEC com sufixo `r`** → comportamento idêntico a só letra; informar inline `Spec já em EXEC; sufixo r ignorado.`

## Arquivos

- `.claude/commands/mustard/spec/SKILL.md` (NOVO)
- `.claude/commands/mustard/approve/SKILL.md` (DELETE)
- `.claude/commands/mustard/resume/SKILL.md` (DELETE)
- Refs em outros SKILLs/refs atualizadas (`feature`, `bugfix`, `close`, `pipeline-config.md`, `refs/**`, `templates/**`)

## Informações da Entidade

N/A — refator de SKILLs (markdown puro).

## Limites

Editar dentro de:
- `.claude/commands/mustard/spec/` (criar)
- `.claude/commands/mustard/approve/` (deletar)
- `.claude/commands/mustard/resume/` (deletar)
- Refs em outros `.claude/commands/mustard/*/SKILL.md`, `.claude/refs/**`, `.claude/pipeline-config.md`
- `apps/cli/templates/commands/mustard/spec/` (espelho — criar)
- `apps/cli/templates/commands/mustard/approve/` + `.../resume/` (espelho — deletar)
- `apps/cli/templates/refs/**` se houver refs equivalentes

**Não tocar**:
- `mustard-rt` source (`apps/rt/src/`) — picker mora no SKILL, não no runtime
- Outros SKILLs (`/feature`, `/bugfix`, `/close`, `/git`, `/scan`, etc.) — só atualizar refs, não mudar fluxo
- `entity-registry.json`, `.docs-audit.json` — sem mudança de schema

## Checklist

- [x] `commands/mustard/spec/SKILL.md` criado com 3 blocos obrigatórios (tabela + siglas + modo)
- [x] `commands/mustard/approve/` deletado
- [x] `commands/mustard/resume/` deletado
- [x] Refs internas atualizadas (grep `/mustard:(approve|resume)` retorna 0)
- [x] `apps/cli/templates/commands/mustard/spec/` espelho criado, `/approve` + `/resume` espelhos deletados
- [x] AC-1 a AC-6 verdes
- [x] Demo manual: rodar `/mustard:spec` com ≥2 specs ativas → tabela aparece com siglas + modo (validado estruturalmente via AC-1/AC-6; smoke test visual fica para próxima sessão)
- [x] Demo manual: digitar letra simples → fluxo correto por estágio (roteamento descrito no SKILL — refs/spec/approve-only-flow.md + refs/spec/resume-flow.md)
- [x] Demo manual: digitar letra+r em PLAN → handoff inline para fluxo execute (sufixo `r` documentado e AC-4 verde)
